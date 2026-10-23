//! Read an Antigravity (`agy`) conversation transcript from its on-disk JSONL
//! into styled pager lines (`^a v`) or gallery image metadata (`^a g`).

use std::collections::{HashMap, HashSet};
use std::io::BufReader;
use std::path::{Path, PathBuf};

use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::ui::theme::Theme;

/// Resolve the conversation JSONL for the Agy session running in the pane.
///
/// Prefers the id pinned to THIS pane (agy's `conversationId`, reported by its
/// status hook), falling back to the session whose start time is closest to the
/// pane's spawn time. Proximity is a guess that collides whenever two panes start
/// within the same second — which is every `-r` restore.
///
/// A pinned id with no transcript on disk yet returns `None` rather than
/// proximity-matching: `None` makes `^a v` flash "no transcript found" and close,
/// which is the right answer for a conversation we've positively identified but
/// have nothing on disk for. Showing a *different* conversation's history — what
/// proximity-matching would do here — is worse than showing none.
/// The `command` field of the query is unused.
pub fn resolve_active_jsonl(q: crate::agent::TranscriptQuery) -> Option<PathBuf> {
    resolve_under_home(Path::new(&std::env::var_os("HOME")?), q)
}

/// [`resolve_active_jsonl`] with `home` passed in, so the resolution order is
/// testable without mutating the process environment.
fn resolve_under_home(home: &Path, q: crate::agent::TranscriptQuery) -> Option<PathBuf> {
    use crate::state::sessions::agy_transcript_path;

    if let Some(id) = q.session_id {
        let path = agy_transcript_path(home, id);
        return path.exists().then_some(path);
    }

    let best = crate::state::sessions::find_agy_sessions(q.cwd)
        .into_iter()
        .min_by_key(|s| s.started_at_secs.abs_diff(q.spawn_epoch_secs))?;
    Some(agy_transcript_path(home, &best.session_id))
}

/// Parse an Agy conversation JSONL into styled pager lines, in
/// chronological order. Returns empty on read failure. Model prose is
/// rendered through the Markdown viewer (`width` hints prose/table
/// reflow); user prompts and tool calls stay plain.
///
/// When a rendered record in `transcript.jsonl` marks `content` (or
/// `tool_calls` while `show_tool_calls` is on) in `truncated_fields`,
/// its untruncated counterpart is resolved by `step_index` from the
/// sibling `transcript_full.jsonl` when present.
pub fn render_transcript(
    path: &Path,
    theme: &Theme,
    width: Option<usize>,
    show_tool_calls: bool,
) -> Vec<Line<'static>> {
    let Ok(text) = crate::state::read_tail_lossy(path, crate::state::MAX_TRANSCRIPT_TAIL_BYTES)
    else {
        return Vec::new();
    };
    let (records, needed_steps) = parse_rendered_records(&text, show_tool_calls);
    let full_records = load_full_records(path, &needed_steps);

    let user_style = Style::default()
        .fg(theme.prompt_prefix)
        .add_modifier(Modifier::BOLD);
    let tool_style = Style::default().fg(theme.take);

    let mut out: Vec<Line<'static>> = Vec::new();
    let mut last_was_blank = true;
    let mut last_was_prose = false;

    for record in &records {
        let val = if needs_full_record(record, show_tool_calls)
            && let Some(step) = record["step_index"].as_u64()
            && let Some(full) = full_records.get(&step)
        {
            full
        } else {
            record
        };

        let source = val["source"].as_str().unwrap_or("");
        let msg_type = val["type"].as_str().unwrap_or("");

        if source == "USER_EXPLICIT" && msg_type == "USER_INPUT" {
            let content = val["content"].as_str().unwrap_or("");
            if content.is_empty() {
                continue;
            }
            let display_content = strip_user_request_tags(content).trim();
            crate::state::push_transcript_prompt(
                &mut out,
                &mut last_was_blank,
                display_content,
                user_style,
            );
            last_was_prose = false;
        } else if source == "MODEL" && msg_type == "PLANNER_RESPONSE" {
            if let Some(content) = val["content"].as_str()
                && !content.trim().is_empty()
            {
                crate::state::push_agent_markdown(
                    &mut out,
                    &mut last_was_blank,
                    content,
                    theme,
                    width,
                );
                last_was_prose = true;
            }

            if show_tool_calls
                && let Some(tool_calls) = val["tool_calls"].as_array()
                && !tool_calls.is_empty()
            {
                if last_was_prose {
                    crate::state::push_transcript_blank(&mut out, &mut last_was_blank);
                    last_was_prose = false;
                }
                for tool in tool_calls {
                    out.push(Line::from(Span::styled(tool_call_label(tool), tool_style)));
                    last_was_blank = false;
                }
            }
        }
    }
    out
}

fn is_rendered_record(val: &serde_json::Value) -> bool {
    let source = val["source"].as_str().unwrap_or("");
    let msg_type = val["type"].as_str().unwrap_or("");
    (source == "USER_EXPLICIT" && msg_type == "USER_INPUT")
        || (source == "MODEL" && msg_type == "PLANNER_RESPONSE")
}

const MAX_GALLERY_IMAGES: usize = 4096;

/// Stream an Agy `transcript.jsonl` and index every user-attached image in
/// chronological order for the `^a g` gallery.
///
/// Agy stores attached media on `USER_INPUT` records as a `"media"` array of
/// `{ "mime_type": "image/...", "uri": "/abs/path" }` references rather than
/// inline base64. If a record's `"content"` or `"media"` was truncated in
/// `transcript.jsonl`, its untruncated step is resolved from sibling
/// `transcript_full.jsonl` when present.
pub fn index_images(path: &Path) -> Vec<super::transcript_images::TranscriptImage> {
    let Ok(file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    if !file.metadata().is_ok_and(|m| m.is_file()) {
        return Vec::new();
    }
    let (candidates, needed_steps, oversized) = scan_image_candidates(file);
    if oversized > 0 {
        crate::debug_log::log(&format!(
            "agy_transcript: skipped {oversized} record(s) over {} bytes in {}",
            crate::state::MAX_TRANSCRIPT_LINE_BYTES,
            path.display()
        ));
    }
    let full_records = load_full_records(path, &needed_steps);
    let mut out = Vec::new();
    for (line_no, record) in &candidates {
        if out.len() >= MAX_GALLERY_IMAGES {
            break;
        }
        let val = if needs_full_image_record(record)
            && let Some(step) = record["step_index"].as_u64()
            && let Some(full) = full_records.get(&step)
        {
            full
        } else {
            record
        };
        index_image_record(val, *line_no, &mut out);
    }
    out
}

fn scan_image_candidates(
    file: std::fs::File,
) -> (Vec<(usize, serde_json::Value)>, HashSet<u64>, usize) {
    let mut reader = BufReader::new(file);
    let mut buf = Vec::new();
    let mut oversized = 0usize;
    let mut seen_steps = HashSet::new();
    let mut candidates = Vec::new();
    let mut needed = HashSet::new();
    for line_no in 0.. {
        match crate::state::read_line_capped(
            &mut reader,
            &mut buf,
            crate::state::MAX_TRANSCRIPT_LINE_BYTES,
        ) {
            Ok(None) | Err(_) => break,
            Ok(Some(false)) => {
                oversized += 1;
                continue;
            }
            Ok(Some(true)) => {}
        }
        let Ok(line) = std::str::from_utf8(&buf) else {
            continue;
        };
        if !line.contains("\"media\"") {
            continue;
        }
        let Ok(val) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if val["source"].as_str() != Some("USER_EXPLICIT")
            || val["type"].as_str() != Some("USER_INPUT")
        {
            continue;
        }
        if let Some(step) = val["step_index"].as_u64() {
            if !seen_steps.insert(step) {
                continue;
            }
            if needs_full_image_record(&val) {
                needed.insert(step);
            }
        }
        candidates.push((line_no, val));
    }
    (candidates, needed, oversized)
}

fn needs_full_image_record(val: &serde_json::Value) -> bool {
    val["truncated_fields"].as_array().is_some_and(|fields| {
        fields
            .iter()
            .any(|f| matches!(f.as_str(), Some("content" | "media")))
    })
}

fn index_image_record(
    val: &serde_json::Value,
    line_no: usize,
    out: &mut Vec<super::transcript_images::TranscriptImage>,
) {
    let Some(media) = val["media"].as_array() else {
        return;
    };
    let text = strip_user_request_tags(val["content"].as_str().unwrap_or(""));
    let labels = super::transcript_images::image_labels(text);
    let excerpt = super::transcript_images::excerpt_of(text);
    let timestamp = val["created_at"]
        .as_str()
        .or_else(|| val["timestamp"].as_str())
        .map(str::to_owned);
    let mut nth = 0usize;
    for item in media {
        if out.len() >= MAX_GALLERY_IMAGES {
            break;
        }
        let media_type = item["mime_type"].as_str().unwrap_or("image");
        if media_type != "image" && !media_type.starts_with("image/") {
            continue;
        }
        let Some(uri) = item["uri"].as_str() else {
            continue;
        };
        let raw_path = uri.strip_prefix("file://").unwrap_or(uri).trim();
        if raw_path.is_empty() {
            continue;
        }
        let img_path = PathBuf::from(raw_path);
        let bytes_len = std::fs::metadata(&img_path)
            .ok()
            .filter(std::fs::Metadata::is_file)
            .map_or(0, |m| {
                m.len().min(super::transcript_images::MAX_IMAGE_BYTES) as usize
            });
        out.push(super::transcript_images::TranscriptImage {
            seq: out.len() + 1,
            agent_label: labels.get(nth).cloned(),
            media_type: media_type.to_owned(),
            bytes_len,
            timestamp: timestamp.clone(),
            prompt_excerpt: excerpt.clone(),
            line: line_no,
            block: nth,
            source: super::transcript_images::ImageSource::Agy { path: img_path },
        });
        nth += 1;
    }
}

fn needs_full_record(val: &serde_json::Value, show_tool_calls: bool) -> bool {
    let Some(fields) = val["truncated_fields"].as_array() else {
        return false;
    };
    fields.iter().any(|f| {
        f.as_str() == Some("content") || (show_tool_calls && f.as_str() == Some("tool_calls"))
    })
}

fn parse_rendered_records(
    text: &str,
    show_tool_calls: bool,
) -> (Vec<serde_json::Value>, HashSet<u64>) {
    let mut records = Vec::new();
    let mut needed = HashSet::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(val) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if !is_rendered_record(&val) {
            continue;
        }
        if needs_full_record(&val, show_tool_calls)
            && let Some(step) = val["step_index"].as_u64()
        {
            needed.insert(step);
        }
        records.push(val);
    }
    (records, needed)
}

fn unrendered_full_record(line: &[u8]) -> bool {
    !line
        .windows(b"PLANNER_RESPONSE".len())
        .any(|w| w == b"PLANNER_RESPONSE")
        && !line
            .windows(b"USER_INPUT".len())
            .any(|w| w == b"USER_INPUT")
}

fn load_full_records(
    compact_path: &Path,
    needed: &HashSet<u64>,
) -> HashMap<u64, serde_json::Value> {
    let mut found = HashMap::new();
    if needed.is_empty() {
        return found;
    }
    let full_path = compact_path.with_file_name("transcript_full.jsonl");
    let Some(records) = crate::state::jsonl_records(
        &full_path,
        crate::state::MAX_TRANSCRIPT_RECORD_BYTES,
        unrendered_full_record,
    ) else {
        return found;
    };
    for line in records {
        let Ok(val) = serde_json::from_str::<serde_json::Value>(line.trim()) else {
            continue;
        };
        if is_rendered_record(&val)
            && let Some(step) = val["step_index"].as_u64()
            && needed.contains(&step)
        {
            found.insert(step, val);
        }
    }
    found
}

/// Strip `<USER_REQUEST>` wrapper tags when well-formed. The closing tag must
/// follow the opening tag or the slice would panic on untrusted input containing
/// a stray `</USER_REQUEST>` before `<USER_REQUEST>`.
fn strip_user_request_tags(content: &str) -> &str {
    let open = "<USER_REQUEST>";
    if let Some(start) = content.find(open)
        && let Some(end) = content.find("</USER_REQUEST>")
        && start + open.len() <= end
    {
        &content[start + open.len()..end]
    } else {
        content
    }
}

/// Format a tool call line as `⚙ name(detail)` or `⚙ name`, unwrapping lazy
/// `call_mcp_tool` invocations to `mcp_<server>_<tool>` and extracting a
/// one-line summary of the most salient argument.
fn tool_call_label(tool: &serde_json::Value) -> String {
    let raw = tool["name"].as_str().unwrap_or("?");
    let args = if tool["args"].is_object() {
        &tool["args"]
    } else {
        &tool["parameters"]
    };
    let (name, detail_input);
    let parsed_mcp_args;
    if raw == "call_mcp_tool"
        && let Some(server) = decode_arg_scalar(&args["ServerName"])
        && let Some(inner) = decode_arg_scalar(&args["ToolName"])
        && !server.is_empty()
        && !inner.is_empty()
    {
        name = format!("mcp_{server}_{inner}");
        parsed_mcp_args = match &args["Arguments"] {
            serde_json::Value::String(s) => {
                serde_json::from_str::<serde_json::Value>(s).unwrap_or(serde_json::Value::Null)
            }
            other => other.clone(),
        };
        detail_input = &parsed_mcp_args;
    } else {
        name = raw.to_owned();
        detail_input = args;
    }

    match tool_use_detail(detail_input) {
        Some(detail) => format!("\u{2699} {name}({detail})"),
        None => format!("\u{2699} {name}"),
    }
}

/// Extract a short one-line summary from a tool call's arguments object.
fn tool_use_detail(input: &serde_json::Value) -> Option<String> {
    const KEYS: &[&str] = &[
        "toolSummary",
        "description",
        "Path",
        "TargetFile",
        "file_path",
        "path",
        "SearchPath",
        "DirectoryPath",
        "pattern",
        "Query",
        "query",
        "CommandLine",
        "command",
        "branch",
        "status",
        "url",
        "Url",
        "Uri",
        "Role",
        "Prompt",
        "prompt",
        "Action",
        "TaskId",
        "root",
        "id",
    ];
    for key in KEYS {
        let Some(raw) = decode_arg_scalar(&input[*key]) else {
            continue;
        };
        let first = raw.lines().next().unwrap_or("").trim();
        if !first.is_empty() {
            return Some(crate::state::truncate_chars(first, 80));
        }
    }
    None
}

/// Decode a scalar argument value from an Agy tool call. `transcript.jsonl`
/// encodes string values inside `args` as JSON string literals (e.g.
/// `"\"src/foo.rs\""`), whereas `parameters` stores plain JSON scalars.
fn decode_arg_scalar(val: &serde_json::Value) -> Option<String> {
    match val {
        serde_json::Value::String(s) => {
            if let Ok(unquoted) = serde_json::from_str::<String>(s) {
                Some(unquoted)
            } else if let Ok(num) = serde_json::from_str::<i64>(s) {
                Some(num.to_string())
            } else {
                Some(s.trim_matches(|c| c == '"' || c == '\'').to_owned())
            }
        }
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_is_empty() {
        let lines = render_transcript(
            Path::new("/nonexistent/x.jsonl"),
            &Theme::default(),
            None,
            true,
        );
        assert!(lines.is_empty());
    }

    /// A pinned `conversationId` addresses the transcript directly. The
    /// proximity fallback picks by spawn time, which collides across panes
    /// restored together — so a pane that KNOWS its conversation must not use it.
    #[test]
    fn pinned_conversation_id_resolves_its_own_transcript() {
        let home = tempfile::tempdir().unwrap();
        let id = "ec33ebf9-0cba-4100-8142-c61503f6c587";
        let expected = home
            .path()
            .join(".gemini/antigravity-cli/brain")
            .join(id)
            .join(".system_generated/logs/transcript.jsonl");
        std::fs::create_dir_all(expected.parent().unwrap()).unwrap();
        std::fs::write(&expected, "{}\n").unwrap();

        let cwd = home.path().to_path_buf();
        let query = |sid: Option<&'static str>| crate::agent::TranscriptQuery {
            cwd: &cwd,
            spawn_epoch_secs: 0,
            command: "agy",
            session_id: sid,
            codex_home: None,
        };

        assert_eq!(
            resolve_under_home(home.path(), query(Some(id))),
            Some(expected)
        );

        // A pin whose transcript isn't on disk yet resolves to nothing rather than
        // proximity-matching some other conversation — `^a v` then flashes
        // "no transcript found" and closes.
        assert_eq!(
            resolve_under_home(
                home.path(),
                query(Some("11111111-1111-1111-1111-111111111111"))
            ),
            None
        );
    }

    /// Flatten rendered lines into plain text for assertions.
    fn flatten(lines: &[Line<'static>]) -> String {
        lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn renders_user_model_tools_and_survives_malformed_tags() {
        use std::io::Write;
        // Write JSONL via write_all (not writeln!) so the literal `{}`
        // braces in the JSON aren't parsed as format specifiers.
        let mut f = tempfile::NamedTempFile::new().unwrap();
        let rows = [
            // Well-formed user input → tags stripped to inner text.
            r#"{"source":"USER_EXPLICIT","type":"USER_INPUT","content":"<USER_REQUEST>hello there</USER_REQUEST>"}"#,
            // Model response with prose + a tool call.
            r#"{"source":"MODEL","type":"PLANNER_RESPONSE","content":"sure thing","tool_calls":[{"name":"read_file"}]}"#,
            // Regression: closing tag before opening — must NOT panic;
            // falls back to the raw content.
            r#"{"source":"USER_EXPLICIT","type":"USER_INPUT","content":"</USER_REQUEST> oops <USER_REQUEST>"}"#,
        ];
        for r in rows {
            f.write_all(r.as_bytes()).unwrap();
            f.write_all(b"\n").unwrap();
        }
        f.flush().unwrap();

        let lines = render_transcript(f.path(), &Theme::default(), None, true);
        let text = flatten(&lines);

        assert!(text.contains("hello there"), "user content rendered");
        assert!(text.contains("sure thing"), "model content rendered");
        assert!(text.contains("read_file"), "tool call rendered");
        // The malformed line rendered its raw content without panicking
        // (reaching this assertion at all proves the guard works).
        assert!(text.contains("oops"), "malformed line survived");

        // With tool calls hidden, the prose stays but the tool name is gone.
        let hidden = flatten(&render_transcript(f.path(), &Theme::default(), None, false));
        assert!(hidden.contains("sure thing"), "model prose kept");
        assert!(!hidden.contains("read_file"), "tool call hidden");
    }

    #[test]
    fn unwraps_call_mcp_tool_names() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("transcript.jsonl");
        let line = r#"{"source":"MODEL","type":"PLANNER_RESPONSE","content":"checking","tool_calls":[{"name":"call_mcp_tool","args":{"Arguments":"{}","ServerName":"\"spyc\"","ToolName":"\"get_spyc_context\""}},{"name":"call_mcp_tool","parameters":{"Arguments":{"query":"agent"},"ServerName":"other","ToolName":"search"}},{"name":"read_file","args":{"Path":"\"src/state/agy_transcript.rs\""}}]}"#;
        std::fs::write(&path, format!("{line}\n")).unwrap();

        let text = flatten(&render_transcript(&path, &Theme::default(), None, true));
        assert!(
            text.contains("checking\n\n\u{2699} mcp_spyc_get_spyc_context\n\u{2699} mcp_other_search(agent)\n\u{2699} read_file(src/state/agy_transcript.rs)"),
            "expected blank line after prose and grouped tool calls with details, got:\n{text}"
        );
        assert!(!text.contains("call_mcp_tool"), "got: {text}");
    }

    #[test]
    fn separates_prose_from_subsequent_turn_tool_calls() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("transcript.jsonl");
        let rows = [
            r#"{"step_index":1,"source":"MODEL","type":"PLANNER_RESPONSE","content":"Running checks now."}"#,
            r#"{"step_index":2,"source":"MODEL","type":"PLANNER_RESPONSE","tool_calls":[{"name":"run_command","args":{"CommandLine":"\"cargo test\"","toolSummary":"\"Run unit tests\""}}]}"#,
            r#"{"step_index":3,"source":"MODEL","type":"PLANNER_RESPONSE","tool_calls":[{"name":"call_mcp_tool","args":{"Arguments":"{\"id\":1}","ServerName":"\"spyc\"","ToolName":"\"release_scope\""}}]}"#,
            r#"{"step_index":4,"source":"MODEL","type":"PLANNER_RESPONSE","content":"All checks passed."}"#,
        ];
        std::fs::write(&path, rows.join("\n") + "\n").unwrap();

        let text = flatten(&render_transcript(&path, &Theme::default(), None, true));
        let expected = "Running checks now.\n\n\u{2699} run_command(Run unit tests)\n\u{2699} mcp_spyc_release_scope(1)\n\nAll checks passed.";
        assert_eq!(text, expected);
    }

    #[test]
    fn truncated_fields_fall_back_to_transcript_full_by_step_index() {
        let dir = tempfile::tempdir().unwrap();
        let compact = dir.path().join("transcript.jsonl");
        let full = dir.path().join("transcript_full.jsonl");

        let compact_rows = [
            r#"{"step_index":10,"source":"USER_EXPLICIT","type":"USER_INPUT","content":"<USER_REQUEST>trunc...","truncated_fields":["content"]}"#,
            r#"{"step_index":11,"source":"MODEL","type":"PLANNER_RESPONSE","content":"short...","tool_calls":[],"truncated_fields":["content","tool_calls"]}"#,
            r#"{"step_index":12,"source":"MODEL","type":"PLANNER_RESPONSE","content":"compact prose kept","truncated_fields":["thinking"]}"#,
        ];
        std::fs::write(&compact, compact_rows.join("\n") + "\n").unwrap();

        // Without transcript_full.jsonl present, compact content still renders.
        let fallback_text = flatten(&render_transcript(&compact, &Theme::default(), None, true));
        assert!(fallback_text.contains("trunc..."), "got: {fallback_text}");
        assert!(fallback_text.contains("short..."), "got: {fallback_text}");

        // Write transcript_full.jsonl with extra prefix records and swapped step order.
        let full_rows = [
            r#"{"step_index":1,"source":"USER_EXPLICIT","type":"USER_INPUT","content":"earlier step not in tail"}"#,
            r#"{"step_index":11,"source":"MODEL","type":"PLANNER_RESPONSE","content":"full planner response body","tool_calls":[{"name":"call_mcp_tool","args":{"ServerName":"\"spyc\"","ToolName":"\"git_status\""}}]}"#,
            r#"{"step_index":10,"source":"USER_EXPLICIT","type":"USER_INPUT","content":"<USER_REQUEST>full user prompt text</USER_REQUEST>"}"#,
            r#"{"step_index":12,"source":"MODEL","type":"PLANNER_RESPONSE","content":"full step 12 should not be used when only thinking was truncated"}"#,
        ];
        std::fs::write(&full, full_rows.join("\n") + "\n").unwrap();

        let text = flatten(&render_transcript(&compact, &Theme::default(), None, true));
        assert!(
            text.contains("full user prompt text"),
            "truncated user input should come from transcript_full.jsonl: {text}"
        );
        assert!(
            text.contains("full planner response body"),
            "truncated planner response should come from transcript_full.jsonl: {text}"
        );
        assert!(
            text.contains("\u{2699} mcp_spyc_git_status"),
            "truncated tool_calls should come from transcript_full.jsonl and unwrap: {text}"
        );
        assert!(
            text.contains("compact prose kept"),
            "record with only thinking truncated should use compact transcript: {text}"
        );
    }

    #[test]
    fn indexes_and_loads_user_media_images_and_filters_non_images() {
        let dir = tempfile::tempdir().unwrap();
        let img1 = dir.path().join("uploaded_media_1.png");
        let img2 = dir.path().join("uploaded_media_2.jpg");
        let pdf = dir.path().join("doc.pdf");
        std::fs::write(&img1, b"\x89PNG\r\n\x1a\nfirst").unwrap();
        std::fs::write(&img2, b"\xff\xd8\xffsecond").unwrap();
        std::fs::write(&pdf, b"%PDF-1.4").unwrap();

        let transcript = dir.path().join("transcript.jsonl");
        let rows = [
            serde_json::json!({
                "step_index": 1,
                "source": "USER_EXPLICIT",
                "type": "USER_INPUT",
                "created_at": "2026-10-23T10:13:55Z",
                "content": "<USER_REQUEST>\ncompare [Image #1] and [Image #2] please\n</USER_REQUEST>\n<ADDITIONAL_METADATA>\nignored\n</ADDITIONAL_METADATA>",
                "media": [
                    {"mime_type": "application/pdf", "uri": pdf.display().to_string()},
                    {"mime_type": "image/png", "uri": img1.display().to_string()},
                    {"mime_type": "image/jpeg", "uri": format!("file://{}", img2.display())},
                ]
            })
            .to_string(),
            // Duplicate replay of step 1 must not double the gallery.
            serde_json::json!({
                "step_index": 1,
                "source": "USER_EXPLICIT",
                "type": "USER_INPUT",
                "created_at": "2026-10-23T10:13:55Z",
                "content": "<USER_REQUEST>compare [Image #1] and [Image #2] please</USER_REQUEST>",
                "media": [
                    {"mime_type": "image/png", "uri": img1.display().to_string()},
                ]
            })
            .to_string(),
            // Model step with media is ignored (only user attachments belong in ^a g).
            serde_json::json!({
                "step_index": 2,
                "source": "MODEL",
                "type": "PLANNER_RESPONSE",
                "content": "model reply",
                "media": [
                    {"mime_type": "image/png", "uri": img1.display().to_string()},
                ]
            })
            .to_string(),
            // Separate user step re-attaching the first image stays a distinct row.
            serde_json::json!({
                "step_index": 3,
                "source": "USER_EXPLICIT",
                "type": "USER_INPUT",
                "created_at": "2026-10-23T10:20:00Z",
                "content": "<USER_REQUEST>again</USER_REQUEST>",
                "media": [
                    {"uri": img1.display().to_string()},
                ]
            })
            .to_string(),
        ];
        std::fs::write(&transcript, rows.join("\n") + "\n").unwrap();

        let images = index_images(&transcript);
        assert_eq!(images.len(), 3);

        assert_eq!(images[0].seq, 1);
        assert_eq!(images[0].agent_label.as_deref(), Some("#1"));
        assert_eq!(images[0].media_type, "image/png");
        assert_eq!(images[0].bytes_len, 13);
        assert_eq!(images[0].timestamp.as_deref(), Some("2026-10-23T10:13:55Z"));
        assert_eq!(images[0].prompt_excerpt, "compare and please");
        assert_eq!(
            crate::state::transcript_images::load(&transcript, &images[0]).unwrap(),
            b"\x89PNG\r\n\x1a\nfirst"
        );

        assert_eq!(images[1].seq, 2);
        assert_eq!(images[1].agent_label.as_deref(), Some("#2"));
        assert_eq!(images[1].media_type, "image/jpeg");
        assert_eq!(images[1].bytes_len, 9);
        assert_eq!(
            crate::state::transcript_images::load(&transcript, &images[1]).unwrap(),
            b"\xff\xd8\xffsecond"
        );

        assert_eq!(images[2].seq, 3);
        assert!(images[2].agent_label.is_none());
        assert_eq!(images[2].media_type, "image");
        assert_eq!(images[2].prompt_excerpt, "again");
    }

    #[test]
    fn truncated_image_step_falls_back_to_transcript_full_and_refuses_bad_files() {
        let dir = tempfile::tempdir().unwrap();
        let img = dir.path().join("shot.png");
        std::fs::write(&img, b"png-bytes").unwrap();
        let huge = dir.path().join("huge.png");
        std::fs::File::create(&huge)
            .unwrap()
            .set_len(crate::state::transcript_images::MAX_IMAGE_BYTES + 1)
            .unwrap();

        let compact = dir.path().join("transcript.jsonl");
        let full = dir.path().join("transcript_full.jsonl");
        std::fs::write(
            &compact,
            serde_json::json!({
                "step_index": 7,
                "source": "USER_EXPLICIT",
                "type": "USER_INPUT",
                "content": "<USER_REQUEST>trunc...",
                "truncated_fields": ["content", "media"]
            })
            .to_string()
                + "\n",
        )
        .unwrap();
        std::fs::write(
            &full,
            serde_json::json!({
                "step_index": 7,
                "source": "USER_EXPLICIT",
                "type": "USER_INPUT",
                "created_at": "2026-10-23T11:00:00Z",
                "content": "<USER_REQUEST>full image prompt [Image #1]</USER_REQUEST>",
                "media": [
                    {"mime_type": "image/png", "uri": img.display().to_string()},
                    {"mime_type": "image/png", "uri": huge.display().to_string()},
                    {"mime_type": "image/png", "uri": dir.path().display().to_string()},
                    {"mime_type": "image/png", "uri": dir.path().join("missing.png").display().to_string()},
                ]
            })
            .to_string()
                + "\n",
        )
        .unwrap();

        let spec = crate::agent::detect("agy")
            .transcript_images()
            .expect("agy declares transcript_images");
        assert!(matches!(
            spec.format,
            crate::state::transcript_images::Format::Agy
        ));

        let images = index_images(&compact);
        assert_eq!(images.len(), 4);
        assert_eq!(images[0].prompt_excerpt, "full image prompt");
        assert_eq!(images[0].agent_label.as_deref(), Some("#1"));
        assert_eq!(
            crate::state::transcript_images::load(&compact, &images[0]).unwrap(),
            b"png-bytes"
        );
        // Oversized, directory, and missing paths fail cleanly on load.
        assert!(crate::state::transcript_images::load(&compact, &images[1]).is_err());
        assert_eq!(images[2].bytes_len, 0, "directory is not sized as a file");
        assert!(crate::state::transcript_images::load(&compact, &images[2]).is_err());
        assert!(crate::state::transcript_images::load(&compact, &images[3]).is_err());
    }
}
