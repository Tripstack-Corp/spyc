//! Repository discovery via gix: resolve the real git directory (following
//! a linked worktree's `.git` *file* to its per-worktree gitdir) and read
//! the current branch for the status-bar display. Replaces the hand-rolled
//! pure-fs `sysinfo::resolve_gitdir` / `read_head_branch` — gix handles
//! detached HEAD, packed refs, submodule/worktree gitlinks and bare repos
//! more robustly.
//!
//! HOT-PATH NOTE: opening a gix `Repository` parses config and sets up the
//! object/ref stores, so it is heavier than a single file read. Callers keep
//! the open off the per-event path: `gitdir` runs only at chdir-into-a-*new*-
//! repo (`set_repo_root` early-returns when the repo is unchanged), and
//! `head_branch`'s caller (`AppState::compute_git_info_fast`, via
//! `cached_head_branch`) memoizes the result by `HEAD`'s mtime — so although
//! `refresh_listing` *calls* it on every fs-event, the gix open only fires
//! when `HEAD` is actually rewritten (checkout / branch-switch). Neither runs
//! on the 1 Hz idle mtime poll, which stats the cached `current_gitdir`
//! directly. The cheap ancestor-walk that finds the repo *root*
//! (`find_repo_root`) stays pure-fs and runs every chdir.

use std::path::{Path, PathBuf};

/// Resolve the git directory for the repository whose working tree is
/// rooted at `repo_root` — for a normal repo `<root>/.git`, for a linked
/// worktree the per-worktree gitdir (`<main>/.git/worktrees/<name>`) where
/// `HEAD` and `index` live (those are what the mtime poll stats). `None`
/// if `repo_root` isn't a repository gix can open.
pub fn gitdir(repo_root: &Path) -> Option<PathBuf> {
    let repo = gix::open(repo_root).ok()?;
    Some(repo.git_dir().to_path_buf())
}

/// Branch display string for the repo at `repo_root`: the short branch
/// name for an attached HEAD (`refs/heads/main` → `main`), or a 7-char
/// commit prefix for a detached HEAD (matching git's `--short` default and
/// the prior `read_head_branch` behavior). `None` if not a repository or
/// HEAD can't be resolved.
pub fn head_branch(repo_root: &Path) -> Option<String> {
    let repo = gix::open(repo_root).ok()?;
    if let Some(name) = repo.head_name().ok()? {
        Some(name.shorten().to_string())
    } else {
        // Detached HEAD — short commit hash.
        let id = repo.head_id().ok()?;
        Some(id.to_hex_with_len(7).to_string())
    }
}

/// True if `path` is tracked by git — present in the repository index (so it's
/// committed or staged). Used by teardown cleanup to refuse to delete a config
/// file the user has checked in. Returns `false` when there's no enclosing
/// repo, the path lies outside its worktree, or it simply isn't in the index.
/// Uses `gix::discover` so a path in a repo subdirectory still resolves.
pub fn is_tracked(path: &Path) -> bool {
    let Some(dir) = path.parent() else {
        return false;
    };
    let Ok(repo) = gix::discover(dir) else {
        return false;
    };
    let Some(workdir) = repo.workdir() else {
        return false;
    };
    let Ok(rela) = path.strip_prefix(workdir) else {
        return false;
    };
    let Ok(index) = repo.index() else {
        return false;
    };
    // The index keys on forward-slash repo-relative paths.
    let rela = rela.to_string_lossy().replace('\\', "/");
    index.entry_by_path(gix::bstr::BStr::new(&rela)).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::test_support::run_git;

    fn init_repo() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap_or_else(|_| tmp.path().to_path_buf());
        run_git(&root, &["init", "-q", "--initial-branch=main"]);
        std::fs::write(root.join("f.txt"), "v1\n").unwrap();
        run_git(&root, &["add", "f.txt"]);
        run_git(&root, &["commit", "-q", "-m", "c1"]);
        (tmp, root)
    }

    #[test]
    fn gitdir_normal_repo_is_dot_git() {
        let (_tmp, root) = init_repo();
        assert_eq!(gitdir(&root).unwrap(), root.join(".git"));
    }

    #[test]
    fn gitdir_none_outside_repo() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(gitdir(tmp.path()).is_none());
    }

    /// LOAD-BEARING: for a linked worktree, `gitdir` must return the
    /// *per-worktree* gitdir (`<main>/.git/worktrees/<name>`), NOT the shared
    /// common `.git`. The 1 Hz git poll + the fs-watch both stat this path's
    /// `index`/`HEAD`; a worktree commit moves the per-worktree `index` but not
    /// the common one, so if this collapsed to the common dir, column b's
    /// markers would freeze at open-time state and never refresh.
    #[test]
    fn gitdir_linked_worktree_is_per_worktree_not_common() {
        let (_tmp, root) = init_repo();
        let wt_parent = tempfile::tempdir().unwrap();
        let wt = std::fs::canonicalize(wt_parent.path())
            .unwrap_or_else(|_| wt_parent.path().to_path_buf())
            .join("wt");
        run_git(
            &root,
            &["worktree", "add", "-b", "feature", wt.to_str().unwrap()],
        );

        let common = gitdir(&root).expect("main gitdir"); // <root>/.git
        let per_wt = gitdir(&wt).expect("worktree gitdir");
        assert_ne!(
            per_wt, common,
            "a linked worktree's gitdir must differ from the common .git"
        );
        assert!(
            per_wt.components().any(|c| c.as_os_str() == "worktrees"),
            "expected a per-worktree gitdir under .git/worktrees, got {per_wt:?}"
        );
        assert!(
            per_wt.join("index").exists(),
            "the per-worktree gitdir should hold the worktree's own index at {per_wt:?}"
        );
    }

    #[test]
    fn head_branch_attached_is_branch_name() {
        let (_tmp, root) = init_repo();
        assert_eq!(head_branch(&root).as_deref(), Some("main"));
    }

    #[test]
    fn head_branch_detached_is_short_hash() {
        let (_tmp, root) = init_repo();
        // Detach onto the commit itself.
        run_git(&root, &["checkout", "-q", "--detach"]);
        let b = head_branch(&root).expect("detached head still resolves");
        assert_eq!(b.len(), 7, "expected 7-char short hash, got {b:?}");
        assert!(b.chars().all(|c| c.is_ascii_hexdigit()), "not hex: {b:?}");
    }

    #[test]
    fn is_tracked_distinguishes_committed_untracked_and_no_repo() {
        let (_tmp, root) = init_repo();
        // `f.txt` was committed by the fixture → tracked.
        assert!(is_tracked(&root.join("f.txt")));
        // A new, unadded file in the repo → not tracked.
        std::fs::write(root.join("untracked.txt"), "x\n").unwrap();
        assert!(!is_tracked(&root.join("untracked.txt")));
        // A path with no enclosing repo → not tracked.
        let outside = tempfile::tempdir().unwrap();
        assert!(!is_tracked(&outside.path().join("anything.txt")));
    }

    #[test]
    fn is_tracked_sees_a_staged_but_uncommitted_file() {
        let (_tmp, root) = init_repo();
        std::fs::write(root.join("staged.txt"), "y\n").unwrap();
        run_git(&root, &["add", "staged.txt"]);
        // Staged (in the index) counts as tracked — the teardown guard must not
        // delete a config the user has `git add`ed even before committing.
        assert!(is_tracked(&root.join("staged.txt")));
    }

    // Linked-worktree gitdir resolution (gix following a worktree's `.git`
    // *file* to `<main>/.git/worktrees/<name>`) is gix's own well-tested
    // feature; a unit test for it here would need `git worktree add`, which
    // is non-idempotent and flaked under the suite's parallel-spawn load
    // (sibling tests `set_current_dir` + drop tempdirs, transiently
    // invalidating the process CWD). It's covered by the PR-3 smoke-test
    // (cd into a real worktree) instead.
}
