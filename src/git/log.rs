//! Recent-commit log — a thin gix rev-walk over HEAD, reusing the diff/show
//! view's [`CommitMeta`]. Backs the `git_log` MCP read tool.
//!
//! Pure facade, in-process gix: paths in, owned `Send` data out, no `App`
//! dependency — same contract as the rest of `src/git/`.

use std::path::Path;

use crate::git::model::CommitMeta;

/// The most recent `limit` commits reachable from HEAD, newest first. Empty when
/// the repo can't be opened, HEAD is unborn, or the walk fails.
pub fn recent(repo_root: &Path, limit: usize) -> Vec<CommitMeta> {
    let Ok(repo) = gix::open(repo_root) else {
        return Vec::new();
    };
    let Ok(head) = repo.head_id() else {
        return Vec::new();
    };
    let Ok(walk) = repo.rev_walk([head.detach()]).all() else {
        return Vec::new();
    };
    walk.filter_map(Result::ok)
        .take(limit)
        .filter_map(|info| repo.find_commit(info.id).ok())
        .filter_map(|c| crate::git::diff_model::commit_meta(&c))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::test_support::run_git;

    fn init_repo() -> (tempfile::TempDir, std::path::PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("repo");
        std::fs::create_dir(&root).unwrap();
        run_git(&root, &["init", "-q", "--initial-branch=main"]);
        (tmp, root)
    }

    fn commit(root: &Path, file: &str, msg: &str) {
        std::fs::write(root.join(file), "x\n").unwrap();
        run_git(root, &["add", "."]);
        run_git(root, &["commit", "-q", "-m", msg]);
    }

    #[test]
    fn recent_returns_commits_newest_first() {
        let (_t, root) = init_repo();
        commit(&root, "a.txt", "first");
        commit(&root, "b.txt", "second");
        commit(&root, "c.txt", "third");

        let log = recent(&root, 10);
        assert_eq!(log.len(), 3);
        assert_eq!(log[0].subject, "third", "newest first");
        assert_eq!(log[2].subject, "first");
        assert_eq!(log[0].short_id.len(), 7, "short id is 7 hex chars");
        assert_eq!(log[0].author, "Ada", "test_support author");
    }

    #[test]
    fn recent_respects_the_limit() {
        let (_t, root) = init_repo();
        for i in 0..5 {
            commit(&root, &format!("f{i}.txt"), &format!("c{i}"));
        }
        assert_eq!(recent(&root, 2).len(), 2, "capped at the limit");
    }

    #[test]
    fn recent_empty_for_non_repo() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(recent(tmp.path(), 10).is_empty());
    }
}
