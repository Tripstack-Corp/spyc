//! Read an Antigravity (`agy`) conversation transcript from its on-disk JSONL
//! and render it as pager lines.

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
/// proximity-matching: we know which conversation this pane is running, so
/// falling through to `^a v`'s terminal capture is right and showing a *different*
/// conversation's history is not. The `command` field of the query is unused.
pub fn resolve_active_jsonl(q: crate::agent::TranscriptQuery) -> Option<PathBuf> {
    resolve_under_home(Path::new(&std::env::var_os("HOME")?), q)
}

/// [`resolve_active_jsonl`] with `home` passed in, so the resolution order is
/// testable without mutating the process environment.
fn resolve_under_home(home: &Path, q: crate::agent::TranscriptQuery) -> Option<PathBuf> {
    let transcript_for = |id: &str| {
        home.join(".gemini/antigravity-cli/brain")
            .join(id)
            .join(".system_generated/logs/transcript.jsonl")
    };

    if let Some(id) = q.session_id {
        let path = transcript_for(id);
        return path.exists().then_some(path);
    }

    let best = crate::state::sessions::find_agy_sessions(q.cwd)
        .into_iter()
        .min_by_key(|s| s.started_at_secs.abs_diff(q.spawn_epoch_secs))?;
    Some(transcript_for(&best.session_id))
}

/// Parse an Agy conversation JSONL into styled pager lines, in
/// chronological order. Returns empty on read failure. Model prose is
/// rendered through the Markdown viewer (`width` hints prose/table
/// reflow); user prompts and tool calls stay plain.
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
    let user_style = Style::default()
        .fg(theme.prompt_prefix)
        .add_modifier(Modifier::BOLD);
    let tool_style = Style::default().fg(theme.take);

    let mut out: Vec<Line<'static>> = Vec::new();
    let mut last_was_blank = true;

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

            crate::state::push_transcript_prompt(
                &mut out,
                &mut last_was_blank,
                display_content,
                user_style,
            );
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
            if show_tool_calls && let Some(tool_calls) = val["tool_calls"].as_array() {
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
        };

        assert_eq!(
            resolve_under_home(home.path(), query(Some(id))),
            Some(expected)
        );

        // A pin whose transcript isn't on disk yet resolves to nothing rather than
        // proximity-matching some other conversation — `^a v` then falls through to
        // terminal capture.
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
}
