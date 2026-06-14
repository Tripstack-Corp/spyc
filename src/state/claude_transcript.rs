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
//!   typed prompt.
//! - `type: "user"` with `content` an array: `{type:"tool_result",
//!   content}` → a dim one-line output preview with a `(+N lines)`
//!   count (the full output is on disk but would swamp a reading
//!   view); `{type:"text"}` → a user prompt that arrived as a block
//!   (e.g. alongside a pasted image).
//! - `type: "assistant"` with `message.content` an array of blocks:
//!   `{type:"text", text}` → assistant prose; `{type:"tool_use",
//!   name, input}` → a tool call labelled with its most salient
//!   argument (the model-authored `description` for Bash, the
//!   file/pattern/command otherwise); `{type:"thinking"}` is
//!   signature-only on disk (no readable text) and skipped.
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
    // Muted but NOT Modifier::DIM: status_suffix is already a
    // comment-gray, and DIM stacked on top of it rendered the result
    // previews near-invisible on dark backgrounds (dogfood report).
    let dim_style = Style::default().fg(theme.status_suffix);

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
        match val["type"].as_str() {
            Some("user") => {
                let content = &val["message"]["content"];
                if let Some(text) = content.as_str() {
                    crate::state::push_transcript_prompt(
                        &mut out,
                        &mut last_was_blank,
                        text,
                        user_style,
                    );
                    continue;
                }
                // Array content: tool results (one-line dim preview)
                // interleaved with any block-form user text (a prompt
                // that arrived alongside a pasted image).
                let Some(blocks) = content.as_array() else {
                    continue;
                };
                for block in blocks {
                    match block["type"].as_str() {
                        Some("tool_result") => {
                            if let Some(preview) = tool_result_preview(block) {
                                out.push(Line::from(Span::styled(
                                    format!("  \u{2514} {preview}"),
                                    dim_style,
                                )));
                                last_was_blank = false;
                            }
                        }
                        Some("text") => {
                            crate::state::push_transcript_prompt(
                                &mut out,
                                &mut last_was_blank,
                                block["text"].as_str().unwrap_or(""),
                                user_style,
                            );
                        }
                        _ => {}
                    }
                }
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
                            let label = match tool_use_detail(&block["input"]) {
                                Some(detail) => format!("\u{2699} {name}({detail})"),
                                None => format!("\u{2699} {name}"),
                            };
                            out.push(Line::from(Span::styled(label, tool_style)));
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

/// Pick the most human-salient detail from a `tool_use` input for the
/// one-line transcript label. The model-authored `description` wins
/// (Bash carries a readable one-liner like "Find Ctrl handling in
/// app/mod.rs"); otherwise fall back through the primary target
/// argument of the common tools. `None` when the input has nothing
/// presentable — the label stays the bare tool name.
fn tool_use_detail(input: &serde_json::Value) -> Option<String> {
    const KEYS: [&str; 8] = [
        "description",
        "file_path",
        "path",
        "pattern",
        "command",
        "url",
        "query",
        "prompt",
    ];
    for key in KEYS {
        if let Some(s) = input[key].as_str() {
            let first = s.lines().next().unwrap_or("").trim();
            if !first.is_empty() {
                return Some(crate::state::truncate_chars(first, 80));
            }
        }
    }
    None
}

/// One-line dim preview of a `tool_result` block: the first non-empty
/// line of the output, truncated, with a `(+N lines)` tail when more
/// follow. The block's `content` is either a plain string or an array
/// of `{type:"text", text}` parts (concatenated). `None` for empty
/// output — no line beats a blank `└`.
fn tool_result_preview(block: &serde_json::Value) -> Option<String> {
    let content = &block["content"];
    let text: String = if let Some(s) = content.as_str() {
        s.to_string()
    } else if let Some(parts) = content.as_array() {
        parts
            .iter()
            .filter_map(|p| p["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        return None;
    };
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let first = lines.next()?.trim();
    let rest = lines.count();
    let preview = crate::state::truncate_chars(first, 100);
    if rest > 0 {
        Some(format!("{preview} (+{rest} lines)"))
    } else {
        Some(preview)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_is_empty() {
        let lines = render_transcript(Path::new("/nonexistent/x.jsonl"), &Theme::default(), None);
        assert!(lines.is_empty());
    }

    fn render_to_flat(content: &str) -> Vec<String> {
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "spyc-claude-test-{}-{:p}.jsonl",
            std::process::id(),
            content
        ));
        std::fs::write(&path, content).unwrap();
        let lines = render_transcript(&path, &Theme::default(), None);
        let _ = std::fs::remove_file(&path);
        lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    #[test]
    fn renders_user_string_and_assistant_blocks() {
        let flat = render_to_flat(concat!(
            r#"{"type":"user","message":{"role":"user","content":"fix the bug"}}"#,
            "\n",
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":""},{"type":"text","text":"On it."},{"type":"tool_use","name":"Edit","input":{"file_path":"src/lib.rs"}}]}}"#,
            "\n",
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"ok"}]}}"#,
            "\n",
            r#"{"type":"file-history-snapshot","foo":1}"#,
            "\n",
        ));
        assert!(flat.iter().any(|l| l.contains("fix the bug")));
        assert!(flat.iter().any(|l| l.contains("On it.")));
        // tool_use is labelled with its salient input.
        assert!(flat.iter().any(|l| l.contains("\u{2699} Edit(src/lib.rs)")));
        // tool_result renders as a dim one-line preview.
        assert!(flat.iter().any(|l| l.contains("\u{2514} ok")));
    }

    #[test]
    fn bash_tool_use_prefers_description_over_command() {
        let flat = render_to_flat(concat!(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","name":"Bash","input":{"command":"grep -rn foo src/ | head","description":"Find foo call sites"}}]}}"#,
            "\n",
        ));
        assert!(
            flat.iter()
                .any(|l| l.contains("\u{2699} Bash(Find foo call sites)"))
        );
        assert!(!flat.iter().any(|l| l.contains("grep -rn")));
    }

    #[test]
    fn tool_use_without_presentable_input_stays_bare() {
        let flat = render_to_flat(concat!(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","name":"ExitPlanMode","input":{}}]}}"#,
            "\n",
        ));
        assert!(flat.iter().any(|l| l.contains("\u{2699} ExitPlanMode")));
        assert!(!flat.iter().any(|l| l.contains("ExitPlanMode(")));
    }

    #[test]
    fn tool_result_preview_counts_extra_lines_and_reads_block_arrays() {
        // Array-of-text-parts content with a multi-line body: preview
        // is the first non-empty line plus a (+N lines) tail.
        let flat = render_to_flat(concat!(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":[{"type":"text","text":"line one\nline two\nline three"}]}]}}"#,
            "\n",
        ));
        assert!(
            flat.iter()
                .any(|l| l.contains("\u{2514} line one (+2 lines)")),
            "got: {flat:?}"
        );
    }

    #[test]
    fn empty_tool_result_renders_nothing() {
        let flat = render_to_flat(concat!(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":""}]}}"#,
            "\n",
        ));
        assert!(!flat.iter().any(|l| l.contains('\u{2514}')));
    }

    #[test]
    fn block_form_user_text_renders_as_prompt() {
        // A prompt that arrived as array content (e.g. alongside a
        // pasted image) used to be invisible.
        let flat = render_to_flat(concat!(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"look at this screenshot"}]}}"#,
            "\n",
        ));
        assert!(
            flat.iter()
                .any(|l| l.contains("\u{276f} look at this screenshot"))
        );
    }
}
