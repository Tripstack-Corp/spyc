//! The entry table for a mounted archive, plus the name normalization that
//! makes everything downstream safe.
//!
//! Every raw member name is normalized on the way in, and anything that could
//! escape the mount (`..`, an absolute path, a NUL) is **rejected here** rather
//! than guarded against later — so zip-slip is structurally impossible for the
//! rest of the feature. The table is sorted by inner path, which is what lets a
//! directory's children be found by a prefix range instead of a full scan.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::ArchiveFormat;
use super::scan::IndexFacts;

/// What an entry is. Narrower than `fs::EntryKind` — an archive member is a
/// file, a directory, or a symlink; anything more exotic is refused by the scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveEntryKind {
    File,
    Dir,
    Symlink,
}

/// Where an entry's bytes come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Locator {
    /// Member `index` of the zip's central directory.
    Zip { index: usize },
    /// Byte offset of the member's data within an uncompressed tar.
    TarData { offset: u64 },
    /// Already extracted into the staging tree — the whole-archive streaming
    /// pass wrote it, or the user added it.
    Staged,
    /// A directory the archive never stored explicitly, synthesized so the tree
    /// is navigable. Carries no bytes and is never written back.
    Implied,
}

/// One archive member.
#[derive(Debug, Clone)]
pub struct IndexEntry {
    /// Normalized, `/`-separated, relative, no `.` or `..` components.
    pub inner: String,
    pub kind: ArchiveEntryKind,
    pub size: u64,
    pub mtime: Option<SystemTime>,
    pub mode: Option<u32>,
    pub uid: Option<u64>,
    pub gid: Option<u64>,
    pub link_target: Option<String>,
    pub locator: Locator,
    /// Nonzero when another entry differs from this one only by case.
    /// Materializing both under one directory would collide on a
    /// case-insensitive filesystem — every default macOS volume — so ranks
    /// above 0 stage under their own prefix. See [`IndexEntry::staging_rel`].
    pub case_rank: u32,
    /// False when spyc can't decompress these bytes (an encrypted or
    /// unsupported-method zip member). It still lists, and a repack still copies
    /// it verbatim; only *reading* it is refused.
    pub readable: bool,
}

impl IndexEntry {
    /// Path relative to the mount's staging root where this entry's bytes live
    /// once materialized. Identity for all but case-colliding entries, which
    /// would otherwise overwrite each other on macOS.
    pub fn staging_rel(&self) -> PathBuf {
        if self.case_rank == 0 {
            PathBuf::from(&self.inner)
        } else {
            PathBuf::from(format!(".spyc-case-{}", self.case_rank)).join(&self.inner)
        }
    }

    /// The last path component — the name a listing row shows.
    pub fn name(&self) -> &str {
        self.inner.rsplit_once('/').map_or(&*self.inner, |(_, n)| n)
    }

    /// The inner path of the directory holding this entry (`""` at the root).
    pub fn parent(&self) -> &str {
        self.inner.rsplit_once('/').map_or("", |(p, _)| p)
    }
}

/// A member's fields before its name has been normalized. Kept separate from
/// [`IndexEntry`] so `inner` can only ever be set by [`IndexBuilder::push`].
#[derive(Debug, Clone)]
pub struct Draft {
    pub kind: ArchiveEntryKind,
    pub size: u64,
    pub mtime: Option<SystemTime>,
    pub mode: Option<u32>,
    pub uid: Option<u64>,
    pub gid: Option<u64>,
    pub link_target: Option<String>,
    pub locator: Locator,
    pub readable: bool,
}

impl Draft {
    pub const fn file(size: u64, locator: Locator) -> Self {
        Self {
            kind: ArchiveEntryKind::File,
            size,
            mtime: None,
            mode: None,
            uid: None,
            gid: None,
            link_target: None,
            locator,
            readable: true,
        }
    }

    pub fn dir(locator: Locator) -> Self {
        Self {
            kind: ArchiveEntryKind::Dir,
            size: 0,
            ..Self::file(0, locator)
        }
    }
}

/// Why a member name was dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reject {
    /// Nothing left after normalization (`/`, `./`, or an empty name).
    Empty,
    /// A `..` component — the zip-slip / tar-slip shape.
    Traversal,
    /// An interior NUL, which no filesystem accepts.
    Nul,
}

impl Reject {
    pub const fn reason(self) -> &'static str {
        match self {
            Self::Empty => "empty name",
            Self::Traversal => "`..` path traversal",
            Self::Nul => "NUL in name",
        }
    }
}

/// A member name cleaned up for use as a relative path, plus the quirks that
/// were fixed on the way (the caller counts them so the mount can warn).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Normalized {
    pub inner: String,
    pub had_backslash: bool,
    pub was_absolute: bool,
}

/// Normalize a raw archive member name into a safe relative path.
///
/// Backslashes become separators: the zip spec mandates `/`, but archives
/// written by Windows tools do use `\`, and reading them as one long filename
/// would flatten the tree. A leading `/` is stripped rather than refused (it's
/// merely sloppy), while `..` is refused outright — that one is how an archive
/// escapes its mount.
pub fn normalize(raw: &str) -> Result<Normalized, Reject> {
    if raw.contains('\0') {
        return Err(Reject::Nul);
    }
    let had_backslash = raw.contains('\\');
    let unified = if had_backslash {
        raw.replace('\\', "/")
    } else {
        raw.to_string()
    };
    let was_absolute = unified.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for part in unified.split('/') {
        match part {
            "" | "." => {}
            ".." => return Err(Reject::Traversal),
            p => parts.push(p),
        }
    }
    if parts.is_empty() {
        return Err(Reject::Empty);
    }
    Ok(Normalized {
        inner: parts.join("/"),
        had_backslash,
        was_absolute,
    })
}

/// Accumulates members into an [`ArchiveIndex`]: normalizes names, drops unsafe
/// ones, resolves duplicates, and (at [`Self::finish`]) synthesizes the implied
/// directories and ranks case collisions.
pub struct IndexBuilder {
    entries: Vec<IndexEntry>,
    /// Inner path → position in `entries`, for last-wins duplicate handling.
    positions: HashMap<String, usize>,
    pub facts: IndexFacts,
    cap: usize,
    truncated: bool,
}

impl IndexBuilder {
    pub fn new(cap: usize) -> Self {
        Self {
            entries: Vec::new(),
            positions: HashMap::new(),
            facts: IndexFacts::default(),
            cap,
            truncated: false,
        }
    }

    /// Add one member. Returns `false` once the cap is reached, which is the
    /// caller's signal to stop walking — the index is marked truncated.
    ///
    /// A later member with the same normalized name replaces the earlier one,
    /// matching what extractors do (last write wins) so the listing agrees with
    /// what a materialize would produce.
    pub fn push(&mut self, raw_name: &str, draft: Draft) -> bool {
        if self.entries.len() >= self.cap {
            self.truncated = true;
            return false;
        }
        let normalized = match normalize(raw_name) {
            Ok(n) => n,
            Err(reject) => {
                self.facts.record_reject(reject);
                return true;
            }
        };
        if normalized.had_backslash {
            self.facts.backslash_names += 1;
        }
        if normalized.was_absolute {
            self.facts.absolute_names += 1;
        }
        let entry = IndexEntry {
            inner: normalized.inner,
            kind: draft.kind,
            size: draft.size,
            mtime: draft.mtime,
            mode: draft.mode,
            uid: draft.uid,
            gid: draft.gid,
            link_target: draft.link_target,
            locator: draft.locator,
            case_rank: 0,
            readable: draft.readable,
        };
        if let Some(&pos) = self.positions.get(&entry.inner) {
            self.facts.duplicates += 1;
            self.entries[pos] = entry;
        } else {
            self.positions
                .insert(entry.inner.clone(), self.entries.len());
            self.entries.push(entry);
        }
        true
    }

    pub fn finish(
        mut self,
        archive: PathBuf,
        format: ArchiveFormat,
        compressed_size: u64,
    ) -> (ArchiveIndex, IndexFacts) {
        self.synthesize_implied_dirs();
        self.entries.sort_unstable_by(|a, b| a.inner.cmp(&b.inner));
        self.rank_case_collisions();
        let total_uncompressed = self
            .entries
            .iter()
            .filter(|e| e.kind == ArchiveEntryKind::File)
            .map(|e| e.size)
            .sum();
        (
            ArchiveIndex {
                archive,
                format,
                entries: self.entries,
                truncated: self.truncated,
                total_uncompressed,
                compressed_size,
            },
            self.facts,
        )
    }

    /// Archives routinely store `a/b/c.txt` with no entry for `a` or `a/b`
    /// (`tar --no-recursion`, most zip writers). Without stand-ins the tree
    /// can't be walked to reach the file.
    fn synthesize_implied_dirs(&mut self) {
        let mut implied: Vec<String> = Vec::new();
        for entry in &self.entries {
            let mut ancestor = entry.parent();
            while !ancestor.is_empty() {
                if !self.positions.contains_key(ancestor) {
                    implied.push(ancestor.to_string());
                }
                ancestor = ancestor.rsplit_once('/').map_or("", |(p, _)| p);
            }
        }
        implied.sort_unstable();
        implied.dedup();
        self.facts.implied_dirs = implied.len();
        for inner in implied {
            self.positions.insert(inner.clone(), self.entries.len());
            self.entries.push(IndexEntry {
                inner,
                kind: ArchiveEntryKind::Dir,
                size: 0,
                mtime: None,
                mode: None,
                uid: None,
                gid: None,
                link_target: None,
                locator: Locator::Implied,
                case_rank: 0,
                readable: true,
            });
        }
    }

    /// Assign a nonzero `case_rank` to every entry after the first in a group of
    /// names that differ only by case. Runs after the sort, so ranks are stable.
    fn rank_case_collisions(&mut self) {
        let mut seen: HashMap<String, u32> = HashMap::new();
        for entry in &mut self.entries {
            let key = entry.inner.to_lowercase();
            let next = seen.entry(key).or_insert(0);
            entry.case_rank = *next;
            *next += 1;
        }
        self.facts.case_collisions = self.entries.iter().filter(|e| e.case_rank > 0).count();
    }
}

/// The immutable entry table for one mounted archive.
#[derive(Debug, Clone)]
pub struct ArchiveIndex {
    pub archive: PathBuf,
    pub format: ArchiveFormat,
    /// Sorted by [`IndexEntry::inner`], which the prefix lookups depend on.
    pub entries: Vec<IndexEntry>,
    /// True when the walk stopped at the entry cap; the listing says so.
    pub truncated: bool,
    /// Summed size of file members — what a full extraction would cost.
    pub total_uncompressed: u64,
    /// Size of the archive file itself.
    pub compressed_size: u64,
}

impl ArchiveIndex {
    /// An index with no members — an empty archive, and the base case for tests.
    pub const fn empty(archive: PathBuf, format: ArchiveFormat) -> Self {
        Self {
            archive,
            format,
            entries: Vec::new(),
            truncated: false,
            total_uncompressed: 0,
            compressed_size: 0,
        }
    }

    pub fn get(&self, inner: &str) -> Option<&IndexEntry> {
        let pos = self
            .entries
            .binary_search_by(|e| e.inner.as_str().cmp(inner))
            .ok()?;
        self.entries.get(pos)
    }

    pub fn is_dir(&self, inner: &str) -> bool {
        inner.is_empty()
            || self
                .get(inner)
                .is_some_and(|e| e.kind == ArchiveEntryKind::Dir)
    }

    /// The contiguous slice of entries at or below `inner_dir` (everything for
    /// the root). Sorted order puts a prefix's entries next to each other, so
    /// this is two binary searches rather than a scan.
    pub fn subtree(&self, inner_dir: &str) -> &[IndexEntry] {
        if inner_dir.is_empty() {
            return &self.entries;
        }
        let prefix = format!("{inner_dir}/");
        let start = self
            .entries
            .partition_point(|e| e.inner.as_str() < prefix.as_str());
        let end =
            start + self.entries[start..].partition_point(|e| e.inner.starts_with(prefix.as_str()));
        &self.entries[start..end]
    }

    /// The direct children of `inner_dir` — one component deeper, nothing below.
    pub fn children_of(&self, inner_dir: &str) -> impl Iterator<Item = &IndexEntry> {
        let depth = if inner_dir.is_empty() {
            0
        } else {
            inner_dir.matches('/').count() + 1
        };
        self.subtree(inner_dir)
            .iter()
            .filter(move |e| e.inner.matches('/').count() == depth)
    }

    /// Total bytes below `inner_dir` inclusive — what materializing that subtree
    /// would cost.
    pub fn subtree_bytes(&self, inner_dir: &str) -> u64 {
        let own = self
            .get(inner_dir)
            .filter(|e| e.kind == ArchiveEntryKind::File)
            .map_or(0, |e| e.size);
        own + self
            .subtree(inner_dir)
            .iter()
            .filter(|e| e.kind == ArchiveEntryKind::File)
            .map(|e| e.size)
            .sum::<u64>()
    }

    /// Absolute mount path for an inner path: the archive file's own path with
    /// the inner path appended. `/src/foo.zip` + `a/b.txt` →
    /// `/src/foo.zip/a/b.txt`. There is no sentinel because the archive is a
    /// *file*, so nothing real can occupy the paths below it.
    pub fn mount_path(&self, inner: &str) -> PathBuf {
        if inner.is_empty() {
            self.archive.clone()
        } else {
            self.archive.join(inner)
        }
    }

    /// Inverse of [`Self::mount_path`]: the inner path for an absolute path
    /// inside this mount (`Some("")` for the mount root itself).
    pub fn inner_of(&self, path: &Path) -> Option<String> {
        if path == self.archive {
            return Some(String::new());
        }
        let rel = path.strip_prefix(&self.archive).ok()?;
        let mut parts: Vec<String> = Vec::new();
        for part in rel.components() {
            match part {
                std::path::Component::Normal(p) => parts.push(p.to_string_lossy().into_owned()),
                // `.`/`..`/root inside a mount path is not something we produced.
                _ => return None,
            }
        }
        Some(parts.join("/"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(names: &[(&str, ArchiveEntryKind, u64)]) -> (ArchiveIndex, IndexFacts) {
        let mut b = IndexBuilder::new(1000);
        for (i, (name, kind, size)) in names.iter().enumerate() {
            let draft = Draft {
                kind: *kind,
                size: *size,
                ..Draft::file(*size, Locator::Zip { index: i })
            };
            assert!(b.push(name, draft));
        }
        b.finish(PathBuf::from("/src/a.zip"), ArchiveFormat::Zip, 100)
    }

    fn inners(index: &ArchiveIndex) -> Vec<&str> {
        index.entries.iter().map(|e| e.inner.as_str()).collect()
    }

    // --- normalization ---

    #[test]
    fn ordinary_names_pass_through() {
        assert_eq!(normalize("a/b/c.txt").unwrap().inner, "a/b/c.txt");
        assert_eq!(normalize("file.txt").unwrap().inner, "file.txt");
    }

    #[test]
    fn a_trailing_slash_and_dot_components_are_dropped() {
        assert_eq!(normalize("a/b/").unwrap().inner, "a/b");
        assert_eq!(normalize("./a//b").unwrap().inner, "a/b");
    }

    /// The whole point of normalizing here: an archive must not be able to name
    /// a path outside its own mount.
    #[test]
    fn traversal_is_refused_wherever_it_appears() {
        for raw in ["../evil", "a/../../evil", "a/b/..", ".."] {
            assert_eq!(normalize(raw), Err(Reject::Traversal), "{raw}");
        }
    }

    #[test]
    fn an_absolute_name_is_made_relative_and_flagged() {
        let n = normalize("/etc/passwd").unwrap();
        assert_eq!(n.inner, "etc/passwd");
        assert!(n.was_absolute);
    }

    #[test]
    fn backslashes_become_separators_and_are_flagged() {
        let n = normalize("dir\\sub\\file.txt").unwrap();
        assert_eq!(n.inner, "dir/sub/file.txt");
        assert!(n.had_backslash);
    }

    #[test]
    fn empty_and_nul_names_are_refused() {
        assert_eq!(normalize(""), Err(Reject::Empty));
        assert_eq!(normalize("/"), Err(Reject::Empty));
        assert_eq!(normalize("./"), Err(Reject::Empty));
        assert_eq!(normalize("a\0b"), Err(Reject::Nul));
    }

    // --- building ---

    #[test]
    fn unsafe_names_are_dropped_and_counted_not_mounted() {
        let mut b = IndexBuilder::new(10);
        assert!(b.push("../evil", Draft::file(1, Locator::Zip { index: 0 })));
        assert!(b.push("ok.txt", Draft::file(2, Locator::Zip { index: 1 })));
        let (index, facts) = b.finish(PathBuf::from("/a.zip"), ArchiveFormat::Zip, 10);
        assert_eq!(inners(&index), ["ok.txt"]);
        assert_eq!(facts.traversal_names, 1);
        assert_eq!(facts.skipped(), 1);
    }

    #[test]
    fn implied_directories_are_synthesized_so_the_tree_is_walkable() {
        let (index, facts) = build(&[("a/b/c.txt", ArchiveEntryKind::File, 5)]);
        assert_eq!(inners(&index), ["a", "a/b", "a/b/c.txt"]);
        assert_eq!(facts.implied_dirs, 2);
        assert_eq!(index.get("a").unwrap().locator, Locator::Implied);
    }

    #[test]
    fn an_explicit_directory_is_not_duplicated_by_an_implied_one() {
        let (index, facts) = build(&[
            ("a/", ArchiveEntryKind::Dir, 0),
            ("a/f.txt", ArchiveEntryKind::File, 3),
        ]);
        assert_eq!(inners(&index), ["a", "a/f.txt"]);
        assert_eq!(facts.implied_dirs, 0);
        assert_ne!(index.get("a").unwrap().locator, Locator::Implied);
    }

    /// Extractors let the last member win; the listing has to agree with what a
    /// materialize would leave on disk.
    #[test]
    fn a_duplicate_name_keeps_the_last_member() {
        let mut b = IndexBuilder::new(10);
        b.push("dup.txt", Draft::file(1, Locator::Zip { index: 0 }));
        b.push("dup.txt", Draft::file(999, Locator::Zip { index: 7 }));
        let (index, facts) = b.finish(PathBuf::from("/a.zip"), ArchiveFormat::Zip, 10);
        assert_eq!(index.entries.len(), 1);
        assert_eq!(index.get("dup.txt").unwrap().size, 999);
        assert_eq!(
            index.get("dup.txt").unwrap().locator,
            Locator::Zip { index: 7 }
        );
        assert_eq!(facts.duplicates, 1);
    }

    #[test]
    fn the_entry_cap_truncates_and_stops_the_walk() {
        let mut b = IndexBuilder::new(2);
        assert!(b.push("a", Draft::file(1, Locator::Staged)));
        assert!(b.push("b", Draft::file(1, Locator::Staged)));
        assert!(
            !b.push("c", Draft::file(1, Locator::Staged)),
            "push reports the cap so the caller stops walking"
        );
        let (index, _) = b.finish(PathBuf::from("/a.zip"), ArchiveFormat::Zip, 10);
        assert!(index.truncated);
        assert_eq!(index.entries.len(), 2);
    }

    /// Two names differing only by case would overwrite each other on a
    /// case-insensitive volume (the macOS default), so all but the first stage
    /// under their own prefix.
    #[test]
    fn case_collisions_get_isolated_staging_paths() {
        let (index, facts) = build(&[
            ("a/README", ArchiveEntryKind::File, 1),
            ("a/readme", ArchiveEntryKind::File, 2),
        ]);
        assert_eq!(facts.case_collisions, 1);
        let first = index.get("a/README").unwrap();
        let second = index.get("a/readme").unwrap();
        assert_eq!(first.case_rank, 0);
        assert_eq!(first.staging_rel(), PathBuf::from("a/README"));
        assert_eq!(second.case_rank, 1);
        assert_eq!(
            second.staging_rel(),
            PathBuf::from(".spyc-case-1/a/readme"),
            "the colliding entry stages somewhere it cannot clobber the first"
        );
    }

    #[test]
    fn distinct_names_never_collide() {
        let (_, facts) = build(&[
            ("a.txt", ArchiveEntryKind::File, 1),
            ("b.txt", ArchiveEntryKind::File, 1),
        ]);
        assert_eq!(facts.case_collisions, 0);
    }

    // --- lookups ---

    fn tree() -> ArchiveIndex {
        build(&[
            ("top.txt", ArchiveEntryKind::File, 10),
            ("a/one.txt", ArchiveEntryKind::File, 20),
            ("a/two.txt", ArchiveEntryKind::File, 30),
            ("a/deep/three.txt", ArchiveEntryKind::File, 40),
            ("a.txt", ArchiveEntryKind::File, 50),
        ])
        .0
    }

    #[test]
    fn children_of_the_root_are_the_top_level_only() {
        let index = tree();
        let names: Vec<&str> = index.children_of("").map(IndexEntry::name).collect();
        assert_eq!(names, ["a", "a.txt", "top.txt"]);
    }

    #[test]
    fn children_of_a_directory_stop_at_one_level() {
        let index = tree();
        let names: Vec<&str> = index.children_of("a").map(IndexEntry::name).collect();
        assert_eq!(names, ["deep", "one.txt", "two.txt"]);
    }

    /// `a.txt` sorts between `a` and `a/…` (`.` is 0x2E, `/` is 0x2F), so a
    /// prefix range that forgot the separator would swallow it.
    #[test]
    fn a_sibling_sharing_a_prefix_is_not_treated_as_a_child() {
        let index = tree();
        let subtree: Vec<&str> = index
            .subtree("a")
            .iter()
            .map(|e| e.inner.as_str())
            .collect();
        assert_eq!(
            subtree,
            ["a/deep", "a/deep/three.txt", "a/one.txt", "a/two.txt"]
        );
        assert!(!subtree.contains(&"a.txt"));
    }

    #[test]
    fn subtree_bytes_sums_the_whole_branch() {
        let index = tree();
        assert_eq!(index.subtree_bytes("a"), 20 + 30 + 40);
        assert_eq!(index.subtree_bytes("a/deep"), 40);
        assert_eq!(
            index.subtree_bytes("a.txt"),
            50,
            "a file is its own subtree"
        );
        assert_eq!(index.subtree_bytes(""), 10 + 20 + 30 + 40 + 50);
    }

    #[test]
    fn total_uncompressed_counts_files_only() {
        let index = tree();
        assert_eq!(index.total_uncompressed, 150);
    }

    #[test]
    fn is_dir_knows_the_root_and_implied_dirs() {
        let index = tree();
        assert!(index.is_dir(""));
        assert!(index.is_dir("a"));
        assert!(index.is_dir("a/deep"));
        assert!(!index.is_dir("a/one.txt"));
        assert!(!index.is_dir("nope"));
    }

    #[test]
    fn mount_paths_round_trip_through_inner_of() {
        let index = tree();
        assert_eq!(index.mount_path(""), PathBuf::from("/src/a.zip"));
        assert_eq!(
            index.mount_path("a/one.txt"),
            PathBuf::from("/src/a.zip/a/one.txt")
        );
        for inner in ["", "a", "a/one.txt", "a/deep/three.txt"] {
            let path = index.mount_path(inner);
            assert_eq!(index.inner_of(&path).as_deref(), Some(inner), "{inner}");
        }
    }

    #[test]
    fn inner_of_rejects_a_path_outside_the_mount() {
        let index = tree();
        assert_eq!(index.inner_of(Path::new("/src/other.zip/a")), None);
        assert_eq!(index.inner_of(Path::new("/src")), None);
    }

    proptest::proptest! {
        /// The security invariant, over arbitrary input: whatever a member is
        /// called, a name that survives normalization is relative, has no `..`
        /// or `.` components, and joins onto a root without leaving it. Member
        /// names in a downloaded archive are attacker-controlled, so this holds
        /// for *every* string, not just the ones we thought of.
        #[test]
        fn a_normalized_name_can_never_escape_its_mount(
            raw in proptest::string::string_regex(r"[a-zA-Z0-9./\\ ]{0,40}").unwrap(),
        ) {
            if let Ok(n) = normalize(&raw) {
                proptest::prop_assert!(!n.inner.is_empty());
                proptest::prop_assert!(!n.inner.starts_with('/'));
                proptest::prop_assert!(!n.inner.contains('\\'));
                for part in n.inner.split('/') {
                    proptest::prop_assert!(!part.is_empty() && part != "." && part != "..");
                }
                let root = std::path::Path::new("/staging");
                proptest::prop_assert!(root.join(&n.inner).starts_with(root));
            }
        }

        /// Normalization is idempotent — feeding a cleaned name back through it
        /// must not change it, or a re-index would drift from the first one.
        #[test]
        fn normalization_is_idempotent(
            raw in proptest::string::string_regex(r"[a-z./\\]{0,24}").unwrap(),
        ) {
            if let Ok(first) = normalize(&raw) {
                let second = normalize(&first.inner).expect("a clean name stays clean");
                proptest::prop_assert_eq!(first.inner, second.inner);
            }
        }
    }

    #[test]
    fn entry_name_and_parent_split_the_inner_path() {
        let index = tree();
        let e = index.get("a/deep/three.txt").unwrap();
        assert_eq!(e.name(), "three.txt");
        assert_eq!(e.parent(), "a/deep");
        let top = index.get("top.txt").unwrap();
        assert_eq!(top.name(), "top.txt");
        assert_eq!(top.parent(), "");
    }
}
