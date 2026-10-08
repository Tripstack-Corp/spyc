//! Pure normalization of Codex rollout records shared by conversation consumers.

use serde_json::Value;

#[derive(Debug, PartialEq, Eq)]
pub struct Record {
    pub id: Option<String>,
    pub kind: RecordKind,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RecordKind {
    User(String),
    Agent(String),
    ToolCall { name: String, arguments: String },
    ToolOutput(String),
}

pub fn decode(value: &Value) -> Vec<Record> {
    let payload = &value["payload"];
    let id = identity(payload);
    let kind = match (value["type"].as_str(), payload["type"].as_str()) {
        (Some("event_msg"), Some("user_message")) => payload["message"]
            .as_str()
            .map(|text| RecordKind::User(text.to_owned())),
        (Some("event_msg"), Some("agent_message")) => payload["message"]
            .as_str()
            .map(|text| RecordKind::Agent(text.to_owned())),
        (Some("event_msg"), Some("item_completed")) => return completed_item(&payload["item"]),
        (Some("response_item"), Some("function_call" | "custom_tool_call")) => payload["name"]
            .as_str()
            .filter(|name| !name.is_empty())
            .map(|name| RecordKind::ToolCall {
                name: name.to_owned(),
                arguments: payload["arguments"]
                    .as_str()
                    .or_else(|| payload["input"].as_str())
                    .unwrap_or_default()
                    .to_owned(),
            }),
        (Some("response_item"), Some("function_call_output" | "custom_tool_call_output")) => {
            text_content(&payload["output"]).map(RecordKind::ToolOutput)
        }
        _ => None,
    };
    kind.map(|kind| Record { id, kind }).into_iter().collect()
}

fn identity(value: &Value) -> Option<String> {
    value["call_id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .or_else(|| value["id"].as_str().filter(|id| !id.is_empty()))
        .map(str::to_owned)
}

fn completed_item(item: &Value) -> Vec<Record> {
    let id = identity(item);
    match item["type"].as_str() {
        Some("UserMessage" | "AgentMessage") => {
            let Some(text) = text_content(&item["content"]) else {
                return Vec::new();
            };
            let kind = if item["type"] == "UserMessage" {
                RecordKind::User(text)
            } else {
                RecordKind::Agent(text)
            };
            vec![Record { id, kind }]
        }
        Some("CommandExecution") => {
            let Some(command) = command_text(&item["command"]) else {
                return Vec::new();
            };
            tool_records(
                id,
                "shell".to_owned(),
                command,
                text_content(&item["aggregated_output"]),
            )
        }
        Some("McpToolCall") => {
            let (Some(server), Some(tool)) = (item["server"].as_str(), item["tool"].as_str())
            else {
                return Vec::new();
            };
            if server.is_empty() || tool.is_empty() {
                return Vec::new();
            }
            let arguments = match &item["arguments"] {
                Value::Null => String::new(),
                Value::String(text) => text.clone(),
                other => other.to_string(),
            };
            tool_records(
                id,
                format!("{server}.{tool}"),
                arguments,
                text_content(&item["result"]),
            )
        }
        _ => Vec::new(),
    }
}

/// Recorded argv is quoted for display only; it is never interpreted or run.
/// Reject a malformed word rather than display a silently shortened command.
fn command_text(command: &Value) -> Option<String> {
    match command {
        Value::String(text) => Some(text.clone()),
        Value::Array(argv) if !argv.is_empty() => argv
            .iter()
            .map(|word| {
                shlex::try_quote(word.as_str()?)
                    .ok()
                    .map(std::borrow::Cow::into_owned)
            })
            .collect::<Option<Vec<_>>>()
            .map(|words| words.join(" ")),
        _ => None,
    }
}

fn tool_records(
    id: Option<String>,
    name: String,
    arguments: String,
    output: Option<String>,
) -> Vec<Record> {
    let mut records = vec![Record {
        id: id.clone(),
        kind: RecordKind::ToolCall { name, arguments },
    }];
    if let Some(output) = output {
        records.push(Record {
            id,
            kind: RecordKind::ToolOutput(output),
        });
    }
    records
}

fn text_content(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Array(parts) => {
            let text = parts
                .iter()
                .filter(|part| {
                    matches!(
                        part["type"].as_str(),
                        Some("text" | "Text" | "input_text" | "output_text")
                    )
                })
                .filter_map(|part| part["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n");
            (!text.is_empty()).then_some(text)
        }
        Value::Object(object) => object.get("content").and_then(text_content),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_and_non_conversation_records_are_ignored() {
        for record in [
            serde_json::json!(null),
            serde_json::json!({"type":"event_msg","payload":{"type":"item_started","item":{"type":"AgentMessage","content":[{"type":"Text","text":"unfinished"}]}}}),
            serde_json::json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"Reasoning","content":[{"type":"Text","text":"private"}]}}}),
            serde_json::json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"Unknown","content":[{"type":"Text","text":"unknown"}]}}}),
            serde_json::json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"UserMessage","content":[{"type":"image","url":"image"}]}}}),
            serde_json::json!({"type":"response_item","payload":{"type":"function_call","arguments":"missing name"}}),
            serde_json::json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"duplicate wire prose"}]}}),
        ] {
            assert!(decode(&record).is_empty(), "{record}");
        }
    }

    #[test]
    fn tool_call_and_result_share_the_call_identity_not_wire_record_ids() {
        let call = serde_json::json!({"type":"response_item","payload":{"type":"function_call","id":"wire-call","call_id":"call-1","name":"shell","arguments":"{}"}});
        let result = serde_json::json!({"type":"response_item","payload":{"type":"function_call_output","id":"wire-result","call_id":"call-1","output":"done"}});
        assert_eq!(decode(&call)[0].id.as_deref(), Some("call-1"));
        assert_eq!(decode(&result)[0].id.as_deref(), Some("call-1"));
        assert_eq!(
            decode(&result)[0].kind,
            RecordKind::ToolOutput("done".to_owned())
        );
    }
}
