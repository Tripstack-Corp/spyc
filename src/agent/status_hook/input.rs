//! Read complete hook JSON without retaining tool arguments or responses.
//!
//! serde's `IgnoredAny` validates and skips the document without allocating its
//! strings. This reader observes only root string fields while bytes pass by.
//! Fixed field slots, a bounded token buffer and a depth limit also bound memory
//! for huge keys, oversized metadata and deeply nested ignored content.

use std::io::{self, Read};

use serde::{Deserialize, de::IgnoredAny};
use serde_json::{Map, Value};

const FIELDS: [&str; 7] = [
    "hook_event_name",
    "tool_name",
    "turn_id",
    "tool_use_id",
    "notification_type",
    "session_id",
    "conversationId",
];
const TOKEN_LIMIT: usize = 1024; // Includes quotes and up to six bytes per ASCII escape.
const STRING_LIMIT: usize = 128;
const DEPTH_LIMIT: usize = 128;

/// Returns only bounded metadata from a valid root object. Content is never
/// retained, including when parsing fails. `PAYLOAD_LIMIT` applies to this small
/// normalized result, rather than to the hook's potentially large stdin body.
pub fn read_payload(reader: impl Read) -> Option<String> {
    let mut input = MetadataReader::new(reader);
    {
        let mut parser = serde_json::Deserializer::from_reader(&mut input);
        IgnoredAny::deserialize(&mut parser).ok()?;
        parser.end().ok()?;
    }
    if !input.root_object {
        return None;
    }
    let fields: Map<String, Value> = FIELDS
        .into_iter()
        .zip(input.fields)
        .filter_map(|(key, value)| value.map(|value| (key.to_owned(), Value::String(value))))
        .collect();
    serde_json::to_string(&fields).ok()
}

#[derive(Clone, Copy)]
enum Token {
    Ignored,
    Key,
    Field(usize),
}

struct MetadataReader<R> {
    inner: R,
    fields: [Option<String>; FIELDS.len()],
    started: bool,
    root_object: bool,
    depth: usize,
    key_next: bool,
    field: Option<usize>,
    in_string: bool,
    escaped: bool,
    token: Token,
    bytes: Vec<u8>,
    overflow: bool,
}

impl<R> MetadataReader<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            fields: std::array::from_fn(|_| None),
            started: false,
            root_object: false,
            depth: 0,
            key_next: true,
            field: None,
            in_string: false,
            escaped: false,
            token: Token::Ignored,
            bytes: Vec::with_capacity(TOKEN_LIMIT),
            overflow: false,
        }
    }

    fn capture(&mut self, byte: u8) {
        if matches!(self.token, Token::Ignored) || self.overflow {
            return;
        }
        if self.bytes.len() == TOKEN_LIMIT {
            self.bytes.clear();
            self.overflow = true;
        } else {
            self.bytes.push(byte);
        }
    }

    fn finish_string(&mut self) {
        let decoded = (!self.overflow)
            .then(|| serde_json::from_slice::<String>(&self.bytes).ok())
            .flatten();
        match self.token {
            Token::Key => {
                self.key_next = false;
                self.field = decoded.and_then(|key| FIELDS.iter().position(|field| *field == key));
                // Match JSON's last-key-wins behaviour even when the later
                // value has an invalid type or exceeds the metadata limit.
                if let Some(field) = self.field {
                    self.fields[field] = None;
                }
            }
            Token::Field(field) => {
                self.fields[field] = decoded.filter(|value| value.len() <= STRING_LIMIT);
            }
            Token::Ignored => {}
        }
        self.in_string = false;
    }

    fn observe(&mut self, byte: u8) -> io::Result<()> {
        if !self.started && !byte.is_ascii_whitespace() {
            self.started = true;
            self.root_object = byte == b'{';
        }
        if self.in_string {
            self.capture(byte);
            if self.escaped {
                self.escaped = false;
            } else if byte == b'\\' {
                self.escaped = true;
            } else if byte == b'"' {
                self.finish_string();
            }
            return Ok(());
        }
        match byte {
            b'"' => {
                self.token = if self.root_object && self.depth == 1 {
                    if self.key_next {
                        Token::Key
                    } else {
                        self.field.map_or(Token::Ignored, Token::Field)
                    }
                } else {
                    Token::Ignored
                };
                self.in_string = true;
                self.escaped = false;
                self.bytes.clear();
                self.overflow = false;
                self.capture(byte);
            }
            b'{' | b'[' => {
                self.depth += 1;
                if self.depth > DEPTH_LIMIT {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "hook JSON depth limit",
                    ));
                }
            }
            b'}' | b']' => self.depth = self.depth.saturating_sub(1),
            b',' if self.depth == 1 => {
                self.key_next = true;
                self.field = None;
            }
            _ => {}
        }
        Ok(())
    }
}

impl<R: Read> Read for MetadataReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = self.inner.read(buffer)?;
        for byte in &buffer[..count] {
            self.observe(*byte)?;
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata(json: &str) -> Value {
        serde_json::from_str(&read_payload(json.as_bytes()).expect("valid hook object")).unwrap()
    }

    #[test]
    fn large_nested_content_does_not_replace_root_metadata() {
        let body = "private-body-".repeat(10_000);
        let json = format!(
            r#"{{"tool_input":{{"turn_id":"wrong-turn","nested":[{{"text":"{body}"}}]}},"hook_event_name":"PreToolUse","turn_id":"turn-1","tool_use_id":"call-1"}}"#
        );
        assert_eq!(
            metadata(&json),
            serde_json::json!({
                "hook_event_name":"PreToolUse", "turn_id":"turn-1", "tool_use_id":"call-1"
            })
        );
    }

    #[test]
    fn escaped_keys_values_and_ignored_delimiters_are_handled() {
        assert_eq!(
            metadata(
                r#"{"ignored":"\"}[]{,","turn_\u0069d":"turn-\u0031","session_id":"session-1"}"#
            ),
            serde_json::json!({"turn_id":"turn-1", "session_id":"session-1"})
        );
    }

    #[test]
    fn oversize_keys_and_metadata_are_discarded_without_losing_other_fields() {
        let huge = "x".repeat(100_000);
        let json = format!(
            r#"{{"{huge}":"private-body","turn_id":"{huge}","session_id":"session-1","tool_name":"{}","hook_event_name":"Stop"}}"#,
            "x".repeat(129)
        );
        assert_eq!(
            metadata(&json),
            serde_json::json!({"session_id":"session-1", "hook_event_name":"Stop"})
        );
    }

    #[test]
    fn duplicate_fields_follow_last_value_and_invalid_types_are_ignored() {
        assert_eq!(
            metadata(
                r#"{"turn_id":"old","turn_id":{"secret":"content"},"session_id":null,"tool_name":5,"tool_use_id":["x"],"notification_type":true,"hook_event_name":"Stop"}"#
            ),
            serde_json::json!({"hook_event_name":"Stop"})
        );
        assert_eq!(
            metadata(r#"{"turn_id":"old","turn_id":"new"}"#),
            serde_json::json!({"turn_id":"new"})
        );
    }

    #[test]
    fn invalid_document_or_excessive_depth_never_produces_partial_metadata() {
        for json in [
            r#"{"hook_event_name":"Stop""#,
            r#"{"hook_event_name":"Stop"} trailing"#,
            "[]",
            "null",
        ] {
            assert!(read_payload(json.as_bytes()).is_none(), "{json}");
        }
        let deep = format!(
            r#"{{"hook_event_name":"Stop","tool_input":{}{}}}"#,
            "[".repeat(129),
            "]".repeat(129)
        );
        assert!(read_payload(deep.as_bytes()).is_none());
    }
}
