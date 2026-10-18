//! Git status/worker plumbing: the off-thread request/result pair, the
//! structured-status cache they carry, the display pair (`GitState`), and the
//! per-column cache + worker outbox (`GitCache`) that wraps them. Extracted
//! out of `state/mod.rs` (a cohesive, self-contained block — the file-size
//! ratchet asked for a split once #99 step 1's `Project`/`ProjectId` additions
//! pushed it over), so the Model field block there stays legible.

use std::path::PathBuf;

use super::{ProjectId, Side};

/// Off-main-thread git-status work item. The chdir hot path sends
/// these to a background worker on cache miss so the UI returns
/// immediately; the worker computes the repo's status via gix
/// (`git::status::repo_status`) — the heavy index/worktree walk on a
/// ~110k-file repo — and echoes results back via `GitWorkerResult`.
#[derive(Debug)]
pub struct GitWorkerRequest {
    pub generation: u64,
    pub repo_root: std::path::PathBuf,
    /// Which project and column requested this — the worker echoes both back
    /// so the result routes to that column's git state + generation gate. A
    /// generation is per column, so the project is what keeps two projects'
    /// columns apart.
    pub project: ProjectId,
    pub side: Side,
}

/// Worker reply. `generation` lets the main thread discard results
/// whose source chdir has been superseded. `entries` is the structured
/// repo-relative status (`git::status::StatusEntry`s), or `None` when
/// the status walk failed (not in a repo, etc.) — the App treats that
/// as "no markers" rather than a hard error.
#[derive(Debug)]
pub struct GitWorkerResult {
    pub generation: u64,
    pub repo_root: std::path::PathBuf,
    /// The project and column this result is for (echoed from the request) —
    /// routes it to that column's git state + generation gate.
    pub project: ProjectId,
    pub side: Side,
    pub entries: Option<Vec<crate::git::status::StatusEntry>>,
    pub index_mtime: Option<std::time::SystemTime>,
    pub head_mtime: Option<std::time::SystemTime>,
}

/// Cached repo status (structured `StatusEntry`s), keyed by repo root +
/// `.git/index` / `.git/HEAD` mtimes. The status doesn't depend on the
/// current listing dir — only on the repo state — so once we have it,
/// every chdir to a sibling/child path within the same repo re-filters
/// it locally (`git::status::map_to_listing` with a freshly-computed
/// prefix) instead of walking the repo again. On a ~110k-file repo that
/// walk dominates drill-in latency.
#[derive(Debug, Clone)]
pub struct GitStatusCache {
    pub repo_root: PathBuf,
    pub index_mtime: std::time::SystemTime,
    pub head_mtime: std::time::SystemTime,
    pub entries: Vec<crate::git::status::StatusEntry>,
}

/// MVU Phase 5: the git **display pair** — the top-bar `info` string and
/// the basename-keyed-per-listing-dir `files` status map. The previously
/// separate `git_info` / `git_files` fields drifted when one was written
/// without the other ("files updated but not info"). Folding them here
/// and routing the recompute sites through [`GitState::set`] makes the
/// pair atomic: top-bar and per-file markers always reflect the same
/// `git status` snapshot. (The git *caches* — `raw_cache`, `repo_root`,
/// `generation`, … — are `AppState` fields; clustering them would be pure
/// relocation, not a fix.)
#[derive(Default)]
pub struct GitState {
    pub info: Option<String>,
    pub files: std::collections::HashMap<String, crate::ui::list_view::GitFileStatus>,
}

impl GitState {
    /// The sole writer of the `info`/`files` pair — set both together so
    /// they can never drift. The recompute sites compute `info` and
    /// `files` from the same cached `git status` snapshot, then commit
    /// them here in one call.
    pub fn set(
        &mut self,
        info: Option<String>,
        files: std::collections::HashMap<String, crate::ui::list_view::GitFileStatus>,
    ) {
        self.info = info;
        self.files = files;
    }
}

/// Git status/worker plumbing cached on the Model: the 1 Hz mtime
/// short-circuit pair, the resolved repo-root/gitdir, the
/// structured-status cache, and the off-thread worker
/// outbox/generation/timing. Grouped out of `AppState`
/// (the loose git-cache fields) so the Model field block stays legible;
/// the display pair lives separately in [`GitState`].
#[derive(Default)]
pub struct GitCache {
    /// Cached mtime pair of `.git/index` and `.git/HEAD` from the
    /// last successful `refresh_git_state` call. The 1 Hz poll
    /// short-circuits when both files' current mtimes match the
    /// cache — their contents haven't changed, so re-running
    /// `git status --porcelain -unormal` would produce identical
    /// output. On a huge working tree (the original report:
    /// ~112k files) that subprocess walks every tracked file, so
    /// skipping it on the idle path drops sustained background CPU
    /// to near zero.
    ///
    /// Event-driven refreshes (from `fsevents`) never consult this
    /// cache — working-tree edits change file mtimes but NOT
    /// `.git/index`, so a cache hit there would silently miss
    /// the ` M filename` markers the refresh exists to surface.
    /// `None` until the first successful poll, or when `.git/index`
    /// can't be stat'd.
    pub git_poll_cache: Option<(std::time::SystemTime, std::time::SystemTime)>,
    /// Memoized branch-display string keyed by `(repo_root, HEAD mtime)`.
    /// `compute_git_info_fast` re-derives the branch via `gix::open` only
    /// when `HEAD`'s mtime moves (checkout / branch-switch / detached-HEAD
    /// commit) — otherwise an active filesystem trips `refresh_listing`
    /// every few seconds and each call would re-open the repo just to read
    /// an unchanged branch name. The dirty `*` flag is NOT cached here: it's
    /// recombined fresh each call from `git_status_cache`, which moves on
    /// working-tree edits that `HEAD`'s mtime can't see. Shares the coarse-
    /// FS-mtime caveat of [`Self::git_poll_cache`] (same key, same 1 Hz poll
    /// gate). `None` until the first resolve, or outside a repo.
    pub head_branch_cache: Option<(std::path::PathBuf, std::time::SystemTime, String)>,
    /// Repo root of the active project (the directory containing
    /// `.git`), or `None` when the listing dir isn't inside any
    /// repo. Resolved + cached on chdir by `update_repo_root`. Used to
    /// compute the `git status --porcelain` prefix without spawning
    /// `git rev-parse --show-toplevel`, as the cache key for
    /// `git_status_cache`, and as the root for the gitignore-aware
    /// FSEvent filter (`git::excludes`).
    pub current_repo_root: Option<std::path::PathBuf>,
    /// The active repo's *resolved* gitdir — `<root>/.git` for a normal
    /// repo, or `<main>/.git/worktrees/<name>/` for a linked worktree
    /// (where `.git` is a *file* pointing there). `None` outside a repo.
    /// Cached on chdir (alongside `current_repo_root`) so the fs-event
    /// filter and the gitdir watch stay IO-free per event. Without this,
    /// a worktree's index/HEAD changes — which live outside the working
    /// tree — were never watched and markers went stale until the next
    /// poll.
    pub current_gitdir: Option<std::path::PathBuf>,
    /// The repo's shared config path, resolved with the gitdir. In the status
    /// cache key because config *changes what `status` reports* — a repo made
    /// bare reports clean with no worktree, and nothing about that moves
    /// `index` or `HEAD`.
    pub current_config_path: Option<std::path::PathBuf>,
    /// Cached structured repo-status (`Vec<StatusEntry>`). On a huge
    /// working tree the gix status walk traverses every tracked file in
    /// the index — 200-500 ms on a ~110k-file repo. After the first
    /// chdir into a project, every subsequent chdir within that
    /// project's tree hits this cache (provided `.git/index` and
    /// `.git/HEAD` mtimes match) and skips the walk; only the
    /// per-listing-dir prefix re-parse is paid.
    pub git_status_cache: Option<GitStatusCache>,
    /// Outbox of git-status worker requests the Model wants dispatched.
    /// `git_file_statuses_cached` pushes here on a cache miss; the App run
    /// loop drains it via `flush_git_requests` (sending each over the
    /// Runtime-owned `git_worker_tx`) before it next blocks on `recv`.
    /// Results arrive back via the App's `git_result_rx`, gated on
    /// `git_generation` to discard stale ones.
    pub pending_git_requests: Vec<GitWorkerRequest>,
    /// Monotonic counter bumped on every chdir-that-issues-a-git-
    /// request. The worker echoes the counter back in its result;
    /// the App event loop discards results whose generation doesn't
    /// match the current value (the user has navigated past them).
    /// Eliminates the need for explicit thread cancellation.
    pub git_generation: u64,
    /// When the last raw-status cache invalidation was performed by
    /// `refresh_listing` (i.e. the event-driven path). Throttles
    /// re-spawning the git worker on huge trees where each spawn
    /// runs full `git status` for 200-500 ms: an active filesystem
    /// (claude editing files, build outputs) could trip
    /// `refresh_listing` every 3 s and burn CPU on
    /// back-to-back worker runs. The 1 s / 10 s safety poll still
    /// invalidates on actual `.git/index` mtime changes, so the only
    /// trade-off is a small lag in working-tree ` ~` markers for
    /// edits within the throttle window.
    pub last_git_invalidation: Option<std::time::Instant>,
    /// When the last git worker request was *sent* (after dedup, etc.).
    /// Read by the activity-monitor overlay so the user can see
    /// roundtrip duration for the most recent `git status` spawn
    /// (which on huge trees can be 200-500 ms).
    pub last_git_request_at: Option<std::time::Instant>,
    /// A working-tree fs-event arrived but `refresh_listing` *throttled* its
    /// cache invalidation, so the new ` ~`/clean state was never re-walked.
    /// The 1 Hz poll's own mtime short-circuit can't cover it — an unstaged
    /// edit moves no `.git/index`/`HEAD` mtime — so without this flag a change
    /// landing inside the throttle window (with no trailing event) would stay
    /// stale until a chdir. Set on a throttle-skip; the next `refresh_git_state`
    /// honours it with a forced re-walk and clears it, bounding staleness to one
    /// poll interval instead of forever.
    pub pending_worktree_rewalk: bool,
}
