//! The metrics projection of one agy transcript record. Only labels,
//! token counts and tool names leave it: no prompt, response, reasoning,
//! tool arguments or tool output.

use jiff::Timestamp;
use serde_json::Value;

use super::Usage;

/// At most this many tool calls are taken from one record.
const MAX_BLOCKS: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolCall {
    pub name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// A planner response with token usage and tool calls.
    PlannerResponse {
        at: Timestamp,
        step_index: Option<u64>,
        usage: Option<Usage>,
        calls: Vec<ToolCall>,
    },
    /// A tool execution record (generic or typed).
    ToolResult {
        at: Timestamp,
        step_index: Option<u64>,
        failed: bool,
        record_type: Option<String>,
    },
    /// Context compaction checkpoint.
    Checkpoint {
        at: Timestamp,
        step_index: Option<u64>,
    },
    /// Model setting update from user settings change.
    Model {
        at: Option<Timestamp>,
        model: String,
    },
}

/// The event `value` contributes.
pub fn decode(value: &Value) -> Option<Event> {
    let typ = value["type"].as_str()?;
    match typ {
        "PLANNER_RESPONSE" => planner_response(value),
        "CHECKPOINT" => checkpoint(value),
        "USER_INPUT" => user_input(value),
        "CONVERSATION_HISTORY" | "EPHEMERAL_MESSAGE" | "SYSTEM_MESSAGE" => None,
        _ => tool_result(value),
    }
}

fn planner_response(value: &Value) -> Option<Event> {
    let at = timestamp(value)?;
    let step_index = value["step_index"].as_u64();
    let usage = usage(value);
    let calls = value["tool_calls"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(tool_call)
        .take(MAX_BLOCKS)
        .collect();
    Some(Event::PlannerResponse {
        at,
        step_index,
        usage,
        calls,
    })
}

fn tool_call(block: &Value) -> Option<ToolCall> {
    let raw_name = block["name"].as_str()?;
    let name = if raw_name == "call_mcp_tool" {
        decode_call_mcp_tool(block)
    } else {
        label(raw_name).map(|s| tool_name(&s))
    };
    Some(ToolCall { name })
}

fn unquote(s: &str) -> &str {
    s.trim_matches(|c| c == '"' || c == '\'')
}

fn decode_call_mcp_tool(block: &Value) -> Option<String> {
    let args = if block["args"].is_object() {
        &block["args"]
    } else {
        &block["parameters"]
    };
    let server = unquote(args["ServerName"].as_str()?);
    if server != "spyc" {
        return Some("mcp".to_owned());
    }
    let tool = unquote(args["ToolName"].as_str()?);
    label(&format!("mcp_spyc_{tool}"))
}

/// An MCP tool's name carries its server's, which can be private: only
/// spyc's own server keeps its name.
fn tool_name(name: &str) -> String {
    if let Some(rest) = name.strip_prefix("mcp_")
        && !rest.starts_with("spyc_")
    {
        return "mcp".to_owned();
    }
    name.to_owned()
}

fn tool_result(value: &Value) -> Option<Event> {
    let at = timestamp(value)?;
    let step_index = value["step_index"].as_u64();
    let status = value["status"].as_str();
    let typ = value["type"].as_str().unwrap_or_default();
    let failed = matches!(status, Some("ERROR" | "INVALID")) || typ == "ERROR_MESSAGE";
    // A `RUNNING` record is the last one a backgrounded call ever gets (no
    // `step_index` repeats), so it means the call returned.
    if status.is_some() || typ == "ERROR_MESSAGE" {
        let record_type = if typ == "GENERIC" || typ == "ERROR_MESSAGE" {
            None
        } else {
            Some(typ.to_owned())
        };
        Some(Event::ToolResult {
            at,
            step_index,
            failed,
            record_type,
        })
    } else {
        None
    }
}

/// Older agy writes a leading `{{ CHECKPOINT 0 }}` after the first turn, which
/// is not a compaction. Newer agy writes no header, and a compaction's summary
/// can contain that text, so only the prefix counts.
fn is_initial_checkpoint(content: &str) -> bool {
    let trimmed = content.trim_start();
    if let Some(rest) = trimmed.strip_prefix("{{") {
        let rest = rest.trim_start();
        if let Some(after) = rest.strip_prefix("CHECKPOINT 0") {
            let after = after.trim_start();
            return after.starts_with('}');
        }
    }
    false
}

fn checkpoint(value: &Value) -> Option<Event> {
    let at = timestamp(value)?;
    let step_index = value["step_index"].as_u64();
    if let Some(content) = value["content"].as_str()
        && is_initial_checkpoint(content)
    {
        return None;
    }
    Some(Event::Checkpoint { at, step_index })
}

fn user_input(value: &Value) -> Option<Event> {
    let content = value["content"].as_str()?;
    let model = model_from_settings(content)?;
    let at = timestamp(value);
    Some(Event::Model { at, model })
}

fn model_from_settings(content: &str) -> Option<String> {
    let start = content.find("<USER_SETTINGS_CHANGE>")?;
    let end = content[start..].find("</USER_SETTINGS_CHANGE>")?;
    let block = &content[start..start + end];
    let marker = "The user changed setting `Model Selection` from ";
    let idx = block.find(marker)?;
    let after_from = &block[idx + marker.len()..];
    let to_idx = after_from.find(" to ")?;
    let after_to = &after_from[to_idx + 4..];
    let end_idx = after_to
        .find(". No need")
        .or_else(|| after_to.find(".\n"))
        .or_else(|| after_to.find('\n'))
        .unwrap_or(after_to.len());
    let raw = after_to[..end_idx].trim();
    label(raw)
}

/// All three counts, or none: a usage missing one is not a measurement. agy's
/// `input_tokens` excludes cache reads, as [`Usage::input`] does, and agy
/// reports no cache writes.
fn usage(value: &Value) -> Option<Usage> {
    Some(Usage {
        input: value["input_tokens"].as_u64()?,
        cache_write: 0,
        cache_read: value["cache_read_tokens"].as_u64()?,
        output: value["output_tokens"].as_u64()?,
    })
}

fn timestamp(value: &Value) -> Option<Timestamp> {
    if let Some(created_at) = value["created_at"].as_str()
        && let Ok(ts) = created_at.parse()
    {
        return Some(ts);
    }
    value["timestamp"].as_str()?.parse().ok()
}

fn label(value: &str) -> Option<String> {
    (!value.is_empty()
        && value.len() <= 80
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.:/ ()-".contains(&b)))
    .then(|| value.to_owned())
}
