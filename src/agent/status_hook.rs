//! The lifecycle metadata a status hook reports, bounded and sanitized for
//! diagnostics: event names, tools and ids, never hook arguments or response
//! content. A pane keeps its last [`HISTORY_LIMIT`] events.

mod input;

pub use input::read_payload;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const HISTORY_LIMIT: usize = 8;
/// Cap on normalized metadata, not on raw hook arguments or responses.
pub const PAYLOAD_LIMIT: usize = 8192;

/// Why agy's `Stop` ended the turn (`exa.hooks_pb.StopHookArgs` on agy 1.4.5:
/// `NO_TOOL_CALL`, `USER_CANCELED`, `ERROR`, `MAX_ITERATIONS`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerminationReason {
    #[serde(rename = "NO_TOOL_CALL")]
    NoToolCall,
    #[serde(rename = "USER_CANCELED")]
    UserCancelled,
    #[serde(rename = "ERROR")]
    Error,
    #[serde(rename = "MAX_ITERATIONS")]
    MaxIterations,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct StatusHookEvent {
    pub hook_event_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notification_type: Option<String>,
    /// Why agy's `Stop` ended the turn.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub termination_reason: Option<TerminationReason>,
    /// Whether agy's `Stop` payload reported the session as completely idle
    /// (`true`) or still waiting on background subagents, tasks or scheduled
    /// timers (`false`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fully_idle: Option<bool>,
    /// How many entries Claude Code's `Stop` payload listed as in-flight
    /// background work (shells, subagents, monitors); `None` when the payload
    /// has no such list, which is not the same as an empty one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background_tasks: Option<u32>,
    /// How many session-scoped wakeups (`CronCreate`, `ScheduleWakeup`,
    /// `/loop`) the same payload listed; `None` when it has no such list.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_crons: Option<u32>,
}

impl StatusHookEvent {
    pub fn from_payload(payload: &str) -> Option<Self> {
        if payload.len() > PAYLOAD_LIMIT {
            return None;
        }
        Self::from_value(&serde_json::from_str::<Value>(payload).ok()?)
    }

    pub fn from_value(value: &Value) -> Option<Self> {
        let count = |key: &str| {
            value[key]
                .as_u64()
                .map(|n| u32::try_from(n).unwrap_or(u32::MAX))
        };
        Some(Self {
            hook_event_name: identifier(&value["hook_event_name"], 64)?,
            tool_name: identifier(&value["tool_name"], 128),
            turn_id: identifier(&value["turn_id"], 128),
            tool_use_id: identifier(&value["tool_use_id"], 128),
            notification_type: identifier(&value["notification_type"], 64),
            termination_reason: TerminationReason::deserialize(&value["termination_reason"]).ok(),
            fully_idle: value["fully_idle"].as_bool(),
            background_tasks: count("background_tasks"),
            session_crons: count("session_crons"),
        })
    }

    /// Whether agy's `Stop` ended because the user cancelled the turn.
    pub fn user_cancelled(&self) -> bool {
        self.termination_reason == Some(TerminationReason::UserCancelled)
    }

    /// Whether the agent said work it started will wake it again: `Some(true)`
    /// when Claude Code's lists have an entry or agy reported `fullyIdle:
    /// false`, `Some(false)` when the payload carried those fields and nothing
    /// is pending, `None` when it carried neither.
    pub fn wakes_pending(&self) -> Option<bool> {
        match (self.background_tasks, self.session_crons) {
            (None, None) => self.fully_idle.map(|idle| !idle),
            (tasks, crons) => Some(tasks.unwrap_or(0) > 0 || crons.unwrap_or(0) > 0),
        }
    }

    /// What the pending wakes are, for a hook-inferred `waiting`'s note:
    /// "2 background tasks", "1 scheduled wakeup", both, or "background work"
    /// when agy reports `fullyIdle: false`.
    pub fn wakes_summary(&self) -> Option<String> {
        let part = |n: Option<u32>, one: &str, many: &str| match n {
            Some(1) => Some(format!("1 {one}")),
            Some(n) if n > 1 => Some(format!("{n} {many}")),
            _ => None,
        };
        let parts: Vec<String> = [
            part(self.background_tasks, "background task", "background tasks"),
            part(self.session_crons, "scheduled wakeup", "scheduled wakeups"),
        ]
        .into_iter()
        .flatten()
        .collect();
        if !parts.is_empty() {
            Some(parts.join(", "))
        } else if self.fully_idle == Some(false) {
            Some("background work".to_string())
        } else {
            None
        }
    }
}

fn identifier(value: &Value, limit: usize) -> Option<String> {
    let text = value.as_str()?;
    (!text.is_empty()
        && text.len() <= limit
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.:/-".contains(&byte)))
    .then(|| text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn status_hook_metadata_preserves_correlation_fields_without_content() {
        let payload = json!({
            "hook_event_name": "PostToolUse", "tool_name": "request_user_input",
            "turn_id": "turn-1", "tool_use_id": "call_1",
            "tool_input": {"questions": ["private-question"]},
            "tool_response": "private-answer", "prompt": "private-prompt",
            "cwd": "/private/project", "last_assistant_message": "private-response"
        });
        let event = StatusHookEvent::from_payload(&payload.to_string()).unwrap();
        assert_eq!(
            serde_json::to_value(event).unwrap(),
            json!({
                "hook_event_name": "PostToolUse", "tool_name": "request_user_input",
                "turn_id": "turn-1", "tool_use_id": "call_1"
            })
        );
    }

    #[test]
    fn status_hook_metadata_does_not_invent_permission_call_ids() {
        let event = StatusHookEvent::from_value(&json!({
            "hook_event_name": "PermissionRequest", "tool_name": "Bash", "turn_id": "turn-1",
            "tool_input": {"command": "private-command"}
        }))
        .unwrap();
        assert_eq!(event.tool_use_id, None);
        assert_eq!(event.tool_name.as_deref(), Some("Bash"));
    }

    #[test]
    fn status_hook_metadata_bounds_and_sanitizes_untrusted_fields() {
        for value in [
            json!(null),
            json!([]),
            json!({}),
            json!({"hook_event_name": 7}),
            json!({"hook_event_name": "Stop\u{1b}[31m"}),
            json!({"hook_event_name": "x".repeat(65)}),
        ] {
            assert!(StatusHookEvent::from_value(&value).is_none(), "{value}");
        }
        let event = StatusHookEvent::from_value(&json!({
            "hook_event_name": "FutureEvent", "tool_name": "\u{1b}[31m", "turn_id": "x".repeat(129),
            "tool_use_id": "has\nnewline", "notification_type": "idle_prompt"
        }))
        .unwrap();
        assert_eq!(event.tool_name, None);
        assert_eq!(event.turn_id, None);
        assert_eq!(event.tool_use_id, None);
        assert_eq!(event.notification_type.as_deref(), Some("idle_prompt"));
        assert!(StatusHookEvent::from_payload("{partial").is_none());
        assert!(StatusHookEvent::from_payload(&" ".repeat(PAYLOAD_LIMIT + 1)).is_none());
    }

    #[test]
    fn agy_hook_metadata_infers_event_names_and_roundtrips_through_wire_json() {
        let parse = |raw: &str| {
            let normalized = read_payload(raw.as_bytes()).unwrap();
            let event = StatusHookEvent::from_payload(&normalized).unwrap();
            let wire = serde_json::to_value(&event).unwrap();
            assert_eq!(StatusHookEvent::from_value(&wire).as_ref(), Some(&event));
            (event, wire)
        };

        let (pre, wire) = parse(
            r#"{"conversationId":"conv-1","invocationNum":3,"initialNumSteps":10,"modelName":"auto"}"#,
        );
        assert_eq!(wire, json!({"hook_event_name": "PreInvocation"}));
        assert_eq!(pre.wakes_pending(), None);

        let (_, wire) = parse(
            r#"{"conversationId":"conv-1","toolCall":{"name":"ask_question","args":{"questions":["private"]}},"stepIdx":19}"#,
        );
        assert_eq!(
            wire,
            json!({"hook_event_name": "PreToolUse", "tool_name": "ask_question"})
        );

        let (waiting_stop, wire) = parse(
            r#"{"conversationId":"conv-1","executionNum":1,"terminationReason":"NO_TOOL_CALL","error":"","fullyIdle":false}"#,
        );
        assert_eq!(
            wire,
            json!({
                "hook_event_name": "Stop",
                "termination_reason": "NO_TOOL_CALL",
                "fully_idle": false
            })
        );
        assert_eq!(waiting_stop.wakes_pending(), Some(true));
        assert!(!waiting_stop.user_cancelled());
        assert_eq!(
            waiting_stop.wakes_summary().as_deref(),
            Some("background work")
        );

        let (idle_stop, _) = parse(
            r#"{"conversationId":"conv-1","executionNum":1,"terminationReason":"USER_CANCELED","error":"cancelled","fullyIdle":true}"#,
        );
        assert!(idle_stop.user_cancelled());
        assert_eq!(idle_stop.wakes_pending(), Some(false));
        assert_eq!(idle_stop.wakes_summary(), None);
    }
}
