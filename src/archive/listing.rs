//! Index + journal → an `fs::Listing`, so a directory inside a mount renders
//! through exactly the same path as a real one.
//!
//! Nothing here touches the filesystem: the rows come from the archive's own
//! metadata. That's what makes entering a multi-gigabyte zip cost nothing — the
//! listing is the index, not an extraction.

use std::path::PathBuf;
use std::time::SystemTime;

use crate::fs::listing::{Listing, SortMode};
use crate::fs::{Entry, EntryKind};

use super::index::{ArchiveEntryKind, ArchiveIndex, IndexEntry};
use super::journal::{Journal, StagedStats};

/// Build the listing for `inner_dir` (a **displayed** path — post-rename, `""`
/// at the mount root).
///
/// With no renames pending this is a binary-searched prefix range over the
/// sorted index. A pending rename breaks that ordering — a member can be
/// displayed under a directory it doesn't live in — so the whole index is
/// scanned instead. That slow path only runs while the user has uncommitted
/// renames, which is a handful of entries in practice, and it keeps the common
/// case at the fast path rather than paying for the rare one everywhere.
pub fn listing_for(
    index: &ArchiveIndex,
    journal: &Journal,
    staged: &StagedStats,
    inner_dir: &str,
) -> Listing {
    let mut entries: Vec<Entry> = Vec::new();

    if journal.has_renames() {
        for e in &index.entries {
            if journal.is_deleted(&e.inner) {
                continue;
            }
            let shown = journal.effective(&e.inner);
            if parent_of(&shown) == inner_dir {
                entries.push(row(index, e, &shown));
            }
        }
    } else {
        let origin = journal.original_of(inner_dir);
        for e in index.children_of(&origin) {
            if journal.is_deleted(&e.inner) {
                continue;
            }
            entries.push(row(index, e, &e.inner));
        }
    }

    for added in journal.additions() {
        let shown = journal.effective(added);
        if parent_of(&shown) != inner_dir {
            continue;
        }
        let stat = staged.get(added);
        entries.push(Entry {
            path: index.mount_path(&shown),
            name: base_of(&shown).to_string(),
            kind: if stat.is_some_and(|s| s.is_dir) {
                EntryKind::Dir
            } else {
                EntryKind::File
            },
            size: stat.map_or(0, |s| s.size),
            mtime: stat.map_or(SystemTime::UNIX_EPOCH, |s| s.mtime),
        });
    }

    let mut listing = Listing {
        dir: index.mount_path(inner_dir),
        entries,
        // A capped index means members are missing from *some* directory, and we
        // can't tell which — so every listing in the mount says so, exactly as a
        // capped on-disk read does.
        truncated: index.truncated,
    };
    // Match `Listing::read`, which hands back name-sorted entries; the caller
    // re-sorts into the column's own order.
    listing.sort(SortMode::Name, false);
    listing
}

fn row(index: &ArchiveIndex, entry: &IndexEntry, shown: &str) -> Entry {
    Entry {
        path: index.mount_path(shown),
        name: base_of(shown).to_string(),
        kind: kind_of(entry),
        size: entry.size,
        mtime: entry.mtime.unwrap_or(SystemTime::UNIX_EPOCH),
    }
}

/// Archive kind → listing kind. The executable bit comes from the stored mode,
/// so a `+x` member is decorated like one on disk; an archive with no mode
/// information (a bare zip) lists everything as a plain file.
fn kind_of(entry: &IndexEntry) -> EntryKind {
    match entry.kind {
        ArchiveEntryKind::Dir => EntryKind::Dir,
        ArchiveEntryKind::Symlink => EntryKind::Symlink,
        ArchiveEntryKind::File => {
            if entry.mode.is_some_and(|m| m & 0o111 != 0) {
                EntryKind::Executable
            } else {
                EntryKind::File
            }
        }
    }
}

fn parent_of(inner: &str) -> &str {
    inner.rsplit_once('/').map_or("", |(p, _)| p)
}

fn base_of(inner: &str) -> &str {
    inner.rsplit_once('/').map_or(inner, |(_, n)| n)
}

/// The mount root's own display path, for the status bar.
pub fn mount_label(index: &ArchiveIndex, inner_dir: &str) -> String {
    let name = index
        .archive
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    if inner_dir.is_empty() {
        name
    } else {
        format!("{name}/{inner_dir}")
    }
}

/// Absolute staging path for an index entry's bytes.
pub fn staging_path(staging_root: &std::path::Path, entry: &IndexEntry) -> PathBuf {
    staging_root.join(entry.staging_rel())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::ArchiveFormat;
    use crate::archive::index::{Draft, IndexBuilder, Locator};

    fn index_of(names: &[(&str, ArchiveEntryKind, u64)]) -> ArchiveIndex {
        let mut b = IndexBuilder::new(1000);
        for (i, (name, kind, size)) in names.iter().enumerate() {
            let mut draft = Draft::file(*size, Locator::Zip { index: i });
            draft.kind = *kind;
            draft.mtime = Some(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1000));
            b.push(name, draft);
        }
        b.finish(PathBuf::from("/src/pkg.zip"), ArchiveFormat::Zip, 500)
            .0
    }

    fn sample() -> ArchiveIndex {
        index_of(&[
            ("README.md", ArchiveEntryKind::File, 10),
            ("src/main.rs", ArchiveEntryKind::File, 20),
            ("src/lib.rs", ArchiveEntryKind::File, 30),
            ("src/deep/mod.rs", ArchiveEntryKind::File, 40),
        ])
    }

    fn names(l: &Listing) -> Vec<String> {
        l.entries.iter().map(Entry::display_name).collect()
    }

    #[test]
    fn the_root_lists_top_level_members_dirs_first() {
        let index = sample();
        let l = listing_for(&index, &Journal::default(), &StagedStats::new(), "");
        assert_eq!(names(&l), ["src/", "README.md"]);
        assert_eq!(l.dir, PathBuf::from("/src/pkg.zip"));
    }

    /// Rows carry the archive's own paths, so every consumer downstream (picks,
    /// the cursor, an op) addresses a member the same way it addresses a file.
    #[test]
    fn rows_are_addressed_by_their_mount_path() {
        let index = sample();
        let l = listing_for(&index, &Journal::default(), &StagedStats::new(), "src");
        assert_eq!(l.dir, PathBuf::from("/src/pkg.zip/src"));
        let paths: Vec<&std::path::Path> = l.entries.iter().map(|e| e.path.as_path()).collect();
        assert!(paths.contains(&std::path::Path::new("/src/pkg.zip/src/main.rs")));
        assert!(paths.contains(&std::path::Path::new("/src/pkg.zip/src/deep")));
    }

    #[test]
    fn sizes_and_mtimes_come_from_the_archive() {
        let index = sample();
        let l = listing_for(&index, &Journal::default(), &StagedStats::new(), "src");
        let main = l.entries.iter().find(|e| e.name == "main.rs").unwrap();
        assert_eq!(main.size, 20);
        assert_eq!(
            main.mtime,
            SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1000)
        );
    }

    #[test]
    fn an_executable_mode_is_decorated_like_one_on_disk() {
        let mut b = IndexBuilder::new(10);
        let mut draft = Draft::file(1, Locator::Zip { index: 0 });
        draft.mode = Some(0o755);
        b.push("run.sh", draft);
        let mut plain = Draft::file(1, Locator::Zip { index: 1 });
        plain.mode = Some(0o644);
        b.push("data.txt", plain);
        let index = b.finish(PathBuf::from("/a.zip"), ArchiveFormat::Zip, 10).0;
        let l = listing_for(&index, &Journal::default(), &StagedStats::new(), "");
        assert_eq!(names(&l), ["run.sh*", "data.txt"]);
    }

    #[test]
    fn a_deleted_member_leaves_the_listing() {
        let index = sample();
        let mut j = Journal::default();
        j.delete("src/lib.rs");
        let l = listing_for(&index, &j, &StagedStats::new(), "src");
        assert_eq!(names(&l), ["deep/", "main.rs"]);
    }

    #[test]
    fn deleting_a_directory_hides_its_whole_subtree() {
        let index = sample();
        let mut j = Journal::default();
        j.delete("src/deep");
        let root = listing_for(&index, &j, &StagedStats::new(), "src");
        assert_eq!(names(&root), ["lib.rs", "main.rs"]);
        let inside = listing_for(&index, &j, &StagedStats::new(), "src/deep");
        assert!(inside.entries.is_empty());
    }

    /// The slow path: a renamed member shows up under its new parent and is gone
    /// from the old one, even though the index still says otherwise.
    #[test]
    fn a_rename_relocates_the_row() {
        let index = sample();
        let mut j = Journal::default();
        j.rename("src/main.rs", "main.rs");
        let root = listing_for(&index, &j, &StagedStats::new(), "");
        assert_eq!(
            root.entries.iter().filter(|e| e.name == "main.rs").count(),
            1
        );
        let src = listing_for(&index, &j, &StagedStats::new(), "src");
        assert_eq!(names(&src), ["deep/", "lib.rs"]);
    }

    #[test]
    fn a_renamed_directory_is_browsable_under_its_new_name() {
        let index = sample();
        let mut j = Journal::default();
        j.rename("src", "source");
        assert_eq!(
            names(&listing_for(&index, &j, &StagedStats::new(), "")),
            ["source/", "README.md"]
        );
        let renamed = listing_for(&index, &j, &StagedStats::new(), "source");
        assert_eq!(names(&renamed), ["deep/", "lib.rs", "main.rs"]);
        assert!(
            listing_for(&index, &j, &StagedStats::new(), "src")
                .entries
                .is_empty(),
            "nothing is left at the old path"
        );
    }

    #[test]
    fn an_added_file_appears_with_its_staged_size() {
        let index = sample();
        let mut j = Journal::default();
        j.add("notes.md");
        let mut staged = StagedStats::new();
        staged.insert(
            "notes.md".to_string(),
            crate::archive::journal::StagedStat {
                size: 77,
                mtime: SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(5),
                is_dir: false,
            },
        );
        let l = listing_for(&index, &j, &staged, "");
        let added = l.entries.iter().find(|e| e.name == "notes.md").unwrap();
        assert_eq!(added.size, 77);
        assert_eq!(added.path, PathBuf::from("/src/pkg.zip/notes.md"));
    }

    #[test]
    fn an_addition_lands_in_the_directory_it_was_put_in() {
        let index = sample();
        let mut j = Journal::default();
        j.add("src/new.rs");
        let root = listing_for(&index, &j, &StagedStats::new(), "");
        assert!(!root.entries.iter().any(|e| e.name == "new.rs"));
        let src = listing_for(&index, &j, &StagedStats::new(), "src");
        assert!(src.entries.iter().any(|e| e.name == "new.rs"));
    }

    #[test]
    fn a_capped_index_marks_every_listing_truncated() {
        let mut b = IndexBuilder::new(1);
        b.push("a.txt", Draft::file(1, Locator::Staged));
        b.push("b.txt", Draft::file(1, Locator::Staged));
        let index = b.finish(PathBuf::from("/a.zip"), ArchiveFormat::Zip, 10).0;
        let l = listing_for(&index, &Journal::default(), &StagedStats::new(), "");
        assert!(l.truncated);
    }

    #[test]
    fn an_empty_directory_lists_as_empty_not_missing() {
        let index = index_of(&[("empty/", ArchiveEntryKind::Dir, 0)]);
        let l = listing_for(&index, &Journal::default(), &StagedStats::new(), "empty");
        assert!(l.entries.is_empty());
        assert_eq!(l.dir, PathBuf::from("/src/pkg.zip/empty"));
    }

    #[test]
    fn the_mount_label_names_the_archive_and_the_path_within_it() {
        let index = sample();
        assert_eq!(mount_label(&index, ""), "pkg.zip");
        assert_eq!(mount_label(&index, "src/deep"), "pkg.zip/src/deep");
    }
}
