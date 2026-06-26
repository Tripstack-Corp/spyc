//! Shared command history for `!` and `;` prompts.
//!
//! Persisted to `$XDG_STATE_HOME/spyc/history` (or
//! `$HOME/.local/state/spyc/history`), one command per line.
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
        let mut entries = disk_path(filename)
            .and_then(|p| std::fs::read_to_string(&p).ok())
            .map(|text| {
                text.lines()
                    .filter(|l| !l.is_empty())
                    .map(String::from)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        // Deduplicate: keep the *last* occurrence of each command (most recent).
        {
            let mut seen = std::collections::HashSet::new();
            let mut deduped = Vec::with_capacity(entries.len());
            for entry in entries.into_iter().rev() {
                if seen.insert(entry.clone()) {
                    deduped.push(entry);
                }
            }
            deduped.reverse();
            entries = deduped;
        }
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
    crate::state::state_root().map(|r| r.join(filename))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build an in-memory History. `push` will attempt best-effort saves
    /// to disk (harmless test artifact); the test assertions are purely
    /// in-memory.
    fn empty_history() -> History {
        History {
            entries: Vec::new(),
            nav: None,
            stashed: String::new(),
            filename: format!("test_history_{}", std::process::id()),
        }
    }

    #[test]
    fn empty_history_has_no_entries() {
        let h = empty_history();
        assert!(h.entries().is_empty());
    }

    #[test]
    fn push_adds_entry() {
        let mut h = empty_history();
        h.push("ls -la");
        assert_eq!(h.entries(), &["ls -la"]);
    }

    #[test]
    fn push_trims_whitespace() {
        let mut h = empty_history();
        h.push("  echo hello  ");
        assert_eq!(h.entries(), &["echo hello"]);
    }

    #[test]
    fn push_ignores_empty() {
        let mut h = empty_history();
        h.push("");
        h.push("   ");
        assert!(h.entries().is_empty());
    }

    #[test]
    fn push_deduplicates_moves_to_end() {
        let mut h = empty_history();
        h.push("first");
        h.push("second");
        h.push("first");
        assert_eq!(h.entries(), &["second", "first"]);
    }

    #[test]
    fn push_caps_at_max() {
        let mut h = empty_history();
        for i in 0..1050 {
            h.push(&format!("cmd-{i}"));
        }
        assert_eq!(h.entries().len(), MAX_ENTRIES);
        // Oldest commands were drained
        assert_eq!(h.entries()[0], "cmd-50");
        assert_eq!(h.entries().last().unwrap(), "cmd-1049");
    }

    #[test]
    fn prev_navigates_backward() {
        let mut h = empty_history();
        h.push("a");
        h.push("b");
        h.push("c");

        assert_eq!(h.prev("live"), Some("c"));
        assert_eq!(h.prev("live"), Some("b"));
        assert_eq!(h.prev("live"), Some("a"));
        assert_eq!(h.prev("live"), None); // at oldest
    }

    #[test]
    fn prev_stashes_current_text() {
        let mut h = empty_history();
        h.push("old");
        h.prev("my typing");
        assert_eq!(h.stashed(), "my typing");
    }

    #[test]
    fn next_navigates_forward() {
        let mut h = empty_history();
        h.push("a");
        h.push("b");
        h.push("c");

        h.prev("live");
        h.prev("live");
        h.prev("live"); // at "a"
        assert_eq!(h.next(), Some("b"));
        assert_eq!(h.next(), Some("c"));
        assert_eq!(h.next(), None); // past newest → restore stashed
    }

    #[test]
    fn next_without_prev_returns_none() {
        let mut h = empty_history();
        h.push("a");
        assert_eq!(h.next(), None);
    }

    #[test]
    fn reset_nav_clears_browsing() {
        let mut h = empty_history();
        h.push("a");
        h.prev("typing");
        h.reset_nav();
        assert_eq!(h.stashed(), "");
        // prev starts from the end again
        assert_eq!(h.prev("new typing"), Some("a"));
    }

    #[test]
    fn remove_entry() {
        let mut h = empty_history();
        h.push("a");
        h.push("b");
        h.push("c");
        h.remove(1); // remove "b"
        assert_eq!(h.entries(), &["a", "c"]);
    }

    #[test]
    fn remove_out_of_bounds_is_noop() {
        let mut h = empty_history();
        h.push("a");
        h.remove(5);
        assert_eq!(h.entries().len(), 1);
    }

    #[test]
    fn push_dedup_preserves_order_like_load() {
        // Tests the same dedup logic that load_file applies on read:
        // when the same command appears multiple times, only the last
        // occurrence is kept. We test via push() which uses the same
        // move-to-end dedup strategy, avoiding env-var races from
        // XDG_STATE_HOME.
        let mut h = empty_history();
        h.push("a");
        h.push("b");
        h.push("a");
        h.push("c");
        h.push("b");
        assert_eq!(h.entries(), &["a", "c", "b"]);
    }
}
