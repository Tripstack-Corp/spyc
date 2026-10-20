//! Shared types for the MCP command channel.
//!
//! The MCP socket server runs on background threads. Writable actions
//! (navigate, filter, pick) must execute on the main thread which owns
//! `AppState`. These types bridge the two via `std::sync::mpsc`.
//!
//! Each request bundles a one-shot reply sender so the MCP thread can
//! block until the main loop processes the command.

/// Maximum non-blocked report lifetime; blocked still requires explicit recovery.
pub const MAX_REPORT_TTL_MS: u64 = 300_000;
/// `waiting`'s default and maximum lifetime: a CI run outlasts five minutes, and
/// the agent re-reports each time its work wakes it.
pub const MAX_WAITING_TTL_MS: u64 = 3_600_000;
/// Cap on a report's note, in characters.
pub const REPORT_NOTE_CHARS: usize = 80;

/// A report's note as spyc keeps and draws it. Control characters and the
/// invisible format characters that reorder or hide text become spaces, runs
/// of whitespace collapse to one, and anything past [`REPORT_NOTE_CHARS`] is
/// cut with `…`. `None` when nothing printable is left.
pub fn report_note(raw: &str) -> Option<String> {
    let invisible = |c: char| {
        matches!(
            c,
            '\u{200B}'..='\u{200F}' | '\u{2028}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}'
        )
    };
    let printable: String = raw
        .chars()
        .map(|c| {
            if c.is_control() || invisible(c) {
                ' '
            } else {
                c
            }
        })
        .collect();
    let note = printable.split_whitespace().collect::<Vec<_>>().join(" ");
    if note.is_empty() {
        return None;
    }
    if note.chars().count() <= REPORT_NOTE_CHARS {
        return Some(note);
    }
    let mut cut: String = note.chars().take(REPORT_NOTE_CHARS - 1).collect();
    cut.push('…');
    Some(cut)
}

use std::sync::mpsc;

/// A request sent from an MCP server thread to the main event loop.
#[derive(Debug)]
pub struct McpRequest {
    pub command: McpCommand,
    pub reply: mpsc::Sender<McpResponse>,
    /// The project of the connection that sent it, from its binding (#99);
    /// `None` from a connection bound to none. It says who is asking, not what
    /// to do, so it rides here rather than on each command.
    pub project: Option<crate::project_id::ProjectId>,
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
    /// `base` overrides the start point for a NEW branch (`None` → the default
    /// branch of the repo it lands in, the POLA default). `open` also opens it
    /// in column `b` (the create→work flow in one call). Replies with the new
    /// worktree's path. The repo is the agent's project's; `pane_id` (the
    /// calling agent's own tab) and then `cwd` (the cwd its proxy named at
    /// `initialize`) are where it is found for a project outside any repo.
    CreateWorktree {
        branch: String,
        base: Option<String>,
        open: bool,
        pane_id: Option<String>,
        cwd: Option<std::path::PathBuf>,
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
    /// channel). `status` is an activity name or a correlated Codex question signal. Targeting, in
    /// priority order: `pane_id` (the stable `SPYC_PANE_ID` uuid — what the
    /// auto-hook sends, and what the socket thread fills in from an attributed
    /// connection when the call names no target), else `pane` (a 1-based
    /// divider `[N]`), else the focused tab. `ttl_ms` overrides the backstop
    /// expiry. Overrides output timing.
    ReportStatus {
        pane_id: Option<String>,
        pane: Option<usize>,
        status: String,
        ttl_ms: Option<u64>,
        /// Already normalized by [`report_note`].
        note: Option<String>,
        /// The agent's live session UUID, piggybacked from the status hook's
        /// stdin (Claude includes `session_id` in every hook payload). `Some`
        /// only on hook-driven reports; a direct agent `report_status` call (or
        /// the Lua binding) omits it. Lets `save_session` prefer the *live*
        /// conversation over the spawn-proximity resolver (P1-3).
        session_id: Option<String>,
        hook_event: Option<crate::agent::status_hook::StatusHookEvent>,
    },
    /// P2 merge/scope coordination (`docs/archive/AGENT_AWARENESS_PLAN.md`): declare
    /// the scope this agent is about to touch. Same targeting priority as
    /// `ReportStatus` (`pane_id` → `pane` → focused) — that tab's stable
    /// `claim_owner` key becomes the claim's `owner`. `intent` is
    /// `editing`/`merging`; only `merging` claims block a
    /// `wait_for_scope_clear`. Returns the new claim's id.
    RegisterScope {
        pane_id: Option<String>,
        pane: Option<usize>,
        paths: Vec<String>,
        intent: String,
        pr: Option<String>,
        note: Option<String>,
    },
    /// P2: the full scope registry as JSON — what an agent checks before
    /// merging, and what the orchestration screen renders.
    ListScopes,
    /// P2: drop a claim by id (no ownership check — advisory tooling; a lead
    /// agent or the user may clean up a stale claim on someone else's behalf).
    /// No-op (not an error) if `id` doesn't match any live claim.
    ReleaseScope { id: u64 },
    /// P2: block until no *other* owner's `Merging` claim overlaps `paths`, or
    /// `timeout_ms` elapses. Same targeting as `RegisterScope` (`pane_id` →
    /// `pane` → focused) resolves the caller's owner key so its own claims never
    /// block it. This request does NOT reply inline — the loop parks its reply
    /// sender (`Runtime::scope_waiters`) and fires it from `settle_scope_waiters`
    /// once the conflict clears or the deadline passes.
    WaitForScopeClear {
        pane_id: Option<String>,
        pane: Option<usize>,
        paths: Vec<String>,
        timeout_ms: u64,
    },
    /// Bind a connection, once, at the `initialize` that names its pane or cwd
    /// (#99): to the live tab carrying `pane_id`, in whichever project holds
    /// it; else to the open project whose root holds `cwd`; else to the active
    /// project. Replies JSON `{pane, project, context_file}`: whether the pane
    /// bound, the project's id, and the context file its reads answer from.
    Bind {
        pane_id: Option<String>,
        cwd: Option<std::path::PathBuf>,
    },
    /// What `get_spyc_context` adds for a bound connection, as JSON: `project`
    /// (its id, label, home, and whether it is the one the user is in) and,
    /// while the tab carrying `pane_id` is live, `pane` (its id, 1-based index,
    /// label, live cwd, worktree root and branch). Asked on every call, since
    /// the tab can close or move under it.
    CallerContext {
        pane_id: Option<String>,
        project: crate::project_id::ProjectId,
    },
    /// An older spyc, one that still pins its own socket in the agents' MCP
    /// entry, rewrote this directory's entry and said so (`spyc/disconnected`).
    /// Nothing this version sends; the TUI tells the user who owns the entry.
    Disconnected { new_pid: u32 },
    /// What Claude Code's statusline told `spyc --statusline` in a pane: the
    /// plan's rate-limit windows and the context window. Fire-and-forget.
    Statusline {
        pane_id: String,
        statusline: crate::agent::claude_statusline::Statusline,
    },
    /// Fire-and-forget telemetry: an agent invoked the named MCP tool. Sent by
    /// the socket dispatch for EVERY `tools/call` (read tools included, which
    /// are otherwise served on the socket thread and never reach the main
    /// loop), so the `A` overlay can show cumulative per-tool call counts. The
    /// reply is ignored.
    ToolCalled {
        name: String,
        /// The socket connection it came over (`None` in the read-only stdio
        /// fallback), so `:activity dump` can count calls per connection.
        conn: Option<u64>,
    },
    /// A socket connection sent its first `initialize`: an agent's MCP session
    /// began. `pane_id` is the tab it bound to, `None` when unattributed. The
    /// status hook's one-shot `report_status` calls never initialize, so they
    /// never appear here. The reply is ignored.
    ConnectionInitialized { conn: u64, pane_id: Option<String> },
    /// An initialized connection is bound to `project` (#99): after its
    /// `ConnectionInitialized`, and again whenever it binds later, as when its
    /// project closes. The reply is ignored.
    ConnectionBound {
        conn: u64,
        project: crate::project_id::ProjectId,
    },
    /// An initialized connection closed. The reply is ignored.
    ConnectionClosed { conn: u64 },
    /// The socket server received a message it couldn't frame/parse and dropped
    /// it. Surfaced as a status-line warning so a silent drop can't hide a
    /// client/framing bug (a bare-newline `--report-status` reporter went
    /// unnoticed for days exactly because the drop was silent). `detail` is the
    /// parse error. The reply is ignored.
    MalformedSocketMessage { detail: String },
}

impl McpCommand {
    /// A command that only records something: a connection's start or end, a
    /// tool call's tally, a statusline report, an older spyc's word.
    pub const fn is_notice(&self) -> bool {
        matches!(
            self,
            Self::Statusline { .. }
                | Self::Disconnected { .. }
                | Self::ConnectionInitialized { .. }
                | Self::ConnectionBound { .. }
                | Self::ConnectionClosed { .. }
                | Self::ToolCalled { .. }
        )
    }
}

/// Response sent back to the MCP thread after command execution.
#[derive(Debug)]
pub enum McpResponse {
    Ok { message: String },
    Error { message: String },
}

#[cfg(test)]
mod tests {
    use super::{REPORT_NOTE_CHARS, report_note};

    /// A note is drawn in the pane divider's diagnostics and the agent HUD, so
    /// nothing in it may move the cursor, colour the terminal or reorder text.
    #[test]
    fn report_note_keeps_only_printable_text_on_one_line() {
        assert_eq!(report_note("CI on #641").as_deref(), Some("CI on #641"));
        assert_eq!(
            report_note("  CI\n\ton  #641\r\n").as_deref(),
            Some("CI on #641")
        );
        assert_eq!(
            report_note("\u{1b}[31mred\u{1b}[0m \u{9b}2J").as_deref(),
            Some("[31mred [0m 2J"),
            "ESC and the C1 CSI are control characters"
        );
        assert_eq!(
            report_note("abc\u{202E}fed\u{200B}x\u{2066}y\u{FEFF}").as_deref(),
            Some("abc fed x y")
        );
        for empty in ["", "   ", "\n\u{1b}\u{202E}"] {
            assert_eq!(report_note(empty), None, "{empty:?}");
        }
    }

    #[test]
    fn report_note_is_cut_at_its_character_cap() {
        let exact = "é".repeat(REPORT_NOTE_CHARS);
        assert_eq!(report_note(&exact).as_deref(), Some(exact.as_str()));
        let cut = report_note(&"é".repeat(REPORT_NOTE_CHARS + 1)).unwrap();
        assert_eq!(cut.chars().count(), REPORT_NOTE_CHARS);
        assert!(cut.ends_with("é…"), "{cut}");
    }
}
