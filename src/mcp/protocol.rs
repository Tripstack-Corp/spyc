//! MCP JSON-RPC dispatch and request handlers; the framing is `framing.rs`'s.
//! Split out of mcp.rs verbatim during the 800-LoC decomposition.
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::mcp_cmd::{McpCommand, McpRequest, McpResponse};

use super::framing::{send_error, send_result, send_tool_error, send_tool_result};
use super::readers::{
    DiffMode, claim_worktree_result, effective_root, git_diff_text, git_log_json, git_status_json,
    grep_matches_to_json, is_in_mount, list_worktrees_json, read_context_or_empty,
    read_cwd_from_context, read_file_content, read_inventory_from_context, read_member_content,
    read_picks_from_context, release_worktree_result,
};
use super::{
    CONTEXT_URI, PROTOCOL_VERSION, PROXY_IO_TIMEOUT, SERVER_INSTRUCTIONS, SERVER_NAME,
    SERVER_VERSION, mcp_log,
};

/// Per-call ceiling for the read tools that walk the filesystem / git (search,
/// git status / log / diff, worktree listing). Kept a few seconds below
/// `PROXY_IO_TIMEOUT` so a slow call fails *server-side* with a clean JSON-RPC
/// error first: the stdio proxy reacts to its own read timeout by killing the
/// whole MCP connection, so the server must reply before that fires. Derived
/// from `PROXY_IO_TIMEOUT` so the two can't drift apart.
const READ_TOOL_TIMEOUT: std::time::Duration = {
    let proxy_secs = PROXY_IO_TIMEOUT.as_secs();
    std::time::Duration::from_secs(if proxy_secs > 5 {
        proxy_secs - 5
    } else {
        proxy_secs
    })
};

/// P2 `wait_for_scope_clear` bounds: the default wait when the caller gives no
/// `timeout_ms`, and a hard ceiling so a wedged waiter can't pin a socket thread
/// forever. The socket-side reply timeout is derived from these + a buffer so it
/// always outlasts the loop's own timed-out reply.
const DEFAULT_SCOPE_WAIT_MS: u64 = 300_000;
const MAX_SCOPE_WAIT_MS: u64 = 600_000;

/// Why a tree-walking tool can't run against `root`, when `root` is inside a
/// mounted archive.
///
/// Both the finder and grep walk real directories, and a mount has none — the
/// members are index entries. Pointing them at the mount root would walk a single
/// binary *file* and report "no matches", which reads as an answer rather than as
/// the wrong question.
fn mount_refusal(root: &Path, ctx_path: &Path, tool: &str) -> Option<String> {
    is_in_mount(root, ctx_path).then(|| {
        format!(
            "{tool}: {} is inside a mounted archive, whose members aren't on disk. \
             Read one with get_file_content, or pass `root` to search a real directory.",
            root.display()
        )
    })
}

/// Run `f` on a detached thread and wait at most `timeout` for its result,
/// returning `Err` on timeout. There is no cancellation: a timed-out thread
/// runs to completion in the background — acceptable because the work is pure
/// reads and the alternative (blocking until the proxy's socket timeout) kills
/// the whole MCP connection.
fn call_with_timeout<T, F>(timeout: std::time::Duration, f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(timeout)
        .map_err(|_| "timed out".to_string())
}

/// The `initialize` `_meta` key the `spyc --mcp` proxy puts its
/// `$SPYC_PANE_ID` under.
pub(super) const PANE_ID_META: &str = "spyc/paneId";

/// The `initialize` `_meta` key the `spyc --mcp` proxy puts its cwd under, so
/// a connection with no pane still finds its project (#99).
pub(super) const CWD_META: &str = "spyc/cwd";

/// Who is on the other end of one connection: the pane its `initialize`
/// named, once the main loop has confirmed that tab is live, and the project
/// it belongs to (#99). Bound at `initialize`, and bound again only once that
/// project has closed, when it was bound to no tab; never taken from a tool
/// call. An unbound caller (an older proxy, the status hook,
/// the read-only fallback) behaves as every caller did before attribution
/// existed.
#[derive(Debug, Default)]
pub(super) struct Caller {
    /// This socket connection's number; `None` in the read-only fallback.
    conn: Option<u64>,
    pane_id: Option<String>,
    /// The project its driving tools act on.
    project: Option<crate::project_id::ProjectId>,
    /// That project's context file, which its reads answer from; `None` reads
    /// the file the server was started with.
    context: Option<PathBuf>,
    /// The cwd its proxy named at `initialize`, which binds it again when its
    /// project closes.
    cwd: Option<PathBuf>,
    /// Whether it has sent `initialize`, which is what makes it an agent's
    /// session rather than the status hook's one-shot call.
    initialized: bool,
}

impl Caller {
    pub(super) fn connection(conn: u64) -> Self {
        Self {
            conn: Some(conn),
            ..Self::default()
        }
    }

    /// Tell the loop this connection is gone, if it was ever an agent's.
    pub(super) fn close(&self, cmd_tx: &std::sync::mpsc::Sender<McpRequest>) {
        if let (true, Some(conn)) = (self.initialized, self.conn) {
            tell(cmd_tx, McpCommand::ConnectionClosed { conn });
        }
    }
}

/// Send the loop a command whose reply nobody reads.
fn tell(tx: &std::sync::mpsc::Sender<McpRequest>, command: McpCommand) {
    let (reply_tx, _) = std::sync::mpsc::channel();
    let _ = tx.send(McpRequest {
        command,
        reply: reply_tx,
        project: None,
    });
}

/// The loop's JSON answer to `command`; `None` when it answers with an error,
/// or not in time.
fn ask(tx: &std::sync::mpsc::Sender<McpRequest>, command: McpCommand) -> Option<Value> {
    let (reply_tx, reply_rx) = std::sync::mpsc::channel();
    tx.send(McpRequest {
        command,
        reply: reply_tx,
        project: None,
    })
    .ok()?;
    match reply_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .ok()?
    {
        McpResponse::Ok { message } => serde_json::from_str(&message).ok(),
        McpResponse::Error { .. } => None,
    }
}

/// Bind `caller` to the pane and project its `initialize` names: the pane id
/// the proxy sent, else its cwd (`McpCommand::Bind`). One that names neither
/// stays unbound, as does one the loop doesn't answer.
fn bind(tx: &std::sync::mpsc::Sender<McpRequest>, meta: &Value, caller: &mut Caller) {
    let pane_id = meta[PANE_ID_META].as_str().filter(|p| !p.is_empty());
    let cwd = meta[CWD_META].as_str().filter(|c| !c.is_empty());
    if pane_id.is_none() && cwd.is_none() {
        return;
    }
    bind_to(tx, pane_id, cwd.map(PathBuf::from), caller, "initialize");
}

/// Ask the loop which pane and project `pane_id`, else `cwd`, else the
/// project the user is in, binds `caller` to, and bind it there. `why` names
/// the occasion in the log.
fn bind_to(
    tx: &std::sync::mpsc::Sender<McpRequest>,
    pane_id: Option<&str>,
    cwd: Option<PathBuf>,
    caller: &mut Caller,
    why: &str,
) {
    let command = McpCommand::Bind {
        pane_id: pane_id.map(String::from),
        cwd: cwd.clone(),
    };
    let Some(bound) = ask(tx, command) else {
        mcp_log(&format!("{why}: the loop didn't answer the bind"));
        return;
    };
    caller.project = bound["project"].as_str().and_then(|p| p.parse().ok());
    caller.context = bound["context_file"].as_str().map(PathBuf::from);
    let by_cwd = cwd.is_some();
    caller.cwd = cwd;
    let project = bound["project"].as_str().unwrap_or("none");
    match pane_id {
        Some(pane_id) if bound["pane"] == true => {
            mcp_log(&format!(
                "{why}: connection bound to pane {pane_id}, project {project}"
            ));
            caller.pane_id = Some(pane_id.to_string());
        }
        Some(pane_id) => mcp_log(&format!(
            "{why}: no live pane {pane_id}; bound to project {project}"
        )),
        None if by_cwd => mcp_log(&format!("{why}: bound by cwd to project {project}")),
        None => mcp_log(&format!("{why}: bound to the user's project {project}")),
    }
    if caller.initialized {
        tell_bound(tx, caller);
    }
}

/// Tell the loop which project `caller`'s connection acts on, for `:activity
/// dump`.
fn tell_bound(tx: &std::sync::mpsc::Sender<McpRequest>, caller: &Caller) {
    if let (Some(conn), Some(project)) = (caller.conn, caller.project) {
        tell(tx, McpCommand::ConnectionBound { conn, project });
    }
}

/// `create_worktree`'s arguments, carrying the connection's own pane and cwd,
/// where the loop finds the agent's repo for a project outside any repo.
fn create_worktree_command(args: &Value, caller: &Caller) -> Result<McpCommand, &'static str> {
    let branch = args["branch"].as_str().unwrap_or("").to_string();
    if branch.trim().is_empty() {
        return Err("missing required parameter: branch");
    }
    Ok(McpCommand::CreateWorktree {
        branch,
        base: args["base"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .map(String::from),
        open: args["open"].as_bool().unwrap_or(false),
        pane_id: caller.pane_id.clone(),
        cwd: caller.cwd.clone(),
    })
}

/// `report_status`'s arguments. A status hook adds the agent's `session_id`
/// and its event's metadata; a direct agent call carries neither.
fn report_status_command(args: &Value, caller: &Caller) -> Result<McpCommand, &'static str> {
    let status = args["status"].as_str().unwrap_or("").to_string();
    if crate::pane::AgentActivity::reportable(&status).is_none()
        && !matches!(
            status.as_str(),
            crate::agent::codex_recovery::QUESTION_START
                | crate::agent::codex_recovery::QUESTION_END
        )
    {
        return Err("status must be one of: working, blocked, idle, done, waiting");
    }
    Ok(McpCommand::ReportStatus {
        pane_id: target_pane_id(args, caller),
        pane: args["pane"].as_u64().and_then(|n| usize::try_from(n).ok()),
        status,
        ttl_ms: args["ttl_ms"].as_u64(),
        note: args["note"].as_str().and_then(crate::mcp_cmd::report_note),
        session_id: args["session_id"].as_str().map(String::from),
        hook_event: crate::agent::status_hook::StatusHookEvent::from_value(&args["hook_event"]),
    })
}

/// The tab a targeting call (`report_status`, the scope tools) means by
/// `pane_id`: the one it names, else this connection's own unless it named a
/// `pane` index instead. `None` leaves the loop's fallback, the focused tab.
fn target_pane_id(args: &Value, caller: &Caller) -> Option<String> {
    match args["pane_id"].as_str() {
        Some(p) => Some(p.to_string()),
        None if args["pane"].is_null() => caller.pane_id.clone(),
        None => None,
    }
}

/// [`dispatch_for`] an unattributed caller.
pub(super) fn dispatch(
    w: &mut impl Write,
    msg: &str,
    ctx_path: &Path,
    cmd_tx: Option<&std::sync::mpsc::Sender<McpRequest>>,
) -> io::Result<()> {
    dispatch_for(w, msg, ctx_path, cmd_tx, &mut Caller::default())
}

/// What a connection bound to a project that has since closed is told.
const PROJECT_CLOSED: &str = "the project this agent belongs to has closed";

/// Dispatch a JSON-RPC request and write the response to `w`.
/// `cmd_tx` is `Some` when running as the socket server
/// (writable actions available), `None` for read-only fallback. `caller` is
/// this connection's attribution, bound by its `initialize`.
pub(super) fn dispatch_for(
    w: &mut impl Write,
    msg: &str,
    ctx_path: &Path,
    cmd_tx: Option<&std::sync::mpsc::Sender<McpRequest>>,
    caller: &mut Caller,
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
            tell(tx, McpCommand::Disconnected { new_pid });
        }
        return Ok(());
    }

    let id = parsed["id"].clone();
    let method = parsed["method"].as_str().unwrap_or("");
    // A bound connection reads its own project's context file (#99), which a
    // project that has closed took with it. One bound to a pane lost its tab
    // with the project, and reads nothing rather than whatever directory
    // spyc's process cwd is, which is another project's. One bound to no tab,
    // by its cwd or to the project the user was in, outlives the close and
    // binds again as it would at `initialize`, waiting on the loop as that
    // does. One not bound yet reads the server's own file, the launch
    // project's, and once that project has closed a call that reads it binds
    // first, as an `initialize` naming nothing would. The calls that never
    // bind read nothing: a hook's one-shot `report_status` names its tab, and
    // a statusline tick reads only its own input.
    let gone = |caller: &Caller| caller.context.as_deref().is_some_and(|p| !p.exists());
    let reads_context = match method {
        "resources/read" => true,
        "tools/call" => parsed["params"]["name"] != "report_status",
        _ => false,
    };
    let default_gone = caller.context.is_none() && reads_context && !ctx_path.exists();
    if (gone(caller) || default_gone)
        && caller.pane_id.is_none()
        && let Some(tx) = cmd_tx
    {
        let cwd = caller.cwd.clone();
        let why = if default_gone { "bind" } else { "re-bind" };
        bind_to(tx, None, cwd, caller, why);
    }
    let closed = gone(caller);
    let bound = caller.context.clone();
    let ctx_path = bound.as_deref().unwrap_or(ctx_path);

    match method {
        "initialize" => handle_initialize(w, &id, &parsed["params"], cmd_tx, caller),
        "resources/list" => handle_resources_list(w, &id),
        "resources/read" if closed => send_error(w, id, -32000, PROJECT_CLOSED),
        "resources/read" => handle_resources_read(w, &id, &parsed["params"], ctx_path),
        "tools/list" => handle_tools_list(w, &id),
        "tools/call" if closed => send_tool_error(w, &id, PROJECT_CLOSED),
        "tools/call" => handle_tools_call(w, &id, &parsed["params"], ctx_path, cmd_tx, caller),
        "ping" => send_result(w, &id, json!({})),
        super::statusline::METHOD => super::statusline::handle(w, &id, &parsed["params"], cmd_tx),
        _ => send_error(w, id, -32601, &format!("Method not found: {method}")),
    }
}

// ── Protocol handlers ────────────────────────────────────────────

fn handle_initialize(
    w: &mut impl Write,
    id: &Value,
    params: &Value,
    cmd_tx: Option<&std::sync::mpsc::Sender<McpRequest>>,
    caller: &mut Caller,
) -> io::Result<()> {
    if caller.project.is_none()
        && let Some(tx) = cmd_tx
    {
        bind(tx, &params["_meta"], caller);
    }
    if !caller.initialized
        && let (Some(tx), Some(conn)) = (cmd_tx, caller.conn)
    {
        caller.initialized = true;
        tell(
            tx,
            McpCommand::ConnectionInitialized {
                conn,
                pane_id: caller.pane_id.clone(),
            },
        );
        tell_bound(tx, caller);
    }
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
            },
            "instructions": SERVER_INSTRUCTIONS
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
    send_result(w, id, super::tool_schemas::tools())
}

fn handle_tools_call(
    w: &mut impl Write,
    id: &Value,
    params: &Value,
    ctx_path: &Path,
    cmd_tx: Option<&std::sync::mpsc::Sender<McpRequest>>,
    caller: &Caller,
) -> io::Result<()> {
    let name = params["name"].as_str().unwrap_or("");
    let args = &params["arguments"];

    // Telemetry: tell the live spyc which tool was called so the `A` overlay
    // can tally per-tool usage. Fire-and-forget (dummy reply) and only when a
    // command channel exists (i.e. served by a running TUI, not the read-only
    // stdio fallback) — read tools serve on the socket thread and would
    // otherwise be invisible to the main-loop counters.
    if let Some(tx) = cmd_tx
        && !name.is_empty()
    {
        let (reply_tx, _) = std::sync::mpsc::channel();
        let _ = tx.send(McpRequest {
            command: McpCommand::ToolCalled {
                name: name.to_string(),
                conn: caller.conn,
            },
            reply: reply_tx,
            project: None,
        });
    }

    match name {
        "get_spyc_context" => get_spyc_context(w, id, ctx_path, cmd_tx, caller),
        "get_file_content" => {
            // Read-only — handled inline, no command channel needed.
            let path_str = args["path"].as_str().unwrap_or("");
            if path_str.is_empty() {
                return send_tool_error(w, id, "missing required parameter: path");
            }
            // Resolve relative paths against the effective root (the focused
            // commander's worktree root / project_home / cwd, or the agent's
            // explicit `root` override) — the same scope `search_paths` /
            // `search_content` use, so their repo-relative results can be read
            // back. (Was cwd-scoped, which broke that round-trip whenever cwd
            // differed from the search root.)
            let root = match effective_root(args, ctx_path) {
                Ok(r) => r,
                Err(e) => return send_tool_error(w, id, &e),
            };
            let resolved = if Path::new(path_str).is_absolute() {
                PathBuf::from(path_str)
            } else {
                root.join(path_str)
            };
            // A member of a mounted archive, before `canonicalize` gets a chance
            // to fail on it: the mount root is a *file*, so every path beneath it
            // is ENOTDIR on disk. Scoped to `root` like every other read — the
            // check lands on the container, which is a real file.
            //
            // The cwd fallback is where an agent reading "the file I'm looking
            // at" points a relative path, so it only applies when the agent did
            // NOT name a root. With an explicit root the agent has said which
            // worktree it means, and answering from the user's cwd instead
            // returns a different file than the one it asked for.
            let member = read_member_content(&resolved, ctx_path, &root).or_else(|| {
                if args.get("root").is_some() {
                    return None;
                }
                let alt = read_cwd_from_context(ctx_path).join(path_str);
                (!Path::new(path_str).is_absolute() && alt != resolved)
                    .then(|| read_member_content(&alt, ctx_path, &root))
                    .flatten()
            });
            if let Some(result) = member {
                return match result {
                    Ok(content) => send_tool_result(w, id, &content),
                    Err(e) => send_tool_error(w, id, &e),
                };
            }
            // Canonicalize to resolve symlinks and ".." components, then verify
            // the path is under the search root to prevent directory traversal.
            let canonical = match std::fs::canonicalize(&resolved) {
                Ok(p) => p,
                Err(e) => return send_tool_error(w, id, &format!("{}: {e}", resolved.display())),
            };
            let canonical_root = match std::fs::canonicalize(&root) {
                Ok(p) => p,
                Err(e) => return send_tool_error(w, id, &format!("root: {e}")),
            };
            if !canonical.starts_with(&canonical_root) {
                return send_tool_error(w, id, "path is outside the project root");
            }
            match read_file_content(&canonical) {
                Ok(content) => send_tool_result(w, id, &content),
                Err(e) => send_tool_error(w, id, &e),
            }
        }
        "search_paths" => {
            let query = args["query"].as_str().unwrap_or("").to_string();
            let limit = args["limit"].as_u64().map_or(100, |n| n.min(1000) as usize);
            let root = match effective_root(args, ctx_path) {
                Ok(r) => r,
                Err(e) => return send_tool_error(w, id, &e),
            };
            if let Some(e) = mount_refusal(&root, ctx_path, "search_paths") {
                return send_tool_error(w, id, &e);
            }
            match call_with_timeout(READ_TOOL_TIMEOUT, move || {
                crate::fs::finder::find_paths(&root, &query, limit)
            }) {
                Ok(paths) => {
                    let arr: Vec<Value> = paths
                        .iter()
                        .map(|p| Value::String(p.to_string_lossy().into_owned()))
                        .collect();
                    send_tool_result(w, id, &Value::Array(arr).to_string())
                }
                Err(msg) => send_tool_error(w, id, &format!("search_paths timed out: {msg}")),
            }
        }
        "search_content" => {
            let pattern = args["pattern"].as_str().unwrap_or("");
            if pattern.is_empty() {
                return send_tool_error(w, id, "missing required parameter: pattern");
            }
            let pattern = pattern.to_string();
            let limit = args["limit"].as_u64().map_or(200, |n| n.min(5000) as usize);
            let root = match effective_root(args, ctx_path) {
                Ok(r) => r,
                Err(e) => return send_tool_error(w, id, &e),
            };
            if let Some(e) = mount_refusal(&root, ctx_path, "search_content") {
                return send_tool_error(w, id, &e);
            }
            match call_with_timeout(READ_TOOL_TIMEOUT, move || {
                crate::fs::grep::search_to_vec(&root, &pattern, limit)
            }) {
                Ok(Ok(hits)) => send_tool_result(w, id, &grep_matches_to_json(&hits).to_string()),
                Ok(Err(e)) => send_tool_error(w, id, &e),
                Err(msg) => send_tool_error(w, id, &format!("search_content timed out: {msg}")),
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
        "list_worktrees" => {
            let (ctx, agent_cwd) = (ctx_path.to_path_buf(), caller.cwd.clone());
            let list = move || list_worktrees_json(&ctx, agent_cwd.as_deref());
            match call_with_timeout(READ_TOOL_TIMEOUT, list) {
                Ok(text) => send_tool_result(w, id, &text),
                Err(msg) => send_tool_error(w, id, &format!("list_worktrees timed out: {msg}")),
            }
        }
        "git_status" => {
            let root = match effective_root(args, ctx_path) {
                Ok(r) => r,
                Err(e) => return send_tool_error(w, id, &e),
            };
            match call_with_timeout(READ_TOOL_TIMEOUT, move || git_status_json(&root)) {
                Ok(text) => send_tool_result(w, id, &text),
                Err(msg) => send_tool_error(w, id, &format!("git_status timed out: {msg}")),
            }
        }
        "git_log" => {
            let limit = args["limit"].as_u64().map_or(20, |n| n.min(500) as usize);
            let root = match effective_root(args, ctx_path) {
                Ok(r) => r,
                Err(e) => return send_tool_error(w, id, &e),
            };
            match call_with_timeout(READ_TOOL_TIMEOUT, move || git_log_json(&root, limit)) {
                Ok(text) => send_tool_result(w, id, &text),
                Err(msg) => send_tool_error(w, id, &format!("git_log timed out: {msg}")),
            }
        }
        "git_diff" => {
            let root = match effective_root(args, ctx_path) {
                Ok(r) => r,
                Err(e) => return send_tool_error(w, id, &e),
            };
            // `unstaged` (index↔worktree) wins over `cached` (index↔HEAD); with
            // neither set it's the working tree vs HEAD.
            let mode = if args["unstaged"].as_bool().unwrap_or(false) {
                DiffMode::Unstaged
            } else if args["cached"].as_bool().unwrap_or(false) {
                DiffMode::Cached
            } else {
                DiffMode::HeadToWorktree
            };
            let paths: Vec<String> = args["paths"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            match call_with_timeout(READ_TOOL_TIMEOUT, move || {
                git_diff_text(&root, mode, &paths)
            }) {
                Ok(text) => send_tool_result(w, id, &text),
                Err(msg) => send_tool_error(w, id, &format!("git_diff timed out: {msg}")),
            }
        }
        "claim_worktree" => {
            let path = args["path"].as_str().unwrap_or("");
            if path.trim().is_empty() {
                return send_tool_error(w, id, "missing required parameter: path");
            }
            let reason = args["reason"].as_str().unwrap_or("");
            match claim_worktree_result(ctx_path, path, reason) {
                Ok(msg) => send_tool_result(w, id, &msg),
                Err(e) => send_tool_error(w, id, &e),
            }
        }
        "release_worktree" => {
            let path = args["path"].as_str().unwrap_or("");
            if path.trim().is_empty() {
                return send_tool_error(w, id, "missing required parameter: path");
            }
            match release_worktree_result(ctx_path, path) {
                Ok(msg) => send_tool_result(w, id, &msg),
                Err(e) => send_tool_error(w, id, &e),
            }
        }
        "navigate_to"
        | "set_filter"
        | "pick_files"
        | "clear_picks"
        | "create_worktree"
        | "remove_worktree"
        | "clean_worktree"
        | "open_worktree"
        | "report_status"
        | "register_scope"
        | "list_scopes"
        | "release_scope"
        | "wait_for_scope_clear" => {
            let Some(tx) = cmd_tx else {
                return send_tool_error(w, id, "writable actions not available in stdio mode");
            };
            let command = match name {
                "report_status" => match report_status_command(args, caller) {
                    Ok(cmd) => cmd,
                    Err(message) => return send_tool_error(w, id, message),
                },
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
                "create_worktree" => match create_worktree_command(args, caller) {
                    Ok(cmd) => cmd,
                    Err(message) => return send_tool_error(w, id, message),
                },
                "remove_worktree" => {
                    let path = args["path"].as_str().unwrap_or("").to_string();
                    if path.trim().is_empty() {
                        return send_tool_error(w, id, "missing required parameter: path");
                    }
                    McpCommand::RemoveWorktree { path }
                }
                "clean_worktree" => {
                    let path = args["path"].as_str().unwrap_or("").to_string();
                    if path.trim().is_empty() {
                        return send_tool_error(w, id, "missing required parameter: path");
                    }
                    McpCommand::CleanWorktree { path }
                }
                "open_worktree" => {
                    let path = args["path"].as_str().unwrap_or("").to_string();
                    if path.trim().is_empty() {
                        return send_tool_error(w, id, "missing required parameter: path");
                    }
                    McpCommand::OpenWorktree { path }
                }
                "register_scope" => {
                    let paths: Vec<String> = args["paths"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default();
                    if paths.is_empty() {
                        return send_tool_error(w, id, "missing required parameter: paths");
                    }
                    let intent = args["intent"].as_str().unwrap_or("").to_string();
                    if !matches!(intent.as_str(), "editing" | "merging") {
                        return send_tool_error(w, id, "intent must be 'editing' or 'merging'");
                    }
                    let pane_id = target_pane_id(args, caller);
                    let pane = args["pane"].as_u64().and_then(|n| usize::try_from(n).ok());
                    let pr = args["pr"].as_str().map(String::from);
                    let note = args["note"].as_str().map(String::from);
                    McpCommand::RegisterScope {
                        pane_id,
                        pane,
                        paths,
                        intent,
                        pr,
                        note,
                    }
                }
                "list_scopes" => McpCommand::ListScopes,
                "release_scope" => {
                    let Some(claim_id) = args["id"].as_u64() else {
                        return send_tool_error(w, id, "missing required parameter: id (integer)");
                    };
                    McpCommand::ReleaseScope { id: claim_id }
                }
                "wait_for_scope_clear" => {
                    let paths: Vec<String> = args["paths"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default();
                    if paths.is_empty() {
                        return send_tool_error(w, id, "missing required parameter: paths");
                    }
                    let pane_id = target_pane_id(args, caller);
                    let pane = args["pane"].as_u64().and_then(|n| usize::try_from(n).ok());
                    let timeout_ms = args["timeout_ms"]
                        .as_u64()
                        .unwrap_or(DEFAULT_SCOPE_WAIT_MS)
                        .min(MAX_SCOPE_WAIT_MS);
                    McpCommand::WaitForScopeClear {
                        pane_id,
                        pane,
                        paths,
                        timeout_ms,
                    }
                }
                _ => unreachable!(),
            };

            // Send command and block for reply with timeout.
            let (reply_tx, reply_rx) = std::sync::mpsc::channel();
            if tx
                .send(McpRequest {
                    command,
                    reply: reply_tx,
                    project: caller.project,
                })
                .is_err()
            {
                return send_tool_error(w, id, "spyc is not running");
            }
            // Worktree mutations can run a status walk + tar.zst archive off
            // the main loop (§5 of docs/archive/WORKTREE_MCP_PLAN.md); a large tree easily
            // outlasts the interactive 5s window, so give them a generous
            // ceiling. Everything else is a fast in-memory model edit. (Stage 0
            // of the async/Tasks plan — §5.1; Stage 1 returns a task handle
            // instead of blocking.)
            let reply_timeout = match name {
                "create_worktree" | "remove_worktree" | "clean_worktree" => {
                    std::time::Duration::from_secs(60)
                }
                // The loop parks this and replies (cleared/timed_out) by
                // `timeout_ms`; wait a hair longer so the socket read always
                // receives that reply instead of giving up first.
                "wait_for_scope_clear" => {
                    let ms = args["timeout_ms"]
                        .as_u64()
                        .unwrap_or(DEFAULT_SCOPE_WAIT_MS)
                        .min(MAX_SCOPE_WAIT_MS);
                    std::time::Duration::from_millis(ms) + std::time::Duration::from_secs(2)
                }
                _ => std::time::Duration::from_secs(5),
            };
            match reply_rx.recv_timeout(reply_timeout) {
                Ok(McpResponse::Ok { message }) => send_tool_result(w, id, &message),
                Ok(McpResponse::Error { message }) => send_tool_error(w, id, &message),
                Err(_) => send_tool_error(
                    w,
                    id,
                    &format!("spyc did not respond within {}s", reply_timeout.as_secs()),
                ),
            }
        }
        _ => send_tool_error(w, id, &format!("unknown tool: {name}")),
    }
}

/// `get_spyc_context`: the caller's project's context file, plus what the loop
/// says about the caller (`McpCommand::CallerContext`). A connection bound to
/// no project gets the file alone, as does one the loop doesn't answer.
fn get_spyc_context(
    w: &mut impl Write,
    id: &Value,
    ctx_path: &Path,
    cmd_tx: Option<&std::sync::mpsc::Sender<McpRequest>>,
    caller: &Caller,
) -> io::Result<()> {
    let text = read_context_or_empty(ctx_path);
    let about = cmd_tx.zip(caller.project).and_then(|(tx, project)| {
        let pane_id = caller.pane_id.clone();
        ask(tx, McpCommand::CallerContext { pane_id, project })
    });
    let text = match about {
        Some(about) => with_caller(text, about),
        None => text,
    };
    send_tool_result(w, id, &text)
}

/// The context file's JSON with the caller's `project` and own tab (`pane`)
/// beside the fields that describe what the user is looking at.
fn with_caller(context: String, about: Value) -> String {
    match (serde_json::from_str::<Value>(&context), about) {
        (Ok(Value::Object(mut map)), Value::Object(about)) => {
            map.extend(about);
            Value::Object(map).to_string()
        }
        _ => context,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn call_with_timeout_returns_value_when_work_completes_in_time() {
        let got = call_with_timeout(std::time::Duration::from_secs(5), || 6 * 7);
        assert_eq!(got, Ok(42));
    }

    #[test]
    fn call_with_timeout_errs_when_work_outlasts_the_deadline() {
        // The slow closure outlives a tiny deadline, so the caller gets a clean
        // Err rather than blocking — the property that keeps a slow tool call
        // from stalling past the proxy's socket timeout and dropping the
        // connection. (The detached thread finishes its sleep harmlessly.)
        let got = call_with_timeout(std::time::Duration::from_millis(20), || {
            std::thread::sleep(std::time::Duration::from_secs(2));
            1
        });
        assert_eq!(got, Err("timed out".to_string()));
    }

    /// The read-tool deadline must sit strictly below the proxy's socket
    /// timeout, or a slow call races the proxy and the connection dies anyway.
    #[test]
    fn read_tool_timeout_stays_below_proxy_timeout() {
        assert!(READ_TOOL_TIMEOUT < PROXY_IO_TIMEOUT);
    }
}
