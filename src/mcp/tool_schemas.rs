//! The tool catalogue `tools/list` advertises: each tool's name, the
//! description an agent reads to decide when to call it, and its input
//! schema. Pure data; `protocol::handle_tools_call` serves each tool.

use serde_json::{Value, json};

/// The `tools/list` result.
pub fn tools() -> Value {
    json!({
        "tools": [
            {
                "name": "get_spyc_context",
                "description": "Get the current spyc file manager state: working directory, cursor position, picked files, inventory, active filter, git branch, project_home (sticky project root), session_name, plus the running spyc's pid and version ('<x.y.z> (<git-sha>)'). Use this to understand what the user is looking at — and to detect a stale server: if a tool you expect is missing, compare version's git SHA against the repo HEAD and ask the user to restart spyc (pid identifies the process). From an agent pane it also returns `pane`: YOUR own tab (id, tab, label, cwd, worktree_root, git_branch) — where you run, which can differ from the directory the user is browsing.",
                "inputSchema": {
                    "type": "object",
                    "properties": {},
                    "required": []
                }
            },
            {
                "name": "report_status",
                "description": "Report YOUR current activity so spyc shows it as a live dot on your pane tab — the 'which agent needs me' signal. Call it as your turn changes: 'working' when you start a non-trivial task, 'blocked' when you stop to ask the user a question or for permission (this is the one that earns attention), 'done' when you finish, 'idle' when waiting with nothing pending. Overrides spyc's output-timing guess and keeps your dot accurate through silent thinking. Targets your own tab by default (the focused tab if your connection named none); pass `pane` for a specific tab. Cheap and idempotent — call it freely.",
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
                            "maximum": crate::mcp_cmd::MAX_REPORT_TTL_MS,
                            "description": "Optional non-blocked report backstop in ms; default and maximum 300000 (five minutes). Larger values are clamped. Blocked stays latched until settled or replaced."
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
                        "pane_id": {"type": "string", "description": "Optional stable pane id (SPYC_PANE_ID); defaults to your own tab."},
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
                        "pane_id": {"type": "string", "description": "Optional stable pane id (SPYC_PANE_ID); defaults to your own tab."},
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
                "description": "Safely tear down a git worktree by path (the path create_worktree returned). Safe by default: archives any untracked + uncommitted changes to spyc's graveyard first (recoverable), removes the worktree, then deletes its branch ONLY if it is merged into the integration base — an unmerged branch's ref is kept (it's the commit backup). Refuses a worktree CLAIMED by another session (claim_worktree) — release it first. A spyc column sitting inside is reset to PROJECT_HOME, not refused. If an earlier removal failed partway (its .git is gone), calling this again finishes it. The teardown half of the worktree flow.",
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
                            "description": "Optional absolute path to resolve relative paths against instead of the user's focused column — e.g. a sibling worktree you're working in (a path from create_worktree/list_worktrees). Defaults to the focused column's worktree root. Must be inside one of this spyc session's roots — anything else is rejected, and the error names the allowed set."
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
                            "description": "Optional absolute path to walk instead of the user's focused column — e.g. a sibling worktree you're working in (a path from create_worktree/list_worktrees). Defaults to the focused column's worktree root. Must be inside one of this spyc session's roots — anything else is rejected, and the error names the allowed set."
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
                            "description": "Optional absolute path to search instead of the user's focused column — e.g. a sibling worktree you're working in (a path from create_worktree/list_worktrees). Defaults to the focused column's worktree root. Must be inside one of this spyc session's roots — anything else is rejected, and the error names the allowed set."
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
                            "description": "Optional absolute path of the worktree to inspect instead of the user's focused column — e.g. a sibling worktree you're working in (a path from create_worktree/list_worktrees). Defaults to the focused column's worktree root. Must be inside one of this spyc session's roots — anything else is rejected, and the error names the allowed set."
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
                            "description": "Optional absolute path of the worktree whose history to read instead of the user's focused column — e.g. a sibling worktree you're working in. Defaults to the focused column's worktree root. Must be inside one of this spyc session's roots — anything else is rejected, and the error names the allowed set."
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
                            "description": "Optional absolute path of the worktree to diff instead of the user's focused column — e.g. a sibling worktree you're working in. Defaults to the focused column's worktree root. Must be inside one of this spyc session's roots — anything else is rejected, and the error names the allowed set."
                        }
                    }
                }
            }
        ]
    })
}
