//! Unix-socket transport: discovery, the listener/serve loop, the stdio
//! proxy, and connection handling. Split out of mcp.rs verbatim.

use std::borrow::Cow;
use std::io::{self, BufRead, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

use crate::mcp_cmd::{McpCommand, McpRequest};

use super::protocol::{
    Caller, PANE_ID_META, dispatch, dispatch_for, read_lsp_message, send_message,
};
use super::{
    PROXY_IO_TIMEOUT, log_bodies, mcp_log, resolve_context_path, root_marker_path_in, socket_path,
    socket_path_for, state_dir,
};

/// Project-scoped discovery: the running spycs whose root contains
/// `caller_cwd`, read from their `mcp-<pid>.root` sidecars, nearest root
/// first. Only the nearest root's pids become candidates; we never aggregate
/// across levels, so a parent-dir spyc doesn't shadow a child-dir one.
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
            "stdio: discover: no spyc rooted at {} or above",
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
        let Some(sock) = socket_path_for(pid) else {
            continue;
        };
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

/// The pids of the running spycs rooted nearest above `start`, or none.
pub(super) fn collect_project_pids(start: &Path) -> Vec<u32> {
    let Some(dir) = state_dir() else {
        return Vec::new();
    };
    collect_project_pids_in(start, &dir)
}

/// As [`collect_project_pids`], with the state dir injected for tests.
///
/// Only the sidecars count. They live in the owner-private state dir, so
/// nothing a repository ships can name a spyc: a marker planted in a cloned
/// tree, the attack an in-tree marker invited, is never read. Roots and `start`
/// compare canonically, so a symlinked path finds the same spyc.
pub(super) fn collect_project_pids_in(start: &Path, state_dir: &Path) -> Vec<u32> {
    let start = std::fs::canonicalize(start).unwrap_or_else(|_| start.to_path_buf());
    let Ok(entries) = std::fs::read_dir(state_dir) else {
        return Vec::new();
    };
    let mut rooted: Vec<(PathBuf, u32)> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name
            .to_str()
            .and_then(|n| n.strip_prefix("mcp-"))
            .and_then(|n| n.strip_suffix(".root"))
            .and_then(|p| p.parse::<u32>().ok())
        else {
            continue;
        };
        let Ok(recorded) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        let root = Path::new(recorded.trim());
        let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        if start.starts_with(&root) {
            rooted.push((root, pid));
        }
    }
    let Some(nearest) = rooted.iter().map(|(r, _)| r.components().count()).max() else {
        return Vec::new();
    };
    rooted
        .into_iter()
        .filter(|(r, _)| r.components().count() == nearest)
        .map(|(_, pid)| pid)
        .collect()
}

/// Record `root` as this process's in its sidecar: discovery's only evidence
/// of where a spyc is rooted. Best-effort — a missing sidecar fails safe,
/// leaving the spyc undiscovered rather than misattributed.
fn write_root_marker(state_dir: &Path, root: &Path) {
    let canon = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let path = root_marker_path_in(state_dir, std::process::id());
    // A non-UTF-8 root would be stored lossily and later fail the
    // canonical compare → refuse; an acceptable (and safe) edge.
    if crate::fs::write_atomic(&path, canon.to_string_lossy().as_bytes()).is_ok() {
        // Match the socket's owner-only posture: the file only holds a
        // directory path, but no reason to expose project roots to other
        // local users.
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
}

/// Move this process's recorded root to `root` (#523): a `spyc -r` that
/// restores a project elsewhere moves the root, and discovery must follow.
pub fn record_root(root: &Path) {
    if let Some(dir) = state_dir() {
        write_root_marker(&dir, root);
    }
}

/// Reap the sidecars and sockets of spycs that exited without tearing down
/// (SIGKILL, a crash), which would otherwise be discovery candidates forever.
/// Never touches a live pid's, ours included.
pub fn sweep_orphan_root_markers(state_dir: &Path, our_pid: u32) -> usize {
    let Ok(entries) = std::fs::read_dir(state_dir) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name.to_str().and_then(|n| {
            let rest = n.strip_prefix("mcp-")?;
            rest.strip_suffix(".root")
                .or_else(|| rest.strip_suffix(".sock"))?
                .parse::<u32>()
                .ok()
        }) else {
            continue;
        };
        if pid == our_pid || crate::sysinfo::pid_alive(pid) {
            continue;
        }
        if std::fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
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

/// `msg` with `pane_id` under `params._meta` when it is an `initialize`
/// request, so the server can bind this connection to the pane; anything
/// else, or no pane id, passes through untouched.
pub(super) fn annotate_initialize<'a>(msg: &'a str, pane_id: Option<&str>) -> Cow<'a, str> {
    let Some(pane_id) = pane_id.filter(|p| !p.is_empty()) else {
        return Cow::Borrowed(msg);
    };
    let Ok(mut request) = serde_json::from_str::<Value>(msg) else {
        return Cow::Borrowed(msg);
    };
    if request["method"] != "initialize" || request.get("id").is_none() {
        return Cow::Borrowed(msg);
    }
    let meta = request
        .as_object_mut()
        .map(|r| r.entry("params").or_insert_with(|| json!({})))
        .and_then(Value::as_object_mut)
        .map(|p| p.entry("_meta").or_insert_with(|| json!({})))
        .and_then(Value::as_object_mut);
    let Some(meta) = meta else {
        return Cow::Borrowed(msg);
    };
    meta.insert(PANE_ID_META.into(), Value::String(pane_id.into()));
    Cow::Owned(request.to_string())
}

/// Proxy stdin/stdout ↔ Unix socket. Messages use Content-Length
/// framing on both sides.
pub(super) fn run_proxy(stream: UnixStream) -> anyhow::Result<()> {
    // Read once: the connection's attribution is fixed at its `initialize`.
    let pane_id = std::env::var("SPYC_PANE_ID").ok();
    proxy_io(
        io::stdin().lock(),
        io::stdout().lock(),
        stream,
        pane_id.as_deref(),
    )
}

/// [`run_proxy`] over any agent-side reader and writer, naming `pane_id` in
/// the `initialize` it forwards.
pub(super) fn proxy_io(
    mut stdin_reader: impl BufRead,
    mut stdout_writer: impl Write,
    stream: UnixStream,
    pane_id: Option<&str>,
) -> anyhow::Result<()> {
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
        let msg = &*annotate_initialize(msg, pane_id);
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
    root: &Path,
    cmd_tx: std::sync::mpsc::Sender<McpRequest>,
) -> anyhow::Result<()> {
    // Refuse rather than fall back: the pre-unification code bound into a
    // world-readable bare `/tmp`, where a predictable socket name invites
    // squatting. No MCP beats MCP on a path we don't trust.
    let Some(sock) = socket_path() else {
        mcp_log("socket: no state directory ($XDG_STATE_HOME / $HOME unset) — not serving");
        anyhow::bail!("no state directory ($XDG_STATE_HOME / $HOME unset) — MCP server disabled");
    };

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

    // Record the directory this spyc is rooted at, next to the socket, for
    // stdio discovery. Done before any connection is served.
    if let Some(dir) = state_dir() {
        write_root_marker(&dir, root);
    }

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
    if let Some(sock) = socket_path() {
        let _ = std::fs::remove_file(sock);
    }
    if let Some(dir) = state_dir() {
        let _ = std::fs::remove_file(root_marker_path_in(&dir, std::process::id()));
    }
}

/// Extract the PID from a socket path like `mcp-12345.sock`.
pub(super) fn pid_from_sock_path(path: &str) -> Option<u32> {
    let fname = Path::new(path).file_name()?.to_str()?;
    let stripped = fname.strip_prefix("mcp-")?.strip_suffix(".sock")?;
    stripped.parse().ok()
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
    let mut caller = Caller::connection(NEXT_CONNECTION.fetch_add(1, Ordering::Relaxed));
    let served = serve_connection(&mut reader, &mut writer, ctx_path, cmd_tx, &mut caller);
    caller.close(cmd_tx);
    served
}

/// Numbers each accepted connection for `:activity dump`.
static NEXT_CONNECTION: AtomicU64 = AtomicU64::new(1);

/// [`handle_socket_connection`]'s read-dispatch loop, until the client closes.
fn serve_connection(
    reader: &mut impl BufRead,
    writer: &mut UnixStream,
    ctx_path: &Path,
    cmd_tx: &std::sync::mpsc::Sender<McpRequest>,
    caller: &mut Caller,
) -> io::Result<()> {
    loop {
        let msg = match read_lsp_message(reader) {
            Ok(msg) => msg,
            Err(e) => {
                if e.kind() == io::ErrorKind::UnexpectedEof {
                    break; // Connection closed.
                }
                // A message we couldn't frame/parse. Don't drop it silently —
                // log it AND surface a status-line warning to the TUI, then
                // close the connection (the client reconnects). Silent drops
                // hid the bare-newline report-status framing bug for days.
                mcp_log(&format!("socket: dropping malformed message: {e}"));
                let (reply_tx, _) = std::sync::mpsc::channel();
                let _ = cmd_tx.send(McpRequest {
                    command: McpCommand::MalformedSocketMessage {
                        detail: e.to_string(),
                    },
                    reply: reply_tx,
                });
                break;
            }
        };
        dispatch_for(writer, &msg, ctx_path, Some(cmd_tx), caller)?;
    }
    Ok(())
}

// ── Protocol handlers ────────────────────────────────────────────
