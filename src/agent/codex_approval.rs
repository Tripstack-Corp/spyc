//! Human-approval fallback for Codex's uncorrelated permission hook.

use super::detect_rules::{DetectionRule, Matcher, Region};
use crate::pane::{AgentActivity, ReportedStatus};
use crate::state::sessions::AgentKind;

/// Recorded Codex command, file-edit, MCP-tool and network approval forms:
/// required choices and footer end the viewport. Permission hooks also run
/// before automatic review, so only the visible form establishes a human wait.
/// Missing required text or changed forms produce no guess.
pub const RULES: &[DetectionRule] = &[
    DetectionRule {
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
    },
    DetectionRule {
        region: Region::BottomNonEmptyLines(usize::MAX),
        matcher: Matcher::AllAtBottom {
            needles: &[
                "Would you like to make the following edits?",
                "Yes, proceed",
                "Yes, and don't ask again for these files",
                "No, and tell Codex what to do differently",
            ],
            last_line: "Press enter to confirm or esc to cancel",
        },
        state: AgentActivity::Blocked,
        visible_blocker: Some("awaiting file-edit approval"),
    },
    DetectionRule {
        region: Region::BottomNonEmptyLines(usize::MAX),
        matcher: Matcher::AllAtBottom {
            needles: &[
                "Field 1/1",
                "1. Allow",
                "Run the tool and continue",
                "Cancel this tool call",
            ],
            last_line: "enter to submit | esc to cancel",
        },
        state: AgentActivity::Blocked,
        visible_blocker: Some("awaiting MCP tool approval"),
    },
    DetectionRule {
        region: Region::BottomNonEmptyLines(usize::MAX),
        matcher: Matcher::AllAtBottom {
            needles: &[
                "Do you want to approve network access to",
                "Yes, just this once",
                "No, and tell Codex what to do differently",
            ],
            last_line: "Press enter to confirm or esc to cancel",
        },
        state: AgentActivity::Blocked,
        visible_blocker: Some("awaiting network approval"),
    },
];

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
