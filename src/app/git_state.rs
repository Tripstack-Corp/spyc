//! MVU Phase 5: git worker-result application, extracted from the event
//! loop (`mod.rs`) to buy anti-monolith headroom and decouple the git
//! result path ahead of the GitState reunion (PR 1/2). `impl App` in a
//! child module — reads App's private `self.state` via the
//! descendant-module rule, same pattern as `effect` / `actions`.

use super::{App, state};

impl App {
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
        let Some(raw) = result.raw else {
            // `git status` failed (not in a repo by the time the
            // worker spawned, or git missing). Leave existing state.
            return false;
        };
        let (Some(index_mtime), Some(head_mtime)) = (result.index_mtime, result.head_mtime) else {
            return false;
        };
        self.state.git_status_raw_cache = Some(state::GitStatusRawCache {
            repo_root: result.repo_root.clone(),
            index_mtime,
            head_mtime,
            raw,
        });
        // Seed the 1 Hz poll cache too — without this the next safety
        // poll would observe a None cache and re-fire `git status`.
        self.state.git_poll_cache = Some((index_mtime, head_mtime));
        // Reparse against the current listing dir's prefix and refresh
        // the display string. `compute_git_info_fast` reads the cache
        // we just stored for its dirty flag.
        let listing_dir = self.state.listing.dir.clone();
        let new_files = {
            let cache = self.state.git_status_raw_cache.as_ref().unwrap();
            let prefix = listing_dir
                .strip_prefix(&cache.repo_root)
                .unwrap_or(std::path::Path::new(""))
                .to_string_lossy()
                .into_owned();
            crate::sysinfo::parse_porcelain_statuses(&cache.raw, &prefix)
        };
        let new_info = self.state.compute_git_info_fast();
        let changed = new_files != self.state.git.files || new_info != self.state.git.info;
        self.state.git.set(new_info, new_files);
        if changed {
            self.state.rebuild_rows();
        }
        changed
    }
}
