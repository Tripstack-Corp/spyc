//! Git status queries: the legacy subprocess backend (`porcelain_raw`), the
//! gix backend (`repo_status`), and the shared path-mapping layer both feed.
//!
//! ## Two-stage pipeline
//!
//! Status flows through two decoupled stages:
//!
//! 1. **Decode** — produce one [`StatusEntry`] per changed *repo-relative*
//!    path. Two backends produce these: the subprocess one decodes
//!    `git status --porcelain` text ([`decode_porcelain`]); the gix one walks
//!    the index/worktree/tree diffs ([`repo_status`]).
//! 2. **Map to listing** — [`map_to_listing`] takes the decoded entries plus a
//!    dir-relative `prefix` and produces the basename-keyed
//!    [`GitFileStatus`](crate::ui::list_view::GitFileStatus) map the list view
//!    consumes (strip prefix, basename for in-dir files, aggregate deep files
//!    onto their parent dir).
//!
//! `sysinfo::parse_porcelain_statuses` is now just `decode_porcelain` +
//! `map_to_listing` — the live status path is unchanged and still authoritative
//! (PR 4 is a parity *spike*; nothing is flipped to gix yet).

use std::collections::HashMap;
use std::path::Path;

use crate::ui::list_view::{GitChange, GitFileStatus};

/// One changed repo-relative path, fully decoded into both porcelain halves.
/// This is the shared intermediate between the subprocess and gix backends:
/// each backend produces a `Vec<StatusEntry>`, and [`map_to_listing`] turns
/// that into the per-listing basename map regardless of which backend produced
/// it.
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
/// via the special-case markers.
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
/// changed path (repo-relative). This is the subprocess backend's stage-1
/// decode, factored out of `parse_porcelain_statuses` so both the live path
/// and the parity tests share it.
///
/// `pub` so the parity tests (and `sysinfo`) can call it; it carries no
/// path-mapping logic — that lives in [`map_to_listing`]. (`pub`, not
/// `pub(crate)`: the enclosing `git` module is private, so clippy's
/// `redundant_pub_crate` treats `pub(crate)` here as redundant.)
pub fn decode_porcelain(porcelain: &str) -> Vec<StatusEntry> {
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
    for entry in entries {
        let raw_path = entry.rela_path.as_str();
        // Strip the directory prefix to get a path relative to the current
        // listing dir (entries carry repo-relative paths).
        let filename = if prefix.is_empty() {
            raw_path
        } else {
            let pfx = if prefix.ends_with('/') {
                prefix.to_string()
            } else {
                format!("{prefix}/")
            };
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

/// Spawn `git status --porcelain` and return the raw stdout. Split out so
/// callers (e.g. the chdir path) can cache the raw text across navigations
/// within the same repo — the index walk is the expensive part of the spawn
/// and produces identical output for every dir under one repo root.
///
/// Returns `None` if the spawn fails or git exits non-zero.
///
/// Always requests untracked files (`-unormal`). We used to switch to `-uno`
/// on "huge" trees, but the huge-tree flag counts *on-disk* subdirs (dominated
/// by gitignored build dirs like `target/`), while git's untracked scan skips
/// gitignored dirs entirely — so `-unormal` is ~as cheap as `-uno` for the
/// repos that tripped the heuristic, and `-uno` was silently hiding the `?`
/// untracked markers. The cost of `-unormal` on a genuinely large *non-ignored*
/// tree is absorbed by the background git worker (off the UI thread) and the
/// 10 s huge-tree poll throttle.
pub fn porcelain_raw(dir: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["status", "--porcelain", "-unormal"])
        .current_dir(dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

/// gix backend (PR 4 parity spike): produce the same `Vec<StatusEntry>` as
/// [`decode_porcelain`] does for `git status --porcelain -unormal`, but without
/// shelling out — walking the index/worktree/tree diffs via gix directly.
///
/// Returns `None` if `repo_root` can't be opened as a repository or the status
/// walk errors. NOT yet wired into the live path; validated against the
/// subprocess truth by the parity tests below.
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
// PR 4 is a parity *spike*: this backend runs only from the parity tests, not
// the live status path (still the subprocess + `parse_porcelain_statuses`).
// PR 5 flips the hot path here and removes this allow. Scoped to this fn so a
// genuinely-dead helper elsewhere still trips the lint.
#[allow(dead_code)]
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

#[cfg(test)]
mod parity_tests {
    use super::{StatusEntry, decode_porcelain, map_to_listing, repo_status};
    use std::path::{Path, PathBuf};

    /// Run `git` in `dir` with a hermetic config (no user/system gitconfig) so
    /// tests are reproducible. Mirrors `discovery::tests::run_git`. `git` is a
    /// test-only fixture dependency — production status is pure gix
    /// (`repo_status`) plus the legacy subprocess (`porcelain_raw`).
    fn run_git(dir: &Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@x")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@x")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .expect("spawn git");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// Hermetic `git status --porcelain -unormal` stdout for `dir`. Uses the
    /// same env as `run_git` so config (e.g. rename detection) is identical to
    /// the setup commands.
    fn porcelain(dir: &Path) -> String {
        let out = std::process::Command::new("git")
            .args(["status", "--porcelain", "-unormal"])
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .expect("spawn git status");
        assert!(out.status.success(), "git status failed");
        String::from_utf8(out.stdout).expect("utf8 porcelain")
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
}
