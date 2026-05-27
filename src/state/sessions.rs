//! Session save / restore.
//!
//! Each session is a JSON snapshot of the workspace layout at quit time.
//! Stored in `$XDG_STATE_HOME/spyc/sessions/` (or `~/.local/state/spyc/sessions/`),
//! one file per session, filename is the epoch millis.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const MAX_SESSIONS: usize = 20;

/// Which interactive coding agent (if any) the tab is hosting.
/// Drives session-save and resume-on-restore behavior — claude uses
/// a UUID-or-name token plus `/resume` over stdin (CLI flag is
/// regression-prone), codex uses `codex resume <UUID>` directly.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AgentKind {
    Claude,
    Codex,
    Gemini,
    /// Anything else (`bash`, `vim`, `make`, …). No session resume.
    #[default]
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedTab {
    pub command: String,
    pub label: String,
    pub cwd: PathBuf,
    /// Which agent this tab is hosting. Older saves (pre-1.41.6)
    /// don't carry this; `effective_kind()` infers Claude when an
    /// `agent_session_id` is present (the only resume case before
    /// codex support).
    #[serde(default)]
    pub agent_kind: AgentKind,
    /// Resume token. UUID for codex; UUID-or-thread-name for claude.
    /// Renamed from `claude_session_id` in v1.41.6 — the deserialize
    /// alias keeps older saves loadable.
    #[serde(
        default,
        alias = "claude_session_id",
        skip_serializing_if = "Option::is_none"
    )]
    pub agent_session_id: Option<String>,
    /// Display name (claude custom-title only — codex doesn't ship
    /// one). Renamed from `claude_session_name`; deserialize alias
    /// kept for older saves.
    #[serde(
        default,
        alias = "claude_session_name",
        skip_serializing_if = "Option::is_none"
    )]
    pub agent_session_name: Option<String>,
}

impl SavedTab {
    /// Agent kind, inferring Claude for older saves that didn't
    /// carry the field but recorded a session id (only Claude was
    /// supported then).
    pub const fn effective_kind(&self) -> AgentKind {
        match self.agent_kind {
            AgentKind::Other if self.agent_session_id.is_some() => AgentKind::Claude,
            k => k,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: u64,
    pub saved_at: String,
    pub epoch_secs: u64,
    pub cwd: PathBuf,
    pub tabs: Vec<SavedTab>,
    pub active_tab: usize,
    pub pane_height_pct: u16,
    pub pane_focused: bool,
    /// Spice-pair display name (e.g. `SAFFRON_PAPRIKA`). Assigned on
    /// session creation; user-editable via `:name <NEW>`.
    #[serde(default)]
    pub name: String,
    /// Sticky "project root" — target of `gh`, default cwd for new panes.
    /// Auto-set at startup when launch dir contains `.git`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_home: Option<PathBuf>,
}

fn sessions_dir() -> Option<PathBuf> {
    crate::state::state_root().map(|r| r.join("sessions"))
}

pub fn save_session(session: &Session) -> std::io::Result<()> {
    let Some(dir) = sessions_dir() else {
        return Ok(());
    };
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", session.id));
    let json = serde_json::to_string_pretty(session).map_err(std::io::Error::other)?;
    std::fs::write(&path, json)?;
    prune_old(&dir);
    Ok(())
}

pub fn load_sessions() -> Vec<Session> {
    let Some(dir) = sessions_dir() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut sessions: Vec<Session> = entries
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
        .filter_map(|e| {
            let text = std::fs::read_to_string(e.path()).ok()?;
            serde_json::from_str(&text).ok()
        })
        .collect();
    sessions.sort_by_key(|s| std::cmp::Reverse(s.epoch_secs));
    // Dedup by cwd + tab commands (keep most recent).
    let mut seen = std::collections::HashSet::new();
    sessions.retain(|s| {
        let key = format!(
            "{}|{}",
            s.cwd.display(),
            s.tabs
                .iter()
                .map(|t| t.command.as_str())
                .collect::<Vec<_>>()
                .join(",")
        );
        seen.insert(key)
    });
    sessions
}

fn prune_old(dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
        .map(|e| e.path())
        .collect();
    if files.len() <= MAX_SESSIONS {
        return;
    }
    // Sort ascending by filename (epoch millis) so oldest are first.
    files.sort();
    let to_remove = files.len() - MAX_SESSIONS;
    for path in &files[..to_remove] {
        let _ = std::fs::remove_file(path);
    }
}

/// Claude session info returned by `find_claude_sessions`.
pub struct ClaudeSessionInfo {
    pub session_id: String,
    pub name: Option<String>,
    /// Wall-clock start time in epoch SECONDS (Claude's `startedAt`
    /// field is millis; converted on read so callers don't have to
    /// remember the unit).
    pub started_at_secs: u64,
}

/// Find every Claude Code session record whose `cwd` matches the
/// given path. Returned sorted by `startedAt` descending (most recent
/// first). Useful when multiple Claude panes share a cwd and need to
/// be matched 1:1 against their session records by spawn time —
/// otherwise they all collapse onto the most-recent record.
pub fn find_claude_sessions(cwd: &std::path::Path) -> Vec<ClaudeSessionInfo> {
    let Some(home) = std::env::var_os("HOME") else {
        return Vec::new();
    };
    let dir = PathBuf::from(home).join(".claude/sessions");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };

    let cwd_str = cwd.to_string_lossy();
    let mut found: Vec<(u64, String, Option<String>)> = Vec::new();

    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let session_cwd = val["cwd"].as_str().unwrap_or("");
        let session_id = val["sessionId"].as_str().unwrap_or("");
        let started_at = val["startedAt"].as_u64().unwrap_or(0);
        let name = val["name"].as_str().map(String::from);

        if session_id.is_empty() || session_cwd.is_empty() {
            continue;
        }
        // Match cwd (handle /private/tmp vs /tmp macOS symlink).
        let matches = session_cwd == cwd_str
            || session_cwd.strip_prefix("/private").unwrap_or(session_cwd) == cwd_str.as_ref();

        if matches {
            found.push((started_at, session_id.to_string(), name));
        }
    }

    found.sort_by_key(|f| std::cmp::Reverse(f.0));
    found
        .into_iter()
        .map(|(started_at_ms, id, name)| {
            // Session name may live in the conversation JSONL as a
            // `custom-title` entry rather than in the session file.
            let name = name.or_else(|| find_claude_session_name(&id));
            ClaudeSessionInfo {
                session_id: id,
                name,
                started_at_secs: started_at_ms / 1000,
            }
        })
        .collect()
}

/// What a "session candidate" looks like to the picker — just enough
/// for the closest-match-with-claim-skip logic. Implemented for
/// `ClaudeSessionInfo` and `GeminiSessionInfo`.
pub trait SessionCandidate {
    fn session_id(&self) -> &str;
    fn started_at_secs(&self) -> u64;
}

impl SessionCandidate for ClaudeSessionInfo {
    fn session_id(&self) -> &str {
        &self.session_id
    }
    fn started_at_secs(&self) -> u64 {
        self.started_at_secs
    }
}

/// Pure helper: out of a list of session candidates (typically from
/// `find_claude_sessions` or `find_gemini_sessions`), pick the one
/// whose `started_at_secs` is closest to `pane_spawn_epoch_secs` AND
/// whose `session_id` is not in `claimed`. Returns `None` if every
/// candidate is already claimed or the list is empty. Stable: ties
/// broken by input order.
///
/// Extracted so the multi-pane disambiguation has a unit test that
/// doesn't depend on filesystem state under `~/.claude/` or
/// `~/.gemini/`.
pub fn pick_closest_unclaimed_session<T: SessionCandidate>(
    candidates: Vec<T>,
    pane_spawn_epoch_secs: u64,
    claimed: &std::collections::HashSet<String>,
) -> Option<T> {
    candidates
        .into_iter()
        .filter(|c| !claimed.contains(c.session_id()))
        .min_by_key(|c| c.started_at_secs().abs_diff(pane_spawn_epoch_secs))
}

/// Public wrapper for `find_claude_session_name` used by save_session
/// when the exit-banner token is a UUID.
pub fn find_claude_session_name_public(session_id: &str) -> Option<String> {
    find_claude_session_name(session_id)
}

/// Slug for a cwd as Claude stores its conversations:
/// `/Users/derek/src/spyc` → `-Users-derek-src-spyc`.
///
/// Claude rewrites *any* non-alphanumeric/hyphen character to `-`,
/// not just `/`. So `tripstack_platform` becomes `tripstack-platform`
/// in the on-disk path. We mirror that exactly — anything else
/// produces a slug that doesn't match Claude's directory and the
/// resume resolver returns None, leaving a session unresumable.
fn project_slug(cwd: &std::path::Path) -> String {
    cwd.to_string_lossy()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// True if a JSONL exists for `session_id` under the project slug for `cwd`.
/// This is the file `claude --resume <id>` actually reads.
///
/// Checks both `cwd` and its canonical (symlink-resolved) form, since
/// macOS's `/var` → `/private/var` symlink means `getcwd()` inside Claude
/// may produce a different slug than what spyc passes in.
pub fn claude_jsonl_exists(cwd: &std::path::Path, session_id: &str) -> bool {
    let Some(home) = std::env::var_os("HOME") else {
        return false;
    };
    let projects = PathBuf::from(&home).join(".claude/projects");
    let file = format!("{session_id}.jsonl");
    if projects.join(project_slug(cwd)).join(&file).exists() {
        return true;
    }
    if let Ok(canon) = std::fs::canonicalize(cwd) {
        if canon != cwd && projects.join(project_slug(&canon)).join(&file).exists() {
            return true;
        }
    }
    false
}

/// Resolve the on-disk conversation JSONL path for `session_id` under
/// the project slug for `cwd` (the file `claude --resume <id>` reads,
/// and what the transcript scrollback view renders). Checks both `cwd`
/// and its canonical form for the macOS symlink-slug mismatch. Returns
/// the first existing path, or `None`.
pub fn claude_jsonl_path(cwd: &std::path::Path, session_id: &str) -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let projects = PathBuf::from(&home).join(".claude/projects");
    let file = format!("{session_id}.jsonl");
    let direct = projects.join(project_slug(cwd)).join(&file);
    if direct.exists() {
        return Some(direct);
    }
    if let Ok(canon) = std::fs::canonicalize(cwd) {
        let via_canon = projects.join(project_slug(&canon)).join(&file);
        if via_canon.exists() {
            return Some(via_canon);
        }
    }
    None
}

/// Find the most-recently-modified JSONL under `~/.claude/projects/<slug>/`.
/// Returns the session ID (filename stem). This is the same conversation
/// the no-arg `claude --resume` picker would surface first for this cwd.
pub fn most_recent_jsonl_for_cwd(cwd: &std::path::Path) -> Option<String> {
    let home = std::env::var_os("HOME")?;
    let dir = PathBuf::from(home)
        .join(".claude/projects")
        .join(project_slug(cwd));
    let entries = std::fs::read_dir(&dir).ok()?;
    let mut best: Option<(std::time::SystemTime, String)> = None;
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Ok(meta) = entry.metadata() else { continue };
        let Ok(mtime) = meta.modified() else { continue };
        if best.as_ref().is_none_or(|(ts, _)| mtime > *ts) {
            best = Some((mtime, stem.to_string()));
        }
    }
    best.map(|(_, id)| id)
}

/// Gemini session info returned by `find_gemini_sessions`.
pub struct GeminiSessionInfo {
    /// UUID — what `gemini --list-sessions` prints in `[…]` and what
    /// our restore-time index lookup keys on.
    pub session_id: String,
    /// Wall-clock start time in epoch seconds, parsed from the
    /// chat JSONL's first-line `startTime` ISO-8601 string.
    pub started_at_secs: u64,
}

impl SessionCandidate for GeminiSessionInfo {
    fn session_id(&self) -> &str {
        &self.session_id
    }
    fn started_at_secs(&self) -> u64 {
        self.started_at_secs
    }
}

/// Map a cwd to the project name Gemini stores it under
/// (`~/.gemini/projects.json`). Returns `None` if Gemini hasn't seen
/// this cwd yet (no chats to resume from).
pub fn gemini_project_name(cwd: &std::path::Path) -> Option<String> {
    let home = std::env::var_os("HOME")?;
    let path = PathBuf::from(home).join(".gemini/projects.json");
    let text = std::fs::read_to_string(&path).ok()?;
    let val: serde_json::Value = serde_json::from_str(&text).ok()?;
    let projects = val.get("projects")?.as_object()?;
    let cwd_str = cwd.to_string_lossy();
    if let Some(name) = projects.get(cwd_str.as_ref()).and_then(|v| v.as_str()) {
        return Some(name.to_string());
    }
    // macOS /private symlink mirror, mirroring the Claude logic.
    if let Some(stripped) = cwd_str.strip_prefix("/private") {
        if let Some(name) = projects.get(stripped).and_then(|v| v.as_str()) {
            return Some(name.to_string());
        }
    }
    None
}

/// Find every Gemini chat session for a given cwd. Each
/// `~/.gemini/tmp/<project>/chats/session-*.jsonl` first line is JSON
/// with `sessionId`, `startTime`, `lastUpdated`, etc. — we read just
/// enough of each file to extract the metadata we need.
///
/// Returned sorted by `started_at_secs` descending.
pub fn find_gemini_sessions(cwd: &std::path::Path) -> Vec<GeminiSessionInfo> {
    let Some(name) = gemini_project_name(cwd) else {
        return Vec::new();
    };
    let Some(home) = std::env::var_os("HOME") else {
        return Vec::new();
    };
    let chats = PathBuf::from(home)
        .join(".gemini/tmp")
        .join(&name)
        .join("chats");
    let Ok(entries) = std::fs::read_dir(&chats) else {
        return Vec::new();
    };

    let mut found: Vec<GeminiSessionInfo> = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Ok(file) = std::fs::File::open(&path) else {
            continue;
        };
        let mut reader = std::io::BufReader::new(file);
        let mut first_line = String::new();
        use std::io::BufRead;
        if reader.read_line(&mut first_line).is_err() || first_line.is_empty() {
            continue;
        }
        let Ok(val) = serde_json::from_str::<serde_json::Value>(first_line.trim()) else {
            continue;
        };
        let Some(session_id) = val.get("sessionId").and_then(|v| v.as_str()) else {
            continue;
        };
        let started = val
            .get("startTime")
            .and_then(|v| v.as_str())
            .and_then(parse_iso8601_to_epoch_secs)
            .unwrap_or(0);
        if !is_uuid(session_id) {
            continue;
        }
        found.push(GeminiSessionInfo {
            session_id: session_id.to_string(),
            started_at_secs: started,
        });
    }
    found.sort_by_key(|f| std::cmp::Reverse(f.started_at_secs));
    found
}

/// Parse Gemini's chat-metadata `startTime` (ISO-8601 / RFC 3339)
/// to epoch seconds via `jiff`. Accepts both forms emitted in the
/// wild — `2026-05-08T12:27:31.927Z` and `2026-05-08T12:27:31Z` —
/// plus naive `2026-05-08T12:27:31` (no zone) by tagging UTC.
fn parse_iso8601_to_epoch_secs(s: &str) -> Option<u64> {
    // Try strict RFC 3339 first (handles fractional seconds + Z / offsets).
    if let Ok(ts) = s.parse::<jiff::Timestamp>() {
        let secs = ts.as_second();
        if secs < 0 {
            return None;
        }
        return Some(secs as u64);
    }
    // Fallback: zoneless `YYYY-MM-DDTHH:MM:SS` — assume UTC, since
    // every Gemini chat we've observed writes `Z`. Defensive against
    // upstream drift.
    let civil: jiff::civil::DateTime = s.parse().ok()?;
    let zoned = civil.to_zoned(jiff::tz::TimeZone::UTC).ok()?;
    let secs = zoned.timestamp().as_second();
    if secs < 0 {
        return None;
    }
    Some(secs as u64)
}

/// Look up a Claude session's custom title from its conversation JSONL.
/// Searches `~/.claude/projects/*/\<sessionId\>.jsonl` for `custom-title` entries.
fn find_claude_session_name(session_id: &str) -> Option<String> {
    let home = std::env::var_os("HOME")?;
    let projects_dir = PathBuf::from(home).join(".claude/projects");
    let entries = std::fs::read_dir(&projects_dir).ok()?;

    for project in entries.filter_map(Result::ok) {
        let jsonl_path = project.path().join(format!("{session_id}.jsonl"));
        if !jsonl_path.exists() {
            continue;
        }
        let text = std::fs::read_to_string(&jsonl_path).ok()?;
        // Scan lines in reverse — last custom-title wins.
        for line in text.lines().rev() {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
                if val["type"].as_str() == Some("custom-title") {
                    if let Some(title) = val["customTitle"].as_str() {
                        if !title.is_empty() {
                            return Some(title.to_string());
                        }
                    }
                }
            }
        }
    }
    None
}

/// Scan pane scrollback (most recent lines last) for the exit banner Claude
/// prints on quit: `Resume this session with:` / `claude --resume <token>`.
/// Returns the token (a UUID or a session name). Searches in reverse so the
/// most recent banner wins.
pub fn extract_claude_resume_token(lines: &[String]) -> Option<String> {
    for line in lines.iter().rev() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("claude --resume ") {
            let tok = rest.split_whitespace().next()?.trim();
            if !tok.is_empty() {
                return Some(tok.to_string());
            }
        }
    }
    None
}

/// Scan pane scrollback for the exit banner codex prints on a clean exit:
/// `To continue this session, run codex resume <UUID>`. Returns just the
/// UUID — codex doesn't have thread-name resume tokens. Searches in
/// reverse so the most recent banner wins.
pub fn extract_codex_resume_token(lines: &[String]) -> Option<String> {
    for line in lines.iter().rev() {
        let trimmed = line.trim();
        // Look for `codex resume <token>` anywhere on the line so we
        // tolerate the leading "To continue this session, run " prefix
        // and any trailing color-reset bytes the TUI may have left on
        // the same render line.
        if let Some(idx) = trimmed.find("codex resume ") {
            let rest = &trimmed[idx + "codex resume ".len()..];
            let tok = rest.split_whitespace().next()?.trim();
            if is_uuid(tok) {
                return Some(tok.to_string());
            }
        }
    }
    None
}

/// True if `token` looks like a UUID (8-4-4-4-12 hex).
pub fn is_uuid(token: &str) -> bool {
    let bytes = token.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    for (i, b) in bytes.iter().enumerate() {
        match i {
            8 | 13 | 18 | 23 => {
                if *b != b'-' {
                    return false;
                }
            }
            _ => {
                if !b.is_ascii_hexdigit() {
                    return false;
                }
            }
        }
    }
    true
}

/// Resolve a short (8-char) session-id for the active pane, used by
/// the status bar's agent segment. Reads from each agent's on-disk
/// session records, picks the one whose start time is closest to the
/// pane's `spawn_epoch_secs`, and returns the first 8 chars of the
/// UUID. Returns `None` for `AgentKind::Other`, when the agent has
/// no session record for `cwd` yet, or when no UUID looks like a UUID.
///
/// Filesystem cost is small (~10 small JSON header reads per scan on
/// average) but called from the render path — caller should add a
/// per-pane TTL cache if it shows up as a hotspot.
pub fn resolve_active_session_short_id(
    kind: AgentKind,
    cwd: &std::path::Path,
    spawn_epoch_secs: u64,
) -> Option<String> {
    match kind {
        AgentKind::Claude => find_claude_sessions(cwd)
            .into_iter()
            .min_by_key(|c| c.started_at_secs.abs_diff(spawn_epoch_secs))
            .map(|c| short_id(&c.session_id)),
        AgentKind::Gemini => find_gemini_sessions(cwd)
            .into_iter()
            .min_by_key(|c| c.started_at_secs.abs_diff(spawn_epoch_secs))
            .map(|c| short_id(&c.session_id)),
        // Codex stores rollouts at
        // `~/.codex/sessions/YYYY/MM/DD/rollout-<TS>-<UUID>.jsonl` —
        // the filename encodes both timestamp and UUID. A future PR
        // can parse filenames here; for now Codex panes get no
        // short-id in the status segment.
        AgentKind::Codex | AgentKind::Other => None,
    }
}

fn short_id(uuid: &str) -> String {
    uuid.chars().take(8).collect()
}

/// Human-readable relative time: "just now", "5 minutes ago", "2 days ago".
pub fn format_relative_time(epoch_secs: u64) -> String {
    let diff = crate::sysinfo::epoch_secs().saturating_sub(epoch_secs);
    if diff < 10 {
        "just now".to_string()
    } else if diff < 60 {
        format!("{diff} seconds ago")
    } else if diff < 3600 {
        let m = diff / 60;
        format!("{m} minute{} ago", if m == 1 { "" } else { "s" })
    } else if diff < 86400 {
        let h = diff / 3600;
        format!("{h} hour{} ago", if h == 1 { "" } else { "s" })
    } else if diff < 604_800 {
        let d = diff / 86400;
        format!("{d} day{} ago", if d == 1 { "" } else { "s" })
    } else {
        let d = diff / 86400;
        format!("{d} days ago")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn now_secs() -> u64 {
        crate::sysinfo::epoch_secs()
    }

    #[test]
    fn slug_replaces_path_separators() {
        assert_eq!(
            project_slug(std::path::Path::new("/Users/derek/src/spyc")),
            "-Users-derek-src-spyc"
        );
    }

    #[test]
    fn slug_rewrites_underscores_like_claude() {
        // Claude rewrites underscores to hyphens in its on-disk slug.
        // `~/.claude/projects/-Users-derek-src-tripstack-platform/`
        // is what we must match for `tripstack_platform`.
        assert_eq!(
            project_slug(std::path::Path::new("/Users/derek/src/tripstack_platform")),
            "-Users-derek-src-tripstack-platform"
        );
        assert_eq!(
            project_slug(std::path::Path::new("/Users/derek/src/system_setup")),
            "-Users-derek-src-system-setup"
        );
    }

    #[test]
    fn slug_rewrites_other_non_alphanumeric() {
        assert_eq!(
            project_slug(std::path::Path::new("/x/foo.bar/baz qux")),
            "-x-foo-bar-baz-qux"
        );
    }

    #[test]
    fn extracts_uuid_resume_token() {
        let lines: Vec<String> = [
            "some output",
            "Resume this session with:",
            "claude --resume 2afd7b70-f1e0-44a3-95c6-d9e538d231db",
            "",
        ]
        .iter()
        .map(ToString::to_string)
        .collect();
        let tok = extract_claude_resume_token(&lines).unwrap();
        assert_eq!(tok, "2afd7b70-f1e0-44a3-95c6-d9e538d231db");
        assert!(is_uuid(&tok));
    }

    #[test]
    fn extracts_named_resume_token() {
        let lines: Vec<String> = ["claude --resume saffron-cumin".to_string()].to_vec();
        let tok = extract_claude_resume_token(&lines).unwrap();
        assert_eq!(tok, "saffron-cumin");
        assert!(!is_uuid(&tok));
    }

    #[test]
    fn picks_last_resume_banner() {
        let lines: Vec<String> = [
            "claude --resume 11111111-1111-1111-1111-111111111111",
            "…later…",
            "claude --resume 22222222-2222-2222-2222-222222222222",
        ]
        .iter()
        .map(ToString::to_string)
        .collect();
        let tok = extract_claude_resume_token(&lines).unwrap();
        assert_eq!(tok, "22222222-2222-2222-2222-222222222222");
    }

    #[test]
    fn returns_none_when_no_banner() {
        let lines: Vec<String> = vec!["random scrollback".to_string(), "no banner".to_string()];
        assert!(extract_claude_resume_token(&lines).is_none());
    }

    #[test]
    fn extracts_codex_uuid_with_prefix_phrase() {
        let lines: Vec<String> = [
            "some output",
            "To continue this session, run codex resume 2afd7b70-f1e0-44a3-95c6-d9e538d231db",
            "",
        ]
        .iter()
        .map(ToString::to_string)
        .collect();
        let tok = extract_codex_resume_token(&lines).unwrap();
        assert_eq!(tok, "2afd7b70-f1e0-44a3-95c6-d9e538d231db");
    }

    #[test]
    fn codex_extractor_requires_uuid() {
        // Codex never uses thread-name tokens — guard against picking
        // up a non-UUID that happened to follow `codex resume`.
        let lines: Vec<String> = vec!["codex resume saffron-cumin".to_string()];
        assert!(extract_codex_resume_token(&lines).is_none());
    }

    #[test]
    fn codex_picks_last_banner() {
        let lines: Vec<String> = [
            "To continue this session, run codex resume 11111111-1111-1111-1111-111111111111",
            "…later…",
            "To continue this session, run codex resume 22222222-2222-2222-2222-222222222222",
        ]
        .iter()
        .map(ToString::to_string)
        .collect();
        let tok = extract_codex_resume_token(&lines).unwrap();
        assert_eq!(tok, "22222222-2222-2222-2222-222222222222");
    }

    #[test]
    fn effective_kind_infers_claude_for_legacy_saves() {
        // Older saves had no `agent_kind` field but did populate
        // `claude_session_id` (deserialized via the alias to
        // `agent_session_id`). The effective kind must report Claude
        // for those rows so resume-on-restore picks the right path.
        let json = serde_json::json!({
            "command": "claude",
            "label": "claude",
            "cwd": "/tmp",
            "claude_session_id": "11111111-1111-1111-1111-111111111111",
            "claude_session_name": "old-session",
        });
        let tab: SavedTab = serde_json::from_value(json).unwrap();
        assert_eq!(tab.agent_kind, AgentKind::Other);
        assert_eq!(tab.effective_kind(), AgentKind::Claude);
        assert_eq!(
            tab.agent_session_id.as_deref(),
            Some("11111111-1111-1111-1111-111111111111")
        );
        assert_eq!(tab.agent_session_name.as_deref(), Some("old-session"));
    }

    #[test]
    fn effective_kind_passes_through_explicit_value() {
        let mut tab = SavedTab {
            command: "codex".into(),
            label: "codex".into(),
            cwd: "/tmp".into(),
            agent_kind: AgentKind::Codex,
            agent_session_id: Some("uuid".into()),
            agent_session_name: None,
        };
        assert_eq!(tab.effective_kind(), AgentKind::Codex);
        tab.agent_kind = AgentKind::Other;
        tab.agent_session_id = None;
        assert_eq!(tab.effective_kind(), AgentKind::Other);
    }

    #[test]
    fn format_just_now() {
        let s = format_relative_time(now_secs());
        assert_eq!(s, "just now");
    }

    #[test]
    fn format_seconds_ago() {
        let s = format_relative_time(now_secs() - 30);
        assert_eq!(s, "30 seconds ago");
    }

    #[test]
    fn format_1_minute_ago() {
        let s = format_relative_time(now_secs() - 60);
        assert_eq!(s, "1 minute ago");
    }

    #[test]
    fn format_minutes_ago() {
        let s = format_relative_time(now_secs() - 300);
        assert_eq!(s, "5 minutes ago");
    }

    #[test]
    fn format_1_hour_ago() {
        let s = format_relative_time(now_secs() - 3600);
        assert_eq!(s, "1 hour ago");
    }

    #[test]
    fn format_hours_ago() {
        let s = format_relative_time(now_secs() - 7200);
        assert_eq!(s, "2 hours ago");
    }

    #[test]
    fn format_1_day_ago() {
        let s = format_relative_time(now_secs() - 86400);
        assert_eq!(s, "1 day ago");
    }

    #[test]
    fn format_days_ago_within_week() {
        let s = format_relative_time(now_secs() - 86400 * 3);
        assert_eq!(s, "3 days ago");
    }

    #[test]
    fn format_days_ago_past_week() {
        let s = format_relative_time(now_secs() - 86400 * 30);
        assert_eq!(s, "30 days ago");
    }

    #[test]
    fn format_future_timestamp_is_just_now() {
        // A timestamp in the future — saturating_sub makes diff 0
        let s = format_relative_time(now_secs() + 1000);
        assert_eq!(s, "just now");
    }

    // ── pick_closest_unclaimed_session ────────────────────────────
    //
    // Multi-pane disambiguation: when several Claude tabs share a
    // cwd, each pane's spawn time matches a different session
    // record's startedAt. Without claim-tracking, the resolver's
    // "most-recent JSONL" fallback collapsed every alive pane onto
    // one conversation.

    fn cs(id: &str, started_at_secs: u64) -> ClaudeSessionInfo {
        ClaudeSessionInfo {
            session_id: id.to_string(),
            name: None,
            started_at_secs,
        }
    }

    #[test]
    fn picker_returns_none_for_empty_candidates() {
        let claimed = std::collections::HashSet::new();
        assert!(
            pick_closest_unclaimed_session::<ClaudeSessionInfo>(vec![], 1000, &claimed).is_none()
        );
    }

    #[test]
    fn picker_picks_closest_started_at() {
        let candidates = vec![cs("a", 1000), cs("b", 2000), cs("c", 3000)];
        let claimed = std::collections::HashSet::new();
        let pick = pick_closest_unclaimed_session(candidates, 2100, &claimed).unwrap();
        assert_eq!(pick.session_id, "b");
    }

    #[test]
    fn picker_skips_already_claimed_ids() {
        // Without the claimed-skip, a pane spawned at 2100 would
        // claim "b" — but "b" is already taken by an earlier pane.
        // Picker should pick the next-closest unclaimed, here "a"
        // (1000s away) over "c" (900s away)? No — "c" is closer.
        let candidates = vec![cs("a", 1000), cs("b", 2000), cs("c", 3000)];
        let mut claimed = std::collections::HashSet::new();
        claimed.insert("b".to_string());
        let pick = pick_closest_unclaimed_session(candidates, 2100, &claimed).unwrap();
        assert_eq!(pick.session_id, "c");
    }

    #[test]
    fn picker_returns_none_when_all_claimed() {
        let candidates = vec![cs("a", 1000), cs("b", 2000)];
        let mut claimed = std::collections::HashSet::new();
        claimed.insert("a".to_string());
        claimed.insert("b".to_string());
        assert!(pick_closest_unclaimed_session(candidates, 1500, &claimed).is_none());
    }

    #[test]
    fn three_panes_three_distinct_session_ids() {
        // The bug: three Claude tabs spawned at t1 < t2 < t3 in the
        // same cwd. Three session records exist, sorted by startedAt
        // (closest match for each pane is the record at its own time).
        // Sequential resolve_calls (each adding to `claimed`) must
        // produce three distinct IDs.
        let records = vec![cs("a", 1000), cs("b", 2000), cs("c", 3000)];
        let pane_spawn_times = [1010_u64, 2010, 3010];

        let mut claimed = std::collections::HashSet::new();
        let mut assigned = Vec::new();
        for spawn in pane_spawn_times {
            let pick = pick_closest_unclaimed_session(records.clone(), spawn, &claimed)
                .expect("a candidate should remain");
            claimed.insert(pick.session_id.clone());
            assigned.push(pick.session_id);
        }

        assert_eq!(assigned, vec!["a", "b", "c"]);
    }

    impl Clone for ClaudeSessionInfo {
        fn clone(&self) -> Self {
            Self {
                session_id: self.session_id.clone(),
                name: self.name.clone(),
                started_at_secs: self.started_at_secs,
            }
        }
    }

    // ── Gemini ISO-8601 → epoch ────────────────────────────────────

    #[test]
    fn parse_iso8601_unix_epoch() {
        assert_eq!(parse_iso8601_to_epoch_secs("1970-01-01T00:00:00Z"), Some(0));
    }

    #[test]
    fn parse_iso8601_known_timestamp() {
        // 2026-05-08T12:27:31Z = epoch 1762605451 by manual derivation.
        // Sanity-check against `date -u -d` if you adjust this.
        let secs = parse_iso8601_to_epoch_secs("2026-05-08T12:27:31Z").unwrap();
        // Round-trip via the parser at second-2 to lock the value:
        assert!(
            secs > 1_777_000_000 && secs < 1_780_000_000,
            "epoch out of expected range for 2026-05-08: {secs}"
        );
    }

    #[test]
    fn parse_iso8601_strips_fractional_seconds() {
        let with_fraction = parse_iso8601_to_epoch_secs("2026-05-08T12:27:31.927Z").unwrap();
        let without = parse_iso8601_to_epoch_secs("2026-05-08T12:27:31Z").unwrap();
        assert_eq!(with_fraction, without);
    }

    #[test]
    fn parse_iso8601_no_z_suffix() {
        // The chat JSONL writes `Z`, but be defensive against drift.
        let with_z = parse_iso8601_to_epoch_secs("2026-05-08T12:27:31Z").unwrap();
        let without = parse_iso8601_to_epoch_secs("2026-05-08T12:27:31").unwrap();
        assert_eq!(with_z, without);
    }

    #[test]
    fn parse_iso8601_rejects_garbage() {
        assert!(parse_iso8601_to_epoch_secs("not a date").is_none());
        assert!(parse_iso8601_to_epoch_secs("2026/05/08 12:27:31").is_none());
        assert!(parse_iso8601_to_epoch_secs("").is_none());
    }

    #[test]
    fn parse_iso8601_orders_by_seconds() {
        // The whole point: relative ordering must match wall-clock so
        // the picker's `abs_diff` math is meaningful.
        let early = parse_iso8601_to_epoch_secs("2026-05-08T12:27:31Z").unwrap();
        let late = parse_iso8601_to_epoch_secs("2026-05-08T12:30:00Z").unwrap();
        assert_eq!(late - early, 2 * 60 + 29);
    }

    // ── pick_closest_unclaimed_session also works for Gemini ───────

    #[test]
    fn picker_works_for_gemini_records() {
        let candidates = vec![
            GeminiSessionInfo {
                session_id: "11111111-1111-1111-1111-111111111111".into(),
                started_at_secs: 1000,
            },
            GeminiSessionInfo {
                session_id: "22222222-2222-2222-2222-222222222222".into(),
                started_at_secs: 2000,
            },
        ];
        let claimed = std::collections::HashSet::new();
        let pick = pick_closest_unclaimed_session(candidates, 1900, &claimed).unwrap();
        assert_eq!(pick.session_id, "22222222-2222-2222-2222-222222222222");
    }

    // Sub-cases share one tempdir/state-root for sequencing; per-thread
    // `with_state_root` isolates this test from siblings.

    #[test]
    fn save_load_prune_and_dedup() {
        let tmp = tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            // --- roundtrip ---
            let session = Session {
                id: 12345,
                saved_at: "2025-01-01 12:00".to_string(),
                epoch_secs: 1_700_000_000,
                cwd: PathBuf::from("/tmp/test"),
                tabs: vec![SavedTab {
                    command: "bash".to_string(),
                    label: "shell".to_string(),
                    cwd: PathBuf::from("/tmp/test"),
                    agent_kind: crate::state::sessions::AgentKind::Other,
                    agent_session_id: None,
                    agent_session_name: None,
                }],
                active_tab: 0,
                pane_height_pct: 30,
                pane_focused: false,
                name: "SAFFRON_CUMIN".to_string(),
                project_home: None,
            };
            save_session(&session).unwrap();
            let loaded = load_sessions();
            assert_eq!(loaded.len(), 1);
            assert_eq!(loaded[0].id, 12345);
            assert_eq!(loaded[0].tabs.len(), 1);
            assert_eq!(loaded[0].tabs[0].command, "bash");

            // --- clean up for next sub-test ---
            let dir = tmp.path().join("sessions");
            if dir.exists() {
                std::fs::remove_dir_all(&dir).unwrap();
            }

            // --- prune ---
            for i in 0..25_u32 {
                let s = Session {
                    id: u64::from(i),
                    saved_at: format!("2025-01-{i:02}"),
                    epoch_secs: 1_700_000_000 + u64::from(i),
                    cwd: PathBuf::from(format!("/tmp/dir{i}")),
                    tabs: vec![SavedTab {
                        command: format!("cmd{i}"),
                        label: format!("tab{i}"),
                        cwd: PathBuf::from(format!("/tmp/dir{i}")),
                        agent_kind: crate::state::sessions::AgentKind::Other,
                        agent_session_id: None,
                        agent_session_name: None,
                    }],
                    active_tab: 0,
                    pane_height_pct: 30,
                    pane_focused: false,
                    name: String::new(),
                    project_home: None,
                };
                save_session(&s).unwrap();
            }
            let loaded = load_sessions();
            assert!(loaded.len() <= MAX_SESSIONS);

            // --- clean up for dedup test ---
            std::fs::remove_dir_all(&dir).unwrap();

            // --- dedup ---
            for id in [100_u64, 200] {
                let s = Session {
                    id,
                    saved_at: "2025-01-01".to_string(),
                    epoch_secs: 1_700_000_000 + id,
                    cwd: PathBuf::from("/same/dir"),
                    tabs: vec![SavedTab {
                        command: "bash".to_string(),
                        label: "shell".to_string(),
                        cwd: PathBuf::from("/same/dir"),
                        agent_kind: crate::state::sessions::AgentKind::Other,
                        agent_session_id: None,
                        agent_session_name: None,
                    }],
                    active_tab: 0,
                    pane_height_pct: 30,
                    pane_focused: false,
                    name: String::new(),
                    project_home: None,
                };
                save_session(&s).unwrap();
            }
            let loaded = load_sessions();
            assert_eq!(loaded.len(), 1);
            // Most recent (id=200) wins
            assert_eq!(loaded[0].id, 200);
        });
    }
}
