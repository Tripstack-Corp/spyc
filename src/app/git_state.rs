//! Git worker-result application (`impl App`), extracted from the event
//! loop so the git result path is decoupled from `mod.rs`. `impl App` in a
//! child module — reads App's private `self.state` via the
//! descendant-module rule, same pattern as `effect` / `actions`.

use std::path::Path;

use crate::ui::pager;

use super::git_view_session::GitViewKind;
use super::{App, state};

impl App {
    /// Drain the Model's git-request outbox onto the Runtime's worker
    /// channel. The Model records cache-miss requests in
    /// `state.pending_git_requests` (it owns no `Sender`); this sends each
    /// over the Runtime-owned `git_worker_tx`. Called once per loop
    /// iteration just before `recv`, and once after the `App::new`
    /// bootstrap request, so a queued request reaches the worker before the
    /// loop next blocks. With no worker wired (the test harness) the outbox
    /// stays empty because `git_worker_available` gates the push, so this is
    /// a no-op; the defensive `clear` only guards the can't-happen case of a
    /// queued request with no sender.
    pub(super) fn flush_git_requests(&mut self) {
        let Some(tx) = self.runtime.git_worker_tx.as_ref() else {
            self.state.pending_git_requests.clear();
            return;
        };
        for req in self.state.pending_git_requests.drain(..) {
            let _ = tx.send(req);
        }
    }

    /// Apply a git worker result to domain state, returning whether the
    /// displayed git info/files changed (so the loop can redraw). Honors
    /// the generation + repo-root drop gates. Moved verbatim from the run
    /// loop (MVU Phase 5 PR 0).
    pub(super) fn apply_git_worker_result(&mut self, result: state::GitWorkerResult) -> bool {
        // Generation gate: the user has navigated past this request.
        if result.generation != self.state.git_generation {
            return false;
        }
        // Relevance gate: even at the same generation, the result is
        // for a specific repo. If the repo root no longer matches the
        // current state, discard. (Unusual — generation bumps cover
        // most of this.)
        if self.state.current_repo_root.as_deref() != Some(result.repo_root.as_path()) {
            return false;
        }
        let Some(entries) = result.entries else {
            // The status walk failed (not in a repo by the time the
            // worker ran, etc.). Leave existing state.
            return false;
        };
        let (Some(index_mtime), Some(head_mtime)) = (result.index_mtime, result.head_mtime) else {
            return false;
        };
        self.state.git_status_cache = Some(state::GitStatusCache {
            repo_root: result.repo_root.clone(),
            index_mtime,
            head_mtime,
            entries,
        });
        // Seed the 1 Hz poll cache too — without this the next safety
        // poll would observe a None cache and re-fire the status walk.
        self.state.git_poll_cache = Some((index_mtime, head_mtime));
        // Re-filter against the current listing dir's prefix and refresh
        // the display string. `compute_git_info_fast` reads the cache
        // we just stored for its dirty flag.
        let listing_dir = self.state.listing.dir.clone();
        let new_files = {
            let cache = self.state.git_status_cache.as_ref().unwrap();
            let prefix = listing_dir
                .strip_prefix(&cache.repo_root)
                .unwrap_or(std::path::Path::new(""))
                .to_string_lossy()
                .into_owned();
            crate::git::status::map_to_listing(&cache.entries, &prefix)
        };
        let new_info = self.state.compute_git_info_fast();
        let changed = new_files != self.state.git.files || new_info != self.state.git.info;
        self.state.git.set(new_info, new_files);
        if changed {
            self.state.rebuild_rows();
        }
        changed
    }

    /// `git show <sha>` into the pager. Uppercase action for a
    /// matched git SHA — the value of the picker for a
    /// commit-discussion workflow. PR 8b: builds the structured model
    /// off-thread (gix) and renders it in-house via the git-view session.
    pub fn open_git_show_pager(&mut self, sha: &str) {
        let Some(root) = self.state.current_repo_root.clone() else {
            self.state.flash_error("git show: not a git repository");
            return;
        };
        self.open_git_view(
            GitViewKind::Show {
                repo_root: root,
                rev: sha.to_string(),
            },
            format!("git show {sha}"),
        );
    }

    /// g d / g D — run `git diff` on selection and show in pager.
    ///
    /// `gd` (cached=false) shows diff-vs-HEAD (staged + unstaged) so it
    /// matches the `~` marker semantics — `~` flags anything different from
    /// HEAD. `gD` (`--cached`) keeps the staged-only "what would commit"
    /// view. PR 8b: builds the structured model off-thread (gix) and renders
    /// it in-house; the gix diff already surfaces untracked files, so the
    /// old `untracked_bytes` synthesis is gone.
    pub fn open_git_diff(&mut self, cached: bool) {
        let paths = self.state.selection_paths();
        if paths.is_empty() {
            return;
        }
        let Some(root) = self.state.current_repo_root.clone() else {
            self.state.flash_error("git diff: not a git repository");
            return;
        };
        // Relativize each selected absolute path to a repo-relative,
        // forward-slash path (mirroring the status path-mapping's
        // `strip_prefix(&repo_root)`). Paths not under the repo root are
        // skipped — they can't appear in this repo's diff.
        let rel: Vec<String> = paths
            .iter()
            .filter_map(|p| repo_relative(p, &root))
            .collect();
        let title = if cached {
            "git diff --cached"
        } else {
            "git diff HEAD (+ new)"
        };
        self.open_git_view(
            GitViewKind::Diff {
                repo_root: root,
                paths: rel,
                cached,
            },
            title.to_string(),
        );
    }

    /// g b — `git blame` on the cursor file. Selection is ignored
    /// (blame on multiple files / a directory is meaningless). PR 8b:
    /// builds the blame model off-thread (gix) and renders it in-house.
    pub fn open_git_blame(&mut self) {
        let Some(row) = self.state.rows.get(self.state.cursor.index) else {
            self.state.flash_error("git blame: no cursor file");
            return;
        };
        let path = row.path.clone();
        if path.is_dir() {
            self.state.flash_error("git blame: cursor is a directory");
            return;
        }
        let Some(root) = self.state.current_repo_root.clone() else {
            self.state.flash_error("git blame: not a git repository");
            return;
        };
        let Some(rel) = repo_relative(&path, &root) else {
            self.state
                .flash_error("git blame: file is outside the repository");
            return;
        };
        let title = format!("git blame {}", row.display);
        self.open_git_view(
            GitViewKind::Blame {
                repo_root: root,
                path: rel,
            },
            title,
        );
    }

    /// W l — list worktrees in a pager; digit keys 1-9 select.
    pub fn worktree_list(&mut self) {
        match crate::git::worktree::list(&self.state.listing.dir) {
            Some(worktrees) => {
                self.state.pending_worktrees =
                    Some(worktrees.iter().map(|w| w.path.clone()).collect());
                let lines: Vec<String> = worktrees
                    .iter()
                    .enumerate()
                    .map(|(i, wt)| {
                        let current = if wt.path == self.state.listing.dir {
                            " ← current"
                        } else {
                            ""
                        };
                        format!(
                            "  [{}]  {:<30} {:>8}  {}{}",
                            i + 1,
                            wt.branch,
                            wt.head,
                            wt.path.display(),
                            current,
                        )
                    })
                    .collect();
                let view = pager::PagerView::new_plain(
                    "git worktrees — press 1-9 to switch, q to close",
                    lines,
                );
                self.set_pager(view);
            }
            None => self
                .state
                .flash_error("not in a git repository (or no worktrees)"),
        }
    }
}

/// Relativize an absolute `path` to a repo-relative, forward-slash path under
/// `repo_root` (the gix builders want repo-relative paths). Mirrors the status
/// path-mapping's `strip_prefix(&repo_root)`. Returns `None` when `path` is not
/// under `repo_root` (it can't appear in this repo's diff/blame) or when the
/// path is the repo root itself (empty relative path).
fn repo_relative(path: &Path, repo_root: &Path) -> Option<String> {
    let rel = path.strip_prefix(repo_root).ok()?;
    let s = rel.to_string_lossy();
    if s.is_empty() {
        return None;
    }
    // On the target platform separators are already '/', but normalize
    // defensively so the gix path spec matches regardless of host.
    Some(s.replace(std::path::MAIN_SEPARATOR, "/"))
}

#[cfg(test)]
mod tests {
    use super::repo_relative;
    use std::path::Path;

    #[test]
    fn repo_relative_strips_root_prefix() {
        let root = Path::new("/home/u/proj");
        assert_eq!(
            repo_relative(Path::new("/home/u/proj/src/main.rs"), root).as_deref(),
            Some("src/main.rs")
        );
    }

    #[test]
    fn repo_relative_top_level_file() {
        let root = Path::new("/home/u/proj");
        assert_eq!(
            repo_relative(Path::new("/home/u/proj/README.md"), root).as_deref(),
            Some("README.md")
        );
    }

    #[test]
    fn repo_relative_outside_root_is_none() {
        let root = Path::new("/home/u/proj");
        assert!(repo_relative(Path::new("/home/u/other/x"), root).is_none());
    }

    #[test]
    fn repo_relative_root_itself_is_none() {
        let root = Path::new("/home/u/proj");
        assert!(repo_relative(Path::new("/home/u/proj"), root).is_none());
    }
}
