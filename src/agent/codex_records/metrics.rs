//! Metrics projection of the shared rollout vocabulary. No conversation content
//! is copied: callers retain only these bounded numeric fields and labels.

use jiff::Timestamp;
use serde_json::Value;

use crate::agent::codex_metrics::{LimitState, Mode, Tokens, Tool, ToolStatus};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Usage {
        at: Timestamp,
        turn_id: String,
        request: Option<Tokens>,
        turn: Option<Tokens>,
        thread: Option<Tokens>,
    },
    Legacy {
        at: Timestamp,
        last: Option<Tokens>,
        thread: Option<Tokens>,
        window: Option<u64>,
        bucket: Option<String>,
        limits: Option<LimitState>,
    },
    Context {
        at: Timestamp,
        turn_id: Option<String>,
        mode: Mode,
    },
    Compacted {
        at: Timestamp,
        number: Option<u64>,
    },
    Tool {
        id: String,
        tool: Tool,
    },
}

pub fn decode(value: &Value, expected_thread: &str) -> Option<Event> {
    let at = value["timestamp"].as_str()?.parse::<Timestamp>().ok()?;
    let payload = &value["payload"];
    if ["thread_id", "session_id"].iter().any(|field| {
        payload[*field]
            .as_str()
            .is_some_and(|id| id != expected_thread)
    }) {
        return None;
    }
    match (value["type"].as_str()?, payload["type"].as_str()) {
        ("token_usage_record", _) => {
            if payload["thread_id"].as_str() != Some(expected_thread)
                || payload["session_id"].as_str() != Some(expected_thread)
            {
                return None;
            }
            Some(Event::Usage {
                at,
                turn_id: bounded_id(&payload["turn_id"])?,
                request: tokens(&payload["usage"]),
                turn: tokens(&payload["turn_token_usage"]),
                thread: tokens(&payload["thread_token_usage"]),
            })
        }
        ("event_msg", Some("token_count")) => Some(Event::Legacy {
            at,
            last: tokens(&payload["info"]["last_token_usage"]),
            thread: tokens(&payload["info"]["total_token_usage"]),
            window: number(&payload["info"]["model_context_window"]).filter(|n| *n > 0),
            bucket: label(&payload["rate_limits"]["limit_id"]),
            limits: payload["rate_limits"].is_object().then(|| {
                if payload["rate_limits"]["primary"].is_object()
                    || payload["rate_limits"]["secondary"].is_object()
                {
                    LimitState::UnsupportedWindow
                } else {
                    LimitState::NotReported
                }
            }),
        }),
        ("turn_context", _) => Some(Event::Context {
            at,
            turn_id: bounded_id(&payload["turn_id"]),
            mode: Mode {
                model: label(&payload["model"]),
                effort: label(&payload["effort"]),
                approval: label(&payload["approval_policy"]),
                sandbox: label(&payload["sandbox_policy"]["type"]),
            },
        }),
        ("compacted", _) => Some(Event::Compacted {
            at,
            number: number(&payload["window_number"]),
        }),
        ("event_msg", Some("item_completed")) => completed(&payload["item"], at),
        ("response_item", Some("function_call" | "custom_tool_call")) => Some(Event::Tool {
            id: identity(payload)?,
            tool: Tool {
                name: label(&payload["name"]),
                status: ToolStatus::Started,
                at,
                elapsed_ms: None,
            },
        }),
        ("response_item", Some("function_call_output" | "custom_tool_call_output")) => {
            Some(Event::Tool {
                id: identity(payload)?,
                tool: Tool {
                    name: None,
                    status: ToolStatus::Completed,
                    at,
                    elapsed_ms: None,
                },
            })
        }
        _ => None,
    }
}

fn identity(value: &Value) -> Option<String> {
    super::identity_ref(value)
        .filter(|id| id.len() <= 128 && !id.chars().any(char::is_control))
        .map(str::to_owned)
}

fn bounded_id(value: &Value) -> Option<String> {
    value
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 128 && !s.chars().any(char::is_control))
        .map(str::to_owned)
}

fn completed(item: &Value, at: Timestamp) -> Option<Event> {
    let name = match item["type"].as_str()? {
        "CommandExecution" => Some("shell".to_owned()),
        "McpToolCall" => label(&item["server"])
            .zip(label(&item["tool"]))
            .map(|(s, t)| format!("{s}.{t}")),
        _ => return None,
    };
    let failed = item["status"] == "Failed"
        || item["error"].is_object()
        || item["exit_code"].as_i64().is_some_and(|code| code != 0);
    let elapsed_ms = number(&item["started_at_ms"])
        .zip(number(&item["completed_at_ms"]))
        .and_then(|(start, end)| end.checked_sub(start));
    Some(Event::Tool {
        id: identity(item)?,
        tool: Tool {
            name,
            status: if failed {
                ToolStatus::Failed
            } else {
                ToolStatus::Completed
            },
            at,
            elapsed_ms,
        },
    })
}

fn label(value: &Value) -> Option<String> {
    value
        .as_str()
        .filter(|s| {
            !s.is_empty()
                && s.len() <= 80
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.:/ -".contains(&b))
        })
        .map(str::to_owned)
}

fn number(value: &Value) -> Option<u64> {
    value.as_i64().filter(|n| *n >= 0).map(|n| n as u64)
}

fn tokens(value: &Value) -> Option<Tokens> {
    let result = Tokens {
        input: number(&value["input_tokens"]),
        cached_input: number(&value["cached_input_tokens"]),
        cache_write_input: number(&value["cache_write_input_tokens"]),
        output: number(&value["output_tokens"]),
        reasoning_output: number(&value["reasoning_output_tokens"]),
        total: number(&value["total_tokens"]),
    };
    (result != Tokens::default()).then_some(result)
}
