use serde::Serialize;
use serde_json::Value;

pub const HISTORY_LIMIT: usize = 8;
pub const PAYLOAD_LIMIT: usize = 8192;

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
}

impl StatusHookEvent {
    pub fn from_payload(payload: &str) -> Option<Self> {
        if payload.len() > PAYLOAD_LIMIT {
            return None;
        }
        Self::from_value(&serde_json::from_str::<Value>(payload).ok()?)
    }

    pub fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            hook_event_name: identifier(&value["hook_event_name"], 64)?,
            tool_name: identifier(&value["tool_name"], 128),
            turn_id: identifier(&value["turn_id"], 128),
            tool_use_id: identifier(&value["tool_use_id"], 128),
            notification_type: identifier(&value["notification_type"], 64),
        })
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
}
