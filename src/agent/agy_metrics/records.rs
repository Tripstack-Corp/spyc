//! The metrics projection of one agy transcript record. Only labels,
//! token counts, tool names and a bounded latest reasoning step leave it:
//! no prompt, response, tool arguments or tool output.

use jiff::Timestamp;
use serde_json::Value;

use super::Usage;

/// At most this many tool calls are taken from one record.
const MAX_BLOCKS: usize = 64;
/// At most this many bytes of a reasoning line are kept, from its end.
const MAX_REASONING_BYTES: usize = 512;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolCall {
    pub name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// A planner response with token usage, latest reasoning step and tool calls.
    PlannerResponse {
        at: Timestamp,
        step_index: Option<u64>,
        usage: Option<Usage>,
        reasoning: Option<String>,
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
    /// The user's prompt, which starts a turn.
    UserInput {
        at: Option<Timestamp>,
        step_index: Option<u64>,
    },
    /// Model setting update from user settings change.
    Model {
        at: Option<Timestamp>,
        step_index: Option<u64>,
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
    let reasoning = value["thinking"].as_str().and_then(clean_reasoning);
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
        reasoning,
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

/// A model change, or the user's own prompt; as in `^a v`, only that starts a
/// turn.
fn user_input(value: &Value) -> Option<Event> {
    let at = timestamp(value);
    let step_index = value["step_index"].as_u64();
    if let Some(model) = value["content"].as_str().and_then(model_from_settings) {
        return Some(Event::Model {
            at,
            step_index,
            model,
        });
    }
    (value["source"].as_str() == Some("USER_EXPLICIT"))
        .then_some(Event::UserInput { at, step_index })
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

/// The last line of a planner response's `thinking` that has words in it, as
/// plain text: markdown markers stripped, whitespace collapsed, and at most
/// [`MAX_REASONING_BYTES`] kept from its end.
fn clean_reasoning(raw: &str) -> Option<String> {
    let line = raw.lines().filter_map(clean_reasoning_line).next_back()?;
    Some(bound_reasoning_tail(line, MAX_REASONING_BYTES))
}

fn clean_reasoning_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with("```") || is_truncation_marker(trimmed) {
        return None;
    }
    let body = trimmed.trim_start_matches(['#', '>', '-', '*', ' ', '\t']);
    let mut out = String::with_capacity(body.len().min(MAX_REASONING_BYTES));
    let mut prev_space = false;
    for token in body.split("**") {
        for ch in token.chars() {
            if ch.is_whitespace() {
                if !out.is_empty() && !prev_space {
                    out.push(' ');
                    prev_space = true;
                }
            } else if !ch.is_control() {
                out.push(ch);
                prev_space = false;
            }
        }
    }
    if prev_space {
        out.pop();
    }
    (!out.is_empty()).then_some(out)
}

/// agy's own line where it cut a long field: `<truncated 6726 bytes>`.
fn is_truncation_marker(s: &str) -> bool {
    s.starts_with("<truncated") && s.ends_with('>')
}

fn bound_reasoning_tail(mut s: String, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s;
    }
    const ELLIPSIS: &str = "…";
    let budget = max_bytes.saturating_sub(ELLIPSIS.len());
    let mut start = s.len().saturating_sub(budget);
    while start < s.len() && !s.is_char_boundary(start) {
        start += 1;
    }
    let tail = s.split_off(start);
    let trimmed = tail.trim_start();
    format!("{ELLIPSIS}{trimmed}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_reasoning_picks_latest_non_empty_line_and_strips_markdown() {
        let raw = "**Analyzing Transcript State**\n\n- Checking `scan_image_candidates` bounds\n<truncated 6726 bytes>\n\n";
        assert_eq!(
            clean_reasoning(raw).as_deref(),
            Some("Checking `scan_image_candidates` bounds")
        );
    }

    #[test]
    fn clean_reasoning_falls_back_to_single_heading_line() {
        assert_eq!(
            clean_reasoning("**Planning HUD layout**").as_deref(),
            Some("Planning HUD layout")
        );
        assert_eq!(clean_reasoning("   \n```\n<truncated 336 bytes>\n"), None);
    }

    #[test]
    fn clean_reasoning_bounds_overlong_line_from_tail() {
        let raw = format!("prefix {} tail-marker", "word ".repeat(200));
        let cleaned = clean_reasoning(&raw).expect("non-empty reasoning");
        assert!(cleaned.len() <= MAX_REASONING_BYTES);
        assert!(cleaned.starts_with('…'));
        assert!(cleaned.ends_with("tail-marker"));
    }
}
