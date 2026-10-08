//! MCP JSON-RPC dispatch, request handlers, and framing helpers.
//! Split out of mcp.rs verbatim during the 800-LoC decomposition.
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::mcp_cmd::{McpCommand, McpRequest, McpResponse};

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

/// Who is on the other end of one connection: the pane its `initialize`
/// named, once the main loop has confirmed that tab is live. Bound once, for
/// the connection's lifetime, and never taken from a tool call. `None` is an
/// unattributed caller (an older proxy, the status hook, the read-only
/// fallback), which behaves as every caller did before attribution existed.
#[derive(Debug, Default)]
pub(super) struct Caller {
    /// This socket connection's number; `None` in the read-only fallback.
    conn: Option<u64>,
    pane_id: Option<String>,
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
    });
}

/// The live tab `pane_id` names, as the main loop describes it; `None` once
/// that tab is gone (or the loop doesn't answer).
fn pane_context(tx: &std::sync::mpsc::Sender<McpRequest>, pane_id: &str) -> Option<Value> {
    let (reply_tx, reply_rx) = std::sync::mpsc::channel();
    tx.send(McpRequest {
        command: McpCommand::PaneContext {
            pane_id: pane_id.to_string(),
        },
        reply: reply_tx,
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
        "initialize" => handle_initialize(w, &id, &parsed["params"], cmd_tx, caller),
        "resources/list" => handle_resources_list(w, &id),
        "resources/read" => handle_resources_read(w, &id, &parsed["params"], ctx_path),
        "tools/list" => handle_tools_list(w, &id),
        "tools/call" => handle_tools_call(w, &id, &parsed["params"], ctx_path, cmd_tx, caller),
        "ping" => send_result(w, &id, json!({})),
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
    if caller.pane_id.is_none()
        && let Some(tx) = cmd_tx
        && let Some(pane_id) = params["_meta"][PANE_ID_META]
            .as_str()
            .filter(|p| !p.is_empty())
    {
        if pane_context(tx, pane_id).is_some() {
            mcp_log(&format!("initialize: connection bound to pane {pane_id}"));
            caller.pane_id = Some(pane_id.to_string());
        } else {
            mcp_log(&format!("initialize: no live pane {pane_id}; unattributed"));
        }
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
        });
    }

    match name {
        "get_spyc_context" => {
            let text = read_context_or_empty(ctx_path);
            let own = caller.pane_id.as_deref().zip(cmd_tx);
            let text = match own.and_then(|(pane_id, tx)| pane_context(tx, pane_id)) {
                Some(pane) => with_pane(text, pane),
                None => text,
            };
            send_tool_result(w, id, &text)
        }
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
            let ctx = ctx_path.to_path_buf();
            match call_with_timeout(READ_TOOL_TIMEOUT, move || list_worktrees_json(&ctx)) {
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
                "report_status" => {
                    let status = args["status"].as_str().unwrap_or("").to_string();
                    if !matches!(
                        status.as_str(),
                        "working"
                            | "blocked"
                            | "idle"
                            | "done"
                            | crate::agent::codex_recovery::QUESTION_START
                            | crate::agent::codex_recovery::QUESTION_END
                    ) {
                        return send_tool_error(
                            w,
                            id,
                            "status must be one of: working, blocked, idle, done",
                        );
                    }
                    let pane_id = target_pane_id(args, caller);
                    let pane = args["pane"].as_u64().and_then(|n| usize::try_from(n).ok());
                    let ttl_ms = args["ttl_ms"].as_u64();
                    // Piggybacked by the status-hook reporter (Claude's hook
                    // stdin carries `session_id`); absent on a direct agent call.
                    let session_id = args["session_id"].as_str().map(String::from);
                    let hook_event =
                        crate::agent::status_hook::StatusHookEvent::from_value(&args["hook_event"]);
                    McpCommand::ReportStatus {
                        pane_id,
                        pane,
                        status,
                        ttl_ms,
                        session_id,
                        hook_event,
                    }
                }
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
                "create_worktree" => {
                    let branch = args["branch"].as_str().unwrap_or("").to_string();
                    if branch.trim().is_empty() {
                        return send_tool_error(w, id, "missing required parameter: branch");
                    }
                    let base = args["base"]
                        .as_str()
                        .filter(|s| !s.trim().is_empty())
                        .map(String::from);
                    let open = args["open"].as_bool().unwrap_or(false);
                    McpCommand::CreateWorktree { branch, base, open }
                }
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

/// The context file's JSON with the caller's own tab under `pane`, beside the
/// fields that describe what the user is looking at.
fn with_pane(context: String, pane: Value) -> String {
    match serde_json::from_str::<Value>(&context) {
        Ok(Value::Object(mut map)) => {
            map.insert("pane".into(), pane);
            Value::Object(map).to_string()
        }
        _ => context,
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

/// Upper bound on a single MCP message body. The header's `Content-Length`
/// is untrusted; we refuse anything larger rather than pre-allocate it.
const MAX_LSP_MESSAGE_BYTES: usize = 64 * 1024 * 1024;

pub(super) fn read_lsp_message(reader: &mut impl BufRead) -> io::Result<String> {
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
        // A "header" line that's actually a JSON body (and we haven't seen a
        // Content-Length yet) means the sender didn't frame the message. Flag
        // it as malformed rather than consuming lines until EOF and dropping it
        // silently — that silent drop is exactly what hid the bare-newline
        // report-status reporter. (Valid frames only reach here with header
        // lines + a blank terminator; the JSON body is read by byte count.)
        if content_length.is_none() && (trimmed.starts_with('{') || trimmed.starts_with('[')) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unframed message: JSON body with no Content-Length header",
            ));
        }
        if let Some(val) = trimmed.strip_prefix("Content-Length:") {
            content_length = val.trim().parse().ok();
        }
    }

    let len = content_length
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing Content-Length"))?;
    // Cap the declared length before allocating: `len` is attacker/garbage
    // controlled, so `vec![0u8; len]` for a multi-GB Content-Length would
    // abort the whole process on allocation failure. 64 MiB is far above any
    // real MCP message (tool args / file slices) but small enough to refuse.
    if len > MAX_LSP_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Content-Length {len} exceeds {MAX_LSP_MESSAGE_BYTES}-byte cap"),
        ));
    }

    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf)?;
    String::from_utf8(buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

pub(super) fn send_message(w: &mut impl Write, body: &str) -> io::Result<()> {
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

    #[test]
    fn read_lsp_message_reads_framed_body() {
        let mut c = Cursor::new(b"Content-Length: 5\r\n\r\nhello".to_vec());
        assert_eq!(read_lsp_message(&mut c).unwrap(), "hello");
    }

    // Regression: the `--report-status` hook reporter wrote a BARE
    // newline-delimited JSON line, which the socket server (Content-Length
    // framed) silently dropped — so hook-driven status never reached spyc.
    // The reporter must frame via `send_message` so `read_lsp_message` reads it
    // back; a bare line must NOT parse (that was the bug).
    #[test]
    fn report_status_framing_round_trips_but_a_bare_line_does_not() {
        let body = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"report_status","arguments":{"status":"blocked"}}}"#;
        let mut framed = Vec::new();
        send_message(&mut framed, body).unwrap();
        assert_eq!(
            read_lsp_message(&mut Cursor::new(framed)).unwrap(),
            body,
            "send_message framing must round-trip through the socket reader"
        );
        // The old reporter's output: bare JSON + '\n', no Content-Length header.
        let mut bare = Cursor::new(format!("{body}\n").into_bytes());
        let err = read_lsp_message(&mut bare).unwrap_err();
        // Reported as InvalidData ("unframed"), NOT a silent EOF — so the socket
        // server can warn the user instead of dropping it unnoticed.
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("unframed"), "got {err}");
    }

    #[test]
    fn read_lsp_message_rejects_oversized_content_length() {
        // A hostile/garbage header must not trigger a multi-GB allocation;
        // it errors before `vec![0u8; len]`.
        let header = format!("Content-Length: {}\r\n\r\n", u64::from(u32::MAX) * 16);
        let mut c = Cursor::new(header.into_bytes());
        let err = read_lsp_message(&mut c).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("exceeds"));
    }

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
