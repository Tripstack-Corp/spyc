//! Named-directory marks, vi-style (`ma`, `'a`).
//!
//! A mark remembers:
//!   - The directory that was current when the mark was set.
//!   - Optionally the file the cursor was on, so jumping back lands on
//!     the same entry rather than the top of the listing.
//!
//! Persisted to `$XDG_STATE_HOME/cspy/marks.toml` (or
//! `$HOME/.local/state/cspy/marks.toml`) so marks survive restarts.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mark {
    pub dir: PathBuf,
    #[serde(default)]
    pub focus: Option<PathBuf>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Marks {
    // TOML only supports string keys, so we store single-char names as
    // one-char strings. The public API stays typed as `char`.
    #[serde(default)]
    pub entries: BTreeMap<String, Mark>,
}

impl Marks {
    pub fn get(&self, letter: char) -> Option<&Mark> {
        self.entries.get(&letter.to_string())
    }

    pub fn set(&mut self, letter: char, mark: Mark) {
        self.entries.insert(letter.to_string(), mark);
    }

    /// Where the marks file lives on disk (may return None on exotic
    /// systems with no $HOME or $XDG_STATE_HOME).
    pub fn disk_path() -> Option<PathBuf> {
        state_dir().map(|d| d.join("marks.toml"))
    }

    /// Best-effort load. Missing or malformed files silently yield empty.
    pub fn load() -> Self {
        let Some(path) = Self::disk_path() else {
            return Self::default();
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        toml::from_str(&text).unwrap_or_default()
    }

    /// Serialize and write atomically-ish. Creates the parent directory
    /// if needed.
    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = Self::disk_path() else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string(self).unwrap_or_default();
        std::fs::write(&path, text)
    }
}

fn state_dir() -> Option<PathBuf> {
    // XDG first, then the conventional fallback.
    if let Some(xdg) = std::env::var_os("XDG_STATE_HOME") {
        return Some(PathBuf::from(xdg).join("cspy"));
    }
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state/cspy"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn roundtrip_toml() {
        let mut m = Marks::default();
        m.set(
            'a',
            Mark {
                dir: PathBuf::from("/tmp/x"),
                focus: Some(PathBuf::from("/tmp/x/foo.rs")),
            },
        );
        m.set(
            'b',
            Mark {
                dir: PathBuf::from("/tmp/y"),
                focus: None,
            },
        );
        let text = toml::to_string(&m).unwrap();
        let parsed: Marks = toml::from_str(&text).unwrap();
        assert_eq!(parsed.entries.len(), 2);
        assert_eq!(parsed.get('a').unwrap().dir, PathBuf::from("/tmp/x"));
        assert_eq!(
            parsed.get('a').unwrap().focus.as_deref(),
            Some(std::path::Path::new("/tmp/x/foo.rs"))
        );
        assert!(parsed.get('b').unwrap().focus.is_none());
    }

    #[test]
    fn load_returns_empty_on_missing_file() {
        // Point XDG_STATE_HOME at an empty tempdir; no file there yet.
        let tmp = tempdir().unwrap();
        unsafe {
            std::env::set_var("XDG_STATE_HOME", tmp.path());
        }
        let m = Marks::load();
        assert!(m.entries.is_empty());
    }
}
