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

/// Clock-jitter tolerance (seconds) when deciding whether a rollout was
/// written *during* a pane's lifetime. The pane spawn time and the rollout's
/// file mtime come from the same machine clock, so this only absorbs sub-second
/// rounding and the brief gap between recording the spawn time and codex's
/// first flush — it doesn't need to be large.
const MTIME_SKEW_SECS: u64 = 5;

/// Walk `~/.codex/sessions/` for the rollout file belonging to the codex
/// session running in this pane.
///
/// The old strategy — pick the rollout whose `session_meta` start time is
/// *closest* to the pane spawn — is wrong for codex, because **resuming a
/// session appends to the original rollout file and leaves `session_meta`
/// frozen at the original creation time**. A pane spawned today resuming a
/// two-week-old session would compute a huge time delta and silently match
/// some *other* session that merely started near "now" (the reported
/// "fresh codex shows a previous session" / "long session goes stale" bugs).
///
/// The robust signals, strongest first:
///
/// 0. **Pinned session id** (`q.session_id`). The pane's session uuid, captured
///    at spawn by `app::codex_pin` (Option B) — an exact rollout match, immune
///    to time skew and multi-pane. The other signals are the fallback while a
///    fresh pane's pin hasn't landed yet.
/// 1. **Explicit session id.** When the spawn command is `codex resume
///    <uuid>`, the uuid is the ground truth — codex embeds it in the rollout
///    filename, so match that exactly. Immune to time skew and multi-pane.
/// 2. **Written during this pane's lifetime.** Otherwise, among rollouts whose
///    `session_meta.cwd` matches, keep those whose file *mtime* is at/after the
///    pane spawn (codex appends every turn, so mtime tracks live activity even
///    when the frozen start time doesn't) and pick the most-recently-written.
///    A fresh session that hasn't flushed yet, or only stale sessions, yields
///    `None` — caller flashes "no transcript yet" rather than the wrong file.
pub fn resolve_active_rollout(q: crate::agent::TranscriptQuery) -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let sessions_dir = PathBuf::from(home).join(".codex/sessions");
    if !sessions_dir.is_dir() {
        return None;
    }

    // Signal 0 (strongest): the session id pinned to this pane at spawn
    // (Option B — `codex_pin`). An exact rollout match, immune to time skew and
    // multi-pane; set once the spawn-time scan resolves it (or at launch for a
    // `codex resume <uuid>` pane).
    if let Some(uuid) = q.session_id
        && let Some(path) = find_rollout_by_uuid(&sessions_dir, uuid)
    {
        return Some(path);
    }

    // Signal 1: exact match on the resume uuid baked into the command — covers a
    // resumed pane before its pin has landed.
    if let Some(uuid) = resume_uuid_from_command(q.command)
        && let Some(path) = find_rollout_by_uuid(&sessions_dir, &uuid)
    {
        return Some(path);
    }

    let cwd_str = q.cwd.to_string_lossy();
    // A symlinked pane cwd (e.g. /var → /private/var on macOS) records
    // its *canonical* path in session_meta, so compare against that form
    // too — mirrors the Claude resolver's canonicalize check.
    let canon_str = std::fs::canonicalize(q.cwd)
        .ok()
        .map(|c| c.to_string_lossy().into_owned());

    // Signal 2: read each rollout's mtime + session_meta, then let the pure
    // ranking pick the winner. Reading the meta for every file matches the
    // pre-existing cost (the old resolver did the same); the decision itself is
    // factored out so it's unit-testable without a `~/.codex` on disk.
    let candidates: Vec<RolloutCandidate> = rollout_files(&sessions_dir)
        .into_iter()
        .filter_map(|path| {
            let mtime = file_mtime_secs(&path).unwrap_or(0);
            let (session_cwd, started_secs) = read_session_meta(&path)?;
            Some(RolloutCandidate {
                path,
                mtime,
                session_cwd,
                started_secs,
            })
        })
        .collect();
    pick_best_rollout(
        &candidates,
        &cwd_str,
        canon_str.as_deref(),
        q.spawn_epoch_secs,
    )
}

/// A rollout's inputs to the pure ranking decision ([`pick_best_rollout`]).
struct RolloutCandidate {
    path: PathBuf,
    /// File mtime (epoch secs) — the "last written" / live-activity signal.
    mtime: u64,
    /// `session_meta.cwd`.
    session_cwd: String,
    /// `session_meta.timestamp` as epoch secs — frozen at original creation.
    started_secs: u64,
}

/// Pure ranking for Signal 2 (the `route.rs` / `focus.rs` template): among
/// candidates whose cwd matches and whose mtime is at/after the pane spawn
/// (written during this pane's lifetime), return the most-recently-written.
/// Tie-break on the start time closest to the spawn, which separates two fresh
/// codex panes sharing a cwd. `None` when nothing qualifies.
fn pick_best_rollout(
    candidates: &[RolloutCandidate],
    cwd_str: &str,
    canon_str: Option<&str>,
    spawn_epoch_secs: u64,
) -> Option<PathBuf> {
    let mut best: Option<(u64, u64, &Path)> = None; // (mtime, start_diff, path)
    for c in candidates {
        // A file last touched before this pane existed can't be its session.
        if c.mtime + MTIME_SKEW_SECS < spawn_epoch_secs {
            continue;
        }
        if !cwd_matches(&c.session_cwd, cwd_str, canon_str) {
            continue;
        }
        // Prefer larger mtime; on a tie prefer the smaller |start − spawn|.
        let start_diff = c.started_secs.abs_diff(spawn_epoch_secs);
        let better = best
            .as_ref()
            .is_none_or(|(m, d, _)| c.mtime > *m || (c.mtime == *m && start_diff < *d));
        if better {
            best = Some((c.mtime, start_diff, &c.path));
        }
    }
    best.map(|(_, _, p)| p.to_path_buf())
}

/// Whether `session_cwd` (from a rollout's `session_meta`) refers to the same
/// directory as the pane's cwd, tolerating the macOS `/private` symlink and a
/// canonicalized form — same matching as the Claude resolver.
fn cwd_matches(session_cwd: &str, cwd_str: &str, canon_str: Option<&str>) -> bool {
    session_cwd == cwd_str
        || session_cwd.strip_prefix("/private").unwrap_or(session_cwd) == cwd_str
        || canon_str == Some(session_cwd)
}

/// File mtime as epoch seconds, or `None` if unreadable. Used as the
/// "written during this pane's lifetime" signal — see [`resolve_active_rollout`].
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
    let mut toks = command.split_whitespace();
    while let Some(tok) = toks.next() {
        if tok == "resume"
            && let Some(next) = toks.next()
            && crate::state::sessions::is_uuid(next)
        {
            return Some(next.to_string());
        }
    }
    None
}

/// Find the rollout whose filename embeds `uuid` (codex names files
/// `rollout-<ts>-<uuid>.jsonl`, and the uuid is unique).
fn find_rollout_by_uuid(sessions_dir: &Path, uuid: &str) -> Option<PathBuf> {
    rollout_files(sessions_dir).into_iter().find(|p| {
        p.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.contains(uuid))
    })
}

/// A rollout's identity for session pinning (Option B): the uuid (from the
/// filename), the `session_meta` cwd, and start time. See [`scan_rollout_metas`].
#[derive(Clone, Debug)]
pub struct RolloutMeta {
    pub uuid: String,
    pub cwd: String,
    pub started_secs: u64,
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
pub fn scan_rollout_metas() -> Vec<RolloutMeta> {
    let Some(home) = std::env::var_os("HOME") else {
        return Vec::new();
    };
    let sessions_dir = PathBuf::from(home).join(".codex/sessions");
    let mut out = Vec::new();
    for path in rollout_files(&sessions_dir) {
        let Some(uuid) = uuid_from_rollout_path(&path) else {
            continue;
        };
        if let Some((cwd, started_secs)) = read_session_meta(&path) {
            out.push(RolloutMeta {
                uuid,
                cwd,
                started_secs,
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
    let mut last_was_blank = true; // suppress leading blank

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
                crate::state::push_transcript_prompt(
                    &mut out,
                    &mut last_was_blank,
                    msg,
                    user_style,
                );
            }
            ("event_msg", "agent_message") => {
                let msg = payload["message"].as_str().unwrap_or("");
                crate::state::push_agent_markdown(&mut out, &mut last_was_blank, msg, theme, width);
            }
            ("response_item", "function_call") if show_tool_calls => {
                let name = payload["name"].as_str().unwrap_or("?");
                let args = payload["arguments"].as_str().unwrap_or("");
                let args_summary = summarize_args(args);
                out.push(Line::from(Span::styled(
                    format!("\u{2699} {name}({args_summary})"),
                    tool_style,
                )));
                last_was_blank = false;
            }
            ("response_item", "function_call_output") if show_tool_calls => {
                let output = payload["output"].as_str().unwrap_or("");
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
    out
}

/// Collapse a tool-call argument JSON blob to a short one-liner for
/// the transcript. Long/multiline args are truncated.
fn summarize_args(args: &str) -> String {
    let flat = args.split_whitespace().collect::<Vec<_>>().join(" ");
    crate::state::truncate_chars(&flat, 80)
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

    fn cand(name: &str, mtime: u64, cwd: &str, started: u64) -> RolloutCandidate {
        RolloutCandidate {
            path: PathBuf::from(name),
            mtime,
            session_cwd: cwd.to_string(),
            started_secs: started,
        }
    }

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

    #[test]
    fn cwd_matches_handles_private_and_canonical() {
        assert!(cwd_matches("/home/x", "/home/x", None));
        assert!(cwd_matches("/private/tmp/p", "/tmp/p", None)); // macOS symlink
        assert!(cwd_matches("/canon/p", "/sym/p", Some("/canon/p"))); // canonicalized
        assert!(!cwd_matches("/other", "/home/x", None));
    }

    #[test]
    fn pick_skips_sessions_that_predate_the_pane() {
        // The "fresh codex shows a previous session" bug: a stale session in
        // the same cwd whose file was last written long before this pane
        // spawned must NOT be matched.
        let spawn = 1_000;
        let prev = cand("prev.jsonl", 500, "/repo", 400); // mtime well before spawn
        assert_eq!(pick_best_rollout(&[prev], "/repo", None, spawn), None);
    }

    #[test]
    fn pick_prefers_resumed_session_over_a_fresh_unrelated_one() {
        // The "long session goes stale" bug: a resumed session has an OLD start
        // time (frozen session_meta) but a RECENT mtime. It must win over an
        // unrelated session that merely started near "now" but isn't being
        // written to as recently.
        let spawn = 10_000;
        let resumed = cand("resumed.jsonl", 10_900, "/repo", 50); // old start, freshest mtime
        let other = cand("other.jsonl", 10_400, "/repo", 9_999); // start near spawn, older mtime
        let best = pick_best_rollout(&[other, resumed], "/repo", None, spawn);
        assert_eq!(best, Some(PathBuf::from("resumed.jsonl")));
    }

    #[test]
    fn pick_filters_by_cwd() {
        let spawn = 1_000;
        let wrong = cand("wrong.jsonl", 2_000, "/other", 1_001);
        let right = cand("right.jsonl", 1_500, "/repo", 1_001);
        let best = pick_best_rollout(&[wrong, right], "/repo", None, spawn);
        assert_eq!(best, Some(PathBuf::from("right.jsonl")));
    }

    #[test]
    fn pick_tie_breaks_equal_mtime_by_start_closest_to_spawn() {
        // Two fresh codex panes sharing a cwd, files flushed in the same second:
        // prefer the one whose start is nearest this pane's spawn.
        let spawn = 5_000;
        let mine = cand("mine.jsonl", 6_000, "/repo", 5_001);
        let theirs = cand("theirs.jsonl", 6_000, "/repo", 5_200);
        let best = pick_best_rollout(&[theirs, mine], "/repo", None, spawn);
        assert_eq!(best, Some(PathBuf::from("mine.jsonl")));
    }

    #[test]
    fn pick_allows_small_clock_skew() {
        // A file whose mtime is a hair before the recorded spawn (sub-second
        // rounding) still counts as written during the pane's life.
        let spawn = 1_000;
        let c = cand("c.jsonl", 1_000 - (MTIME_SKEW_SECS - 1), "/repo", 999);
        assert_eq!(
            pick_best_rollout(&[c], "/repo", None, spawn),
            Some(PathBuf::from("c.jsonl"))
        );
    }
}
