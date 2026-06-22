//! Clean out a git worktree, preserving its untracked files in the graveyard.
//!
//! `worktree::remove` refuses a dirty worktree (like `git worktree remove`).
//! "Clean out" is the variant that doesn't choke on untracked junk: it archives
//! the untracked files into the graveyard under `<worktree-name>-<timestamp>`
//! (so they're recoverable via `gy` / `:undo`), then removes the worktree. It
//! still **refuses** a worktree with uncommitted changes to *tracked* files —
//! commit or stash those first; only untracked files are preserved (owner's
//! call: tracked work lives in git, untracked is the part teardown would lose).
//! A *wholly* untracked directory is archived in full: git collapses it to one
//! untracked entry, so any gitignored content nested inside it rides along —
//! harmless, since the directory is being removed regardless.
//!
//! App-layer, not `git::worktree` (which is pure git infra): this bridges
//! `git::status` + `git::worktree` + the `graveyard` (app state). It takes a
//! path and needs no `App`, so it's unit-testable on its own.

use std::path::Path;

use crate::fs::ops::copy_tree;
use crate::git::{status, worktree};
use crate::state::graveyard::Graveyard;

/// What a successful clean-out preserved.
#[derive(Debug)]
pub struct CleanReport {
    /// Number of untracked entries archived (0 when the worktree was clean).
    pub archived: usize,
    /// Graveyard label the untracked files were stored under, if any.
    pub label: Option<String>,
}

/// Archive a worktree's untracked files into the graveyard under
/// `<worktree-name>-<timestamp>`, then remove the worktree.
///
/// Errors — and changes nothing — when the worktree has uncommitted changes to
/// *tracked* files, or when it isn't a readable git worktree. Only untracked
/// files are preserved; tracked changes are git's job.
pub fn clean_worktree(path: &Path) -> std::io::Result<CleanReport> {
    let statuses = status::repo_status(path)
        .ok_or_else(|| std::io::Error::other("not a git worktree, or its status can't be read"))?;

    // Safety: never silently discard uncommitted work on TRACKED files. Bail
    // before touching anything so the worktree is left exactly as we found it.
    if statuses
        .iter()
        .any(|e| e.staged.is_some() || e.unstaged.is_some())
    {
        return Err(std::io::Error::other(
            "worktree has uncommitted changes to tracked files — commit or stash them first",
        ));
    }

    let untracked: Vec<&str> = statuses
        .iter()
        .filter(|e| e.untracked)
        .map(|e| e.rela_path.as_str())
        .collect();

    let label = if untracked.is_empty() {
        None
    } else {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("worktree");
        let stamp = crate::sysinfo::epoch_secs();
        let entry_name = format!("{name}-{stamp}");

        // Stage the untracked files into a temp tree mirroring their relative
        // paths — OUTSIDE the worktree, so the copy doesn't re-discover itself —
        // archive that tree under the worktree-name label, then drop the
        // staging copy. Copy (not move): if archiving fails the worktree is
        // still intact and nothing is lost.
        let staging =
            std::env::temp_dir().join(format!(".spyc-wt-clean-{}-{stamp}", std::process::id()));
        let _ = std::fs::remove_dir_all(&staging); // clear any stale leftover
        std::fs::create_dir_all(&staging)?;
        let archive = (|| -> std::io::Result<()> {
            for rel in &untracked {
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

        // Untracked files are safely in the graveyard now; delete them from the
        // worktree so it's clean and `worktree::remove` (which refuses any dirt)
        // proceeds.
        for rel in &untracked {
            let p = path.join(rel);
            let _ = if p.is_dir() {
                std::fs::remove_dir_all(&p)
            } else {
                std::fs::remove_file(&p)
            };
        }
        Some(entry_name)
    };

    // Tracked-clean (verified above) + untracked now gone → safe remove, which
    // also tears down the worktree's admin dir.
    worktree::remove(path)?;
    Ok(CleanReport {
        archived: untracked.len(),
        label,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_git(dir: &Path, args: &[&str]) {
        let ok = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@x")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@x")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .status()
            .expect("spawn git")
            .success();
        assert!(ok, "git {args:?} failed");
    }

    fn make_repo(root: &Path) {
        std::fs::create_dir_all(root).unwrap();
        run_git(root, &["init", "-q", "--initial-branch=main"]);
        std::fs::write(root.join("f.txt"), "v1\n").unwrap();
        run_git(root, &["add", "f.txt"]);
        run_git(root, &["commit", "-q", "-m", "v1"]);
    }

    /// Untracked files are archived to the graveyard under `<worktree>-<ts>`,
    /// then the worktree is removed.
    #[test]
    fn clean_archives_untracked_then_removes() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let repo = std::fs::canonicalize(tmp.path()).unwrap().join("repo");
            make_repo(&repo);
            let wt = worktree::add(&repo, "wt-clean").unwrap();
            std::fs::write(wt.join("scratch.log"), "junk\n").unwrap(); // untracked

            let report = clean_worktree(&wt).expect("clean succeeds");
            assert_eq!(report.archived, 1, "one untracked entry archived");
            let label = report.label.expect("a graveyard label");
            assert!(
                label.starts_with("wt-clean-"),
                "labelled by worktree + timestamp: {label}"
            );
            assert!(!wt.exists(), "worktree removed");

            // The untracked file is recoverable from the graveyard.
            let g = Graveyard::load();
            assert!(
                g.entries.iter().any(|e| e.filename == label),
                "graveyard holds the archived entry"
            );
        });
    }

    /// Uncommitted changes to a TRACKED file → refuse, leaving the worktree
    /// (and the changes) intact. Only untracked files are ever preserved.
    #[test]
    fn clean_refuses_dirty_tracked_and_leaves_worktree() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let repo = std::fs::canonicalize(tmp.path()).unwrap().join("repo");
            make_repo(&repo);
            let wt = worktree::add(&repo, "wt-dirty").unwrap();
            std::fs::write(wt.join("f.txt"), "v2-uncommitted\n").unwrap(); // tracked edit

            let err = clean_worktree(&wt).unwrap_err();
            assert!(
                err.to_string().contains("tracked"),
                "refusal cites tracked changes: {err}"
            );
            assert!(wt.exists(), "worktree left intact on refusal");
            assert_eq!(
                std::fs::read_to_string(wt.join("f.txt")).unwrap(),
                "v2-uncommitted\n",
                "the tracked edit is preserved"
            );
        });
    }
}
