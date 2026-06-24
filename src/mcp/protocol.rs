//! MCP JSON-RPC dispatch, request handlers, and framing helpers.
//! Split out of mcp.rs verbatim during the 800-LoC decomposition.
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::mcp_cmd::{McpCommand, McpRequest, McpResponse};

use super::readers::{
    grep_matches_to_json, list_worktrees_json, read_context_or_empty, read_file_content,
    read_inventory_from_context, read_picks_from_context, search_root,
};
use super::{CONTEXT_URI, PROTOCOL_VERSION, SERVER_INSTRUCTIONS, SERVER_NAME, SERVER_VERSION};
pub(super) fn dispatch(
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
    send_result(
        w,
        id,
        json!({
            "tools": [
                {
                    "name": "get_spyc_context",
                    "description": "Get the current spyc file manager state: working directory, cursor position, picked files, inventory, active filter, git branch, project_home (sticky project root), session_name, plus the running spyc's pid and version ('<x.y.z> (<git-sha>)'). Use this to understand what the user is looking at — and to detect a stale server: if a tool you expect is missing, compare version's git SHA against the repo HEAD and ask the user to restart spyc (pid identifies the process).",
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
                    "name": "create_worktree",
                    "description": "Create a git worktree off the focused commander's repo for the given branch (uses an existing branch, else creates it at HEAD). The worktree lands in a sibling `<repo>.worktrees/<branch>/` dir. Returns {branch, path} — point a second column or navigate_to there to work in it while the main column stays on its branch. Errors if the focused commander isn't in a repo or the branch is already checked out elsewhere.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "branch": {
                                "type": "string",
                                "description": "Branch to check out in the new worktree. Existing branch is reused; otherwise created at the current HEAD."
                            }
                        },
                        "required": ["branch"]
                    }
                },
                {
                    "name": "remove_worktree",
                    "description": "Tear down a git worktree by path (the path create_worktree returned). Refuses a worktree with uncommitted/untracked changes, a locked one, or one a spyc column is currently open in; the branch ref is left intact. The teardown half of the worktree flow.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "Path of the worktree to remove (as returned by create_worktree)."
                            }
                        },
                        "required": ["path"]
                    }
                },
                {
                    "name": "clean_worktree",
                    "description": "Clean out and remove a worktree by path: archive its UNTRACKED files into spyc's graveyard (recoverable, under '<worktree>-<timestamp>'), then remove it. Unlike remove_worktree this doesn't choke on untracked junk — it preserves it. Still refuses if a column is open in it or there are uncommitted changes to TRACKED files (commit/stash those first; only untracked files are preserved). The branch ref is left intact.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "Path of the worktree to clean out and remove (as returned by create_worktree)."
                            }
                        },
                        "required": ["path"]
                    }
                },
                {
                    "name": "open_worktree",
                    "description": "Open the second spyc column (column 'b') at the given worktree path (as returned by create_worktree) — so you can work in the worktree while the main column stays where the user left it. Re-targets column b if it's already open. After this, navigate_to / search / pick_files act on column b.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "Path of the worktree (or any directory) to open in column b."
                            }
                        },
                        "required": ["path"]
                    }
                },
                {
                    "name": "get_file_content",
                    "description": "Read the text contents of a file (up to 100KB). Binary files are rejected. Relative paths resolved against the project root (the focused commander's worktree root, else PROJECT_HOME, else cwd) — the same scope as search_paths/search_content, so their results can be read back.",
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
                    "description": "Project-wide fuzzy filename search. Walks the focused commander's worktree root (its repo root, else PROJECT_HOME, else cwd) honoring .gitignore, scores candidates against the query with fzf-style ranking (basename hits beat parent-dir hits). Returns a JSON array of repo-relative paths, best match first. Empty query returns paths in walk order, truncated.",
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
                    "description": "Project-wide content search using ripgrep's matcher (gitignore-aware, smart-case, binary files skipped). Walks the focused commander's worktree root (its repo root, else PROJECT_HOME, else cwd). Returns a JSON array of {path, line, col, text} match objects.",
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
                },
                {
                    "name": "list_worktrees",
                    "description": "List the git worktrees of the focused column's repo — the orient/inspect entry point for worktree cleanup. Returns a JSON array, one object per worktree: {path, branch, head, is_current, dirty:{staged,unstaged,untracked}}. Consult it before remove_worktree (which tree is dirty, which is the current one).",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
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
            },
            reply: reply_tx,
        });
    }

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
            // Resolve relative paths against the SEARCH ROOT (the focused
            // commander's worktree root / project_home / cwd) — the same scope
            // `search_paths` / `search_content` use, so their repo-relative
            // results can be read back. (Was cwd-scoped, which broke that
            // round-trip whenever cwd differed from the search root.)
            let root = search_root(ctx_path);
            let resolved = if Path::new(path_str).is_absolute() {
                PathBuf::from(path_str)
            } else {
                root.join(path_str)
            };
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
        "list_worktrees" => send_tool_result(w, id, &list_worktrees_json(ctx_path)),
        "navigate_to" | "set_filter" | "pick_files" | "clear_picks" | "create_worktree"
        | "remove_worktree" | "clean_worktree" | "open_worktree" => {
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
                "create_worktree" => {
                    let branch = args["branch"].as_str().unwrap_or("").to_string();
                    if branch.trim().is_empty() {
                        return send_tool_error(w, id, "missing required parameter: branch");
                    }
                    McpCommand::CreateWorktree { branch }
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
            // the main loop (§5 of WORKTREE_MCP_PLAN.md); a large tree easily
            // outlasts the interactive 5s window, so give them a generous
            // ceiling. Everything else is a fast in-memory model edit. (Stage 0
            // of the async/Tasks plan — §5.1; Stage 1 returns a task handle
            // instead of blocking.)
            let reply_timeout = match name {
                "create_worktree" | "remove_worktree" | "clean_worktree" => {
                    std::time::Duration::from_secs(60)
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

/// Pick the search root: prefer `search_root` from the context file (the
/// focused commander's worktree root), then `project_home`, then `cwd`.
/// Used by `search_paths` and `search_content` so the MCP tools scope
/// themselves to the same worktree the in-TUI `F` and `:grep` commands do.
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
}
