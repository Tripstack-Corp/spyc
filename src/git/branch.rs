//! Default-base detection — the repo's integration branch (`main`/`master`/…),
//! resolved *without regard to what any worktree currently has checked out*.
//!
//! This is what lets `create_worktree` base a new worktree off PROJECT_HOME's
//! default branch instead of the focused column's HEAD (the POLA fix in
//! `docs/archive/WORKTREE_MCP_PLAN.md` §3/§7). The richer branch-relationship queries
//! (merge-base, ahead/behind, merged-ness) land with `list_worktrees`, their
//! first consumer, and [`fork_point`] is where a branch left its base, which a
//! diff of the branch's own changes counts from.
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

/// How a commit relates to the repo's integration base.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BranchStatus {
    /// Commits reachable from the tip but not the base (`git rev-list --count base..tip`).
    pub ahead: usize,
    /// Commits reachable from the base but not the tip (`git rev-list --count tip..base`).
    pub behind: usize,
    /// `true` when the tip is fully contained in the base (`ahead == 0`): removing
    /// the worktree/branch loses no unmerged commits — the "is this safe to clean
    /// up?" signal `list_worktrees` surfaces and `remove_worktree`'s `delete_branch:
    /// auto` will key off.
    pub merged: bool,
}

/// Relationship of `tip` (any rev — a branch name, short hash, …) to `base`
/// (another rev) within the repo at `repo_root`: ahead/behind commit counts and
/// merged-ness, like `git rev-list --left-right --count base...tip`. `None` if
/// the repo or either rev can't be resolved (e.g. an empty/unborn tip, or a
/// bare entry with no commit).
///
/// Pure facade, in-process gix — same contract as [`default_base`].
pub fn branch_status(repo_root: &Path, tip: &str, base: &str) -> Option<BranchStatus> {
    branch_status_in(&gix::open(repo_root).ok()?, tip, base)
}

fn branch_status_in(repo: &gix::Repository, tip: &str, base: &str) -> Option<BranchStatus> {
    use gix::bstr::BStr;
    let tip_id = repo.rev_parse_single(BStr::new(tip)).ok()?.detach();
    let base_id = repo.rev_parse_single(BStr::new(base)).ok()?.detach();
    if tip_id == base_id {
        return Some(BranchStatus {
            ahead: 0,
            behind: 0,
            merged: true,
        });
    }
    let ahead = count_reachable(repo, tip_id, base_id)?;
    let behind = count_reachable(repo, base_id, tip_id)?;
    Some(BranchStatus {
        ahead,
        behind,
        merged: ahead == 0,
    })
}

/// Count commits reachable from `tip` but not from `hidden` (`git rev-list
/// --count hidden..tip`).
fn count_reachable(
    repo: &gix::Repository,
    tip: gix::ObjectId,
    hidden: gix::ObjectId,
) -> Option<usize> {
    Some(
        repo.rev_walk([tip])
            .with_hidden([hidden])
            .all()
            .ok()?
            .filter_map(Result::ok)
            .count(),
    )
}

/// Where `HEAD` left a base: the commit a branch's own changes are counted
/// from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForkPoint {
    /// The base as it was named, or the repo's [`default_base`].
    pub base: String,
    /// The merge-base of `HEAD` and the base.
    pub commit: gix::ObjectId,
}

/// Where `HEAD` left `base`, the repo's [`default_base`] when `None`: their
/// merge-base. A base naming a local branch that has an `origin/<name>`
/// counterpart forks at the later of `HEAD`'s merge-bases with the two: a
/// branch rebased onto `origin/main` while local `main` lags would otherwise
/// claim the commits local `main` hasn't pulled. Any other base is the rev it
/// names. Each error names its cause: an unborn `HEAD`, a base that names no
/// commit, histories with nothing in common.
pub fn fork_point(repo: &gix::Repository, base: Option<&str>) -> anyhow::Result<ForkPoint> {
    use anyhow::anyhow;
    let head = repo
        .head_id()
        .map_err(|e| anyhow!("HEAD has no commit yet: {}", super::error_chain(&e)))?
        .detach();
    let base = match base {
        Some(base) => base.to_string(),
        None => default_base_in(repo).ok_or_else(|| anyhow!("the repository has no branch"))?,
    };
    let mut forks = Vec::new();
    for tip in base_tips(repo, &base)? {
        match repo.merge_base(head, tip) {
            Ok(fork) => forks.push(fork.detach()),
            Err(gix::repository::merge_base::Error::NotFound { .. }) => {}
            Err(e) => {
                return Err(anyhow!(
                    "finding where HEAD left `{base}`: {}",
                    super::error_chain(&e)
                ));
            }
        }
    }
    let commit = forks
        .into_iter()
        .reduce(|a, b| later(repo, a, b))
        .ok_or_else(|| anyhow!("HEAD and `{base}` share no history"))?;
    Ok(ForkPoint { base, commit })
}

/// The commits `base` names: a local branch and its `origin/<name>`
/// counterpart when it has one, else the rev as written. A name only `origin`
/// has, as `default_base` can read from `origin/HEAD`, is that remote branch.
fn base_tips(repo: &gix::Repository, base: &str) -> anyhow::Result<Vec<gix::ObjectId>> {
    let branch = |prefix: &str| {
        let mut reference = repo.find_reference(&format!("{prefix}{base}")).ok()?;
        Some(reference.peel_to_commit().ok()?.id)
    };
    let remote = branch("refs/remotes/origin/");
    if let Some(local) = branch("refs/heads/") {
        return Ok(std::iter::once(local).chain(remote).collect());
    }
    let named = repo
        .rev_parse_single(gix::bstr::BStr::new(base))
        .ok()
        .and_then(|id| Some(id.object().ok()?.peel_to_commit().ok()?.id));
    named
        .or(remote)
        .map(|tip| vec![tip])
        .ok_or_else(|| anyhow::anyhow!("base `{base}` names no commit"))
}

/// The later of two commits: the one the other is an ancestor of, else the
/// more recently committed.
fn later(repo: &gix::Repository, a: gix::ObjectId, b: gix::ObjectId) -> gix::ObjectId {
    match repo.merge_base(a, b).map(gix::Id::detach) {
        Ok(shared) if shared == a => b,
        Ok(shared) if shared == b => a,
        _ => {
            let time = |id| {
                repo.find_commit(id)
                    .ok()
                    .and_then(|c| c.time().ok())
                    .map(|t| t.seconds)
            };
            if time(b) > time(a) { b } else { a }
        }
    }
}

/// Delete the local branch ref `refs/heads/<branch>` in the repo at
/// `repo_root`. Unconditional at the gix level — safe-remove only calls this
/// once it has confirmed the branch is merged (`delete_branch: auto`), so the
/// "don't lose unmerged commits" policy lives in the caller, not here.
///
/// Pure facade, in-process gix — same contract as [`default_base`].
pub fn delete(repo_root: &Path, branch: &str) -> std::io::Result<()> {
    let repo = gix::open(repo_root).map_err(std::io::Error::other)?;
    repo.find_reference(&format!("refs/heads/{branch}"))
        .map_err(std::io::Error::other)?
        .delete()
        .map_err(std::io::Error::other)
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

    #[test]
    fn branch_status_counts_ahead_behind() {
        let (_t, root) = init_repo(); // main @ c1
        // feature branches at c1 and adds two commits → 2 ahead, 0 behind.
        run_git(&root, &["checkout", "-q", "-b", "feature"]);
        for (f, c) in [("b.txt", "c2"), ("c.txt", "c3")] {
            std::fs::write(root.join(f), "x\n").unwrap();
            run_git(&root, &["add", "."]);
            run_git(&root, &["commit", "-q", "-m", c]);
        }
        let s = branch_status(&root, "feature", "main").unwrap();
        assert_eq!((s.ahead, s.behind, s.merged), (2, 0, false));

        // main advances by one commit → feature is now also 1 behind.
        run_git(&root, &["checkout", "-q", "main"]);
        std::fs::write(root.join("d.txt"), "d\n").unwrap();
        run_git(&root, &["add", "."]);
        run_git(&root, &["commit", "-q", "-m", "c4"]);
        let s = branch_status(&root, "feature", "main").unwrap();
        assert_eq!((s.ahead, s.behind, s.merged), (2, 1, false));
    }

    #[test]
    fn branch_status_merged_when_contained() {
        let (_t, root) = init_repo();
        // A ref pointing at the same commit as the base is fully merged.
        run_git(&root, &["branch", "stale", "main"]);
        let s = branch_status(&root, "stale", "main").unwrap();
        assert_eq!((s.ahead, s.behind, s.merged), (0, 0, true));
    }

    #[test]
    fn branch_status_none_for_unresolvable_rev() {
        let (_t, root) = init_repo();
        assert!(branch_status(&root, "does-not-exist", "main").is_none());
        assert!(branch_status(&root, "", "main").is_none());
    }

    #[test]
    fn delete_removes_the_branch_ref() {
        let (_t, root) = init_repo();
        run_git(&root, &["branch", "doomed", "main"]);
        let exists = |b: &str| {
            gix::open(&root)
                .unwrap()
                .find_reference(&format!("refs/heads/{b}"))
                .is_ok()
        };
        assert!(exists("doomed"), "precondition: branch present");
        delete(&root, "doomed").expect("delete the branch");
        assert!(!exists("doomed"), "branch ref removed");
        // Deleting a missing branch errors (no ref to find).
        assert!(delete(&root, "doomed").is_err());
    }
}
