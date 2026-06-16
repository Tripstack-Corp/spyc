//! `AppState` git display/cache state: status refresh, the cached-status
//! lookup, fast info/mtime probes, and repo-root resolution. Split from
//! `state` verbatim.

use std::path::Path;

use super::{AppState, GitStatusCache, GitWorkerRequest};

use super::find_repo_root;

impl AppState {
    /// Re-poll just git state (`git_info` + `git_files`) and update
    /// only if it changed. Returns `true` iff anything was different.
    /// 1Hz safety net for FSEvents missing the `.git/index.lock` →
    /// `.git/index` rename on commit (inode replacement is the macOS
    /// FSEvents soft spot). The diff guard preserves the 0-dps-idle
    /// target: when nothing changed, we don't bump `list_generation`
    /// or request a repaint.
    ///
    /// **Mtime short-circuit.** Before running the gix status walk +
    /// branch read, stat `.git/index` + `.git/HEAD` and compare
    /// against the cache from the last successful call. When both
    /// mtimes match, the inputs to `git status` are bit-identical
    /// and we return false immediately. On a 100k-file working
    /// tree the `git status --porcelain -unormal` walk costs real
    /// CPU; skipping it on the idle path drops sustained background
    /// load to near zero.
    ///
    /// The cache is intentionally scoped to *this* poll — the
    /// event-driven `refresh_listing` path never consults it
    /// because working-tree edits change file mtimes but NOT
    /// `.git/index`/`HEAD`, and a cache hit there would silently
    /// miss the ` M filename` markers that refresh exists to
    /// surface.
    pub fn refresh_git_state(&mut self) -> bool {
        let key = self.compute_git_mtime_key_fast();
        // A throttled working-tree change (refresh_listing deferred its
        // invalidation) forces a re-walk: the mtime key below can't see an
        // unstaged edit, so honoring this flag is the only thing that converges
        // a stale ` M`/clean marker without waiting for a chdir.
        let force_rewalk = std::mem::take(&mut self.git_cache.pending_worktree_rewalk);
        if !force_rewalk && key.is_some() && key == self.git_cache.git_poll_cache {
            return false;
        }
        // mtime moved — invalidate the raw-status cache before going
        // through `git_file_statuses_cached`, which will re-spawn and
        // refill it on this dir.
        self.git_cache.git_status_cache = None;
        let listing_dir = self.listing.dir.clone();
        let new_git_files = self.git_file_statuses_cached(&listing_dir);
        let new_git_info = self.compute_git_info_fast();
        // Stash on success so the next idle poll skips the
        // subprocesses. Stat fail (e.g. shallow repo, .git missing)
        // ⇒ key is None and we'll keep running until it appears.
        self.git_cache.git_poll_cache = key;
        if new_git_info == self.git.info && new_git_files == self.git.files {
            return false;
        }
        self.git.set(new_git_info, new_git_files);
        self.rebuild_rows();
        true
    }

    /// Get the file-status map for `canonical`, using the raw-status
    /// cache when valid. The cache hit path skips the `git status`
    /// subprocess entirely and just re-parses the previously-captured
    /// porcelain text against the new dir's prefix — the slow part of
    /// the spawn is the index walk, which is identical for every
    /// chdir within the same repo. On the ~110k-file Java monorepo,
    /// this drops per-chdir cost from 200-500 ms to sub-millisecond.
    ///
    /// Caller must have already called [`Self::update_repo_root`] for
    /// `canonical` so `current_repo_root` reflects the new dir.
    pub fn git_file_statuses_cached(
        &mut self,
        canonical: &Path,
    ) -> std::collections::HashMap<String, crate::ui::list_view::GitFileStatus> {
        let Some(repo_root) = self.git_cache.current_repo_root.clone() else {
            // Not in a repo — nothing to do, no cache to maintain.
            return std::collections::HashMap::new();
        };
        let mtimes = self.compute_git_mtime_key_fast();
        // Decide whether to reuse the cached raw output.
        let reuse = self.git_cache.git_status_cache.as_ref().is_some_and(|c| {
            c.repo_root == repo_root
                && mtimes.is_some_and(|(idx, head)| idx == c.index_mtime && head == c.head_mtime)
        });
        if !reuse {
            // Cache miss — the `git status` spawn walks the entire
            // index (200-500 ms on a ~110k-file repo) and would
            // block the UI thread. Hand it to the background worker
            // and return an empty map for now; the App event loop
            // will fill in real markers when the worker posts its
            // result (matched against `git_generation` so navigating
            // away mid-spawn discards the stale result).
            if self.git_cache.git_worker_available {
                self.git_cache.git_generation = self.git_cache.git_generation.wrapping_add(1);
                self.git_cache.pending_git_requests.push(GitWorkerRequest {
                    generation: self.git_cache.git_generation,
                    repo_root,
                });
                self.git_cache.last_git_request_at = Some(std::time::Instant::now());
                return std::collections::HashMap::new();
            }
            // No worker (tests, App::new bootstrap) — fall through
            // to the synchronous walk path below.
            self.git_cache.git_status_cache = None;
            if let Some(entries) = crate::git::status::repo_status(&repo_root)
                && let Some((index_mtime, head_mtime)) = mtimes
            {
                self.git_cache.git_status_cache = Some(GitStatusCache {
                    repo_root,
                    index_mtime,
                    head_mtime,
                    entries,
                });
            }
        }
        // Re-filter the cached repo-wide entries to this listing dir's
        // prefix — no repo re-walk needed (the cache survives chdir
        // within the repo).
        let Some(cache) = self.git_cache.git_status_cache.as_ref() else {
            return std::collections::HashMap::new();
        };
        let prefix = canonical
            .strip_prefix(&cache.repo_root)
            .unwrap_or(Path::new(""))
            .to_string_lossy()
            .into_owned();
        crate::git::status::map_to_listing(&cache.entries, &prefix)
    }

    /// Compute the `git_info` display string (`main`, `main*`,
    /// `abc1234`, `(no git)` — well, `None`) from cached state
    /// without spawning any subprocesses. Replaces
    /// `sysinfo::git_status`, which spawned both
    /// `git rev-parse --abbrev-ref HEAD` AND a full
    /// `git status --porcelain` (with `-unormal`, walking every
    /// untracked file on the 110k-file tree) per chdir.
    ///
    /// - Branch comes from gix (`head_name` / `head_id`), which resolves
    ///   worktree/submodule gitlinks, packed refs, and detached HEAD.
    /// - Dirty flag comes from the raw porcelain we already cached
    ///   in [`Self::git_file_statuses_cached`]. Empty raw output ⇒
    ///   clean. Non-empty ⇒ dirty.
    ///
    /// Returns `None` if the listing dir isn't in a repo, mirroring
    /// the old `sysinfo::git_status` contract.
    pub fn compute_git_info_fast(&mut self) -> Option<String> {
        let repo_root = self.git_cache.current_repo_root.clone()?;
        let branch = self.cached_head_branch(&repo_root)?;
        // Only trust the raw cache for the dirty marker if it was
        // captured for *this* repo. Without the `c.repo_root` filter,
        // a worktree switch left the top-bar showing the previous
        // worktree's dirty state for a frame (until the async git
        // worker filled the new cache) — reported by Spencer as
        // "stale markers" after switching worktrees.
        let dirty = self
            .git_cache
            .git_status_cache
            .as_ref()
            .filter(|c| c.repo_root == repo_root)
            .is_some_and(|c| !c.entries.is_empty());
        Some(if dirty { format!("{branch}*") } else { branch })
    }

    /// Branch-display string for `repo_root`, memoized by `HEAD`'s mtime in
    /// [`GitCache::head_branch_cache`] so the `gix::open` only fires when
    /// `HEAD` is rewritten (checkout / branch-switch / detached-HEAD commit).
    /// On an active filesystem `refresh_listing` trips every few seconds and
    /// the unmemoized read re-opened the repo each time just to recover an
    /// unchanged branch name. When `HEAD` can't be stat'd we can't validate
    /// freshness, so we skip the cache and re-resolve (the prior behavior).
    fn cached_head_branch(&mut self, repo_root: &Path) -> Option<String> {
        let head_mtime = self
            .git_cache
            .current_gitdir
            .as_ref()
            .and_then(|d| std::fs::metadata(d.join("HEAD")).ok())
            .and_then(|m| m.modified().ok());
        if let Some(mtime) = head_mtime
            && let Some((cached_root, cached_mtime, branch)) =
                self.git_cache.head_branch_cache.as_ref()
            && cached_root == repo_root
            && *cached_mtime == mtime
        {
            return Some(branch.clone());
        }
        let branch = crate::git::discovery::head_branch(repo_root)?;
        if let Some(mtime) = head_mtime {
            self.git_cache.head_branch_cache =
                Some((repo_root.to_path_buf(), mtime, branch.clone()));
        }
        Some(branch)
    }

    /// Stat `index` and `HEAD` in the cached gitdir — the hot-path
    /// (1 Hz poll) freshness key. Reads the `current_gitdir` resolved
    /// once at chdir (`set_repo_root`), so this never opens gix or
    /// re-resolves the gitdir; it's pure `lstat` + `modified()`.
    pub(super) fn compute_git_mtime_key_fast(
        &self,
    ) -> Option<(std::time::SystemTime, std::time::SystemTime)> {
        let gitdir = self.git_cache.current_gitdir.as_ref()?;
        let index_mt = std::fs::metadata(gitdir.join("index"))
            .and_then(|m| m.modified())
            .ok()?;
        let head_mt = std::fs::metadata(gitdir.join("HEAD"))
            .and_then(|m| m.modified())
            .ok()?;
        Some((index_mt, head_mt))
    }

    /// Set `current_repo_root` and the derived `current_gitdir` together
    /// so they never drift. The gitdir resolution (gix) follows a linked
    /// worktree's `.git` *file* to its real gitdir.
    ///
    /// Early-returns when the repo root is unchanged — this runs on every
    /// chdir (incl. within-repo navigation), so the gix repo-open only
    /// fires when actually crossing into a different repo. The cached
    /// `current_gitdir` is then reused by the 1 Hz mtime poll with no gix.
    pub(super) fn set_repo_root(&mut self, repo_root: Option<std::path::PathBuf>) {
        if self.git_cache.current_repo_root == repo_root {
            return;
        }
        self.git_cache.current_gitdir =
            repo_root.as_deref().and_then(crate::git::discovery::gitdir);
        self.git_cache.current_repo_root = repo_root;
    }

    /// Resolve + cache the active repo root (and gitdir) for the given
    /// canonical dir, called on every chdir. `find_repo_root` walks up to the
    /// enclosing `.git`; cheap, so there's no per-project memoization.
    ///
    /// Also wipes the raw-status cache when crossing into a *different* repo
    /// (worktree switch): `git_file_statuses_cached` does its own key check on
    /// the marker path, but `compute_git_info_fast` didn't, and Spencer
    /// reported brief "stale dirty marker" flashes on worktree switches.
    /// Going from a repo to a non-repo dir doesn't satisfy `repo_root.is_some()`
    /// here, so the cache lives on for re-entry.
    pub fn update_repo_root(&mut self, canonical: &Path) {
        let repo_root = find_repo_root(canonical);
        if let Some(new_root) = repo_root.as_ref()
            && let Some(c) = self.git_cache.git_status_cache.as_ref()
            && &c.repo_root != new_root
        {
            self.git_cache.git_status_cache = None;
        }
        self.set_repo_root(repo_root);
    }
}
