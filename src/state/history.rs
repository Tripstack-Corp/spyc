//! Shared command history for `!` and `;` prompts.
//!
//! Persisted to `$XDG_STATE_HOME/cspy/history` (or
//! `$HOME/.local/state/cspy/history`), one command per line.
//! Deduplicates consecutive repeats.

use std::path::PathBuf;

const MAX_ENTRIES: usize = 1000;

#[derive(Debug, Clone)]
pub struct History {
    entries: Vec<String>,
    /// Points into `entries` while the user is cycling through history.
    /// `None` = not browsing; `Some(i)` = showing `entries[i]`.
    nav: Option<usize>,
    /// The text the user had typed before starting to browse — restored
    /// if they cycle past the end (back to the "live" buffer).
    stashed: String,
    /// Filename within the state dir (e.g. "history", "pane_history").
    filename: String,
}

impl History {
    pub fn load() -> Self {
        Self::load_file("history")
    }

    pub fn load_file(filename: &str) -> Self {
        let entries = disk_path(filename)
            .and_then(|p| std::fs::read_to_string(&p).ok())
            .map(|text| {
                text.lines()
                    .filter(|l| !l.is_empty())
                    .map(String::from)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        Self {
            entries,
            nav: None,
            stashed: String::new(),
            filename: filename.to_string(),
        }
    }

    /// Append a command. Removes any earlier duplicate so the most recent
    /// use is always at the end. Saves to disk best-effort.
    pub fn push(&mut self, cmd: &str) {
        let cmd = cmd.trim();
        if cmd.is_empty() {
            return;
        }
        // Remove earlier duplicate (move-to-end dedup).
        self.entries.retain(|e| e != cmd);
        self.entries.push(cmd.to_string());
        // Cap at MAX_ENTRIES.
        if self.entries.len() > MAX_ENTRIES {
            let drain = self.entries.len() - MAX_ENTRIES;
            self.entries.drain(..drain);
        }
        let _ = self.save();
    }

    /// Start (or continue) browsing. `current_text` is the live buffer
    /// so we can stash it on first browse.
    pub fn prev(&mut self, current_text: &str) -> Option<&str> {
        if self.entries.is_empty() {
            return None;
        }
        match self.nav {
            None => {
                self.stashed = current_text.to_string();
                let idx = self.entries.len() - 1;
                self.nav = Some(idx);
                Some(&self.entries[idx])
            }
            Some(i) if i > 0 => {
                let idx = i - 1;
                self.nav = Some(idx);
                Some(&self.entries[idx])
            }
            _ => None, // Already at the oldest entry.
        }
    }

    /// Move forward through history. Returns None when past the newest
    /// entry — caller should restore the stashed text.
    pub fn next(&mut self) -> Option<&str> {
        let Some(i) = self.nav else {
            return None;
        };
        if i + 1 < self.entries.len() {
            let idx = i + 1;
            self.nav = Some(idx);
            Some(&self.entries[idx])
        } else {
            // Past the newest — return to live buffer.
            self.nav = None;
            None
        }
    }

    /// The stashed text from before the user started browsing.
    pub fn stashed(&self) -> &str {
        &self.stashed
    }

    /// Reset the browse position (call when the prompt is dismissed).
    pub fn reset_nav(&mut self) {
        self.nav = None;
        self.stashed.clear();
    }

    /// Read-only access to the full entry list (oldest first).
    pub fn entries(&self) -> &[String] {
        &self.entries
    }

    /// Remove the entry at `index`. Saves to disk best-effort.
    pub fn remove(&mut self, index: usize) {
        if index < self.entries.len() {
            self.entries.remove(index);
            let _ = self.save();
        }
    }

    fn save(&self) -> std::io::Result<()> {
        let Some(path) = disk_path(&self.filename) else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text: String = self.entries.join("\n") + "\n";
        std::fs::write(&path, text)
    }
}

fn disk_path(filename: &str) -> Option<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_STATE_HOME") {
        return Some(PathBuf::from(xdg).join(format!("cspy/{filename}")));
    }
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(format!(".local/state/cspy/{filename}")))
}
