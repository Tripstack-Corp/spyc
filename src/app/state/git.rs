//! `AppState` git display/cache state: status refresh, the cached-status
//! lookup, fast info/mtime probes, and repo-root resolution. Split from
//! `state` verbatim.
//!
//! Git state is **per-column** (`Commander.git` + `Commander.git_cache`): each
//! file-commander resolves its own repo/worktree, so `b` in a different
//! worktree shows its own markers and the two columns never collide on a single
//! generation. These methods take a `Side` and operate on `self.col(side)`; the
//! orchestrators (`refresh_git_state`) iterate `active_sides()`.

use std::path::Path;

use super::{AppState, GitStatusCache, GitWorkerRequest, Side};

use super::find_repo_root;

impl AppState {
    /// Re-poll git state for **every** active column and rebuild rows once if
    /// anything changed. The 1 Hz safety-net poll (see
    /// [`Self::refresh_git_state_for`] for the per-column mechanics + the mtime
    /// short-circuit). Returns `true` iff any column's git changed.
    pub fn refresh_git_state(&mut self) -> bool {
        // Refresh each active column independently (per-column git). Explicit
        // sides rather than iterating `active_sides()` so the loop body can take
        // `&mut self` without collecting the borrow first.
        let mut any = self.refresh_git_state_for(Side::Left);
        if self.right.is_some() {
            any |= self.refresh_git_state_for(Side::Right);
        }
        if any {
            self.rebuild_rows();
        }
        any
    }

    /// Force the next git poll to re-walk every active column whose working
    /// tree contains `path`. Called for a working-tree fs-event (an edit /
    /// `git restore` / external write — anything that is NOT a `.git/index`
    /// or `HEAD` change). The poll's mtime short-circuit
    /// ([`Self::refresh_git_state_for`]) can't see such a change (no
    /// `.git/index`/`HEAD` mtime moves), and the listing-refresh path only
    /// invalidates the **focused** column — so without this an UNFOCUSED
    /// column's `~`/` M` markers stay stale after an external edit until a git
    /// op or chdir. Sets the existing `pending_worktree_rewalk` escape hatch,
    /// which the next 1 Hz poll honors (re-walking once, off-thread). Flagging
    /// the focused column too is harmless — its listing refresh clears the flag
    /// on success. Returns whether any column was flagged.
    pub fn flag_worktree_rewalk_for_path(&mut self, path: &Path) -> bool {
        let mut flagged = false;
        for side in [Side::Left, Side::Right] {
            if side == Side::Right && self.right.is_none() {
                continue;
            }
            if path.starts_with(&self.col(side).listing.dir) {
                self.col_mut(side).git_cache.pending_worktree_rewalk = true;
                flagged = true;
            }
        }
        flagged
    }

    /// Re-poll one column's git state (`git.info` + `git.files`) and update only
    /// if it changed. Returns `true` iff that column was different. Does NOT
    /// rebuild rows — the [`Self::refresh_git_state`] orchestrator does that once
    /// across all columns.
    ///
    /// **Mtime short-circuit.** Before the gix status walk + branch read, stat
    /// `.git/index` + `.git/HEAD` and compare against this column's cache from
    /// the last successful call. When both match, the inputs to `git status` are
    /// bit-identical and we return false immediately — on a 100k-file tree the
    /// walk costs real CPU, so skipping it on the idle path drops background load
    /// to near zero. The cache is intentionally scoped to *this* poll; the
    /// event-driven `refresh_listing` path never consults it (working-tree edits
    /// move file mtimes but NOT `.git/index`/`HEAD`, so a hit there would miss
    /// the ` M filename` markers refresh exists to surface).
    pub(super) fn refresh_git_state_for(&mut self, side: Side) -> bool {
        let key = self.compute_git_mtime_key_fast(side);
        // A throttled working-tree change (refresh_listing deferred its
        // invalidation) forces a re-walk: the mtime key can't see an unstaged
        // edit, so honoring this flag is the only thing that converges a stale
        // ` M`/clean marker without waiting for a chdir.
        let force_rewalk =
            std::mem::take(&mut self.col_mut(side).git_cache.pending_worktree_rewalk);
        if !force_rewalk && key.is_some() && key == self.col(side).git_cache.git_poll_cache {
            return false;
        }
        // A re-walk is warranted (mtime moved, or `force_rewalk` — a working-tree
        // edit the mtime key can't see). `git_file_statuses_cached` hands it to
        // the background worker and returns the CURRENT (stale-but-same-repo)
        // markers rather than blanking the cache, so the dirty star + gutter hold
        // steady until `apply_git_worker_result` swaps in the fresh walk. Nulling
        // here is what made the status bar flicker under worktree churn.
        let listing_dir = self.col(side).listing.dir.clone();
        let new_git_files = self.git_file_statuses_cached(side, &listing_dir, force_rewalk);
        let new_git_info = self.compute_git_info_fast(side);
        // Stash on success so the next idle poll skips the subprocesses.
        self.col_mut(side).git_cache.git_poll_cache = key;
        if new_git_info == self.col(side).git.info && new_git_files == self.col(side).git.files {
            return false;
        }
        self.col_mut(side).git.set(new_git_info, new_git_files);
        true
    }

    /// Get the file-status map for `canonical` in `side`'s column, using that
    /// column's raw-status cache when valid. The cache hit path skips the
    /// `git status` subprocess and just re-parses the previously-captured
    /// porcelain against the new dir's prefix — the slow part of the spawn is
    /// the index walk, identical for every chdir within the same repo. On a
    /// ~110k-file monorepo this drops per-chdir cost from 200-500 ms to
    /// sub-millisecond.
    ///
    /// Caller must have already called [`Self::update_repo_root`] for `side` so
    /// its `current_repo_root` reflects the new dir.
    ///
    /// `force` requests a fresh walk even when the mtime cache would be reused —
    /// a working-tree edit moves no `.git/index`/`HEAD` mtime, so the poll and
    /// the listing refresh flag it to converge a stale ` M`/clean marker. When a
    /// walk is needed and the worker is live, it runs off-thread and this returns
    /// the CURRENT cache's markers (stale but same-repo), NOT an empty map, and
    /// does not clear the cache — [`Self::apply_git_worker_result`] swaps in the
    /// fresh entries when the walk lands.
    pub fn git_file_statuses_cached(
        &mut self,
        side: Side,
        canonical: &Path,
        force: bool,
    ) -> std::collections::HashMap<String, crate::ui::list_view::GitFileStatus> {
        let Some(repo_root) = self.col(side).git_cache.current_repo_root.clone() else {
            // Not in a repo — nothing to do, no cache to maintain.
            return std::collections::HashMap::new();
        };
        let mtimes = self.compute_git_mtime_key_fast(side);
        // Decide whether to reuse the cached raw output.
        let reuse = self
            .col(side)
            .git_cache
            .git_status_cache
            .as_ref()
            .is_some_and(|c| {
                c.repo_root == repo_root
                    && mtimes
                        .is_some_and(|(idx, head)| idx == c.index_mtime && head == c.head_mtime)
            });
        if force || !reuse {
            // A walk is needed. The `git status` spawn walks the entire index
            // (200-500 ms on a ~110k-file repo) and would block the UI thread,
            // so hand it to the background worker (stamped with this column's
            // `side` + generation so the result routes back to the right column
            // and stale results are discarded) and fall through to return the
            // CURRENT cache below — the display holds steady until the walk lands
            // (`apply_git_worker_result`). Returning an empty map / nulling here
            // flickers the dirty star + gutter for the ~1 poll the walk takes
            // under worktree churn.
            if self.col(side).git_cache.git_worker_available {
                let generation = self.col(side).git_cache.git_generation.wrapping_add(1);
                let c = self.col_mut(side);
                c.git_cache.git_generation = generation;
                c.git_cache.pending_git_requests.push(GitWorkerRequest {
                    generation,
                    repo_root,
                    side,
                });
                c.git_cache.last_git_request_at = Some(std::time::Instant::now());
            } else {
                // No worker (tests, App::new bootstrap) — walk synchronously and
                // refill in-band; the fresh entries are immediate, no async
                // window to flicker.
                self.col_mut(side).git_cache.git_status_cache = None;
                if let Some(entries) = crate::git::status::repo_status(&repo_root)
                    && let Some((index_mtime, head_mtime)) = mtimes
                {
                    self.col_mut(side).git_cache.git_status_cache = Some(GitStatusCache {
                        repo_root: repo_root.clone(),
                        index_mtime,
                        head_mtime,
                        entries,
                    });
                }
            }
        }
        // Re-filter the cached repo-wide entries to this listing dir's prefix — no
        // repo re-walk needed (the cache survives chdir within the repo). On the
        // async path this is the previous (stale-but-same-repo) snapshot; with no
        // cache yet (first walk / just switched repos) it's empty — the correct
        // "nothing to show yet".
        let Some(cache) = self.col(side).git_cache.git_status_cache.as_ref() else {
            return std::collections::HashMap::new();
        };
        let prefix = canonical
            .strip_prefix(&cache.repo_root)
            .unwrap_or(Path::new(""))
            .to_string_lossy()
            .into_owned();
        crate::git::status::map_to_listing(&cache.entries, &prefix)
    }

    /// Compute `side`'s `git_info` display string (`main`, `main*`, `abc1234`,
    /// or `None` outside a repo) from cached state without spawning any
    /// subprocesses.
    ///
    /// - Branch comes from gix (`head_name` / `head_id`), resolving
    ///   worktree/submodule gitlinks, packed refs, and detached HEAD.
    /// - Dirty flag comes from the raw porcelain cached in
    ///   [`Self::git_file_statuses_cached`] for *this* column.
    pub fn compute_git_info_fast(&mut self, side: Side) -> Option<String> {
        let repo_root = self.col(side).git_cache.current_repo_root.clone()?;
        let branch = self.cached_head_branch(side, &repo_root)?;
        // Only trust the raw cache for the dirty marker if it was captured for
        // *this* repo — without the filter a worktree switch left the bar
        // showing the previous worktree's dirty state for a frame.
        let dirty = self
            .col(side)
            .git_cache
            .git_status_cache
            .as_ref()
            .filter(|c| c.repo_root == repo_root)
            .is_some_and(|c| !c.entries.is_empty());
        Some(if dirty { format!("{branch}*") } else { branch })
    }

    /// Branch-display string for `repo_root`, memoized by `HEAD`'s mtime in
    /// `side`'s [`super::GitCache::head_branch_cache`] so `gix::open` only fires
    /// when `HEAD` is rewritten (checkout / branch-switch / detached-HEAD
    /// commit). When `HEAD` can't be stat'd we skip the cache and re-resolve.
    fn cached_head_branch(&mut self, side: Side, repo_root: &Path) -> Option<String> {
        let head_mtime = self
            .col(side)
            .git_cache
            .current_gitdir
            .as_ref()
            .and_then(|d| std::fs::metadata(d.join("HEAD")).ok())
            .and_then(|m| m.modified().ok());
        if let Some(mtime) = head_mtime
            && let Some((cached_root, cached_mtime, branch)) =
                self.col(side).git_cache.head_branch_cache.as_ref()
            && cached_root == repo_root
            && *cached_mtime == mtime
        {
            return Some(branch.clone());
        }
        let branch = crate::git::discovery::head_branch(repo_root)?;
        if let Some(mtime) = head_mtime {
            self.col_mut(side).git_cache.head_branch_cache =
                Some((repo_root.to_path_buf(), mtime, branch.clone()));
        }
        Some(branch)
    }

    /// Stat `index` and `HEAD` in `side`'s cached gitdir — the hot-path (1 Hz
    /// poll) freshness key. Reads the `current_gitdir` resolved once at chdir,
    /// so this never opens gix; it's pure `lstat` + `modified()`.
    pub(super) fn compute_git_mtime_key_fast(
        &self,
        side: Side,
    ) -> Option<(std::time::SystemTime, std::time::SystemTime)> {
        let gitdir = self.col(side).git_cache.current_gitdir.as_ref()?;
        let index_mt = std::fs::metadata(gitdir.join("index"))
            .and_then(|m| m.modified())
            .ok()?;
        let head_mt = std::fs::metadata(gitdir.join("HEAD"))
            .and_then(|m| m.modified())
            .ok()?;
        Some((index_mt, head_mt))
    }

    /// Set `side`'s `current_repo_root` and the derived `current_gitdir`
    /// together so they never drift. The gitdir resolution (gix) follows a
    /// linked worktree's `.git` *file* to its real gitdir. Early-returns when
    /// the repo root is unchanged, so the gix repo-open only fires when actually
    /// crossing into a different repo.
    pub(super) fn set_repo_root(&mut self, side: Side, repo_root: Option<std::path::PathBuf>) {
        if self.col(side).git_cache.current_repo_root == repo_root {
            return;
        }
        // Resolve the gitdir before borrowing `col_mut` (it doesn't touch self).
        let gitdir = repo_root.as_deref().and_then(crate::git::discovery::gitdir);
        let c = self.col_mut(side);
        c.git_cache.current_gitdir = gitdir;
        c.git_cache.current_repo_root = repo_root;
    }

    /// Resolve + cache `side`'s active repo root (and gitdir) for `canonical`,
    /// called on every chdir of that column. Also wipes the raw-status cache
    /// when crossing into a *different* repo (worktree switch), so the dirty
    /// marker doesn't briefly reflect the previous worktree.
    pub fn update_repo_root(&mut self, side: Side, canonical: &Path) {
        let repo_root = find_repo_root(canonical);
        if let Some(new_root) = repo_root.as_ref()
            && let Some(c) = self.col(side).git_cache.git_status_cache.as_ref()
            && &c.repo_root != new_root
        {
            self.col_mut(side).git_cache.git_status_cache = None;
        }
        self.set_repo_root(side, repo_root);
    }
}

#[cfg(test)]
mod tests {
    use crate::app::App;
    use crate::app::state::Side;
    use std::path::Path;

    #[test]
    fn working_tree_event_flags_rewalk_for_the_owning_column() {
        let dir = std::env::temp_dir().join("spyc-flag-rewalk-test");
        let _ = std::fs::create_dir_all(&dir);
        let mut app = App::test_app(dir.clone());
        // A path inside the column's tree forces its next poll to re-walk —
        // this is what converges an UNFOCUSED column after a working-tree edit
        // the mtime short-circuit can't see.
        assert!(
            app.state
                .flag_worktree_rewalk_for_path(&dir.join("src/lib.rs"))
        );
        assert!(
            app.state.col(Side::Left).git_cache.pending_worktree_rewalk,
            "an in-tree edit flags the column for a forced re-walk"
        );

        // A path outside every column's tree flags nothing (and doesn't churn
        // the flag back on).
        app.state
            .col_mut(Side::Left)
            .git_cache
            .pending_worktree_rewalk = false;
        assert!(
            !app.state
                .flag_worktree_rewalk_for_path(Path::new("/nowhere/near/here"))
        );
        assert!(!app.state.col(Side::Left).git_cache.pending_worktree_rewalk);
    }
}
