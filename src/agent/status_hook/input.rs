//! Read complete hook JSON without retaining tool arguments or responses.
//!
//! serde's `IgnoredAny` validates and skips the document without allocating its
//! strings. This reader observes only root metadata fields, `toolCall.name`, and
//! the entry count of two root arrays, while bytes pass by.
//! Fixed field slots, a bounded token buffer and a depth limit also bound memory
//! for huge keys, oversized metadata and deeply nested ignored content.

use std::io::{self, Read};

use serde::{Deserialize, de::IgnoredAny};
use serde_json::{Map, Value};

#[derive(Clone, Copy)]
#[repr(usize)]
enum Field {
    HookEventName,
    ToolName,
    TurnId,
    ToolUseId,
    NotificationType,
    SessionId,
    ConversationId,
    TerminationReason,
}

impl Field {
    const ALL: [Self; 8] = [
        Self::HookEventName,
        Self::ToolName,
        Self::TurnId,
        Self::ToolUseId,
        Self::NotificationType,
        Self::SessionId,
        Self::ConversationId,
        Self::TerminationReason,
    ];

    fn from_key(key: &str) -> Option<Self> {
        match key {
            "hook_event_name" => Some(Self::HookEventName),
            "tool_name" => Some(Self::ToolName),
            "turn_id" => Some(Self::TurnId),
            "tool_use_id" => Some(Self::ToolUseId),
            "notification_type" => Some(Self::NotificationType),
            "session_id" => Some(Self::SessionId),
            "conversationId" => Some(Self::ConversationId),
            "terminationReason" => Some(Self::TerminationReason),
            _ => None,
        }
    }

    const fn wire_name(self) -> &'static str {
        match self {
            Self::HookEventName => "hook_event_name",
            Self::ToolName => "tool_name",
            Self::TurnId => "turn_id",
            Self::ToolUseId => "tool_use_id",
            Self::NotificationType => "notification_type",
            Self::SessionId => "session_id",
            Self::ConversationId => "conversationId",
            Self::TerminationReason => "termination_reason",
        }
    }
}

/// Root non-negative integer fields from agy's mutually exclusive
/// `exa.hooks_pb.HookArgs` `oneof` (`PreInvocationHookArgs.invocationNum`,
/// `StopHookArgs.executionNum`, `PreToolHookArgs.stepIdx`; confirmed by probe
/// on agy 1.4.5), which identify the hook event when `hook_event_name` is
/// absent.
#[derive(Clone, Copy)]
#[repr(usize)]
enum IntField {
    InvocationNum,
    ExecutionNum,
    StepIdx,
}

impl IntField {
    const COUNT: usize = 3;

    fn from_key(key: &str) -> Option<Self> {
        match key {
            "invocationNum" => Some(Self::InvocationNum),
            "executionNum" => Some(Self::ExecutionNum),
            "stepIdx" => Some(Self::StepIdx),
            _ => None,
        }
    }
}

/// Root arrays kept as their count of object entries, never their content:
/// Claude Code's `Stop` lists in-flight background work and scheduled wakeups.
const COUNTED: [&str; 2] = ["background_tasks", "session_crons"];
const TOKEN_LIMIT: usize = 1024; // Includes quotes and up to six bytes per ASCII escape.
const STRING_LIMIT: usize = 128;
const DEPTH_LIMIT: usize = 128;

/// Returns only bounded metadata from a valid root object, normalized to the
/// wire's field names. Content is never retained, including when parsing fails.
/// `PAYLOAD_LIMIT` applies to this small normalized result, rather than to the
/// hook's potentially large stdin body.
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
    let agy_tool_call = input.tool_call_name.is_some();
    if !input.has_field(Field::ToolName) {
        *input.field_mut(Field::ToolName) = input.tool_call_name.take();
    }
    if !input.has_field(Field::HookEventName) {
        *input.field_mut(Field::HookEventName) = if input.has_field(Field::TerminationReason)
            || input.fully_idle.is_some()
            || input.int(IntField::ExecutionNum).is_some()
        {
            Some("Stop".to_owned())
        } else if agy_tool_call || input.int(IntField::StepIdx).is_some() {
            Some("PreToolUse".to_owned())
        } else if input.int(IntField::InvocationNum).is_some() {
            Some("PreInvocation".to_owned())
        } else {
            None
        };
    }
    let mut fields: Map<String, Value> = Field::ALL
        .into_iter()
        .filter_map(|field| {
            input.fields[field as usize]
                .take()
                .map(|value| (field.wire_name().to_owned(), Value::String(value)))
        })
        .chain(
            COUNTED
                .into_iter()
                .zip(input.counts)
                .filter_map(|(key, count)| count.map(|count| (key.to_owned(), Value::from(count)))),
        )
        .collect();
    if let Some(idle) = input.fully_idle {
        fields.insert("fully_idle".to_owned(), Value::Bool(idle));
    }
    serde_json::to_string(&fields).ok()
}

/// Which root metadata slot the current root key's value populates.
#[derive(Clone, Copy)]
enum RootSlot {
    Field(Field),
    Int(IntField),
    Counted(usize),
    ToolCall,
    FullyIdle,
}

#[derive(Clone, Copy)]
enum Token {
    Ignored,
    Key,
    Field(Field),
    ToolCallKey,
    ToolCallName,
}

struct MetadataReader<R> {
    inner: R,
    fields: [Option<String>; Field::ALL.len()],
    ints: [Option<u32>; IntField::COUNT],
    counts: [Option<u32>; COUNTED.len()],
    tool_call_name: Option<String>,
    fully_idle: Option<bool>,
    slot: Option<RootSlot>,
    counting: Option<usize>,
    int_acc: Option<u32>,
    int_invalid: bool,
    in_tool_call: bool,
    nested_key_next: bool,
    nested_name_field: bool,
    awaiting_colon: bool,
    started: bool,
    root_object: bool,
    depth: usize,
    key_next: bool,
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
            ints: [None; IntField::COUNT],
            counts: [None; COUNTED.len()],
            tool_call_name: None,
            fully_idle: None,
            slot: None,
            counting: None,
            int_acc: None,
            int_invalid: false,
            in_tool_call: false,
            nested_key_next: false,
            nested_name_field: false,
            awaiting_colon: false,
            started: false,
            root_object: false,
            depth: 0,
            key_next: true,
            in_string: false,
            escaped: false,
            token: Token::Ignored,
            bytes: Vec::with_capacity(TOKEN_LIMIT),
            overflow: false,
        }
    }

    const fn has_field(&self, field: Field) -> bool {
        self.fields[field as usize].is_some()
    }

    const fn field_mut(&mut self, field: Field) -> &mut Option<String> {
        &mut self.fields[field as usize]
    }

    const fn int(&self, field: IntField) -> Option<u32> {
        self.ints[field as usize]
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

    fn select_root_key(&mut self, key: Option<&str>) {
        self.key_next = false;
        self.awaiting_colon = true;
        self.slot = key.and_then(|key| {
            if let Some(field) = Field::from_key(key) {
                Some(RootSlot::Field(field))
            } else if let Some(field) = IntField::from_key(key) {
                Some(RootSlot::Int(field))
            } else if let Some(idx) = COUNTED.iter().position(|counted| *counted == key) {
                Some(RootSlot::Counted(idx))
            } else if key == "toolCall" {
                Some(RootSlot::ToolCall)
            } else if key == "fullyIdle" {
                Some(RootSlot::FullyIdle)
            } else {
                None
            }
        });
        // Match JSON's last-key-wins behaviour even when the later
        // value has an invalid type or exceeds the metadata limit.
        match self.slot {
            Some(RootSlot::Field(field)) => *self.field_mut(field) = None,
            Some(RootSlot::Int(field)) => {
                self.ints[field as usize] = None;
                self.int_acc = None;
                self.int_invalid = false;
            }
            Some(RootSlot::Counted(counted)) => self.counts[counted] = None,
            Some(RootSlot::ToolCall) => self.tool_call_name = None,
            Some(RootSlot::FullyIdle) => self.fully_idle = None,
            None => {}
        }
    }

    fn finish_string(&mut self) {
        let decoded = (!self.overflow)
            .then(|| serde_json::from_slice::<String>(&self.bytes).ok())
            .flatten();
        match self.token {
            Token::Key => self.select_root_key(decoded.as_deref()),
            Token::Field(field) => {
                *self.field_mut(field) = decoded.filter(|value| value.len() <= STRING_LIMIT);
            }
            Token::ToolCallKey => {
                self.nested_key_next = false;
                self.nested_name_field = decoded.as_deref() == Some("name");
                if self.nested_name_field {
                    self.tool_call_name = None;
                }
            }
            Token::ToolCallName => {
                self.tool_call_name = decoded.filter(|value| value.len() <= STRING_LIMIT);
                self.nested_name_field = false;
            }
            Token::Ignored => {}
        }
        self.in_string = false;
    }

    fn observe_root_scalar(&mut self, byte: u8) {
        if self.depth != 1 || self.key_next {
            return;
        }
        if self.awaiting_colon {
            if byte == b':' {
                self.awaiting_colon = false;
            }
            return;
        }
        if byte.is_ascii_whitespace() || matches!(byte, b',' | b'}') {
            return;
        }
        match self.slot {
            Some(RootSlot::FullyIdle) => {
                self.fully_idle = match byte {
                    b't' => Some(true),
                    b'f' => Some(false),
                    _ => None,
                };
                self.slot = None;
            }
            Some(RootSlot::Int(_)) => {
                if byte.is_ascii_digit()
                    && !self.int_invalid
                    && let Some(next) = self
                        .int_acc
                        .unwrap_or(0)
                        .checked_mul(10)
                        .and_then(|n| n.checked_add(u32::from(byte - b'0')))
                {
                    self.int_acc = Some(next);
                } else {
                    self.int_invalid = true;
                    self.int_acc = None;
                }
            }
            _ => {}
        }
    }

    const fn finish_root_value(&mut self) {
        if let Some(RootSlot::Int(field)) = self.slot
            && !self.int_invalid
        {
            self.ints[field as usize] = self.int_acc.take();
        }
        self.key_next = true;
        self.awaiting_colon = false;
        self.slot = None;
        self.int_acc = None;
        self.int_invalid = false;
    }

    const fn string_token(&self) -> Token {
        if self.root_object && self.depth == 1 {
            if self.key_next {
                Token::Key
            } else if let Some(RootSlot::Field(field)) = self.slot {
                Token::Field(field)
            } else {
                Token::Ignored
            }
        } else if self.in_tool_call && self.depth == 2 {
            if self.nested_key_next {
                Token::ToolCallKey
            } else if self.nested_name_field {
                Token::ToolCallName
            } else {
                Token::Ignored
            }
        } else {
            Token::Ignored
        }
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
        self.observe_root_scalar(byte);
        match byte {
            b'"' => {
                self.token = self.string_token();
                self.in_string = true;
                self.escaped = false;
                self.bytes.clear();
                self.overflow = false;
                self.capture(byte);
            }
            b'{' | b'[' => {
                if byte == b'[' && self.depth == 1 && !self.key_next {
                    if let Some(RootSlot::Counted(counted)) = self.slot {
                        self.counting = Some(counted);
                        self.counts[counted] = Some(0);
                    }
                } else if byte == b'{'
                    && self.depth == 1
                    && !self.key_next
                    && matches!(self.slot, Some(RootSlot::ToolCall))
                {
                    self.in_tool_call = true;
                    self.nested_key_next = true;
                    self.nested_name_field = false;
                } else if self.depth == 2 {
                    self.nested_name_field = false;
                    if byte == b'{'
                        && let Some(counted) = self.counting
                    {
                        self.counts[counted] = self.counts[counted].map(|n| n.saturating_add(1));
                    }
                }
                self.depth += 1;
                if self.depth > DEPTH_LIMIT {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "hook JSON depth limit",
                    ));
                }
            }
            b'}' | b']' => {
                if byte == b'}' && self.depth == 1 {
                    self.finish_root_value();
                }
                self.depth = self.depth.saturating_sub(1);
                if self.depth <= 1 {
                    self.counting = None;
                    self.in_tool_call = false;
                    self.nested_name_field = false;
                }
            }
            b',' if self.depth == 1 => self.finish_root_value(),
            b',' if self.depth == 2 && self.in_tool_call => {
                self.nested_key_next = true;
                self.nested_name_field = false;
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

    /// Claude Code's `Stop` lists in-flight work with descriptions and shell
    /// command lines; only how many entries each list had survives.
    #[test]
    fn stop_work_lists_are_counted_without_their_content() {
        let json = r#"{"hook_event_name":"Stop","background_tasks":[
            {"id":"b1","type":"shell","status":"running","description":"private-desc","command":"private-cmd"},
            {"id":"b2","type":"monitor","status":"running","description":"[{x}]","server":"s"}
        ],"session_crons":[{"id":"c1","schedule":"0 9 * * *","recurring":true,"prompt":"private-prompt"}],
        "last_assistant_message":"private-response"}"#;
        assert_eq!(
            metadata(json),
            serde_json::json!({"hook_event_name":"Stop", "background_tasks":2, "session_crons":1})
        );
        assert_eq!(
            metadata(r#"{"hook_event_name":"Stop","background_tasks":[],"session_crons":[]}"#),
            serde_json::json!({"hook_event_name":"Stop", "background_tasks":0, "session_crons":0})
        );
    }

    /// An absent list stays absent: "no list" and "an empty list" mean
    /// different things to `waiting` (`StatusHookEvent::wakes_pending`).
    #[test]
    fn only_root_arrays_of_the_counted_keys_are_counted() {
        assert_eq!(
            metadata(
                r#"{"tool_input":{"background_tasks":[{"a":1}]},"session_crons":"none","background_tasks":{"x":[{}]},"hook_event_name":"Stop"}"#
            ),
            serde_json::json!({"hook_event_name":"Stop"}),
            "a nested look-alike, a string and an object are not lists"
        );
        assert_eq!(
            metadata(r#"{"background_tasks":[{"a":[{},{}]},"x",3,{}],"hook_event_name":"Stop"}"#),
            serde_json::json!({"hook_event_name":"Stop", "background_tasks":2}),
            "object entries only, and an entry's own nested objects are not entries"
        );
        assert_eq!(
            metadata(r#"{"background_tasks":[{},{}],"background_tasks":null}"#),
            serde_json::json!({}),
            "last key wins, even when its value is not a list"
        );
    }

    /// Confirmed against `exa.hooks_pb.HookArgs` protojson on agy 1.4.5,
    /// including protojson's variable spacing after colons and commas.
    #[test]
    fn agy_hook_payloads_normalize_to_wire_names_and_skip_tool_arguments() {
        let pre_invocation = r#"{"conversationId":"conv-1","workspacePaths":["/private/proj"],"invocationNum":3,"initialNumSteps":10,"modelName":"auto"}"#;
        assert_eq!(
            metadata(pre_invocation),
            serde_json::json!({"conversationId":"conv-1", "hook_event_name":"PreInvocation"})
        );

        let pre_tool = "{\n  \"conversationId\": \"conv-1\",\n  \"toolCall\": { \"args\": { \"name\": \"wrong\", \"questions\": [\"private-q\"] }, \"name\": \"ask_question\" },\n  \"stepIdx\": 19\n}";
        assert_eq!(
            metadata(pre_tool),
            serde_json::json!({
                "conversationId":"conv-1",
                "hook_event_name":"PreToolUse",
                "tool_name":"ask_question"
            })
        );

        let stop = r#"{"conversationId":"conv-1", "executionNum": 1, "terminationReason": "NO_TOOL_CALL", "error": "private-error", "fullyIdle": false}"#;
        assert_eq!(
            metadata(stop),
            serde_json::json!({
                "conversationId":"conv-1",
                "hook_event_name":"Stop",
                "termination_reason":"NO_TOOL_CALL",
                "fully_idle":false
            })
        );

        assert_eq!(
            metadata(r#"{"tool_name":"Bash"}"#),
            serde_json::json!({"tool_name":"Bash"}),
            "only agy's toolCall.name or stepIdx infers PreToolUse"
        );

        assert_eq!(
            metadata(
                r#"{"toolCall":{"name":"old"},"toolCall":null,"fullyIdle":true,"fullyIdle":"no","invocationNum":2,"invocationNum":-1,"stepIdx":4,"stepIdx":4.5,"executionNum":1,"executionNum":"1"}"#
            ),
            serde_json::json!({}),
            "duplicate agy fields follow last-key-wins and reject invalid types"
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
