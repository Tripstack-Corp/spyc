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

use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use crate::context;

const SERVER_NAME: &str = "spyc";
// Version + short git SHA (e.g. `x.y.z (25abd0a)`) so the `initialize`
// handshake announces the exact build — an MCP client can tell whether the
// running server predates a tool it expects.
const SERVER_VERSION: &str = crate::VERSION;
const PROTOCOL_VERSION: &str = "2024-11-05";

/// Returned in the `initialize` response's `instructions` field — the MCP
/// spec's slot for "how to use this server". Claude Code folds it into the
/// system prompt at connect, which is the one ephemeral, no-files-written way
/// to bias a spyc-launched agent toward spyc's own tools (it otherwise reaches
/// for `Bash rg` / `git worktree` and never touches them). Kept short on
/// purpose — clients truncate instructions, so the prioritization comes first.
const SERVER_INSTRUCTIONS: &str = "\
You are running inside spyc, a terminal file/worktree manager, with its tools \
on this server. Prefer them over shell equivalents — even mid-task, not only \
when answering questions about the user's view:\n\
- Call `get_spyc_context` first to ground yourself: the user's cwd, cursor \
file, picks, filter, git branch, and the running spyc's pid + version.\n\
- `search_content` / `search_paths` instead of `Bash rg` / `find`, and \
`git_status` / `git_log` / `git_diff` instead of shelling out to git — all \
in-process, gitignore-aware, and structured. `git_diff` has three scopes: \
default vs HEAD, `cached:true` for staged, and `unstaged:true` for the index \
vs the working tree (what changed since the last `git add` — use it after a \
staged checkpoint). They scope to the focused column \
by default; when you're working in a DIFFERENT worktree, pass its path as the \
`root` argument so they target it (otherwise shell with explicit paths is the \
right call).\n\
- `navigate_to` to move the user's view; `pick_files` / `set_filter` to drive \
their selection; `get_file_content` to read what they're viewing.\n\
- `report_status` to keep your pane tab's activity dot accurate: call it \
`working` when you start a task, `blocked` when you pause to ask the user a \
question or for permission (so they see at a glance which agent needs them), \
and `done` when you finish. Cheap and idempotent — call it freely as your turn \
changes; it overrides spyc's output-timing guess.\n\
- Worktree lifecycle, all in-process (never `git worktree`): `list_worktrees` \
lists them (branch, dirty counts, which is current, whether each is merged / \
ahead-behind the base — the safe-to-remove signal — and whether one is claimed \
by another session), `create_worktree` adds one (pass `open:true` to also open \
it in column b and work there right away), `open_worktree` opens an existing one \
in column b while the user's column stays put, and `remove_worktree` tears one \
down safely — archiving any untracked + uncommitted changes to spyc's \
graveyard, then deleting the branch only if it's merged (`clean_worktree` is \
an alias).\n\
- Coordinating with another agent on the same repo: `claim_worktree(path, reason)` \
to lease the worktree you're working in (a cooperative lock — others' \
remove/clean will refuse it), and `release_worktree(path)` when you're done. \
Before removing a worktree you didn't create, `list_worktrees` first and skip \
any that are `locked` by someone else.\n\
If a tool you expect is missing, the running spyc is older than this repo — \
tell the user to restart it (compare `version`'s git SHA to the repo HEAD).";
const CONTEXT_URI: &str = "spyc://context";

/// Socket IO deadline for the stdio proxy. Bounds how long it waits on a
/// server response (and on a write) so a wedged / silent / panicked server
/// thread surfaces as a clean JSON-RPC error to the agent instead of
/// hanging it indefinitely. Generous — well above the server's own 5 s
/// writable-action timeout — so a legitimately slow reply isn't cut off;
/// only a genuine indefinite stall trips it.
const PROXY_IO_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// Append a line to `<state>/mcp.log` for debugging MCP connection issues.
/// Owner-only (0600) in the XDG state dir — not the old world-readable,
/// symlink-followable `/tmp/spyc-mcp.log`.
fn mcp_log(msg: &str) {
    use std::io::Write;
    if let Some(mut f) = crate::state::open_state_file_append("mcp.log") {
        let _ = writeln!(f, "spyc-mcp: {msg}");
    }
}

/// Whether to log full MCP message/response *bodies* (opt-in). Off by
/// default: a `get_file_content` response is the entire file text, so
/// logging bodies mirrors every file the agent reads into the log. Set
/// `SPYC_MCP_DEBUG=1` to include bodies when diagnosing a protocol issue.
fn log_bodies() -> bool {
    std::env::var_os("SPYC_MCP_DEBUG").is_some_and(|v| !v.is_empty() && v != "0")
}

// ── Shared JSON-RPC dispatch ────────────────────────────────────

/// spyc's per-user state directory: `~/.local/state/spyc` (falling back
/// to `/tmp` when `$HOME` is unset). Holds the MCP socket and the
/// trusted-root sidecars — all owner-private; an attacker who can only
/// plant files in a *cloned repo* cannot write here.
pub fn state_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(".local/state/spyc")
}

/// Socket path for a given PID: `~/.local/state/spyc/mcp-<pid>.sock`.
pub fn socket_path_for(pid: u32) -> PathBuf {
    state_dir().join(format!("mcp-{pid}.sock"))
}

/// Trusted-root sidecar path for a PID, in the given state dir:
/// `<state_dir>/mcp-<pid>.root`. The running spyc writes the directory
/// it is rooted at here (next to its socket); discovery cross-checks a
/// `.spyc-context-<pid>.json` marker's location against it so a planted
/// marker — which an attacker *can* write into a repo, but whose pid is
/// really rooted elsewhere — can't redirect attachment cross-project.
/// Parameterized on `state_dir` so tests can inject a temp dir (no env).
pub fn root_marker_path_in(state_dir: &Path, pid: u32) -> PathBuf {
    state_dir.join(format!("mcp-{pid}.root"))
}

/// Socket path for the current process.
pub fn socket_path() -> PathBuf {
    socket_path_for(std::process::id())
}

mod config;
mod hooks;
mod protocol;
mod readers;
mod server;

pub use config::{
    ConfigCleanup, McpConfigStatus, cleanup_codex_config, cleanup_mcp_json, detect_existing_spyc,
    detect_existing_spyc_codex, ensure_codex_config_toml, ensure_mcp_json, enterprise_defines_spyc,
    sweep_orphan_spyc_configs,
};
pub use hooks::{cleanup_claude_status_hooks, ensure_claude_status_hooks, set_status_trace};
pub use server::{cleanup_socket, start_socket_server};

use server::{discover_live_socket, run_direct, run_proxy};

/// Resolve context path from env var or project root.
fn resolve_context_path(project_root: &Path) -> PathBuf {
    if let Ok(p) = std::env::var(context::CONTEXT_ENV_VAR)
        && !p.is_empty()
    {
        return PathBuf::from(p);
    }
    context::context_path(project_root)
}

/// The *effective* report state, given the hook's configured state and the
/// Claude Code event JSON piped to the hook's stdin. The one remap: a
/// `Notification` whose `notification_type` is `idle_prompt` ("Claude is
/// waiting for your input") is the agent **finished and waiting** — that's
/// `done` (the calm teal square), NOT `blocked` (the alarming red "needs me"
/// square). Without it an idle agent flips to a false-red square after Claude's
/// ~60s idle nudge — both panes "switched to stop squares" when the user looked
/// away. A `permission_prompt` Notification (and the `PermissionRequest` event,
/// which fires for real tool-permission prompts) keeps `blocked`. An empty or
/// unparseable payload leaves the configured state unchanged. Pure + tested.
fn effective_report_state<'a>(configured: &'a str, hook_payload: &str) -> &'a str {
    if configured != "blocked" {
        return configured;
    }
    let idle_notification = serde_json::from_str::<serde_json::Value>(hook_payload)
        .ok()
        .is_some_and(|v| {
            v["hook_event_name"] == "Notification" && v["notification_type"] == "idle_prompt"
        });
    if idle_notification {
        "done"
    } else {
        configured
    }
}

/// Agent status-hook reporter (`spyc --report-status <state>`): a one-shot that
/// pings the running spyc so it can set this pane's activity dot. Reads
/// `SPYC_MCP_SOCK` (the socket) + `SPYC_PANE_ID` (which tab) from the
/// environment — both injected into the agent pane by spyc and inherited by the
/// agent's lifecycle hook.
///
/// **Best-effort and silent by contract:** it runs *inside* the agent's hook,
/// so a missing socket / dead spyc / wedged server must NEVER error or block
/// the agent — every failure path just returns. A tight 2 s IO timeout (far
/// below the proxy's 20 s) bounds the worst case so the hook can't stall the
/// agent's turn.
pub fn report_status_to_socket(state: &str, trace: bool) {
    use std::io::{BufRead, BufReader};
    const REPORT_IO_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

    // Opt-in diagnostic trace (`--status-trace`, baked into the installed hook
    // command so it survives any env sanitization — it travels as a cmdline arg,
    // not an env var). OFF by default: this reporter fires on every agent turn,
    // so always-on would spam mcp.log. When on, it logs each invocation + the
    // env it actually saw, distinguishing a hook that FIRED-but-couldn't-reach
    // (no SPYC_MCP_SOCK) from one that never fired. `grep report-status mcp.log`.
    let trace_log = |msg: &str| {
        if trace {
            mcp_log(msg);
        }
    };

    let sock = std::env::var("SPYC_MCP_SOCK").unwrap_or_default();
    let pane_id = std::env::var("SPYC_PANE_ID").ok();
    trace_log(&format!(
        "report-status: state={state} SPYC_MCP_SOCK={} SPYC_PANE_ID={}",
        if sock.is_empty() {
            "MISSING"
        } else {
            sock.as_str()
        },
        if pane_id.as_deref().unwrap_or("").is_empty() {
            "MISSING"
        } else {
            "set"
        },
    ));
    // Claude Code pipes the hook event JSON to the hook's stdin (carrying
    // `hook_event_name`, `notification_type`, `tool_name`, …) then closes it.
    // Read it ALWAYS (not just when tracing): the *effective* state depends on
    // it — `effective_report_state` downgrades an `idle_prompt` Notification
    // from `blocked` to `done`. Guarded on `!is_terminal()` so a manual
    // `spyc --report-status …` from a shell never blocks on read, and capped so
    // a pathological payload can't balloon mcp.log.
    let payload = {
        use std::io::{IsTerminal, Read};
        let stdin = std::io::stdin();
        if stdin.is_terminal() {
            String::new()
        } else {
            let mut s = String::new();
            let _ = stdin.lock().take(8192).read_to_string(&mut s);
            s.trim().to_string()
        }
    };
    if trace && !payload.is_empty() {
        // Max-info diagnostic: see EXACTLY which event fired this report (the
        // hook *command* is identical across PermissionRequest / Notification /
        // PreToolUse, so the event name only lives in the payload).
        trace_log(&format!("report-status: hook stdin: {payload}"));
    }
    // An idle Notification means "finished, waiting" → `done`, not the alarming
    // `blocked` square (the false-red-on-idle bug); permission stays `blocked`.
    let state = effective_report_state(state, &payload);
    if trace {
        trace_log(&format!("report-status: effective state={state}"));
    }
    if sock.is_empty() {
        return;
    }
    let Ok(mut stream) = UnixStream::connect(&sock) else {
        trace_log(&format!("report-status: connect FAILED to {sock}"));
        return; // spyc not running / different instance — nothing to update
    };
    let _ = stream.set_read_timeout(Some(REPORT_IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(REPORT_IO_TIMEOUT));
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "report_status",
            "arguments": { "status": state, "pane_id": pane_id },
        },
    })
    .to_string();
    // The socket server reads Content-Length-framed messages (`read_lsp_message`,
    // same framing the proxy forwards). A bare newline-delimited line has no
    // `Content-Length` header, so the server never parses it and the report is
    // silently dropped — which is why hook-driven reports never moved the dot,
    // while MCP-tool calls (routed through the framing proxy) did. Frame it the
    // same way via `send_message` (which also flushes).
    if protocol::send_message(&mut stream, &req).is_err() {
        trace_log("report-status: write FAILED");
        return;
    }
    trace_log(&format!("report-status: sent state={state}"));
    // Read the one-line reply so spyc has applied the command before we exit —
    // closing the socket immediately would otherwise race the main-loop apply.
    // Best-effort: a timeout/EOF here doesn't matter, the command is queued.
    let mut line = String::new();
    let _ = BufReader::new(stream).read_line(&mut line);
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

#[cfg(test)]
mod tests;
