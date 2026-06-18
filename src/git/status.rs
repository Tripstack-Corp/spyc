//! Git status queries — gix only (no subprocess).
//!
//! ## Two-stage pipeline
//!
//! Status flows through two decoupled stages:
//!
//! 1. **Decode** — produce one [`StatusEntry`] per changed *repo-relative*
//!    path. [`repo_status`] walks the index/worktree/tree diffs in-process via
//!    gix. (A porcelain-text decoder survives as `#[cfg(test)]` scaffolding for
//!    the parity tests, which cross-check `repo_status` against `git`.)
//! 2. **Map to listing** — [`map_to_listing`] takes the decoded entries plus a
//!    dir-relative `prefix` and produces the basename-keyed
//!    [`GitFileStatus`](crate::ui::list_view::GitFileStatus) map the list view
//!    consumes (strip prefix, basename for in-dir files, aggregate deep files
//!    onto their parent dir).
//!
//! The live status path (the background git worker, `bootstrap.rs`) runs
//! [`repo_status`]; the result `Vec<StatusEntry>` is cached repo-wide and
//! re-filtered per listing dir via [`map_to_listing`] on each chdir.

use std::collections::HashMap;
use std::path::Path;
use std::time::SystemTime;

use crate::ui::list_view::{GitChange, GitFileStatus};

/// One changed repo-relative path, fully decoded into both porcelain halves.
/// Produced by the gix `repo_status` walk in production (and by the test-only
/// `decode_porcelain` for the parity tests), and is the input to
/// [`map_to_listing`], which turns it into the per-listing basename map.
///
/// `rela_path` is repo-root-relative with forward slashes. For a rename it is
/// the *destination* (new) path — matching git porcelain, which keys a rename
/// on its new name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusEntry {
    pub rela_path: String,
    pub staged: Option<GitChange>,
    pub unstaged: Option<GitChange>,
    pub untracked: bool,
}

/// Decode one porcelain XY half (X = index/staged, Y = working tree) into a
/// `GitChange`. ` ` (and `?`/`!`) yield None — those are handled by the caller
/// via the special-case markers. Test-only scaffolding (paired with
/// [`decode_porcelain`]); production status is gix.
#[cfg(test)]
const fn decode_half(c: char) -> Option<GitChange> {
    match c {
        'M' | 'T' => Some(GitChange::Modified),
        'A' => Some(GitChange::Added),
        'D' => Some(GitChange::Deleted),
        'R' | 'C' => Some(GitChange::Renamed),
        'U' => Some(GitChange::Conflicted),
        _ => None,
    }
}

/// Decode raw `git status --porcelain` text into one [`StatusEntry`] per
/// changed path (repo-relative). Production status runs entirely on gix
/// ([`repo_status`]); this porcelain decoder survives only as **test
/// scaffolding** — the parity tests cross-check `repo_status` against
/// `decode_porcelain(git status --porcelain)`. Hence `#[cfg(test)]`.
#[cfg(test)]
fn decode_porcelain(porcelain: &str) -> Vec<StatusEntry> {
    let mut entries = Vec::new();
    for line in porcelain.lines() {
        if line.len() < 4 {
            continue;
        }
        let xy = &line[..2];
        let path_str = &line[3..];
        // For renames ("R  old -> new"), take the new (destination) name.
        let raw_path = path_str.rsplit(" -> ").next().unwrap_or(path_str);

        let (staged, unstaged, untracked) = if xy == "??" {
            (None, None, true)
        } else if xy == "!!" {
            continue; // ignored
        } else if xy.contains('U') || xy == "DD" || xy == "AA" {
            (
                Some(GitChange::Conflicted),
                Some(GitChange::Conflicted),
                false,
            )
        } else {
            let mut chars = xy.chars();
            let x = chars.next().unwrap_or(' ');
            let y = chars.next().unwrap_or(' ');
            (decode_half(x), decode_half(y), false)
        };
        entries.push(StatusEntry {
            rela_path: raw_path.to_string(),
            staged,
            unstaged,
            untracked,
        });
    }
    entries
}

/// Stage 2: map decoded [`StatusEntry`]s to the basename-keyed
/// [`GitFileStatus`] map for one listing dir.
///
/// Lifted verbatim (behavior-identical) from the original
/// `parse_porcelain_statuses`: strip `prefix`, give in-this-dir files a
/// basename entry, and aggregate deep files onto their top-level parent dir
/// (untracked-only subtree → untracked dir; any tracked change →
/// unstaged-Modified dir; tracked outranks untracked and never downgrades).
///
/// Shared by both backends so the path-mapping rules live in exactly one place.
#[must_use]
pub fn map_to_listing(entries: &[StatusEntry], prefix: &str) -> HashMap<String, GitFileStatus> {
    let mut map: HashMap<String, GitFileStatus> = HashMap::new();
    // Loop-invariant: the trailing-slash-normalized prefix only depends on
    // `prefix`, so build it once instead of per entry.
    let pfx = if prefix.is_empty() || prefix.ends_with('/') {
        prefix.to_string()
    } else {
        format!("{prefix}/")
    };
    for entry in entries {
        let raw_path = entry.rela_path.as_str();
        // Strip the directory prefix to get a path relative to the current
        // listing dir (entries carry repo-relative paths).
        let filename = if prefix.is_empty() {
            raw_path
        } else {
            match raw_path.strip_prefix(&pfx) {
                Some(rest) => rest,
                None => continue, // not under this directory
            }
        };
        let name = Path::new(filename)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        // Top component relative to THIS directory.
        let top_component = filename.split('/').next().unwrap_or(filename).to_string();
        let in_this_dir = top_component == filename;
        let status = GitFileStatus {
            staged: entry.staged,
            unstaged: entry.unstaged,
            untracked: entry.untracked,
        };
        // Only file rows in THIS directory get a basename entry. Otherwise a
        // deep entry like `content-acquisition/AGENTS.md` would write
        // `AGENTS.md → Modified` and dirty the unrelated root-level
        // `AGENTS.md` row.
        if in_this_dir && !name.is_empty() {
            map.entry(name).or_insert(status);
        }
        // Mark the parent directory as dirty for entries in subtrees.
        // Directories don't have a meaningful per-half staging concept, so we
        // collapse to one of two shapes:
        //   - untracked (`?`) when the subtree's changes are *only* untracked
        //     content, and
        //   - unstaged-Modified (`~`) for any tracked change.
        // Tracked outranks untracked: a dir with both a modified file and a new
        // file reads as changed (`~`), not untracked. Because siblings arrive
        // in arbitrary order, an untracked flag set by an earlier sibling is
        // upgraded to `~` when a tracked sibling shows up; the reverse never
        // downgrades.
        if !in_this_dir && !top_component.is_empty() {
            let dir = map
                .entry(format!("{top_component}/"))
                .or_insert_with(GitFileStatus::clean);
            if status.untracked {
                if dir.is_clean() {
                    dir.untracked = true;
                }
            } else {
                // Overwrite wholesale: a tracked change supersedes any
                // untracked flag a prior sibling set, and dirs carry no
                // per-half staging detail.
                *dir = GitFileStatus::unstaged(GitChange::Modified);
            }
        }
    }
    map
}

/// The live status backend (run by the background git worker, bootstrap.rs):
/// produce the same `Vec<StatusEntry>` as [`decode_porcelain`] does for
/// `git status --porcelain -unormal`, but without shelling out — walking the
/// index/worktree/tree diffs via gix directly.
///
/// Returns `None` if `repo_root` can't be opened as a repository or the status
/// walk errors. The `#[cfg(test)]` parity tests below cross-check it against
/// `git status --porcelain -unormal`.
///
/// ## Platform config (matching `git status --porcelain -unormal`)
///
/// * `untracked_files(Collapsed)` — this maps to gix's
///   `EmissionMode::CollapseDirectory`, which is exactly what `-unormal` does:
///   a *fully*-untracked directory collapses to a single `dir/` entry, while
///   an untracked file inside an otherwise-tracked directory is listed
///   individually. `UntrackedFiles::Files` (the value PR 4's plan first
///   suggested) maps to `EmissionMode::Matching`, which lists *every* untracked
///   file — so a wholly-untracked `sub/` came back as `sub/a.txt`, `sub/b.txt`
///   instead of git's single `sub/` entry (see DIVERGENCE note in
///   `case10_fully_untracked_subdir`). `Collapsed` is the right match and is
///   in fact gix's default.
/// * `tree_index_track_renames(Given(default))` and
///   `index_worktree_rewrites(default)` — enable rename detection at 50%
///   similarity (`gix_diff::Rewrites::default().percentage == 0.5`), matching
///   git's `status` default of `-M50%`. Without this, a staged rename shows as
///   a delete + add pair instead of a single `R`.
/// * default `head_tree` (HEAD) — compared against the index for the staged
///   column.
#[must_use]
pub fn repo_status(repo_root: &Path) -> Option<Vec<StatusEntry>> {
    use gix::bstr::ByteSlice;
    use gix::diff::index::ChangeRef;
    use gix::status::index_worktree::Item as IwItem;
    use gix::status::plumbing::index_as_worktree::{Change, EntryStatus};
    use gix::status::tree_index::TrackRenames;
    use gix::status::{Item, UntrackedFiles};

    let repo = gix::open(repo_root).ok()?;
    let platform = repo
        .status(gix::progress::Discard)
        .ok()?
        // `-unormal`: collapse a fully-untracked dir to one `dir/` entry, but
        // list an untracked file inside a tracked dir individually. See the
        // doc comment above for why this is `Collapsed`, not `Files`.
        .untracked_files(UntrackedFiles::Collapsed)
        // Match git's `status` default rename detection (-M50%).
        .tree_index_track_renames(TrackRenames::Given(gix::diff::Rewrites::default()))
        .index_worktree_rewrites(gix::diff::Rewrites::default());

    // Accumulate per repo-relative path: the staged column (TreeIndex) and the
    // unstaged/untracked column (IndexWorktree) for the same path merge into
    // one StatusEntry, mirroring how porcelain packs both into one `XY` line.
    let mut by_path: HashMap<String, StatusEntry> = HashMap::new();
    // Fetch-or-create the entry for `path`. A free fn (not a closure) because a
    // closure returning a `&mut` into a captured map can't satisfy `FnMut`.
    fn entry(by_path: &mut HashMap<String, StatusEntry>, path: String) -> &mut StatusEntry {
        by_path.entry(path.clone()).or_insert(StatusEntry {
            rela_path: path,
            staged: None,
            unstaged: None,
            untracked: false,
        })
    }

    let iter = platform.into_iter(None).ok()?;
    for item in iter {
        let item = item.ok()?;
        match item {
            // STAGED column: HEAD-tree vs index.
            Item::TreeIndex(change) => match change {
                ChangeRef::Addition { location, .. } => {
                    entry(&mut by_path, location.to_str_lossy().into_owned()).staged =
                        Some(GitChange::Added);
                }
                ChangeRef::Deletion { location, .. } => {
                    entry(&mut by_path, location.to_str_lossy().into_owned()).staged =
                        Some(GitChange::Deleted);
                }
                ChangeRef::Modification { location, .. } => {
                    entry(&mut by_path, location.to_str_lossy().into_owned()).staged =
                        Some(GitChange::Modified);
                }
                ChangeRef::Rewrite { location, .. } => {
                    // `location` is the destination (new) path; porcelain keys
                    // a rename on its new name. Emit nothing for source.
                    entry(&mut by_path, location.to_str_lossy().into_owned()).staged =
                        Some(GitChange::Renamed);
                }
            },
            // UNSTAGED + UNTRACKED column: index vs worktree.
            Item::IndexWorktree(iw) => match iw {
                IwItem::Modification {
                    rela_path, status, ..
                } => {
                    let path = rela_path.to_str_lossy().into_owned();
                    match status {
                        EntryStatus::Conflict { .. } => {
                            let e = entry(&mut by_path, path);
                            e.staged = Some(GitChange::Conflicted);
                            e.unstaged = Some(GitChange::Conflicted);
                        }
                        EntryStatus::Change(c) => match c {
                            Change::Removed => {
                                entry(&mut by_path, path).unstaged = Some(GitChange::Deleted);
                            }
                            Change::Type { .. }
                            | Change::Modification { .. }
                            | Change::SubmoduleModification(_) => {
                                entry(&mut by_path, path).unstaged = Some(GitChange::Modified);
                            }
                        },
                        // No observable change (`NeedsUpdate` is a stat-refresh
                        // hint), or an intent-to-add placeholder we don't model.
                        EntryStatus::NeedsUpdate(_) | EntryStatus::IntentToAdd => {}
                    }
                }
                IwItem::DirectoryContents {
                    entry: dir_entry, ..
                } => {
                    if dir_entry.status == gix::dir::entry::Status::Untracked {
                        // git porcelain reports a *collapsed* fully-untracked
                        // directory with a trailing slash (`?? sub/`). gix's
                        // `rela_path` carries no slash, so re-add it for a
                        // directory entry to match porcelain byte-for-byte (and
                        // so `map_to_listing` routes it through the parent-dir
                        // aggregation, not as an in-dir basename).
                        let mut path = dir_entry.rela_path.to_str_lossy().into_owned();
                        if dir_entry.disk_kind == Some(gix::dir::entry::Kind::Directory) {
                            path.push('/');
                        }
                        entry(&mut by_path, path).untracked = true;
                    }
                    // Ignored / Pruned / Tracked → skip.
                }
                IwItem::Rewrite { dirwalk_entry, .. } => {
                    entry(
                        &mut by_path,
                        dirwalk_entry.rela_path.to_str_lossy().into_owned(),
                    )
                    .unstaged = Some(GitChange::Renamed);
                }
            },
        }
    }

    Some(by_path.into_values().collect())
}

/// [`repo_status`] guarded against the racy-snapshot pitfall, returning the
/// entries alongside the `.git/index` + `HEAD` mtimes they're consistent with
/// (the `GitWorkerResult` shape the off-thread git worker sends).
///
/// The status walk takes hundreds of ms on a large tree. If `.git/index` or
/// `HEAD` is rewritten *during* it (a `commit` / `checkout` / `git add` landing
/// concurrently — e.g. a rapid `git add … && git commit …`), the entries can be
/// paired with a mtime that coincides with the now-current index. That single
/// combination — stale entries stamped with the current mtime — is the one the
/// mtime-keyed status caches (`AppState::refresh_git_state`'s short-circuit and
/// `git_file_statuses_cached`'s reuse check) can never recover from: they treat
/// it as fresh forever and never re-walk, so committed files keep showing as
/// modified until an unrelated write moves the mtime.
///
/// Guard it: stat the mtimes before AND after the walk, and only trust the
/// result when they're unchanged across it — then the entries provably belong
/// to the stamped mtime. If they moved, the walk raced a write; retry for a
/// stable window. On persistent churn fall back to the last walk stamped with
/// its *before* mtimes: `before` is older than the now-moved on-disk mtime, so
/// the 1 Hz poll still diffs and re-walks (the safe "older key, newer status"
/// direction). We never stamp a stale snapshot as current.
pub fn repo_status_stable(
    repo_root: &Path,
) -> (
    Option<Vec<StatusEntry>>,
    Option<SystemTime>,
    Option<SystemTime>,
) {
    let gitdir = crate::git::discovery::gitdir(repo_root);
    let stat = || {
        gitdir.as_ref().map_or((None, None), |gd| {
            let index = std::fs::metadata(gd.join("index"))
                .and_then(|m| m.modified())
                .ok();
            let head = std::fs::metadata(gd.join("HEAD"))
                .and_then(|m| m.modified())
                .ok();
            (index, head)
        })
    };
    let (entries, (index_mtime, head_mtime)) = stable_walk(stat, || repo_status(repo_root), 3);
    (entries, index_mtime, head_mtime)
}

/// Generic stat-walk-stat consistency guard (see [`repo_status_stable`]).
/// Extracted from the IO so the race handling is unit-testable with injected
/// `stat`/`walk` closures. Returns the walk result paired with the `stat` key
/// it is consistent with: the `before` key of a walk the key didn't move
/// across, or — on persistent churn after `max_tries` — the last walk stamped
/// with its (older) `before` key, which still forces a re-walk on the next poll.
fn stable_walk<K, E>(
    mut stat: impl FnMut() -> K,
    mut walk: impl FnMut() -> E,
    max_tries: u32,
) -> (E, K)
where
    K: PartialEq,
{
    let mut fallback: Option<(E, K)> = None;
    for _ in 0..max_tries.max(1) {
        let before = stat();
        let result = walk();
        let after = stat();
        if before == after {
            // Key stable across the walk → `result` corresponds to it.
            return (result, before);
        }
        // A write raced the walk; the pair may be inconsistent. Keep it stamped
        // with the older `before` key as a fallback (the on-disk key has since
        // moved, so a re-walk is still forced) and retry for a stable window.
        fallback = Some((result, before));
    }
    fallback.expect("loop runs at least once and sets fallback whenever it doesn't return early")
}

#[cfg(test)]
mod stable_walk_tests {
    use super::stable_walk;
    use std::cell::Cell;

    /// Key stable across the walk → the first walk is trusted (one walk only).
    #[test]
    fn trusts_first_walk_when_key_stable() {
        let walks = Cell::new(0u32);
        let (res, key) = stable_walk(
            || 7u32,
            || {
                walks.set(walks.get() + 1);
                "entries"
            },
            3,
        );
        assert_eq!(res, "entries");
        assert_eq!(key, 7);
        assert_eq!(walks.get(), 1, "stable on the first try → exactly one walk");
    }

    /// Key moves across the first walk, then settles → the racy first result is
    /// discarded and the second (stable) walk is returned, stamped with its key.
    #[test]
    fn retries_until_key_is_stable() {
        let n = Cell::new(0u32);
        // stat sequence: before1=1, after1=2 (differ) → retry; before2=9,
        // after2=9 (same) → trust the second walk.
        let key = || {
            let i = n.get();
            n.set(i + 1);
            match i {
                0 => 1,
                1 => 2,
                _ => 9,
            }
        };
        let walks = Cell::new(0u32);
        let (_res, k) = stable_walk(
            key,
            || {
                walks.set(walks.get() + 1);
                walks.get()
            },
            3,
        );
        assert_eq!(k, 9, "stamped with the stable key");
        assert_eq!(walks.get(), 2, "one retry after the racy first walk");
    }

    /// Persistent churn (key changes across every walk) terminates after
    /// `max_tries` and falls back to the last walk's *before* key — never the
    /// post-walk key, so a stale snapshot can't be stamped as current.
    #[test]
    fn persistent_churn_falls_back_to_before_key() {
        let n = Cell::new(0u32);
        let key = || {
            let i = n.get();
            n.set(i + 1);
            i // strictly increasing → before != after on every iteration
        };
        let (res, k) = stable_walk(key, || "e", 3);
        assert_eq!(res, "e");
        // 3 iterations consume keys 0..6; befores are 0, 2, 4 → last before = 4.
        assert_eq!(
            k, 4,
            "fallback stamps the last walk's BEFORE key, not its after"
        );
    }
}

#[cfg(test)]
mod map_tests {
    //! Path-mapping rules: `decode_porcelain` → `map_to_listing`. Relocated
    //! from `sysinfo::tests` when the porcelain parser was split into the
    //! shared decode + map stages (gix flip, PR 5). These pin the
    //! prefix/basename/parent-dir-aggregation behavior that both backends
    //! share.
    use super::{decode_porcelain, map_to_listing};
    use crate::ui::list_view::{GitChange, GitFileStatus};
    use std::collections::HashMap;

    /// The production decode→map composition the old `parse_porcelain_statuses`
    /// performed, so the test bodies read unchanged.
    fn parse(porcelain: &str, prefix: &str) -> HashMap<String, GitFileStatus> {
        map_to_listing(&decode_porcelain(porcelain), prefix)
    }

    #[test]
    fn deep_modification_does_not_dirty_same_basename_at_root() {
        // Regression: a root listing of `git status` showing
        // `content-acquisition/AGENTS.md` modified must NOT mark a
        // separate root-level `AGENTS.md` as modified.
        let porcelain = " M content-acquisition/AGENTS.md\n";
        let map = parse(porcelain, "");
        // The deep file's basename is NOT a root entry.
        assert!(!map.contains_key("AGENTS.md"));
        // The parent dir IS marked dirty (unstaged-Modified).
        let dir_status = map.get("content-acquisition/").unwrap();
        assert_eq!(dir_status.unstaged, Some(GitChange::Modified));
        assert!(dir_status.staged.is_none());
        assert!(!dir_status.untracked);
    }

    #[test]
    fn root_modification_marks_basename() {
        // ` M` = unstaged-only modify.
        let map = parse(" M AGENTS.md\n", "");
        let s = map.get("AGENTS.md").unwrap();
        assert_eq!(s.unstaged, Some(GitChange::Modified));
        assert!(s.staged.is_none());
        assert!(!s.untracked);
    }

    #[test]
    fn root_and_deep_same_basename_uses_root_status() {
        // Both a root file and a sibling-named deep file are dirty.
        // The root entry must reflect the root file's actual status,
        // not the deep file's.
        let porcelain = "?? new.md\n M sub/new.md\n";
        let map = parse(porcelain, "");
        let new_md = map.get("new.md").unwrap();
        assert!(new_md.untracked);
        assert!(new_md.staged.is_none() && new_md.unstaged.is_none());
        assert_eq!(map.get("sub/").unwrap().unstaged, Some(GitChange::Modified));
    }

    #[test]
    fn prefix_strips_listing_dir() {
        // Listing `sub/` under a repo root: only entries under `sub/`
        // contribute, and they're keyed relative to the listing dir.
        let porcelain = " M sub/foo.txt\n M other/bar.txt\n";
        let map = parse(porcelain, "sub");
        assert_eq!(
            map.get("foo.txt").unwrap().unstaged,
            Some(GitChange::Modified)
        );
        assert!(!map.contains_key("bar.txt"));
    }

    #[test]
    fn untracked_surfaces_in_subdirectory_listing() {
        // Viewing `docs/` with untracked files in it must surface them
        // as basename-keyed untracked entries.
        let porcelain = "?? docs/PATH_HANDOFF_PLAN.md\n?? docs/TEST_IMPROVEMENT_PLAN.md\n";
        let map = parse(porcelain, "docs");
        let a = map.get("PATH_HANDOFF_PLAN.md").unwrap();
        assert!(a.untracked);
        assert!(a.staged.is_none() && a.unstaged.is_none());
        assert!(map.get("TEST_IMPROVEMENT_PLAN.md").unwrap().untracked);
    }

    #[test]
    fn untracked_only_subdir_collapses_to_untracked_dir() {
        // A subtree whose only change is untracked content marks the
        // intermediate directory `?` (untracked), not `~` (modified).
        let map = parse("?? docs/drafts/notes.md\n", "docs");
        let dir = map.get("drafts/").unwrap();
        assert!(dir.untracked);
        assert!(dir.staged.is_none() && dir.unstaged.is_none());
        assert!(!map.contains_key("notes.md"));
    }

    #[test]
    fn mixed_subdir_prefers_modified_over_untracked() {
        // A dir containing both a tracked modification and an untracked
        // file reads as changed (`~`), regardless of which row git
        // emits first — tracked outranks untracked and never downgrades.
        let untracked_first = parse("?? sub/new.md\n M sub/old.md\n", "");
        let modified_first = parse(" M sub/old.md\n?? sub/new.md\n", "");
        for map in [untracked_first, modified_first] {
            let dir = map.get("sub/").unwrap();
            assert_eq!(dir.unstaged, Some(GitChange::Modified));
            assert!(!dir.untracked);
        }
    }

    #[test]
    fn rename_takes_new_name() {
        // `R ` = staged rename, working tree clean.
        let porcelain = "R  old.md -> new.md\n";
        let map = parse(porcelain, "");
        let s = map.get("new.md").unwrap();
        assert_eq!(s.staged, Some(GitChange::Renamed));
        assert!(s.unstaged.is_none());
        assert!(!map.contains_key("old.md"));
    }

    #[test]
    fn staged_only_modify() {
        let map = parse("M  foo.rs\n", "");
        let s = map.get("foo.rs").unwrap();
        assert_eq!(s.staged, Some(GitChange::Modified));
        assert!(s.unstaged.is_none());
    }

    #[test]
    fn partially_staged_modify() {
        // `MM` — staged modify + further unstaged edits. Both halves set.
        let map = parse("MM foo.rs\n", "");
        let s = map.get("foo.rs").unwrap();
        assert_eq!(s.staged, Some(GitChange::Modified));
        assert_eq!(s.unstaged, Some(GitChange::Modified));
    }

    #[test]
    fn conflict_marks_both_halves() {
        // `UU` — both sides unmerged. We collapse to Conflicted on both.
        let map = parse("UU foo.rs\n", "");
        let s = map.get("foo.rs").unwrap();
        assert_eq!(s.staged, Some(GitChange::Conflicted));
        assert_eq!(s.unstaged, Some(GitChange::Conflicted));
    }
}

#[cfg(test)]
mod parity_tests {
    use super::{StatusEntry, decode_porcelain, map_to_listing, repo_status, repo_status_stable};
    use crate::git::test_support::run_git;
    use std::path::{Path, PathBuf};

    /// Hermetic `git status --porcelain -unormal` stdout for `dir`, via the
    /// shared `run_git` fixture (so config — e.g. rename detection — matches
    /// the setup commands exactly).
    fn porcelain(dir: &Path) -> String {
        run_git(dir, &["status", "--porcelain", "-unormal"])
    }

    /// Fresh empty repo on `main`, canonicalized path (macOS /var→/private/var
    /// so gix's repo-relative paths line up). NO commit yet — callers add one.
    fn empty_repo() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap_or_else(|_| tmp.path().to_path_buf());
        run_git(&root, &["init", "-q", "--initial-branch=main"]);
        (tmp, root)
    }

    /// `empty_repo` plus one committed file `base.txt` so HEAD exists.
    fn repo_with_commit() -> (tempfile::TempDir, PathBuf) {
        let (tmp, root) = empty_repo();
        std::fs::write(root.join("base.txt"), "base\n").unwrap();
        run_git(&root, &["add", "base.txt"]);
        run_git(&root, &["commit", "-q", "-m", "c1"]);
        (tmp, root)
    }

    /// Sort entries by path for set-equality comparison (order from the two
    /// backends differs; gix runs producers in parallel threads).
    fn as_set(mut v: Vec<StatusEntry>) -> Vec<StatusEntry> {
        v.sort_by(|a, b| a.rela_path.cmp(&b.rela_path));
        v
    }

    /// The whole assertion bundle for one corpus case: gix decode == subprocess
    /// decode (as sets), AND `map_to_listing` agrees for prefix="" and a subdir
    /// prefix (isolating gix decode-correctness from the path mapping).
    fn assert_parity(root: &Path, subdir_prefix: &str) {
        let raw = porcelain(root);
        let subprocess = decode_porcelain(&raw);
        let gix = repo_status(root).expect("repo_status opens the repo");

        assert_eq!(
            as_set(gix.clone()),
            as_set(subprocess.clone()),
            "gix decode != subprocess decode\nporcelain:\n{raw}"
        );

        for prefix in ["", subdir_prefix] {
            let gix_map = map_to_listing(&gix, prefix);
            let sub_map = map_to_listing(&subprocess, prefix);
            assert_eq!(
                gix_map, sub_map,
                "map_to_listing diverged at prefix {prefix:?}\nporcelain:\n{raw}"
            );
        }
    }

    // --- Corpus: each case builds its OWN repo in its OWN tempdir. ----------

    #[test]
    fn case01_clean_repo() {
        let (_t, root) = repo_with_commit();
        let gix = repo_status(&root).expect("opens");
        assert!(gix.is_empty(), "clean repo has no entries, got {gix:?}");
        assert_parity(&root, "sub");
    }

    #[test]
    fn case02_modified_unstaged() {
        let (_t, root) = repo_with_commit();
        std::fs::write(root.join("base.txt"), "base\nmore\n").unwrap();
        assert_parity(&root, "sub");
    }

    #[test]
    fn case03_staged_only() {
        let (_t, root) = repo_with_commit();
        std::fs::write(root.join("base.txt"), "base\nstaged\n").unwrap();
        run_git(&root, &["add", "base.txt"]);
        assert_parity(&root, "sub");
    }

    #[test]
    fn case04_staged_then_modified() {
        let (_t, root) = repo_with_commit();
        std::fs::write(root.join("base.txt"), "base\nstaged\n").unwrap();
        run_git(&root, &["add", "base.txt"]);
        std::fs::write(root.join("base.txt"), "base\nstaged\nthen-more\n").unwrap();
        assert_parity(&root, "sub");
    }

    #[test]
    fn case05_added() {
        let (_t, root) = repo_with_commit();
        std::fs::write(root.join("new.txt"), "added\n").unwrap();
        run_git(&root, &["add", "new.txt"]);
        assert_parity(&root, "sub");
    }

    #[test]
    fn case06_staged_deletion() {
        let (_t, root) = repo_with_commit();
        run_git(&root, &["rm", "-q", "base.txt"]);
        assert_parity(&root, "sub");
    }

    #[test]
    fn case07_unstaged_deletion() {
        let (_t, root) = repo_with_commit();
        std::fs::remove_file(root.join("base.txt")).unwrap();
        assert_parity(&root, "sub");
    }

    #[test]
    fn case08_staged_rename() {
        let (_t, root) = repo_with_commit();
        // Give the rename real content so 50%-similarity detection fires.
        std::fs::write(root.join("orig.txt"), "line1\nline2\nline3\nline4\n").unwrap();
        run_git(&root, &["add", "orig.txt"]);
        run_git(&root, &["commit", "-q", "-m", "add orig"]);
        run_git(&root, &["mv", "orig.txt", "renamed.txt"]);
        assert_parity(&root, "sub");
    }

    #[test]
    fn case09_untracked_file() {
        let (_t, root) = repo_with_commit();
        std::fs::write(root.join("untracked.txt"), "u\n").unwrap();
        assert_parity(&root, "sub");
    }

    #[test]
    fn case10_fully_untracked_subdir() {
        // A wholly-untracked directory must collapse to a single `sub/` entry
        // (matching `-unormal`), not one entry per file. This requires
        // `UntrackedFiles::Collapsed` AND re-adding the trailing slash that
        // gix's `rela_path` drops for the collapsed directory — see
        // `repo_status`'s `DirectoryContents` arm.
        let (_t, root) = repo_with_commit();
        std::fs::create_dir(root.join("sub")).unwrap();
        std::fs::write(root.join("sub").join("a.txt"), "a\n").unwrap();
        std::fs::write(root.join("sub").join("b.txt"), "b\n").unwrap();
        assert_parity(&root, "sub");
    }

    #[test]
    fn case11_ignored_file_absent() {
        let (_t, root) = repo_with_commit();
        std::fs::write(root.join(".gitignore"), "ignored.log\n").unwrap();
        run_git(&root, &["add", ".gitignore"]);
        run_git(&root, &["commit", "-q", "-m", "gitignore"]);
        std::fs::write(root.join("ignored.log"), "noise\n").unwrap();
        let gix = repo_status(&root).expect("opens");
        assert!(
            !gix.iter().any(|e| e.rela_path == "ignored.log"),
            "ignored file must not appear: {gix:?}"
        );
        assert_parity(&root, "sub");
    }

    #[test]
    fn case12_deep_tracked_modified() {
        let (_t, root) = repo_with_commit();
        std::fs::create_dir(root.join("sub")).unwrap();
        std::fs::write(root.join("sub").join("deep.txt"), "v1\n").unwrap();
        run_git(&root, &["add", "sub/deep.txt"]);
        run_git(&root, &["commit", "-q", "-m", "add deep"]);
        std::fs::write(root.join("sub").join("deep.txt"), "v1\nv2\n").unwrap();
        assert_parity(&root, "sub");
    }

    #[test]
    fn case13_empty_repo_unborn_head() {
        let (_t, root) = empty_repo();
        // No commits — unborn HEAD. An untracked file should still surface.
        std::fs::write(root.join("first.txt"), "first\n").unwrap();
        let gix = repo_status(&root).expect("repo_status works on unborn HEAD");
        assert!(
            gix.iter()
                .any(|e| e.rela_path == "first.txt" && e.untracked),
            "untracked file on unborn HEAD should surface: {gix:?}"
        );
        assert_parity(&root, "sub");
    }

    #[test]
    fn case14_detached_head_clean() {
        let (_t, root) = repo_with_commit();
        run_git(&root, &["checkout", "-q", "--detach"]);
        let gix = repo_status(&root).expect("opens detached");
        assert!(
            gix.is_empty(),
            "detached clean tree has no entries: {gix:?}"
        );
        assert_parity(&root, "sub");
    }

    /// `repo_status_stable` on a quiescent repo agrees with a direct
    /// `repo_status` and stamps the live cache-key mtimes (the happy path; the
    /// racy-snapshot handling that fixes the stale-marker bug is unit-tested in
    /// `stable_walk_tests`).
    #[test]
    fn repo_status_stable_agrees_with_repo_status() {
        let (_t, root) = repo_with_commit();
        std::fs::write(root.join("base.txt"), "base\nmod\n").unwrap();
        let (entries, index_mtime, head_mtime) = repo_status_stable(&root);
        assert_eq!(
            as_set(entries.expect("stable walk yields entries")),
            as_set(repo_status(&root).expect("opens")),
            "stable walk agrees with a direct walk on a quiescent repo"
        );
        assert!(
            index_mtime.is_some() && head_mtime.is_some(),
            "quiescent repo → both cache-key mtimes stamped"
        );
    }
}
