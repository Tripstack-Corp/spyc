//! The metrics projection of one Claude Code transcript record. Only labels,
//! token counts and bounded ids leave it: no prompt, response, reasoning, tool
//! input or tool result.

use jiff::Timestamp;
use serde_json::Value;

use super::{Compaction, Plan, Usage};

/// At most this many tool calls, or results, are taken from one record.
const MAX_BLOCKS: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolCall {
    pub id: String,
    pub name: Option<String>,
    pub background: bool,
    /// `TodoWrite`'s list, when this call is one.
    pub plan: Option<Plan>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// One assistant record. Claude Code writes one per content block, each
    /// repeating its message's id and usage.
    Assistant {
        at: Timestamp,
        message_id: String,
        model: Option<String>,
        effort: Option<String>,
        usage: Option<Usage>,
        calls: Vec<ToolCall>,
    },
    /// A user record's tool results, as `(tool_use_id, is_error)`.
    Results {
        at: Timestamp,
        results: Vec<(String, bool)>,
    },
    /// A `permission-mode` record. It has no timestamp: its place in the file
    /// dates it.
    PermissionMode(String),
    Compaction {
        at: Timestamp,
        compaction: Compaction,
    },
}

/// The event `value` contributes, when it belongs to `expected_session`'s
/// main thread. A subagent's (`isSidechain`) records describe its own context,
/// not the session's, so they contribute nothing.
pub fn decode(value: &Value, expected_session: &str) -> Option<Event> {
    if value["sessionId"].as_str() != Some(expected_session) || value["isSidechain"] == true {
        return None;
    }
    match value["type"].as_str()? {
        "permission-mode" => label(&value["permissionMode"]).map(Event::PermissionMode),
        "assistant" => assistant(value),
        "user" => results(value),
        "system" if value["subtype"] == "compact_boundary" => Some(Event::Compaction {
            at: timestamp(value)?,
            compaction: Compaction {
                trigger: label(&value["compactMetadata"]["trigger"]),
                pre_tokens: number(&value["compactMetadata"]["preTokens"]),
            },
        }),
        _ => None,
    }
}

fn assistant(value: &Value) -> Option<Event> {
    let message = &value["message"];
    let calls = blocks(&message["content"], "tool_use")
        .filter_map(tool_call)
        .take(MAX_BLOCKS)
        .collect();
    Some(Event::Assistant {
        at: timestamp(value)?,
        message_id: bounded_id(&message["id"])?,
        model: label(&message["model"]),
        effort: label(&value["effort"]),
        usage: usage(&message["usage"]),
        calls,
    })
}

fn tool_call(block: &Value) -> Option<ToolCall> {
    let name = label(&block["name"]);
    let input = &block["input"];
    Some(ToolCall {
        id: bounded_id(&block["id"])?,
        background: input["run_in_background"] == true,
        plan: if name.as_deref() == Some("TodoWrite") {
            plan(&input["todos"])
        } else {
            None
        },
        name,
    })
}

fn plan(todos: &Value) -> Option<Plan> {
    let mut plan = Plan::default();
    for todo in todos.as_array()? {
        let slot = match todo["status"].as_str() {
            Some("pending") => &mut plan.pending,
            Some("in_progress") => &mut plan.in_progress,
            Some("completed") => &mut plan.completed,
            _ => continue,
        };
        *slot = slot.saturating_add(1);
    }
    Some(plan)
}

fn results(value: &Value) -> Option<Event> {
    let results: Vec<(String, bool)> = blocks(&value["message"]["content"], "tool_result")
        .filter_map(|block| {
            Some((
                bounded_id(&block["tool_use_id"])?,
                block["is_error"] == true,
            ))
        })
        .take(MAX_BLOCKS)
        .collect();
    if results.is_empty() {
        return None;
    }
    Some(Event::Results {
        at: timestamp(value)?,
        results,
    })
}

fn blocks<'a>(content: &'a Value, kind: &'a str) -> impl Iterator<Item = &'a Value> {
    content
        .as_array()
        .into_iter()
        .flatten()
        .filter(move |block| block["type"] == kind)
}

/// All four counts, or none: a usage missing one is not a measurement.
fn usage(value: &Value) -> Option<Usage> {
    Some(Usage {
        input: number(&value["input_tokens"])?,
        cache_write: number(&value["cache_creation_input_tokens"])?,
        cache_read: number(&value["cache_read_input_tokens"])?,
        output: number(&value["output_tokens"])?,
    })
}

fn timestamp(value: &Value) -> Option<Timestamp> {
    value["timestamp"].as_str()?.parse().ok()
}

fn bounded_id(value: &Value) -> Option<String> {
    value
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 128 && !s.chars().any(char::is_control))
        .map(str::to_owned)
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
    value.as_u64()
}
