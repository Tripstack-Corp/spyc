//! Shared types for the MCP command channel.
//!
//! The MCP socket server runs on background threads. Writable actions
//! (navigate, filter, pick) must execute on the main thread which owns
//! `AppState`. These types bridge the two via `std::sync::mpsc`.
//!
//! Each request bundles a one-shot reply sender so the MCP thread can
//! block until the main loop processes the command.

use std::sync::mpsc;

/// A request sent from an MCP server thread to the main event loop.
#[derive(Debug)]
pub struct McpRequest {
    pub command: McpCommand,
    pub reply: mpsc::Sender<McpResponse>,
}

/// Commands that Claude can send to mutate the TUI workspace.
#[derive(Debug)]
pub enum McpCommand {
    /// Navigate to a directory or file (file → parent dir + cursor focus).
    NavigateTo { path: String },
    /// Set or clear the limit filter. `None` or empty clears.
    SetFilter { pattern: Option<String> },
    /// Pick files matching glob patterns (additive to existing picks).
    PickFiles { patterns: Vec<String> },
    /// Clear all picks.
    ClearPicks,
    /// Create a git worktree for `branch` (existing branch, else a new one).
    /// `base` overrides the start point for a NEW branch (`None` → PROJECT_HOME's
    /// default branch, the POLA default). `open` also opens it in column `b`
    /// (the create→work flow in one call). Replies with the new worktree's path.
    CreateWorktree {
        branch: String,
        base: Option<String>,
        open: bool,
    },
    /// Tear down a worktree by path (the path `CreateWorktree` returned).
    /// Refuses a dirty/locked worktree or one a column is currently in; the
    /// branch ref is left intact. The teardown half of the skill flow.
    RemoveWorktree { path: String },
    /// Clean out a worktree: archive its untracked files into the graveyard
    /// (under `<worktree>-<timestamp>`), then remove it. Refuses if a column is
    /// in it or there are uncommitted changes to *tracked* files. Like
    /// `RemoveWorktree` but doesn't choke on untracked junk — it preserves it.
    CleanWorktree { path: String },
    /// Open the second commander (column `b`) at `path` — typically the worktree
    /// `CreateWorktree` returned — so a skill can work in it while `a` stays put.
    /// Re-targets `b` if it's already open. The "work in it in b" step.
    OpenWorktree { path: String },
    /// Agent self-reports its activity for the per-tab dot (P1 semantic
    /// channel). `status` is `working`/`blocked`/`idle`/`done`. Targeting, in
    /// priority order: `pane_id` (the stable `SPYC_PANE_ID` uuid — what the
    /// auto-hook sends), else `pane` (a 1-based divider `[N]`), else the focused
    /// tab. `ttl_ms` overrides the backstop expiry. Overrides output timing.
    ReportStatus {
        pane_id: Option<String>,
        pane: Option<usize>,
        status: String,
        ttl_ms: Option<u64>,
    },
    /// Another spyc instance has taken over the MCP socket for this
    /// directory. The TUI should warn the user.
    Disconnected { new_pid: u32 },
    /// Fire-and-forget telemetry: an agent invoked the named MCP tool. Sent by
    /// the socket dispatch for EVERY `tools/call` (read tools included, which
    /// are otherwise served on the socket thread and never reach the main
    /// loop), so the `A` overlay can show cumulative per-tool call counts. The
    /// reply is ignored.
    ToolCalled { name: String },
    /// The socket server received a message it couldn't frame/parse and dropped
    /// it. Surfaced as a status-line warning so a silent drop can't hide a
    /// client/framing bug (a bare-newline `--report-status` reporter went
    /// unnoticed for days exactly because the drop was silent). `detail` is the
    /// parse error. The reply is ignored.
    MalformedSocketMessage { detail: String },
}

/// Response sent back to the MCP thread after command execution.
#[derive(Debug)]
pub enum McpResponse {
    Ok { message: String },
    Error { message: String },
}
