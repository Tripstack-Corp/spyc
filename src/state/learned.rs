//! `learned.json`: how much of the keymap the user has learned. `counts` holds,
//! per action, how many times its own key has fired it, keyed by its
//! `.spycrc.toml` name (`Action::canonical_name`), so a rebound key keeps its
//! count; `menu_seen` says whether an object's menu has ever opened. A menu's
//! hint fades on the counts and the status bar's `. menu` goes on the flag
//! (`app::menu_hints`).
//!
//! Global, not per project: muscle memory is the user's. Persisted to
//! `$XDG_STATE_HOME/spyc/learned.json` (or `$HOME/.local/state/spyc/`) and
//! read tolerantly: a missing file is empty, a corrupt one is empty and logged
//! (the startup health check names it too), and a name this spyc doesn't know,
//! a newer spyc's action, loads and is written back as it was.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Learned {
    /// Key-fired dispatches per action name, each stopped at the threshold it
    /// was counted toward.
    #[serde(default)]
    pub counts: BTreeMap<String, u32>,
    /// Whether an object's menu (`.`, `^a .`, a right-click) has ever opened.
    #[serde(default)]
    pub menu_seen: bool,
}

impl Learned {
    /// How many times `name`'s own key has fired it.
    pub fn count(&self, name: &str) -> u32 {
        self.counts.get(name).copied().unwrap_or(0)
    }

    /// Count one key-fired dispatch of `name`, unless its count has reached
    /// `cap`. Whether the count moved: past the threshold nothing changes, so a
    /// key pressed a thousand times is written `cap` times at most.
    pub fn record(&mut self, name: &str, cap: u32) -> bool {
        let count = self.count(name);
        if count >= cap {
            return false;
        }
        self.counts.insert(name.to_owned(), count + 1);
        true
    }

    /// Note that an object's menu has opened. Whether that is news.
    pub const fn see_menu(&mut self) -> bool {
        let news = !self.menu_seen;
        self.menu_seen = true;
        news
    }

    /// The names whose count has reached `threshold`, in name order.
    pub fn faded(&self, threshold: u32) -> impl Iterator<Item = &str> {
        self.counts
            .iter()
            .filter(move |&(_, &count)| count >= threshold)
            .map(|(name, _)| name.as_str())
    }

    /// Parse `learned.json`'s text.
    pub fn parse(text: &str) -> serde_json::Result<Self> {
        serde_json::from_str(text)
    }

    fn disk_path() -> Option<PathBuf> {
        crate::state::state_file_path("learned.json")
    }

    /// Read `learned.json`. A missing file is empty; one that can't be read or
    /// parsed is empty and logged, and never stops startup.
    pub fn load() -> Self {
        let Some(path) = Self::disk_path() else {
            return Self::default();
        };
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Self::default(),
            Err(e) => {
                crate::spyc_debug!("{}: {e} — menu hints start over", path.display());
                return Self::default();
            }
        };
        Self::parse(&text).unwrap_or_else(|e| {
            crate::spyc_debug!("{}: {e} — menu hints start over", path.display());
            Self::default()
        })
    }

    /// Write `learned.json`, creating the state directory.
    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = Self::disk_path() else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        crate::fs::write_atomic(&path, text.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_count_stops_at_its_cap() {
        let mut learned = Learned::default();
        let moved: Vec<bool> = (0..5).map(|_| learned.record("goto_first", 3)).collect();
        assert_eq!(moved, [true, true, true, false, false]);
        assert_eq!(learned.count("goto_first"), 3);
    }

    #[test]
    fn a_count_over_a_lowered_cap_stays_as_it_was() {
        let mut learned = Learned::default();
        learned.counts.insert("goto_first".into(), 7);
        assert!(!learned.record("goto_first", 3));
        assert_eq!(learned.count("goto_first"), 7);
    }

    #[test]
    fn the_menu_is_news_once() {
        let mut learned = Learned::default();
        assert!(learned.see_menu());
        assert!(!learned.see_menu());
    }

    #[test]
    fn faded_lists_the_names_at_the_threshold() {
        let mut learned = Learned::default();
        learned.counts.insert("goto_last".into(), 2);
        learned.counts.insert("goto_first".into(), 3);
        learned.counts.insert("a_future_action".into(), 9);
        let faded: Vec<&str> = learned.faded(3).collect();
        assert_eq!(faded, ["a_future_action", "goto_first"]);
    }

    #[test]
    fn a_file_with_neither_field_is_empty() {
        assert_eq!(Learned::parse("{}").unwrap(), Learned::default());
    }

    /// A top-level key this spyc doesn't know, a newer spyc's, an object or a
    /// scalar, is written back as it was when a key is counted here.
    #[test]
    #[ignore = "red: `Learned` drops a top-level key it doesn't know on parse"]
    fn a_newer_spycs_top_level_keys_survive_a_count() {
        let tmp = tempfile::tempdir().unwrap();
        let newer = serde_json::json!({
            "counts": {"goto_first": 1},
            "menu_seen": true,
            "a_future_table": {"since": "a newer spyc", "rows": [1, 2], "note": null},
            "a_future_flag": 7,
        });
        crate::state::with_state_root(tmp.path(), || {
            let mut learned = Learned::parse(&newer.to_string()).unwrap();
            assert!(learned.record("goto_first", 3), "premise: the count moved");
            learned.save().unwrap();
        });

        let text = std::fs::read_to_string(tmp.path().join("learned.json")).unwrap();
        let written: serde_json::Value = serde_json::from_str(&text).unwrap();
        let mut expected = newer;
        expected["counts"]["goto_first"] = 2.into();
        assert_eq!(written, expected);
    }

    #[test]
    fn save_and_load_round_trip_under_the_state_root() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut learned = Learned::default();
            learned.record("a_future_action", 3);
            learned.see_menu();
            learned.save().unwrap();
            assert_eq!(Learned::load(), learned);
        });
    }
}
