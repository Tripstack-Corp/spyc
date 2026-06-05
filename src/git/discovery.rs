//! Repository discovery via gix: resolve the real git directory (following
//! a linked worktree's `.git` *file* to its per-worktree gitdir) and read
//! the current branch for the status-bar display. Replaces the hand-rolled
//! pure-fs `sysinfo::resolve_gitdir` / `read_head_branch` — gix handles
//! detached HEAD, packed refs, submodule/worktree gitlinks and bare repos
//! more robustly.
//!
//! HOT-PATH NOTE: opening a gix `Repository` parses config and sets up the
//! object/ref stores, so it is heavier than a single file read. These are
//! called only at chdir-into-a-*new*-repo (`set_repo_root` early-returns
//! when the repo is unchanged) and on HEAD/index change (branch refresh) —
//! never on the 1 Hz idle mtime poll, which stats the cached
//! `current_gitdir` directly. The cheap ancestor-walk that finds the repo
//! *root* (`find_repo_root`) stays pure-fs and runs every chdir.

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

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a throwaway repo and run `git` in it with a hermetic config
    /// (no user/system `.gitconfig`) so the test is reproducible. `git` is
    /// a test-only fixture dependency — production discovery is pure gix.
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

    // Linked-worktree gitdir resolution (gix following a worktree's `.git`
    // *file* to `<main>/.git/worktrees/<name>`) is gix's own well-tested
    // feature; a unit test for it here would need `git worktree add`, which
    // is non-idempotent and flaked under the suite's parallel-spawn load
    // (sibling tests `set_current_dir` + drop tempdirs, transiently
    // invalidating the process CWD). It's covered by the PR-3 smoke-test
    // (cd into a real worktree) instead.
}
