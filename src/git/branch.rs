//! Default-base detection — the repo's integration branch (`main`/`master`/…),
//! resolved *without regard to what any worktree currently has checked out*.
//!
//! This is what lets `create_worktree` base a new worktree off PROJECT_HOME's
//! default branch instead of the focused column's HEAD (the POLA fix in
//! `docs/WORKTREE_MCP_PLAN.md` §3/§7). The richer branch-relationship queries
//! (merge-base, ahead/behind, merged-ness) land with `list_worktrees`, their
//! first consumer.
//!
//! Pure facade, in-process gix: paths in, owned `Send` data out, no `App`
//! dependency — same contract as the rest of `src/git/`.

use std::path::Path;

/// The repo's default / integration branch, resolved in order:
/// `origin/HEAD`'s symref target → `main` → `master` → first local branch.
/// `None` if `repo_root` isn't a repository or has no branches.
///
/// Independent of `HEAD`: it answers "what is the trunk?", not "what is checked
/// out?" — so a new worktree can branch off the trunk regardless of where the
/// asking column happens to be.
pub fn default_base(repo_root: &Path) -> Option<String> {
    let repo = gix::open(repo_root).ok()?;
    default_base_in(&repo)
}

fn default_base_in(repo: &gix::Repository) -> Option<String> {
    // 1. `origin/HEAD` is a symref to `refs/remotes/origin/<default>` — the
    //    most authoritative signal when there's a remote.
    if let Ok(r) = repo.find_reference("refs/remotes/origin/HEAD")
        && let Some(name) = r.target().try_name()
        && let Some(branch) = name
            .as_bstr()
            .to_string()
            .strip_prefix("refs/remotes/origin/")
        && !branch.is_empty()
        && branch != "HEAD"
    {
        return Some(branch.to_string());
    }
    // 2. Conventional integration-branch names.
    for cand in ["main", "master"] {
        if repo.find_reference(&format!("refs/heads/{cand}")).is_ok() {
            return Some(cand.to_string());
        }
    }
    // 3. Fall back to the first local branch.
    repo.references()
        .ok()?
        .local_branches()
        .ok()?
        .find_map(Result::ok)
        .map(|r| r.name().shorten().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::test_support::run_git;
    use std::path::PathBuf;

    /// A repo at `<tmp>/repo` on `main` with one commit.
    fn init_repo() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("repo");
        std::fs::create_dir(&root).unwrap();
        run_git(&root, &["init", "-q", "--initial-branch=main"]);
        std::fs::write(root.join("a.txt"), "a\n").unwrap();
        run_git(&root, &["add", "."]);
        run_git(&root, &["commit", "-q", "-m", "c1"]);
        (tmp, root)
    }

    #[test]
    fn prefers_main() {
        let (_t, root) = init_repo();
        assert_eq!(default_base(&root).as_deref(), Some("main"));
    }

    #[test]
    fn falls_back_to_master() {
        let (_t, root) = init_repo();
        run_git(&root, &["branch", "-m", "main", "master"]);
        assert_eq!(default_base(&root).as_deref(), Some("master"));
    }

    #[test]
    fn ignores_checked_out_branch() {
        // The whole point: the default stays `main` even with a feature branch
        // checked out — the focused-column-HEAD trap the POLA fix avoids.
        let (_t, root) = init_repo();
        run_git(&root, &["checkout", "-q", "-b", "feature"]);
        assert_eq!(default_base(&root).as_deref(), Some("main"));
    }

    #[test]
    fn none_outside_repo() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(default_base(tmp.path()), None);
    }
}
