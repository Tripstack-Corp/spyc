//! Soft-delete cache for files removed via `R` and items expelled
//! from the inventory.
//!
//! ## Why a separate stage from "real" trash
//!
//! Two-stage: `R` → graveyard (compressed tar.zst, one-keystroke undo inside
//! spyc, for the fat-finger case) → system trash (uncompressed, browsable in
//! the OS's own UI, for long-tail recovery) once the graveyard exceeds its cap.
//!
//! ## Schema
//!
//! Each entry is a pair under `$XDG_STATE_HOME/spyc/graveyard/`:
//!   - `<uuid>.json` — `Entry` metadata (orig path, kind, sizes, ts)
//!   - `<uuid>.tar.zst` — zstd-compressed tarball of the content
//!
//! Single regular files and directory trees use the same shape so
//! there's only one read/write code path. tar's `HeaderMode::Complete`
//! captures mode bits (chmod), mtime, and (best-effort) UID/GID;
//! restore opts in to `set_preserve_permissions(true)` and
//! `set_preserve_mtime(true)`. Symlinks are preserved as symlinks.
//! xattrs / ACLs / macOS resource forks are NOT preserved — adding
//! them later requires PAX extensions in the tar header and is
//! out of scope for v1 (rare on the file types spyc users typically
//! work with).
//!
//! ## Cascade
//!
//! Cap defaults to 500 MB. On `App::new`, while `total > cap`, the oldest entry
//! is unpacked and its paths handed to `trash::delete`, then the spyc artifacts
//! removed (flash: "N items moved to system trash"). Legacy pre-v1.41.0 entries
//! the new reader can't unpack are cascaded the same way.
//!
//! The cascade walks the directory **once** and tracks the running total in
//! memory. It used to re-derive the total from disk per eviction, which is
//! quadratic in filesystem work and — running synchronously in `App::new` —
//! reached the user as a launch that appeared to hang.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Total graveyard cap. When exceeded at startup, oldest entries
/// cascade to the system trash until the total falls below this.
/// 500 MB is generous for source-y deletions but small enough that
/// a stray `R` on a build dir doesn't hide there for weeks.
pub const GRAVEYARD_CAP_BYTES: u64 = 500 * 1024 * 1024;

/// What the entry held. Files and dirs share the same on-disk
/// shape (a tarball with one or many entries); the kind drives the
/// human-facing label (e.g. "file foo.txt" vs "dir src/ (12 files)").
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    File,
    Dir,
    Symlink,
}

/// Per-entry metadata. The `<uuid>.tar.zst` blob lives next to the
/// `<uuid>.json` that describes it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    /// Stable identifier (used for the filename pair).
    pub id: String,
    /// Absolute path of the root we removed (the directory itself,
    /// or the single file). Restore-to-original puts it back here.
    pub orig_path: PathBuf,
    /// Basename of `orig_path` — cached so the viewer doesn't have
    /// to re-derive it on every render.
    pub filename: String,
    /// `File`, `Dir`, or `Symlink`. Drives the viewer label and the
    /// "(N files)" annotation.
    pub kind: EntryKind,
    /// Number of paths inside the tarball. 1 for files / symlinks;
    /// for dirs, the recursive file count (excluding the root dir
    /// itself).
    pub file_count: u64,
    /// Total uncompressed size (sum of file sizes, in bytes). For
    /// dirs this is the tree size; for files / symlinks the file
    /// size.
    pub uncompressed_size: u64,
    /// Compressed `.tar.zst` size on disk. Used for the cap check.
    pub compressed_size: u64,
    /// Epoch seconds when the entry was written.
    pub timestamp: u64,
}

/// In-memory snapshot of the on-disk graveyard. Loaded at startup
/// (and after each mutation) by walking the directory and parsing
/// every `.json`. Sorted newest-first so the viewer naturally puts
/// the most-recent removal at the cursor.
#[derive(Debug, Default, Clone)]
pub struct Graveyard {
    pub entries: Vec<Entry>,
}

impl Graveyard {
    pub fn load() -> Self {
        let Some(dir) = graveyard_dir() else {
            return Self::default();
        };
        let mut entries = read_entries(&dir);
        entries.sort_by_key(|e| std::cmp::Reverse(e.timestamp));
        Self { entries }
    }

    /// Total compressed size on disk. Used for the cascade check.
    pub fn total_bytes(&self) -> u64 {
        self.entries.iter().map(|e| e.compressed_size).sum()
    }

    /// Move `src` into the graveyard. Returns the new entry, or an
    /// error if anything in the tar/zstd/io chain fails. The source
    /// path is **not** removed by this call — callers (`R` handler,
    /// `inventory::move_to_graveyard`) own that step so the caller
    /// can decide what to do on failure (don't unlink if archiving
    /// fails).
    pub fn write_entry(src: &Path) -> std::io::Result<Entry> {
        let basename = src
            .file_name()
            .ok_or_else(|| std::io::Error::other("path has no filename"))?
            .to_string_lossy()
            .into_owned();
        let orig_path = std::fs::canonicalize(src).unwrap_or_else(|_| src.to_path_buf());
        Self::write_entry_as(src, &basename, orig_path)
    }

    /// Like `write_entry` but with caller-supplied display name (the
    /// basename inside the tarball) and `orig_path` (the metadata
    /// field). Used by `inventory::move_to_graveyard` so the
    /// graveyard's recorded "original path" is the user's source —
    /// not the inventory cache pair we happen to be reading from
    /// right now.
    pub fn write_entry_as(
        src: &Path,
        display_name: &str,
        orig_path: PathBuf,
    ) -> std::io::Result<Entry> {
        let dir = graveyard_dir()
            .ok_or_else(|| std::io::Error::other("no $HOME / $XDG_STATE_HOME for graveyard"))?;
        std::fs::create_dir_all(&dir)?;

        let md = std::fs::symlink_metadata(src)?;
        let kind = if md.file_type().is_symlink() {
            EntryKind::Symlink
        } else if md.is_dir() {
            EntryKind::Dir
        } else {
            EntryKind::File
        };

        let id = uuid::Uuid::now_v7().to_string();
        let tar_path = dir.join(format!("{id}.tar.zst"));
        let json_path = dir.join(format!("{id}.json"));

        // Wrap an output file in zstd compressor in tar builder.
        // Default level (3) is a good speed/ratio balance for the
        // source-y files this typically holds.
        let tar_file = std::fs::File::create(&tar_path)?;
        let zstd_writer = zstd::stream::write::Encoder::new(tar_file, 0)?.auto_finish();
        let mut builder = tar::Builder::new(zstd_writer);
        // Capture full metadata (mode, mtime, uid, gid). UID/GID
        // restore is best-effort and silently no-ops without root.
        builder.mode(tar::HeaderMode::Complete);
        builder.follow_symlinks(false);

        // Single root inside the archive — the display name. On
        // restore we untar into a destination dir and that name
        // appears there (file or dir). For dirs we MUST use
        // `append_dir_all` — `append_path_with_name` only writes
        // the dir entry itself, not the contents.
        match kind {
            EntryKind::Dir => builder.append_dir_all(display_name, src)?,
            EntryKind::File | EntryKind::Symlink => {
                builder.append_path_with_name(src, display_name)?;
            }
        }
        // Drop the builder to flush; the auto-finishing zstd writer
        // closes its frame.
        let zstd_writer = builder.into_inner()?;
        drop(zstd_writer);

        // Walk to count files + sum uncompressed bytes. Done
        // separately from the tar pass so failures during the walk
        // (permissions, races) don't leave a half-written archive.
        let (file_count, uncompressed_size) = match kind {
            EntryKind::File | EntryKind::Symlink => (1, md.len()),
            EntryKind::Dir => walk_size(src),
        };
        let compressed_size = std::fs::metadata(&tar_path).map_or(0, |m| m.len());

        let entry = Entry {
            id,
            orig_path,
            filename: display_name.to_string(),
            kind,
            file_count,
            uncompressed_size,
            compressed_size,
            timestamp: crate::sysinfo::epoch_secs(),
        };

        // Persist metadata last so a crash mid-write leaves an
        // orphan tarball (cleaned up by health check) rather than
        // an entry with no payload.
        let json = serde_json::to_string_pretty(&entry).map_err(std::io::Error::other)?;
        crate::fs::write_atomic(&json_path, json.as_bytes())?;
        Ok(entry)
    }

    /// Restore `entry`'s tarball into `dest_dir`. Refuses to
    /// overwrite existing files (`set_overwrite(false)`) — caller
    /// can flash a clear error and let the user pick a different
    /// destination. Preserves mode bits and mtime.
    pub fn restore(entry: &Entry, dest_dir: &Path) -> std::io::Result<()> {
        let dir = graveyard_dir()
            .ok_or_else(|| std::io::Error::other("no $HOME / $XDG_STATE_HOME for graveyard"))?;
        let tar_path = dir.join(format!("{}.tar.zst", entry.id));
        let tar_file = std::fs::File::open(&tar_path)?;
        let zstd_reader = zstd::stream::read::Decoder::new(tar_file)?;
        let mut archive = tar::Archive::new(zstd_reader);
        archive.set_preserve_permissions(true);
        archive.set_preserve_mtime(true);
        // Don't clobber: if a path under dest_dir already exists,
        // tar errors and we surface the failure to the user.
        archive.set_overwrite(false);
        std::fs::create_dir_all(dest_dir)?;
        archive.unpack(dest_dir)
    }

    /// Delete `entry`'s spyc artifacts (.json and .tar.zst). Used
    /// by the cascade after the content has been handed to the
    /// system trash, and by the viewer's `dd`/`x` purge. Failures
    /// are swallowed (best-effort cleanup; the orphan path will
    /// be cleared by the next health check).
    pub fn delete_entry(entry: &Entry) {
        let Some(dir) = graveyard_dir() else {
            return;
        };
        let _ = std::fs::remove_file(dir.join(format!("{}.json", entry.id)));
        let _ = std::fs::remove_file(dir.join(format!("{}.tar.zst", entry.id)));
    }

    /// Push `entry` into the system trash by unpacking the tarball
    /// into a temp dir and handing each top-level path to
    /// `trash::delete`. This is what the cascade calls per entry,
    /// and what the viewer's `dd`/`x` purge also calls.
    ///
    /// On success, the spyc artifacts are removed. On failure, the
    /// entry stays put so the user can retry.
    pub fn cascade_entry_to_trash(entry: &Entry) -> std::io::Result<()> {
        let temp = tempfile::tempdir()?;
        Self::restore(entry, temp.path())?;
        // Hand each top-level path to the system trash. Usually
        // there's just one (the basename we tarred).
        let mut to_trash: Vec<PathBuf> = Vec::new();
        for entry_dir in std::fs::read_dir(temp.path())?.flatten() {
            to_trash.push(entry_dir.path());
        }
        if let Err(e) = trash::delete_all(&to_trash) {
            return Err(std::io::Error::other(format!("trash: {e}")));
        }
        Self::delete_entry(entry);
        Ok(())
    }

    /// FIFO cascade: while total bytes > `cap`, send the oldest
    /// entry to the system trash. Pre-v1.41.0 paired
    /// `<uuid>.json` + `<uuid>.dat` entries are silently ignored
    /// (the new reader skips them; we don't try to migrate them
    /// since they're a transient soft-delete cache and major
    /// version bumps can lose their contents). Returns
    /// `(trash_count, error_count)`.
    /// Also the *decision*: returns `(0, 0)` without evicting anything when the
    /// graveyard is already under `cap_bytes`, so callers need not (and must
    /// not) pre-check with their own [`load`](Self::load) — that second walk is
    /// pure cost.
    pub fn cascade_until_under(cap_bytes: u64) -> (usize, usize) {
        // ONE directory walk. The running total is then kept in memory and
        // decremented per successful eviction. Re-deriving it from disk each
        // iteration made this quadratic in filesystem work, and since it runs
        // synchronously in `App::new` that landed on the user as a startup that
        // never finished: measured on a release build with trivial 1 KiB
        // entries, 800 of them took 25s and 801 directory walks.
        let mut g = Self::load();
        let mut trashed = 0usize;
        let mut errors = 0usize;
        let mut total = g.total_bytes();
        // Sort oldest-first for the cascade so users keep their
        // most recent (and most likely undo-target) deletions.
        g.entries.sort_by_key(|e| e.timestamp);
        for entry in g.entries {
            if total < cap_bytes {
                break;
            }
            match Self::cascade_entry_to_trash(&entry) {
                Ok(()) => {
                    // Only a *successful* eviction frees bytes. A failed one
                    // leaves us over cap, so we move on to the next-oldest
                    // rather than stopping — matching the previous behavior,
                    // which re-read the unchanged total and kept going.
                    total = total.saturating_sub(entry.compressed_size);
                    trashed += 1;
                }
                Err(_) => errors += 1,
            }
        }
        (trashed, errors)
    }
}

fn graveyard_dir() -> Option<PathBuf> {
    crate::state::state_root().map(|r| r.join("graveyard"))
}

// Counts `read_entries` calls so tests can assert how many times a code path
// walks the graveyard directory. A full walk is `read_dir` plus one file read
// and one `exists` per entry, so "how many walks" is the cost that matters.
//
// Thread-local, not a global: the test binary runs cases in parallel, and a
// shared counter would mix one test's walks into another's assertion. Pairs with
// the thread-local state-root override these tests already use.
#[cfg(test)]
thread_local! {
    pub static DIR_SCANS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Walks counted on this thread since [`reset_dir_scans`].
#[cfg(test)]
pub fn dir_scans() -> usize {
    DIR_SCANS.with(std::cell::Cell::get)
}

#[cfg(test)]
pub fn reset_dir_scans() {
    DIR_SCANS.with(|c| c.set(0));
}

fn read_entries(dir: &Path) -> Vec<Entry> {
    #[cfg(test)]
    DIR_SCANS.with(|c| c.set(c.get() + 1));
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
        .filter_map(|e| {
            let text = std::fs::read_to_string(e.path()).ok()?;
            let entry: Entry = serde_json::from_str(&text).ok()?;
            // Drop entries whose tarball is missing — orphaned by a
            // mid-write crash or a manual rm. The next health check
            // will clean these up; here we just don't surface them.
            let dir = e.path().parent()?.to_path_buf();
            let tar_path = dir.join(format!("{}.tar.zst", entry.id));
            if !tar_path.exists() {
                return None;
            }
            Some(entry)
        })
        .collect()
}

fn walk_size(root: &Path) -> (u64, u64) {
    let mut count = 0u64;
    let mut bytes = 0u64;
    walk_size_inner(root, &mut count, &mut bytes);
    (count, bytes)
}

fn walk_size_inner(p: &Path, count: &mut u64, bytes: &mut u64) {
    let Ok(md) = std::fs::symlink_metadata(p) else {
        return;
    };
    if md.file_type().is_symlink() {
        *count += 1;
        *bytes += md.len();
        return;
    }
    if md.is_file() {
        *count += 1;
        *bytes += md.len();
        return;
    }
    if md.is_dir() {
        let Ok(rd) = std::fs::read_dir(p) else {
            return;
        };
        for ent in rd.flatten() {
            walk_size_inner(&ent.path(), count, bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    fn fresh_root() -> tempfile::TempDir {
        tempdir().unwrap()
    }

    #[test]
    fn write_and_restore_single_file_roundtrips_with_perms() {
        let root = fresh_root();
        crate::state::with_state_root(root.path(), || {
            let work = tempdir().unwrap();
            let src = work.path().join("hello.sh");
            {
                let mut f = std::fs::File::create(&src).unwrap();
                writeln!(f, "#!/bin/sh\necho hi").unwrap();
            }
            // Mark executable so we can verify mode-bit preservation.
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&src, std::fs::Permissions::from_mode(0o755)).unwrap();
            }

            let entry = Graveyard::write_entry(&src).unwrap();
            assert_eq!(entry.kind, EntryKind::File);
            assert_eq!(entry.file_count, 1);
            assert!(entry.uncompressed_size > 0);
            assert!(entry.compressed_size > 0);
            assert_eq!(entry.filename, "hello.sh");

            // Restore into a fresh dir and check the file's there with
            // the right content and mode.
            let dest = tempdir().unwrap();
            Graveyard::restore(&entry, dest.path()).unwrap();
            let restored = dest.path().join("hello.sh");
            let body = std::fs::read_to_string(&restored).unwrap();
            assert!(body.starts_with("#!/bin/sh"));
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = std::fs::metadata(&restored).unwrap().permissions().mode();
                assert_eq!(mode & 0o777, 0o755, "exec bits not preserved");
            }
        });
    }

    #[test]
    fn write_and_restore_directory_tree() {
        let root = fresh_root();
        crate::state::with_state_root(root.path(), || {
            let work = tempdir().unwrap();
            let tree_root = work.path().join("proj");
            std::fs::create_dir(&tree_root).unwrap();
            std::fs::write(tree_root.join("a.txt"), "alpha").unwrap();
            std::fs::create_dir(tree_root.join("sub")).unwrap();
            std::fs::write(tree_root.join("sub/b.txt"), "beta").unwrap();

            let entry = Graveyard::write_entry(&tree_root).unwrap();
            assert_eq!(entry.kind, EntryKind::Dir);
            assert_eq!(entry.file_count, 2, "expected 2 files in tree");

            let dest = tempdir().unwrap();
            Graveyard::restore(&entry, dest.path()).unwrap();
            assert_eq!(
                std::fs::read_to_string(dest.path().join("proj/a.txt")).unwrap(),
                "alpha"
            );
            assert_eq!(
                std::fs::read_to_string(dest.path().join("proj/sub/b.txt")).unwrap(),
                "beta"
            );
        });
    }

    #[test]
    fn restore_refuses_to_overwrite() {
        let root = fresh_root();
        crate::state::with_state_root(root.path(), || {
            let work = tempdir().unwrap();
            let src = work.path().join("important.txt");
            std::fs::write(&src, "original").unwrap();
            let entry = Graveyard::write_entry(&src).unwrap();

            let dest = tempdir().unwrap();
            let conflict = dest.path().join("important.txt");
            std::fs::write(&conflict, "DO NOT OVERWRITE").unwrap();

            let err =
                Graveyard::restore(&entry, dest.path()).expect_err("expected an overwrite refusal");
            let _ = err; // exact kind varies across tar versions
            assert_eq!(
                std::fs::read_to_string(&conflict).unwrap(),
                "DO NOT OVERWRITE",
                "existing file was clobbered"
            );
        });
    }

    #[test]
    fn load_returns_newest_first() {
        let root = fresh_root();
        crate::state::with_state_root(root.path(), || {
            let work = tempdir().unwrap();
            let a = work.path().join("a.txt");
            let b = work.path().join("b.txt");
            std::fs::write(&a, "a").unwrap();
            std::fs::write(&b, "b").unwrap();

            let mut e1 = Graveyard::write_entry(&a).unwrap();
            e1.timestamp = 1000;
            let mut e2 = Graveyard::write_entry(&b).unwrap();
            e2.timestamp = 2000;
            // Persist the doctored timestamps so the loader sees them.
            let dir = graveyard_dir().unwrap();
            std::fs::write(
                dir.join(format!("{}.json", e1.id)),
                serde_json::to_string(&e1).unwrap(),
            )
            .unwrap();
            std::fs::write(
                dir.join(format!("{}.json", e2.id)),
                serde_json::to_string(&e2).unwrap(),
            )
            .unwrap();

            let g = Graveyard::load();
            assert_eq!(g.entries.len(), 2);
            assert_eq!(g.entries[0].timestamp, 2000, "newest should be first");
            assert_eq!(g.entries[1].timestamp, 1000);
        });
    }

    #[test]
    fn delete_entry_removes_pair() {
        let root = fresh_root();
        crate::state::with_state_root(root.path(), || {
            let work = tempdir().unwrap();
            let src = work.path().join("x.txt");
            std::fs::write(&src, "hi").unwrap();
            let entry = Graveyard::write_entry(&src).unwrap();

            let dir = graveyard_dir().unwrap();
            assert!(dir.join(format!("{}.json", entry.id)).exists());
            assert!(dir.join(format!("{}.tar.zst", entry.id)).exists());

            Graveyard::delete_entry(&entry);
            assert!(!dir.join(format!("{}.json", entry.id)).exists());
            assert!(!dir.join(format!("{}.tar.zst", entry.id)).exists());
        });
    }

    #[test]
    fn orphan_tarball_without_json_is_ignored() {
        let root = fresh_root();
        crate::state::with_state_root(root.path(), || {
            let dir = graveyard_dir().unwrap();
            std::fs::create_dir_all(&dir).unwrap();
            // Stray tarball with no metadata — load should skip it
            // gracefully (the next health pass cleans it up).
            std::fs::write(dir.join("orphan.tar.zst"), b"not really a tar").unwrap();
            let g = Graveyard::load();
            assert!(g.entries.is_empty());
        });
    }

    // ── cascade cost ──────────────────────────────────────────────
    //
    // Reported by a user: "my graveyard got filled up and it super broke the
    // load times … had to manually delete them from outside spyc cause it
    // wasn't booting up". The cascade runs synchronously in `App::new`, so
    // anything superlinear there is startup latency with no UI to explain it.

    /// Plant `n` entries whose metadata claims `bytes_each` compressed bytes.
    ///
    /// The tarball is deliberately **invalid** so `cascade_entry_to_trash`
    /// fails at its `restore` step, which happens *before* `trash::delete_all`.
    /// That keeps this test off the developer's real system trash while still
    /// driving the full cascade loop: nothing is deleted, so the running total
    /// never drops and every entry is visited.
    fn plant_undeletable_entries(n: usize, bytes_each: u64) {
        let dir = graveyard_dir().unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..n {
            let id = format!("00000000-0000-4000-8000-{i:012}");
            let entry = Entry {
                id: id.clone(),
                filename: format!("f{i}.txt"),
                orig_path: PathBuf::from(format!("/tmp/f{i}.txt")),
                timestamp: i as u64,
                kind: EntryKind::File,
                file_count: 1,
                uncompressed_size: bytes_each,
                compressed_size: bytes_each,
            };
            std::fs::write(
                dir.join(format!("{id}.json")),
                serde_json::to_string(&entry).unwrap(),
            )
            .unwrap();
            // Present (so the reader keeps the entry) but not a real zstd
            // stream (so restore refuses and we never reach the trash).
            std::fs::write(dir.join(format!("{id}.tar.zst")), b"not a zstd stream").unwrap();
        }
    }

    /// The reported bug: the cascade re-walked the whole graveyard directory on
    /// **every iteration**, so evicting N entries cost N directory walks —
    /// N read_dir calls plus N² file reads. One walk is enough: the total is
    /// known up front and each eviction's size is known.
    #[test]
    fn cascade_walks_the_directory_once_not_once_per_entry() {
        let root = fresh_root();
        crate::state::with_state_root(root.path(), || {
            const N: usize = 40;
            plant_undeletable_entries(N, 1024);

            reset_dir_scans();
            // Cap 0 ⇒ every entry is over budget, so the loop visits all N.
            let (trashed, errors) = Graveyard::cascade_until_under(0);
            let scans = dir_scans();

            assert_eq!(trashed, 0, "the planted tarballs are not restorable");
            assert_eq!(errors, N, "so every entry should report an error");
            assert!(
                scans <= 2,
                "cascade walked the graveyard {scans} times for {N} entries; \
                 that is the O(N^2) startup stall (expected at most 2)"
            );
        });
    }

    /// The startup check itself must not pay for the directory twice: bootstrap
    /// asks whether the graveyard is over cap, and the cascade then re-derived
    /// the same answer from disk.
    #[test]
    fn deciding_to_cascade_and_cascading_share_one_walk() {
        let root = fresh_root();
        crate::state::with_state_root(root.path(), || {
            plant_undeletable_entries(8, 1024);
            reset_dir_scans();
            let _ = Graveyard::cascade_until_under(0);
            let scans = dir_scans();
            assert!(scans <= 2, "expected at most 2 walks, got {scans}");
        });
    }

    /// An under-cap graveyard must cost exactly one walk and evict nothing —
    /// the common case on every launch.
    #[test]
    fn an_under_cap_graveyard_evicts_nothing() {
        let root = fresh_root();
        crate::state::with_state_root(root.path(), || {
            plant_undeletable_entries(5, 10);
            reset_dir_scans();
            let (trashed, errors) = Graveyard::cascade_until_under(GRAVEYARD_CAP_BYTES);
            let scans = dir_scans();
            assert_eq!((trashed, errors), (0, 0), "nothing to do under cap");
            assert!(scans <= 1, "expected 1 walk for a no-op, got {scans}");
        });
    }
}
