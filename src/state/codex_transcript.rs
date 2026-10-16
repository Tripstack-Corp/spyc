//! Read a codex conversation transcript from its on-disk rollout
//! file and render it as pager lines.
//!
//! Codex (OpenAI's terminal agent) confines its conversation
//! history to a DECSTBM scroll region above its viewport — in both
//! alt-screen and `--no-alt-screen` modes — so completed turns
//! never scroll into the terminal's main buffer, where vt100, spyc's
//! engine before 2.2, could capture them; screen-scraping never got
//! codex history.
//!
//! But codex writes the full transcript, live and flushed per turn,
//! under the pane's captured Codex home at
//! `sessions/YYYY/MM/DD/rollout-<ts>-<uuid>.jsonl`. This
//! module reads that file directly — the source of truth — and
//! renders a clean, structured conversation for the pager. Better
//! than terminal capture: real text, no grid artifacts, searchable.
//!
//! ## Rollout JSONL shape
//!
//! Each line: `{ "timestamp": ..., "type": <T>, "payload": {...} }`.
//! Current `event_msg/item_completed` records carry structured user/agent
//! messages and command/MCP items. The shared `agent::codex_records` normalizer
//! also accepts the legacy subset that maps to a readable conversation:
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
//! - `response_item` / `custom_tool_call` and `custom_tool_call_output` → custom
//!   tool input and result.
//!
//! Everything else (reasoning [encrypted], token_count, turn_context,
//! session_meta, …) is skipped.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::agent::codex_records::{self, RecordKind};
use crate::ui::theme::Theme;

/// Resolve only this pane's pinned session or explicit resume selector.
/// Discovery runs through the spawn-ordered claim worker; a reader must never
/// substitute another pane's history when its exact rollout is unavailable.
pub fn resolve_active_rollout(q: crate::agent::TranscriptQuery) -> Option<PathBuf> {
    let sessions = crate::agent::codex_home::sessions(q.codex_home?);
    let uuid = q
        .session_id
        .map(str::to_owned)
        .or_else(|| resume_uuid_from_command(q.command))?;
    find_rollout_by_uuid(&sessions, &uuid)
}

/// File mtime as epoch seconds, or `None` if unreadable. Used as the
/// "written during this pane's lifetime" signal for the claim worker.
fn file_mtime_secs(path: &Path) -> Option<u64> {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

/// Extract the session uuid from a `codex resume <uuid>` command. Returns
/// `None` for a fresh `codex`, `codex resume --last`, or any non-uuid token.
/// `pub` so the pane-launch path can pin a resumed session immediately
/// ([`crate::app`]'s `codex_pin`).
pub fn resume_uuid_from_command(command: &str) -> Option<String> {
    crate::agent::codex_command::resume_selector(command)?
        .selector
        .filter(|id| crate::state::sessions::is_uuid(id))
}

/// True when `command` resumes a codex session without naming it — `resume
/// --last`, or a spyc restore whose session id was never captured. These are
/// the only codex tabs Signal 1 cannot pin: `codex resume <UUID>` carries its
/// identity, and a fresh `codex` writes a rollout that postdates the pane.
pub fn is_resume_without_id(command: &str) -> bool {
    crate::agent::codex_command::resume_selector(command).is_some_and(|selection| {
        selection
            .selector
            .is_none_or(|id| !crate::state::sessions::is_uuid(&id))
    })
}

/// Find the rollout whose filename embeds `uuid` (codex names files
/// `rollout-<ts>-<uuid>.jsonl`, and the uuid is unique).
pub(super) fn find_rollout_by_uuid(sessions_dir: &Path, uuid: &str) -> Option<PathBuf> {
    if !crate::state::sessions::is_uuid(uuid) {
        return None;
    }
    rollout_files(sessions_dir)
        .into_iter()
        .find(|path| uuid_from_rollout_path(path).as_deref() == Some(uuid))
}

/// A rollout's identity for session pinning (Option B): the uuid (from the
/// filename), the `session_meta` cwd, and start time. See [`scan_rollout_metas`].
#[derive(Clone, Debug)]
pub struct RolloutMeta {
    pub uuid: String,
    pub cwd: String,
    pub started_secs: u64,
    /// Last-write time. A *resumed* session appends to its original rollout
    /// with a frozen `session_meta`, so `started_secs` can predate the pane by
    /// weeks — this is the only field that shows the file is live.
    pub mtime_secs: u64,
}

/// The trailing uuid of a `rollout-<ts>-<uuid>.jsonl` filename (the codex
/// timestamp itself contains hyphens, so take the last 36 chars and validate).
fn uuid_from_rollout_path(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?; // drops the `.jsonl`
    let uuid = stem.get(stem.len().checked_sub(36)?..)?;
    crate::state::sessions::is_uuid(uuid).then(|| uuid.to_string())
}

/// Snapshot every rollout's `(uuid, cwd, start)` for session pinning. Heavy IO
/// (reads each file's first line) — call OFF the loop (`codex_pin`'s worker).
pub fn scan_rollout_metas(home: &Path) -> Vec<RolloutMeta> {
    let sessions_dir = crate::agent::codex_home::sessions(home);
    let mut out = Vec::new();
    for path in rollout_files(&sessions_dir) {
        let Some(uuid) = uuid_from_rollout_path(&path) else {
            continue;
        };
        if let Some((cwd, started_secs)) = read_session_meta(&path) {
            let mtime_secs = file_mtime_secs(&path).unwrap_or(0);
            out.push(RolloutMeta {
                uuid,
                cwd,
                started_secs,
                mtime_secs,
            });
        }
    }
    out
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
pub fn render_transcript(
    path: &Path,
    theme: &Theme,
    width: Option<usize>,
    show_tool_calls: bool,
) -> Vec<Line<'static>> {
    let Ok(text) =
        super::codex_history::read_with_history(path, crate::state::MAX_TRANSCRIPT_TAIL_BYTES)
    else {
        return Vec::new();
    };
    let user_style = Style::default()
        .fg(theme.prompt_prefix)
        .add_modifier(Modifier::BOLD);
    let tool_style = Style::default().fg(theme.take);
    // Muted but NOT Modifier::DIM: status_suffix is already a
    // comment-grey, and DIM stacked on top of it rendered the result
    // previews near-invisible on dark backgrounds (dogfood report).
    let dim_style = Style::default().fg(theme.status_suffix);

    let mut out: Vec<Line<'static>> = Vec::new();
    let mut last_was_blank = true; // suppress leading blank
    let mut seen = HashSet::new();
    let mut session = String::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(val) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if val["type"] == "session_meta" {
            val["payload"]["id"]
                .as_str()
                .or_else(|| val["payload"]["session_id"].as_str())
                .unwrap_or_default()
                .clone_into(&mut session);
        }
        for record in codex_records::decode(&val) {
            if let Some(id) = record.id
                && !seen.insert((session.clone(), std::mem::discriminant(&record.kind), id))
            {
                continue;
            }
            match record.kind {
                RecordKind::User(msg) => {
                    crate::state::push_transcript_prompt(
                        &mut out,
                        &mut last_was_blank,
                        &msg,
                        user_style,
                    );
                }
                RecordKind::Agent(msg) => {
                    crate::state::push_agent_markdown(
                        &mut out,
                        &mut last_was_blank,
                        &msg,
                        theme,
                        width,
                    );
                }
                RecordKind::ToolCall { name, arguments } if show_tool_calls => {
                    let args_summary = summarize_args(&arguments);
                    out.push(Line::from(Span::styled(
                        format!("\u{2699} {name}({args_summary})"),
                        tool_style,
                    )));
                    last_was_blank = false;
                }
                RecordKind::ToolOutput(output) if show_tool_calls => {
                    let first = output.lines().next().unwrap_or("");
                    let summary = crate::state::truncate_chars(first, 100);
                    out.push(Line::from(Span::styled(
                        format!("  \u{2514} {summary}"),
                        dim_style,
                    )));
                    last_was_blank = false;
                }
                _ => {}
            }
        }
    }
    out
}

/// Collapse a tool-call argument JSON blob to a short one-liner for
/// the transcript. Long/multiline args are truncated.
fn summarize_args(args: &str) -> String {
    let flat = args.split_whitespace().collect::<Vec<_>>().join(" ");
    crate::state::truncate_chars(&flat, 80)
}

#[cfg(test)]
#[path = "codex_transcript_tests.rs"]
mod record_tests;

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
        assert_eq!(crate::state::truncate_chars("hello", 10), "hello");
        assert_eq!(crate::state::truncate_chars("hello", 3), "hel\u{2026}");
    }

    #[test]
    fn render_transcript_missing_file_is_empty() {
        let lines = render_transcript(
            Path::new("/nonexistent/rollout.jsonl"),
            &Theme::default(),
            None,
            true,
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
        let lines = render_transcript(&path, &Theme::default(), None, true);
        let flat: Vec<String> = lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect();
        assert!(flat.iter().any(|l| l.contains("hello codex")));
        assert!(flat.iter().any(|l| l.contains("hi there")));
        assert!(flat.iter().any(|l| l.contains("shell(")));

        // show_tool_calls=false keeps the prose but drops the tool call.
        let hidden = render_transcript(&path, &Theme::default(), None, false);
        let _ = std::fs::remove_file(&path);
        let flat_hidden: Vec<String> = hidden
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect();
        assert!(flat_hidden.iter().any(|l| l.contains("hi there")));
        assert!(
            !flat_hidden.iter().any(|l| l.contains("shell(")),
            "tool call hidden when show_tool_calls=false"
        );
    }

    const UUID: &str = "019e8b21-9e7c-7553-a118-d1cdada725fd";

    #[test]
    fn resume_uuid_parsed_only_for_a_real_uuid() {
        assert_eq!(
            resume_uuid_from_command(&format!("codex resume {UUID}")).as_deref(),
            Some(UUID)
        );
        assert_eq!(
            resume_uuid_from_command(&format!("/usr/local/bin/codex resume {UUID} --foo"))
                .as_deref(),
            Some(UUID)
        );
        assert_eq!(resume_uuid_from_command("codex resume --last"), None);
        assert_eq!(resume_uuid_from_command("codex"), None);
        assert_eq!(resume_uuid_from_command("codex resume not-a-uuid"), None);
    }

    #[test]
    fn uuid_from_rollout_path_takes_the_trailing_uuid() {
        let p = PathBuf::from(format!(
            "/x/.codex/sessions/2026/06/02/rollout-2026-06-02T21-38-16-{UUID}.jsonl"
        ));
        assert_eq!(uuid_from_rollout_path(&p).as_deref(), Some(UUID));
        // No uuid / wrong shape → None.
        assert_eq!(
            uuid_from_rollout_path(Path::new("/x/rollout-shortname.jsonl")),
            None
        );
        assert_eq!(uuid_from_rollout_path(Path::new("/x/notes.txt")), None);
    }

    /// `is_resume_without_id` is what selects the mode, so pin the classification
    /// the ranking depends on.
    #[test]
    fn resume_classification_drives_the_ranking_mode() {
        assert!(is_resume_without_id("codex resume --last"));
        assert!(is_resume_without_id("codex resume"));
        assert!(!is_resume_without_id("codex"));
        // A named resume carries its identity, so Signal 1 pins it and Signal 2's
        // mode never matters — but it must not classify as resume-without-id.
        assert!(!is_resume_without_id(
            "codex resume 019df582-fc65-7863-b349-f5c6f691f71c"
        ));
    }
}
