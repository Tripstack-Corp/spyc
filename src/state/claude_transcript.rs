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

/// Resolve the conversation JSONL for the Claude session running in the pane.
///
/// Prefers the pane's pinned live session id (`q.session_id`, lifted from the
/// status hook) when it names an on-disk conversation — the exact per-tab match.
/// Only with no pin (or its file gone) does it fall back to the session whose
/// start time is closest to the pane's spawn. The proximity guess alone
/// collapses two Claude tabs sharing a cwd onto one transcript (both `^a v`
/// views showed the same conversation); the pin is the reliable discriminator.
/// The `command` field of the query is unused.
pub fn resolve_active_jsonl(q: crate::agent::TranscriptQuery) -> Option<PathBuf> {
    let sessions: Vec<(u64, String)> = crate::state::sessions::find_claude_sessions(q.cwd)
        .into_iter()
        .map(|s| (s.started_at_secs, s.session_id))
        .collect();
    let chosen = choose_session(
        q.session_id,
        |sid| crate::state::sessions::claude_jsonl_path(q.cwd, sid).is_some(),
        &sessions,
        q.spawn_epoch_secs,
    )?;
    crate::state::sessions::claude_jsonl_path(q.cwd, &chosen)
}

/// Choose the Claude session id for a pane: the pinned live id when it names an
/// existing on-disk conversation (`exists`), else the candidate whose start time
/// is closest to the pane's spawn. Pure so the pin-wins-over-proximity rule is
/// unit-testable without touching `$HOME` / the filesystem.
fn choose_session(
    pinned: Option<&str>,
    exists: impl Fn(&str) -> bool,
    sessions: &[(u64, String)],
    spawn_epoch_secs: u64,
) -> Option<String> {
    if let Some(sid) = pinned
        && exists(sid)
    {
        return Some(sid.to_string());
    }
    sessions
        .iter()
        .min_by_key(|(started, _)| started.abs_diff(spawn_epoch_secs))
        .map(|(_, id)| id.clone())
}

/// Parse a Claude conversation JSONL into styled pager lines, in
/// chronological order. Returns empty on read failure. Assistant prose
/// is rendered through the Markdown viewer (`width` hints prose/table
/// reflow); user prompts and tool calls stay plain, agent-styled.
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
    // Muted via status_suffix's gray, NOT Modifier::DIM — DIM on top of it
    // renders near-invisible on dark backgrounds.
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
                            if show_tool_calls && let Some(preview) = tool_result_preview(block) {
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
                        Some("tool_use") if show_tool_calls => {
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
        let lines = render_transcript(
            Path::new("/nonexistent/x.jsonl"),
            &Theme::default(),
            None,
            true,
        );
        assert!(lines.is_empty());
    }

    fn render_to_flat(content: &str) -> Vec<String> {
        render_to_flat_opts(content, true)
    }

    fn render_to_flat_opts(content: &str, show_tool_calls: bool) -> Vec<String> {
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "spyc-claude-test-{}-{:p}-{show_tool_calls}.jsonl",
            std::process::id(),
            content
        ));
        std::fs::write(&path, content).unwrap();
        let lines = render_transcript(&path, &Theme::default(), None, show_tool_calls);
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
    fn hiding_tool_calls_keeps_prose_and_drops_tool_lines() {
        let content = concat!(
            r#"{"type":"user","message":{"role":"user","content":"fix the bug"}}"#,
            "\n",
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"On it."},{"type":"tool_use","name":"Edit","input":{"file_path":"src/lib.rs"}}]}}"#,
            "\n",
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"ok"}]}}"#,
            "\n",
        );
        let flat = render_to_flat_opts(content, false);
        // Prompt + assistant prose stay.
        assert!(flat.iter().any(|l| l.contains("fix the bug")));
        assert!(flat.iter().any(|l| l.contains("On it.")));
        // tool_use and tool_result lines are gone.
        assert!(
            !flat.iter().any(|l| l.contains("\u{2699}")),
            "no tool-use line"
        );
        assert!(
            !flat.iter().any(|l| l.contains("\u{2514}")),
            "no tool-result line"
        );
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

    // Two sessions in one cwd, started ~3 min apart — mirrors the real repro
    // (dfbcf56f @231, 1d1bc388 @412): two Claude tabs whose `^a v` both resolved
    // to the same transcript.
    fn two_sessions() -> Vec<(u64, String)> {
        vec![(231, "pr-review".to_string()), (412, "docs".to_string())]
    }

    #[test]
    fn pinned_session_wins_over_spawn_proximity() {
        // The PR-review pane, but spawned closest to the *docs* session's start —
        // proximity alone picks "docs" (the bug). The pin must win so the tab
        // shows its own transcript.
        let chosen = choose_session(Some("pr-review"), |_| true, &two_sessions(), 412);
        assert_eq!(chosen.as_deref(), Some("pr-review"));
    }

    #[test]
    fn falls_back_to_proximity_when_unpinned() {
        // No pin yet (hook hasn't fired) → closest start to spawn.
        let chosen = choose_session(None, |_| true, &two_sessions(), 410);
        assert_eq!(chosen.as_deref(), Some("docs"));
    }

    #[test]
    fn falls_back_when_pinned_file_missing() {
        // Pinned id whose JSONL is gone (rotated/deleted) → proximity, not a dead
        // pin that resolves to nothing.
        let chosen = choose_session(Some("ghost"), |sid| sid != "ghost", &two_sessions(), 230);
        assert_eq!(chosen.as_deref(), Some("pr-review"));
    }

    #[test]
    fn none_when_no_candidates_and_no_pin() {
        assert_eq!(choose_session(None, |_| true, &[], 100), None);
    }
}
