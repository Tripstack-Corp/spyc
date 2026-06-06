//! Minimal MCP (Model Context Protocol) server.
//!
//! Two transports:
//!   - **stdio** (`spyc --mcp`): Spawned by Claude Code. Reads
//!     JSON-RPC from stdin, proxies to the running spyc instance
//!     via a Unix domain socket, and writes responses to stdout.
//!   - **Unix socket** (`start_socket_server`): Spawns a background
//!     thread listening on `~/.local/state/spyc/mcp-<PID>.sock`.
//!     The stdio process connects here; writable actions go through
//!     the command channel to the main event loop.
//!
//! Multiple spyc instances coexist via PID-scoped sockets. The
//! `.mcp.json` carries `SPYC_MCP_SOCK` in its `env` block so the
//! stdio proxy connects to the right instance.
//!
//! Both transports share the same JSON-RPC dispatch logic.

use std::io::{self, BufRead, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};

use crate::context;
use crate::mcp_cmd::{McpCommand, McpRequest, McpResponse};

const SERVER_NAME: &str = "spyc";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
const PROTOCOL_VERSION: &str = "2024-11-05";
const CONTEXT_URI: &str = "spyc://context";

/// Socket IO deadline for the stdio proxy. Bounds how long it waits on a
/// server response (and on a write) so a wedged / silent / panicked server
/// thread surfaces as a clean JSON-RPC error to the agent instead of
/// hanging it indefinitely. Generous — well above the server's own 5 s
/// writable-action timeout — so a legitimately slow reply isn't cut off;
/// only a genuine indefinite stall trips it.
const PROXY_IO_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// Log to /tmp/spyc-mcp.log for debugging MCP connection issues.
fn mcp_log(msg: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/spyc-mcp.log")
    {
        let _ = writeln!(f, "spyc-mcp: {msg}");
    }
}

// ── Shared JSON-RPC dispatch ────────────────────────────────────

/// Socket path for a given PID: `~/.local/state/spyc/mcp-<pid>.sock`.
pub fn socket_path_for(pid: u32) -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let state_dir = PathBuf::from(home).join(".local/state/spyc");
    state_dir.join(format!("mcp-{pid}.sock"))
}

/// Socket path for the current process.
pub fn socket_path() -> PathBuf {
    socket_path_for(std::process::id())
}

/// Dispatch a JSON-RPC request and write the response to `w`.
/// `cmd_tx` is `Some` when running as the socket server
/// (writable actions available), `None` for read-only fallback.
fn dispatch(
    w: &mut impl Write,
    msg: &str,
    ctx_path: &Path,
    cmd_tx: Option<&std::sync::mpsc::Sender<McpRequest>>,
) -> io::Result<()> {
    let parsed: Value = match serde_json::from_str(msg) {
        Ok(v) => v,
        Err(_) => return send_error(w, Value::Null, -32700, "Parse error"),
    };

    // Notifications (no "id") — no response, but some have side effects.
    if parsed.get("id").is_none() {
        let method = parsed["method"].as_str().unwrap_or("");
        if method == "spyc/disconnected"
            && let Some(tx) = cmd_tx
        {
            let new_pid = parsed["params"]["new_pid"].as_u64().unwrap_or(0) as u32;
            let (reply_tx, _) = std::sync::mpsc::channel();
            let _ = tx.send(McpRequest {
                command: McpCommand::Disconnected { new_pid },
                reply: reply_tx,
            });
        }
        return Ok(());
    }

    let id = parsed["id"].clone();
    let method = parsed["method"].as_str().unwrap_or("");

    match method {
        "initialize" => handle_initialize(w, &id, &parsed["params"]),
        "resources/list" => handle_resources_list(w, &id),
        "resources/read" => handle_resources_read(w, &id, &parsed["params"], ctx_path),
        "tools/list" => handle_tools_list(w, &id),
        "tools/call" => handle_tools_call(w, &id, &parsed["params"], ctx_path, cmd_tx),
        "ping" => send_result(w, &id, json!({})),
        _ => send_error(w, id, -32601, &format!("Method not found: {method}")),
    }
}

// ── Stdio transport (spyc --mcp) ────────────────────────────────
//
// Proxies JSON-RPC from stdin/stdout to the running spyc instance's
// Unix domain socket. Falls back to read-only local dispatch if the
// socket isn't available (no running spyc).

/// Resolve context path from env var or project root.
fn resolve_context_path(project_root: &Path) -> PathBuf {
    if let Ok(p) = std::env::var(context::CONTEXT_ENV_VAR)
        && !p.is_empty()
    {
        return PathBuf::from(p);
    }
    context::context_path(project_root)
}

/// Run the stdio MCP server. Reads JSONL from stdin, dispatches
/// locally, writes JSONL to stdout. If the running spyc instance's
/// Unix socket is available, proxies through it for writable access.
///
/// Socket resolution order:
/// 1. `$SPYC_MCP_SOCK` (set in `.mcp.json`'s `env` block) — exact match
/// 2. Project-scoped discovery: walk `caller_cwd` upward looking for
///    `.spyc-context-<pid>.json` markers; map those PIDs to live
///    sockets. Refuses cross-project attachment (a spyc running in
///    a different project tree can no longer be picked up).
/// 3. Falls back to read-only direct mode if nothing matches.
pub fn run(project_root: PathBuf) -> anyhow::Result<()> {
    // Try explicit socket path from env first.
    if let Ok(p) = std::env::var("SPYC_MCP_SOCK")
        && !p.is_empty()
    {
        let sock = PathBuf::from(&p);
        mcp_log(&format!("stdio: trying env socket {}", sock.display()));
        if let Ok(stream) = UnixStream::connect(&sock) {
            mcp_log("stdio: connected via env socket, proxying");
            return run_proxy(stream);
        }
    }

    // Discovery: only consider spyc instances rooted in this project
    // tree (caller's cwd or any ancestor). See `discover_live_socket`.
    if let Some(stream) = discover_live_socket(&project_root) {
        return run_proxy(stream);
    }

    // Direct mode: read-only local dispatch (no writable actions).
    mcp_log("stdio: no live socket found, running direct JSONL");
    run_direct(project_root)
}

/// Project-scoped discovery: walk `caller_cwd` upward looking for any
/// `.spyc-context-<pid>.json` markers (each is written by a running
/// spyc rooted at that directory — see `context::context_path`).
/// The first ancestor with at least one marker is the "project
/// boundary"; only those PIDs become candidates. We never aggregate
/// across levels: a parent-dir spyc shouldn't shadow a child-dir spyc
/// when both exist.
///
/// Why this shape: prior to this fix, discovery scanned every socket
/// in `~/.local/state/spyc/` and returned the first connectable one,
/// happily attaching a claude in project A to a spyc running in
/// project B (or even another user's spyc, depending on `$HOME`
/// scoping). Project-scoped discovery rules that out while keeping
/// the "claude launched outside the pane just works" ergonomic — as
/// long as it's launched somewhere inside the spyc instance's tree.
fn discover_live_socket(caller_cwd: &Path) -> Option<UnixStream> {
    let candidates = collect_project_pids(caller_cwd);
    if candidates.is_empty() {
        mcp_log(&format!(
            "stdio: discover: no .spyc-context-*.json found in {} or ancestors",
            caller_cwd.display(),
        ));
        return None;
    }
    mcp_log(&format!(
        "stdio: discover: {} project-scoped candidate(s) for {}",
        candidates.len(),
        caller_cwd.display(),
    ));
    for pid in candidates {
        let sock = socket_path_for(pid);
        mcp_log(&format!("stdio: discover trying {}", sock.display()));
        match UnixStream::connect(&sock) {
            Ok(stream) => {
                mcp_log(&format!(
                    "stdio: discovered live socket {} (pid {})",
                    sock.display(),
                    pid,
                ));
                return Some(stream);
            }
            Err(e) => {
                // Only delete on "no peer there" errors — connect
                // can also fail under transient resource pressure
                // (EAGAIN, EMFILE) where a live peer's socket
                // would survive the next attempt. Pruning on those
                // would race-delete a healthy peer.
                let stale = matches!(
                    e.kind(),
                    std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound,
                );
                if stale {
                    let _ = std::fs::remove_file(&sock);
                }
                mcp_log(&format!(
                    "stdio: discover skip {}: {} (stale={stale})",
                    sock.display(),
                    e.kind(),
                ));
            }
        }
    }
    None
}

/// Walk `start` toward the filesystem root looking for
/// `.spyc-context-<pid>.json` markers. Returns the PIDs from the
/// first ancestor that has any matches; empty Vec otherwise.
fn collect_project_pids(start: &Path) -> Vec<u32> {
    let mut here: &Path = start;
    loop {
        let pids = read_context_pids_in_dir(here);
        if !pids.is_empty() {
            return pids;
        }
        let Some(parent) = here.parent() else {
            return Vec::new();
        };
        if parent == here {
            return Vec::new();
        }
        here = parent;
    }
}

/// Read `.spyc-context-<pid>.json` filenames in `dir`, returning the
/// PIDs parsed out of them. Order is unspecified (matches `read_dir`),
/// which is fine because the caller tries each candidate in turn.
fn read_context_pids_in_dir(dir: &Path) -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut pids = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let Some(rest) = name_str.strip_prefix(".spyc-context-") else {
            continue;
        };
        let Some(pid_str) = rest.strip_suffix(".json") else {
            continue;
        };
        if let Ok(pid) = pid_str.parse::<u32>() {
            pids.push(pid);
        }
    }
    pids
}

/// Direct JSONL stdio server — no socket proxy.
#[allow(clippy::significant_drop_tightening)]
fn run_direct(project_root: PathBuf) -> anyhow::Result<()> {
    let context_path = resolve_context_path(&project_root);
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = stdin.lock();
    let mut writer = stdout.lock();

    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => {
                mcp_log("direct: stdin closed");
                break;
            }
            Ok(_) => {}
            Err(e) => {
                mcp_log(&format!("direct: stdin read error: {e}"));
                if e.kind() == io::ErrorKind::UnexpectedEof {
                    break;
                }
                return Err(e.into());
            }
        }
        let msg = line.trim();
        if msg.is_empty() {
            continue;
        }
        mcp_log(&format!("direct: recv ({} bytes)", msg.len()));

        // Dispatch writes Content-Length framed output. We need to
        // capture it and re-emit as JSONL.
        let mut buf = Vec::new();
        dispatch(&mut buf, msg, &context_path, None)?;

        // Extract the JSON body from Content-Length framing.
        let framed = String::from_utf8_lossy(&buf);
        if let Some(pos) = framed.find("\r\n\r\n") {
            let json_body = &framed[pos + 4..];
            if !json_body.is_empty() {
                mcp_log(&format!("direct: send ({} bytes)", json_body.len()));
                writeln!(writer, "{json_body}")?;
                writer.flush()?;
            }
        }
    }
    Ok(())
}

/// Proxy stdin/stdout ↔ Unix socket. Messages use Content-Length
/// framing on both sides.
#[allow(clippy::significant_drop_tightening)]
fn run_proxy(stream: UnixStream) -> anyhow::Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut stdin_reader = stdin.lock();
    let mut stdout_writer = stdout.lock();
    let sock_clone = match stream.try_clone() {
        Ok(c) => c,
        Err(e) => {
            mcp_log(&format!("proxy: stream clone failed: {e}"));
            return Err(e.into());
        }
    };
    // Bound socket IO (see `PROXY_IO_TIMEOUT`) so a wedged server can't hang
    // the agent forever — `read_lsp_message` below would otherwise block
    // indefinitely on a silent server thread.
    let _ = sock_clone.set_read_timeout(Some(PROXY_IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(PROXY_IO_TIMEOUT));
    let mut sock_reader = io::BufReader::new(sock_clone);
    let mut sock_writer = stream;
    mcp_log("proxy: ready, waiting for stdin");

    loop {
        // Read a JSON-RPC message from stdin. Claude Code uses newline-
        // delimited JSON (one JSON object per line), not Content-Length
        // framing.
        let mut line = String::new();
        match stdin_reader.read_line(&mut line) {
            Ok(0) => {
                mcp_log("proxy: stdin closed");
                break;
            }
            Ok(_) => {}
            Err(e) => {
                mcp_log(&format!("proxy: stdin read error: {e}"));
                if e.kind() == io::ErrorKind::UnexpectedEof {
                    break;
                }
                return Err(e.into());
            }
        }
        let msg = line.trim();
        if msg.is_empty() {
            continue; // skip blank lines
        }
        mcp_log(&format!(
            "proxy: stdin → socket ({} bytes): {}",
            msg.len(),
            msg
        ));

        // Check if this is a request (has "id") or notification (no "id").
        let is_request = serde_json::from_str::<Value>(msg).map_or(true, |v| v.get("id").is_some()); // assume request if parse fails

        // Forward to socket (Content-Length framed for the socket server).
        send_message(&mut sock_writer, msg)?;

        // Only read a response for requests (notifications get no reply).
        if is_request {
            let response = match read_lsp_message(&mut sock_reader) {
                Ok(r) => r,
                Err(e) => {
                    // Timeout or socket error waiting for the server. Reply to
                    // the agent with a JSON-RPC error (reusing the request id
                    // so the client matches it) so its tool call returns an
                    // error instead of hanging, then end the proxy cleanly —
                    // a late reply would desync the stream framing.
                    mcp_log(&format!("proxy: socket read error/timeout: {e}"));
                    let id = serde_json::from_str::<Value>(msg)
                        .ok()
                        .and_then(|v| v.get("id").cloned())
                        .unwrap_or(Value::Null);
                    let err = json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": {
                            "code": -32000,
                            "message": format!("spyc MCP server did not respond ({e})"),
                        }
                    });
                    let _ = writeln!(stdout_writer, "{err}");
                    let _ = stdout_writer.flush();
                    break;
                }
            };
            mcp_log(&format!(
                "proxy: socket → stdout ({} bytes): {}",
                response.len(),
                response
            ));
            // Write back as newline-delimited JSON (what Claude Code expects).
            writeln!(stdout_writer, "{response}")?;
            stdout_writer.flush()?;
        }
    }
    Ok(())
}

// ── Unix socket server (background thread) ──────────────────────

/// Start the MCP server on a Unix domain socket. The socket path is
/// `~/.local/state/spyc/mcp-<PID>.sock`. The server runs on a
/// background thread and reads context from `ctx_path`. `cmd_tx` is the
/// write end of the command channel — writable actions go through it to
/// the main event loop.
pub fn start_socket_server(
    ctx_path: PathBuf,
    cmd_tx: std::sync::mpsc::Sender<McpRequest>,
) -> anyhow::Result<()> {
    let sock = socket_path();

    // Ensure the parent directory exists.
    if let Some(parent) = sock.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Remove stale socket from a previous run.
    let _ = std::fs::remove_file(&sock);

    // Restrict socket permissions to owner-only (0o700) so other users
    // on a shared machine cannot connect and read files or mutate the TUI.
    let old_umask = rustix::process::umask(rustix::fs::Mode::from_bits_truncate(0o077));
    let bind_result = UnixListener::bind(&sock);
    rustix::process::umask(old_umask);
    let listener = bind_result?;
    let ctx_path = Arc::new(ctx_path);
    let cmd_tx = Arc::new(cmd_tx);

    mcp_log(&format!("socket: listening on {}", sock.display()));

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            mcp_log("socket: accepted connection");
            let ctx = Arc::clone(&ctx_path);
            let tx = Arc::clone(&cmd_tx);
            std::thread::spawn(move || {
                if let Err(e) = handle_socket_connection(stream, &ctx, &tx) {
                    mcp_log(&format!("socket: connection error: {e}"));
                }
            });
        }
    });

    Ok(())
}

/// Clean up the socket file on shutdown.
pub fn cleanup_socket() {
    let sock = socket_path();
    let _ = std::fs::remove_file(&sock);
}

/// Status of MCP configuration for this directory.
pub enum McpConfigStatus {
    /// .mcp.json written/updated to point at our socket.
    Configured,
    /// Took over from another instance (notified it). PID of old instance.
    TookOver { old_pid: u32 },
    /// Detected another live instance and the caller asked us not to
    /// take over — `.mcp.json` left pointing at the old PID.
    SkippedTakeover { old_pid: u32 },
    /// Enterprise managed-settings.json blocks spyc.
    BlockedByEnterprise,
    /// Enterprise managed-mcp.json already defines spyc — Claude
    /// resolves through the org config; we run the socket server but
    /// skip writing local `.mcp.json` (and clean up any prior write).
    ManagedByEnterprise,
}

/// Detect a live spyc instance currently owning MCP for `dir` without
/// modifying `.mcp.json`. Returns the old instance's PID if its socket
/// is reachable, else None. Used by the startup takeover prompt so we
/// can ask the user before clobbering another instance's registration.
pub fn detect_existing_spyc(dir: &Path) -> Option<u32> {
    let our_sock = socket_path();
    let path = dir.join(".mcp.json");
    let text = std::fs::read_to_string(&path).ok()?;
    let parsed: Value = serde_json::from_str(&text).ok()?;
    let old_sock_str = parsed
        .pointer("/mcpServers/spyc/env/SPYC_MCP_SOCK")
        .and_then(|v| v.as_str())?;
    let old_sock = PathBuf::from(old_sock_str);
    if old_sock == our_sock {
        return None;
    }
    UnixStream::connect(&old_sock).ok()?;
    pid_from_sock_path(old_sock_str)
}

/// Well-known paths for Claude Code enterprise managed settings.
const MANAGED_SETTINGS_PATHS: &[&str] = &[
    // macOS system-wide
    "/Library/Application Support/ClaudeCode/managed-settings.json",
    // Linux / WSL system-wide
    "/etc/claude-code/managed-settings.json",
];

/// Well-known paths for Claude Code enterprise-deployed MCP definitions.
/// When this file exists and defines a server named "spyc", the org has
/// already wired Claude → spyc and our per-project `.mcp.json` writes
/// are redundant (and just collide on the server name).
const MANAGED_MCP_PATHS: &[&str] = &[
    "/Library/Application Support/ClaudeCode/managed-mcp.json",
    "/etc/claude-code/managed-mcp.json",
];

/// Check whether enterprise managed-settings.json blocks "spyc".
/// Checks `deniedMcpServers` (by serverName) and `allowedMcpServers`.
/// Returns `None` if no enterprise config exists or if there's no
/// restriction. Returns `Some(false)` if spyc is denied or not in
/// an allowlist.
fn enterprise_allows_spyc() -> Option<bool> {
    for path in MANAGED_SETTINGS_PATHS {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let Ok(parsed) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        // Denylist takes absolute precedence.
        if let Some(denied) = parsed.get("deniedMcpServers")
            && let Some(arr) = denied.as_array()
            && arr
                .iter()
                .any(|entry| entry["serverName"].as_str() == Some("spyc"))
        {
            return Some(false);
        }
        // Allowlist: if present, spyc must be in it.
        if let Some(allowed) = parsed.get("allowedMcpServers")
            && let Some(arr) = allowed.as_array()
        {
            let ok = arr
                .iter()
                .any(|entry| entry["serverName"].as_str() == Some("spyc"));
            return Some(ok);
        }
        return None; // Enterprise config exists but no MCP restrictions.
    }
    None // No enterprise config found.
}

/// True when an enterprise-deployed `managed-mcp.json` defines a
/// server named "spyc". In that case Claude already knows how to
/// reach us and we should not also write per-project `.mcp.json`
/// files (a name collision results in Claude picking the org
/// definition, with the per-project entry only adding noise).
pub fn enterprise_defines_spyc() -> bool {
    for path in MANAGED_MCP_PATHS {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let Ok(parsed) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        if parsed.pointer("/mcpServers/spyc").is_some() {
            return true;
        }
    }
    false
}

/// Extract the PID from a socket path like `mcp-12345.sock`.
fn pid_from_sock_path(path: &str) -> Option<u32> {
    let fname = Path::new(path).file_name()?.to_str()?;
    let stripped = fname.strip_prefix("mcp-")?.strip_suffix(".sock")?;
    stripped.parse().ok()
}

/// Try to send a `spyc/disconnected` notification to the old instance's
/// socket. Best-effort — if it fails, we proceed with takeover anyway.
fn notify_disconnect(old_sock: &Path, new_pid: u32) {
    let Ok(mut stream) = UnixStream::connect(old_sock) else {
        return;
    };
    let notification = json!({
        "jsonrpc": "2.0",
        "method": "spyc/disconnected",
        "params": { "new_pid": new_pid }
    });
    let _ = send_message(&mut stream, &notification.to_string());
    mcp_log(&format!("sent spyc/disconnected to {}", old_sock.display()));
}

/// Ensure `.mcp.json` has the spyc entry using stdio transport.
/// Checks enterprise policy first. If another spyc instance owns
/// the entry, sends it a disconnect notification and takes over.
pub fn ensure_mcp_json(dir: &Path, takeover_allowed: bool) -> Result<McpConfigStatus, io::Error> {
    if enterprise_allows_spyc() == Some(false) {
        return Ok(McpConfigStatus::BlockedByEnterprise);
    }

    if enterprise_defines_spyc() {
        // Org config (managed-mcp.json) is the source of truth for the
        // "spyc" server identifier — anything we write to .mcp.json just
        // collides with it. Remove any prior spyc entry we (or an older
        // spyc) wrote, preserving any other servers the user has defined.
        // If the file only contained spyc, remove it entirely.
        clean_local_mcp_entry(dir);
        return Ok(McpConfigStatus::ManagedByEnterprise);
    }

    let our_sock = socket_path();
    let our_pid = std::process::id();
    let path = dir.join(".mcp.json");
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("spyc"));

    // Check for an existing live spyc instance in this directory.
    let mut took_over: Option<u32> = None;
    if let Ok(text) = std::fs::read_to_string(&path)
        && let Ok(parsed) = serde_json::from_str::<Value>(&text)
        && let Some(old_sock_str) = parsed
            .pointer("/mcpServers/spyc/env/SPYC_MCP_SOCK")
            .and_then(|v| v.as_str())
    {
        let old_sock = PathBuf::from(old_sock_str);
        if old_sock != our_sock {
            // Another instance — check if it's still alive.
            if UnixStream::connect(&old_sock).is_ok() {
                let old_pid = pid_from_sock_path(old_sock_str).unwrap_or(0);
                if !takeover_allowed {
                    mcp_log(&format!(
                        "skipped takeover from PID {old_pid} ({})",
                        old_sock.display()
                    ));
                    return Ok(McpConfigStatus::SkippedTakeover { old_pid });
                }
                notify_disconnect(&old_sock, our_pid);
                took_over = Some(old_pid);
                mcp_log(&format!(
                    "taking over from PID {old_pid} ({})",
                    old_sock.display()
                ));
            }
        }
    }

    let spyc_entry = json!({
        "command": exe.to_string_lossy(),
        "args": ["--mcp"],
        "env": {
            "SPYC_MCP_SOCK": our_sock.to_string_lossy()
        }
    });

    // Default content when there's no existing file or we can't safely
    // splice into it (parse error, top-level not an object, mcpServers
    // present but not an object). In all those cases we overwrite with
    // a clean shape rather than panicking on `.as_object_mut().unwrap()`.
    let fresh =
        || serde_json::to_string_pretty(&json!({ "mcpServers": { "spyc": spyc_entry } })).unwrap();
    let content = match std::fs::read_to_string(&path) {
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(mut parsed) => {
                let top = parsed.as_object_mut();
                let servers = top.and_then(|t| {
                    let entry = t.entry("mcpServers").or_insert_with(|| json!({}));
                    entry.as_object_mut()
                });
                match servers {
                    Some(map) => {
                        map.insert("spyc".to_string(), spyc_entry);
                        serde_json::to_string_pretty(&parsed)
                            .expect("serializing a serde_json::Value cannot fail")
                    }
                    None => fresh(),
                }
            }
            Err(_) => fresh(),
        },
        Err(_) => fresh(),
    };

    std::fs::write(&path, content + "\n")?;
    mcp_log(&format!(
        "wrote .mcp.json (sock={}, exe={})",
        our_sock.display(),
        exe.display()
    ));

    match took_over {
        Some(old_pid) => Ok(McpConfigStatus::TookOver { old_pid }),
        None => Ok(McpConfigStatus::Configured),
    }
}

/// Codex's equivalent of `ensure_mcp_json`. Writes a stdio MCP entry
/// for spyc into `<dir>/.codex/config.toml` so the codex CLI discovers
/// us automatically, the same way claude does via `.mcp.json`. Codex
/// reads both `~/.codex/config.toml` (user-scope) and
/// `<cwd>/.codex/config.toml` (project-scope); we only ever write the
/// project file to mirror claude's project-scoped behavior and avoid
/// touching the user's main config.
///
/// Codex's TOML schema is `[mcp_servers.<name>]` with `command`,
/// `args`, and `env` keys for stdio servers (parallel to
/// claude's `.mcp.json` shape):
///
/// ```toml
/// [mcp_servers.spyc]
/// command = "spyc"
/// args = ["--mcp"]
///
/// [mcp_servers.spyc.env]
/// SPYC_MCP_SOCK = "/Users/x/.local/state/spyc/mcp-12345.sock"
/// ```
///
/// Takeover semantics match `ensure_mcp_json`: an existing live spyc
/// socket in another PID gets a `spyc/disconnected` notification and
/// we replace the entry. Enterprise policies are claude-specific and
/// don't apply here.
pub fn ensure_codex_config_toml(
    dir: &Path,
    takeover_allowed: bool,
) -> Result<McpConfigStatus, io::Error> {
    let our_sock = socket_path();
    let our_pid = std::process::id();
    let path = dir.join(".codex").join("config.toml");
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("spyc"));

    // Takeover detection: existing entry pointing at a different
    // live socket means another spyc instance owns this directory.
    let mut took_over: Option<u32> = None;
    if let Ok(text) = std::fs::read_to_string(&path)
        && let Ok(parsed) = toml::from_str::<toml::Value>(&text)
        && let Some(old_sock_str) = parsed
            .get("mcp_servers")
            .and_then(|m| m.get("spyc"))
            .and_then(|s| s.get("env"))
            .and_then(|e| e.get("SPYC_MCP_SOCK"))
            .and_then(toml::Value::as_str)
    {
        let old_sock = PathBuf::from(old_sock_str);
        if old_sock != our_sock && UnixStream::connect(&old_sock).is_ok() {
            let old_pid = pid_from_sock_path(old_sock_str).unwrap_or(0);
            if !takeover_allowed {
                mcp_log(&format!(
                    "codex: skipped takeover from PID {old_pid} ({})",
                    old_sock.display()
                ));
                return Ok(McpConfigStatus::SkippedTakeover { old_pid });
            }
            notify_disconnect(&old_sock, our_pid);
            took_over = Some(old_pid);
            mcp_log(&format!(
                "codex: taking over from PID {old_pid} ({})",
                old_sock.display()
            ));
        }
    }

    // Build a fresh `[mcp_servers.spyc]` table — used both as the
    // splice target and as the whole-file fallback when the existing
    // file is malformed or has the wrong shape (top-level not a
    // table, mcp_servers not a table, etc.).
    let build_entry = || {
        let mut env_table = toml::Table::new();
        env_table.insert(
            "SPYC_MCP_SOCK".into(),
            toml::Value::String(our_sock.to_string_lossy().into_owned()),
        );
        let mut entry = toml::Table::new();
        entry.insert(
            "command".into(),
            toml::Value::String(exe.to_string_lossy().into_owned()),
        );
        entry.insert(
            "args".into(),
            toml::Value::Array(vec![toml::Value::String("--mcp".into())]),
        );
        entry.insert("env".into(), toml::Value::Table(env_table));
        entry
    };
    let fresh = || {
        let mut servers = toml::Table::new();
        servers.insert("spyc".into(), toml::Value::Table(build_entry()));
        let mut root = toml::Table::new();
        root.insert("mcp_servers".into(), toml::Value::Table(servers));
        toml::to_string_pretty(&toml::Value::Table(root)).unwrap_or_default()
    };

    let content = match std::fs::read_to_string(&path) {
        Ok(text) => match toml::from_str::<toml::Value>(&text) {
            Ok(mut parsed) => {
                let top = parsed.as_table_mut();
                let servers_ok = top.and_then(|t| {
                    let entry = t
                        .entry("mcp_servers")
                        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
                    entry.as_table_mut()
                });
                match servers_ok {
                    Some(map) => {
                        map.insert("spyc".to_string(), toml::Value::Table(build_entry()));
                        toml::to_string_pretty(&parsed).unwrap_or_else(|_| fresh())
                    }
                    None => fresh(),
                }
            }
            Err(_) => fresh(),
        },
        Err(_) => fresh(),
    };

    // Create the `.codex/` parent directory if missing.
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, content)?;
    mcp_log(&format!(
        "wrote .codex/config.toml (sock={}, exe={})",
        our_sock.display(),
        exe.display()
    ));

    match took_over {
        Some(old_pid) => Ok(McpConfigStatus::TookOver { old_pid }),
        None => Ok(McpConfigStatus::Configured),
    }
}

/// Detect a live spyc instance currently owning codex MCP for `dir`
/// without modifying `.codex/config.toml`. Mirrors
/// `detect_existing_spyc` for the codex side; used by startup so a
/// single takeover prompt covers both claude and codex.
pub fn detect_existing_spyc_codex(dir: &Path) -> Option<u32> {
    let our_sock = socket_path();
    let path = dir.join(".codex").join("config.toml");
    let text = std::fs::read_to_string(&path).ok()?;
    let parsed: toml::Value = toml::from_str(&text).ok()?;
    let old_sock_str = parsed
        .get("mcp_servers")?
        .get("spyc")?
        .get("env")?
        .get("SPYC_MCP_SOCK")?
        .as_str()?;
    let old_sock = PathBuf::from(old_sock_str);
    if old_sock == our_sock {
        return None;
    }
    UnixStream::connect(&old_sock).ok()?;
    pid_from_sock_path(old_sock_str)
}

/// Remove just the "spyc" entry from `<dir>/.mcp.json` if present,
/// preserving any other servers the user (or another tool) may have
/// added. If after removal `mcpServers` is empty *and* no other
/// top-level keys remain, delete the file. All errors are best-effort
/// — this is cleanup, not load-bearing.
fn clean_local_mcp_entry(dir: &Path) {
    let path = dir.join(".mcp.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return;
    };
    let Ok(mut parsed) = serde_json::from_str::<Value>(&text) else {
        return;
    };
    let Some(root) = parsed.as_object_mut() else {
        return;
    };
    let Some(servers) = root.get_mut("mcpServers").and_then(Value::as_object_mut) else {
        return;
    };
    if servers.remove("spyc").is_none() {
        return;
    }
    let servers_empty = servers.is_empty();
    let only_servers = root.len() == 1; // i.e. just `mcpServers`
    if only_servers && servers_empty {
        let _ = std::fs::remove_file(&path);
        mcp_log(&format!(
            "removed empty .mcp.json after cleaning spyc entry ({})",
            path.display()
        ));
        return;
    }
    if let Ok(out) = serde_json::to_string_pretty(&parsed) {
        let _ = std::fs::write(&path, out + "\n");
        mcp_log(&format!(
            "cleaned spyc entry from .mcp.json (preserved other servers, {})",
            path.display()
        ));
    }
}

/// Handle a single Unix socket connection. Uses the same Content-Length
/// framing as the stdio transport.
fn handle_socket_connection(
    stream: UnixStream,
    ctx_path: &Path,
    cmd_tx: &std::sync::mpsc::Sender<McpRequest>,
) -> io::Result<()> {
    let mut reader = io::BufReader::new(stream.try_clone()?);
    // Bound writes so a stalled client (proxy) can't wedge this server thread
    // indefinitely. The read is intentionally left blocking — the loop waits
    // for the next request, which is idle for minutes between agent calls.
    let _ = stream.set_write_timeout(Some(PROXY_IO_TIMEOUT));
    let mut writer = stream;

    loop {
        let msg = match read_lsp_message(&mut reader) {
            Ok(msg) => msg,
            Err(e) => {
                if e.kind() == io::ErrorKind::UnexpectedEof {
                    break; // Connection closed.
                }
                return Err(e);
            }
        };
        dispatch(&mut writer, &msg, ctx_path, Some(cmd_tx))?;
    }
    Ok(())
}

// ── Protocol handlers ────────────────────────────────────────────

fn handle_initialize(w: &mut impl Write, id: &Value, _params: &Value) -> io::Result<()> {
    send_result(
        w,
        id,
        json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {
                "resources": {},
                "tools": {}
            },
            "serverInfo": {
                "name": SERVER_NAME,
                "version": SERVER_VERSION
            }
        }),
    )
}

fn handle_resources_list(w: &mut impl Write, id: &Value) -> io::Result<()> {
    send_result(
        w,
        id,
        json!({
            "resources": [
                {
                    "uri": CONTEXT_URI,
                    "name": "spyc context",
                    "description": "Current spyc state: working directory, cursor position, picks, inventory, filter, git branch, project home, session name.",
                    "mimeType": "application/json"
                }
            ]
        }),
    )
}

fn handle_resources_read(
    w: &mut impl Write,
    id: &Value,
    params: &Value,
    ctx_path: &Path,
) -> io::Result<()> {
    let uri = params["uri"].as_str().unwrap_or("");
    if uri != CONTEXT_URI {
        return send_error(w, id.clone(), -32602, &format!("Unknown resource: {uri}"));
    }

    let text = read_context_or_empty(ctx_path);
    send_result(
        w,
        id,
        json!({
            "contents": [
                {
                    "uri": CONTEXT_URI,
                    "mimeType": "application/json",
                    "text": text
                }
            ]
        }),
    )
}

fn handle_tools_list(w: &mut impl Write, id: &Value) -> io::Result<()> {
    send_result(
        w,
        id,
        json!({
            "tools": [
                {
                    "name": "get_spyc_context",
                    "description": "Get the current spyc file manager state: working directory, cursor position, picked files, inventory, active filter, git branch, project_home (sticky project root), and session_name. Use this to understand what the user is looking at in their file manager.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {},
                        "required": []
                    }
                },
                {
                    "name": "navigate_to",
                    "description": "Navigate spyc to a directory or file. If the path is a directory, changes to it. If a file, navigates to its parent directory and places the cursor on it.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "Absolute or relative path. Relative paths resolved against spyc's cwd. Supports ~ and $VAR expansion."
                            }
                        },
                        "required": ["path"]
                    }
                },
                {
                    "name": "set_filter",
                    "description": "Set or clear the file listing filter. When set, only files matching the glob pattern are shown. Pass null or empty string to clear.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "pattern": {
                                "type": ["string", "null"],
                                "description": "Glob pattern (e.g. '*.rs', 'test_*'), or null/empty to clear the filter."
                            }
                        }
                    }
                },
                {
                    "name": "pick_files",
                    "description": "Select (pick) files in the current directory matching glob patterns. Picks are additive. Use clear_picks first for a clean selection.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "patterns": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Glob patterns to match against filenames (e.g. ['*.rs', 'Cargo.*'])."
                            }
                        },
                        "required": ["patterns"]
                    }
                },
                {
                    "name": "clear_picks",
                    "description": "Clear all picked (selected) files in spyc.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
                    }
                },
                {
                    "name": "get_file_content",
                    "description": "Read the text contents of a file (up to 100KB). Binary files are rejected. Relative paths resolved against spyc's cwd.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "Absolute or relative path to the file."
                            }
                        },
                        "required": ["path"]
                    }
                },
                {
                    "name": "search_paths",
                    "description": "Project-wide fuzzy filename search. Walks PROJECT_HOME (or cwd if no project root) honoring .gitignore, scores candidates against the query with fzf-style ranking (basename hits beat parent-dir hits). Returns a JSON array of repo-relative paths, best match first. Empty query returns paths in walk order, truncated.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "query": {
                                "type": "string",
                                "description": "Fuzzy-match query. Empty string returns natural walk order."
                            },
                            "limit": {
                                "type": "integer",
                                "description": "Maximum results to return. Default 100, max 1000.",
                                "minimum": 1
                            }
                        },
                        "required": ["query"]
                    }
                },
                {
                    "name": "search_content",
                    "description": "Project-wide content search using ripgrep's matcher (gitignore-aware, smart-case, binary files skipped). Walks PROJECT_HOME (or cwd). Returns a JSON array of {path, line, col, text} match objects.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "pattern": {
                                "type": "string",
                                "description": "Regex pattern. Smart-case: lowercase pattern matches case-insensitively, mixed-case is sensitive."
                            },
                            "limit": {
                                "type": "integer",
                                "description": "Maximum matches to return. Default 200, max 5000.",
                                "minimum": 1
                            }
                        },
                        "required": ["pattern"]
                    }
                },
                {
                    "name": "search_picks",
                    "description": "Search content within ONLY the user's currently-picked files (multi-select state). Picks are spyc UI state Claude can't see directly, so this is the only way to grep the user's intended subset. Returns a JSON array of {path, line, col, text} match objects.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "pattern": {
                                "type": "string",
                                "description": "Regex pattern. Smart-case applied."
                            },
                            "limit": {
                                "type": "integer",
                                "description": "Maximum matches. Default 200, max 5000.",
                                "minimum": 1
                            }
                        },
                        "required": ["pattern"]
                    }
                },
                {
                    "name": "search_inventory",
                    "description": "Search content within the user's persistent inventory cache (yanked-into-cache files that survive across sessions). Like search_picks but spans sessions, so it's the way to grep accumulated 'interesting files'. Returns a JSON array of {path, line, col, text} match objects.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "pattern": {
                                "type": "string",
                                "description": "Regex pattern. Smart-case applied."
                            },
                            "limit": {
                                "type": "integer",
                                "description": "Maximum matches. Default 200, max 5000.",
                                "minimum": 1
                            }
                        },
                        "required": ["pattern"]
                    }
                }
            ]
        }),
    )
}

fn handle_tools_call(
    w: &mut impl Write,
    id: &Value,
    params: &Value,
    ctx_path: &Path,
    cmd_tx: Option<&std::sync::mpsc::Sender<McpRequest>>,
) -> io::Result<()> {
    let name = params["name"].as_str().unwrap_or("");
    let args = &params["arguments"];

    match name {
        "get_spyc_context" => {
            let text = read_context_or_empty(ctx_path);
            send_tool_result(w, id, &text)
        }
        "get_file_content" => {
            // Read-only — handled inline, no command channel needed.
            let path_str = args["path"].as_str().unwrap_or("");
            if path_str.is_empty() {
                return send_tool_error(w, id, "missing required parameter: path");
            }
            // Resolve relative paths against the cwd from the context file.
            let cwd = read_cwd_from_context(ctx_path);
            let resolved = if Path::new(path_str).is_absolute() {
                PathBuf::from(path_str)
            } else {
                cwd.join(path_str)
            };
            // Canonicalize to resolve symlinks and ".." components, then
            // verify the path is under the working directory to prevent
            // directory traversal attacks.
            let canonical = match std::fs::canonicalize(&resolved) {
                Ok(p) => p,
                Err(e) => return send_tool_error(w, id, &format!("{}: {e}", resolved.display())),
            };
            let canonical_cwd = match std::fs::canonicalize(&cwd) {
                Ok(p) => p,
                Err(e) => return send_tool_error(w, id, &format!("cwd: {e}")),
            };
            if !canonical.starts_with(&canonical_cwd) {
                return send_tool_error(w, id, "path is outside the working directory");
            }
            match read_file_content(&canonical) {
                Ok(content) => send_tool_result(w, id, &content),
                Err(e) => send_tool_error(w, id, &e),
            }
        }
        "search_paths" => {
            let query = args["query"].as_str().unwrap_or("").to_string();
            let limit = args["limit"].as_u64().map_or(100, |n| n.min(1000) as usize);
            let root = search_root(ctx_path);
            let paths = crate::fs::finder::find_paths(&root, &query, limit);
            let arr: Vec<Value> = paths
                .iter()
                .map(|p| Value::String(p.to_string_lossy().into_owned()))
                .collect();
            send_tool_result(w, id, &Value::Array(arr).to_string())
        }
        "search_content" => {
            let pattern = args["pattern"].as_str().unwrap_or("");
            if pattern.is_empty() {
                return send_tool_error(w, id, "missing required parameter: pattern");
            }
            let limit = args["limit"].as_u64().map_or(200, |n| n.min(5000) as usize);
            let root = search_root(ctx_path);
            match crate::fs::grep::search_to_vec(&root, pattern, limit) {
                Ok(hits) => send_tool_result(w, id, &grep_matches_to_json(&hits).to_string()),
                Err(e) => send_tool_error(w, id, &e),
            }
        }
        "search_picks" => {
            let pattern = args["pattern"].as_str().unwrap_or("");
            if pattern.is_empty() {
                return send_tool_error(w, id, "missing required parameter: pattern");
            }
            let limit = args["limit"].as_u64().map_or(200, |n| n.min(5000) as usize);
            let (files, root) = read_picks_from_context(ctx_path);
            match crate::fs::grep::search_files(&files, pattern, root.as_deref(), limit) {
                Ok(hits) => send_tool_result(w, id, &grep_matches_to_json(&hits).to_string()),
                Err(e) => send_tool_error(w, id, &e),
            }
        }
        "search_inventory" => {
            let pattern = args["pattern"].as_str().unwrap_or("");
            if pattern.is_empty() {
                return send_tool_error(w, id, "missing required parameter: pattern");
            }
            let limit = args["limit"].as_u64().map_or(200, |n| n.min(5000) as usize);
            let files = read_inventory_from_context(ctx_path);
            // Inventory paths are absolute (cache files); display
            // root is None so we report absolute paths to Claude.
            match crate::fs::grep::search_files(&files, pattern, None, limit) {
                Ok(hits) => send_tool_result(w, id, &grep_matches_to_json(&hits).to_string()),
                Err(e) => send_tool_error(w, id, &e),
            }
        }
        "navigate_to" | "set_filter" | "pick_files" | "clear_picks" => {
            let Some(tx) = cmd_tx else {
                return send_tool_error(w, id, "writable actions not available in stdio mode");
            };
            let command = match name {
                "navigate_to" => {
                    let path = args["path"].as_str().unwrap_or("").to_string();
                    if path.is_empty() {
                        return send_tool_error(w, id, "missing required parameter: path");
                    }
                    McpCommand::NavigateTo { path }
                }
                "set_filter" => {
                    let pattern = args["pattern"].as_str().map(String::from);
                    McpCommand::SetFilter { pattern }
                }
                "pick_files" => {
                    let patterns: Vec<String> = args["patterns"]
                        .as_array()
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default();
                    if patterns.is_empty() {
                        return send_tool_error(w, id, "missing required parameter: patterns");
                    }
                    McpCommand::PickFiles { patterns }
                }
                "clear_picks" => McpCommand::ClearPicks,
                _ => unreachable!(),
            };

            // Send command and block for reply with timeout.
            let (reply_tx, reply_rx) = std::sync::mpsc::channel();
            if tx
                .send(McpRequest {
                    command,
                    reply: reply_tx,
                })
                .is_err()
            {
                return send_tool_error(w, id, "spyc is not running");
            }
            match reply_rx.recv_timeout(std::time::Duration::from_secs(5)) {
                Ok(McpResponse::Ok { message }) => send_tool_result(w, id, &message),
                Ok(McpResponse::Error { message }) => send_tool_error(w, id, &message),
                Err(_) => send_tool_error(w, id, "spyc did not respond within 5 seconds"),
            }
        }
        _ => send_tool_error(w, id, &format!("unknown tool: {name}")),
    }
}

/// Helper: send a successful tool result.
fn send_tool_result(w: &mut impl Write, id: &Value, text: &str) -> io::Result<()> {
    send_result(w, id, json!({"content": [{"type": "text", "text": text}]}))
}

/// Helper: send a tool error.
fn send_tool_error(w: &mut impl Write, id: &Value, text: &str) -> io::Result<()> {
    send_result(
        w,
        id,
        json!({"isError": true, "content": [{"type": "text", "text": text}]}),
    )
}

/// Pick the search root: prefer `project_home` from the context
/// file (the spyc-blessed project root), fall back to `cwd`.
/// Used by `search_paths` and `search_content` so the MCP tools
/// scope themselves the same way the in-TUI `F` and `:grep`
/// commands do.
fn search_root(ctx_path: &Path) -> PathBuf {
    if let Ok(text) = std::fs::read_to_string(ctx_path)
        && let Ok(v) = serde_json::from_str::<Value>(&text)
    {
        if let Some(home) = v["project_home"].as_str()
            && !home.is_empty()
        {
            return PathBuf::from(home);
        }
        if let Some(cwd) = v["cwd"].as_str() {
            return PathBuf::from(cwd);
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"))
}

/// Read picks from the context file as absolute paths (resolved
/// against `cwd`). Returns the picks plus the cwd to use as the
/// display-relative root for match formatting. Picks list may be
/// empty (no picks selected); that's a valid state -- search_picks
/// just returns no matches.
fn read_picks_from_context(ctx_path: &Path) -> (Vec<PathBuf>, Option<PathBuf>) {
    let Ok(text) = std::fs::read_to_string(ctx_path) else {
        return (Vec::new(), None);
    };
    let Ok(v) = serde_json::from_str::<Value>(&text) else {
        return (Vec::new(), None);
    };
    let cwd = v["cwd"].as_str().map(PathBuf::from);
    let picks = v["picks"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|p| p.as_str())
                .map(|s| {
                    let p = Path::new(s);
                    if p.is_absolute() {
                        p.to_path_buf()
                    } else if let Some(c) = &cwd {
                        c.join(p)
                    } else {
                        p.to_path_buf()
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    (picks, cwd)
}

/// Read inventory paths from the context file. Inventory entries
/// are stored as absolute paths in the persistent state, so no
/// resolution against cwd is needed.
fn read_inventory_from_context(ctx_path: &Path) -> Vec<PathBuf> {
    let Ok(text) = std::fs::read_to_string(ctx_path) else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    v["inventory"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|p| p.as_str())
                .map(PathBuf::from)
                .collect()
        })
        .unwrap_or_default()
}

/// Render a slice of grep matches as a JSON array of objects with
/// `{path, line, col, text}` shape. Used by all three content-search
/// tools so the response shape is uniform.
fn grep_matches_to_json(hits: &[crate::fs::grep::GrepMatch]) -> Value {
    let arr: Vec<Value> = hits
        .iter()
        .map(|m| {
            json!({
                "path": m.path.to_string_lossy(),
                "line": m.line,
                "col": m.col,
                "text": m.text,
            })
        })
        .collect();
    Value::Array(arr)
}

/// Read the cwd from the context file (for resolving relative paths).
fn read_cwd_from_context(ctx_path: &Path) -> PathBuf {
    if let Ok(text) = std::fs::read_to_string(ctx_path)
        && let Ok(v) = serde_json::from_str::<Value>(&text)
        && let Some(cwd) = v["cwd"].as_str()
    {
        return PathBuf::from(cwd);
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"))
}

/// Read file content (up to 100KB, text only).
fn read_file_content(path: &Path) -> Result<String, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !meta.is_file() {
        return Err(format!("{}: not a regular file", path.display()));
    }
    if meta.len() > 100 * 1024 {
        return Err(format!(
            "{}: file too large ({} KB, limit 100 KB)",
            path.display(),
            meta.len() / 1024
        ));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    // Reject binary files (null bytes in first 8KB).
    let check_len = bytes.len().min(8192);
    if bytes[..check_len].contains(&0) {
        return Err(format!("{}: binary file", path.display()));
    }
    String::from_utf8(bytes).map_err(|_| format!("{}: not valid UTF-8", path.display()))
}

fn read_context_or_empty(ctx_path: &Path) -> String {
    std::fs::read_to_string(ctx_path).unwrap_or_else(|_| {
        json!({
            "cwd": null,
            "cursor_file": null,
            "picks": [],
            "inventory": [],
            "filter": null,
            "git_branch": null,
            "project_home": null,
            "session_name": ""
        })
        .to_string()
    })
}

// ── Framing helpers ─────────────────────────────────────────────

fn read_lsp_message(reader: &mut impl BufRead) -> io::Result<String> {
    let mut content_length: Option<usize> = None;
    let mut header = String::new();
    loop {
        header.clear();
        let n = reader.read_line(&mut header)?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "stdin closed"));
        }
        let trimmed = header.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some(val) = trimmed.strip_prefix("Content-Length:") {
            content_length = val.trim().parse().ok();
        }
    }

    let len = content_length
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing Content-Length"))?;

    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf)?;
    String::from_utf8(buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

fn send_message(w: &mut impl Write, body: &str) -> io::Result<()> {
    write!(w, "Content-Length: {}\r\n\r\n{}", body.len(), body)?;
    w.flush()
}

fn send_result(w: &mut impl Write, id: &Value, result: Value) -> io::Result<()> {
    let msg = json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    });
    send_message(w, &msg.to_string())
}

fn send_error(w: &mut impl Write, id: Value, code: i32, message: &str) -> io::Result<()> {
    let msg = json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message
        }
    });
    send_message(w, &msg.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn make_request(id: u64, method: &str, params: Value) -> String {
        let body = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        })
        .to_string();
        format!("Content-Length: {}\r\n\r\n{}", body.len(), body)
    }

    fn parse_response(raw: &[u8]) -> Value {
        let s = std::str::from_utf8(raw).unwrap();
        let body_start = s.find("\r\n\r\n").unwrap() + 4;
        serde_json::from_str(&s[body_start..]).unwrap()
    }

    #[test]
    fn initialize_response() {
        let input = make_request(1, "initialize", json!({}));
        let mut reader = Cursor::new(input.as_bytes().to_vec());
        let msg = read_lsp_message(&mut reader).unwrap();

        let mut output = Vec::new();
        dispatch(&mut output, &msg, Path::new("/tmp"), None).unwrap();
        let resp = parse_response(&output);
        assert_eq!(resp["result"]["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(resp["result"]["serverInfo"]["name"], "spyc");
    }

    #[test]
    fn resources_list_response() {
        let mut output = Vec::new();
        dispatch(
            &mut output,
            &json!({"jsonrpc":"2.0","id":2,"method":"resources/list"}).to_string(),
            Path::new("/tmp"),
            None,
        )
        .unwrap();
        let resp = parse_response(&output);
        let resources = resp["result"]["resources"].as_array().unwrap();
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0]["uri"], CONTEXT_URI);
    }

    #[test]
    fn resources_read_with_context_file() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = context::SpycContext {
            cwd: PathBuf::from("/home/user/project"),
            cursor_file: Some("main.rs".into()),
            picks: vec![],
            inventory: vec![],
            filter: None,
            git_branch: Some("develop".into()),
            project_home: None,
            session_name: String::new(),
        };
        let ctx_path = context::context_path(tmp.path());
        context::write_context_file(&ctx_path, &ctx).unwrap();

        let mut output = Vec::new();
        dispatch(
            &mut output,
            &json!({"jsonrpc":"2.0","id":3,"method":"resources/read","params":{"uri":CONTEXT_URI}})
                .to_string(),
            &ctx_path,
            None,
        )
        .unwrap();
        let resp = parse_response(&output);
        let text = resp["result"]["contents"][0]["text"].as_str().unwrap();
        let inner: Value = serde_json::from_str(text).unwrap();
        assert_eq!(inner["cwd"], "/home/user/project");
        assert_eq!(inner["git_branch"], "develop");
    }

    #[test]
    fn tools_list_response() {
        let mut output = Vec::new();
        dispatch(
            &mut output,
            &json!({"jsonrpc":"2.0","id":4,"method":"tools/list"}).to_string(),
            Path::new("/tmp"),
            None,
        )
        .unwrap();
        let resp = parse_response(&output);
        let tools = resp["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 10);
        assert_eq!(tools[0]["name"], "get_spyc_context");
        assert_eq!(tools[1]["name"], "navigate_to");
        assert_eq!(tools[2]["name"], "set_filter");
        assert_eq!(tools[3]["name"], "pick_files");
        assert_eq!(tools[4]["name"], "clear_picks");
        assert_eq!(tools[5]["name"], "get_file_content");
        assert_eq!(tools[6]["name"], "search_paths");
        assert_eq!(tools[7]["name"], "search_content");
        assert_eq!(tools[8]["name"], "search_picks");
        assert_eq!(tools[9]["name"], "search_inventory");
    }

    #[test]
    fn tools_call_returns_context() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = context::SpycContext {
            cwd: PathBuf::from("/projects/spyc"),
            cursor_file: Some("Cargo.toml".into()),
            picks: vec![PathBuf::from("src/main.rs")],
            inventory: vec![],
            filter: Some("*.rs".into()),
            git_branch: Some("feature".into()),
            project_home: None,
            session_name: String::new(),
        };
        let ctx_path = context::context_path(tmp.path());
        context::write_context_file(&ctx_path, &ctx).unwrap();

        let mut output = Vec::new();
        dispatch(
            &mut output,
            &json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"get_spyc_context","arguments":{}}}).to_string(),
            &ctx_path,
            None,
        )
        .unwrap();
        let resp = parse_response(&output);
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        let inner: Value = serde_json::from_str(text).unwrap();
        assert_eq!(inner["cursor_file"], "Cargo.toml");
        assert_eq!(inner["filter"], "*.rs");
    }

    #[test]
    fn unknown_resource_returns_error() {
        let mut output = Vec::new();
        dispatch(
            &mut output,
            &json!({"jsonrpc":"2.0","id":6,"method":"resources/read","params":{"uri":"spyc://bogus"}}).to_string(),
            Path::new("/tmp"),
            None,
        )
        .unwrap();
        let resp = parse_response(&output);
        assert!(
            resp["error"]["message"]
                .as_str()
                .unwrap()
                .contains("Unknown resource")
        );
    }

    #[test]
    fn notification_is_silent() {
        let mut output = Vec::new();
        dispatch(
            &mut output,
            &json!({"jsonrpc":"2.0","method":"notifications/initialized"}).to_string(),
            Path::new("/tmp"),
            None,
        )
        .unwrap();
        assert!(output.is_empty());
    }

    #[test]
    fn search_paths_tool_walks_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join("alpha.rs"), "").unwrap();
        std::fs::write(root.join("beta.rs"), "").unwrap();
        std::fs::write(root.join("gamma.txt"), "").unwrap();
        let ctx = context::SpycContext {
            cwd: root.to_path_buf(),
            cursor_file: None,
            picks: vec![],
            inventory: vec![],
            filter: None,
            git_branch: None,
            project_home: Some(root.to_path_buf()),
            session_name: String::new(),
        };
        let ctx_path = context::context_path(tmp.path());
        context::write_context_file(&ctx_path, &ctx).unwrap();

        let mut output = Vec::new();
        dispatch(
            &mut output,
            &json!({"jsonrpc":"2.0","id":7,"method":"tools/call",
                "params":{"name":"search_paths","arguments":{"query":"alpha"}}})
            .to_string(),
            &ctx_path,
            None,
        )
        .unwrap();
        let resp = parse_response(&output);
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        let arr: Value = serde_json::from_str(text).unwrap();
        let paths: Vec<&str> = arr
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert!(paths.contains(&"alpha.rs"));
        assert!(!paths.contains(&"gamma.txt"));
    }

    #[test]
    fn search_content_tool_returns_matches() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join("a.txt"), "hello world\n").unwrap();
        let ctx = context::SpycContext {
            cwd: root.to_path_buf(),
            cursor_file: None,
            picks: vec![],
            inventory: vec![],
            filter: None,
            git_branch: None,
            project_home: Some(root.to_path_buf()),
            session_name: String::new(),
        };
        let ctx_path = context::context_path(tmp.path());
        context::write_context_file(&ctx_path, &ctx).unwrap();

        let mut output = Vec::new();
        dispatch(
            &mut output,
            &json!({"jsonrpc":"2.0","id":8,"method":"tools/call",
                "params":{"name":"search_content","arguments":{"pattern":"hello"}}})
            .to_string(),
            &ctx_path,
            None,
        )
        .unwrap();
        let resp = parse_response(&output);
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        let arr: Value = serde_json::from_str(text).unwrap();
        assert_eq!(arr.as_array().unwrap().len(), 1);
        assert_eq!(arr[0]["path"], "a.txt");
        assert_eq!(arr[0]["line"], 1);
    }

    #[test]
    fn search_picks_tool_only_picked_files() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("picked.txt"), "TARGET in picked\n").unwrap();
        std::fs::write(root.join("unpicked.txt"), "TARGET in unpicked\n").unwrap();
        let ctx = context::SpycContext {
            cwd: root.to_path_buf(),
            cursor_file: None,
            // Picks stored as relative paths in spyc's UI; resolved
            // against cwd by the tool.
            picks: vec![PathBuf::from("picked.txt")],
            inventory: vec![],
            filter: None,
            git_branch: None,
            project_home: Some(root.to_path_buf()),
            session_name: String::new(),
        };
        let ctx_path = context::context_path(tmp.path());
        context::write_context_file(&ctx_path, &ctx).unwrap();

        let mut output = Vec::new();
        dispatch(
            &mut output,
            &json!({"jsonrpc":"2.0","id":9,"method":"tools/call",
                "params":{"name":"search_picks","arguments":{"pattern":"TARGET"}}})
            .to_string(),
            &ctx_path,
            None,
        )
        .unwrap();
        let resp = parse_response(&output);
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        let arr: Value = serde_json::from_str(text).unwrap();
        let paths: Vec<&str> = arr
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v["path"].as_str())
            .collect();
        assert!(paths.contains(&"picked.txt"));
        assert!(
            !paths.iter().any(|p| p.contains("unpicked")),
            "unpicked file leaked into results: {paths:?}"
        );
    }

    #[test]
    fn read_lsp_message_parses() {
        let body = r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#;
        let framed = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
        let mut reader = Cursor::new(framed.as_bytes().to_vec());
        let msg = read_lsp_message(&mut reader).unwrap();
        assert_eq!(msg, body);
    }

    #[test]
    fn socket_server_responds() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = context::SpycContext {
            cwd: PathBuf::from("/test"),
            cursor_file: None,
            picks: vec![],
            inventory: vec![],
            filter: None,
            git_branch: None,
            project_home: None,
            session_name: String::new(),
        };
        let ctx_path = context::context_path(tmp.path());
        context::write_context_file(&ctx_path, &ctx).unwrap();

        // Use a temp socket path to avoid interfering with a running instance.
        let sock_path = tmp.path().join("test-mcp.sock");
        let listener = UnixListener::bind(&sock_path).unwrap();

        let (cmd_tx, _cmd_rx) = std::sync::mpsc::channel();
        let ctx_for_thread = ctx_path;
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            handle_socket_connection(stream, &ctx_for_thread, &cmd_tx).unwrap_or(());
        });

        // Give the server thread a moment to start.
        std::thread::sleep(std::time::Duration::from_millis(50));

        let stream = UnixStream::connect(&sock_path).unwrap();
        let mut reader = io::BufReader::new(stream.try_clone().unwrap());
        let mut writer = stream;

        let body = json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}).to_string();
        send_message(&mut writer, &body).unwrap();

        let response = read_lsp_message(&mut reader).unwrap();
        assert!(response.contains("get_spyc_context"));
    }

    #[test]
    fn disconnect_notification_routes_through_channel() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = context::SpycContext {
            cwd: PathBuf::from("/test"),
            cursor_file: None,
            picks: vec![],
            inventory: vec![],
            filter: None,
            git_branch: None,
            project_home: None,
            session_name: String::new(),
        };
        let ctx_path = context::context_path(tmp.path());
        context::write_context_file(&ctx_path, &ctx).unwrap();

        let sock_path = tmp.path().join("test-disconnect.sock");
        let listener = UnixListener::bind(&sock_path).unwrap();

        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
        let ctx_for_thread = ctx_path;
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            handle_socket_connection(stream, &ctx_for_thread, &cmd_tx).unwrap_or(());
        });

        std::thread::sleep(std::time::Duration::from_millis(50));

        let mut stream = UnixStream::connect(&sock_path).unwrap();
        let notification = json!({
            "jsonrpc": "2.0",
            "method": "spyc/disconnected",
            "params": { "new_pid": 99999 }
        });
        send_message(&mut stream, &notification.to_string()).unwrap();
        drop(stream); // close connection so handler exits

        let req = cmd_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        match req.command {
            McpCommand::Disconnected { new_pid } => assert_eq!(new_pid, 99999),
            other => panic!("expected Disconnected, got {other:?}"),
        }
    }

    #[test]
    fn pid_from_sock_path_parses() {
        assert_eq!(
            pid_from_sock_path("/home/user/.local/state/spyc/mcp-12345.sock"),
            Some(12345)
        );
        assert_eq!(pid_from_sock_path("mcp-1.sock"), Some(1));
        assert_eq!(pid_from_sock_path("mcp.sock"), None);
        assert_eq!(pid_from_sock_path("mcp-.sock"), None);
    }

    #[test]
    fn socket_path_contains_pid() {
        let path = socket_path();
        let pid = std::process::id();
        assert!(path.to_string_lossy().contains(&format!("mcp-{pid}.sock")));
    }

    // ── Project-scoped discovery ──────────────────────────────

    fn touch_context(dir: &Path, pid: u32) {
        std::fs::write(dir.join(format!(".spyc-context-{pid}.json")), b"{}").unwrap();
    }

    #[test]
    fn read_context_pids_finds_markers() {
        let tmp = tempfile::tempdir().unwrap();
        touch_context(tmp.path(), 100);
        touch_context(tmp.path(), 200);
        // Decoy: not our prefix.
        std::fs::write(tmp.path().join("not-spyc-300.json"), "{}").unwrap();
        // Decoy: malformed PID.
        std::fs::write(tmp.path().join(".spyc-context-abc.json"), "{}").unwrap();
        let mut pids = read_context_pids_in_dir(tmp.path());
        pids.sort_unstable();
        assert_eq!(pids, vec![100, 200]);
    }

    #[test]
    fn read_context_pids_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(read_context_pids_in_dir(tmp.path()).is_empty());
    }

    #[test]
    fn collect_pids_finds_marker_in_caller_dir() {
        let tmp = tempfile::tempdir().unwrap();
        touch_context(tmp.path(), 42);
        assert_eq!(collect_project_pids(tmp.path()), vec![42]);
    }

    #[test]
    fn collect_pids_walks_up_to_ancestor_marker() {
        // spyc started at /tmp/.../proj; claude in /tmp/.../proj/src/sub.
        let tmp = tempfile::tempdir().unwrap();
        let proj = tmp.path().join("proj");
        let sub = proj.join("src").join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        touch_context(&proj, 7);
        assert_eq!(collect_project_pids(&sub), vec![7]);
    }

    #[test]
    fn collect_pids_first_ancestor_with_match_wins() {
        // A spyc at /proj and another at /proj/inner. Caller in
        // /proj/inner should NOT see /proj's spyc — locality
        // beats inheritance.
        let tmp = tempfile::tempdir().unwrap();
        let proj = tmp.path().join("proj");
        let inner = proj.join("inner");
        std::fs::create_dir_all(&inner).unwrap();
        touch_context(&proj, 1);
        touch_context(&inner, 2);
        assert_eq!(collect_project_pids(&inner), vec![2]);
    }

    #[test]
    fn collect_pids_returns_all_pids_at_same_dir() {
        // Two spyc instances rooted at the same dir → both candidates.
        let tmp = tempfile::tempdir().unwrap();
        touch_context(tmp.path(), 100);
        touch_context(tmp.path(), 200);
        let mut pids = collect_project_pids(tmp.path());
        pids.sort_unstable();
        assert_eq!(pids, vec![100, 200]);
    }

    #[test]
    fn collect_pids_no_match_returns_empty() {
        // Cross-project case: caller's tree has no .spyc-context-*.json.
        // Sibling dir does, but that's deliberately invisible.
        let tmp = tempfile::tempdir().unwrap();
        let project_a = tmp.path().join("a");
        let project_b = tmp.path().join("b");
        std::fs::create_dir_all(&project_a).unwrap();
        std::fs::create_dir_all(&project_b).unwrap();
        touch_context(&project_b, 99);
        // Walking up from project_a hits tmp.path() (no marker), then
        // ancestors of the temp dir which we can't predict — but the
        // test passes as long as none of THOSE happen to have a
        // spyc-context file. In CI / typical dev machines they won't.
        // To make the test deterministic we anchor at project_a only.
        let pids = collect_project_pids(&project_a);
        assert!(
            !pids.contains(&99),
            "must not pick up sibling project's spyc"
        );
    }

    #[test]
    fn discover_live_socket_returns_none_without_project_marker() {
        let tmp = tempfile::tempdir().unwrap();
        // No .spyc-context-*.json in the caller's tree → discovery
        // must NOT fall through to scanning every socket on the
        // host (the cross-project bug we're fixing).
        assert!(discover_live_socket(tmp.path()).is_none());
    }

    #[test]
    fn get_file_content_blocks_path_traversal() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("project");
        std::fs::create_dir_all(&project).unwrap();

        // Create a file outside the project directory.
        let secret = tmp.path().join("secret.txt");
        std::fs::write(&secret, "top secret").unwrap();

        // Create a file inside the project directory.
        std::fs::write(project.join("ok.txt"), "public").unwrap();

        // Set up context with cwd = project.
        let ctx = context::SpycContext {
            cwd: project,
            cursor_file: None,
            picks: vec![],
            inventory: vec![],
            filter: None,
            git_branch: None,
            project_home: None,
            session_name: String::new(),
        };
        let ctx_path = context::context_path(tmp.path());
        context::write_context_file(&ctx_path, &ctx).unwrap();

        // Reading a file inside cwd should succeed.
        let mut output = Vec::new();
        dispatch(
            &mut output,
            &json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{
                "name":"get_file_content","arguments":{"path":"ok.txt"}
            }})
            .to_string(),
            &ctx_path,
            None,
        )
        .unwrap();
        let resp = parse_response(&output);
        assert!(
            resp["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("public")
        );

        // Traversal via ../secret.txt should be blocked.
        let mut output = Vec::new();
        dispatch(
            &mut output,
            &json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
                "name":"get_file_content","arguments":{"path":"../secret.txt"}
            }})
            .to_string(),
            &ctx_path,
            None,
        )
        .unwrap();
        let resp = parse_response(&output);
        assert!(
            resp["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("outside the working directory")
        );
    }

    // ---- ensure_codex_config_toml ------------------------------------

    #[test]
    fn codex_config_writes_fresh_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let status = ensure_codex_config_toml(tmp.path(), true).unwrap();
        assert!(matches!(status, McpConfigStatus::Configured));
        let written = std::fs::read_to_string(tmp.path().join(".codex").join("config.toml"))
            .expect("config.toml created");
        let parsed: toml::Value = toml::from_str(&written).unwrap();
        // Schema check: mcp_servers.spyc.{command,args,env.SPYC_MCP_SOCK}
        let spyc = &parsed["mcp_servers"]["spyc"];
        assert!(spyc["command"].as_str().unwrap_or("").contains("spyc"));
        assert_eq!(spyc["args"][0].as_str(), Some("--mcp"));
        assert!(
            spyc["env"]["SPYC_MCP_SOCK"]
                .as_str()
                .unwrap_or("")
                .contains("mcp-")
        );
    }

    #[test]
    fn codex_config_preserves_other_servers() {
        // A pre-existing entry from another tool must survive the splice.
        let tmp = tempfile::tempdir().unwrap();
        let codex_dir = tmp.path().join(".codex");
        std::fs::create_dir_all(&codex_dir).unwrap();
        let path = codex_dir.join("config.toml");
        std::fs::write(
            &path,
            r#"
[mcp_servers.other]
command = "/usr/local/bin/other-mcp"
args = ["--stdio"]

[mcp_servers.other.env]
KEY = "val"
"#,
        )
        .unwrap();
        ensure_codex_config_toml(tmp.path(), true).unwrap();
        let written = std::fs::read_to_string(&path).unwrap();
        let parsed: toml::Value = toml::from_str(&written).unwrap();
        // Both servers present.
        assert!(parsed["mcp_servers"].get("other").is_some());
        assert!(parsed["mcp_servers"].get("spyc").is_some());
        assert_eq!(
            parsed["mcp_servers"]["other"]["command"].as_str(),
            Some("/usr/local/bin/other-mcp")
        );
    }

    #[test]
    fn codex_config_fresh_rewrite_on_malformed_input() {
        // Top-level array (not a table) must not panic — mirror the
        // mcp.json shape-check fix from v1.41.5.
        let tmp = tempfile::tempdir().unwrap();
        let codex_dir = tmp.path().join(".codex");
        std::fs::create_dir_all(&codex_dir).unwrap();
        let path = codex_dir.join("config.toml");
        std::fs::write(&path, "not_a_section = 1\nrandom = \"junk\"\n").unwrap();
        // Don't crash; either splice into the (now-empty) file or rewrite.
        let status = ensure_codex_config_toml(tmp.path(), true).unwrap();
        assert!(matches!(status, McpConfigStatus::Configured));
        let written = std::fs::read_to_string(&path).unwrap();
        let parsed: toml::Value = toml::from_str(&written).unwrap();
        assert!(parsed["mcp_servers"]["spyc"].is_table());
    }

    #[test]
    fn codex_config_rewrites_completely_invalid_toml() {
        // Non-TOML content (e.g. corrupted file) falls back to a fresh
        // rewrite rather than failing.
        let tmp = tempfile::tempdir().unwrap();
        let codex_dir = tmp.path().join(".codex");
        std::fs::create_dir_all(&codex_dir).unwrap();
        let path = codex_dir.join("config.toml");
        std::fs::write(&path, "}}}}{{{ this is not toml").unwrap();
        let status = ensure_codex_config_toml(tmp.path(), true).unwrap();
        assert!(matches!(status, McpConfigStatus::Configured));
        let written = std::fs::read_to_string(&path).unwrap();
        let parsed: toml::Value = toml::from_str(&written).unwrap();
        assert!(parsed["mcp_servers"]["spyc"].is_table());
    }
}
