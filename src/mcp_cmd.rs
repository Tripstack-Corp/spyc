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
    /// Create a git worktree off the focused column's repo for `branch`
    /// (existing branch, else a new one at HEAD). Lets a skill spin up a
    /// worktree to work in `b` while `a` stays on its branch. Replies with
    /// the new worktree's path.
    CreateWorktree { branch: String },
    /// Tear down a worktree by path (the path `CreateWorktree` returned).
    /// Refuses a dirty/locked worktree or one a column is currently in; the
    /// branch ref is left intact. The teardown half of the skill flow.
    RemoveWorktree { path: String },
    /// Clean out a worktree: archive its untracked files into the graveyard
    /// (under `<worktree>-<timestamp>`), then remove it. Refuses if a column is
    /// in it or there are uncommitted changes to *tracked* files. Like
    /// `RemoveWorktree` but doesn't choke on untracked junk — it preserves it.
    CleanWorktree { path: String },
    /// Another spyc instance has taken over the MCP socket for this
    /// directory. The TUI should warn the user.
    Disconnected { new_pid: u32 },
}

/// Response sent back to the MCP thread after command execution.
#[derive(Debug)]
pub enum McpResponse {
    Ok { message: String },
    Error { message: String },
}
