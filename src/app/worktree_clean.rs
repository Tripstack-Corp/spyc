//! Safe-by-default worktree teardown — the shared implementation behind the
//! MCP `remove_worktree` *and* `clean_worktree` (folded: `clean` is an alias).
//!
//! Strict `worktree::remove` refuses a dirty worktree (like `git worktree
//! remove`). Safe-remove instead **preserves** the work and tears the tree
//! down: it archives the worktree's untracked *and* uncommitted-tracked file
//! contents into the graveyard (recoverable via `gy` / `:undo`), force-removes
//! the worktree, and then deletes its branch **iff that branch is merged** into
//! the repo's integration base (`delete_branch: auto`) — an unmerged branch's
//! ref is kept, since the ref itself is the commit backup (owner decisions,
//! 2026-06-24). A *claimed* (locked) worktree is still refused — a lease is
//! honored even here; release it first.
//!
//! Why archive file *contents* rather than a `.patch`: the current files are
//! the work, they round-trip through the graveyard's existing tree-archive
//! path with no diff machinery, and the committed history stays in the repo /
//! on the kept ref. App-layer (bridges `git::status` + `git::worktree` +
//! `git::branch` + the `graveyard`); takes a path, needs no `App`, unit-tested.

use std::path::{Path, PathBuf};

use crate::fs::ops::copy_tree;
use crate::git::{status, worktree};
use crate::state::graveyard::Graveyard;

/// What a successful safe-remove preserved / did.
#[derive(Debug)]
pub struct SafeRemoveReport {
    /// Number of untracked + uncommitted-tracked entries archived (0 when clean).
    pub archived: usize,
    /// Graveyard label the archived files were stored under, if any.
    pub label: Option<String>,
    /// The worktree's branch (`None` for a detached HEAD).
    pub branch: Option<String>,
    /// `true` when the branch was deleted because it was merged into the base.
    pub branch_deleted: bool,
    /// `Some(n)` when the branch was *kept* because it has `n` unmerged commits.
    pub kept_unmerged_ahead: Option<usize>,
}

/// Archive the worktree's untracked + uncommitted-tracked content to the
/// graveyard, force-remove it, then delete its branch iff merged. Errors —
/// changing nothing — when `path` isn't a readable git worktree, is claimed
/// (locked), or archiving fails.
pub fn safe_remove_worktree(path: &Path) -> std::io::Result<SafeRemoveReport> {
    let statuses = status::repo_status(path)
        .ok_or_else(|| std::io::Error::other("not a git worktree, or its status can't be read"))?;

    // Honor a lease BEFORE archiving — don't archive then refuse. A claim is
    // respected even by safe-remove; release it first.
    if let Some(reason) = worktree::lock_reason(path) {
        let reason = reason.trim();
        return Err(std::io::Error::other(if reason.is_empty() {
            "worktree is locked (claimed) — release it first".to_string()
        } else {
            format!("worktree is locked (claimed): {reason} — release it first")
        }));
    }

    // Branch / merged-ness, resolved BEFORE removal (the branch ref must still
    // exist and the tree must still be present to read its status).
    let (branch, repo_root, base) = resolve_branch_context(path);
    let merged_status = match (repo_root.as_deref(), branch.as_deref(), base.as_deref()) {
        (Some(root), Some(br), Some(base)) => crate::git::branch::branch_status(root, br, base),
        _ => None,
    };
    let merged = merged_status.is_some_and(|s| s.merged);

    // Archive every dirty entry that still exists on disk. A deletion (tracked
    // file removed) leaves nothing to copy — it's recoverable from the commit /
    // kept ref, so skip it.
    let dirty: Vec<&str> = statuses
        .iter()
        .filter(|e| e.untracked || e.staged.is_some() || e.unstaged.is_some())
        .map(|e| e.rela_path.as_str())
        .filter(|rel| path.join(rel).exists())
        .collect();
    let (archived, label) = archive_dirty(path, &dirty)?;

    // Tree is preserved → force past the dirty refusal. (A lease was ruled out
    // above; `remove_force` still re-checks it, harmlessly.)
    worktree::remove_force(path)?;

    // delete_branch: auto — delete iff merged, and never the base branch itself.
    let branch_deleted = merged
        && match (repo_root.as_deref(), branch.as_deref(), base.as_deref()) {
            (Some(root), Some(br), Some(base)) if br != base => {
                crate::git::branch::delete(root, br).is_ok()
            }
            _ => false,
        };
    let kept_unmerged_ahead = if merged {
        None
    } else {
        merged_status.map(|s| s.ahead)
    };

    Ok(SafeRemoveReport {
        archived,
        label,
        branch,
        branch_deleted,
        kept_unmerged_ahead,
    })
}

/// Resolve a worktree's branch, its repo's MAIN root (from the shared common
/// dir — survives whichever worktree `path` is), and the integration base.
fn resolve_branch_context(path: &Path) -> (Option<String>, Option<PathBuf>, Option<String>) {
    let Ok(repo) = gix::discover(path) else {
        return (None, None, None);
    };
    let branch = repo
        .head_name()
        .ok()
        .flatten()
        .map(|n| n.shorten().to_string());
    let repo_root = std::fs::canonicalize(repo.common_dir())
        .ok()
        .and_then(|cd| gix::open(&cd).ok())
        .and_then(|main| main.workdir().map(Path::to_path_buf));
    let base = repo_root
        .as_deref()
        .and_then(crate::git::branch::default_base);
    (branch, repo_root, base)
}

/// Copy the listed dirty entries into a temp staging tree (mirroring their
/// relative paths, OUTSIDE the worktree so the copy doesn't re-discover
/// itself), archive that tree under `<worktree-name>-<ts>`, drop the staging
/// copy. Copy (not move): a failed archive leaves the worktree intact.
fn archive_dirty(path: &Path, dirty: &[&str]) -> std::io::Result<(usize, Option<String>)> {
    if dirty.is_empty() {
        return Ok((0, None));
    }
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("worktree");
    let stamp = crate::sysinfo::epoch_secs();
    let entry_name = format!("{name}-{stamp}");
    // Staging dir must be unique PER CALL. PID + epoch-seconds was not: two
    // safe-removes in the same wall-clock second (the parallel test suite, but
    // equally two concurrent MCP worktree jobs in one process) computed the
    // same path, and one's cleanup `remove_dir_all` then nuked the other's
    // staging mid-copy → a spurious `NotFound`. A v7 uuid is collision-free, so
    // no pre-clear is needed (the dir is always fresh).
    let staging = std::env::temp_dir().join(format!(".spyc-wt-remove-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&staging)?;
    let archive = (|| -> std::io::Result<()> {
        for rel in dirty {
            let dst = staging.join(rel);
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent)?;
            }
            copy_tree(&path.join(rel), &dst)?;
        }
        Graveyard::write_entry_as(&staging, &entry_name, path.to_path_buf())?;
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&staging);
    archive?; // archiving failed → worktree untouched, bail.
    Ok((dirty.len(), Some(entry_name)))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Route through the cwd-pinned + retrying test helper; the local
    // `current_dir(dir)` spawn flaked under the parallel suite's cwd thrash
    // (`safe_remove_archives_uncommitted_tracked_changes` was intermittently
    // failing the gate).
    fn run_git(dir: &Path, args: &[&str]) {
        crate::git::test_support::run_git(dir, args);
    }

    fn make_repo(root: &Path) {
        std::fs::create_dir_all(root).unwrap();
        run_git(root, &["init", "-q", "--initial-branch=main"]);
        std::fs::write(root.join("f.txt"), "v1\n").unwrap();
        run_git(root, &["add", "f.txt"]);
        run_git(root, &["commit", "-q", "-m", "v1"]);
    }

    fn branch_exists(repo: &Path, branch: &str) -> bool {
        gix::open(repo)
            .unwrap()
            .find_reference(&format!("refs/heads/{branch}"))
            .is_ok()
    }

    /// Untracked files are archived to the graveyard, the worktree removed, and
    /// a merged branch (no commits past base) deleted.
    #[test]
    fn safe_remove_archives_untracked_and_deletes_merged_branch() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let repo = std::fs::canonicalize(tmp.path()).unwrap().join("repo");
            make_repo(&repo);
            let wt = worktree::add(&repo, "wt-clean", None).unwrap();
            std::fs::write(wt.join("scratch.log"), "junk\n").unwrap(); // untracked

            let report = safe_remove_worktree(&wt).expect("safe-remove succeeds");
            assert_eq!(report.archived, 1, "one untracked entry archived");
            let label = report.label.expect("a graveyard label");
            assert!(
                label.starts_with("wt-clean-"),
                "labelled by worktree: {label}"
            );
            assert!(!wt.exists(), "worktree removed");
            assert_eq!(report.branch.as_deref(), Some("wt-clean"));
            assert!(report.branch_deleted, "merged branch deleted");
            assert!(!branch_exists(&repo, "wt-clean"), "branch ref gone");

            let g = Graveyard::load();
            assert!(
                g.entries.iter().any(|e| e.filename == label),
                "graveyard holds the archived entry"
            );
        });
    }

    /// Uncommitted changes to a TRACKED file are now ARCHIVED (not refused),
    /// the worktree removed.
    #[test]
    fn safe_remove_archives_uncommitted_tracked_changes() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let repo = std::fs::canonicalize(tmp.path()).unwrap().join("repo");
            make_repo(&repo);
            let wt = worktree::add(&repo, "wt-dirty", None).unwrap();
            std::fs::write(wt.join("f.txt"), "v2-uncommitted\n").unwrap(); // tracked edit

            let report = safe_remove_worktree(&wt).expect("safe-remove archives & removes");
            assert_eq!(report.archived, 1, "the modified tracked file is archived");
            assert!(!wt.exists(), "worktree removed despite the tracked edit");

            // The edited content is recoverable from the graveyard staging tree.
            let label = report.label.expect("label");
            let g = Graveyard::load();
            assert!(g.entries.iter().any(|e| e.filename == label));
        });
    }

    /// An UNMERGED branch (commits past base) is KEPT — its ref is the backup.
    #[test]
    fn safe_remove_keeps_unmerged_branch() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let repo = std::fs::canonicalize(tmp.path()).unwrap().join("repo");
            make_repo(&repo);
            let wt = worktree::add(&repo, "wt-ahead", None).unwrap();
            std::fs::write(wt.join("new.txt"), "x\n").unwrap();
            run_git(&wt, &["add", "new.txt"]);
            run_git(&wt, &["commit", "-q", "-m", "ahead"]); // 1 commit past main

            let report = safe_remove_worktree(&wt).expect("safe-remove succeeds");
            assert!(!wt.exists(), "worktree removed");
            assert!(!report.branch_deleted, "unmerged branch kept");
            assert_eq!(
                report.kept_unmerged_ahead,
                Some(1),
                "reports 1 commit ahead"
            );
            assert!(branch_exists(&repo, "wt-ahead"), "branch ref preserved");
        });
    }

    /// A claimed (locked) worktree is refused, intact, before any archiving.
    #[test]
    fn safe_remove_refuses_a_claimed_worktree() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let repo = std::fs::canonicalize(tmp.path()).unwrap().join("repo");
            make_repo(&repo);
            let wt = worktree::add(&repo, "wt-leased", None).unwrap();
            worktree::lock(&wt, "agent B: busy").unwrap();

            let err = safe_remove_worktree(&wt).unwrap_err();
            assert!(
                err.to_string().contains("agent B: busy"),
                "cites lease: {err}"
            );
            assert!(wt.exists(), "leased worktree left intact");
        });
    }
}
