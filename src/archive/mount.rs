//! A live mount and the registry of them.
//!
//! A mount is an [`ArchiveIndex`] plus the user's pending changes — no OS
//! handles, so it lives in the Model. Mounts are addressed by the archive file's
//! own path: `/src/pkg.zip` is the root, `/src/pkg.zip/a/b.txt` is a member, and
//! since the archive is a *file*, nothing real can occupy the paths beneath it.

use std::path::{Path, PathBuf};

use crate::fs::listing::Listing;

use super::index::{ArchiveIndex, IndexEntry};
use super::journal::{Journal, StagedStats};
use super::scan::Capability;
use super::{ArchiveFormat, listing};

/// How many archives stay mounted at once.
///
/// Each holds its index in memory, so the cap is about not accumulating tens of
/// megabytes of entry tables across a long session — the staging bytes are
/// capped separately.
pub const MAX_MOUNTS: usize = 8;

/// One mounted archive.
#[derive(Debug)]
pub struct ArchiveMount {
    pub index: ArchiveIndex,
    /// Changes the user has made but not written back.
    pub journal: Journal,
    /// What spyc has put in the staging tree, and what it looked like then.
    pub staged: StagedStats,
    pub capability: Capability,
    /// Notes worth showing the user about how odd this archive is.
    pub warnings: Vec<String>,
    /// Directory holding this mount's extracted bytes.
    pub staging_root: PathBuf,
    /// Monotonic stamp of the last time a column entered this mount, so the
    /// least-recently-used one is the one evicted.
    pub last_used: u64,
}

impl ArchiveMount {
    pub fn archive(&self) -> &Path {
        &self.index.archive
    }

    pub const fn format(&self) -> ArchiveFormat {
        self.index.format
    }

    /// The archive's file name, plus the path within it — the status-bar label.
    pub fn label(&self, inner: &str) -> String {
        listing::mount_label(&self.index, inner)
    }

    /// The rows for a directory inside this mount.
    pub fn listing_for(&self, inner: &str) -> Listing {
        listing::listing_for(&self.index, &self.journal, &self.staged, inner)
    }

    /// The member behind an absolute mount path, resolving through any pending
    /// rename — a row the user is looking at may be displayed somewhere other
    /// than where the archive stores it.
    pub fn entry_at(&self, path: &Path) -> Option<&IndexEntry> {
        let shown = self.index.inner_of(path)?;
        self.index.get(&self.journal.original_of(&shown))
    }

    /// Absolute staging path for a member's bytes.
    pub fn staging_path(&self, entry: &IndexEntry) -> PathBuf {
        listing::staging_path(&self.staging_root, entry)
    }

    /// Whether the member's bytes are already on disk.
    pub fn is_materialized(&self, entry: &IndexEntry) -> bool {
        self.staging_path(entry).exists()
    }

    pub const fn is_dirty(&self) -> bool {
        self.journal.is_dirty()
    }
}

/// The mounted archives, newest activity last.
#[derive(Debug, Default)]
pub struct Mounts {
    mounts: Vec<ArchiveMount>,
    tick: u64,
}

impl Mounts {
    pub const fn is_empty(&self) -> bool {
        self.mounts.is_empty()
    }

    pub const fn len(&self) -> usize {
        self.mounts.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &ArchiveMount> {
        self.mounts.iter()
    }

    /// The mount containing `path`, with the inner path within it.
    ///
    /// Prefers the **longest** matching archive path, so a mount nested inside
    /// another resolves to the inner one rather than whichever was registered
    /// first.
    pub fn resolve(&self, path: &Path) -> Option<(&ArchiveMount, String)> {
        self.mounts
            .iter()
            .filter_map(|m| m.index.inner_of(path).map(|inner| (m, inner)))
            .max_by_key(|(m, _)| m.archive().as_os_str().len())
    }

    /// Whether `path` is inside any mount.
    pub fn contains(&self, path: &Path) -> bool {
        self.resolve(path).is_some()
    }

    pub fn get(&self, archive: &Path) -> Option<&ArchiveMount> {
        self.mounts.iter().find(|m| m.archive() == archive)
    }

    pub fn get_mut(&mut self, archive: &Path) -> Option<&mut ArchiveMount> {
        self.mounts.iter_mut().find(|m| m.archive() == archive)
    }

    /// Mark a mount as just used, which is what keeps it off the eviction block.
    pub fn touch(&mut self, archive: &Path) {
        self.tick += 1;
        let tick = self.tick;
        if let Some(m) = self.get_mut(archive) {
            m.last_used = tick;
        }
    }

    /// Register a mount, evicting the least-recently-used ones past
    /// [`MAX_MOUNTS`]. Returns the staging roots of everything evicted, for the
    /// caller to clean up.
    ///
    /// `protected` names archives a column is currently inside, or that carry
    /// unwritten changes — evicting either would pull the ground out from under
    /// the user, so they're never candidates however old they are.
    pub fn insert(&mut self, mount: ArchiveMount, protected: &[PathBuf]) -> Vec<PathBuf> {
        self.tick += 1;
        let mut mount = mount;
        mount.last_used = self.tick;
        // Re-mounting an archive replaces its entry rather than duplicating it.
        if let Some(pos) = self
            .mounts
            .iter()
            .position(|m| m.archive() == mount.archive())
        {
            let old = std::mem::replace(&mut self.mounts[pos], mount);
            return if old.staging_root == self.mounts[pos].staging_root {
                Vec::new()
            } else {
                vec![old.staging_root]
            };
        }
        self.mounts.push(mount);

        let mut evicted = Vec::new();
        while self.mounts.len() > MAX_MOUNTS {
            let victim = self
                .mounts
                .iter()
                .enumerate()
                .filter(|(_, m)| !protected.iter().any(|p| p == m.archive()) && !m.is_dirty())
                .min_by_key(|(_, m)| m.last_used)
                .map(|(i, _)| i);
            match victim {
                Some(i) => evicted.push(self.mounts.remove(i).staging_root),
                // Everything is in use or dirty: hold them all rather than
                // discard state the user still needs.
                None => break,
            }
        }
        evicted
    }

    /// Drop a mount, returning its staging root to clean up.
    pub fn remove(&mut self, archive: &Path) -> Option<PathBuf> {
        let pos = self.mounts.iter().position(|m| m.archive() == archive)?;
        Some(self.mounts.remove(pos).staging_root)
    }

    /// Drop every mount, returning all staging roots — the quit path.
    pub fn drain_all(&mut self) -> Vec<PathBuf> {
        self.mounts.drain(..).map(|m| m.staging_root).collect()
    }

    pub fn dirty_count(&self) -> usize {
        self.mounts.iter().filter(|m| m.is_dirty()).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::index::{Draft, IndexBuilder, Locator};

    fn mount_at(archive: &str, members: &[&str]) -> ArchiveMount {
        let mut b = IndexBuilder::new(1000);
        for (i, name) in members.iter().enumerate() {
            b.push(name, Draft::file(1, Locator::Zip { index: i }));
        }
        let (index, _) = b.finish(PathBuf::from(archive), ArchiveFormat::Zip, 10);
        ArchiveMount {
            index,
            journal: Journal::default(),
            staged: StagedStats::new(),
            capability: Capability::ReadWrite,
            warnings: Vec::new(),
            staging_root: PathBuf::from(format!("/staging/{}", archive.replace('/', "_"))),
            last_used: 0,
        }
    }

    #[test]
    fn a_path_inside_a_mount_resolves_to_its_inner_path() {
        let mut mounts = Mounts::default();
        mounts.insert(mount_at("/src/pkg.zip", &["a/b.txt"]), &[]);

        let (m, inner) = mounts.resolve(Path::new("/src/pkg.zip/a/b.txt")).unwrap();
        assert_eq!(m.archive(), Path::new("/src/pkg.zip"));
        assert_eq!(inner, "a/b.txt");

        let (_, root) = mounts.resolve(Path::new("/src/pkg.zip")).unwrap();
        assert_eq!(root, "", "the archive's own path is the mount root");
    }

    #[test]
    fn a_path_outside_every_mount_resolves_to_nothing() {
        let mut mounts = Mounts::default();
        mounts.insert(mount_at("/src/pkg.zip", &["a.txt"]), &[]);
        assert!(mounts.resolve(Path::new("/src/other.zip/a")).is_none());
        assert!(mounts.resolve(Path::new("/src")).is_none());
        assert!(!mounts.contains(Path::new("/etc/passwd")));
    }

    /// An archive mounted inside another mount's staging tree must resolve to
    /// the inner one, or entering it would list the outer archive's members.
    #[test]
    fn the_longest_matching_mount_wins() {
        let mut mounts = Mounts::default();
        mounts.insert(mount_at("/a.zip", &["inner.zip"]), &[]);
        mounts.insert(mount_at("/a.zip/inner.zip", &["deep.txt"]), &[]);

        let (m, inner) = mounts
            .resolve(Path::new("/a.zip/inner.zip/deep.txt"))
            .unwrap();
        assert_eq!(m.archive(), Path::new("/a.zip/inner.zip"));
        assert_eq!(inner, "deep.txt");
    }

    #[test]
    fn re_mounting_replaces_rather_than_duplicates() {
        let mut mounts = Mounts::default();
        mounts.insert(mount_at("/a.zip", &["one.txt"]), &[]);
        mounts.insert(mount_at("/a.zip", &["one.txt", "two.txt"]), &[]);
        assert_eq!(mounts.len(), 1);
        assert_eq!(
            mounts.get(Path::new("/a.zip")).unwrap().index.entries.len(),
            2
        );
    }

    #[test]
    fn the_least_recently_used_mount_is_evicted_past_the_cap() {
        let mut mounts = Mounts::default();
        for i in 0..MAX_MOUNTS {
            mounts.insert(mount_at(&format!("/a{i}.zip"), &["f.txt"]), &[]);
        }
        // Re-enter the oldest so it is no longer the eviction candidate.
        mounts.touch(Path::new("/a0.zip"));

        let evicted = mounts.insert(mount_at("/new.zip", &["f.txt"]), &[]);
        assert_eq!(mounts.len(), MAX_MOUNTS);
        assert_eq!(evicted.len(), 1);
        assert!(
            mounts.get(Path::new("/a0.zip")).is_some(),
            "recently used, kept"
        );
        assert!(
            mounts.get(Path::new("/a1.zip")).is_none(),
            "oldest, evicted"
        );
        assert!(
            evicted[0].to_string_lossy().contains("a1.zip"),
            "the evicted staging root comes back for cleanup: {evicted:?}"
        );
    }

    /// Evicting the archive a column is standing in would strand it, and
    /// evicting a dirty one would silently discard the user's edits.
    #[test]
    fn a_mount_in_use_or_dirty_is_never_evicted() {
        let mut mounts = Mounts::default();
        let protected = vec![PathBuf::from("/a0.zip")];
        for i in 0..MAX_MOUNTS {
            mounts.insert(mount_at(&format!("/a{i}.zip"), &["f.txt"]), &protected);
        }
        mounts
            .get_mut(Path::new("/a1.zip"))
            .unwrap()
            .journal
            .delete("f.txt");

        mounts.insert(mount_at("/new.zip", &["f.txt"]), &protected);
        assert!(mounts.get(Path::new("/a0.zip")).is_some(), "in use");
        assert!(mounts.get(Path::new("/a1.zip")).is_some(), "dirty");
        assert!(
            mounts.get(Path::new("/a2.zip")).is_none(),
            "the next oldest goes"
        );
        assert_eq!(mounts.dirty_count(), 1);
    }

    /// With every mount pinned there is nothing safe to evict, so the cap gives
    /// way rather than dropping state the user still needs.
    #[test]
    fn the_cap_yields_when_nothing_can_be_evicted() {
        let mut mounts = Mounts::default();
        let protected: Vec<PathBuf> = (0..=MAX_MOUNTS)
            .map(|i| PathBuf::from(format!("/a{i}.zip")))
            .collect();
        for i in 0..=MAX_MOUNTS {
            mounts.insert(mount_at(&format!("/a{i}.zip"), &["f.txt"]), &protected);
        }
        assert_eq!(mounts.len(), MAX_MOUNTS + 1);
    }

    #[test]
    fn removing_and_draining_hand_back_the_staging_roots() {
        let mut mounts = Mounts::default();
        mounts.insert(mount_at("/a.zip", &["f.txt"]), &[]);
        mounts.insert(mount_at("/b.zip", &["f.txt"]), &[]);

        let root = mounts.remove(Path::new("/a.zip")).unwrap();
        assert!(root.to_string_lossy().contains("a.zip"));
        assert_eq!(mounts.len(), 1);

        assert_eq!(mounts.drain_all().len(), 1);
        assert!(mounts.is_empty());
    }

    #[test]
    fn an_entry_is_found_through_a_pending_rename() {
        let mut mounts = Mounts::default();
        mounts.insert(mount_at("/a.zip", &["src/main.rs"]), &[]);
        let m = mounts.get_mut(Path::new("/a.zip")).unwrap();
        m.journal.rename("src", "source");

        let entry = m
            .entry_at(Path::new("/a.zip/source/main.rs"))
            .expect("the displayed path resolves back to the stored member");
        assert_eq!(entry.inner, "src/main.rs");
    }
}
