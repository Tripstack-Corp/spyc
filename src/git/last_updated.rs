//! When a worktree was last updated: the newest modification time among its
//! files and directories that git doesn't ignore, and its own git dir's
//! `HEAD`, `index` and `logs/HEAD`.
//!
//! The files are the truth for work not yet committed: git's own records only
//! move on a commit, a checkout or an `add`. A directory's time moves when an
//! entry in it is created, renamed or deleted, which a deleted file leaves no
//! other trace of. Ignored paths (`target/`, caches) are skipped, so a build
//! is not an update. The git dir's files carry a commit or a checkout that
//! touched no tracked file.

use std::path::Path;
use std::time::SystemTime;

/// When `worktree` was last updated, or `None` when it has no directory to
/// read (a worktree whose checkout was moved or deleted).
pub fn last_updated(worktree: &Path) -> Option<SystemTime> {
    if !worktree.is_dir() {
        return None;
    }
    let files = ignore::WalkBuilder::new(worktree)
        .hidden(false)
        .parents(false)
        .require_git(false)
        .filter_entry(|entry| entry.file_name() != ".git")
        .build()
        .filter_map(Result::ok)
        .filter_map(|entry| modified(entry.path()));
    let git_dir = gix::open(worktree)
        .ok()
        .map(|repo| repo.git_dir().to_path_buf());
    let records = git_dir.into_iter().flat_map(|dir| {
        ["HEAD", "index", "logs/HEAD"]
            .into_iter()
            .filter_map(move |name| modified(&dir.join(name)))
    });
    files.chain(records).max()
}

/// `path`'s own modification time, not its symlink target's.
fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::symlink_metadata(path)
        .and_then(|m| m.modified())
        .ok()
}

#[cfg(test)]
mod tests {
    use super::last_updated;
    use std::path::Path;
    use std::time::{Duration, SystemTime};

    fn set_mtime(path: &Path, at: SystemTime) {
        std::fs::File::options()
            .write(true)
            .open(path)
            .or_else(|_| std::fs::File::open(path))
            .unwrap()
            .set_modified(at)
            .unwrap();
    }

    fn ago(secs: u64) -> SystemTime {
        SystemTime::now() - Duration::from_secs(secs)
    }

    /// A tree whose every file and directory, `.git` included, is `secs` old.
    fn aged_tree(secs: u64) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("target")).unwrap();
        std::fs::write(root.join(".gitignore"), "target/\n").unwrap();
        std::fs::write(root.join("src/lib.rs"), "").unwrap();
        std::fs::write(root.join("target/out"), "").unwrap();
        std::fs::write(root.join(".git/index"), "").unwrap();
        for path in [
            ".gitignore",
            "src/lib.rs",
            "target/out",
            ".git/index",
            "src",
            "target",
            ".git",
            ".",
        ] {
            set_mtime(&root.join(path), ago(secs));
        }
        tmp
    }

    fn age_of(root: &Path) -> u64 {
        SystemTime::now()
            .duration_since(last_updated(root).expect("a tree has a time"))
            .unwrap_or_default()
            .as_secs()
    }

    #[test]
    fn an_edited_file_dates_the_tree() {
        let tree = aged_tree(86_400);
        set_mtime(&tree.path().join("src/lib.rs"), ago(60));
        assert!(
            (59..=70).contains(&age_of(tree.path())),
            "{}",
            age_of(tree.path())
        );
    }

    #[test]
    fn a_deleted_file_dates_the_tree_through_its_directory() {
        let tree = aged_tree(86_400);
        std::fs::remove_file(tree.path().join("src/lib.rs")).unwrap();
        assert!(age_of(tree.path()) < 10);
    }

    #[test]
    fn ignored_build_output_does_not_date_the_tree() {
        let tree = aged_tree(86_400);
        std::fs::write(tree.path().join("target/out"), "rebuilt").unwrap();
        set_mtime(&tree.path().join("target"), SystemTime::now());
        assert!(age_of(tree.path()) >= 86_400);
    }

    #[test]
    fn a_tree_with_no_directory_has_no_time() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(last_updated(&tmp.path().join("gone")), None);
    }

    /// A commit touches no file in the tree, only the worktree's git dir.
    #[test]
    fn a_commit_dates_the_tree_through_its_git_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        let git = |args: &[&str]| crate::git::test_support::run_git(&repo, args);
        std::fs::create_dir_all(&repo).unwrap();
        git(&["init", "-q"]);
        std::fs::write(repo.join("f.txt"), "v1\n").unwrap();
        git(&["add", "f.txt"]);
        let old = ago(86_400);
        for path in [
            repo.join("f.txt"),
            repo.join(".git/index"),
            repo.join(".git/HEAD"),
            repo.clone(),
        ] {
            set_mtime(&path, old);
        }
        assert!(age_of(&repo) >= 86_400, "the tree starts a day old");
        git(&["commit", "-q", "-m", "c1"]);
        assert!(age_of(&repo) < 10);
    }
}
