//! MCP JSON-RPC dispatch, request handlers, and framing helpers.
//! Split out of mcp.rs verbatim during the 800-LoC decomposition.
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::mcp_cmd::{McpCommand, McpRequest, McpResponse};

use super::readers::{
    DiffMode, claim_worktree_result, effective_root, git_diff_text, git_log_json, git_status_json,
    grep_matches_to_json, list_worktrees_json, read_context_or_empty, read_file_content,
    read_inventory_from_context, read_picks_from_context, release_worktree_result,
};
use super::{
    CONTEXT_URI, PROTOCOL_VERSION, PROXY_IO_TIMEOUT, SERVER_INSTRUCTIONS, SERVER_NAME,
    SERVER_VERSION,
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

/// Dispatch a JSON-RPC request and write the response to `w`.
/// `cmd_tx` is `Some` when running as the socket server
/// (writable actions available), `None` for read-only fallback.
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
                    "name": "report_status",
                    "description": "Report YOUR current activity so spyc shows it as a live dot on your pane tab — the 'which agent needs me' signal. Call it as your turn changes: 'working' when you start a non-trivial task, 'blocked' when you stop to ask the user a question or for permission (this is the one that earns attention), 'done' when you finish, 'idle' when waiting with nothing pending. Overrides spyc's output-timing guess and keeps your dot accurate through silent thinking. Targets your own (focused) tab by default; pass `pane` for a specific tab. Cheap and idempotent — call it freely.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "status": {
                                "type": "string",
                                "enum": ["working", "blocked", "idle", "done"],
                                "description": "working = actively doing a task; blocked = waiting on the user (needs attention); done = finished a turn; idle = nothing pending."
                            },
                            "pane_id": {
                                "type": "string",
                                "description": "Optional stable pane id (the `SPYC_PANE_ID` env var spyc set for your pane). The auto-hook passes this; you normally don't need it."
                            },
                            "pane": {
                                "type": "integer",
                                "description": "Optional 1-based tab number (the `[N]` in the divider) to report for. Defaults to the focused tab — normally omit it."
                            },
                            "ttl_ms": {
                                "type": "integer",
                                "description": "Optional backstop in ms after which the report expires and the dot falls back to output timing. Defaults to a few minutes; rarely needed."
                            }
                        },
                        "required": ["status"]
                    }
                },
                {
                    "name": "register_scope",
                    "description": "Declare the files/globs YOU are about to touch and whether you're `editing` or about to be `merging` — the merge-coordination registry. Another agent can `list_scopes` to see your claim and `wait_for_scope_clear` before merging overlapping files, so concurrent agents queue instead of colliding. Call it before a merge with intent='merging' and your PR's file set; `release_scope` when done. Returns {claim_id, conflicting_merges:[...]} — a non-empty conflicting_merges means someone else is mid-merge on your files. Advisory: spyc never blocks a merge.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "paths": {"type": "array", "items": {"type": "string"}, "description": "File paths or globs (glob::Pattern syntax, e.g. 'src/app/*.rs') you're touching."},
                            "intent": {"type": "string", "enum": ["editing", "merging"], "description": "editing = informational; merging = blocks another agent's wait_for_scope_clear on overlapping paths."},
                            "pr": {"type": "string", "description": "Optional PR identifier this claim is for (e.g. '#661')."},
                            "note": {"type": "string", "description": "Optional free-text note shown in list_scopes / the orchestration screen."},
                            "pane_id": {"type": "string", "description": "Optional stable pane id (SPYC_PANE_ID); defaults to your focused tab."},
                            "pane": {"type": "integer", "description": "Optional 1-based tab number; defaults to the focused tab."}
                        },
                        "required": ["paths", "intent"]
                    }
                },
                {
                    "name": "list_scopes",
                    "description": "List all active scope claims in this spyc — each {id, owner_label, paths, intent, pr, note, claimed_at_secs}. Check it before you merge to see who else is touching your files and whether anyone is mid-merge (intent='merging'). Also what the orchestration screen renders.",
                    "inputSchema": {"type": "object", "properties": {}, "required": []}
                },
                {
                    "name": "release_scope",
                    "description": "Release a scope claim by its `id` (from register_scope / list_scopes) once you're done with those files. No-op if the id doesn't match a live claim. No ownership check — a lead agent or the user may clear a stale claim on someone's behalf.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {"id": {"type": "integer", "description": "The claim id to release."}},
                        "required": ["id"]
                    }
                },
                {
                    "name": "wait_for_scope_clear",
                    "description": "Block until no OTHER agent's `merging` scope claim overlaps `paths` (or `timeout_ms` elapses) — the coordination verb for the merge train. Register your merge (register_scope intent='merging'), then wait_for_scope_clear on the same paths: you resume once whoever's mid-merge on overlapping files releases, so concurrent agents serialize instead of colliding + rebasing. Returns {outcome: 'cleared'|'timed_out', conflicts:[...]}. Your OWN claims never block you. Always bounded by a timeout (default 5m, hard cap 10m).",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "paths": {"type": "array", "items": {"type": "string"}, "description": "File paths/globs to wait on (usually the same set you register_scope'd)."},
                            "timeout_ms": {"type": "integer", "description": "Max wait in ms (default 300000, capped 600000). Returns outcome='timed_out' if it elapses."},
                            "pane_id": {"type": "string", "description": "Optional stable pane id (SPYC_PANE_ID); defaults to your focused tab."},
                            "pane": {"type": "integer", "description": "Optional 1-based tab number; defaults to the focused tab."}
                        },
                        "required": ["paths"]
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
                    "description": "Create a git worktree for the given branch (existing branch reused, else a NEW branch created off the repo's default/integration branch — pass `base` to override that start point). It lands in a sibling `<repo>.worktrees/<branch>/` dir, anchored on the MAIN repo even when called from inside a linked worktree. Returns {branch, path}. Pass `open:true` to also open it in column b and work there right away (otherwise navigate_to / open_worktree later). Errors if not in a repo or the branch is already checked out elsewhere.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "branch": {
                                "type": "string",
                                "description": "Branch to check out in the new worktree. Existing branch is reused; otherwise created off `base` (or the repo's default branch)."
                            },
                            "base": {
                                "type": "string",
                                "description": "Start point (branch/rev) for a NEW branch. Optional — defaults to the repo's default branch. Ignored when `branch` already exists."
                            },
                            "open": {
                                "type": "boolean",
                                "description": "If true, also open the new worktree in column b (and focus it) so you can work in it immediately. Default false."
                            }
                        },
                        "required": ["branch"]
                    }
                },
                {
                    "name": "remove_worktree",
                    "description": "Safely tear down a git worktree by path (the path create_worktree returned). Safe by default: archives any untracked + uncommitted changes to spyc's graveyard first (recoverable), removes the worktree, then deletes its branch ONLY if it is merged into the integration base — an unmerged branch's ref is kept (it's the commit backup). Refuses a worktree CLAIMED by another session (claim_worktree) — release it first. A spyc column sitting inside is reset to PROJECT_HOME, not refused. The teardown half of the worktree flow.",
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
                    "description": "Alias of remove_worktree (kept for familiarity) — identical safe-by-default teardown: archives untracked + uncommitted changes to the graveyard under '<worktree>-<timestamp>', removes the worktree, and deletes the branch iff merged. Prefer remove_worktree.",
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
                    "description": "Read the text contents of a file (up to 100KB). Binary files are rejected. Relative paths resolved against the project root (the focused commander's worktree root, else PROJECT_HOME, else cwd) — the same scope as search_paths/search_content, so their results can be read back. Pass `root` to resolve against a different worktree you're working in.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "Absolute or relative path to the file."
                            },
                            "root": {
                                "type": "string",
                                "description": "Optional absolute path to resolve relative paths against instead of the user's focused column — e.g. a sibling worktree you're working in (a path from create_worktree/list_worktrees). Defaults to the focused column's worktree root."
                            }
                        },
                        "required": ["path"]
                    }
                },
                {
                    "name": "search_paths",
                    "description": "Project-wide fuzzy filename search. Walks the focused commander's worktree root (its repo root, else PROJECT_HOME, else cwd) honoring .gitignore, scores candidates against the query with fzf-style ranking (basename hits beat parent-dir hits). Returns a JSON array of repo-relative paths, best match first. Empty query returns paths in walk order, truncated. Pass `root` to walk a different worktree you're working in.",
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
                            },
                            "root": {
                                "type": "string",
                                "description": "Optional absolute path to walk instead of the user's focused column — e.g. a sibling worktree you're working in (a path from create_worktree/list_worktrees). Defaults to the focused column's worktree root."
                            }
                        },
                        "required": ["query"]
                    }
                },
                {
                    "name": "search_content",
                    "description": "Project-wide content search using ripgrep's matcher (gitignore-aware, smart-case, binary files skipped). Walks the focused commander's worktree root (its repo root, else PROJECT_HOME, else cwd). Returns a JSON array of {path, line, col, text} match objects. Pass `root` to search a different worktree you're working in.",
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
                            },
                            "root": {
                                "type": "string",
                                "description": "Optional absolute path to search instead of the user's focused column — e.g. a sibling worktree you're working in (a path from create_worktree/list_worktrees). Defaults to the focused column's worktree root."
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
                    "description": "List the git worktrees of the focused column's repo — the orient/inspect entry point for worktree cleanup. Returns a JSON array, one object per worktree: {path, branch, head, is_current, dirty:{staged,unstaged,untracked}, ahead, behind, merged, locked, lock_reason}. ahead/behind/merged are relative to the repo's integration base (null when unresolvable) — `merged:true` means removing that worktree/branch loses no unmerged commits. `locked:true` (with `lock_reason`) means another session has claimed it via claim_worktree and remove/clean will refuse. Consult it before remove_worktree (which tree is dirty, which is merged and safe to drop, which is claimed by someone else, which is the current one).",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
                    }
                },
                {
                    "name": "claim_worktree",
                    "description": "Claim a worktree for your exclusive use — a cooperative lease so another spyc session (e.g. a second agent) won't tear it down underneath you. Sets git's native worktree lock with your `reason`, so remove_worktree/clean_worktree (here and via plain git) refuse it until released. Claim the worktree you're working in before you start editing; release_worktree when done. Locking the MAIN worktree is not possible (mirrors git).",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string", "description": "Path of the worktree to claim (as returned by create_worktree); relative paths resolve against the focused column's cwd." },
                            "reason": { "type": "string", "description": "Human-readable owner/reason recorded on the lease and shown to other sessions (e.g. 'agent A: refactoring auth'). Optional." }
                        },
                        "required": ["path"]
                    }
                },
                {
                    "name": "release_worktree",
                    "description": "Release a claim_worktree lease (clear the lock), so the worktree can be removed/cleaned again. Call it when you're done working in a worktree you claimed. No-op if it wasn't locked.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string", "description": "Path of the worktree to release; relative paths resolve against the focused column's cwd." }
                        },
                        "required": ["path"]
                    }
                },
                {
                    "name": "git_status",
                    "description": "Working-tree status of the focused column's worktree, gitignore-aware and in-process (don't shell out to `git status`). Returns a JSON array, one object per changed path: {path, staged, unstaged, untracked} — `staged`/`unstaged` are the change kind ('modified'|'added'|'deleted'|'renamed'|'conflicted') or null. Empty array when the tree is clean. Pass `root` to inspect a different worktree you're working in.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "root": {
                                "type": "string",
                                "description": "Optional absolute path of the worktree to inspect instead of the user's focused column — e.g. a sibling worktree you're working in (a path from create_worktree/list_worktrees). Defaults to the focused column's worktree root."
                            }
                        }
                    }
                },
                {
                    "name": "git_log",
                    "description": "Recent commit history of the focused column's worktree (HEAD, newest first), in-process. Returns a JSON array: {short_id, author, time, subject} per commit. Use it to orient on what's landed without shelling out to `git log`. Pass `root` for a different worktree you're working in.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "limit": { "type": "integer", "description": "Max commits to return (default 20, capped at 500)." },
                            "root": {
                                "type": "string",
                                "description": "Optional absolute path of the worktree whose history to read instead of the user's focused column — e.g. a sibling worktree you're working in. Defaults to the focused column's worktree root."
                            }
                        }
                    }
                },
                {
                    "name": "git_diff",
                    "description": "Unified diff of the focused column's worktree, in-process (don't shell out to `git diff` — and the production guard forbids it). Three scopes: default = the working tree (staged + unstaged + untracked) vs HEAD; `cached:true` = staged vs HEAD (what would commit); `unstaged:true` = the index vs the working tree (plain `git diff` — only what changed SINCE you staged). The last is the read you want when someone stages a checkpoint and then keeps editing. Returns `git diff`-style unified text (empty string when there's nothing to show). Pass `root` for a different worktree, and `paths` to restrict to specific files/subtrees.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "cached": {
                                "type": "boolean",
                                "description": "If true, diff the staged changes (index vs HEAD). Default false = working tree (staged + unstaged + untracked) vs HEAD."
                            },
                            "unstaged": {
                                "type": "boolean",
                                "description": "If true, diff the index vs the working tree (plain `git diff` — only the unstaged changes, i.e. what changed since you last staged). Takes precedence over `cached`."
                            },
                            "paths": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Optional repo-relative paths (forward-slash) to restrict the diff to. Empty/omitted = the whole worktree."
                            },
                            "root": {
                                "type": "string",
                                "description": "Optional absolute path of the worktree to diff instead of the user's focused column — e.g. a sibling worktree you're working in. Defaults to the focused column's worktree root."
                            }
                        }
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
                    if !matches!(status.as_str(), "working" | "blocked" | "idle" | "done") {
                        return send_tool_error(
                            w,
                            id,
                            "status must be one of: working, blocked, idle, done",
                        );
                    }
                    let pane_id = args["pane_id"].as_str().map(String::from);
                    let pane = args["pane"].as_u64().and_then(|n| usize::try_from(n).ok());
                    let ttl_ms = args["ttl_ms"].as_u64();
                    // Piggybacked by the status-hook reporter (Claude's hook
                    // stdin carries `session_id`); absent on a direct agent call.
                    let session_id = args["session_id"].as_str().map(String::from);
                    McpCommand::ReportStatus {
                        pane_id,
                        pane,
                        status,
                        ttl_ms,
                        session_id,
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
                    let pane_id = args["pane_id"].as_str().map(String::from);
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
                    let pane_id = args["pane_id"].as_str().map(String::from);
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
