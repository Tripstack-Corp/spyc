//! Domain state for the application — everything testable without a terminal.
//!
//! `AppState` holds navigation, selection, filtering, bookmarks, input mode,
//! config, history, and cached info. Event handlers that operate on pure
//! domain logic live here; the `App` shell in `mod.rs` owns terminal state
//! (pager widget, pane tabs, pty handles) and delegates to `AppState`.

use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::fs::Listing;
use crate::keymap::{Resolver, UserKeymap};
use crate::state::{Cursor, Frecency, History, IgnoreMasks, Inventory, Marks, Picks};
use crate::ui::list_view::GridDims;

use super::{Effect, FlashKind, FlashMessage, Mode, RowData, View};

mod apply;
mod dispatch;
mod git;
mod listing;
mod navigation;
mod selection;

/// Result of `AppState::dispatch_command` — tells the `App` caller what to do.
#[derive(Debug)]
pub enum CommandResult {
    /// Handled entirely by `AppState`. No terminal action needed.
    Handled,
    /// Open the marks view in the pager. Carries the lines.
    OpenPager { title: String, lines: Vec<String> },
    /// `:q` / `:quit`. Caller must run the full quit lifecycle
    /// (`App::request_quit`) — pure-domain can't reach
    /// `save_session()` or the pane/background-task counts. A typed
    /// variant (rather than `NotHandled`) makes the App-side
    /// exhaustive match a compile-time check: dropping the arm
    /// breaks the build instead of silently regressing `:q` to an
    /// "unknown command" path.
    Quit,
    /// Effects for the event loop to execute (MVU Phase 5 PR9: the
    /// carrier-unification that lets a pure-domain `:`-command emit an
    /// `Effect` — e.g. `:cd` → `Effect::ChangeDir` — instead of doing the
    /// blocking chdir IO inline, matching `ApplyResult::Post`). Empty ==
    /// nothing to do.
    Post(Vec<Effect>),
    /// The input was a shell/pager/overlay command — caller should handle it.
    NotHandled,
}

/// Result of `AppState::dispatch_prompt` — tells the `App` caller what to do.
#[derive(Debug)]
pub enum PromptResult {
    /// Handled entirely by `AppState`.
    Handled,
    /// Needs terminal: caller handles this prompt kind.
    NotHandled,
}

/// Result of `AppState::apply` — tells the `App` caller what to do.
#[derive(Debug)]
pub enum ApplyResult {
    /// Handled entirely by `AppState`. Cursor already clamped.
    Handled,
    /// Open a pager with these contents.
    OpenPager(PagerRequest),
    /// Effects for the event loop to execute (e.g. a `ForegroundExec`
    /// for `$SHELL`). Empty == nothing to do.
    Post(Vec<Effect>),
    /// Caller must handle this action (terminal-touching).
    NotHandled,
}

/// MVU Stage 3: the unified result of any pure-domain producer
/// (`apply` / `dispatch_command` / `dispatch_prompt`). Collapses the three
/// per-producer result enums above so the single `App::update` entry point
/// (Stage 3C) can handle every transition uniformly:
/// - `Handled(effects)` carries the former `Post` effects (empty `Vec` for the
///   old plain `Handled` — the App side runs whatever's there);
/// - `OpenPager` / `Quit` stay typed (App-only capability + a compile-time
///   wiring guard — dropping the arm is a build error, not a silent regress);
/// - `Defer` is the old `NotHandled` — the App-side executor takes over.
///
/// The `From` impls are the single, tested mapping site; producers keep their
/// own result enums until 3C routes everything through `Update`.
/// The three App-side bridges (`apply_inner` / `dispatch_command` /
/// `dispatch_prompt`) normalize their producer's result into this via `From`
/// and match it uniformly (MVU Stage 3C). The producers still return their own
/// enums for now; Stage 3D switches them to return `Update` directly and
/// deletes the three, and adds the single `App::update(msg)` entry point.
#[derive(Debug)]
pub enum Update {
    Handled(Vec<Effect>),
    OpenPager(PagerRequest),
    Quit,
    Defer,
}

impl From<ApplyResult> for Update {
    fn from(r: ApplyResult) -> Self {
        match r {
            ApplyResult::Handled => Self::Handled(Vec::new()),
            ApplyResult::OpenPager(req) => Self::OpenPager(req),
            ApplyResult::Post(fx) => Self::Handled(fx),
            ApplyResult::NotHandled => Self::Defer,
        }
    }
}

impl From<CommandResult> for Update {
    fn from(r: CommandResult) -> Self {
        match r {
            CommandResult::Handled => Self::Handled(Vec::new()),
            // Normalize the marks-view shape to a PagerRequest matching the
            // command path's old `PagerView::new_plain(title, lines)` (columns
            // = 1, no fit-to-content — `new_plain` defaults).
            CommandResult::OpenPager { title, lines } => Self::OpenPager(PagerRequest {
                title,
                lines,
                columns: 1,
                fit_to_content: false,
            }),
            CommandResult::Quit => Self::Quit,
            CommandResult::Post(fx) => Self::Handled(fx),
            CommandResult::NotHandled => Self::Defer,
        }
    }
}

impl From<PromptResult> for Update {
    fn from(r: PromptResult) -> Self {
        match r {
            PromptResult::Handled => Self::Handled(Vec::new()),
            PromptResult::NotHandled => Self::Defer,
        }
    }
}

/// Outcome of [`AppState::take`] (yanking files into the inventory). A typed
/// result so the caller dispatches on the variant instead of string-prefixing
/// the flash message to tell success from error.
#[derive(Debug)]
pub enum TakeOutcome {
    /// Files were yanked; carries the success flash message.
    Yanked(String),
    /// Nothing yanked; carries the error flash message.
    Failed(String),
    /// Nothing to do — wrong view, or an empty selection with no error.
    Noop,
}

/// Description of a pager to open, without importing UI types.
#[derive(Debug)]
pub struct PagerRequest {
    pub title: String,
    pub lines: Vec<String>,
    pub columns: u8,
    /// When true, the pager height auto-shrinks to fit content (top edge
    /// stays anchored to the standard centered position; the box just
    /// grows shorter from the bottom). Line-number gutter is suppressed
    /// since it's noise for short summaries.
    pub fit_to_content: bool,
}

/// Off-main-thread git-status work item. The chdir hot path sends
/// these to a background worker on cache miss so the UI returns
/// immediately; the worker computes the repo's status via gix
/// (`git::status::repo_status`) — the heavy index/worktree walk on a
/// ~110k-file repo — and echoes results back via `GitWorkerResult`.
#[derive(Debug)]
pub struct GitWorkerRequest {
    pub generation: u64,
    pub repo_root: std::path::PathBuf,
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

/// Which surface owns the keyboard — the single focus axis, and the
/// **authority** the router reads for the persistent regions. Kept correct
/// at every loop top by `recompute_focus` (re-derived from the live surfaces
/// via the pure `decide_focus`), so `route::route_input` maps `Overlay` →
/// overlay-pty and `Pane` → bottom-pane directly rather than reconstructing
/// from `top_overlay.is_some()` / a separate `pane_focused` flag.
/// `pane_focused()` (== `matches!(self, Focus::Pane)`) is the same value,
/// now sourced here. The *transient* modal overlays
/// (finder/capture/quick-select/…) are the orthogonal `route::Modal` axis,
/// not `Focus` variants. A single `Focus` names only the focused region; a
/// `D` TopPane pager mounted above a focused bottom scrollback is tracked
/// separately (see `route::RouteSnapshot`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Focus {
    /// File-list area owns input (no pane focused, no overlay/pager
    /// owning keys). Idle / default.
    #[default]
    FileList,
    /// Bottom pty pane owns input.
    Pane,
    /// Top overlay subprocess (`V` editor / `;cmd` / huge-file `$PAGER`)
    /// owns input.
    Overlay,
    /// In-app pager owns input, tagged with its mount slot.
    Pager(crate::ui::pager::Mount),
}

/// MVU Phase 5: the git **display pair** — the top-bar `info` string and
/// the basename-keyed-per-listing-dir `files` status map. The previously
/// separate `git_info` / `git_files` fields drifted when one was written
/// without the other ("files updated but not info"). Folding them here
/// and routing the recompute sites through [`GitState::set`] makes the
/// pair atomic: top-bar and per-file markers always reflect the same
/// `git status` snapshot. (The git *caches* — `raw_cache`, `repo_root`,
/// `generation`, … — stay as `AppState` fields for now; clustering them
/// is pure relocation, not a bug, and is deferred.)
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

/// MVU Phase 5: a cheap Model-side snapshot of the active pane's
/// routing-relevant flags, refreshed once per loop iteration (after the
/// tab drain + `mark_exited`, before `recv`). `route_snapshot` reads these
/// instead of the live `PtyHost` so key routing decouples from the
/// Runtime. Behavior-equivalent: refreshed from the live pane at the same
/// loop point the old live read would have observed, and the only mutator
/// of `is_scrolling`/`is_closed` (key handlers / child-exit drain) runs
/// either before this refresh or after `route_snapshot`. (Render keeps its
/// own live reads; the richer per-tab snapshot for render purity is Phase 6.)
#[derive(Default, Clone, Copy)]
pub struct PaneSnapshot {
    pub is_scrolling: bool,
    pub is_closed: bool,
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
    /// Cached structured repo-status (`Vec<StatusEntry>`). On a huge
    /// working tree the gix status walk traverses every tracked file in
    /// the index — 200-500 ms on a ~110k-file repo. After the first
    /// chdir into a project, every subsequent chdir within that
    /// project's tree hits this cache (provided `.git/index` and
    /// `.git/HEAD` mtimes match) and skips the walk; only the
    /// per-listing-dir prefix re-parse is paid.
    pub git_status_cache: Option<GitStatusCache>,
    /// Whether a background git-status worker is wired. Set by `App::new`
    /// once the worker thread spawns; `false` in tests and during the
    /// AppState bootstrap before the worker exists. When `true`, a
    /// cache-miss in `git_file_statuses_cached` enqueues a request into
    /// `pending_git_requests` — the Model holds no channel, so the App run
    /// loop flushes that outbox to the Runtime-owned `git_worker_tx`. When
    /// `false`, the synchronous `git status` spawn path runs inline.
    pub git_worker_available: bool,
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
    /// trade-off is a small lag in working-tree ` M` markers for
    /// edits within the throttle window.
    pub last_git_invalidation: Option<std::time::Instant>,
    /// When the last git worker request was *sent* (after dedup, etc.).
    /// Read by the activity-monitor overlay so the user can see
    /// roundtrip duration for the most recent `git status` spawn
    /// (which on huge trees can be 200-500 ms).
    pub last_git_request_at: Option<std::time::Instant>,
    /// A working-tree fs-event arrived but `refresh_listing` *throttled* its
    /// cache invalidation, so the new ` M`/clean state was never re-walked.
    /// The 1 Hz poll's own mtime short-circuit can't cover it — an unstaged
    /// edit moves no `.git/index`/`HEAD` mtime — so without this flag a change
    /// landing inside the throttle window (with no trailing event) would stay
    /// stale until a chdir. Set on a throttle-skip; the next `refresh_git_state`
    /// honors it with a forced re-walk and clears it, bounding staleness to one
    /// poll interval instead of forever.
    pub pending_worktree_rewalk: bool,
}

/// Bottom-pane layout + prompt-echo state: the split percentage, zoom
/// (with the focus captured at zoom-on), the hide toggle, the once-per-loop
/// routing snapshot, and the `^a c` prompt-echo buffers. Grouped out of
/// `AppState`'s loose pane fields.
#[derive(Default)]
pub struct PaneLayout {
    pub pane_prompt_buf: String,
    pub last_pane_prompt: Option<String>,
    /// MVU Phase 5: active-pane routing flags, refreshed at loop-top (see
    /// [`PaneSnapshot`]). Read by `route_snapshot` so routing reads the
    /// Model, not the live pane host.
    pub pane_snapshot: PaneSnapshot,
    pub pane_height_pct: u16,
    /// Tmux-style "zoom": when true, the bottom pane fills the middle
    /// region (list collapses to 0 rows). The user's preferred
    /// `pane_height_pct` is preserved untouched so un-zoom restores
    /// exactly the prior split.
    pub pane_zoomed: bool,
    /// Focus state captured at zoom-on, restored at zoom-off. `None`
    /// when not zoomed.
    pub pane_focus_before_zoom: Option<bool>,
    /// `F10` / `^a-\` (TogglePane) toggles this. When true, the pane
    /// row is hidden — render skips the pane area, layout treats it
    /// as if no pane existed — but `pane_tabs` and every child pty
    /// stay alive. Re-toggle flips it back; the rendered grid picks
    /// up wherever the running processes left off. Previous behavior
    /// was destructive (`pane_tabs = None`, SIGKILL via `Drop for
    /// PtyHost`), which the user experienced as "I lost my claude
    /// conversation every time I needed the whole screen for a
    /// second."  Explicit kill of a tab still goes through `^a-x`
    /// (`PaneCloseTab`).
    pub pane_hidden: bool,
}

pub struct AppState {
    pub listing: Listing,
    pub picks: Picks,
    pub inventory: Inventory,
    pub marks: Marks,
    pub masks: IgnoreMasks,
    pub temp_filter: Option<String>,
    pub sort_order: crate::fs::listing::SortMode,
    /// When true, invert the per-mode natural direction (Name/Ext
    /// ascending → descending, Size/Mtime descending → ascending).
    /// Toggled by `gs` and `:sort reverse`. Dirs-first grouping is
    /// always preserved regardless.
    pub sort_reversed: bool,
    pub view: View,
    pub cursor: Cursor,
    pub resolver: Resolver,
    pub user_keymap: UserKeymap,
    pub config: Config,
    pub mode: Mode,
    pub start_dir: PathBuf,
    pub project_home: Option<PathBuf>,
    pub session_name: Option<String>,
    pub prev_dir: Option<PathBuf>,
    pub last_search: Option<String>,
    pub last_captured_cmd: Option<String>,
    pub history: History,
    /// Command history for the "pane command:" prompt (`^a c`). Holds
    /// only the *commands* the user launched tabs with — never the
    /// directories entered at the follow-up "pane cwd:" step, which
    /// live in [`Self::pane_cwd_history`]. Sharing one bucket polluted
    /// the command browse with directory paths.
    pub pane_history: History,
    /// Destination history for the "pane cwd:" prompt — previously-used
    /// working directories, so Up/Down recalls them without mixing into
    /// the command history above.
    pub pane_cwd_history: History,
    /// Persistent history for `J` (jump-to-path) prompts. Up / Down
    /// in the prompt cycle through previously-jumped destinations,
    /// independent of the shell-command and pane-prompt histories
    /// so they don't pollute each other.
    pub jump_history: History,
    /// Persistent history for `:` (vim-style command-line) prompts.
    /// Kept separate from shell-command history so `:make sync-all`
    /// (a typo for `!make sync-all`) doesn't surface back as a `:`
    /// command on Up arrow and explode with "unknown command".
    pub command_history: History,
    pub flash: Option<FlashMessage>,
    pub should_quit: bool,
    pub quit_pending: Option<std::time::Instant>,
    /// MVU Phase 5: the git display pair (top-bar string + per-file
    /// status map), written only together via [`GitState::set`]. See
    /// [`GitState`].
    pub git: GitState,
    /// Snapshot of the active harpoon ancestor-set (slot paths plus
    /// every parent directory of every slot). App refreshes this
    /// whenever the harpoon list mutates so `apply_temp_filter`
    /// remains pure-domain. Empty when no `PROJECT_HOME` is active.
    pub harpoon_filter_set: std::collections::HashSet<PathBuf>,
    /// MVU Phase 5: domain fields relocated from `App` (Model
    /// consolidation). The active harpoon list (pinned per-project file
    /// pointers; `None` when `PROJECT_HOME` is unset); the bottom-pane
    /// prompt-echo buffer being tracked for `yP`; and the last typed pane
    /// prompt. (`pager_positions` is deferred — its `::load()` ctor does
    /// disk IO unwanted in `test_default`.)
    pub harpoon: Option<crate::state::Harpoon>,
    /// Rows about to be deleted, populated while a `RemoveConfirm`
    /// prompt is active. Drives the warning-color row highlight in
    /// the list view: the user sees exactly which files the `y` /
    /// `Y` keystroke will affect. Cleared when the prompt resolves
    /// (confirm or cancel). Stored as paths so a mid-prompt
    /// listing refresh that drops a row doesn't leave a stale
    /// index lit up.
    pub pending_delete_preview: Option<Vec<PathBuf>>,
    /// Snapshot of the graveyard, refreshed when `View::Graveyard`
    /// is opened or after any entry-mutating action. Newest-first
    /// (matches `Graveyard::load`). Empty when not in graveyard
    /// view; the rebuild_rows path keys off `view` so we don't pay
    /// the disk read except when the user is looking at it.
    pub graveyard: Vec<crate::state::graveyard::Entry>,
    pub user_host: String,
    pub pending_new_tab_cmd: Option<String>,
    pub pending_worktrees: Option<Vec<PathBuf>>,
    pub pending_sessions: Option<Vec<crate::state::sessions::Session>>,
    pub frecency: Frecency,
    /// Which surface owns the keyboard. Replaces the old `pane_focused:
    /// bool`; read the derived bool via `self.pane_focused()`.
    pub focus: Focus,
    pub rows: Vec<RowData>,
    /// MVU Phase 5: the geometry slice of the last rendered grid (cols ×
    /// rows-per-col), written by render and read by cursor/page-math. Was
    /// `last_grid: Grid`; slimmed to drop the render-only `col_widths`.
    pub grid_dims: GridDims,
    /// Monotonic counter bumped whenever the display row list changes.
    /// Used by App to skip redundant `build_rows()` calls.
    pub list_generation: u64,
    /// Git status/worker plumbing (see [`GitCache`]).
    pub git_cache: GitCache,
    /// Bottom-pane layout + prompt state (see [`PaneLayout`]).
    pub pane: PaneLayout,
}

impl AppState {
    /// Old `pane_focused` axis, derived from the single `focus` field.
    /// True exactly when the bottom pty pane owns the keyboard. Every
    /// former read of `self.state.pane_focused` is now
    /// `self.state.pane_focused()`; behavior-equivalent because the
    /// router, the render DIM cue, the `^C` gate and the paste filter
    /// only ever consumed the bool, never the non-Pane discriminant.
    pub const fn pane_focused(&self) -> bool {
        matches!(self.focus, Focus::Pane)
    }

    // --- Cursor/navigation (Phase 1) ---

    pub fn flash_info<S: Into<String>>(&mut self, text: S) {
        self.flash = Some(FlashMessage {
            text: text.into(),
            kind: FlashKind::Info,
        });
    }

    pub fn flash_error<S: Into<String>>(&mut self, text: S) {
        self.flash = Some(FlashMessage {
            text: text.into(),
            kind: FlashKind::Error,
        });
    }

    /// Flash the outcome of a "save pane contents to file" action: the
    /// saved file's basename on success, the error text on failure. Shared
    /// by the two pane-scrollback save handlers (the `PaneScrollSave` action
    /// and the `s` key in pane-scroll mode), which were byte-for-byte
    /// identical. Errors stay `flash_info` (not `flash_error`) to preserve
    /// the pre-existing presentation.
    pub fn flash_saved_file(&mut self, result: std::io::Result<std::path::PathBuf>) {
        match result {
            Ok(path) => {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                self.flash_info(format!("saved: {name}"));
            }
            Err(e) => self.flash_info(format!("save error: {e}")),
        }
    }

    /// Human-readable session name for status bar / overlays; falls back to
    /// `"(unnamed)"` when no name is set (e.g. restored from an old session
    /// file that predates the name field).
    pub fn session_display(&self) -> &str {
        self.session_name.as_deref().unwrap_or("(unnamed)")
    }

    /// Human-readable PROJECT_HOME path or `"(unset)"` when none.
    pub fn project_home_display(&self) -> String {
        self.project_home
            .as_ref()
            .map_or_else(|| "(unset)".to_string(), |p| crate::paths::display_tilde(p))
    }
}

/// Compact relative-age string for the graveyard view ("3m ago",
/// "2h ago", "yesterday", "2026-04-15"). Coarsened deliberately —
/// the user only needs to know "very recent" vs "older than today"
/// to find what they just deleted.
pub(super) fn format_age(epoch: u64) -> String {
    let now = crate::sysinfo::epoch_secs();
    let dt = now.saturating_sub(epoch);
    if dt < 60 {
        "just now".to_string()
    } else if dt < 60 * 60 {
        format!("{}m ago", dt / 60)
    } else if dt < 24 * 60 * 60 {
        format!("{}h ago", dt / 3600)
    } else if dt < 2 * 24 * 60 * 60 {
        "yesterday".to_string()
    } else if dt < 7 * 24 * 60 * 60 {
        format!("{}d ago", dt / (24 * 60 * 60))
    } else {
        // Older than a week → date stamp. Use jiff (already a dep)
        // to format Y-m-d in local TZ.
        match jiff::Timestamp::from_second(epoch as i64) {
            Ok(ts) => ts
                .to_zoned(jiff::tz::TimeZone::system())
                .strftime("%Y-%m-%d")
                .to_string(),
            Err(_) => "long ago".to_string(),
        }
    }
}

/// Count regular-file entries inside `dir`, **stopping once `cap` is reached**.
/// Used by the `R` confirm prompt to surface the blast radius before the user
/// types `y`. Symlinks count as a single entry (not followed) to match what
/// `remove_tree` will actually unlink.
///
/// Runs on the input thread (inside the pure `apply`), so it is bounded: the
/// old version was a fully-recursive, uncapped `read_dir` walk that froze
/// the event loop for the whole tree when the cursor sat on a `node_modules`
/// / `target/` (~100k+ files) — exactly the blocking-IO-on-input scaling
/// pitfall the project has been bitten by. Iterative bounded-DFS (no recursion
/// → no stack blow-up on a deep tree either), same early-termination shape as
/// [`crate::app::util::count_subdirs_capped`]. Returns a value `<= cap`; a
/// return of exactly `cap` means "at least `cap`" (the caller shows `N+`).
pub(super) fn count_files_in_dir_capped(dir: &Path, cap: u64) -> u64 {
    let mut n = 0u64;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        if n >= cap {
            break;
        }
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for ent in rd.flatten() {
            let Ok(md) = std::fs::symlink_metadata(ent.path()) else {
                continue;
            };
            if md.file_type().is_symlink() || md.is_file() {
                n += 1;
                if n >= cap {
                    return n;
                }
            } else if md.is_dir() {
                stack.push(ent.path());
            }
        }
    }
    n
}

/// Walk up from `start` looking for an enclosing `.git` (dir or
/// gitfile). Returns the directory containing the `.git/`, or
/// `None` if we hit the filesystem root without finding one.
/// Used by `AppState::update_repo_root` to cache the current repo
/// root, so every chdir within a project reuses the same root
/// without re-walking, and the git-status cache is invalidated only
/// when the root actually changes.
///
/// Filesystem-only (no `git rev-parse` subprocess) — a few `lstat`
/// calls per ancestor.
pub(super) fn find_repo_root(start: &Path) -> Option<std::path::PathBuf> {
    for ancestor in start.ancestors() {
        // Both `.git` directories (normal repo) and `.git` files
        // (worktrees, submodules, gitlink) count.
        if ancestor.join(".git").exists() {
            return Some(ancestor.to_path_buf());
        }
    }
    None
}

#[cfg(test)]
impl AppState {
    /// Canonical test `AppState` for a given cwd — the single fixture
    /// builder (REFACTOR_PLAN Phase -1 intent). `test_state()` and the
    /// `App` test harness (`App::test_app`) both go through this, so
    /// adding an `AppState` field touches exactly one place. History
    /// buckets use `harness_*` names; wrap callers in
    /// `crate::state::with_state_root` for an isolated state dir.
    pub(crate) fn test_default(cwd: std::path::PathBuf) -> Self {
        Self {
            listing: Listing::empty(cwd.clone()),
            picks: Picks::new(),
            inventory: Inventory::new(),
            marks: Marks::default(),
            masks: IgnoreMasks::default(),
            temp_filter: None,
            sort_order: crate::fs::listing::SortMode::Name,
            sort_reversed: false,
            view: View::Dir,
            cursor: Cursor::new(),
            resolver: Resolver::new(),
            user_keymap: UserKeymap::default(),
            config: Config::default(),
            mode: Mode::Normal,
            start_dir: cwd,
            project_home: None,
            session_name: None,
            prev_dir: None,
            last_search: None,
            last_captured_cmd: None,
            history: History::load_file("harness_h"),
            pane_history: History::load_file("harness_ph"),
            pane_cwd_history: History::load_file("harness_pch"),
            jump_history: History::load_file("harness_jh"),
            command_history: History::load_file("harness_ch"),
            flash: None,
            should_quit: false,
            quit_pending: None,
            git: GitState::default(),
            git_cache: GitCache::default(),
            harpoon_filter_set: std::collections::HashSet::new(),
            harpoon: None,
            pending_delete_preview: None,
            graveyard: Vec::new(),
            user_host: "test@host".to_string(),
            pending_new_tab_cmd: None,
            pending_worktrees: None,
            pending_sessions: None,
            frecency: Frecency::default(),
            focus: Focus::FileList,
            pane: PaneLayout {
                pane_height_pct: 30,
                ..Default::default()
            },
            rows: Vec::new(),
            grid_dims: GridDims {
                cols: 1,
                rows_per_col: 20,
            },
            list_generation: 0,
        }
    }
}

#[cfg(test)]
mod tests;
