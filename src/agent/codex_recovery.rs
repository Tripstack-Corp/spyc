//! Correlate question-tool lifecycle reports within one pane.

use super::status_hook::StatusHookEvent;
use crate::pane::AgentActivity;

/// Distinct wire values keep older hosts from applying uncorrelated recovery.
pub const QUESTION_START: &str = "codex-question-start";
pub const QUESTION_END: &str = "codex-question-end";
const PENDING_LIMIT: usize = 8;

#[derive(PartialEq, Eq)]
struct Question {
    session: String,
    turn: String,
    call: String,
}

#[derive(Default)]
pub struct CodexRecovery {
    pending: Vec<Question>,
    overflow: bool,
    other_blocked: bool,
}

impl CodexRecovery {
    pub const fn question_waiting(&self) -> bool {
        !self.pending.is_empty() || self.overflow
    }

    /// Fold bounded lifecycle metadata; argument/response content never enters.
    /// Explicit ordinary reports remain authoritative and retire these waits.
    pub fn report(
        &mut self,
        signal: &str,
        status: AgentActivity,
        session: Option<&str>,
        event: Option<&StatusHookEvent>,
    ) -> Result<AgentActivity, &'static str> {
        let expected = match signal {
            QUESTION_START => ("PreToolUse", AgentActivity::Blocked),
            QUESTION_END => ("PostToolUse", AgentActivity::Working),
            _ => {
                if status == AgentActivity::Blocked {
                    self.other_blocked = true;
                } else {
                    *self = Self::default();
                }
                return Ok(status);
            }
        };
        let key = question_key(session, event, expected.0)
            .filter(|_| status == expected.1)
            .ok_or("question hook requires matching tool/event and session/turn/call metadata")?;
        if signal == QUESTION_START {
            if !self.pending.contains(&key) {
                if self.pending.len() < PENDING_LIMIT {
                    self.pending.push(key);
                } else {
                    self.overflow = true;
                }
            }
            return Ok(AgentActivity::Blocked);
        }
        let Some(index) = self.pending.iter().position(|pending| pending == &key) else {
            return Err("no matching pending question in this pane");
        };
        self.pending.remove(index);
        if self.question_waiting() || self.other_blocked {
            return Err("another question or uncorrelated blocked report remains pending");
        }
        Ok(AgentActivity::Working)
    }
}

fn question_key(
    session: Option<&str>,
    event: Option<&StatusHookEvent>,
    expected: &str,
) -> Option<Question> {
    let event = event?;
    if event.hook_event_name != expected || event.tool_name.as_deref() != Some("request_user_input")
    {
        return None;
    }
    let session = session.filter(|value| {
        !value.is_empty()
            && value.len() <= 128
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_.:/-".contains(&byte))
    })?;
    Some(Question {
        session: session.into(),
        turn: event.turn_id.as_ref()?.clone(),
        call: event.tool_use_id.as_ref()?.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event(name: &str, turn: &str, call: &str) -> StatusHookEvent {
        StatusHookEvent::from_value(&json!({"hook_event_name":name,
            "tool_name":"request_user_input", "turn_id":turn, "tool_use_id":call}))
        .unwrap()
    }

    fn start(state: &mut CodexRecovery, turn: &str, call: &str) {
        assert_eq!(
            state.report(
                QUESTION_START,
                AgentActivity::Blocked,
                Some("session-1"),
                Some(&event("PreToolUse", turn, call))
            ),
            Ok(AgentActivity::Blocked)
        );
    }

    #[test]
    fn question_completion_requires_the_same_session_turn_and_call() {
        let mut state = CodexRecovery::default();
        start(&mut state, "turn-1", "call-1");
        for (session, turn, call) in [
            ("session-2", "turn-1", "call-1"),
            ("session-1", "turn-2", "call-1"),
            ("session-1", "turn-1", "call-2"),
        ] {
            assert!(
                state
                    .report(
                        QUESTION_END,
                        AgentActivity::Working,
                        Some(session),
                        Some(&event("PostToolUse", turn, call))
                    )
                    .is_err()
            );
            assert!(state.question_waiting());
        }
        assert_eq!(
            state.report(
                QUESTION_END,
                AgentActivity::Working,
                Some("session-1"),
                Some(&event("PostToolUse", "turn-1", "call-1"))
            ),
            Ok(AgentActivity::Working)
        );
        assert!(!state.question_waiting());
    }

    #[test]
    fn question_completion_cannot_clear_another_question_or_unidentified_blocker() {
        let mut state = CodexRecovery::default();
        start(&mut state, "turn-1", "call-1");
        start(&mut state, "turn-1", "call-2");
        assert!(
            state
                .report(
                    QUESTION_END,
                    AgentActivity::Working,
                    Some("session-1"),
                    Some(&event("PostToolUse", "turn-1", "call-1"))
                )
                .is_err()
        );
        assert!(state.question_waiting());
        state
            .report(
                "blocked",
                AgentActivity::Blocked,
                Some("session-1"),
                Some(
                    &StatusHookEvent::from_value(&json!({"hook_event_name":"PermissionRequest",
                "tool_name":"Bash", "turn_id":"turn-1"}))
                    .unwrap(),
                ),
            )
            .unwrap();
        assert!(
            state
                .report(
                    QUESTION_END,
                    AgentActivity::Working,
                    Some("session-1"),
                    Some(&event("PostToolUse", "turn-1", "call-2"))
                )
                .is_err()
        );
    }

    #[test]
    fn question_reports_require_complete_metadata_and_the_exact_tool_event() {
        for signal in [QUESTION_START, QUESTION_END] {
            let mut state = CodexRecovery::default();
            assert!(
                state
                    .report(signal, AgentActivity::Working, Some("session-1"), None)
                    .is_err()
            );
        }
        let mut state = CodexRecovery::default();
        start(&mut state, "turn-1", "call-1");
        for change in [
            json!({"tool_name":"Bash"}),
            json!({"tool_use_id":null}),
            json!({"turn_id":null}),
            json!({"hook_event_name":"PreToolUse"}),
        ] {
            let mut value = serde_json::to_value(event("PostToolUse", "turn-1", "call-1")).unwrap();
            for (key, value_change) in change.as_object().unwrap() {
                value[key] = value_change.clone();
            }
            let event = StatusHookEvent::from_value(&value).unwrap();
            assert!(
                state
                    .report(
                        QUESTION_END,
                        AgentActivity::Working,
                        Some("session-1"),
                        Some(&event)
                    )
                    .is_err()
            );
        }
        assert!(
            state
                .report(
                    QUESTION_END,
                    AgentActivity::Working,
                    None,
                    Some(&event("PostToolUse", "turn-1", "call-1"))
                )
                .is_err()
        );
        assert!(state.question_waiting());
    }

    #[test]
    fn terminal_reports_and_explicit_agent_reports_retire_pending_questions() {
        for (signal, status) in [
            ("done", AgentActivity::Done),
            ("idle", AgentActivity::Idle),
            ("working", AgentActivity::Working),
        ] {
            let mut state = CodexRecovery::default();
            start(&mut state, "turn-1", "call-1");
            state
                .report(signal, status, Some("session-1"), None)
                .unwrap();
            assert!(!state.question_waiting());
            assert!(
                state
                    .report(
                        QUESTION_END,
                        AgentActivity::Working,
                        Some("session-1"),
                        Some(&event("PostToolUse", "turn-1", "call-1"))
                    )
                    .is_err()
            );
        }
    }

    #[test]
    fn duplicate_question_reports_are_idempotent_and_pending_storage_is_bounded() {
        let mut state = CodexRecovery::default();
        start(&mut state, "turn-1", "call-1");
        start(&mut state, "turn-1", "call-1");
        assert_eq!(
            state.report(
                QUESTION_END,
                AgentActivity::Working,
                Some("session-1"),
                Some(&event("PostToolUse", "turn-1", "call-1"))
            ),
            Ok(AgentActivity::Working)
        );
        assert!(
            state
                .report(
                    QUESTION_END,
                    AgentActivity::Working,
                    Some("session-1"),
                    Some(&event("PostToolUse", "turn-1", "call-1"))
                )
                .is_err()
        );
        for index in 0..32 {
            start(&mut state, "turn-2", &format!("call-{index}"));
        }
        for index in 0..32 {
            assert!(
                state
                    .report(
                        QUESTION_END,
                        AgentActivity::Working,
                        Some("session-1"),
                        Some(&event("PostToolUse", "turn-2", &format!("call-{index}")))
                    )
                    .is_err()
            );
        }
        assert!(
            state.question_waiting(),
            "overflow must remain conservative until a lifecycle report"
        );
    }
}
