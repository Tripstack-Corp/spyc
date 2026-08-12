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
use std::path::PathBuf;
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

/// Staged bytes by **journal path**.
///
/// That's the archive's namespace — not the displayed one, and not the
/// staging-relative one. A case-colliding member stages under a prefix
/// ([`crate::archive::IndexEntry::staging_rel`]), so keying by the file's location
/// would give one member two different names depending on how you looked it up.
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

/// What a listing row should say about a member with a pending change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberChange {
    /// Brought in by the user; not in the archive yet.
    Added,
    /// Still in the archive, but the staged copy supersedes it — an edit.
    Replaced,
    /// Shown somewhere other than where the archive stores it.
    Renamed,
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

    /// What is pending for one member, for a per-row marker in the listing.
    ///
    /// `None` for a member nobody has touched. Deletion isn't a state here —
    /// a deleted member doesn't appear in the listing at all.
    pub fn state_of(&self, inner: &str) -> Option<MemberChange> {
        if self.additions().any(|a| a == inner) {
            return Some(MemberChange::Added);
        }
        if self.is_replaced(inner) {
            return Some(MemberChange::Replaced);
        }
        (self.effective(inner) != inner).then_some(MemberChange::Renamed)
    }

    /// Whether `inner` is a file the user brought in and hasn't written back.
    pub fn is_addition(&self, inner: &str) -> bool {
        self.additions().any(|added| added == inner)
    }

    /// Un-add a pending addition, as deleting one does.
    ///
    /// Recording a `Deleted` against it instead would also hide the row, but the
    /// archive never held those bytes — so the pair would describe removing
    /// something that was never there, and the staged copy would linger under a
    /// name a second put of the same file then collides with. Dropping the
    /// addition outright is what "delete a file I just brought in" means.
    ///
    /// Renames of the addition go with it: they name a path that no longer
    /// exists, and leaving one behind keeps the listing on its slow path forever.
    /// Returns whether anything was pending.
    pub fn forget_addition(&mut self, inner: &str) -> bool {
        let before = self.changes.len();
        self.changes.retain(|change| match change {
            Change::Added { inner: added } => added != inner,
            Change::Renamed { from, .. } => from != inner,
            _ => true,
        });
        self.changes.len() != before
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

    /// Deleting a file the user brought in un-adds it rather than recording a
    /// removal of something the archive never held — which is also what frees the
    /// name for a second put of the same file.
    #[test]
    fn un_adding_an_addition_leaves_no_trace_of_it() {
        let mut j = Journal::default();
        j.add("brought.txt");
        j.add("keep.txt");
        assert!(j.is_addition("brought.txt"));

        assert!(j.forget_addition("brought.txt"));
        assert!(!j.is_addition("brought.txt"));
        assert_eq!(j.additions().collect::<Vec<_>>(), ["keep.txt"]);
        assert!(
            !j.is_deleted("brought.txt"),
            "no removal is recorded for bytes the archive never had"
        );
        assert_eq!(j.counts().deleted, 0);
        assert!(!j.forget_addition("brought.txt"), "and it is idempotent");
    }

    /// A rename of the addition names a path that no longer exists, so it goes
    /// too — left behind it would hold the listing on its slow path forever.
    #[test]
    fn un_adding_takes_its_rename_with_it() {
        let mut j = Journal::default();
        j.add("brought.txt");
        j.rename("brought.txt", "renamed.txt");
        assert!(j.has_renames());

        assert!(j.forget_addition("brought.txt"));
        assert!(!j.is_dirty(), "nothing is left pending");
        assert!(!j.has_renames());
    }

    /// An archived member is untouched by the addition path: deleting one is
    /// still a recorded removal, because the bytes really are in the container.
    #[test]
    fn un_adding_does_not_apply_to_an_archived_member() {
        let mut j = Journal::default();
        assert!(!j.is_addition("src/main.rs"));
        assert!(!j.forget_addition("src/main.rs"));
        j.delete("src/main.rs");
        assert!(j.is_deleted("src/main.rs"));
        assert_eq!(j.counts().deleted, 1);
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

/// One member of the archive a repack is about to write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepackStep {
    /// The member's name in the *new* archive — post-rename.
    pub out: String,
    pub source: StepSource,
}

/// Where a repacked member's bytes come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepSource {
    /// The stored bytes, carried across as they are. For a zip that means a raw
    /// copy — no decompress, no recompress — which is what keeps a repack
    /// I/O-bound instead of a re-zip.
    Archived { inner: String },
    /// A file in the staging tree, `staging_root`-relative.
    ///
    /// `mode` is the **archive's** mode for a member the index still knows, and
    /// `None` for an addition, whose only mode is the one on disk. Staging's own
    /// permissions are spyc's (see `read::apply_mode`), so reading them back
    /// here would write spyc's cache policy into the user's archive — editing a
    /// `0o444` member would silently publish it as writable.
    Staging { rel: PathBuf, mode: Option<u32> },
}

/// The exact content list of the archive a write-back would produce.
///
/// The rule is one sentence: **every member is carried across unless it was
/// deleted, under its post-rename name, from staging if its bytes changed and
/// from the archive if they didn't.** Getting that wrong in the "didn't change"
/// direction costs a recompress; getting it wrong the other way loses an edit, so
/// an unknown staged file is treated as changed.
///
/// `recorded` is what spyc wrote at materialize time and `now` is what's on disk,
/// both keyed by journal path. Their difference is how an edit spyc didn't perform
/// — `$EDITOR`, or an agent in the pane — is noticed at all. Neither is stat'ed
/// here; the caller gathers both so this stays a pure decision.
pub fn plan_repack(
    index: &crate::archive::ArchiveIndex,
    journal: &Journal,
    recorded: &StagedStats,
    now: &StagedStats,
) -> Vec<RepackStep> {
    let mut steps: Vec<RepackStep> = Vec::new();
    for entry in &index.entries {
        // A directory the archive never stored is spyc's own scaffolding, so
        // emitting it would add an entry the original didn't have.
        if entry.locator == crate::archive::Locator::Implied {
            continue;
        }
        if journal.is_deleted(&entry.inner) {
            continue;
        }
        let out = journal.effective(&entry.inner);
        let source = if takes_from_staging(&entry.inner, journal, recorded, now) {
            StepSource::Staging {
                rel: entry.staging_rel(),
                mode: entry.mode,
            }
        } else {
            StepSource::Archived {
                inner: entry.inner.clone(),
            }
        };
        steps.push(RepackStep { out, source });
    }
    for added in journal.additions() {
        steps.push(RepackStep {
            out: journal.effective(added),
            source: StepSource::Staging {
                rel: PathBuf::from(added),
                mode: None,
            },
        });
    }
    steps
}

fn takes_from_staging(
    inner: &str,
    journal: &Journal,
    recorded: &StagedStats,
    now: &StagedStats,
) -> bool {
    if journal.is_replaced(inner) {
        return true;
    }
    match now.get(inner) {
        // Not extracted, so there is nothing local that could differ.
        None => false,
        // Extracted, and either unaccounted for or no longer what we wrote.
        Some(current) => recorded.get(inner) != Some(current),
    }
}

#[cfg(test)]
mod repack_tests {
    use super::*;
    use crate::archive::index::{Draft, IndexBuilder, Locator};
    use crate::archive::{ArchiveFormat, ArchiveIndex};

    fn index_of(members: &[&str]) -> ArchiveIndex {
        let mut b = IndexBuilder::new(1000);
        for (i, name) in members.iter().enumerate() {
            b.push(name, Draft::file(1, Locator::Zip { index: i }));
        }
        b.finish(PathBuf::from("/src/pkg.zip"), ArchiveFormat::Zip, 10)
            .0
    }

    fn stat(size: u64, secs: u64) -> StagedStat {
        StagedStat {
            size,
            mtime: SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs),
            is_dir: false,
        }
    }

    fn outs(steps: &[RepackStep]) -> Vec<&str> {
        steps.iter().map(|s| s.out.as_str()).collect()
    }

    /// A clean mount repacks to exactly what it was, every member carried across
    /// without touching its bytes.
    #[test]
    fn an_untouched_archive_repacks_to_itself() {
        let index = index_of(&["a.txt", "d/b.txt"]);
        let steps = plan_repack(
            &index,
            &Journal::default(),
            &StagedStats::new(),
            &StagedStats::new(),
        );
        // `d` is implied (only `d/b.txt` was stored), so it is not written back.
        assert_eq!(outs(&steps), ["a.txt", "d/b.txt"]);
        assert!(
            steps
                .iter()
                .all(|s| matches!(s.source, StepSource::Archived { .. })),
            "nothing is recompressed: {steps:?}"
        );
    }

    /// The directories spyc synthesized to make the tree walkable were never in
    /// the archive, so a repack must not invent them.
    #[test]
    fn implied_directories_are_not_written_back() {
        let index = index_of(&["deep/nested/a.txt"]);
        let steps = plan_repack(
            &index,
            &Journal::default(),
            &StagedStats::new(),
            &StagedStats::new(),
        );
        assert_eq!(outs(&steps), ["deep/nested/a.txt"]);
    }

    #[test]
    fn a_deleted_member_is_absent_from_the_plan() {
        let index = index_of(&["a.txt", "b.txt"]);
        let mut j = Journal::default();
        j.delete("a.txt");
        let steps = plan_repack(&index, &j, &StagedStats::new(), &StagedStats::new());
        assert_eq!(outs(&steps), ["b.txt"]);
    }

    #[test]
    fn deleting_a_directory_drops_its_whole_subtree() {
        let index = index_of(&["keep.txt", "d/one.txt", "d/two.txt"]);
        let mut j = Journal::default();
        j.delete("d");
        let steps = plan_repack(&index, &j, &StagedStats::new(), &StagedStats::new());
        assert_eq!(outs(&steps), ["keep.txt"]);
    }

    /// A rename changes the name in the output and nothing else — the bytes are
    /// still carried across untouched, which is why renaming costs nothing.
    #[test]
    fn a_rename_only_changes_the_output_name() {
        let index = index_of(&["src/main.rs"]);
        let mut j = Journal::default();
        j.rename("src", "source");
        let steps = plan_repack(&index, &j, &StagedStats::new(), &StagedStats::new());
        assert_eq!(outs(&steps), ["source/main.rs"]);
        assert_eq!(
            steps[0].source,
            StepSource::Archived {
                inner: "src/main.rs".to_string()
            },
            "still a raw copy, just under a new name"
        );
    }

    /// An extracted-but-untouched member still comes from the archive: taking it
    /// from staging would be correct but would recompress it for nothing.
    #[test]
    fn an_unchanged_extracted_member_is_still_copied_from_the_archive() {
        let index = index_of(&["a.txt"]);
        let mut recorded = StagedStats::new();
        recorded.insert("a.txt".to_string(), stat(10, 100));
        let now = recorded.clone();

        let steps = plan_repack(&index, &Journal::default(), &recorded, &now);
        assert!(matches!(steps[0].source, StepSource::Archived { .. }));
    }

    /// The WinRAR trick: an edit spyc never performed shows up as a staged file
    /// that no longer matches what spyc wrote.
    #[test]
    fn an_externally_edited_member_is_taken_from_staging() {
        let index = index_of(&["a.txt"]);
        let mut recorded = StagedStats::new();
        recorded.insert("a.txt".to_string(), stat(10, 100));
        let mut now = StagedStats::new();
        now.insert("a.txt".to_string(), stat(12, 200));

        let steps = plan_repack(&index, &Journal::default(), &recorded, &now);
        assert_eq!(
            steps[0].source,
            StepSource::Staging {
                rel: PathBuf::from("a.txt"),
                mode: index.get("a.txt").expect("indexed").mode,
            }
        );
    }

    /// A staged file spyc has no record of is treated as changed. Guessing wrong
    /// this way costs a recompress; guessing wrong the other way loses an edit.
    #[test]
    fn an_unaccounted_staged_file_is_treated_as_changed() {
        let index = index_of(&["a.txt"]);
        let mut now = StagedStats::new();
        now.insert("a.txt".to_string(), stat(10, 100));

        let steps = plan_repack(&index, &Journal::default(), &StagedStats::new(), &now);
        assert!(matches!(steps[0].source, StepSource::Staging { .. }));
    }

    #[test]
    fn an_explicit_replacement_comes_from_staging() {
        let index = index_of(&["a.txt"]);
        let mut j = Journal::default();
        j.replace("a.txt");
        let steps = plan_repack(&index, &j, &StagedStats::new(), &StagedStats::new());
        assert!(matches!(steps[0].source, StepSource::Staging { .. }));
    }

    #[test]
    fn additions_are_appended_from_staging() {
        let index = index_of(&["a.txt"]);
        let mut j = Journal::default();
        j.add("notes.md");
        let steps = plan_repack(&index, &j, &StagedStats::new(), &StagedStats::new());
        assert_eq!(outs(&steps), ["a.txt", "notes.md"]);
        assert_eq!(
            steps[1].source,
            StepSource::Staging {
                rel: PathBuf::from("notes.md"),
                // An addition has no index entry, so the file on disk is the only
                // thing that knows its mode.
                mode: None,
            }
        );
    }

    /// A case-colliding member stages under its own prefix, so the plan has to
    /// name that path rather than the member's — otherwise the repack would read
    /// the wrong file on a case-insensitive volume.
    #[test]
    fn a_case_colliding_member_is_read_from_its_isolated_staging_path() {
        let index = index_of(&["README", "readme"]);
        let mut j = Journal::default();
        j.replace("readme");
        let steps = plan_repack(&index, &j, &StagedStats::new(), &StagedStats::new());
        let staged = steps
            .iter()
            .find(|s| s.out == "readme")
            .expect("the colliding member is planned");
        assert_eq!(
            staged.source,
            StepSource::Staging {
                rel: index
                    .get("readme")
                    .expect("the colliding member is indexed")
                    .staging_rel(),
                mode: index
                    .get("readme")
                    .expect("the colliding member is indexed")
                    .mode,
            }
        );
    }

    proptest::proptest! {
        /// The invariant a repack rests on: every member is accounted for exactly
        /// once, and no two end up sharing a name. A duplicate output name would
        /// produce an archive that extracts differently than it lists; a missing
        /// one would silently drop a member the user never deleted.
        #[test]
        fn a_plan_never_loses_or_duplicates_a_member(
            keep in proptest::collection::vec("[a-c]{1,3}", 0..6),
            delete_first in proptest::bool::ANY,
            rename in proptest::bool::ANY,
        ) {
            let names: Vec<&str> = keep.iter().map(String::as_str).collect();
            let index = index_of(&names);
            let mut journal = Journal::default();
            let stored: Vec<String> = index
                .entries
                .iter()
                .filter(|e| e.locator != crate::archive::Locator::Implied)
                .map(|e| e.inner.clone())
                .collect();
            if delete_first && let Some(first) = stored.first() {
                journal.delete(first.clone());
            }
            if rename && let Some(last) = stored.last() {
                journal.rename(last.clone(), format!("{last}-renamed"));
            }

            let steps = plan_repack(&index, &journal, &StagedStats::new(), &StagedStats::new());

            let mut seen = std::collections::HashSet::new();
            for step in &steps {
                proptest::prop_assert!(
                    seen.insert(step.out.clone()),
                    "duplicate output name {:?}", step.out
                );
            }
            let expected = stored
                .iter()
                .filter(|inner| !journal.is_deleted(inner))
                .count();
            proptest::prop_assert_eq!(steps.len(), expected);
        }
    }
}
