//! Remembered scroll positions for file-backed pager views.
//!
//! When the user opens a file in the pager, scrolls, and later
//! reopens the same file (in this spyc session or a future one),
//! restore the scroll position rather than dropping them at the
//! top. Only files (`PagerView::source_path = Some(_)`) participate
//! — command-output buffers, help overlays, pickers, etc. are
//! intentionally excluded because their content is ephemeral and
//! "start at top" is the expected interaction.
//!
//! Persisted to `$XDG_STATE_HOME/spyc/pager_positions.json` (or
//! `$HOME/.local/state/spyc/pager_positions.json`). LRU-capped at
//! [`MAX_ENTRIES`]; the file shrunk → clamp-to-last semantics live
//! at the caller (see `apply_initial_scroll`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const MAX_ENTRIES: usize = 500;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    /// Top-line index of the viewport when the view was last seen.
    /// `u64` (a fixed width for the persisted format, vs. the in-memory
    /// `usize` of `PagerView::scroll`) so positions in files past 65 535
    /// lines round-trip — old JSON written when this was `u16` still
    /// parses (any small number deserializes into `u64`).
    scroll: u64,
    /// Epoch seconds of the last save. Drives LRU eviction.
    last_visit: u64,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct PagerPositions {
    entries: HashMap<PathBuf, Entry>,
}

impl PagerPositions {
    /// Canonicalize for use as the HashMap key. `record` and `get`
    /// MUST agree on this — the original v1 of this module
    /// canonicalized on save but not on load, so a path like
    /// `./foo.md` from the listing-dir-relative caller would save
    /// as `/abs/foo.md` and then miss on lookup.
    fn key(path: &Path) -> PathBuf {
        std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
    }

    /// Look up the saved scroll for `path`. Returns `None` if the
    /// path was never seen.
    pub fn get(&self, path: &Path) -> Option<u64> {
        self.entries.get(&Self::key(path)).map(|e| e.scroll)
    }

    /// Record (or update) the scroll for `path`. Bumps `last_visit`
    /// and prunes if we're over the cap.
    pub fn record(&mut self, path: &Path, scroll: u64) {
        let now = crate::sysinfo::epoch_secs();
        self.entries.insert(
            Self::key(path),
            Entry {
                scroll,
                last_visit: now,
            },
        );
        if self.entries.len() > MAX_ENTRIES {
            self.prune();
        }
        let _ = self.save();
    }

    /// Drop the oldest entries until under the cap. Cheap — runs
    /// only on overflow.
    fn prune(&mut self) {
        let mut by_visit: Vec<(PathBuf, u64)> = self
            .entries
            .iter()
            .map(|(p, e)| (p.clone(), e.last_visit))
            .collect();
        by_visit.sort_by_key(|(_, v)| *v); // oldest first
        let to_remove = self.entries.len().saturating_sub(MAX_ENTRIES);
        for (p, _) in by_visit.into_iter().take(to_remove) {
            self.entries.remove(&p);
        }
    }

    fn disk_path() -> Option<PathBuf> {
        crate::state::state_root().map(|d| d.join("pager_positions.json"))
    }

    /// Best-effort load. Missing or malformed files silently yield empty.
    pub fn load() -> Self {
        let Some(path) = Self::disk_path() else {
            return Self::default();
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        serde_json::from_str(&text).unwrap_or_default()
    }

    /// Serialize and write. Creates the parent directory if needed.
    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = Self::disk_path() else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string(self).unwrap_or_default();
        std::fs::write(&path, text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_returns_none_for_unknown_path() {
        let pp = PagerPositions::default();
        assert!(pp.get(Path::new("/no/such/file")).is_none());
    }

    #[test]
    fn record_then_get_round_trips() {
        let mut pp = PagerPositions::default();
        // Use the source file itself so canonicalize succeeds.
        let here = std::env::current_dir().unwrap();
        pp.entries.insert(
            here.clone(),
            Entry {
                scroll: 42,
                last_visit: 0,
            },
        );
        assert_eq!(pp.get(&here), Some(42));
    }

    #[test]
    fn record_then_get_round_trip_through_canonicalization() {
        // Regression for the v1 bug: `record` canonicalized the
        // key but `get` looked up the raw path, so any caller
        // passing a slightly-different form (e.g. with `.`
        // segments, trailing slash, or `/private/tmp` vs `/tmp`
        // on macOS) silently missed. Test exercises a real file
        // so canonicalize() actually has an effect.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.md");
        std::fs::write(&path, "hello").unwrap();

        let mut pp = PagerPositions::default();
        pp.record(&path, 42);

        // Look up via the same path: must hit.
        assert_eq!(pp.get(&path), Some(42));

        // Look up via a redundant-segment form of the same path
        // (canonicalize collapses it). Without the symmetric
        // canonicalization in `get`, this would miss.
        let with_dot = path.parent().unwrap().join(".").join("note.md");
        assert_eq!(pp.get(&with_dot), Some(42));
    }

    #[test]
    fn prune_keeps_newest_when_over_cap() {
        let mut pp = PagerPositions::default();
        // Insert MAX + 5 entries with monotonically increasing
        // last_visit so the 5 oldest are clearly the eviction targets.
        for i in 0..(MAX_ENTRIES + 5) {
            pp.entries.insert(
                PathBuf::from(format!("/tmp/file{i}")),
                Entry {
                    scroll: 0,
                    last_visit: i as u64,
                },
            );
        }
        pp.prune();
        assert_eq!(pp.entries.len(), MAX_ENTRIES);
        // The 5 oldest (file0..file4) should be gone; file5..fileN+4 retained.
        assert!(!pp.entries.contains_key(&PathBuf::from("/tmp/file0")));
        assert!(pp.entries.contains_key(&PathBuf::from("/tmp/file5")));
    }
}
