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
    let base = if let Some(xdg) = std::env::var_os("XDG_STATE_HOME") {
        PathBuf::from(xdg).join("spyc")
    } else {
        PathBuf::from(std::env::var_os("HOME")?).join(".local/state/spyc")
    };
    Some(base.join("sessions"))
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

/// Claude session info returned by `find_claude_session`.
pub struct ClaudeSessionInfo {
    pub session_id: String,
    pub name: Option<String>,
}

/// Find the most recent Claude Code session for a given working directory.
/// Scans `~/.claude/sessions/*.json` for entries whose `cwd` matches.
pub fn find_claude_session(cwd: &std::path::Path) -> Option<ClaudeSessionInfo> {
    let home = std::env::var_os("HOME")?;
    let dir = PathBuf::from(home).join(".claude/sessions");
    let entries = std::fs::read_dir(&dir).ok()?;

    let mut best: Option<(u64, String, Option<String>)> = None; // (startedAt, sessionId, name)

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
        let cwd_str = cwd.to_string_lossy();
        let matches = session_cwd == cwd_str
            || session_cwd.strip_prefix("/private").unwrap_or(session_cwd) == cwd_str.as_ref();

        if matches && best.as_ref().is_none_or(|(ts, _, _)| started_at > *ts) {
            best = Some((started_at, session_id.to_string(), name));
        }
    }

    best.map(|(_, id, name)| {
        // Session name may be in the conversation JSONL as a
        // `custom-title` entry rather than in the session file.
        let name = name.or_else(|| find_claude_session_name(&id));
        ClaudeSessionInfo {
            session_id: id,
            name,
        }
    })
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

    // Note: save/load/prune tests use the shared XDG_STATE_HOME env var.
    // The lock from `crate::state::env_test_lock()` serializes us
    // against the other state-module tests that mutate the same env
    // var (graveyard / harpoon / inventory / marks).

    #[test]
    fn save_load_prune_and_dedup() {
        let _lock = crate::state::env_test_lock();
        let tmp = tempdir().unwrap();
        unsafe {
            std::env::set_var("XDG_STATE_HOME", tmp.path());
        }

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
        let dir = tmp.path().join("spyc/sessions");
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
    }
}
