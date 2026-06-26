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
    Agy,
    Zot,
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
    /// The vertical (left/right) split open at save time — its shape and the
    /// previewed file — restored on `-r`. `None` when no split was open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vsplit: Option<SavedVsplit>,
}

/// Persisted vertical-split state for `-r` restore. Primitives only, so the
/// `sessions` layer stays free of `app::state` types (the app layer converts).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedVsplit {
    /// The right column's width share (percent).
    pub width_pct: u16,
    /// `true` = full-height layout, `false` = top-only.
    pub full_height: bool,
    /// `true` = the right column (`b`) owned the keyboard.
    pub focus_right: bool,
    /// The previewed file, re-loaded into the right column on restore (Stage-1
    /// preview split). Mutually exclusive with `right_cwd`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_path: Option<PathBuf>,
    /// The second *commander*'s (column `b`) cwd, reopened on restore (PR G).
    /// `None` for a preview split; `Some` for a full second commander.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right_cwd: Option<PathBuf>,
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

/// Slug for a cwd as Claude stores its conversations:
/// `/Users/x/src/spyc` → `-Users-x-src-spyc`.
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
    if let Ok(canon) = std::fs::canonicalize(cwd)
        && canon != cwd
        && projects.join(project_slug(&canon)).join(&file).exists()
    {
        return true;
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
    if let Some(stripped) = cwd_str.strip_prefix("/private")
        && let Some(name) = projects.get(stripped).and_then(|v| v.as_str())
    {
        return Some(name.to_string());
    }
    None
}

#[derive(Debug, Clone)]
pub struct AgySessionInfo {
    pub session_id: String,
    pub started_at_secs: u64,
}

impl SessionCandidate for AgySessionInfo {
    fn session_id(&self) -> &str {
        &self.session_id
    }
    fn started_at_secs(&self) -> u64 {
        self.started_at_secs
    }
}

/// Find every Agy session for a given cwd by parsing `history.jsonl`.
/// Agy writes all history to `~/.gemini/antigravity-cli/history.jsonl`.
///
/// Returned sorted by `started_at_secs` descending.
pub fn find_agy_sessions(cwd: &std::path::Path) -> Vec<AgySessionInfo> {
    let Some(home) = std::env::var_os("HOME") else {
        return Vec::new();
    };
    let history_path = PathBuf::from(home).join(".gemini/antigravity-cli/history.jsonl");
    let Ok(text) = std::fs::read_to_string(&history_path) else {
        return Vec::new();
    };

    let cwd_str = cwd.to_string_lossy();
    let mut sessions: std::collections::HashMap<String, u64> = std::collections::HashMap::new();

    for line in text.lines() {
        let Ok(val) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let Some(workspace) = val.get("workspace").and_then(|v| v.as_str()) else {
            continue;
        };
        let matches = workspace == cwd_str
            || workspace.strip_prefix("/private").unwrap_or(workspace) == cwd_str.as_ref();
        if !matches {
            continue;
        }
        let Some(conversation_id) = val.get("conversationId").and_then(|v| v.as_str()) else {
            continue;
        };
        let timestamp = val
            .get("timestamp")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let secs = timestamp / 1000;

        let entry = sessions.entry(conversation_id.to_string()).or_insert(secs);
        // We want the earliest timestamp seen for this conversationId
        if secs < *entry {
            *entry = secs;
        }
    }

    let mut found: Vec<AgySessionInfo> = sessions
        .into_iter()
        .map(|(session_id, started_at_secs)| AgySessionInfo {
            session_id,
            started_at_secs,
        })
        .collect();

    found.sort_by_key(|f| std::cmp::Reverse(f.started_at_secs));
    found
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
pub fn parse_iso8601_to_epoch_secs(s: &str) -> Option<u64> {
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
/// (`pub`, not `pub(crate)`: the enclosing `sessions` module is private, so
/// clippy's `redundant_pub_crate` rejects `pub(crate)` here.)
pub fn find_claude_session_name(session_id: &str) -> Option<String> {
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
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(line)
                && val["type"].as_str() == Some("custom-title")
                && let Some(title) = val["customTitle"].as_str()
                && !title.is_empty()
            {
                return Some(title.to_string());
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
        if let Some(rest) = trimmed.strip_prefix("claude --resume ")
            && let Some(tok) = rest.split_whitespace().next()
        {
            let tok = tok.trim();
            if !tok.is_empty() {
                return Some(tok.to_string());
            }
        }
    }
    None
}

/// Scan `lines` in reverse (most-recent banner wins) for the first line
/// containing any of `markers`, returning the whitespace-delimited token
/// immediately after the marker when it satisfies `valid`. Uses `find`
/// (not `strip_prefix`) so a leading "To continue this session, run "
/// prefix and any trailing color-reset bytes on the same render line are
/// tolerated.
///
/// Shared by the codex / agy extractors below. (The claude extractor stays
/// separate: it anchors with `strip_prefix` and accepts a session *name*,
/// not just a UUID.)
fn extract_token_after(
    lines: &[String],
    markers: &[&str],
    valid: impl Fn(&str) -> bool,
) -> Option<String> {
    for line in lines.iter().rev() {
        let trimmed = line.trim();
        for marker in markers {
            if let Some(idx) = trimmed.find(marker) {
                let rest = &trimmed[idx + marker.len()..];
                if let Some(tok) = rest.split_whitespace().next() {
                    let tok = tok.trim();
                    if valid(tok) {
                        return Some(tok.to_string());
                    }
                }
            }
        }
    }
    None
}

/// Scan pane scrollback for the exit banner codex prints on a clean exit:
/// `To continue this session, run codex resume <UUID>`. Returns just the
/// UUID — codex doesn't have thread-name resume tokens.
pub fn extract_codex_resume_token(lines: &[String]) -> Option<String> {
    extract_token_after(lines, &["codex resume "], is_uuid)
}

/// Scan pane scrollback for the exit banner agy prints on exit
/// (`agy --conversation <UUID>` or its `-c` short form). Returns the UUID.
pub fn extract_agy_resume_token(lines: &[String]) -> Option<String> {
    extract_token_after(lines, &["agy --conversation ", "agy -c "], is_uuid)
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

// Short-id resolution for the status bar's agent segment now lives in
// each `AgentProfile::resolve_short_id` (see `crate::agent`), driven by
// the shared `closest_short_id` helper. `short_id` below is the shared
// formatter both the profiles and tests use.

pub fn short_id(uuid: &str) -> String {
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
mod tests;
