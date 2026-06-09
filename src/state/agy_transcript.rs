//! Read an Antigravity (`agy`) conversation transcript from its on-disk JSONL
//! and render it as pager lines.

use std::path::{Path, PathBuf};

use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::ui::theme::Theme;

/// Resolve the conversation JSONL for the Agy session running in
/// `cwd`, spawned around `spawn_epoch_secs`. Picks the session whose
/// start time is closest to the pane's spawn time.
pub fn resolve_active_jsonl(cwd: &Path, spawn_epoch_secs: u64) -> Option<PathBuf> {
    let sessions = crate::state::sessions::find_agy_sessions(cwd);
    let best = sessions
        .into_iter()
        .min_by_key(|s| s.started_at_secs.abs_diff(spawn_epoch_secs))?;

    std::env::var_os("HOME").map(|h| {
        PathBuf::from(h)
            .join(".gemini/antigravity-cli/brain")
            .join(best.session_id)
            .join(".system_generated/logs/transcript.jsonl")
    })
}

/// Parse an Agy conversation JSONL into styled pager lines, in
/// chronological order. Returns empty on read failure. Model prose is
/// rendered through the Markdown viewer (`width` hints prose/table
/// reflow); user prompts and tool calls stay plain.
pub fn render_transcript(path: &Path, theme: &Theme, width: Option<usize>) -> Vec<Line<'static>> {
    let Ok(text) = crate::state::read_tail_lossy(path, crate::state::MAX_TRANSCRIPT_TAIL_BYTES)
    else {
        return Vec::new();
    };
    let user_style = Style::default()
        .fg(theme.prompt_prefix)
        .add_modifier(Modifier::BOLD);
    let tool_style = Style::default().fg(theme.take);

    let mut out: Vec<Line<'static>> = Vec::new();
    let mut last_was_blank = true;
    let push_blank = |out: &mut Vec<Line<'static>>, last_was_blank: &mut bool| {
        if !*last_was_blank {
            out.push(Line::from(""));
            *last_was_blank = true;
        }
    };

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(val) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };

        let source = val["source"].as_str().unwrap_or("");
        let msg_type = val["type"].as_str().unwrap_or("");

        if source == "USER_EXPLICIT" && msg_type == "USER_INPUT" {
            let content = val["content"].as_str().unwrap_or("");
            if content.is_empty() {
                continue;
            }

            // Optionally, strip <USER_REQUEST> tags if they exist.
            // The closing tag must come after the opening one (+ its
            // length) or the slice would panic — content is untrusted
            // (a user message can contain a stray `</USER_REQUEST>`
            // before a `<USER_REQUEST>`), so guard the ordering and
            // fall back to the raw content when it doesn't hold.
            let open = "<USER_REQUEST>";
            let display_content = if let Some(start) = content.find(open)
                && let Some(end) = content.find("</USER_REQUEST>")
                && start + open.len() <= end
            {
                &content[start + open.len()..end]
            } else {
                content
            };
            let display_content = display_content.trim();

            push_blank(&mut out, &mut last_was_blank);
            for (i, body) in display_content.lines().enumerate() {
                let prefix = if i == 0 { "❯ " } else { "  " };
                out.push(Line::from(vec![
                    Span::styled(prefix, user_style),
                    Span::styled(body.to_string(), user_style),
                ]));
            }
            last_was_blank = false;
        } else if source == "MODEL" && msg_type == "PLANNER_RESPONSE" {
            if let Some(content) = val["content"].as_str() {
                crate::state::push_agent_markdown(
                    &mut out,
                    &mut last_was_blank,
                    content,
                    theme,
                    width,
                );
            }

            // Format tool_calls
            if let Some(tool_calls) = val["tool_calls"].as_array() {
                for tool in tool_calls {
                    let name = tool["name"].as_str().unwrap_or("?");
                    out.push(Line::from(Span::styled(
                        format!("\u{2699} {name}"),
                        tool_style,
                    )));
                    last_was_blank = false;
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_is_empty() {
        let lines = render_transcript(Path::new("/nonexistent/x.jsonl"), &Theme::default(), None);
        assert!(lines.is_empty());
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

        let lines = render_transcript(f.path(), &Theme::default(), None);
        let text = flatten(&lines);

        assert!(text.contains("hello there"), "user content rendered");
        assert!(text.contains("sure thing"), "model content rendered");
        assert!(text.contains("read_file"), "tool call rendered");
        // The malformed line rendered its raw content without panicking
        // (reaching this assertion at all proves the guard works).
        assert!(text.contains("oops"), "malformed line survived");
    }
}
