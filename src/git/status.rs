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

/// Build the gix status [`Platform`] with the parity-sensitive options shared
/// between [`repo_status`] and
/// [`crate::git::diff_model::build::collect_worktree_plan`]: staged-rename
/// detection at git's default (-M50%), no index↔worktree rewrite detection
/// (see `case08b_unstaged_rename` for why that must stay off). The `untracked`
/// mode differs between callers — `Collapsed` for porcelain parity, `Files`
/// for the working-tree diff plan. Keeping this config in one place ensures
/// the two callers can't drift on parity-sensitive options.
pub fn make_status_platform(
    repo: &gix::Repository,
    untracked: gix::status::UntrackedFiles,
) -> Option<gix::status::Platform<'_, gix::progress::Discard>> {
    use gix::status::tree_index::TrackRenames;
    Some(
        repo.status(gix::progress::Discard)
            .ok()?
            .untracked_files(untracked)
            .tree_index_track_renames(TrackRenames::Given(gix::diff::Rewrites::default())),
    )
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
    use gix::status::{Item, UntrackedFiles};

    let repo = gix::open(repo_root).ok()?;
    // `-unormal`: collapse a fully-untracked dir to one `dir/` entry, but list
    // an untracked file inside a tracked dir individually. See the doc comment
    // above for why this is `Collapsed`, not `Files`. Rename-detection config
    // is shared with `collect_worktree_plan` via `make_status_platform`.
    let platform = make_status_platform(&repo, UntrackedFiles::Collapsed)?;

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
        // Tolerate a single bad item: one path that fails to decode (e.g. an
        // unreadable worktree entry) must not blank the *entire* repo's git
        // markers. Skip it and keep surfacing status for every other path —
        // partial status beats none. (A failure to even start the walk above
        // still returns None: there's nothing to show.)
        let Ok(item) = item else { continue };
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
                IwItem::Rewrite {
                    source,
                    dirwalk_entry,
                    ..
                } => {
                    // With `index_worktree_rewrites` left disabled (see the
                    // platform setup above) gix doesn't emit this for the
                    // index↔worktree half, matching git porcelain: a
                    // worktree-only `mv` (no `git add`) shows as a deletion of
                    // the source plus an *untracked* destination — git won't
                    // pair them into one `R`, because the destination isn't
                    // tracked. Decode it that way so the porcelain parity
                    // contract holds even if rewrite detection is ever
                    // re-enabled here.
                    entry(&mut by_path, source.rela_path().to_str_lossy().into_owned()).unstaged =
                        Some(GitChange::Deleted);
                    entry(
                        &mut by_path,
                        dirwalk_entry.rela_path.to_str_lossy().into_owned(),
                    )
                    .untracked = true;
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
mod tests;
