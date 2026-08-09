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
    live: Vec<ArchiveMount>,
    /// Staging trees left behind by a mount dropped mid-session, still owed a
    /// removal at teardown.
    orphaned: Vec<PathBuf>,
    tick: u64,
}

impl Mounts {
    pub const fn is_empty(&self) -> bool {
        self.live.is_empty()
    }

    pub const fn len(&self) -> usize {
        self.live.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &ArchiveMount> {
        self.live.iter()
    }

    /// The mount containing `path`, with the inner path within it.
    ///
    /// Prefers the **longest** matching archive path, so a mount nested inside
    /// another resolves to the inner one rather than whichever was registered
    /// first.
    pub fn resolve(&self, path: &Path) -> Option<(&ArchiveMount, String)> {
        self.live
            .iter()
            .filter_map(|m| m.index.inner_of(path).map(|inner| (m, inner)))
            .max_by_key(|(m, _)| m.archive().as_os_str().len())
    }

    /// Whether `path` is at or inside any mount.
    ///
    /// True for the archive file itself, which is what makes it a valid
    /// *destination* — putting a file into `pkg.zip` means its root directory.
    /// Asking "is this a member?" is [`Self::holds_member`].
    pub fn contains(&self, path: &Path) -> bool {
        self.resolve(path).is_some()
    }

    /// The mount `path` is a **member** of, excluding the mount root.
    ///
    /// A mounted archive is still an ordinary file where it lives: it can be
    /// entered, yanked, copied, renamed and deleted like any other. Treating it
    /// as a member of itself asks the index for the entry named `""`, which is
    /// how re-entering a mounted archive once failed with "no such member".
    pub fn member_of(&self, path: &Path) -> Option<(&ArchiveMount, String)> {
        self.resolve(path).filter(|(_, inner)| !inner.is_empty())
    }

    /// Whether `path` names a member inside a mount, rather than a container.
    pub fn holds_member(&self, path: &Path) -> bool {
        self.member_of(path).is_some()
    }

    pub fn get(&self, archive: &Path) -> Option<&ArchiveMount> {
        self.live.iter().find(|m| m.archive() == archive)
    }

    pub fn get_mut(&mut self, archive: &Path) -> Option<&mut ArchiveMount> {
        self.live.iter_mut().find(|m| m.archive() == archive)
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
            .live
            .iter()
            .position(|m| m.archive() == mount.archive())
        {
            let old = std::mem::replace(&mut self.live[pos], mount);
            return if old.staging_root == self.live[pos].staging_root {
                Vec::new()
            } else {
                vec![old.staging_root]
            };
        }
        self.live.push(mount);

        let mut evicted = Vec::new();
        while self.live.len() > MAX_MOUNTS {
            let victim = self
                .live
                .iter()
                .enumerate()
                .filter(|(_, m)| !protected.iter().any(|p| p == m.archive()) && !m.is_dirty())
                .min_by_key(|(_, m)| m.last_used)
                .map(|(i, _)| i);
            match victim {
                Some(i) => evicted.push(self.live.remove(i).staging_root),
                // Everything is in use or dirty: hold them all rather than
                // discard state the user still needs.
                None => break,
            }
        }
        evicted
    }

    /// Drop a mount, returning its staging root to clean up.
    pub fn remove(&mut self, archive: &Path) -> Option<PathBuf> {
        let pos = self.live.iter().position(|m| m.archive() == archive)?;
        Some(self.live.remove(pos).staging_root)
    }

    /// Hand back a staging root whose mount is gone but which nobody has removed
    /// yet, so teardown still gets to it.
    ///
    /// The caller that drops a mount can't always emit the removal itself — the
    /// effect screen returns one effect, and it's the user's.
    pub fn defer_cleanup(&mut self, root: PathBuf) {
        self.orphaned.push(root);
    }

    /// Drop every mount, returning all staging roots — the quit path.
    pub fn drain_all(&mut self) -> Vec<PathBuf> {
        let mut roots: Vec<PathBuf> = self.live.drain(..).map(|m| m.staging_root).collect();
        roots.append(&mut self.orphaned);
        roots
    }

    pub fn dirty_count(&self) -> usize {
        self.live.iter().filter(|m| m.is_dirty()).count()
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

    /// A container is not a member of itself. Conflating the two asks the index
    /// for the entry named `""`, which is how re-entering a mounted archive once
    /// failed with "no such member" — and it's why a mounted `pkg.zip` can still
    /// be yanked, copied, renamed and deleted like the file it is.
    #[test]
    fn a_mount_root_is_not_one_of_its_own_members() {
        let mut mounts = Mounts::default();
        mounts.insert(mount_at("/src/pkg.zip", &["a/b.txt"]), &[]);

        let root = Path::new("/src/pkg.zip");
        assert!(mounts.contains(root), "the root is at a mount");
        assert!(!mounts.holds_member(root), "but it is not a member");
        assert!(mounts.member_of(root).is_none());

        let member = Path::new("/src/pkg.zip/a/b.txt");
        assert!(mounts.holds_member(member));
        assert_eq!(
            mounts.member_of(member).map(|(_, inner)| inner),
            Some("a/b.txt".to_string())
        );
    }

    /// A staging tree handed over when its mount was dropped mid-session is still
    /// owed a removal, so teardown has to hand it back.
    #[test]
    fn a_deferred_staging_tree_is_still_returned_at_teardown() {
        let mut mounts = Mounts::default();
        mounts.insert(mount_at("/src/pkg.zip", &["a.txt"]), &[]);
        let orphan = mounts.remove(Path::new("/src/pkg.zip")).unwrap();
        mounts.defer_cleanup(orphan.clone());

        assert_eq!(mounts.drain_all(), vec![orphan]);
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
