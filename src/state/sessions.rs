//! Session save / restore.
//!
//! Each session is a JSON snapshot of the workspace layout at quit time.
//! Stored in `$XDG_STATE_HOME/spyc/sessions/` (or `~/.local/state/spyc/sessions/`),
//! one file per session, filename is the epoch millis.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const MAX_SESSIONS: usize = 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedTab {
    pub command: String,
    pub label: String,
    pub cwd: PathBuf,
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

/// Human-readable relative time: "just now", "5 minutes ago", "2 days ago".
pub fn format_relative_time(epoch_secs: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let diff = now.saturating_sub(epoch_secs);
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
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
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
    // Run them serially to avoid interference between parallel tests.
    // We use `#[serial_test::serial]` conceptually but since that's not a
    // dep, we combine them into a single test.

    #[test]
    fn save_load_prune_and_dedup() {
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
            }],
            active_tab: 0,
            pane_height_pct: 30,
            pane_focused: false,
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
                }],
                active_tab: 0,
                pane_height_pct: 30,
                pane_focused: false,
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
                }],
                active_tab: 0,
                pane_height_pct: 30,
                pane_focused: false,
            };
            save_session(&s).unwrap();
        }
        let loaded = load_sessions();
        assert_eq!(loaded.len(), 1);
        // Most recent (id=200) wins
        assert_eq!(loaded[0].id, 200);
    }
}
