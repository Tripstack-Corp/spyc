//! Read a Claude Code conversation transcript from its on-disk JSONL
//! and render it as pager lines — the Claude analogue of
//! [`crate::state::codex_transcript`].
//!
//! Unlike codex (whose history can't be captured from the terminal
//! at all), Claude renders inline and its output *does* scroll into
//! the main buffer, so `^a v` works via terminal capture today. This
//! transcript view is therefore **opt-in** (`[pane]
//! claude_transcript_scrollback = true`): it trades the verbatim
//! terminal capture for a cleaner, structured conversation with no
//! grid/repaint artifacts.
//!
//! ## Claude JSONL shape (Claude Code 2.1)
//!
//! `~/.claude/projects/<slug>/<session-id>.jsonl`, one JSON object
//! per line. The lines we render:
//! - `type: "user"` with `message.content` a *string* → the user's
//!   typed prompt. (When `content` is an array it's a tool_result
//!   echo, which we skip — it's noise in a reading view.)
//! - `type: "assistant"` with `message.content` an array of blocks:
//!   `{type:"text", text}` → assistant prose; `{type:"tool_use",
//!   name}` → a tool call; `{type:"thinking"}` is signature-only on
//!   disk (no readable text) and skipped.
//!
//! Everything else (system, file-history-snapshot, ai-title,
//! permission-mode, attachment, …) is skipped.

use std::path::{Path, PathBuf};

use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::ui::theme::Theme;

/// Resolve the conversation JSONL for the Claude session running in
/// `cwd`, spawned around `spawn_epoch_secs`. Picks the session whose
/// start time is closest to the pane's spawn time (handles multiple
/// Claude tabs sharing a cwd), then maps it to its on-disk path.
pub fn resolve_active_jsonl(cwd: &Path, spawn_epoch_secs: u64) -> Option<PathBuf> {
    let sessions = crate::state::sessions::find_claude_sessions(cwd);
    let best = sessions
        .into_iter()
        .min_by_key(|s| s.started_at_secs.abs_diff(spawn_epoch_secs))?;
    crate::state::sessions::claude_jsonl_path(cwd, &best.session_id)
}

/// Parse a Claude conversation JSONL into styled pager lines, in
/// chronological order. Returns empty on read failure. Assistant prose
/// is rendered through the Markdown viewer (`width` hints prose/table
/// reflow); user prompts and tool calls stay plain, agent-styled.
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
        match val["type"].as_str() {
            Some("user") => {
                // Only string content is a real prompt; array content
                // is a tool_result echo (skip).
                let Some(text) = val["message"]["content"].as_str() else {
                    continue;
                };
                if text.is_empty() {
                    continue;
                }
                push_blank(&mut out, &mut last_was_blank);
                for (i, body) in text.lines().enumerate() {
                    let prefix = if i == 0 { "❯ " } else { "  " };
                    out.push(Line::from(vec![
                        Span::styled(prefix, user_style),
                        Span::styled(body.to_string(), user_style),
                    ]));
                }
                last_was_blank = false;
            }
            Some("assistant") => {
                let Some(blocks) = val["message"]["content"].as_array() else {
                    continue;
                };
                for block in blocks {
                    match block["type"].as_str() {
                        Some("text") => {
                            let body = block["text"].as_str().unwrap_or("");
                            crate::state::push_agent_markdown(
                                &mut out,
                                &mut last_was_blank,
                                body,
                                theme,
                                width,
                            );
                        }
                        Some("tool_use") => {
                            let name = block["name"].as_str().unwrap_or("?");
                            out.push(Line::from(Span::styled(
                                format!("\u{2699} {name}"),
                                tool_style,
                            )));
                            last_was_blank = false;
                        }
                        // thinking blocks are signature-only on disk.
                        _ => {}
                    }
                }
            }
            _ => {}
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

    #[test]
    fn renders_user_string_and_assistant_blocks() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("spyc-claude-test-{}.jsonl", std::process::id()));
        let content = concat!(
            r#"{"type":"user","message":{"role":"user","content":"fix the bug"}}"#,
            "\n",
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":""},{"type":"text","text":"On it."},{"type":"tool_use","name":"Edit"}]}}"#,
            "\n",
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"ok"}]}}"#,
            "\n",
            r#"{"type":"file-history-snapshot","foo":1}"#,
            "\n",
        );
        std::fs::write(&path, content).unwrap();
        let lines = render_transcript(&path, &Theme::default(), None);
        let _ = std::fs::remove_file(&path);
        let flat: Vec<String> = lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect();
        assert!(flat.iter().any(|l| l.contains("fix the bug")));
        assert!(flat.iter().any(|l| l.contains("On it.")));
        assert!(flat.iter().any(|l| l.contains("Edit")));
        // tool_result echo (array content) is skipped.
        assert!(!flat.iter().any(|l| l.contains("ok")));
    }
}
