//! Unix-socket transport: discovery, the listener/serve loop, the stdio
//! proxy, and connection handling. Split out of mcp.rs verbatim.

use std::io::{self, BufRead, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};

use crate::mcp_cmd::McpRequest;

use super::protocol::{dispatch, read_lsp_message, send_message};
use super::{
    PROXY_IO_TIMEOUT, log_bodies, mcp_log, resolve_context_path, root_marker_path_in, socket_path,
    socket_path_for, state_dir,
};

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
pub(super) fn discover_live_socket(caller_cwd: &Path) -> Option<UnixStream> {
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
/// first ancestor that has any *trusted* matches; empty Vec otherwise.
pub(super) fn collect_project_pids(start: &Path) -> Vec<u32> {
    collect_project_pids_in(start, &state_dir())
}

/// As [`collect_project_pids`], but with the state dir injected so tests
/// can point the trusted-root lookup at a temp dir. A marker only counts
/// if its `.spyc-context-<pid>.json` lives in the directory the running
/// spyc with that pid actually recorded as its root (see
/// [`root_marker_path_in`]). Markers with no sidecar, or rooted
/// elsewhere (the planted-marker attack), are skipped — and we keep
/// walking up rather than letting an untrusted marker form a boundary
/// that shadows a legitimate ancestor spyc.
pub(super) fn collect_project_pids_in(start: &Path, state_dir: &Path) -> Vec<u32> {
    let mut here: &Path = start;
    loop {
        let pids: Vec<u32> = read_context_pids_in_dir(here)
            .into_iter()
            .filter(|&pid| marker_root_is_trusted(state_dir, pid, here))
            .collect();
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

/// True iff the running spyc that owns `pid` recorded `marker_dir` as its
/// root in its trusted sidecar. A missing sidecar fails safe (untrusted).
fn marker_root_is_trusted(state_dir: &Path, pid: u32, marker_dir: &Path) -> bool {
    let Ok(recorded) = std::fs::read_to_string(root_marker_path_in(state_dir, pid)) else {
        return false;
    };
    root_matches(Path::new(recorded.trim()), marker_dir)
}

/// Compare a recorded root against a marker directory by canonical form,
/// so symlink / relative-path differences don't cause false rejects.
/// Falls back to a literal compare if either path can't be canonicalized.
fn root_matches(recorded: &Path, marker_dir: &Path) -> bool {
    match (
        std::fs::canonicalize(recorded),
        std::fs::canonicalize(marker_dir),
    ) {
        (Ok(a), Ok(b)) => a == b,
        _ => recorded == marker_dir,
    }
}

/// Write the trusted-root sidecar for the current process: the directory
/// `ctx_path` lives in (the dir where this spyc writes its
/// `.spyc-context-<pid>.json` marker). Best-effort — discovery treats a
/// missing sidecar as untrusted, so a failed write fails safe (refuse),
/// never open.
fn write_root_marker(state_dir: &Path, ctx_path: &Path) {
    let root = ctx_path.parent().unwrap_or_else(|| Path::new("."));
    let canon = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let path = root_marker_path_in(state_dir, std::process::id());
    // A non-UTF-8 root would be stored lossily and later fail the
    // canonical compare → refuse; an acceptable (and safe) edge.
    if std::fs::write(&path, canon.to_string_lossy().as_bytes()).is_ok() {
        // Match the socket's owner-only posture: the file only holds a
        // directory path, but no reason to expose project roots to other
        // local users.
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
}

/// Read `.spyc-context-<pid>.json` filenames in `dir`, returning the
/// PIDs parsed out of them. Order is unspecified (matches `read_dir`),
/// which is fine because the caller tries each candidate in turn.
pub(super) fn read_context_pids_in_dir(dir: &Path) -> Vec<u32> {
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
pub(super) fn run_direct(project_root: PathBuf) -> anyhow::Result<()> {
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
pub(super) fn run_proxy(stream: UnixStream) -> anyhow::Result<()> {
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
        if log_bodies() {
            mcp_log(&format!(
                "proxy: stdin → socket ({} bytes): {}",
                msg.len(),
                msg
            ));
        } else {
            mcp_log(&format!("proxy: stdin → socket ({} bytes)", msg.len()));
        }

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
            if log_bodies() {
                mcp_log(&format!(
                    "proxy: socket → stdout ({} bytes): {}",
                    response.len(),
                    response
                ));
            } else {
                mcp_log(&format!(
                    "proxy: socket → stdout ({} bytes)",
                    response.len()
                ));
            }
            // Write back as newline-delimited JSON (what Claude Code expects).
            writeln!(stdout_writer, "{response}")?;
            stdout_writer.flush()?;
        }
    }
    Ok(())
}

// ── Unix socket server (background thread) ──────────────────────

/// Turn a failed MCP socket bind into a helpful error. A permission-denied
/// bind (EACCES / EPERM) almost always means a restricted sandbox refused the
/// bind — not a real misconfiguration — so point the user at rerunning under
/// normal permissions instead of leaving them with a bare "Operation not
/// permitted". Other errors get plain path context.
pub(super) fn socket_bind_error(err: std::io::Error, sock: &Path) -> anyhow::Error {
    if err.kind() == std::io::ErrorKind::PermissionDenied {
        anyhow::anyhow!(
            "MCP socket bind denied at {} ({err}) — this usually means a restricted \
             sandbox; rerun under normal permissions",
            sock.display()
        )
    } else {
        anyhow::Error::new(err).context(format!("binding MCP socket at {}", sock.display()))
    }
}

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
    let listener = bind_result.map_err(|e| socket_bind_error(e, &sock))?;

    // Record the directory this spyc is rooted at, next to the socket, so
    // stdio discovery can verify a `.spyc-context-<pid>.json` marker
    // really belongs to a spyc rooted there — not a planted decoy. Done
    // before any connection is served.
    write_root_marker(&state_dir(), &ctx_path);

    let ctx_path = Arc::new(ctx_path);
    let cmd_tx = Arc::new(cmd_tx);

    mcp_log(&format!("socket: listening on {}", sock.display()));

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let stream = match stream {
                Ok(s) => s,
                Err(e) => {
                    // A *persistent* accept error — classically EMFILE/ENFILE
                    // (the process or system fd table is full) — would spin
                    // this loop at 100% CPU: `incoming()` yields the same
                    // error immediately on every iteration. Back off briefly
                    // so the descriptor pressure can ease, then retry instead
                    // of busy-looping. Transient errors cost only one sleep.
                    mcp_log(&format!("socket: accept error: {e}"));
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    continue;
                }
            };
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

/// Clean up the socket file and trusted-root sidecar on shutdown.
pub fn cleanup_socket() {
    let _ = std::fs::remove_file(socket_path());
    let _ = std::fs::remove_file(root_marker_path_in(&state_dir(), std::process::id()));
}

/// Extract the PID from a socket path like `mcp-12345.sock`.
pub(super) fn pid_from_sock_path(path: &str) -> Option<u32> {
    let fname = Path::new(path).file_name()?.to_str()?;
    let stripped = fname.strip_prefix("mcp-")?.strip_suffix(".sock")?;
    stripped.parse().ok()
}

/// Try to send a `spyc/disconnected` notification to the old instance's
/// socket. Best-effort — if it fails, we proceed with takeover anyway.
pub(super) fn notify_disconnect(old_sock: &Path, new_pid: u32) {
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

/// Handle a single Unix socket connection. Uses the same Content-Length
/// framing as the stdio transport.
pub(super) fn handle_socket_connection(
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
