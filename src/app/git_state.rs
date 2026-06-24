//! Git worker-result application (`impl App`), extracted from the event
//! loop so the git result path is decoupled from `mod.rs`. `impl App` in a
//! child module — reads App's private `self.state` via the
//! descendant-module rule, same pattern as `effect` / `actions`.

use std::path::Path;

use crate::ui::pager;

use super::git_view_session::GitViewKind;
use super::{App, Effect, state};

impl App {
    /// `g r` — restore the cursor's deleted (struck-through) file from the
    /// index/HEAD back into the worktree. Only acts on a ghost row; the actual
    /// blob read + write runs off-thread via `Effect::FileOp` (no blocking IO
    /// on the input path), and the resulting refresh clears the ghost.
    pub fn git_restore_cursor(&mut self) -> Vec<Effect> {
        let Some(row) = self.state.cur().rows.get(self.state.cur().cursor.index) else {
            return Vec::new();
        };
        if !row.deleted {
            self.state
                .flash_error("g r restores a deleted file — cursor isn't on one");
            return Vec::new();
        }
        let path = row.path.clone();
        let Some(repo_root) = self.state.cur().git_cache.current_repo_root.clone() else {
            self.state.flash_error("restore: not in a git repository");
            return Vec::new();
        };
        // Repo-relative, forward slashes (matches how git keys paths).
        let Some(rela_path) = repo_relative(&path, &repo_root) else {
            self.state.flash_error("restore: file is outside the repo");
            return Vec::new();
        };
        vec![Effect::FileOp(super::file_ops::FileOp::GitRestore {
            repo_root,
            rela_path,
        })]
    }

    /// Drain the Model's git-request outbox onto the Runtime's worker
    /// channel. The Model records cache-miss requests in
    /// `state.git_cache.pending_git_requests` (it owns no `Sender`); this sends each
    /// over the Runtime-owned `git_worker_tx`. Called once per loop
    /// iteration just before `recv`, and once after the `App::new`
    /// bootstrap request, so a queued request reaches the worker before the
    /// loop next blocks. With no worker wired (the test harness) the outbox
    /// stays empty because `git_worker_available` gates the push, so this is
    /// a no-op; the defensive `clear` only guards the can't-happen case of a
    /// queued request with no sender.
    pub(super) fn flush_git_requests(&mut self) {
        let Some(tx) = self.runtime.git_worker_tx.as_ref() else {
            // No worker (test harness): the outboxes stay empty
            // (`git_worker_available` gates the push), so just defensively clear.
            self.state.left.git_cache.pending_git_requests.clear();
            if let Some(r) = self.state.right.as_mut() {
                r.git_cache.pending_git_requests.clear();
            }
            return;
        };
        // Both columns share the one worker channel; each request carries its
        // `side` so the result routes back to the right column.
        for req in self.state.left.git_cache.pending_git_requests.drain(..) {
            let _ = tx.send(req);
        }
        if let Some(r) = self.state.right.as_mut() {
            for req in r.git_cache.pending_git_requests.drain(..) {
                let _ = tx.send(req);
            }
        }
    }

    /// Apply a git worker result to domain state, returning whether the
    /// displayed git info/files changed (so the loop can redraw). Honors
    /// the generation + repo-root drop gates. Moved verbatim from the run
    /// loop (MVU Phase 5 PR 0).
    pub(super) fn apply_git_worker_result(&mut self, result: state::GitWorkerResult) -> bool {
        // Route to the column that requested this (the worker echoed its side).
        let side = result.side;
        // Generation gate (per-column): that column navigated past this request.
        if result.generation != self.state.col(side).git_cache.git_generation {
            return false;
        }
        // Relevance gate: even at the same generation, the result is for a
        // specific repo. If the column's repo root no longer matches, discard.
        if self.state.col(side).git_cache.current_repo_root.as_deref()
            != Some(result.repo_root.as_path())
        {
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
        self.state.col_mut(side).git_cache.git_status_cache = Some(state::GitStatusCache {
            repo_root: result.repo_root.clone(),
            index_mtime,
            head_mtime,
            entries,
        });
        // Seed the 1 Hz poll cache too — without this the next safety
        // poll would observe a None cache and re-fire the status walk.
        self.state.col_mut(side).git_cache.git_poll_cache = Some((index_mtime, head_mtime));
        // Re-filter against that column's listing dir prefix and refresh its
        // display string. `compute_git_info_fast` reads the cache just stored.
        let listing_dir = self.state.col(side).listing.dir.clone();
        let new_files = {
            let cache = self
                .state
                .col(side)
                .git_cache
                .git_status_cache
                .as_ref()
                .expect("git_status_cache set just above");
            let prefix = listing_dir
                .strip_prefix(&cache.repo_root)
                .unwrap_or(std::path::Path::new(""))
                .to_string_lossy()
                .into_owned();
            crate::git::status::map_to_listing(&cache.entries, &prefix)
        };
        let new_info = self.state.compute_git_info_fast(side);
        let changed = new_files != self.state.col(side).git.files
            || new_info != self.state.col(side).git.info;
        self.state.col_mut(side).git.set(new_info, new_files);
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
        let Some(root) = self.state.cur().git_cache.current_repo_root.clone() else {
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
        let Some(root) = self.state.cur().git_cache.current_repo_root.clone() else {
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
        let Some(row) = self.state.cur().rows.get(self.state.cur().cursor.index) else {
            self.state.flash_error("git blame: no cursor file");
            return;
        };
        let path = row.path.clone();
        if path.is_dir() {
            self.state.flash_error("git blame: cursor is a directory");
            return;
        }
        let Some(root) = self.state.cur().git_cache.current_repo_root.clone() else {
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
        match crate::git::worktree::list(&self.state.cur().listing.dir) {
            Some(worktrees) => {
                let cur_dir = self.state.cur().listing.dir.clone();
                // Start the cursor on the worktree the focused column is in (so
                // the highlight reflects "where I am"), else the first row.
                let start = worktrees
                    .iter()
                    .position(|w| w.path == cur_dir)
                    .unwrap_or(0);
                self.state.pending_worktrees =
                    Some(worktrees.iter().map(|w| w.path.clone()).collect());
                let lines: Vec<String> = worktrees
                    .iter()
                    .enumerate()
                    .map(|(i, wt)| {
                        let current = if wt.path == cur_dir {
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
                let mut view = pager::PagerView::new_plain(
                    "git worktrees — j/k+Enter or 1-9 to switch, / to search, q to close",
                    lines,
                );
                // A highlighted picker row (j/k move it, Enter switches to it);
                // `/` search syncs the cursor to its match (see
                // `handle_pager_search_typing`). Mirrors the jump-history popup.
                view.picker_cursor = Some(start);
                view.no_history = true;
                view.show_line_numbers = false;
                view.wrap = false;
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

    #[test]
    fn git_restore_cursor_emits_restore_effect_for_a_ghost() {
        use crate::app::file_ops::FileOp;
        use crate::app::{App, Effect, RowData};
        use crate::fs::EntryKind;
        use std::path::PathBuf;

        let root = PathBuf::from("/repo");
        let mut app = App::test_app(root.clone());
        app.state.left.git_cache.current_repo_root = Some(root.clone());
        app.state.left.rows = vec![RowData {
            path: root.join("gone.txt"),
            display: "gone.txt".into(),
            kind: EntryKind::File,
            deleted: true,
        }];
        app.state.left.cursor.index = 0;

        match app.git_restore_cursor().as_slice() {
            [
                Effect::FileOp(FileOp::GitRestore {
                    repo_root,
                    rela_path,
                }),
            ] => {
                assert_eq!(repo_root, &root);
                assert_eq!(rela_path, "gone.txt");
            }
            _ => panic!("expected a single GitRestore effect"),
        }
    }

    #[test]
    fn git_restore_cursor_is_a_noop_on_a_live_row() {
        use crate::app::{App, RowData};
        use crate::fs::EntryKind;
        use std::path::PathBuf;

        let root = PathBuf::from("/repo");
        let mut app = App::test_app(root.clone());
        app.state.left.git_cache.current_repo_root = Some(root.clone());
        app.state.left.rows = vec![RowData {
            path: root.join("real.txt"),
            display: "real.txt".into(),
            kind: EntryKind::File,
            deleted: false,
        }];
        app.state.left.cursor.index = 0;
        assert!(
            app.git_restore_cursor().is_empty(),
            "a live (non-deleted) row produces no restore effect"
        );
    }
}
