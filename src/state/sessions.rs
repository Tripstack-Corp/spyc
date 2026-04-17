//! Session save / restore.
//!
//! Each session is a JSON snapshot of the workspace layout at quit time.
//! Stored in `$XDG_STATE_HOME/cspy/sessions/` (or `~/.local/state/cspy/sessions/`),
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
        PathBuf::from(xdg).join("cspy")
    } else {
        PathBuf::from(std::env::var_os("HOME")?).join(".local/state/cspy")
    };
    Some(base.join("sessions"))
}

pub fn save_session(session: &Session) -> std::io::Result<()> {
    let Some(dir) = sessions_dir() else {
        return Ok(());
    };
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", session.id));
    let json = serde_json::to_string_pretty(session)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
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
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|ext| ext == "json")
        })
        .filter_map(|e| {
            let text = std::fs::read_to_string(e.path()).ok()?;
            serde_json::from_str(&text).ok()
        })
        .collect();
    sessions.sort_by(|a, b| b.epoch_secs.cmp(&a.epoch_secs));
    // Dedup by cwd + tab commands (keep most recent).
    let mut seen = std::collections::HashSet::new();
    sessions.retain(|s| {
        let key = format!(
            "{}|{}",
            s.cwd.display(),
            s.tabs.iter().map(|t| t.command.as_str()).collect::<Vec<_>>().join(",")
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
        .filter_map(|e| e.ok())
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
