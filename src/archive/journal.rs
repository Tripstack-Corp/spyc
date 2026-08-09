//! Pending changes to a mounted archive, held until a repack.
//!
//! Nothing is rewritten as the user works: a delete, rename, add or replace is
//! recorded here and applied once, at write time. That's what lets a 500 MB
//! member be deleted without ever extracting it, and a rename cost zero bytes.
//!
//! **Every change is keyed on the archive's own namespace**, never on what the
//! user currently sees. Recording a delete against the displayed path would let
//! a later rename of an ancestor orphan it; keying on the index path means the
//! two are independent and can be applied in any order. [`Journal::effective`]
//! maps index path → displayed path, [`Journal::original_of`] maps back.

use std::collections::HashMap;
use std::time::SystemTime;

/// What a staged file looked like when spyc last wrote it.
///
/// The `(size, mtime)` pair is how an edit spyc didn't perform itself — `$EDITOR`
/// on a materialized member, or an agent in the pane — is noticed at repack
/// time. It's the trick WinRAR uses to know an opened member changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StagedStat {
    pub size: u64,
    pub mtime: SystemTime,
    pub is_dir: bool,
}

/// Staged bytes by journal path (the archive's namespace, not the displayed one).
pub type StagedStats = HashMap<String, StagedStat>;

/// One pending change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// A file the user brought in. Its bytes live in the staging tree under this
    /// same path, which is why a later rename never has to move them.
    Added { inner: String },
    /// Gone at repack time. Covers the whole subtree when `inner` is a
    /// directory.
    Deleted { inner: String },
    /// Same bytes, different path. Applies to the subtree for a directory.
    Renamed { from: String, to: String },
    /// Still there, but the staged copy supersedes the archived bytes.
    Replaced { inner: String },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub added: usize,
    pub deleted: usize,
    pub renamed: usize,
    pub replaced: usize,
}

impl Counts {
    pub const fn total(&self) -> usize {
        self.added + self.deleted + self.renamed + self.replaced
    }

    /// The status-bar badge for a dirty mount, e.g. `+2 ~1 -3`.
    pub fn badge(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.added > 0 {
            parts.push(format!("+{}", self.added));
        }
        if self.replaced + self.renamed > 0 {
            parts.push(format!("~{}", self.replaced + self.renamed));
        }
        if self.deleted > 0 {
            parts.push(format!("-{}", self.deleted));
        }
        parts.join(" ")
    }
}

#[derive(Debug, Clone, Default)]
pub struct Journal {
    changes: Vec<Change>,
}

impl Journal {
    pub const fn is_dirty(&self) -> bool {
        !self.changes.is_empty()
    }

    pub fn changes(&self) -> &[Change] {
        &self.changes
    }

    pub fn clear(&mut self) {
        self.changes.clear();
    }

    /// True when any rename is pending, which is what forces the listing off its
    /// prefix-range fast path.
    pub fn has_renames(&self) -> bool {
        self.changes
            .iter()
            .any(|c| matches!(c, Change::Renamed { .. }))
    }

    pub fn add(&mut self, inner: impl Into<String>) {
        self.changes.push(Change::Added {
            inner: inner.into(),
        });
    }

    pub fn delete(&mut self, inner: impl Into<String>) {
        self.changes.push(Change::Deleted {
            inner: inner.into(),
        });
    }

    pub fn rename(&mut self, from: impl Into<String>, to: impl Into<String>) {
        self.changes.push(Change::Renamed {
            from: from.into(),
            to: to.into(),
        });
    }

    pub fn replace(&mut self, inner: impl Into<String>) {
        self.changes.push(Change::Replaced {
            inner: inner.into(),
        });
    }

    /// Where an index path shows up now, after every pending rename. Renames
    /// apply in the order they were made, so a chain (`a`→`b`, `b`→`c`) resolves
    /// to the end of it.
    pub fn effective(&self, inner: &str) -> String {
        let mut current = inner.to_string();
        for change in &self.changes {
            if let Change::Renamed { from, to } = change
                && let Some(rest) = strip_path_prefix(&current, from)
            {
                current = join_rest(to, rest);
            }
        }
        current
    }

    /// Inverse of [`Self::effective`] — the index path behind a displayed one, so
    /// a change the user makes can be recorded in the archive's namespace.
    pub fn original_of(&self, effective: &str) -> String {
        let mut current = effective.to_string();
        for change in self.changes.iter().rev() {
            if let Change::Renamed { from, to } = change
                && let Some(rest) = strip_path_prefix(&current, to)
            {
                current = join_rest(from, rest);
            }
        }
        current
    }

    /// Whether an index path is gone — directly, or because an ancestor was
    /// deleted.
    pub fn is_deleted(&self, inner: &str) -> bool {
        self.changes.iter().any(|c| match c {
            Change::Deleted { inner: victim } => strip_path_prefix(inner, victim).is_some(),
            _ => false,
        })
    }

    /// Whether a staged copy supersedes this member's archived bytes.
    pub fn is_replaced(&self, inner: &str) -> bool {
        self.changes
            .iter()
            .any(|c| matches!(c, Change::Replaced { inner: r } if r == inner))
    }

    /// Paths the user added that are still present.
    pub fn additions(&self) -> impl Iterator<Item = &str> {
        self.changes
            .iter()
            .filter_map(|c| match c {
                Change::Added { inner } => Some(inner.as_str()),
                _ => None,
            })
            .filter(|inner| !self.is_deleted(inner))
    }

    pub fn counts(&self) -> Counts {
        let mut counts = Counts::default();
        for change in &self.changes {
            match change {
                Change::Added { .. } => counts.added += 1,
                Change::Deleted { .. } => counts.deleted += 1,
                Change::Renamed { .. } => counts.renamed += 1,
                Change::Replaced { .. } => counts.replaced += 1,
            }
        }
        counts
    }
}

/// `path` relative to `prefix` when `prefix` is `path` itself or one of its
/// ancestor directories; `None` otherwise.
///
/// The separator check is what keeps `a` from matching `ab/c` — a plain
/// `starts_with` would rename or delete the wrong subtree.
fn strip_path_prefix<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
    if path == prefix {
        return Some("");
    }
    path.strip_prefix(prefix)?.strip_prefix('/')
}

fn join_rest(base: &str, rest: &str) -> String {
    if rest.is_empty() {
        base.to_string()
    } else {
        format!("{base}/{rest}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_journal_is_clean() {
        let j = Journal::default();
        assert!(!j.is_dirty());
        assert!(!j.has_renames());
        assert_eq!(j.counts(), Counts::default());
        assert_eq!(j.effective("a/b.txt"), "a/b.txt");
    }

    #[test]
    fn a_rename_moves_the_file_and_its_subtree() {
        let mut j = Journal::default();
        j.rename("src", "source");
        assert_eq!(j.effective("src"), "source");
        assert_eq!(j.effective("src/main.rs"), "source/main.rs");
        assert_eq!(j.effective("src/a/b.rs"), "source/a/b.rs");
        assert_eq!(j.effective("docs/x.md"), "docs/x.md");
    }

    /// The prefix trap: renaming `src` must not touch `srcgen`.
    #[test]
    fn a_rename_respects_path_boundaries() {
        let mut j = Journal::default();
        j.rename("src", "source");
        assert_eq!(j.effective("srcgen/x.rs"), "srcgen/x.rs");
        assert_eq!(j.effective("src.txt"), "src.txt");
    }

    #[test]
    fn renames_chain_in_the_order_they_were_made() {
        let mut j = Journal::default();
        j.rename("a", "b");
        j.rename("b", "c");
        assert_eq!(j.effective("a/x"), "c/x");
        assert_eq!(j.original_of("c/x"), "a/x");
    }

    #[test]
    fn effective_and_original_round_trip() {
        let mut j = Journal::default();
        j.rename("src", "source");
        j.rename("source/main.rs", "source/lib.rs");
        for inner in ["src", "src/main.rs", "src/other.rs", "docs/a.md"] {
            let shown = j.effective(inner);
            assert_eq!(j.original_of(&shown), inner, "{inner} → {shown}");
        }
    }

    #[test]
    fn a_delete_covers_the_whole_subtree() {
        let mut j = Journal::default();
        j.delete("target");
        assert!(j.is_deleted("target"));
        assert!(j.is_deleted("target/debug/spyc"));
        assert!(!j.is_deleted("targetish"));
        assert!(!j.is_deleted("src"));
    }

    /// The reason changes are keyed on the index namespace: deleting a file and
    /// then renaming its parent must leave the file deleted.
    #[test]
    fn a_delete_survives_a_later_rename_of_its_parent() {
        let mut j = Journal::default();
        j.delete("src/old.rs");
        j.rename("src", "source");
        assert!(
            j.is_deleted("src/old.rs"),
            "the delete is keyed on the archive path, not the displayed one"
        );
        assert_eq!(j.effective("src/keep.rs"), "source/keep.rs");
    }

    #[test]
    fn additions_are_listed_until_deleted() {
        let mut j = Journal::default();
        j.add("notes.md");
        j.add("scratch.txt");
        assert_eq!(
            j.additions().collect::<Vec<_>>(),
            ["notes.md", "scratch.txt"]
        );
        j.delete("scratch.txt");
        assert_eq!(j.additions().collect::<Vec<_>>(), ["notes.md"]);
    }

    #[test]
    fn a_renamed_addition_keeps_its_staging_path() {
        let mut j = Journal::default();
        j.add("notes.md");
        j.rename("notes.md", "docs/notes.md");
        assert_eq!(
            j.additions().collect::<Vec<_>>(),
            ["notes.md"],
            "the staged bytes stay where they were written"
        );
        assert_eq!(j.effective("notes.md"), "docs/notes.md");
    }

    #[test]
    fn replacement_is_tracked_per_member() {
        let mut j = Journal::default();
        j.replace("Cargo.toml");
        assert!(j.is_replaced("Cargo.toml"));
        assert!(!j.is_replaced("Cargo.lock"));
    }

    #[test]
    fn counts_and_badge_summarize_the_pending_work() {
        let mut j = Journal::default();
        j.add("a");
        j.add("b");
        j.replace("c");
        j.rename("d", "e");
        j.delete("f");
        let counts = j.counts();
        assert_eq!(
            counts,
            Counts {
                added: 2,
                deleted: 1,
                renamed: 1,
                replaced: 1
            }
        );
        assert_eq!(counts.total(), 5);
        assert_eq!(counts.badge(), "+2 ~2 -1");
    }

    #[test]
    fn a_clean_badge_is_empty() {
        assert_eq!(Counts::default().badge(), "");
    }

    #[test]
    fn clear_drops_everything() {
        let mut j = Journal::default();
        j.delete("x");
        j.clear();
        assert!(!j.is_dirty());
        assert!(!j.is_deleted("x"));
    }
}
