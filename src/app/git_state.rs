//! MVU Phase 5: git worker-result application, extracted from the event
//! loop (`mod.rs`) to buy anti-monolith headroom and decouple the git
//! result path ahead of the GitState reunion (PR 1/2). `impl App` in a
//! child module — reads App's private `self.state` via the
//! descendant-module rule, same pattern as `effect` / `actions`.

use crate::ui::pager;

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
    /// commit-discussion workflow.
    pub fn open_git_show_pager(&mut self, sha: &str) {
        match crate::git::diff::show(&self.state.listing.dir, sha) {
            Ok(out) if out.status.success() && !out.stdout.is_empty() => {
                let title = format!("git show {sha}");
                self.view.pager = Some(pager::PagerView::new_ansi(title, &out.stdout));
            }
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                let msg = stderr.lines().next().unwrap_or("no output").trim();
                self.state.flash_error(format!("git show: {msg}"));
            }
            Err(e) => self.state.flash_error(format!("git show: {e}")),
        }
    }

    /// g d / g D — run `git diff` on selection and show in pager.
    ///
    /// `gd` (cached=false) also surfaces *untracked* files in the
    /// selection — without this, the cursor sitting on a `?`/`~`-flagged
    /// new file gives empty diff output and looks broken. We synthesize
    /// an "added" diff per untracked file via `git diff --no-index
    /// /dev/null <file>`, which exits 1 but still produces the diff bytes
    /// we want to render.
    pub fn open_git_diff(&mut self, cached: bool) {
        let paths = self.state.selection_paths();
        if paths.is_empty() {
            return;
        }
        let cwd = &self.state.listing.dir;
        let path_strings: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();

        // `gd` shows diff-vs-HEAD (staged + unstaged) so it matches the
        // `~` marker semantics — `~` flags anything different from HEAD,
        // and a user pressing `gd` to see "what's the change" expects
        // the same scope. Pre-1.41.7 ran bare `git diff` which only
        // showed unstaged work, so `git add` followed by `gd` produced
        // a confusing "no unstaged changes" flash on a row that was
        // visibly marked dirty. `gD` (`--cached`) keeps the
        // staged-only "what would commit" view.
        let modified_out = match crate::git::diff::working(cwd, &path_strings, cached) {
            Ok(o) => o,
            Err(e) => {
                self.state.flash_error(format!("git diff: {e}"));
                return;
            }
        };

        let mut combined = modified_out;
        if !cached {
            combined.extend(crate::git::diff::untracked_bytes(cwd, &path_strings));
        }

        if combined.is_empty() {
            let label = if cached { "staged" } else { "uncommitted" };
            self.state.flash_info(format!("no {label} changes"));
            return;
        }
        let label = if cached {
            "git diff --cached"
        } else {
            "git diff HEAD (+ new)"
        };
        self.view.pager = Some(pager::PagerView::new_ansi(label, &combined));
    }

    /// g b — `git blame` on the cursor file. Selection is ignored
    /// (blame on multiple files / a directory is meaningless).
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
        let path_str = path.display().to_string();
        match crate::git::diff::blame(&self.state.listing.dir, &path_str) {
            Ok(out) if out.status.success() && !out.stdout.is_empty() => {
                let title = format!("git blame {}", row.display);
                self.view.pager = Some(pager::PagerView::new_ansi(title, &out.stdout));
            }
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                let msg = stderr.lines().next().unwrap_or("no output").trim();
                self.state.flash_error(format!("git blame: {msg}"));
            }
            Err(e) => self.state.flash_error(format!("git blame: {e}")),
        }
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
