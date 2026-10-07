//! Human-approval fallback for Codex's uncorrelated permission hook.

use super::detect_rules::{DetectionRule, Matcher, Region};
use crate::pane::{AgentActivity, ReportedStatus};
use crate::state::sessions::AgentKind;

/// Captured Codex command-approval dialogue: the complete choice list ends the
/// viewport. Permission hooks also run before automatic review, so only this
/// visible modal establishes a human wait. A missing/clipped/customized modal
/// produces no guess.
pub const RULES: &[DetectionRule] = &[DetectionRule {
    // Input is the current viewport, already bounded by the terminal geometry.
    // Command details can span many lines; the modal header must stay visible.
    region: Region::BottomNonEmptyLines(usize::MAX),
    matcher: Matcher::AllAtBottom {
        needles: &[
            "Would you like to run the following command?",
            "Yes, proceed",
            "No, and tell Codex what to do differently",
        ],
        last_line: "Press enter to confirm or esc to cancel",
    },
    state: AgentActivity::Blocked,
    visible_blocker: Some("awaiting command approval"),
}];

/// The modal takes precedence over a non-blocked report without discarding it:
/// after approval the silent-work report resumes. Identified questions and
/// explicit agent blocks remain authoritative until their own recovery.
pub fn overrides_report(
    reported: Option<ReportedStatus>,
    scrape: Option<AgentActivity>,
    kind: AgentKind,
) -> bool {
    kind == AgentKind::Codex
        && scrape == Some(AgentActivity::Blocked)
        && reported.is_none_or(|report| report.status != AgentActivity::Blocked)
}
