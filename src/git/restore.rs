//! In-process `git restore` for a single deleted path — gix only.
//!
//! Brings a git-tracked file that's been removed from the worktree back to
//! disk, the content side of `git restore <file>` / `git checkout -- <file>`.
//! Production status/diff/blame are all gix (no subprocess; the
//! `no_subprocess_git_in_production` guard enforces it), and so is this: the
//! blob is read from the index (preferred, matching git's default source) or
//! HEAD and written straight to the worktree.
//!
//! Scope is deliberately additive — restore only ever *creates* a missing file
//! and refuses if the path already exists, so it can never clobber a working
//! copy. (Discarding local modifications — the destructive half of
//! `git restore` — is intentionally out of scope.)

use std::path::Path;

/// Restore `rela_path` (repo-relative, forward slashes) into the worktree at
/// `repo_root`, reading its blob from the index (then HEAD as a fallback for a
/// fully-staged deletion). Refuses if the file already exists. Returns the
/// number of bytes written on success, or a human-facing error string.
pub fn restore_to_worktree(repo_root: &Path, rela_path: &str) -> Result<usize, String> {
    let repo = gix::open(repo_root).map_err(|e| format!("open repo: {e}"))?;

    let dest = repo_root.join(rela_path);
    if dest.exists() {
        return Err(format!("{rela_path} already exists"));
    }

    // Prefer the index (git's default restore source), then HEAD.
    let (oid, executable) = index_blob(&repo, rela_path)
        .or_else(|| head_blob(&repo, rela_path))
        .ok_or_else(|| format!("{rela_path} is not tracked in the index or HEAD"))?;

    let data = repo
        .find_object(oid)
        .map_err(|e| format!("read blob: {e}"))?
        .detach()
        .data;

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create dir: {e}"))?;
    }
    std::fs::write(&dest, &data).map_err(|e| format!("write {rela_path}: {e}"))?;

    #[cfg(unix)]
    if executable {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(&dest) {
            let mut perms = meta.permissions();
            perms.set_mode(perms.mode() | 0o111);
            let _ = std::fs::set_permissions(&dest, perms);
        }
    }
    let _ = executable; // only consulted on unix

    Ok(data.len())
}

/// The blob id + executable bit for `rela_path` in the index, if present.
fn index_blob(repo: &gix::Repository, rela_path: &str) -> Option<(gix::ObjectId, bool)> {
    use gix::bstr::BStr;
    let index = repo.index().ok()?;
    let entry = index.entry_by_path(BStr::new(rela_path.as_bytes()))?;
    let executable = entry.mode == gix::index::entry::Mode::FILE_EXECUTABLE;
    Some((entry.id, executable))
}

/// The blob id + executable bit for `rela_path` in the HEAD tree, if present.
fn head_blob(repo: &gix::Repository, rela_path: &str) -> Option<(gix::ObjectId, bool)> {
    let tree = repo.head_tree().ok()?;
    let entry = tree.lookup_entry_by_path(Path::new(rela_path)).ok()??;
    let executable = matches!(
        entry.mode().kind(),
        gix::object::tree::EntryKind::BlobExecutable
    );
    Some((entry.object_id(), executable))
}

#[cfg(test)]
mod tests {
    use super::restore_to_worktree;
    use crate::git::test_support::run_git;
    use std::path::PathBuf;

    fn repo_with_commit() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap_or_else(|_| tmp.path().to_path_buf());
        run_git(&root, &["init", "-q", "--initial-branch=main"]);
        std::fs::write(root.join("keep.txt"), "hello\nworld\n").unwrap();
        run_git(&root, &["add", "keep.txt"]);
        run_git(&root, &["commit", "-q", "-m", "c1"]);
        (tmp, root)
    }

    #[test]
    fn restores_an_unstaged_deletion_from_the_index() {
        let (_t, root) = repo_with_commit();
        std::fs::remove_file(root.join("keep.txt")).unwrap();
        assert!(!root.join("keep.txt").exists());

        let n = restore_to_worktree(&root, "keep.txt").expect("restore");
        assert_eq!(n, "hello\nworld\n".len());
        assert_eq!(
            std::fs::read_to_string(root.join("keep.txt")).unwrap(),
            "hello\nworld\n",
            "content comes back byte-for-byte"
        );
    }

    #[test]
    fn restores_a_staged_deletion_from_head() {
        let (_t, root) = repo_with_commit();
        // `git rm` removes from BOTH the index and the worktree → the blob is
        // no longer in the index, so restore falls back to HEAD.
        run_git(&root, &["rm", "-q", "keep.txt"]);
        assert!(!root.join("keep.txt").exists());

        restore_to_worktree(&root, "keep.txt").expect("restore from HEAD");
        assert_eq!(
            std::fs::read_to_string(root.join("keep.txt")).unwrap(),
            "hello\nworld\n"
        );
    }

    #[test]
    fn refuses_to_clobber_an_existing_file() {
        let (_t, root) = repo_with_commit();
        // keep.txt still on disk → restore must refuse rather than overwrite.
        let err = restore_to_worktree(&root, "keep.txt").unwrap_err();
        assert!(err.contains("already exists"), "got: {err}");
    }

    #[test]
    fn errors_on_an_untracked_path() {
        let (_t, root) = repo_with_commit();
        let err = restore_to_worktree(&root, "never-existed.txt").unwrap_err();
        assert!(err.contains("not tracked"), "got: {err}");
    }
}
