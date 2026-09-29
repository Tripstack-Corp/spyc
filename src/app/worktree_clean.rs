//! Safe-by-default worktree teardown — the shared implementation behind the
//! MCP `remove_worktree` *and* `clean_worktree` (folded: `clean` is an alias).
//!
//! Strict `worktree::remove` refuses a dirty worktree (like `git worktree
//! remove`). Safe-remove instead **preserves** the work and tears the tree
//! down: it archives the worktree's untracked *and* uncommitted-tracked file
//! contents into the graveyard (recoverable via `:graveyard` / `:undo`), force-removes
//! the worktree, and then deletes its branch **iff that branch is merged** into
//! the repo's integration base (`delete_branch: auto`) — an unmerged branch's
//! ref is kept, since the ref itself is the commit backup (owner decisions,
//! 2026-06-24). A *claimed* (locked) worktree is still refused — a lease is
//! honoured even here; release it first.
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
    /// `true` when this finished an earlier removal that failed partway.
    pub resumed: bool,
    /// What a process still writing into the worktree kept from being
    /// deleted (`worktree::Removal::leftovers`). The removal itself succeeded.
    pub leftovers: Vec<PathBuf>,
}

/// Archive the worktree's untracked + uncommitted-tracked content to the
/// graveyard, force-remove it, then delete its branch iff merged. Errors —
/// changing nothing — when `path` isn't a readable git worktree, is claimed
/// (locked), or archiving fails.
///
/// `repo_hint` is any dir inside the worktree's repo. It finds a worktree
/// whose `.git` is already gone (an earlier removal that failed partway), so
/// this call finishes that removal instead of refusing it.
pub fn safe_remove_worktree(
    path: &Path,
    repo_hint: Option<&Path>,
) -> std::io::Result<SafeRemoveReport> {
    // No `.git` but an admin dir that names this path: an earlier removal
    // failed partway (#327). Finish it rather than refuse it.
    if !path.join(".git").exists()
        && let Some(hint) = repo_hint
        && let Some(admin_dir) = worktree::stranded_admin_dir(path, hint)
    {
        return finish_stranded(path, &admin_dir, hint);
    }

    // Distinguish the three ways this can fail, because they need different
    // responses from whoever reads the message: a wrong path, a directory that
    // isn't a worktree, or a status read that failed for a reason of its own
    // (a lock held by a concurrent git, a permissions problem). The single
    // collapsed message this replaced blamed the path in all three cases, so a
    // transient failure looked structural.
    let statuses = if !path.exists() {
        return Err(std::io::Error::other(format!(
            "no such path: {}",
            path.display()
        )));
    } else if !path.join(".git").exists() {
        return Err(std::io::Error::other(format!(
            "not a git worktree (no .git): {}",
            path.display()
        )));
    } else {
        status::repo_status_result(path)
            .map_err(|e| std::io::Error::other(format!("reading worktree status: {e}")))?
    };

    // Honour a lease BEFORE archiving — don't archive then refuse. A claim is
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
    let context = BranchContext::of(path);

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
    let removal = worktree::remove_force(path)?;
    Ok(context.finish(archived, label, removal, false))
}

/// Finish a removal that failed partway: the `.git` gitfile is gone, so there
/// is no status to read, and nothing is archived — the attempt that stranded
/// it archived the work before it deleted anything, and what its delete left
/// is a partial tree. The branch comes from the admin dir's `HEAD`.
fn finish_stranded(
    path: &Path,
    admin_dir: &Path,
    repo_hint: &Path,
) -> std::io::Result<SafeRemoveReport> {
    let mut context = gix::discover(repo_hint).map_or_else(
        |_| BranchContext::default(),
        |repo| BranchContext::from_repo(&repo),
    );
    context.branch = worktree::admin_branch(admin_dir);
    context.resolve_merged();
    let removal = worktree::finish_removal(path, admin_dir)?;
    Ok(context.finish(0, None, removal, true))
}

/// A worktree's branch, its repo's MAIN root and integration base, and whether
/// the branch is merged there: resolved before removal, used after it.
#[derive(Default)]
struct BranchContext {
    branch: Option<String>,
    repo_root: Option<PathBuf>,
    base: Option<String>,
    merged_status: Option<crate::git::branch::BranchStatus>,
}

impl BranchContext {
    fn of(path: &Path) -> Self {
        let Ok(repo) = gix::discover(path) else {
            return Self::default();
        };
        let mut context = Self::from_repo(&repo);
        context.branch = repo
            .head_name()
            .ok()
            .flatten()
            .map(|n| n.shorten().to_string());
        context.resolve_merged();
        context
    }

    /// The MAIN root (from the shared common dir, so it survives whichever
    /// worktree `repo` is) and its integration base.
    fn from_repo(repo: &gix::Repository) -> Self {
        let repo_root = std::fs::canonicalize(repo.common_dir())
            .ok()
            .and_then(|cd| gix::open(&cd).ok())
            .and_then(|main| main.workdir().map(Path::to_path_buf));
        let base = repo_root
            .as_deref()
            .and_then(crate::git::branch::default_base);
        Self {
            branch: None,
            repo_root,
            base,
            merged_status: None,
        }
    }

    fn resolve_merged(&mut self) {
        self.merged_status = match (
            self.repo_root.as_deref(),
            self.branch.as_deref(),
            self.base.as_deref(),
        ) {
            (Some(root), Some(br), Some(base)) => crate::git::branch::branch_status(root, br, base),
            _ => None,
        };
    }

    /// delete_branch: auto — delete iff merged, and never the base branch
    /// itself; then report.
    fn finish(
        self,
        archived: usize,
        label: Option<String>,
        removal: worktree::Removal,
        resumed: bool,
    ) -> SafeRemoveReport {
        let merged = self.merged_status.as_ref().is_some_and(|s| s.merged);
        let branch_deleted = merged
            && match (
                self.repo_root.as_deref(),
                self.branch.as_deref(),
                self.base.as_deref(),
            ) {
                (Some(root), Some(br), Some(base)) if br != base => {
                    crate::git::branch::delete(root, br).is_ok()
                }
                _ => false,
            };
        let kept_unmerged_ahead = if merged {
            None
        } else {
            self.merged_status.map(|s| s.ahead)
        };
        SafeRemoveReport {
            archived,
            label,
            branch: self.branch,
            branch_deleted,
            kept_unmerged_ahead,
            resumed,
            leftovers: removal.leftovers,
        }
    }
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

            let report = safe_remove_worktree(&wt, None).expect("safe-remove succeeds");
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

            let report = safe_remove_worktree(&wt, None).expect("safe-remove archives & removes");
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

            let report = safe_remove_worktree(&wt, None).expect("safe-remove succeeds");
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

            let err = safe_remove_worktree(&wt, None).unwrap_err();
            assert!(
                err.to_string().contains("agent B: busy"),
                "cites lease: {err}"
            );
            assert!(wt.exists(), "leased worktree left intact");
        });
    }

    /// The admin dir a worktree's `.git` gitfile points at.
    fn admin_of(wt: &Path) -> PathBuf {
        let gitfile = std::fs::read_to_string(wt.join(".git")).unwrap();
        PathBuf::from(gitfile.trim().strip_prefix("gitdir:").unwrap().trim())
    }

    /// #327: a removal that failed partway (a background writer in `target/`
    /// made `remove_dir_all` hit ENOTEMPTY) had already unlinked the `.git`
    /// gitfile, leaving a half-deleted tree, its admin dir and its branch.
    /// Removing it again finishes the job instead of refusing it as "not a git
    /// worktree".
    #[test]
    fn a_partially_removed_worktree_is_finished_not_refused() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let repo = std::fs::canonicalize(tmp.path()).unwrap().join("repo");
            make_repo(&repo);
            let wt = worktree::add(&repo, "wt-stranded", None).unwrap();
            let admin = admin_of(&wt);
            std::fs::remove_file(wt.join(".git")).unwrap();
            std::fs::create_dir_all(wt.join("target/debug")).unwrap();
            std::fs::write(wt.join("target/debug/leftover"), "x").unwrap();

            let report = safe_remove_worktree(&wt, Some(&repo)).expect("finishes the removal");
            assert!(!wt.exists(), "the half-deleted tree is gone");
            assert!(!admin.exists(), "no admin dir left under .git/worktrees");
            assert!(report.branch_deleted, "the merged branch is deleted");
            assert!(!branch_exists(&repo, "wt-stranded"));
            assert!(report.resumed, "reported as finishing an earlier removal");
        });
    }

    /// Finishing a removal still honours a claim, and a plain directory no
    /// admin dir points at is still refused, hint or not.
    #[test]
    fn finishing_a_removal_keeps_the_refusals() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let repo = std::fs::canonicalize(tmp.path()).unwrap().join("repo");
            make_repo(&repo);
            let wt = worktree::add(&repo, "wt-claimed", None).unwrap();
            let admin = admin_of(&wt);
            std::fs::write(admin.join("locked"), "agent-7 is merging").unwrap();
            std::fs::remove_file(wt.join(".git")).unwrap();
            let err = safe_remove_worktree(&wt, Some(&repo))
                .unwrap_err()
                .to_string();
            assert!(err.contains("agent-7 is merging"), "{err}");
            assert!(admin.is_dir(), "a claimed worktree is left alone");

            let plain = repo.parent().unwrap().join("plain");
            std::fs::create_dir(&plain).unwrap();
            let err = safe_remove_worktree(&plain, Some(&repo))
                .unwrap_err()
                .to_string();
            assert!(err.contains("not a git worktree"), "{err}");
            assert!(plain.is_dir(), "a plain dir is never deleted");
        });
    }

    /// A refusal must say WHICH failure it hit and name the path.
    ///
    /// These three used to share one message — "not a git worktree, or its
    /// status can't be read" — which blames the target in every case. That is
    /// how a transient status failure got investigated as a structural bug.
    #[test]
    fn safe_remove_refusals_name_their_cause_and_path() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("nope");
        let err = safe_remove_worktree(&missing, None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("no such path"), "missing path: {err}");
        assert!(err.contains("nope"), "names the path: {err}");

        // Exists, but isn't a worktree.
        let plain = tmp.path().join("plain");
        std::fs::create_dir(&plain).unwrap();
        let err = safe_remove_worktree(&plain, None).unwrap_err().to_string();
        assert!(err.contains("not a git worktree"), "plain dir: {err}");
        assert!(err.contains("plain"), "names the path: {err}");
        assert!(
            !err.contains("no such path"),
            "must not claim the path is missing: {err}"
        );
    }

    /// `repo_status_result` carries the cause the `Option` variant discards,
    /// and the `Option` variant still answers `None` for the same input — the
    /// marker hot path keeps its behaviour.
    #[test]
    fn repo_status_result_reports_why_and_option_still_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let plain = tmp.path().join("not-a-repo");
        std::fs::create_dir(&plain).unwrap();

        let err = crate::git::status::repo_status_result(&plain)
            .unwrap_err()
            .to_string();
        assert!(err.contains("open"), "names the failing step: {err}");
        assert!(err.contains("not-a-repo"), "names the path: {err}");
        assert!(
            crate::git::status::repo_status(&plain).is_none(),
            "the Option variant is unchanged for the hot path"
        );
    }
}
