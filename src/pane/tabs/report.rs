//! A tab's semantic activity reports ([`ReportedStatus`]) and its time out of
//! view ([`Away`]).

use super::AgentActivity;

/// Why a previously live [`ReportedStatus`] yielded before its TTL backstop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetiredReason {
    /// The pane remained quiet for the agent's [`crate::agent::AgentProfile::working_quiet_window`].
    PaneQuiet(std::time::Duration),
}

/// A tab's time out of view: when it left, and the turns its agent finished
/// since, the last when.
#[derive(Clone, Copy, Debug)]
pub struct Away {
    pub left: std::time::Instant,
    pub turns: u32,
    pub last_done: Option<std::time::Instant>,
}

/// A semantic activity self-report from a cooperative agent via the
/// `report_status` MCP tool — the P1 channel that supersedes output timing.
///
/// Authority model (`App::effective_activity`): a report wins over the timing
/// fallback until it expires (`expiry`, a backstop against a crashed agent's
/// stale report). Visible approval prompts in Codex and agy temporarily
/// override a non-blocked report without discarding it. Every report survives
/// redraws and silent tool waits, except as [`Self::yields_to_timing`] says.
#[derive(Clone, Debug)]
pub struct ReportedStatus {
    /// The reported state.
    pub status: AgentActivity,
    /// When the report was received (monotonic).
    pub at: std::time::Instant,
    /// Non-blocked backstop expiry (five minutes at most, an hour for
    /// `waiting`); blocked remains latched until settled while the pane is alive.
    pub expiry: std::time::Instant,
    /// Base instant for [`crate::agent::AgentProfile::working_quiet_window`];
    /// starts at `at` and advances when a visible approval override holds or is
    /// answered so time spent at the prompt does not consume the post-answer
    /// quiet grace.
    pub quiet_since: std::time::Instant,
    /// True while a visible approval prompt overrides this report, or until the
    /// first activity settle after that prompt clears stamps its timestamp into
    /// [`Self::quiet_since`].
    pub overridden_by_approval: bool,
    /// Why a previously live report yielded before TTL expiry, for `:why-status`.
    pub retired_reason: Option<RetiredReason>,
    /// What the agent said it is doing, bounded and stripped of control
    /// characters (`mcp_cmd::report_note`). For a hook-inferred `waiting`, what
    /// it waits on.
    pub note: Option<String>,
    /// What a hook's `blocked` report waits on, from the event's metadata
    /// alone (`agent::waiting::blocked_on`): the tool, `question`, `plan`, or
    /// why the turn failed.
    pub blocked_on: Option<String>,
    /// What the tool it waits on acts on, held only while `[notify]
    /// blocked_detail` is on (`agent::waiting::blocked_detail`).
    pub blocked_detail: Option<crate::agent::status_hook::BlockedDetail>,
    /// A `waiting` report has absorbed the hook `done` that ended the turn
    /// which reported it (`agent::waiting::meet`).
    pub turn_end_absorbed: bool,
}

impl ReportedStatus {
    pub const fn new(
        status: AgentActivity,
        at: std::time::Instant,
        expiry: std::time::Instant,
    ) -> Self {
        Self {
            status,
            at,
            expiry,
            quiet_since: at,
            overridden_by_approval: false,
            retired_reason: None,
            note: None,
            blocked_on: None,
            blocked_detail: None,
            turn_end_absorbed: false,
        }
    }

    /// When a `Working` report's per-agent quiet grace ends (`None` for other
    /// statuses or agents without [`crate::agent::AgentProfile::working_quiet_window`]).
    pub fn working_quiet_expiry(
        &self,
        last_output_at: Option<std::time::Instant>,
        kind: crate::state::sessions::AgentKind,
    ) -> Option<std::time::Instant> {
        (self.status == AgentActivity::Working)
            .then(|| crate::agent::profile_for(kind).working_quiet_window())
            .flatten()
            .map(|window| {
                last_output_at.map_or(self.quiet_since, |o| o.max(self.quiet_since)) + window
            })
    }

    /// Keep [`Self::quiet_since`] pinned to `now` while a visible approval
    /// prompt overrides this report, and on the first settle after that prompt
    /// clears.
    pub fn settle_approval_override(&mut self, scrape_overrides: bool, now: std::time::Instant) {
        if scrape_overrides || self.overridden_by_approval {
            self.quiet_since = self.quiet_since.max(now);
            self.overridden_by_approval = scrape_overrides;
        }
    }

    /// Whether this report has given way to output timing. Only a `Working`
    /// report from an agent with a
    /// [`crate::agent::AgentProfile::working_quiet_window`] (agy) yields, once
    /// the pane has been quiet that long, so a missing `Stop` can't pulse for
    /// the whole TTL. Every other report holds through prompt redraws, footer
    /// flushes after `Stop` and background-task updates until a newer report
    /// or its TTL, and `Blocked` is latched until settled (`Enter`, `Esc`, `^c`
    /// in `run_effects`, or a matching tool completion for a Codex question).
    pub fn yields_to_timing(
        &self,
        last_output_at: Option<std::time::Instant>,
        kind: crate::state::sessions::AgentKind,
        now: std::time::Instant,
    ) -> bool {
        self.working_quiet_expiry(last_output_at, kind)
            .is_some_and(|exp| now >= exp)
    }
}
