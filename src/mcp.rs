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
        if method == "spyc/disconnected" {
            if let Some(tx) = cmd_tx {
                let new_pid = parsed["params"]["new_pid"].as_u64().unwrap_or(0) as u32;
                let (reply_tx, _) = std::sync::mpsc::channel();
                let _ = tx.send(McpRequest {
                    command: McpCommand::Disconnected { new_pid },
                    reply: reply_tx,
                });
            }
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
    if let Ok(p) = std::env::var(context::CONTEXT_ENV_VAR) {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    context::context_path(project_root)
}

/// Run the stdio MCP server. Reads JSONL from stdin, dispatches
/// locally, writes JSONL to stdout. If the running spyc instance's
/// Unix socket is available, proxies through it for writable access.
///
/// Socket resolution order:
/// 1. `$SPYC_MCP_SOCK` (set in `.mcp.json`'s `env` block) — exact match
/// 2. Discovery scan of `~/.local/state/spyc/mcp-*.sock` — finds any
///    live instance (handles enterprise managed-mcp.json, manual testing)
/// 3. Falls back to read-only direct mode if nothing is alive.
pub fn run(project_root: PathBuf) -> anyhow::Result<()> {
    // Try explicit socket path from env first.
    if let Ok(p) = std::env::var("SPYC_MCP_SOCK") {
        if !p.is_empty() {
            let sock = PathBuf::from(&p);
            mcp_log(&format!("stdio: trying env socket {}", sock.display()));
            if let Ok(stream) = UnixStream::connect(&sock) {
                mcp_log("stdio: connected via env socket, proxying");
                return run_proxy(stream);
            }
        }
    }

    // Discovery: scan for any live mcp-*.sock.
    if let Some(stream) = discover_live_socket() {
        return run_proxy(stream);
    }

    // Direct mode: read-only local dispatch (no writable actions).
    mcp_log("stdio: no live socket found, running direct JSONL");
    run_direct(project_root)
}

/// Scan `~/.local/state/spyc/` for `mcp-*.sock` files and try to
/// connect to each. Returns the first live connection found.
fn discover_live_socket() -> Option<UnixStream> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let state_dir = PathBuf::from(home).join(".local/state/spyc");
    let entries = std::fs::read_dir(&state_dir).ok()?;

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if !name_str.starts_with("mcp-") || !name_str.ends_with(".sock") {
            continue;
        }
        let sock = entry.path();
        mcp_log(&format!("stdio: discover trying {}", sock.display()));
        if let Ok(stream) = UnixStream::connect(&sock) {
            mcp_log(&format!("stdio: discovered live socket {}", sock.display()));
            return Some(stream);
        }
        // Stale socket — clean it up.
        let _ = std::fs::remove_file(&sock);
    }
    None
}

/// Direct JSONL stdio server — no socket proxy.
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
        mcp_log(&format!("proxy: stdin → socket ({} bytes): {}", msg.len(), msg));

        // Check if this is a request (has "id") or notification (no "id").
        let is_request = serde_json::from_str::<Value>(msg)
            .map(|v| v.get("id").is_some())
            .unwrap_or(true); // assume request if parse fails

        // Forward to socket (Content-Length framed for the socket server).
        send_message(&mut sock_writer, msg)?;

        // Only read a response for requests (notifications get no reply).
        if is_request {
            let response = read_lsp_message(&mut sock_reader)?;
            mcp_log(&format!("proxy: socket → stdout ({} bytes): {}", response.len(), response));
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
    let old_umask = unsafe { libc::umask(0o077) };
    let bind_result = UnixListener::bind(&sock);
    unsafe { libc::umask(old_umask) };
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
    /// Enterprise managed-settings.json blocks spyc.
    BlockedByEnterprise,
}

/// Well-known paths for Claude Code enterprise managed settings.
const MANAGED_SETTINGS_PATHS: &[&str] = &[
    // macOS system-wide
    "/Library/Application Support/ClaudeCode/managed-settings.json",
    // Linux / WSL system-wide
    "/etc/claude-code/managed-settings.json",
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
        if let Some(denied) = parsed.get("deniedMcpServers") {
            if let Some(arr) = denied.as_array() {
                if arr.iter().any(|entry| entry["serverName"].as_str() == Some("spyc")) {
                    return Some(false);
                }
            }
        }
        // Allowlist: if present, spyc must be in it.
        if let Some(allowed) = parsed.get("allowedMcpServers") {
            if let Some(arr) = allowed.as_array() {
                let ok = arr.iter().any(|entry| entry["serverName"].as_str() == Some("spyc"));
                return Some(ok);
            }
        }
        return None; // Enterprise config exists but no MCP restrictions.
    }
    None // No enterprise config found.
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
    mcp_log(&format!(
        "sent spyc/disconnected to {}",
        old_sock.display()
    ));
}

/// Ensure `.mcp.json` has the spyc entry using stdio transport.
/// Checks enterprise policy first. If another spyc instance owns
/// the entry, sends it a disconnect notification and takes over.
pub fn ensure_mcp_json(dir: &Path) -> Result<McpConfigStatus, io::Error> {
    if let Some(false) = enterprise_allows_spyc() {
        return Ok(McpConfigStatus::BlockedByEnterprise);
    }

    let our_sock = socket_path();
    let our_pid = std::process::id();
    let path = dir.join(".mcp.json");
    let exe = std::env::current_exe()
        .unwrap_or_else(|_| PathBuf::from("spyc"));

    // Check for an existing live spyc instance in this directory.
    let mut took_over: Option<u32> = None;
    if let Ok(text) = std::fs::read_to_string(&path) {
        if let Ok(parsed) = serde_json::from_str::<Value>(&text) {
            if let Some(old_sock_str) = parsed
                .pointer("/mcpServers/spyc/env/SPYC_MCP_SOCK")
                .and_then(|v| v.as_str())
            {
                let old_sock = PathBuf::from(old_sock_str);
                if old_sock != our_sock {
                    // Another instance — check if it's still alive.
                    if UnixStream::connect(&old_sock).is_ok() {
                        let old_pid = pid_from_sock_path(old_sock_str).unwrap_or(0);
                        notify_disconnect(&old_sock, our_pid);
                        took_over = Some(old_pid);
                        mcp_log(&format!(
                            "taking over from PID {old_pid} ({})",
                            old_sock.display()
                        ));
                    }
                }
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

    let content = if let Ok(text) = std::fs::read_to_string(&path) {
        if let Ok(mut parsed) = serde_json::from_str::<Value>(&text) {
            parsed
                .as_object_mut()
                .unwrap()
                .entry("mcpServers")
                .or_insert_with(|| json!({}))
                .as_object_mut()
                .unwrap()
                .insert("spyc".to_string(), spyc_entry);
            serde_json::to_string_pretty(&parsed).unwrap()
        } else {
            serde_json::to_string_pretty(&json!({ "mcpServers": { "spyc": spyc_entry } })).unwrap()
        }
    } else {
        serde_json::to_string_pretty(&json!({ "mcpServers": { "spyc": spyc_entry } })).unwrap()
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

/// Handle a single Unix socket connection. Uses the same Content-Length
/// framing as the stdio transport.
fn handle_socket_connection(
    stream: UnixStream,
    ctx_path: &Path,
    cmd_tx: &std::sync::mpsc::Sender<McpRequest>,
) -> io::Result<()> {
    let mut reader = io::BufReader::new(stream.try_clone()?);
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
            if tx.send(McpRequest { command, reply: reply_tx }).is_err() {
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

/// Read the cwd from the context file (for resolving relative paths).
fn read_cwd_from_context(ctx_path: &Path) -> PathBuf {
    if let Ok(text) = std::fs::read_to_string(ctx_path) {
        if let Ok(v) = serde_json::from_str::<Value>(&text) {
            if let Some(cwd) = v["cwd"].as_str() {
                return PathBuf::from(cwd);
            }
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"))
}

/// Read file content (up to 100KB, text only).
fn read_file_content(path: &Path) -> Result<String, String> {
    let meta = std::fs::metadata(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
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
    let bytes = std::fs::read(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    // Reject binary files (null bytes in first 8KB).
    let check_len = bytes.len().min(8192);
    if bytes[..check_len].contains(&0) {
        return Err(format!("{}: binary file", path.display()));
    }
    String::from_utf8(bytes)
        .map_err(|_| format!("{}: not valid UTF-8", path.display()))
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

    let len = content_length.ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "missing Content-Length")
    })?;

    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf)?;
    String::from_utf8(buf)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
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
            &json!({"jsonrpc":"2.0","id":3,"method":"resources/read","params":{"uri":CONTEXT_URI}}).to_string(),
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
        assert_eq!(tools.len(), 6);
        assert_eq!(tools[0]["name"], "get_spyc_context");
        assert_eq!(tools[1]["name"], "navigate_to");
        assert_eq!(tools[2]["name"], "set_filter");
        assert_eq!(tools[3]["name"], "pick_files");
        assert_eq!(tools[4]["name"], "clear_picks");
        assert_eq!(tools[5]["name"], "get_file_content");
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
        assert!(resp["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Unknown resource"));
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
        let ctx_for_thread = ctx_path.clone();
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
        let ctx_for_thread = ctx_path.clone();
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

        let req = cmd_rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        match req.command {
            McpCommand::Disconnected { new_pid } => assert_eq!(new_pid, 99999),
            other => panic!("expected Disconnected, got {other:?}"),
        }
    }

    #[test]
    fn pid_from_sock_path_parses() {
        assert_eq!(pid_from_sock_path("/home/user/.local/state/spyc/mcp-12345.sock"), Some(12345));
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
            cwd: project.clone(),
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
            }}).to_string(),
            &ctx_path,
            None,
        ).unwrap();
        let resp = parse_response(&output);
        assert!(resp["result"]["content"][0]["text"].as_str().unwrap().contains("public"));

        // Traversal via ../secret.txt should be blocked.
        let mut output = Vec::new();
        dispatch(
            &mut output,
            &json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
                "name":"get_file_content","arguments":{"path":"../secret.txt"}
            }}).to_string(),
            &ctx_path,
            None,
        ).unwrap();
        let resp = parse_response(&output);
        assert!(resp["result"]["content"][0]["text"].as_str().unwrap().contains("outside the working directory"));
    }
}
