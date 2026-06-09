//! Read a codex conversation transcript from its on-disk rollout
//! file and render it as pager lines.
//!
//! Codex (OpenAI's terminal agent) confines its conversation
//! history to a DECSTBM scroll region above its viewport — in both
//! alt-screen and `--no-alt-screen` modes — so completed turns
//! never scroll into the terminal's main buffer where spyc's vt100
//! emulator could capture them. `^a v` therefore can't screen-scrape
//! codex history.
//!
//! But codex writes the full transcript, live and flushed per turn,
//! to `~/.codex/sessions/YYYY/MM/DD/rollout-<ts>-<uuid>.jsonl`. This
//! module reads that file directly — the source of truth — and
//! renders a clean, structured conversation for the pager. Better
//! than terminal capture: real text, no grid artifacts, searchable.
//!
//! ## Rollout JSONL shape (codex 0.133)
//!
//! Each line: `{ "timestamp": ..., "type": <T>, "payload": {...} }`.
//! We render from the subset that maps to a readable conversation:
//! - `event_msg` / `user_message` → the user's typed text
//!   (`payload.message`). Preferred over the `response_item` user
//!   message, which is prefixed with the giant AGENTS.md system
//!   injection.
//! - `event_msg` / `agent_message` → the agent's reply text
//!   (`payload.message`).
//! - `response_item` / `function_call` → a tool call
//!   (`payload.name` + `payload.arguments`).
//! - `response_item` / `function_call_output` → tool result
//!   (`payload.output`), truncated.
//!
//! Everything else (reasoning [encrypted], token_count, turn_context,
//! session_meta, …) is skipped.

use std::path::{Path, PathBuf};

use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::ui::theme::Theme;

/// Walk `~/.codex/sessions/` for the rollout file belonging to the
/// codex session running in `cwd`, spawned around `spawn_epoch_secs`.
///
/// Match strategy mirrors the Claude resolver: read each rollout's
/// `session_meta` (the first line), filter to those whose `cwd`
/// matches, and pick the one whose start time is closest to the
/// pane's spawn time. Reading only the first line of each candidate
/// keeps this cheap enough for an on-demand (`^a v`) call.
pub fn resolve_active_rollout(cwd: &Path, spawn_epoch_secs: u64) -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let sessions_dir = PathBuf::from(home).join(".codex/sessions");
    if !sessions_dir.is_dir() {
        return None;
    }
    let cwd_str = cwd.to_string_lossy();
    // A symlinked pane cwd (e.g. /var → /private/var on macOS) records
    // its *canonical* path in session_meta, so compare against that form
    // too — mirrors the Claude resolver's canonicalize check.
    let canon_str = std::fs::canonicalize(cwd)
        .ok()
        .map(|c| c.to_string_lossy().into_owned());

    let mut best: Option<(u64, PathBuf)> = None; // (abs time diff, path)
    for path in rollout_files(&sessions_dir) {
        let Some((session_cwd, started_secs)) = read_session_meta(&path) else {
            continue;
        };
        // macOS /private/tmp vs /tmp symlink, same as Claude resolver.
        let matches = session_cwd == cwd_str
            || session_cwd.strip_prefix("/private").unwrap_or(&session_cwd) == cwd_str.as_ref()
            || canon_str.as_deref() == Some(session_cwd.as_str());
        if !matches {
            continue;
        }
        let diff = started_secs.abs_diff(spawn_epoch_secs);
        if best.as_ref().is_none_or(|(d, _)| diff < *d) {
            best = Some((diff, path));
        }
    }
    best.map(|(_, p)| p)
}

/// Enumerate `rollout-*.jsonl` files under the date-nested sessions
/// directory (`YYYY/MM/DD/`). Bounded, shallow walk — three levels.
fn rollout_files(sessions_dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(years) = std::fs::read_dir(sessions_dir) else {
        return out;
    };
    for year in years.filter_map(Result::ok) {
        let Ok(months) = std::fs::read_dir(year.path()) else {
            continue;
        };
        for month in months.filter_map(Result::ok) {
            let Ok(days) = std::fs::read_dir(month.path()) else {
                continue;
            };
            for day in days.filter_map(Result::ok) {
                let Ok(files) = std::fs::read_dir(day.path()) else {
                    continue;
                };
                for file in files.filter_map(Result::ok) {
                    let path = file.path();
                    let name_ok = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("rollout-"));
                    let ext_ok = path
                        .extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case("jsonl"));
                    if name_ok && ext_ok {
                        out.push(path);
                    }
                }
            }
        }
    }
    out
}

/// Read the first line of a rollout file (the `session_meta`) and
/// return `(cwd, started_at_secs)`. Returns `None` if the file is
/// empty, unreadable, or the first line isn't a session_meta.
fn read_session_meta(path: &Path) -> Option<(String, u64)> {
    use std::io::BufRead;
    let file = std::fs::File::open(path).ok()?;
    let mut first = String::new();
    std::io::BufReader::new(file).read_line(&mut first).ok()?;
    let val: serde_json::Value = serde_json::from_str(first.trim()).ok()?;
    if val["type"].as_str() != Some("session_meta") {
        return None;
    }
    let payload = &val["payload"];
    let cwd = payload["cwd"].as_str()?.to_string();
    let started_secs = payload["timestamp"]
        .as_str()
        .and_then(crate::state::sessions::parse_iso8601_to_epoch_secs)
        .unwrap_or(0);
    Some((cwd, started_secs))
}

/// Parse a codex rollout JSONL file into styled pager lines, in
/// chronological order. Returns an empty vec on read failure. Agent
/// prose is rendered through the Markdown viewer (`width` hints
/// prose/table reflow); user prompts and tool lines stay plain.
pub fn render_transcript(path: &Path, theme: &Theme, width: Option<usize>) -> Vec<Line<'static>> {
    let Ok(text) = crate::state::read_tail_lossy(path, crate::state::MAX_TRANSCRIPT_TAIL_BYTES)
    else {
        return Vec::new();
    };
    let user_style = Style::default()
        .fg(theme.prompt_prefix)
        .add_modifier(Modifier::BOLD);
    let tool_style = Style::default().fg(theme.take);
    let dim_style = Style::default()
        .fg(theme.status_suffix)
        .add_modifier(Modifier::DIM);

    let mut out: Vec<Line<'static>> = Vec::new();
    let mut last_was_blank = true; // suppress leading blank
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
        let top = val["type"].as_str().unwrap_or("");
        let payload = &val["payload"];
        let ptype = payload["type"].as_str().unwrap_or("");

        match (top, ptype) {
            ("event_msg", "user_message") => {
                let msg = payload["message"].as_str().unwrap_or("");
                push_blank(&mut out, &mut last_was_blank);
                for (i, body) in msg.lines().enumerate() {
                    let prefix = if i == 0 { "❯ " } else { "  " };
                    out.push(Line::from(vec![
                        Span::styled(prefix, user_style),
                        Span::styled(body.to_string(), user_style),
                    ]));
                }
                last_was_blank = false;
            }
            ("event_msg", "agent_message") => {
                let msg = payload["message"].as_str().unwrap_or("");
                crate::state::push_agent_markdown(&mut out, &mut last_was_blank, msg, theme, width);
            }
            ("response_item", "function_call") => {
                let name = payload["name"].as_str().unwrap_or("?");
                let args = payload["arguments"].as_str().unwrap_or("");
                let args_summary = summarize_args(args);
                out.push(Line::from(Span::styled(
                    format!("\u{2699} {name}({args_summary})"),
                    tool_style,
                )));
                last_was_blank = false;
            }
            ("response_item", "function_call_output") => {
                let output = payload["output"].as_str().unwrap_or("");
                let first = output.lines().next().unwrap_or("");
                let summary = truncate_chars(first, 100);
                out.push(Line::from(Span::styled(
                    format!("  \u{2514} {summary}"),
                    dim_style,
                )));
                last_was_blank = false;
            }
            _ => {}
        }
    }
    out
}

/// Collapse a tool-call argument JSON blob to a short one-liner for
/// the transcript. Long/multiline args are truncated.
fn summarize_args(args: &str) -> String {
    let flat = args.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_chars(&flat, 80)
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max).collect();
        format!("{truncated}\u{2026}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarize_args_flattens_and_truncates() {
        assert_eq!(summarize_args("{ \"a\": 1 }"), "{ \"a\": 1 }");
        let long = "x".repeat(200);
        let s = summarize_args(&long);
        assert!(s.ends_with('\u{2026}'));
        assert_eq!(s.chars().count(), 81); // 80 + ellipsis
    }

    #[test]
    fn truncate_respects_char_boundaries() {
        assert_eq!(truncate_chars("hello", 10), "hello");
        assert_eq!(truncate_chars("hello", 3), "hel\u{2026}");
    }

    #[test]
    fn render_transcript_missing_file_is_empty() {
        let lines = render_transcript(
            Path::new("/nonexistent/rollout.jsonl"),
            &Theme::default(),
            None,
        );
        assert!(lines.is_empty());
    }

    #[test]
    fn render_transcript_parses_user_and_agent() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("spyc-codex-test-{}.jsonl", std::process::id()));
        let content = concat!(
            r#"{"timestamp":"2026-05-27T01:20:40.151Z","type":"session_meta","payload":{"id":"x","timestamp":"2026-05-27T01:20:40.965Z","cwd":"/tmp"}}"#,
            "\n",
            r#"{"type":"event_msg","payload":{"type":"user_message","message":"hello codex"}}"#,
            "\n",
            r#"{"type":"event_msg","payload":{"type":"agent_message","message":"hi there"}}"#,
            "\n",
            r#"{"type":"response_item","payload":{"type":"function_call","name":"shell","arguments":"{\"cmd\":\"ls\"}"}}"#,
            "\n",
        );
        std::fs::write(&path, content).unwrap();
        let lines = render_transcript(&path, &Theme::default(), None);
        let _ = std::fs::remove_file(&path);
        let flat: Vec<String> = lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect();
        assert!(flat.iter().any(|l| l.contains("hello codex")));
        assert!(flat.iter().any(|l| l.contains("hi there")));
        assert!(flat.iter().any(|l| l.contains("shell(")));
    }
}
