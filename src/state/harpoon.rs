//! Harpoon — a small, hand-curated, project-scoped list of file
//! pointers for muscle-memory navigation.
//!
//! The model is borrowed directly from ThePrimeagen's neovim plugin:
//! you "harpoon" a file (`Ha`), then jump back to it instantly via
//! its slot number (`H1`..`H9`). The whole point is single-keystroke
//! recall of the 3-5 files you're cycling between *right now* — not
//! a bookmarks system, not a fuzzy finder, not an MRU.
//!
//! Spyc-specific tweaks vs. the original:
//!   - **Files OR directories** — harpoon was files-only, but spyc is
//!     a file manager. Harpooning a directory is a useful "the place
//!     I'm working in" primitive; jumping into it chdirs there.
//!   - **9 slots** (vs. harpoon's typical 4) since `H<digit>` only
//!     costs one chord regardless of count.
//!   - **Cursor-land semantics**: jumping to a file places the cursor
//!     on it in its parent dir, leaving the verb (`Enter` / `V` /
//!     `^a s`) to the user. Spyc is "navigate first, decide second."
//!
//! Persistence: `$XDG_STATE_HOME/spyc/harpoon/<basename>.<hash>.toml`,
//! one file per project keyed by `PROJECT_HOME`. Auto-saved on every
//! mutation. The hash component is a 64-bit `DefaultHasher` digest of
//! the absolute project path, hex-encoded; the `<basename>` prefix is
//! a human-readable disambiguator. If `DefaultHasher` ever changes
//! across Rust versions, users will see a fresh empty list rather
//! than a corrupt one — the old file becomes an orphan.
//!
//! Cleared/swapped at chdir time when `PROJECT_HOME` changes; missing
//! files in the list are *not* auto-pruned (the user may have just
//! reverted a deletion). `Hjump` flashes "gone" if the path no longer
//! resolves and bails.

use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Hard cap on slots — matches `H1`..`H9` chord coverage.
pub const MAX_SLOTS: usize = 9;

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Harpoon {
    /// Ordered absolute paths. Index 0 is slot 1 (`H1`), index 8 is
    /// slot 9 (`H9`). Empty slots are simply absent (no `None` holes).
    #[serde(default)]
    pub slots: Vec<PathBuf>,
    /// `PROJECT_HOME` this list is scoped to. Stored for sanity-check
    /// on load — if it disagrees with the active project, treat the
    /// list as empty (defensive; shouldn't happen given filename
    /// keying).
    #[serde(default)]
    pub project: PathBuf,

    /// Cached set of slot paths plus all their ancestor directories,
    /// used by `=h` to filter the listing. Recomputed on every
    /// mutation. Not persisted.
    #[serde(skip)]
    ancestor_cache: HashSet<PathBuf>,
}

impl Harpoon {
    /// Slot index `n` (1-based, `H1`..`H9`). Returns `None` if empty
    /// or out of range.
    pub fn get(&self, n: u8) -> Option<&Path> {
        if n == 0 || n as usize > MAX_SLOTS {
            return None;
        }
        self.slots.get((n - 1) as usize).map(PathBuf::as_path)
    }

    pub const fn is_full(&self) -> bool {
        self.slots.len() >= MAX_SLOTS
    }

    pub fn contains(&self, path: &Path) -> bool {
        self.slots.iter().any(|p| p == path)
    }

    /// Append a path to the list. Idempotent (already-harpooned paths
    /// are no-ops). Returns `Added(slot)` with the 1-based slot number
    /// on success, `AlreadyPresent` if the path was already harpooned,
    /// or `Full` if all slots are taken.
    pub fn append(&mut self, path: PathBuf) -> AppendResult {
        if self.contains(&path) {
            return AppendResult::AlreadyPresent;
        }
        if self.is_full() {
            return AppendResult::Full;
        }
        self.slots.push(path);
        self.rebuild_ancestors();
        AppendResult::Added(self.slots.len() as u8)
    }

    /// Remove the first slot matching `path`. Returns the slot number
    /// that was removed (1-based), or `None` if not present.
    pub fn remove(&mut self, path: &Path) -> Option<u8> {
        let idx = self.slots.iter().position(|p| p == path)?;
        self.slots.remove(idx);
        self.rebuild_ancestors();
        Some((idx + 1) as u8)
    }

    /// Remove the slot at 0-based index. Used by the menu's `dd`
    /// binding. Returns true if a slot was removed.
    pub fn remove_at(&mut self, idx: usize) -> bool {
        if idx >= self.slots.len() {
            return false;
        }
        self.slots.remove(idx);
        self.rebuild_ancestors();
        true
    }

    /// Swap two slots (0-based). Used by the menu's reorder bindings.
    /// No-op if either index is out of range.
    pub fn swap(&mut self, a: usize, b: usize) {
        if a < self.slots.len() && b < self.slots.len() && a != b {
            self.slots.swap(a, b);
            // Ancestors don't change on swap, but cache rebuild is
            // cheap and keeps semantics clean.
        }
    }

    /// Set of paths that should match `=h`: every slot path plus
    /// every ancestor directory of every slot path, all the way to
    /// the filesystem root. Used to render `foo/` when `src/foo/bar`
    /// is harpooned and you're viewing `src/`.
    pub const fn ancestor_set(&self) -> &HashSet<PathBuf> {
        &self.ancestor_cache
    }

    fn rebuild_ancestors(&mut self) {
        let mut set = HashSet::with_capacity(self.slots.len() * 4);
        for path in &self.slots {
            set.insert(path.clone());
            for ancestor in path.ancestors().skip(1) {
                set.insert(ancestor.to_path_buf());
            }
        }
        self.ancestor_cache = set;
    }

    // ---- persistence ------------------------------------------------

    /// On-disk path for the harpoon file scoped to `project`. Returns
    /// `None` on exotic systems with no `$HOME` or `$XDG_STATE_HOME`.
    pub fn disk_path(project: &Path) -> Option<PathBuf> {
        let dir = state_dir()?.join("harpoon");
        let basename = project
            .file_name()
            .map_or_else(|| "_root".to_string(), |n| sanitize(&n.to_string_lossy()));
        // Hash of the absolute project path so two projects with the
        // same basename don't collide (e.g. `~/work/spyc` vs `~/play/spyc`).
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        project.hash(&mut hasher);
        let hash = hasher.finish();
        Some(dir.join(format!("{basename}.{hash:016x}.toml")))
    }

    /// Best-effort load. Missing or malformed file → empty harpoon
    /// scoped to `project`. The on-disk `project` field is rechecked
    /// against the caller's `project` for sanity; if they disagree
    /// (shouldn't happen given filename keying) we discard the loaded
    /// list rather than apply someone else's slots.
    pub fn load(project: &Path) -> Self {
        let mut h = (|| -> Option<Self> {
            let path = Self::disk_path(project)?;
            let text = std::fs::read_to_string(&path).ok()?;
            let parsed: Self = toml::from_str(&text).ok()?;
            if parsed.project != project {
                return None;
            }
            Some(parsed)
        })()
        .unwrap_or_else(|| Self {
            project: project.to_path_buf(),
            ..Default::default()
        });
        h.project = project.to_path_buf(); // ensure set even on fallback
        h.rebuild_ancestors();
        h
    }

    /// Serialize and write. Creates the parent directory if needed.
    /// Errors are returned to the caller so the App can flash them.
    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = Self::disk_path(&self.project) else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string(self).unwrap_or_default();
        std::fs::write(&path, text)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppendResult {
    Added(u8), // slot number (1-based)
    AlreadyPresent,
    Full,
}

fn state_dir() -> Option<PathBuf> {
    crate::state::state_root()
}

/// Replace filesystem-unsafe characters in the basename component
/// of the harpoon filename. The hash suffix already disambiguates,
/// so this is purely cosmetic.
fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// Returns a tempdir that callers pass to `with_state_root` to
    /// isolate persistence to a per-test directory. The tempdir's
    /// path becomes `state_root()` for the closure body.
    fn fresh_root() -> tempfile::TempDir {
        tempdir().unwrap()
    }

    #[test]
    fn append_assigns_slots_in_order() {
        let mut h = Harpoon::default();
        assert_eq!(h.append(PathBuf::from("/a")), AppendResult::Added(1));
        assert_eq!(h.append(PathBuf::from("/b")), AppendResult::Added(2));
        assert_eq!(h.append(PathBuf::from("/c")), AppendResult::Added(3));
        assert_eq!(h.get(1), Some(Path::new("/a")));
        assert_eq!(h.get(2), Some(Path::new("/b")));
        assert_eq!(h.get(3), Some(Path::new("/c")));
        assert_eq!(h.get(4), None);
    }

    #[test]
    fn append_idempotent() {
        let mut h = Harpoon::default();
        h.append(PathBuf::from("/a"));
        assert_eq!(h.append(PathBuf::from("/a")), AppendResult::AlreadyPresent);
        assert_eq!(h.slots.len(), 1);
    }

    #[test]
    fn append_full_returns_full() {
        let mut h = Harpoon::default();
        for i in 0..MAX_SLOTS {
            h.append(PathBuf::from(format!("/f{i}")));
        }
        assert_eq!(h.append(PathBuf::from("/extra")), AppendResult::Full);
    }

    #[test]
    fn remove_returns_slot_number() {
        let mut h = Harpoon::default();
        h.append(PathBuf::from("/a"));
        h.append(PathBuf::from("/b"));
        h.append(PathBuf::from("/c"));
        assert_eq!(h.remove(Path::new("/b")), Some(2));
        assert_eq!(h.slots, vec![PathBuf::from("/a"), PathBuf::from("/c")]);
        assert_eq!(h.remove(Path::new("/missing")), None);
    }

    #[test]
    fn swap_reorders() {
        let mut h = Harpoon::default();
        h.append(PathBuf::from("/a"));
        h.append(PathBuf::from("/b"));
        h.swap(0, 1);
        assert_eq!(h.slots[0], PathBuf::from("/b"));
        assert_eq!(h.slots[1], PathBuf::from("/a"));
    }

    #[test]
    fn ancestor_set_includes_all_parents() {
        let mut h = Harpoon::default();
        h.append(PathBuf::from("/Users/x/src/foo/bar/hello.c"));
        let set = h.ancestor_set();
        assert!(set.contains(Path::new("/Users/x/src/foo/bar/hello.c")));
        assert!(set.contains(Path::new("/Users/x/src/foo/bar")));
        assert!(set.contains(Path::new("/Users/x/src/foo")));
        assert!(set.contains(Path::new("/Users/x/src")));
        assert!(set.contains(Path::new("/Users/x")));
        assert!(set.contains(Path::new("/Users")));
        assert!(set.contains(Path::new("/")));
    }

    #[test]
    fn ancestor_set_unions_across_slots() {
        let mut h = Harpoon::default();
        h.append(PathBuf::from("/a/b/c"));
        h.append(PathBuf::from("/a/x/y"));
        let set = h.ancestor_set();
        assert!(set.contains(Path::new("/a")));
        assert!(set.contains(Path::new("/a/b")));
        assert!(set.contains(Path::new("/a/b/c")));
        assert!(set.contains(Path::new("/a/x")));
        assert!(set.contains(Path::new("/a/x/y")));
    }

    #[test]
    fn roundtrip_persistence() {
        let root = fresh_root();
        crate::state::with_state_root(root.path(), || {
            let project = PathBuf::from("/tmp/myproj");
            let mut h = Harpoon::load(&project);
            h.append(PathBuf::from("/tmp/myproj/src/main.rs"));
            h.append(PathBuf::from("/tmp/myproj/Cargo.toml"));
            h.save().unwrap();

            let loaded = Harpoon::load(&project);
            assert_eq!(loaded.slots, h.slots);
            assert_eq!(loaded.project, project);
            // Ancestor cache rebuilt on load.
            assert!(loaded.ancestor_set().contains(Path::new("/tmp/myproj/src")));
        });
    }

    #[test]
    fn load_missing_file_yields_empty() {
        let root = fresh_root();
        crate::state::with_state_root(root.path(), || {
            let h = Harpoon::load(Path::new("/tmp/never-saved"));
            assert!(h.slots.is_empty());
            assert_eq!(h.project, Path::new("/tmp/never-saved"));
        });
    }

    #[test]
    fn project_path_collision_resistant() {
        let a = Harpoon::disk_path(Path::new("/work/spyc")).unwrap();
        let b = Harpoon::disk_path(Path::new("/play/spyc")).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn project_mismatch_yields_empty() {
        // Pre-write a file claiming a different project; load should
        // discard it rather than serve someone else's slots.
        let root = fresh_root();
        crate::state::with_state_root(root.path(), || {
            let real = PathBuf::from("/tmp/realproj");
            let mut wrong = Harpoon::load(&real);
            wrong.append(PathBuf::from("/tmp/realproj/foo"));
            // Save under real's path but mutate `project` field to lie.
            wrong.project = PathBuf::from("/tmp/imposter");
            let path = Harpoon::disk_path(&real).unwrap();
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, toml::to_string(&wrong).unwrap()).unwrap();

            let loaded = Harpoon::load(&real);
            assert!(loaded.slots.is_empty(), "imposter slots were applied");
            assert_eq!(loaded.project, real);
        });
    }
}
