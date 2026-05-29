//! Domain state for the application — everything testable without a terminal.
//!
//! `AppState` holds navigation, selection, filtering, bookmarks, input mode,
//! config, history, and cached info. Event handlers that operate on pure
//! domain logic live here; the `App` shell in `mod.rs` owns terminal state
//! (pager widget, pane tabs, pty handles) and delegates to `AppState`.

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::config::Config;
use crate::fs;
use crate::fs::Listing;
use crate::keymap::{Action, Resolver, UserKeymap};
use crate::state::{Cursor, Frecency, History, IgnoreMasks, Inventory, Mark, Marks, Picks};
use crate::ui::list_view::Grid;

use super::{
    FlashKind, FlashMessage, Matcher, Mode, PostAction, Prompt, PromptKind, RowData, View,
    row_from_entry,
};

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
    /// Return this `PostAction` to the event loop (e.g. `Spawn` for `$SHELL`).
    Post(PostAction),
    /// Caller must handle this action (terminal-touching).
    NotHandled,
}

/// Description of a pager to open, without importing UI types.
#[derive(Debug)]
pub struct PagerRequest {
    pub title: String,
    pub lines: PagerLines,
    pub columns: u8,
    /// When true, the pager height auto-shrinks to fit content (top edge
    /// stays anchored to the standard centered position; the box just
    /// grows shorter from the bottom). Line-number gutter is suppressed
    /// since it's noise for short summaries.
    pub fit_to_content: bool,
}

/// Content for a pager — either plain strings or pre-styled lines.
#[derive(Debug)]
pub enum PagerLines {
    Plain(Vec<String>),
}

/// Off-main-thread git-status work item. The chdir hot path sends
/// these to a background worker on cache miss so the UI returns
/// immediately; the worker spawns `git status --porcelain` (the
/// 200-500 ms operation on a ~110k-file index) and echoes results
/// back via `GitWorkerResult`.
#[derive(Debug)]
pub struct GitWorkerRequest {
    pub generation: u64,
    pub canonical: std::path::PathBuf,
    pub repo_root: std::path::PathBuf,
    pub huge: bool,
}

/// Worker reply. `generation` lets the main thread discard results
/// whose source chdir has been superseded. `raw` is `None` when the
/// `git status` spawn failed (not in a repo, git missing, etc.) —
/// the App treats that as "no markers" rather than a hard error.
#[derive(Debug)]
pub struct GitWorkerResult {
    pub generation: u64,
    pub repo_root: std::path::PathBuf,
    pub huge: bool,
    pub raw: Option<String>,
    pub index_mtime: Option<std::time::SystemTime>,
    pub head_mtime: Option<std::time::SystemTime>,
}

/// Cached raw `git status --porcelain` output, keyed by repo root +
/// `.git/index` / `.git/HEAD` mtimes. The status text doesn't depend
/// on the current listing dir — only on the repo state — so once we
/// have it, every chdir to a sibling/child path within the same repo
/// can re-parse it locally with a freshly-computed prefix instead of
/// spawning `git status` again. On a ~110k-file index that spawn is
/// 200-500 ms, dominating drill-in latency.
#[derive(Debug, Clone)]
pub struct GitStatusRawCache {
    pub repo_root: PathBuf,
    pub index_mtime: std::time::SystemTime,
    pub head_mtime: std::time::SystemTime,
    /// `true` if captured with `-uno` (huge-tree mode). Treated as a
    /// distinct cache shape: a small-tree consumer must NOT reuse a
    /// huge-tree capture (it would be missing `?` untracked entries).
    pub huge: bool,
    pub raw: String,
}

/// Canonical list of `:` command base names, used by the prompt's
/// Tab-completion path. Keep in sync with the matchers in
/// `dispatch_command` here and in `App::dispatch_command`. Sorted for
/// deterministic completion output.
///
/// Special prefixes (`!`, `;`) that take free-form shell arguments
/// are intentionally omitted — they're typed as one keystroke each
/// and don't benefit from name-completion.
pub const SPYC_COMMANDS: &[&str] = &[
    "bnext",
    "bprev",
    "cd",
    "date",
    "dump-scrollback",
    "fg",
    "graveyard",
    "grep",
    "limit",
    "marks",
    "name",
    "pane-to-task",
    "pause",
    "project",
    "q",
    "quit",
    "resume",
    "set",
    "sort",
    "startdir",
    "task",
    "task-to-pane",
    "undo",
    "version",
    "whoami",
];

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
    pub git_info: Option<String>,
    pub git_files: std::collections::HashMap<String, crate::ui::list_view::GitFileStatus>,
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
    /// Set at `chdir` when the new project root has more than
    /// `HUGE_TREE_SUBDIR_THRESHOLD` subdirs (measured with the
    /// bounded-DFS `count_subdirs_capped`). Drives three adaptive
    /// behaviors in the event loop:
    ///
    /// - Slower poll cadence: `REFRESH_QUIET` 500 ms → 3 s,
    ///   `GIT_POLL_INTERVAL` 1 s → 10 s.
    /// - Cheaper `git status`: `-uno` (skip untracked enumeration)
    ///   instead of `-unormal` — trade is no `?` markers for
    ///   untracked files on huge trees, in exchange for not
    ///   walking the entire working tree to enumerate them.
    /// - Initial flash on first detection so the user sees the
    ///   trade.
    ///
    /// Decision is cached by [`Self::huge_tree_anchor`]: drilling
    /// down or popping up within the same project reuses the
    /// previous decision without re-walking. Re-evaluated only
    /// when the chdir crosses into a different project root (or
    /// out of any repo). Without the cache, drilling into a
    /// deeply-nested Java package layout (`src/main/java/com/...`)
    /// took hundreds of ms per chdir as each step re-ran
    /// `count_subdirs_capped` against an increasingly large
    /// subtree.
    pub is_huge_tree: bool,
    /// Path that `is_huge_tree` is cached against — the repo root
    /// of the active project, or the canonical listing dir when
    /// not in a repo. `chdir` recomputes `is_huge_tree` only when
    /// the new dir resolves to a different anchor. Without this,
    /// every navigation step on a deeply-nested tree (typical
    /// Java package layout: `src/main/java/com/...`) would re-run
    /// `count_subdirs_capped` — observed as multi-hundred-ms
    /// drill-in latency on a ~110k-file project.
    pub huge_tree_anchor: Option<std::path::PathBuf>,
    /// Multi-slot cache of huge-tree decisions per anchor path,
    /// keyed on the same value that `huge_tree_anchor` would hold.
    /// `huge_tree_anchor` is a single-slot "current" pointer — it
    /// tells us whether the *active* project is huge — but the
    /// decision behind it is project-wide and stable. Caching every
    /// previously-computed decision means re-entering a project we
    /// already classified (after a brief excursion to its parent dir
    /// or to a sibling repo) skips `count_subdirs_capped` entirely.
    pub huge_tree_decisions: std::collections::HashMap<std::path::PathBuf, bool>,
    /// Repo root of the active project (the directory containing
    /// `.git`), or `None` when the listing dir isn't inside any
    /// repo. Updated alongside `huge_tree_anchor` in
    /// `update_huge_tree`. Used to compute the
    /// `git status --porcelain` prefix without spawning
    /// `git rev-parse --show-toplevel`, and as the cache key for
    /// `git_status_raw_cache`.
    pub current_repo_root: Option<std::path::PathBuf>,
    /// Cached raw output of `git status --porcelain`. On a huge
    /// working tree the subprocess walks every tracked file in the
    /// index — even with `-uno`, that's 200-500 ms per spawn on a
    /// ~110k-file repo. After the first chdir into a project,
    /// every subsequent chdir within that project's tree hits this
    /// cache (provided `.git/index` and `.git/HEAD` mtimes match)
    /// and skips the spawn entirely; only the per-listing-dir
    /// prefix re-parse is paid.
    pub git_status_raw_cache: Option<GitStatusRawCache>,
    /// Sender to the background git-status worker. `None` in tests
    /// or when the worker thread hasn't been initialized yet (the
    /// AppState constructor runs before the App-level worker spawn).
    /// `chdir` cache-miss paths enqueue requests; results arrive
    /// back via the App's `git_result_rx` and are gated on
    /// `git_generation` to discard stale ones.
    pub git_worker_tx: Option<std::sync::mpsc::Sender<GitWorkerRequest>>,
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
    /// Snapshot of the active harpoon ancestor-set (slot paths plus
    /// every parent directory of every slot). App refreshes this
    /// whenever the harpoon list mutates so `apply_temp_filter`
    /// remains pure-domain. Empty when no `PROJECT_HOME` is active.
    pub harpoon_filter_set: std::collections::HashSet<PathBuf>,
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
    pub pane_focused: bool,
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
    pub rows: Vec<RowData>,
    pub last_grid: Grid,
    /// Monotonic counter bumped whenever the display row list changes.
    /// Used by App to skip redundant `build_rows()` calls.
    pub list_generation: u64,
}

impl AppState {
    // --- Cursor/navigation (Phase 1) ---

    /// j/k — move within the current column only. Wraps at column
    /// boundaries. Returns false (flash) if the column has only one row.
    pub fn cursor_move_vertical(&mut self, delta: isize, len: usize) -> bool {
        if len == 0 {
            return false;
        }
        let rows_per_col = self.last_grid.rows.max(1) as usize;
        let col_start = (self.cursor.index / rows_per_col) * rows_per_col;
        let col_end = (col_start + rows_per_col).min(len);
        let col_len = col_end - col_start;
        if col_len <= 1 {
            return false; // single-item column — flash
        }
        let row_in_col = self.cursor.index - col_start;
        let new_row = (row_in_col as isize + delta).rem_euclid(col_len as isize) as usize;
        self.cursor.index = col_start + new_row;
        true
    }

    /// Move across the entire list (PageUp/PageDown). Wraps globally.
    pub const fn cursor_move_global(&mut self, delta: isize, len: usize) {
        if len == 0 {
            return;
        }
        let new_idx = (self.cursor.index as isize + delta).rem_euclid(len as isize);
        self.cursor.index = new_idx as usize;
    }

    /// `gg` — jump to the first entry of the current column.
    pub const fn goto_col_top(&mut self) {
        let rows_per_col = self.last_grid.rows as usize;
        if rows_per_col == 0 {
            self.cursor.index = 0;
            return;
        }
        let current_col = self.cursor.index / rows_per_col;
        self.cursor.index = current_col * rows_per_col;
    }

    /// `G` — jump to the last entry of the current column.
    pub fn goto_col_bottom(&mut self, len: usize) {
        if len == 0 {
            return;
        }
        let rows_per_col = self.last_grid.rows as usize;
        if rows_per_col == 0 {
            self.cursor.index = len - 1;
            return;
        }
        let current_col = self.cursor.index / rows_per_col;
        let col_end = ((current_col + 1) * rows_per_col).min(len);
        self.cursor.index = col_end - 1;
    }

    /// Move the cursor to the next (or previous) row whose git status
    /// is non-clean — i.e., a row carrying a `~`/`+`/`?`/`-` marker
    /// in the listing. Wraps around the end of the list (so a user
    /// can keep pressing `]g` without worrying about direction).
    /// Returns `false` when no row in the listing has a git change,
    /// so the caller can flash an empty-search message.
    pub fn jump_to_git_change(&mut self, forward: bool) -> bool {
        let len = self.rows.len();
        if len == 0 || self.git_files.is_empty() {
            return false;
        }
        let cur = self.cursor.index.min(len.saturating_sub(1));
        let is_changed = |idx: usize| -> bool {
            self.rows.get(idx).is_some_and(|r| {
                self.git_files
                    .get(&r.display)
                    .copied()
                    .is_some_and(|s| !s.is_clean())
            })
        };
        // Walk every other index, in the requested direction, with wrap.
        // `n` = 1..len means we never re-test the cursor's own row, so a
        // press from a dirty row advances to the *next* dirty one rather
        // than staying put.
        for n in 1..=len {
            let idx = if forward {
                (cur + n) % len
            } else {
                (cur + len - (n % len)) % len
            };
            if is_changed(idx) {
                self.cursor.index = idx;
                return true;
            }
        }
        false
    }

    /// Move the cursor by `delta` columns.
    /// h/l — move across columns on the same row. Wraps at row
    /// boundaries. Returns false (flash) if only one column on this row.
    pub fn cursor_move_columns(&mut self, delta: isize, rows_per_col: usize, len: usize) -> bool {
        if rows_per_col == 0 || len == 0 {
            return false;
        }
        let num_cols = len.div_ceil(rows_per_col);
        let current_col = self.cursor.index / rows_per_col;
        let current_row = self.cursor.index % rows_per_col;
        // Count how many columns actually have an item on this row.
        let cols_on_row = {
            let mut n = 0usize;
            for c in 0..num_cols {
                if c * rows_per_col + current_row < len {
                    n += 1;
                }
            }
            n
        };
        if cols_on_row <= 1 {
            return false; // single-column row — flash
        }
        // Wrap within the columns that exist on this row.
        let col_in_row = {
            // Map current_col to its ordinal among valid columns on this row.
            let mut ord = 0usize;
            for c in 0..current_col {
                if c * rows_per_col + current_row < len {
                    ord += 1;
                }
            }
            ord
        };
        let new_ord = (col_in_row as isize + delta).rem_euclid(cols_on_row as isize) as usize;
        // Map ordinal back to column index.
        let mut target_col = 0usize;
        let mut count = 0usize;
        for c in 0..num_cols {
            if c * rows_per_col + current_row < len {
                if count == new_ord {
                    target_col = c;
                    break;
                }
                count += 1;
            }
        }
        let target_idx = target_col * rows_per_col + current_row;
        if target_idx < len {
            self.cursor.index = target_idx;
        }
        true
    }

    pub fn ensure_cursor_visible(&mut self) {
        let per_page = self.last_grid.items_per_page();
        if per_page == 0 || self.rows.is_empty() {
            self.cursor.view_top = 0;
            return;
        }
        let page = self.cursor.index / per_page;
        self.cursor.view_top = page * per_page;
    }

    pub fn find_match(&self, query: &str, from: usize, backward: bool) -> Option<usize> {
        if self.rows.is_empty() {
            return None;
        }
        let matcher = Matcher::build(query);
        let n = self.rows.len();
        for step in 0..n {
            let i = if backward {
                (from + n - step) % n
            } else {
                (from + step) % n
            };
            if matcher.matches(&self.rows[i].display) {
                return Some(i);
            }
        }
        None
    }

    // --- Selection/data helpers (Phase 2) ---

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

    pub fn selection_paths(&self) -> Vec<&Path> {
        if self.view == View::Dir && !self.picks.is_empty() {
            self.picks.iter().map(PathBuf::as_path).collect()
        } else if let Some(row) = self.rows.get(self.cursor.index) {
            vec![row.path.as_path()]
        } else {
            Vec::new()
        }
    }

    pub fn set_mark(&mut self, letter: char) {
        let focus = self.rows.get(self.cursor.index).map(|r| r.path.clone());
        self.marks.set(
            letter,
            Mark {
                dir: self.listing.dir.clone(),
                focus,
            },
        );
        match self.marks.save() {
            Ok(()) => self.flash_info(format!("mark '{letter}' set")),
            Err(e) => self.flash_error(format!("mark saved in-memory only: {e}")),
        }
    }

    pub fn jump_to_mark(&mut self, letter: char) {
        let Some(mark) = self.marks.get(letter).cloned() else {
            self.flash_error(format!("mark '{letter}' not set"));
            return;
        };
        if let Err(e) = self.chdir(&mark.dir) {
            self.flash_error(format!("jump failed: {e}"));
            return;
        }
        if let Some(focus) = mark.focus {
            self.focus_on_path(&focus);
        }
        self.flash_info(format!("jumped to mark '{letter}'"));
    }

    pub fn toggle_pick_cursor(&mut self) {
        if self.view != View::Dir {
            return;
        }
        if let Some(row) = self.rows.get(self.cursor.index) {
            self.picks.toggle(&row.path);
            self.list_generation = self.list_generation.wrapping_add(1);
        }
    }

    pub fn toggle_all_picks(&mut self) {
        if self.view != View::Dir {
            return;
        }
        let any_unpicked = self.rows.iter().any(|r| !self.picks.contains(&r.path));
        if any_unpicked {
            for r in &self.rows {
                self.picks.insert(&r.path);
            }
        } else {
            self.picks.clear();
        }
        self.list_generation = self.list_generation.wrapping_add(1);
    }

    /// Yank files into the inventory cache. Takes picks if any, else
    /// cursor item. Only regular files are accepted.
    pub fn take(&mut self) -> Option<String> {
        if self.view != View::Dir {
            return None;
        }
        let to_take: Vec<PathBuf> = if !self.picks.is_empty() {
            self.picks.iter().cloned().collect()
        } else if let Some(row) = self.rows.get(self.cursor.index) {
            vec![row.path.clone()]
        } else {
            vec![]
        };
        let total = to_take.len();
        let (count, err) = self.inventory.yank_many(&to_take);
        self.rebuild_rows();
        let skipped = total - count;
        if count > 0 {
            let msg = if skipped > 0 {
                format!("yanked {count} file(s), skipped {skipped} (dirs/special)")
            } else {
                format!("yanked {count} file(s) to inventory")
            };
            return Some(msg);
        }
        err
    }

    /// Remove the cursor item from inventory (move to graveyard).
    pub fn drop_cursor(&mut self) {
        self.inventory.remove_at(self.cursor.index);
        self.rebuild_rows();
        self.cursor.clamp(self.rows.len());
    }

    pub fn toggle_inventory_view(&mut self) {
        self.view = match self.view {
            View::Dir | View::Graveyard => View::Inventory,
            View::Inventory => View::Dir,
        };
        // Leaving graveyard view drops the snapshot so a stale set
        // of entries can't bleed into a later open.
        self.graveyard.clear();
        self.cursor = Cursor::new();
        self.rebuild_rows();
    }

    /// Open the graveyard view: load a fresh snapshot from disk
    /// and switch the visible list. Toggle on second call.
    pub fn open_graveyard_view(&mut self) {
        if matches!(self.view, View::Graveyard) {
            self.graveyard.clear();
            self.view = View::Dir;
        } else {
            self.graveyard = crate::state::graveyard::Graveyard::load().entries;
            self.view = View::Graveyard;
        }
        self.cursor = Cursor::new();
        self.rebuild_rows();
    }

    pub fn focus_on_path(&mut self, path: &Path) {
        if let Some(i) = self.rows.iter().position(|r| r.path == path) {
            self.cursor.index = i;
        }
    }

    pub fn rebuild_rows(&mut self) {
        self.list_generation = self.list_generation.wrapping_add(1);
        self.rows = match self.view {
            View::Dir => {
                let base: Vec<RowData> = self
                    .listing
                    .entries
                    .iter()
                    .filter(|e| !self.masks.hides(&e.name))
                    .map(row_from_entry)
                    .collect();
                self.apply_temp_filter(base)
            }
            View::Inventory => self
                .inventory
                .items()
                .map(|item| RowData {
                    path: item.orig_path.clone(),
                    display: format!(
                        "{}  ← {}",
                        item.filename,
                        item.orig_path.parent().unwrap_or(Path::new("/")).display()
                    ),
                    kind: crate::fs::EntryKind::File,
                })
                .collect(),
            View::Graveyard => self
                .graveyard
                .iter()
                .map(|e| {
                    let glyph = match e.kind {
                        crate::state::graveyard::EntryKind::File => "[f]",
                        crate::state::graveyard::EntryKind::Dir => "[d]",
                        crate::state::graveyard::EntryKind::Symlink => "[l]",
                    };
                    let parent = e
                        .orig_path
                        .parent()
                        .map_or_else(|| "/".to_string(), |p| p.display().to_string());
                    let count_tag = if matches!(e.kind, crate::state::graveyard::EntryKind::Dir)
                        && e.file_count > 0
                    {
                        format!(" ({} files)", e.file_count)
                    } else {
                        String::new()
                    };
                    let age = format_age(e.timestamp);
                    let kind = match e.kind {
                        crate::state::graveyard::EntryKind::Dir => crate::fs::EntryKind::Dir,
                        _ => crate::fs::EntryKind::File,
                    };
                    RowData {
                        path: e.orig_path.clone(),
                        display: format!("{glyph} {}{count_tag} ({age})  ← {parent}", e.filename),
                        kind,
                    }
                })
                .collect(),
        };
        self.cursor.clamp(self.rows.len());
    }

    pub fn apply_temp_filter(&self, rows: Vec<RowData>) -> Vec<RowData> {
        let Some(ref pattern) = self.temp_filter else {
            return rows;
        };
        if pattern == "!" {
            rows.into_iter()
                .filter(|r| self.picks.contains(&r.path))
                .collect()
        } else if pattern == "h" {
            // Harpoon filter — keep entries whose absolute path is
            // in the project's harpoon set (slot paths plus all
            // their ancestor directories). Empty set → empty list.
            rows.into_iter()
                .filter(|r| self.harpoon_filter_set.contains(&r.path))
                .collect()
        } else if pattern == "git" {
            // Show only entries that appear in `git status` with a
            // non-Clean state. `git_files` keys files by basename and
            // also marks parent directories that contain changes
            // (basename + trailing `/`), so directories show up too —
            // useful for navigating into a subtree with edits.
            rows.into_iter()
                .filter(|r| {
                    self.git_files
                        .get(&r.display)
                        .copied()
                        .is_some_and(|s| !s.is_clean())
                })
                .collect()
        } else {
            let matcher = Matcher::build(pattern);
            rows.into_iter()
                .filter(|r| matcher.matches(&r.display))
                .collect()
        }
    }

    pub fn refresh_listing(&mut self) {
        match Listing::read(&self.listing.dir) {
            Ok(new) => {
                self.listing = new;
                // Refresh the top-bar branch/dirty string too — without
                // this the bar stays on `main` after edits and only
                // updates when the user changes directories. Event-
                // driven refresh would normally invalidate the raw
                // cache (file mtimes moved but `.git/index` may not
                // have — and we need fresh content for ` M`
                // markers).
                //
                // But: on huge trees, `git status` is 200-500 ms per
                // spawn, and an active filesystem (claude writing
                // findings, build outputs, even some IDE auto-saves)
                // can trip `refresh_listing` every 3 s — burning the
                // worker thread nonstop. Throttle the invalidation
                // (huge: 10 s, small: 1 s). The 1 Hz / 10 Hz safety
                // poll in `refresh_git_state` still catches
                // `.git/index` changes immediately; the only
                // trade-off is a small lag in working-tree ` M`
                // markers for edits within the throttle window.
                let throttle = if self.is_huge_tree {
                    std::time::Duration::from_secs(10)
                } else {
                    std::time::Duration::from_secs(1)
                };
                let should_invalidate = self
                    .last_git_invalidation
                    .is_none_or(|t| t.elapsed() >= throttle);
                if should_invalidate {
                    self.git_status_raw_cache = None;
                    self.last_git_invalidation = Some(std::time::Instant::now());
                }
                let dir = self.listing.dir.clone();
                let new_git_files = self.git_file_statuses_cached(&dir);
                let new_git_info = self.compute_git_info_fast();
                let mut new_keys: Vec<&str> = new_git_files.keys().map(String::as_str).collect();
                new_keys.sort_unstable();
                crate::spyc_debug!(
                    "refresh_listing: dir={} git_info: {:?} → {:?}, git_files: {} → {} (new={:?})",
                    self.listing.dir.display(),
                    self.git_info,
                    new_git_info,
                    self.git_files.len(),
                    new_git_files.len(),
                    new_keys,
                );
                self.git_info = new_git_info;
                self.git_files = new_git_files;
                self.rebuild_rows();
            }
            Err(e) => {
                crate::spyc_debug!(
                    "refresh_listing: Listing::read({}) failed: {e}",
                    self.listing.dir.display(),
                );
            }
        }
    }

    /// Re-poll just git state (`git_info` + `git_files`) and update
    /// only if it changed. Returns `true` iff anything was different.
    /// 1Hz safety net for FSEvents missing the `.git/index.lock` →
    /// `.git/index` rename on commit (inode replacement is the macOS
    /// FSEvents soft spot). The diff guard preserves the 0-dps-idle
    /// target: when nothing changed, we don't bump `list_generation`
    /// or request a repaint.
    ///
    /// **Mtime short-circuit.** Before spawning the two `git`
    /// subprocesses, stat `.git/index` + `.git/HEAD` and compare
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
        if key.is_some() && key == self.git_poll_cache {
            return false;
        }
        // mtime moved — invalidate the raw-status cache before going
        // through `git_file_statuses_cached`, which will re-spawn and
        // refill it on this dir.
        self.git_status_raw_cache = None;
        let listing_dir = self.listing.dir.clone();
        let new_git_files = self.git_file_statuses_cached(&listing_dir);
        let new_git_info = self.compute_git_info_fast();
        // Stash on success so the next idle poll skips the
        // subprocesses. Stat fail (e.g. shallow repo, .git missing)
        // ⇒ key is None and we'll keep running until it appears.
        self.git_poll_cache = key;
        if new_git_info == self.git_info && new_git_files == self.git_files {
            return false;
        }
        self.git_info = new_git_info;
        self.git_files = new_git_files;
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
    /// Caller must have already called [`Self::update_huge_tree`] for
    /// `canonical` so `current_repo_root` and `is_huge_tree` reflect
    /// the new dir.
    pub fn git_file_statuses_cached(
        &mut self,
        canonical: &Path,
    ) -> std::collections::HashMap<String, crate::ui::list_view::GitFileStatus> {
        let Some(repo_root) = self.current_repo_root.clone() else {
            // Not in a repo — nothing to do, no cache to maintain.
            return std::collections::HashMap::new();
        };
        let mtimes = self.compute_git_mtime_key_fast();
        // Decide whether to reuse the cached raw output.
        let reuse = self.git_status_raw_cache.as_ref().is_some_and(|c| {
            c.repo_root == repo_root
                && c.huge == self.is_huge_tree
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
            if let Some(tx) = self.git_worker_tx.as_ref() {
                self.git_generation = self.git_generation.wrapping_add(1);
                let _ = tx.send(GitWorkerRequest {
                    generation: self.git_generation,
                    canonical: canonical.to_path_buf(),
                    repo_root,
                    huge: self.is_huge_tree,
                });
                self.last_git_request_at = Some(std::time::Instant::now());
                return std::collections::HashMap::new();
            }
            // No worker (tests, App::new bootstrap) — fall through
            // to the synchronous spawn path below.
            self.git_status_raw_cache = None;
            if let Some(raw) =
                crate::sysinfo::git_status_porcelain_raw(canonical, self.is_huge_tree)
                && let Some((index_mtime, head_mtime)) = mtimes
            {
                self.git_status_raw_cache = Some(GitStatusRawCache {
                    repo_root,
                    index_mtime,
                    head_mtime,
                    huge: self.is_huge_tree,
                    raw,
                });
            }
        }
        // Parse with the prefix derived from the listing dir relative
        // to the repo root — no `git rev-parse --show-toplevel`
        // subprocess needed.
        let Some(cache) = self.git_status_raw_cache.as_ref() else {
            return std::collections::HashMap::new();
        };
        let prefix = canonical
            .strip_prefix(&cache.repo_root)
            .unwrap_or(Path::new(""))
            .to_string_lossy()
            .into_owned();
        crate::sysinfo::parse_porcelain_statuses(&cache.raw, &prefix)
    }

    /// Compute the `git_info` display string (`main`, `main*`,
    /// `abc1234`, `(no git)` — well, `None`) from cached state
    /// without spawning any subprocesses. Replaces
    /// `sysinfo::git_status`, which spawned both
    /// `git rev-parse --abbrev-ref HEAD` AND a full
    /// `git status --porcelain` (with `-unormal`, walking every
    /// untracked file on the 110k-file tree) per chdir.
    ///
    /// - Branch comes from `.git/HEAD` (or the gitfile pointer for
    ///   worktrees/submodules) — pure file IO.
    /// - Dirty flag comes from the raw porcelain we already cached
    ///   in [`Self::git_file_statuses_cached`]. Empty raw output ⇒
    ///   clean. Non-empty ⇒ dirty.
    ///
    /// Returns `None` if the listing dir isn't in a repo, mirroring
    /// the old `sysinfo::git_status` contract.
    pub fn compute_git_info_fast(&self) -> Option<String> {
        let repo_root = self.current_repo_root.as_ref()?;
        let gitdir = crate::sysinfo::resolve_gitdir(repo_root)?;
        let branch = crate::sysinfo::read_head_branch(&gitdir)?;
        // Only trust the raw cache for the dirty marker if it was
        // captured for *this* repo. Without the `c.repo_root` filter,
        // a worktree switch left the top-bar showing the previous
        // worktree's dirty state for a frame (until the async git
        // worker filled the new cache) — reported by Spencer as
        // "stale markers" after switching worktrees.
        let dirty = self
            .git_status_raw_cache
            .as_ref()
            .filter(|c| &c.repo_root == repo_root)
            .is_some_and(|c| !c.raw.is_empty());
        Some(if dirty { format!("{branch}*") } else { branch })
    }

    /// Stat `.git/index` and `.git/HEAD` against the cached repo
    /// root — the no-subprocess version of `git_mtime_key`. Used
    /// to seed `git_poll_cache` on chdir without spawning
    /// `git rev-parse --git-dir`.
    fn compute_git_mtime_key_fast(&self) -> Option<(std::time::SystemTime, std::time::SystemTime)> {
        let repo_root = self.current_repo_root.as_ref()?;
        let gitdir = crate::sysinfo::resolve_gitdir(repo_root)?;
        let index_mt = std::fs::metadata(gitdir.join("index"))
            .and_then(|m| m.modified())
            .ok()?;
        let head_mt = std::fs::metadata(gitdir.join("HEAD"))
            .and_then(|m| m.modified())
            .ok()?;
        Some((index_mt, head_mt))
    }

    /// Recompute `is_huge_tree` / `huge_tree_anchor` for the given
    /// canonical dir. The cached anchor (the active project's repo
    /// root, or the dir itself when not in a repo) short-circuits
    /// re-walking on every chdir within the same project — drilling
    /// into a deeply-nested Java package layout was observed taking
    /// hundreds of ms per step because each chdir re-ran the
    /// bounded-DFS walk over the package subtree.
    ///
    /// On a real anchor change (different repo / different non-repo
    /// dir), runs `count_subdirs_capped` and flashes if newly huge.
    /// Returns true if `is_huge_tree` is now set, mostly for tests.
    pub fn update_huge_tree(&mut self, canonical: &Path) -> bool {
        let repo_root = find_repo_root(canonical);
        let new_anchor = repo_root.clone().unwrap_or_else(|| canonical.to_path_buf());
        if self.huge_tree_anchor.as_ref() == Some(&new_anchor) {
            // Same project — keep the cached decision and skip the
            // walk. No flash either (we've already flashed this
            // project's first entry if it was huge).
            self.current_repo_root = repo_root;
            return self.is_huge_tree;
        }
        let was_huge = self.is_huge_tree;
        // Look up a previously-cached decision for this anchor before
        // walking. Multi-slot cache survives leave-and-return cycles
        // (drill into the huge project → up to its parent → back in)
        // that the single-slot `huge_tree_anchor` thrashed on,
        // forcing a fresh `count_subdirs_capped` walk every time.
        self.is_huge_tree = if let Some(&cached) = self.huge_tree_decisions.get(&new_anchor) {
            cached
        } else {
            let huge = crate::app::count_subdirs_capped(
                &new_anchor,
                crate::app::HUGE_TREE_SUBDIR_THRESHOLD,
            ) > crate::app::HUGE_TREE_SUBDIR_THRESHOLD;
            self.huge_tree_decisions.insert(new_anchor.clone(), huge);
            huge
        };
        self.huge_tree_anchor = Some(new_anchor);
        // Worktree-switch belt-and-suspenders: if we're crossing
        // into a *different* repo than the raw cache holds, wipe
        // the cache. `git_file_statuses_cached` does its own key
        // check that catches this on the marker path, but
        // `compute_git_info_fast` previously didn't, and Spencer
        // reported brief "stale dirty marker" flashes on worktree
        // switches. Clearing here is precautionary and explicit;
        // the cache check in `git_file_statuses_cached` still
        // covers the cache-survives-leave-and-return optimization
        // because going from a repo to a non-repo dir doesn't
        // satisfy `repo_root.is_some()` here, so the cache lives
        // on for re-entry.
        if let Some(new_root) = repo_root.as_ref() {
            if let Some(c) = self.git_status_raw_cache.as_ref() {
                if &c.repo_root != new_root {
                    self.git_status_raw_cache = None;
                }
            }
        }
        self.current_repo_root = repo_root;
        if self.is_huge_tree && !was_huge {
            self.flash_info(format!(
                "large tree ({}+ subdirs) — git poll throttled, untracked markers off",
                crate::app::HUGE_TREE_SUBDIR_THRESHOLD,
            ));
        }
        self.is_huge_tree
    }

    pub fn chdir(&mut self, path: &Path) -> Result<()> {
        let canonical = std::fs::canonicalize(path)?;
        let new_listing = Listing::read(&canonical)?;
        if self.listing.dir != canonical {
            self.prev_dir = Some(self.listing.dir.clone());
        }
        let _ = std::env::set_current_dir(&canonical);
        // If the directory had more than `MAX_ENTRIES`, the read
        // stopped early. Surface that to the user with a flash so a
        // partial listing isn't mistaken for the whole picture — the
        // alternative was the pre-fix behavior of hanging the event
        // loop for many seconds on a 1M-entry tmp dir.
        if new_listing.truncated {
            self.flash_info(format!(
                "listing capped at {} entries — directory has more",
                crate::fs::listing::MAX_ENTRIES
            ));
        }
        self.listing = new_listing;
        self.listing.sort(self.sort_order, self.sort_reversed);
        // Maybe re-evaluate "is this a huge tree?" — only runs the
        // bounded-DFS walk when the chdir crosses into a different
        // project (different repo root, or out of any repo). Drilling
        // around inside the same project inherits the cached value.
        // Must happen *before* the git calls below so they see the
        // right `huge` value on the first run after chdir.
        self.update_huge_tree(&canonical);
        // Refill the raw-status cache (if needed) before computing
        // branch/dirty — `compute_git_info_fast` reads `dirty` off
        // the cached raw output, so it must be current.
        self.git_files = self.git_file_statuses_cached(&canonical);
        self.git_info = self.compute_git_info_fast();
        // Cache key from the cached repo root — no subprocess. The
        // chdir implicitly switched repos if the new tree has a
        // different `.git/`, so seed the cache here rather than wait
        // for the next 1 Hz poll to detect the mismatch.
        self.git_poll_cache = self.compute_git_mtime_key_fast();
        self.picks.clear();
        self.temp_filter = None;
        self.cursor = Cursor::new();
        self.view = View::Dir;
        self.rebuild_rows();
        self.frecency.record(&canonical);
        Ok(())
    }

    pub fn climb(&mut self) {
        if self.view == View::Inventory {
            self.view = View::Dir;
            self.rebuild_rows();
            return;
        }
        let prev_name = self
            .listing
            .dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned());
        if let Some(parent) = self.listing.dir.parent().map(Path::to_path_buf) {
            if let Err(e) = self.chdir(&parent) {
                self.flash_error(format!("chdir: {e}"));
                return;
            }
            if let Some(name) = prev_name {
                if let Some(idx) = self
                    .rows
                    .iter()
                    .position(|r| r.display == name || r.display == format!("{name}/"))
                {
                    self.cursor.index = idx;
                }
            }
        }
    }

    // --- Action dispatch (pure-domain arms) ---

    /// Handle the pure-domain arms of `Action` dispatch.
    ///
    /// Returns `ApplyResult::Handled` when the action was fully processed
    /// (cursor is clamped before returning), `ApplyResult::OpenPager` when
    /// the caller should open a pager, `ApplyResult::Post` for a `PostAction`,
    /// or `ApplyResult::NotHandled` when the caller must handle the action
    /// (terminal-touching: pager, pane, theme, redraw, etc.).
    pub fn apply(&mut self, action: &Action) -> ApplyResult {
        let len = self.rows.len();
        let rows_per_col = self.last_grid.rows as usize;
        let per_page = self.last_grid.items_per_page();

        match action {
            // -- Cursor motion --
            Action::Up(n) => {
                if !self.cursor_move_vertical(-(*n as isize), len) {
                    self.flash_info("~");
                }
            }
            Action::Down(n) => {
                if !self.cursor_move_vertical(*n as isize, len) {
                    self.flash_info("~");
                }
            }
            Action::Left(n) => {
                if !self.cursor_move_columns(-(*n as isize), rows_per_col, len) {
                    self.flash_info("~");
                }
            }
            Action::Right(n) => {
                if !self.cursor_move_columns(*n as isize, rows_per_col, len) {
                    self.flash_info("~");
                }
            }
            Action::PageUp => self.cursor_move_global(-(per_page as isize), len),
            Action::PageDown => self.cursor_move_global(per_page as isize, len),
            Action::GotoFirst => self.goto_col_top(),
            Action::GotoLast => self.goto_col_bottom(len),

            // ]g / [g — cursor to next/prev git-changed entry. Wraps
            // when there's no match in the desired direction so the
            // user can keep pressing the chord without thinking about
            // direction. No-op flash when the listing has no changes.
            Action::JumpNextGitChange => {
                if !self.jump_to_git_change(true) {
                    self.flash_info("no git changes in this directory");
                }
            }
            Action::JumpPrevGitChange => {
                if !self.jump_to_git_change(false) {
                    self.flash_info("no git changes in this directory");
                }
            }

            // -- Navigation --
            Action::Climb => self.climb(),
            Action::Home => {
                if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
                    if let Err(e) = self.chdir(&home) {
                        self.flash_error(format!("chdir: {e}"));
                    }
                }
            }

            // -- Picks --
            Action::TogglePick => self.toggle_pick_cursor(),
            Action::PickPatternPrompt => {
                if self.view == View::Dir {
                    self.mode =
                        Mode::Prompting(Prompt::simple(PromptKind::PatternPick, "pick pattern: "));
                }
            }
            Action::PickToggleAll => self.toggle_all_picks(),

            // -- Inventory --
            Action::Take => match self.take() {
                Some(msg) if msg.starts_with("yanked") => self.flash_info(msg),
                Some(err) => self.flash_error(err),
                None => {}
            },
            Action::Untake => {
                if self.view != View::Dir {
                    return ApplyResult::Handled;
                }
                if let Some(row) = self.rows.get(self.cursor.index) {
                    let path = row.path.clone();
                    if self.inventory.contains(&path) {
                        // Find and remove by original path.
                        let id = self
                            .inventory
                            .items()
                            .find(|i| i.orig_path == path)
                            .map(|i| i.id.clone());
                        if let Some(id) = id {
                            self.inventory.remove_by_id(&id);
                            self.flash_info("removed from inventory");
                        }
                    } else {
                        self.flash_error("not in inventory");
                    }
                }
                self.rebuild_rows();
            }
            Action::Drop => {
                // In dir view, p = put (handled by App, not here).
                // This arm only fires from inventory view fallthrough.
                self.drop_cursor();
            }
            Action::ToggleInventoryView => self.toggle_inventory_view(),
            Action::EmptyInventory => {
                self.inventory.clear();
                self.rebuild_rows();
            }

            // -- Masks & filtering --
            Action::ToggleMask(n) => {
                if *n == 1 {
                    self.masks.toggle_mask1();
                } else if *n == 2 {
                    self.masks.toggle_mask2();
                }
                self.rebuild_rows();
            }
            Action::LimitPrompt => {
                let prefix = if self.temp_filter.is_some() {
                    "limit (active)="
                } else {
                    "limit="
                };
                self.mode = Mode::Prompting(Prompt::simple(PromptKind::Limit, prefix));
            }
            Action::CommandPrompt => {
                self.mode = Mode::Prompting(Prompt::shell(PromptKind::Command, ":"));
            }

            // -- Shell prompts --
            Action::ShellCapturedPrompt => {
                self.mode = Mode::Prompting(Prompt::shell(PromptKind::ShellCmdCaptured, "!"));
            }
            Action::ShellForegroundPrompt => {
                self.mode = Mode::Prompting(Prompt::shell(PromptKind::ShellCmd, ";"));
            }
            Action::StartShell => {
                let sh = crate::envset::var("SHELL").unwrap_or_else(|| "/bin/sh".into());
                return ApplyResult::Post(PostAction::Spawn {
                    program: sh,
                    args: vec![],
                    pause_after: false,
                });
            }

            // -- Search --
            Action::SearchPrompt => {
                self.mode = Mode::Prompting(Prompt::simple(
                    PromptKind::Search {
                        saved_cursor: self.cursor.index,
                    },
                    "/",
                ));
            }
            Action::SearchNext => {
                if let Some(term) = self.last_search.clone() {
                    let n = self.rows.len();
                    if n > 0 {
                        let start = (self.cursor.index + 1) % n;
                        if let Some(i) = self.find_match(&term, start, false) {
                            self.cursor.index = i;
                        }
                    }
                }
            }
            Action::SearchPrev => {
                if let Some(term) = self.last_search.clone() {
                    let n = self.rows.len();
                    if n > 0 {
                        let start = if self.cursor.index == 0 {
                            n - 1
                        } else {
                            self.cursor.index - 1
                        };
                        if let Some(i) = self.find_match(&term, start, true) {
                            self.cursor.index = i;
                        }
                    }
                }
            }

            // -- Navigation prompts --
            Action::JumpPrompt => {
                // Vi line editor so the user can pull up a history
                // entry (j/k in Normal, Up/Down anywhere) and tweak
                // it before submitting -- e.g. recall ~/src/spyc
                // and append `/src` before pressing Enter.
                self.mode = Mode::Prompting(Prompt::shell(PromptKind::Jump, "jump to: "));
            }

            // -- File operation prompts --
            Action::CopyPrompt => {
                if !self.selection_paths().is_empty() {
                    // `shell` constructor gives the prompt a vi line
                    // editor so the user can navigate / edit the
                    // destination path with familiar bindings (w b
                    // 0 $ cw etc.). Up/Down history nav is skipped
                    // for path prompts in `handle_vi_prompt_key`
                    // so the shell command history doesn't surface.
                    self.mode = Mode::Prompting(Prompt::shell(PromptKind::CopyTo, "copy to: "));
                }
            }
            Action::MovePrompt => {
                if !self.selection_paths().is_empty() {
                    self.mode = Mode::Prompting(Prompt::shell(PromptKind::MoveTo, "move to: "));
                }
            }
            Action::MakeDirPrompt => {
                self.mode = Mode::Prompting(Prompt::shell(PromptKind::MakeDir, "mkdir: "));
            }
            Action::NewFilePrompt => {
                self.mode = Mode::Prompting(Prompt::simple(PromptKind::NewFile, "new file: "));
            }
            Action::RemovePrompt(count) => {
                // `count.is_some()` = explicit `Ndd` from the user.
                // None = bare `R` or bare `dd` → picks-or-cursor
                // semantics (existing `R` behavior).
                let paths: Vec<PathBuf> = if let Some(n) = count {
                    // Cursor + (n-1) entries below, clamped at end
                    // of list. No wrap. Ignores picks — the count
                    // is the user being explicit.
                    let start = self.cursor.index;
                    self.rows
                        .iter()
                        .skip(start)
                        .take(*n)
                        .map(|r| r.path.clone())
                        .collect()
                } else {
                    self.selection_paths()
                        .into_iter()
                        .map(Path::to_path_buf)
                        .collect()
                };
                if paths.is_empty() {
                    return ApplyResult::Handled;
                }
                // Borrow the slice for the rest of the function.
                let paths: Vec<&Path> = paths.iter().map(PathBuf::as_path).collect();
                // Pre-walk to count files inside any selected dirs so
                // the user sees the actual blast radius of `R`. Cheap
                // (interactive flow, sub-second on any sane subtree)
                // and load-bearing for safety: today's prompt just
                // says "N file(s)?" which a user can reflexively `y`
                // their way through, even if N includes a directory
                // tree that would recursively delete thousands.
                let mut file_count = 0u64;
                let mut dir_count = 0u64;
                let mut dir_files = 0u64;
                for p in &paths {
                    match std::fs::symlink_metadata(p) {
                        Ok(md) if md.is_dir() => {
                            dir_count += 1;
                            dir_files += count_files_in_dir(p);
                        }
                        _ => file_count += 1,
                    }
                }
                let prompt = if dir_count == 0 {
                    format!("remove {file_count} file(s)? (y/N): ")
                } else if file_count == 0 && dir_count == 1 {
                    format!("remove DIR (recursive, {dir_files} file(s))? (y/N): ")
                } else if file_count == 0 {
                    format!("remove {dir_count} dir(s) (recursive, {dir_files} file(s))? (y/N): ")
                } else {
                    format!(
                        "remove {file_count} file(s) + {dir_count} dir(s) (recursive, {dir_files} file(s))? (y/N): "
                    )
                };
                // Capture the targeted paths so the list view can
                // highlight them in the warning color while the
                // confirm prompt is up. Cleared on confirm/cancel
                // in `handle_remove_confirm_key`.
                self.pending_delete_preview =
                    Some(paths.iter().map(|p| (*p).to_path_buf()).collect());
                self.mode = Mode::Prompting(Prompt::simple(PromptKind::RemoveConfirm, prompt));
            }

            // -- Long listing (pager) --
            Action::LongList => {
                let owned: Vec<PathBuf>;
                let paths: Vec<&Path> = if self.selection_paths().is_empty() {
                    owned = self
                        .listing
                        .entries
                        .iter()
                        .map(|e| e.path.clone())
                        .collect();
                    owned.iter().map(PathBuf::as_path).collect()
                } else {
                    self.selection_paths()
                };
                let lines = fs::ops::format_long_listing(&paths);
                let title = format!("long listing — {}", self.listing.dir.display());
                self.cursor.clamp(self.rows.len());
                return ApplyResult::OpenPager(PagerRequest {
                    title,
                    lines: PagerLines::Plain(lines),
                    columns: 1,
                    fit_to_content: true,
                });
            }

            // -- File type --
            Action::FileType => {
                let paths = self.selection_paths();
                if paths.is_empty() {
                    self.cursor.clamp(self.rows.len());
                    return ApplyResult::Post(PostAction::None);
                }
                if paths.len() == 1 {
                    let label = fs::ops::file_type_label(paths[0]);
                    let name = paths[0].file_name().map_or_else(
                        || paths[0].display().to_string(),
                        |n| n.to_string_lossy().into_owned(),
                    );
                    self.flash_info(format!("{name}: {label}"));
                } else {
                    let lines: Vec<String> = paths
                        .iter()
                        .map(|p| {
                            let name = p.file_name().map_or_else(
                                || p.display().to_string(),
                                |n| n.to_string_lossy().into_owned(),
                            );
                            format!("{name}: {}", fs::ops::file_type_label(p))
                        })
                        .collect();
                    self.cursor.clamp(self.rows.len());
                    return ApplyResult::OpenPager(PagerRequest {
                        title: "file types".to_string(),
                        lines: PagerLines::Plain(lines),
                        columns: 1,
                        fit_to_content: false,
                    });
                }
            }

            // -- Marks --
            Action::SetMark(letter) => self.set_mark(*letter),
            Action::JumpMark(letter) => self.jump_to_mark(*letter),
            Action::JumpStartDir => {
                let dir = self.start_dir.clone();
                if let Err(e) = self.chdir(&dir) {
                    self.flash_error(format!("jump to start failed: {e}"));
                }
            }
            Action::JumpProjectHome => match self.project_home.clone() {
                Some(dir) => {
                    if let Err(e) = self.chdir(&dir) {
                        self.flash_error(format!("jump to project home failed: {e}"));
                    }
                }
                None => self.flash_error("PROJECT_HOME not set (gP to set, :project)"),
            },
            Action::SetProjectHomeHere => {
                let dir = self.listing.dir.clone();
                self.flash_info(format!("PROJECT_HOME: {}", dir.display()));
                self.project_home = Some(dir);
            }
            Action::SetStartDirHere => {
                let dir = self.listing.dir.clone();
                self.flash_info(format!("start dir: {}", dir.display()));
                self.start_dir = dir;
            }
            Action::ShowUserHost => self.flash_info(self.user_host.clone()),
            Action::JumpPrevDir => {
                if let Some(prev) = self.prev_dir.clone() {
                    if let Err(e) = self.chdir(&prev) {
                        self.flash_error(format!("jump back failed: {e}"));
                    }
                } else {
                    self.flash_error("no previous directory");
                }
            }

            // -- Info --
            Action::Date => self.flash_info(crate::sysinfo::format_now()),
            Action::Version => {
                self.flash_info(format!(
                    "\u{1f336}\u{fe0f} spyc {}",
                    env!("CARGO_PKG_VERSION")
                ));
            }
            Action::SetEnvPrompt => {
                self.mode =
                    Mode::Prompting(Prompt::simple(PromptKind::SetEnv, "setenv NAME=VALUE: "));
            }

            // -- Worktree prompts (pure state: just set mode) --
            Action::WorktreeNew => {
                if self.git_info.is_none() {
                    self.flash_error("not in a git repository");
                } else {
                    let p = Prompt::shell(PromptKind::WorktreeNewBranch, "worktree branch: ");
                    self.mode = Mode::Prompting(p);
                }
            }
            Action::WorktreeDelete => {
                if self.git_info.is_none() {
                    self.flash_error("not in a git repository");
                } else {
                    let dir = self.listing.dir.display().to_string();
                    self.mode = Mode::Prompting(Prompt::simple(
                        PromptKind::WorktreeDeleteConfirm,
                        format!("remove worktree {dir}? (y/N): "),
                    ));
                }
            }

            // -- No-op --
            Action::Noop => {}

            // -- Reserved keys (flash a hint instead of doing something
            //    unintended; the actual feature is on the roadmap) --
            Action::MacroRecordReserved => {
                self.flash_info("q reserved for future macro recording — Q or :q to quit");
            }

            // -- Everything else stays in App --
            _ => return ApplyResult::NotHandled,
        }

        self.cursor.clamp(self.rows.len());
        ApplyResult::Handled
    }

    // --- Command / prompt dispatch (pure-domain arms) ---

    /// Handle the pure-domain arms of `:` commands.
    ///
    /// (See `SPYC_COMMANDS` for the canonical list of base names —
    /// kept right next to the dispatcher so the two stay in sync.)
    ///
    /// Returns `CommandResult::Handled` when the command was fully processed,
    /// `CommandResult::OpenPager` when the caller should open the pager with
    /// the supplied lines, or `CommandResult::NotHandled` when the caller
    /// (which owns the terminal) must process it.
    pub fn dispatch_command(&mut self, input: &str) -> CommandResult {
        let input = input.trim();
        if input.is_empty() {
            return CommandResult::Handled;
        }

        // :q / :quit — defer to App so the path matches Action::Quit
        // exactly (double-tap confirm, running-process warning, and
        // save_session on confirm). Setting should_quit from
        // pure-domain would skip pane teardown + session persistence.
        // The typed `Quit` variant forces the App-side match to
        // handle it; dropping that arm is a compile error.
        if input == "q" || input == "quit" {
            return CommandResult::Quit;
        }

        // :limit [pattern]
        if input == "limit" {
            self.temp_filter = None;
            self.flash_info("limit cleared");
            self.rebuild_rows();
            return CommandResult::Handled;
        }
        if let Some(pat) = input.strip_prefix("limit ") {
            let pat = pat.trim();
            if pat.is_empty() {
                self.temp_filter = None;
                self.flash_info("limit cleared");
            } else if pat == "!" {
                self.temp_filter = Some("!".to_string());
                self.flash_info("limit: picks only");
            } else {
                self.temp_filter = Some(pat.to_string());
                self.flash_info(format!("limit: {pat}"));
            }
            self.rebuild_rows();
            return CommandResult::Handled;
        }

        // :cd <path>
        if input == "cd" {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
            match self.chdir(&PathBuf::from(home)) {
                Ok(()) => {}
                Err(e) => self.flash_error(format!("cd: {e}")),
            }
            return CommandResult::Handled;
        }
        if let Some(raw) = input.strip_prefix("cd ") {
            let raw = raw.trim();
            if raw.is_empty() {
                self.flash_error("cd: missing path");
                return CommandResult::Handled;
            }
            let path = crate::paths::expand(raw);
            match self.chdir(&path) {
                Ok(()) => {}
                Err(e) => self.flash_error(format!("cd: {e}")),
            }
            return CommandResult::Handled;
        }

        // :sort [mode]
        if input == "sort" {
            self.flash_info(format!("sort: {}", self.sort_order));
            return CommandResult::Handled;
        }
        if let Some(rest) = input.strip_prefix("sort ") {
            let rest = rest.trim();
            // `:sort reverse` / `:sort -` toggles direction.
            if rest == "reverse" || rest == "-" {
                self.sort_reversed = !self.sort_reversed;
                self.listing.sort(self.sort_order, self.sort_reversed);
                self.rebuild_rows();
                self.flash_info(format!(
                    "sort: {}{}",
                    self.sort_order,
                    if self.sort_reversed {
                        " (reversed)"
                    } else {
                        ""
                    },
                ));
                return CommandResult::Handled;
            }
            match crate::fs::listing::SortMode::parse(rest) {
                Some(mode) => {
                    self.sort_order = mode;
                    self.listing.sort(mode, self.sort_reversed);
                    self.rebuild_rows();
                    self.flash_info(format!(
                        "sort: {mode}{}",
                        if self.sort_reversed {
                            " (reversed)"
                        } else {
                            ""
                        },
                    ));
                }
                None => self.flash_error(format!(
                    "unknown sort mode: {rest} (name|size|mtime|ext|reverse)"
                )),
            }
            return CommandResult::Handled;
        }

        // :version
        if input == "version" {
            self.flash_info(format!(
                "\u{1f336}\u{fe0f} spyc {}",
                env!("CARGO_PKG_VERSION")
            ));
            return CommandResult::Handled;
        }

        // :whoami — flash user@host.
        if input == "whoami" {
            self.flash_info(self.user_host.clone());
            return CommandResult::Handled;
        }

        // :startdir [.|<path>] — manage the `` ` `` jump target.
        if input == "startdir" {
            self.flash_info(format!("start dir: {}", self.start_dir.display()));
            return CommandResult::Handled;
        }
        if let Some(arg) = input.strip_prefix("startdir ") {
            match self.resolve_dir_arg(arg.trim()) {
                Ok(canon) => {
                    self.flash_info(format!("start dir: {}", canon.display()));
                    self.start_dir = canon;
                }
                Err(e) => self.flash_error(format!("startdir: {e}")),
            }
            return CommandResult::Handled;
        }

        // :project [.|<path>|clear] — manage PROJECT_HOME.
        if input == "project" {
            self.flash_info(format!("PROJECT_HOME: {}", self.project_home_display()));
            return CommandResult::Handled;
        }
        if let Some(arg) = input.strip_prefix("project ") {
            let arg = arg.trim();
            if arg == "clear" {
                self.project_home = None;
                self.flash_info("PROJECT_HOME cleared");
                return CommandResult::Handled;
            }
            match self.resolve_dir_arg(arg) {
                Ok(canon) => {
                    self.flash_info(format!("PROJECT_HOME: {}", canon.display()));
                    self.project_home = Some(canon);
                }
                Err(e) => self.flash_error(format!("project: {e}")),
            }
            return CommandResult::Handled;
        }

        // :name [NEW] — rename session, or print current name when bare.
        if input == "name" {
            self.flash_info(format!("session name: {}", self.session_display()));
            return CommandResult::Handled;
        }
        if let Some(arg) = input.strip_prefix("name ") {
            match crate::state::session_names::normalize(arg) {
                Some(norm) => {
                    self.flash_info(format!("session name: {norm}"));
                    self.session_name = Some(norm);
                }
                None => self.flash_error("name: empty after normalization"),
            }
            return CommandResult::Handled;
        }

        // :marks
        if input == "marks" {
            if self.marks.entries.is_empty() {
                self.flash_info("no marks set");
                return CommandResult::Handled;
            }
            let lines: Vec<String> = self
                .marks
                .entries
                .iter()
                .map(|(key, mark)| {
                    let focus = match &mark.focus {
                        Some(p) => format!("  → {}", p.display()),
                        None => String::new(),
                    };
                    format!("  {key}  {}{focus}", mark.dir.display())
                })
                .collect();
            return CommandResult::OpenPager {
                title: "marks".to_string(),
                lines,
            };
        }

        // :set key=value
        if input == "set" {
            self.flash_error("usage: :set key=value");
            return CommandResult::Handled;
        }
        if let Some(assignment) = input.strip_prefix("set ") {
            let assignment = assignment.trim();
            if let Some((key, value)) = assignment.split_once('=') {
                let key = key.trim();
                let value = value.trim();
                match key {
                    "sort" => match crate::fs::listing::SortMode::parse(value) {
                        Some(mode) => {
                            self.sort_order = mode;
                            self.listing.sort(mode, self.sort_reversed);
                            self.rebuild_rows();
                            self.flash_info(format!("sort={mode}"));
                        }
                        None => self.flash_error(format!("invalid sort mode: {value}")),
                    },
                    _ => self.flash_error(format!("unknown setting: {key}")),
                }
            } else {
                self.flash_error("usage: :set key=value");
            }
            return CommandResult::Handled;
        }

        // Commands that need terminal/pager/overlay: :!cmd, :!!, :;cmd,
        // :bprev, :bnext, :fg [N], :task [N], :grep <pattern>,
        // :pause [N], :resume [N]
        if input.starts_with('!')
            || input.starts_with(';')
            || input == "bprev"
            || input == "bnext"
            || input == "fg"
            || input.starts_with("fg ")
            || input == "task"
            || input.starts_with("task ")
            || input == "task-to-pane"
            || input.starts_with("task-to-pane ")
            || input == "pane-to-task"
            || input.starts_with("pane-to-task ")
            || input == "grep"
            || input.starts_with("grep ")
            || input == "pause"
            || input.starts_with("pause ")
            || input == "resume"
            || input.starts_with("resume ")
            || input == "undo"
            || input == "graveyard"
            || input == "date"
            || input == "dump-scrollback"
        {
            return CommandResult::NotHandled;
        }

        // Unknown command
        self.flash_error(format!("unknown command: {input}"));
        CommandResult::Handled
    }

    /// Handle the pure-domain arms of prompt submission.
    ///
    /// Returns `PromptResult::Handled` when fully processed, or
    /// `PromptResult::NotHandled` when the caller must handle it (terminal I/O).
    pub fn dispatch_prompt(&mut self, kind: &PromptKind, buffer: &str) -> PromptResult {
        match kind {
            PromptKind::PatternPick => {
                if let Ok(pat) = glob::Pattern::new(buffer) {
                    for e in &self.listing.entries {
                        if pat.matches(&e.name) {
                            self.picks.insert(&e.path);
                        }
                    }
                    self.list_generation = self.list_generation.wrapping_add(1);
                }
                PromptResult::Handled
            }
            PromptKind::Search { .. } => {
                if !buffer.is_empty() {
                    self.last_search = Some(buffer.to_string());
                }
                PromptResult::Handled
            }
            PromptKind::Jump => {
                let trimmed = buffer.trim();
                if !trimmed.is_empty() {
                    let _ = self.jump_to(trimmed);
                }
                PromptResult::Handled
            }
            PromptKind::MakeDir => {
                let name = buffer.trim();
                if !name.is_empty() {
                    let target = crate::paths::expand(name);
                    let resolved = if target.is_absolute() {
                        target
                    } else {
                        self.listing.dir.join(&target)
                    };
                    match std::fs::create_dir_all(&resolved) {
                        Ok(()) => self.flash_info(format!("created {}", resolved.display())),
                        Err(e) => self.flash_error(format!("error: {e}")),
                    }
                    self.refresh_listing();
                }
                PromptResult::Handled
            }
            PromptKind::SetEnv => {
                let line = buffer.trim();
                if let Some((name, value)) = line.split_once('=') {
                    let name = name.trim();
                    if name.is_empty() {
                        self.flash_error("setenv: missing variable name");
                    } else {
                        // Record a runtime override instead of mutating the
                        // process env: `std::env::set_var` is unsound now
                        // that worker threads may read env concurrently.
                        // `envset` layers this over the real environment and
                        // is merged into every child spawn. See `crate::envset`.
                        crate::envset::set(name, value);
                        self.flash_info(format!("setenv {name}={value}"));
                    }
                } else if !line.is_empty() {
                    self.flash_error("setenv: expected NAME=VALUE");
                }
                PromptResult::Handled
            }
            PromptKind::Limit => {
                let pattern = buffer.trim();
                if pattern.is_empty() {
                    self.temp_filter = None;
                    self.flash_info("limit cleared");
                } else if pattern == "!" {
                    self.temp_filter = Some("!".to_string());
                    self.flash_info("limit: picks only");
                } else if pattern == "h" || pattern == "harpoon" {
                    if self.harpoon_filter_set.is_empty() {
                        self.flash_error(
                            "harpoon empty (or PROJECT_HOME unset) — nothing to filter",
                        );
                        return PromptResult::Handled;
                    }
                    self.temp_filter = Some("h".to_string());
                    self.flash_info("limit: harpoon");
                } else if pattern == "git" || pattern == "g" {
                    if self.git_files.is_empty() {
                        self.flash_error("not in a git repo (or no changes)");
                        return PromptResult::Handled;
                    }
                    self.temp_filter = Some("git".to_string());
                    self.flash_info("limit: git changes");
                } else {
                    self.temp_filter = Some(pattern.to_string());
                    self.flash_info(format!("limit: {pattern}"));
                }
                self.rebuild_rows();
                PromptResult::Handled
            }
            PromptKind::WorktreeNewBranch => {
                let branch = buffer.trim();
                if branch.is_empty() {
                    return PromptResult::Handled;
                }
                match crate::sysinfo::git_worktree_add(&self.listing.dir, branch) {
                    Ok(path) => {
                        self.flash_info(format!("created worktree: {}", path.display()));
                        if let Err(e) = self.chdir(&path) {
                            self.flash_error(format!("chdir: {e}"));
                        } else {
                            // Re-anchor PROJECT_HOME on the new
                            // worktree — same reasoning as the
                            // `W l` picker (see
                            // `App::handle_pager_key`):
                            // harpoon / grep / MCP context all
                            // want the worktree root, not the
                            // parent repo. App-side
                            // `reconcile_harpoon` runs on the
                            // next `apply`/`dispatch_prompt`
                            // boundary and reloads the per-project
                            // harpoon list.
                            self.project_home = Some(self.listing.dir.clone());
                        }
                    }
                    Err(e) => self.flash_error(format!("worktree add: {e}")),
                }
                PromptResult::Handled
            }
            PromptKind::WorktreeDeleteConfirm => {
                let confirmed = buffer.trim().eq_ignore_ascii_case("y");
                if !confirmed {
                    return PromptResult::Handled;
                }
                let dir = self.listing.dir.clone();
                // Capture the main repo path *before* removing — once the
                // worktree's directory is gone we can't `git worktree
                // list` from inside it anymore, and the chdir-to-parent
                // below typically lands in a non-git directory
                // (`~/src/spyc-worktrees/`) so PROJECT_HOME would have
                // no home to reanchor on. The main worktree is always
                // the first entry of `git worktree list --porcelain`.
                let main_repo = crate::sysinfo::git_worktree_list(&dir)
                    .and_then(|wts| wts.into_iter().next().map(|wt| wt.path));
                match crate::sysinfo::git_worktree_remove(&dir) {
                    Ok(()) => {
                        self.flash_info(format!("removed worktree: {}", dir.display()));
                        if let Some(parent) = dir.parent() {
                            let _ = self.chdir(parent);
                        }
                        // Re-anchor PROJECT_HOME on the main repo so
                        // harpoon / MCP context / `gh` don't keep
                        // pointing at the just-deleted directory. The
                        // chdir target stays the parent (existing
                        // behavior — the user might be browsing other
                        // sibling worktrees there); listing.dir and
                        // project_home can differ, that's normal.
                        // App::dispatch_prompt's Handled arm reloads
                        // harpoon for whatever project_home points
                        // at after this returns.
                        self.project_home = main_repo;
                    }
                    Err(e) => self.flash_error(format!("worktree remove: {e}")),
                }
                PromptResult::Handled
            }
            PromptKind::PaneNewTabCmd => {
                let cmd = buffer.trim().to_string();
                if cmd.is_empty() {
                    return PromptResult::Handled;
                }
                self.pending_new_tab_cmd = Some(cmd);
                let cwd_default = self.listing.dir.display().to_string();
                let mut p = Prompt::shell(PromptKind::PaneNewTabCwd, "pane cwd: ");
                p.buffer.clone_from(&cwd_default);
                if let Some(ed) = p.editor.as_mut() {
                    ed.set_content(&cwd_default);
                }
                self.mode = Mode::Prompting(p);
                PromptResult::Handled
            }
            PromptKind::RemoveConfirm
            | PromptKind::ClaudeCrashRecover { .. }
            | PromptKind::GraveyardPurgeAllConfirm => PromptResult::Handled,
            // These need terminal/overlay/pager — caller handles them.
            PromptKind::NewFile
            | PromptKind::ShellCmd
            | PromptKind::ShellCmdCaptured
            | PromptKind::CopyTo
            | PromptKind::MoveTo
            | PromptKind::PaneNewTabCwd
            | PromptKind::PaneRenameTab
            | PromptKind::Command => PromptResult::NotHandled,
        }
    }

    /// Resolve a `:project`/`:startdir` argument to an absolute directory.
    /// Accepts `.` (current listing dir), `~`-expanded paths, absolute paths,
    /// or relative paths (resolved against the listing dir). Rejects files
    /// and non-existent paths with a descriptive error.
    pub fn resolve_dir_arg(&self, arg: &str) -> std::result::Result<PathBuf, String> {
        let target = if arg == "." {
            self.listing.dir.clone()
        } else {
            crate::paths::expand(arg)
        };
        let abs = if target.is_absolute() {
            target
        } else {
            self.listing.dir.join(&target)
        };
        let canon = std::fs::canonicalize(&abs).map_err(|e| e.to_string())?;
        if !canon.is_dir() {
            return Err(format!("not a directory: {}", abs.display()));
        }
        Ok(canon)
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

    pub fn jump_to(&mut self, target: &str) -> Result<()> {
        let expanded = crate::paths::expand(target);
        let abs = if expanded.is_absolute() {
            expanded
        } else {
            self.listing.dir.join(&expanded)
        };
        let canonical = std::fs::canonicalize(&abs)?;
        let md = std::fs::metadata(&canonical)?;
        if md.is_dir() {
            if let Err(e) = self.chdir(&canonical) {
                self.flash_error(format!("chdir: {e}"));
                return Ok(());
            }
        } else if let Some(parent) = canonical.parent() {
            if let Err(e) = self.chdir(parent) {
                self.flash_error(format!("chdir: {e}"));
                return Ok(());
            }
            self.focus_on_path(&canonical);
        }
        Ok(())
    }
}

/// Compact relative-age string for the graveyard view ("3m ago",
/// "2h ago", "yesterday", "2026-04-15"). Coarsened deliberately —
/// the user only needs to know "very recent" vs "older than today"
/// to find what they just deleted.
fn format_age(epoch: u64) -> String {
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

/// Recursively count regular-file entries inside `dir`. Used by the
/// `R` confirm prompt to surface the actual blast radius before the
/// user types `y`. Symlinks count as a single entry (not followed)
/// to match what `remove_tree` will actually unlink.
fn count_files_in_dir(dir: &Path) -> u64 {
    let mut n = 0u64;
    let Ok(rd) = std::fs::read_dir(dir) else {
        return n;
    };
    for ent in rd.flatten() {
        let Ok(md) = std::fs::symlink_metadata(ent.path()) else {
            continue;
        };
        if md.file_type().is_symlink() || md.is_file() {
            n += 1;
        } else if md.is_dir() {
            n += count_files_in_dir(&ent.path());
        }
    }
    n
}

/// Walk up from `start` looking for an enclosing `.git` (dir or
/// gitfile). Returns the directory containing the `.git/`, or
/// `None` if we hit the filesystem root without finding one.
/// Used as the cache key for `AppState::update_huge_tree` — the
/// huge-tree decision is project-wide, so anchoring on the repo
/// root lets every chdir within that project reuse the same
/// determination without re-walking the subtree.
///
/// Filesystem-only (no `git rev-parse` subprocess) — a few `lstat`
/// calls per ancestor.
fn find_repo_root(start: &Path) -> Option<std::path::PathBuf> {
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
mod tests {
    use super::*;
    use crate::fs::entry::EntryKind;
    use crate::fs::listing::SortMode;

    /// Build a minimal `AppState` for testing. Uses an empty listing
    /// and sensible defaults — no disk I/O, no terminal.
    fn test_state() -> AppState {
        AppState {
            listing: Listing::empty(PathBuf::from("/tmp/test")),
            picks: Picks::new(),
            inventory: Inventory::new(),
            marks: Marks::default(),
            masks: IgnoreMasks::default(),
            temp_filter: None,
            sort_order: SortMode::Name,
            sort_reversed: false,
            view: View::Dir,
            cursor: Cursor::new(),
            resolver: Resolver::new(),
            user_keymap: UserKeymap::default(),
            config: Config::default(),
            mode: Mode::Normal,
            start_dir: PathBuf::from("/tmp/test"),
            project_home: None,
            session_name: None,
            prev_dir: None,
            last_search: None,
            last_captured_cmd: None,
            history: History::load_file("test_state_h"),
            pane_history: History::load_file("test_state_ph"),
            pane_cwd_history: History::load_file("test_state_pch"),
            jump_history: History::load_file("test_state_jh"),
            command_history: History::load_file("test_state_ch"),
            flash: None,
            should_quit: false,
            quit_pending: None,
            git_info: None,
            git_files: std::collections::HashMap::new(),
            git_poll_cache: None,
            is_huge_tree: false,
            huge_tree_anchor: None,
            huge_tree_decisions: std::collections::HashMap::new(),
            current_repo_root: None,
            git_status_raw_cache: None,
            git_worker_tx: None,
            git_generation: 0,
            last_git_invalidation: None,
            last_git_request_at: None,
            harpoon_filter_set: std::collections::HashSet::new(),
            pending_delete_preview: None,
            graveyard: Vec::new(),
            user_host: "test@host".to_string(),
            pending_new_tab_cmd: None,
            pending_worktrees: None,
            pending_sessions: None,
            frecency: Frecency::default(),
            pane_focused: false,
            pane_height_pct: 30,
            pane_zoomed: false,
            pane_focus_before_zoom: None,
            pane_hidden: false,
            rows: Vec::new(),
            last_grid: Grid {
                cols: 1,
                rows: 20,
                col_widths: vec![20],
            },
            list_generation: 0,
        }
    }

    /// Build a test state with named rows (simulating a directory listing).
    fn state_with_rows(names: &[&str]) -> AppState {
        let mut s = test_state();
        s.rows = names
            .iter()
            .map(|n| RowData {
                path: PathBuf::from(format!("/tmp/test/{n}")),
                display: n.to_string(),
                kind: EntryKind::File,
            })
            .collect();
        s
    }

    // ── cursor_move_vertical ──────────────────────────────────────

    #[test]
    fn vertical_move_wraps_forward() {
        let mut s = state_with_rows(&["a", "b", "c"]);
        s.cursor.index = 2;
        s.cursor_move_vertical(1, 3);
        assert_eq!(s.cursor.index, 0);
    }

    #[test]
    fn vertical_move_wraps_backward() {
        let mut s = state_with_rows(&["a", "b", "c"]);
        s.cursor.index = 0;
        s.cursor_move_vertical(-1, 3);
        assert_eq!(s.cursor.index, 2);
    }

    #[test]
    fn vertical_move_no_op_on_empty() {
        let mut s = test_state();
        s.cursor_move_vertical(1, 0);
        assert_eq!(s.cursor.index, 0);
    }

    #[test]
    fn vertical_move_multi_step() {
        let mut s = state_with_rows(&["a", "b", "c", "d", "e"]);
        s.cursor.index = 1;
        s.cursor_move_vertical(3, 5);
        assert_eq!(s.cursor.index, 4);
    }

    // ── goto_col_top / goto_col_bottom ────────────────────────────

    #[test]
    fn goto_col_top_first_column() {
        let mut s = state_with_rows(&["a", "b", "c", "d", "e"]);
        s.last_grid = Grid {
            cols: 2,
            rows: 3,
            col_widths: vec![10, 10],
        };
        s.cursor.index = 2; // last in first column
        s.goto_col_top();
        assert_eq!(s.cursor.index, 0);
    }

    #[test]
    fn goto_col_top_second_column() {
        let mut s = state_with_rows(&["a", "b", "c", "d", "e"]);
        s.last_grid = Grid {
            cols: 2,
            rows: 3,
            col_widths: vec![10, 10],
        };
        s.cursor.index = 4; // second column, row 1
        s.goto_col_top();
        assert_eq!(s.cursor.index, 3); // top of second column
    }

    #[test]
    fn goto_col_bottom_first_column() {
        let mut s = state_with_rows(&["a", "b", "c", "d", "e"]);
        s.last_grid = Grid {
            cols: 2,
            rows: 3,
            col_widths: vec![10, 10],
        };
        s.cursor.index = 0;
        s.goto_col_bottom(5);
        assert_eq!(s.cursor.index, 2); // last in first column (3 rows)
    }

    #[test]
    fn goto_col_bottom_partial_column() {
        let mut s = state_with_rows(&["a", "b", "c", "d", "e"]);
        s.last_grid = Grid {
            cols: 2,
            rows: 3,
            col_widths: vec![10, 10],
        };
        s.cursor.index = 3; // second column
        s.goto_col_bottom(5);
        assert_eq!(s.cursor.index, 4); // last entry in partial column
    }

    // ── cursor_move_columns ───────────────────────────────────────

    #[test]
    fn column_move_right() {
        let mut s = state_with_rows(&["a", "b", "c", "d", "e", "f"]);
        s.last_grid = Grid {
            cols: 2,
            rows: 3,
            col_widths: vec![10, 10],
        };
        s.cursor.index = 1; // col 0, row 1
        s.cursor_move_columns(1, 3, 6);
        assert_eq!(s.cursor.index, 4); // col 1, row 1
    }

    #[test]
    fn column_move_left() {
        let mut s = state_with_rows(&["a", "b", "c", "d", "e", "f"]);
        s.last_grid = Grid {
            cols: 2,
            rows: 3,
            col_widths: vec![10, 10],
        };
        s.cursor.index = 4; // col 1, row 1
        s.cursor_move_columns(-1, 3, 6);
        assert_eq!(s.cursor.index, 1); // col 0, row 1
    }

    #[test]
    fn column_move_wraps_at_edge() {
        let mut s = state_with_rows(&["a", "b", "c", "d", "e", "f"]);
        s.last_grid = Grid {
            cols: 2,
            rows: 3,
            col_widths: vec![10, 10],
        };
        s.cursor.index = 4; // col 1, row 1
        s.cursor_move_columns(1, 3, 6); // wraps to col 0
        assert_eq!(s.cursor.index, 1); // col 0, row 1
    }

    #[test]
    fn column_move_single_column_noop() {
        let mut s = state_with_rows(&["a", "b"]);
        s.last_grid = Grid {
            cols: 1,
            rows: 10,
            col_widths: vec![20],
        };
        s.cursor.index = 0;
        s.cursor_move_columns(1, 10, 2);
        assert_eq!(s.cursor.index, 0); // no-op
    }

    // ── ensure_cursor_visible ─────────────────────────────────────

    #[test]
    fn ensure_visible_snaps_view_top() {
        let mut s = state_with_rows(&["a", "b", "c", "d", "e", "f", "g", "h"]);
        s.last_grid = Grid {
            cols: 1,
            rows: 3,
            col_widths: vec![20],
        }; // 3 items per page
        s.cursor.index = 5; // page 1 (items 3-5)
        s.ensure_cursor_visible();
        assert_eq!(s.cursor.view_top, 3);
    }

    #[test]
    fn ensure_visible_first_page() {
        let mut s = state_with_rows(&["a", "b", "c", "d"]);
        s.last_grid = Grid {
            cols: 1,
            rows: 3,
            col_widths: vec![20],
        };
        s.cursor.index = 1;
        s.ensure_cursor_visible();
        assert_eq!(s.cursor.view_top, 0);
    }

    // ── find_match ────────────────────────────────────────────────

    #[test]
    fn find_prefix_match() {
        let s = state_with_rows(&["alpha", "beta", "gamma"]);
        assert_eq!(s.find_match("b", 0, false), Some(1));
    }

    #[test]
    fn find_wraps_around() {
        // Pick names with no shared substrings so the wrap behavior is
        // unambiguous under substring matching: only `foo` contains `f`.
        let s = state_with_rows(&["foo", "bar", "baz"]);
        assert_eq!(s.find_match("f", 1, false), Some(0)); // wraps from bar/baz back to foo
    }

    #[test]
    fn find_backward() {
        let s = state_with_rows(&["alpha", "beta", "gamma"]);
        assert_eq!(s.find_match("b", 2, true), Some(1));
    }

    #[test]
    fn find_no_match() {
        let s = state_with_rows(&["alpha", "beta"]);
        assert_eq!(s.find_match("xyz", 0, false), None);
    }

    #[test]
    fn find_glob_pattern() {
        let s = state_with_rows(&["foo.rs", "bar.txt", "baz.rs"]);
        assert_eq!(s.find_match("*.rs", 0, false), Some(0));
        assert_eq!(s.find_match("*.rs", 1, false), Some(2));
    }

    #[test]
    fn find_empty_rows() {
        let s = test_state();
        assert_eq!(s.find_match("a", 0, false), None);
    }

    /// Regression: `/env` used to anchor at the start of the name,
    /// so dot-prefixed files (`.env`, `.envrc`) were unreachable
    /// without typing the dot. Now substring — `env` finds them all.
    #[test]
    fn find_substring_matches_dot_prefixed_file() {
        let s = state_with_rows(&[".env", ".envrc", "main.rs", "environment.toml"]);
        assert_eq!(s.find_match("env", 0, false), Some(0));
        assert_eq!(s.find_match("env", 1, false), Some(1));
        assert_eq!(s.find_match("env", 2, false), Some(3));
    }

    /// Substring match is case-insensitive on both sides.
    #[test]
    fn find_substring_is_case_insensitive() {
        let s = state_with_rows(&["README.md", "src", "Cargo.toml"]);
        assert_eq!(s.find_match("readme", 0, false), Some(0));
        assert_eq!(s.find_match("CARGO", 0, false), Some(2));
    }

    /// Globs are still anchor-aware (no implicit substring) so the
    /// power-user escape hatch keeps working: `env*` only matches
    /// names *starting* with `env`, hiding `.env` etc.
    #[test]
    fn find_glob_remains_anchored() {
        let s = state_with_rows(&[".env", "envoy", "main.rs"]);
        assert_eq!(s.find_match("env*", 0, false), Some(1));
    }

    // ── flash ─────────────────────────────────────────────────────

    #[test]
    fn flash_info_sets_message() {
        let mut s = test_state();
        s.flash_info("hello");
        let flash = s.flash.as_ref().unwrap();
        assert_eq!(flash.text, "hello");
        assert!(matches!(flash.kind, FlashKind::Info));
    }

    #[test]
    fn flash_error_sets_message() {
        let mut s = test_state();
        s.flash_error("oops");
        let flash = s.flash.as_ref().unwrap();
        assert_eq!(flash.text, "oops");
        assert!(matches!(flash.kind, FlashKind::Error));
    }

    // ── selection_paths ───────────────────────────────────────────

    #[test]
    fn selection_returns_cursor_item_when_no_picks() {
        let s = state_with_rows(&["a.txt", "b.txt"]);
        let paths = s.selection_paths();
        assert_eq!(paths.len(), 1);
        assert!(paths[0].ends_with("a.txt"));
    }

    #[test]
    fn selection_returns_picks_when_present() {
        let mut s = state_with_rows(&["a.txt", "b.txt", "c.txt"]);
        s.picks.toggle(Path::new("/tmp/test/b.txt"));
        s.picks.toggle(Path::new("/tmp/test/c.txt"));
        let paths = s.selection_paths();
        assert_eq!(paths.len(), 2);
    }

    #[test]
    fn selection_empty_when_no_rows() {
        let s = test_state();
        assert!(s.selection_paths().is_empty());
    }

    // ── toggle_pick_cursor ────────────────────────────────────────

    #[test]
    fn toggle_pick_adds_and_removes() {
        let mut s = state_with_rows(&["a.txt", "b.txt"]);
        s.toggle_pick_cursor();
        assert!(s.picks.contains(Path::new("/tmp/test/a.txt")));
        s.toggle_pick_cursor();
        assert!(!s.picks.contains(Path::new("/tmp/test/a.txt")));
    }

    #[test]
    fn toggle_pick_noop_in_inventory_view() {
        let mut s = state_with_rows(&["a.txt"]);
        s.view = View::Inventory;
        s.toggle_pick_cursor();
        assert!(s.picks.is_empty());
    }

    // ── toggle_all_picks ──────────────────────────────────────────

    #[test]
    fn toggle_all_picks_selects_then_clears() {
        let mut s = state_with_rows(&["a", "b", "c"]);
        s.toggle_all_picks();
        assert_eq!(s.picks.len(), 3);
        s.toggle_all_picks();
        assert!(s.picks.is_empty());
    }

    // ── take / drop / inventory ───────────────────────────────────

    fn state_with_real_files(tmp: &std::path::Path, names: &[&str]) -> AppState {
        let mut s = test_state();
        for name in names {
            std::fs::write(tmp.join(name), format!("content of {name}")).unwrap();
        }
        s.rows = names
            .iter()
            .map(|n| RowData {
                path: tmp.join(n),
                display: n.to_string(),
                kind: EntryKind::File,
            })
            .collect();
        s
    }

    #[test]
    fn take_cursor_item_to_inventory() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut s = state_with_real_files(tmp.path(), &["a.txt", "b.txt"]);
            s.take();
            assert_eq!(s.inventory.len(), 1);
            assert!(s.inventory.contains(&tmp.path().join("a.txt")));
        });
    }

    #[test]
    fn take_picks_to_inventory() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut s = state_with_real_files(tmp.path(), &["a.txt", "b.txt"]);
            s.picks.toggle(&tmp.path().join("a.txt"));
            s.picks.toggle(&tmp.path().join("b.txt"));
            s.take();
            assert_eq!(s.inventory.len(), 2);
        });
    }

    #[test]
    fn drop_removes_from_inventory() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut s = state_with_real_files(tmp.path(), &["a.txt"]);
            s.take(); // yank it first
            assert_eq!(s.inventory.len(), 1);
            // Switch to inventory view to drop
            s.toggle_inventory_view();
            s.drop_cursor();
            assert!(s.inventory.is_empty());
        });
    }

    // ── toggle_inventory_view ─────────────────────────────────────

    #[test]
    fn toggle_inventory_switches_view() {
        let mut s = test_state();
        assert_eq!(s.view, View::Dir);
        s.toggle_inventory_view();
        assert_eq!(s.view, View::Inventory);
        s.toggle_inventory_view();
        assert_eq!(s.view, View::Dir);
    }

    // ── focus_on_path ─────────────────────────────────────────────

    #[test]
    fn focus_on_path_sets_cursor() {
        let mut s = state_with_rows(&["a", "b", "c"]);
        s.focus_on_path(Path::new("/tmp/test/c"));
        assert_eq!(s.cursor.index, 2);
    }

    #[test]
    fn focus_on_missing_path_is_noop() {
        let mut s = state_with_rows(&["a", "b"]);
        s.cursor.index = 1;
        s.focus_on_path(Path::new("/tmp/test/nope"));
        assert_eq!(s.cursor.index, 1); // unchanged
    }

    // ── dispatch_command ──────────────────────────────────────────

    #[test]
    fn cmd_empty_is_handled() {
        let mut s = test_state();
        assert!(matches!(s.dispatch_command(""), CommandResult::Handled));
        assert!(matches!(s.dispatch_command("   "), CommandResult::Handled));
    }

    #[test]
    fn cmd_quit_defers_to_app() {
        // :q / :quit are App-layer commands now — they need save_session
        // and the double-tap confirm, neither of which the pure-domain
        // layer can see. Pure-domain must return the typed Quit variant
        // (forcing the App-side match to handle it) and must NOT flip
        // should_quit on its own.
        let mut s = test_state();
        assert!(matches!(s.dispatch_command("q"), CommandResult::Quit));
        assert!(!s.should_quit);
    }

    #[test]
    fn cmd_quit_long_defers_to_app() {
        let mut s = test_state();
        assert!(matches!(s.dispatch_command("quit"), CommandResult::Quit));
        assert!(!s.should_quit);
    }

    #[test]
    fn cmd_limit_set_and_clear() {
        let mut s = state_with_rows(&["foo.rs", "bar.txt", "baz.rs"]);
        s.dispatch_command("limit *.rs");
        assert_eq!(s.temp_filter.as_deref(), Some("*.rs"));
        assert!(s.flash.as_ref().unwrap().text.contains("limit:"));

        s.dispatch_command("limit");
        assert!(s.temp_filter.is_none());
        assert!(s.flash.as_ref().unwrap().text.contains("cleared"));
    }

    #[test]
    fn cmd_limit_picks_only() {
        let mut s = test_state();
        s.dispatch_command("limit !");
        assert_eq!(s.temp_filter.as_deref(), Some("!"));
    }

    #[test]
    fn cmd_sort_query() {
        let mut s = test_state();
        s.dispatch_command("sort");
        assert!(s.flash.as_ref().unwrap().text.contains("name"));
    }

    #[test]
    fn cmd_sort_set() {
        let mut s = test_state();
        s.dispatch_command("sort size");
        assert_eq!(s.sort_order, SortMode::Size);
        assert!(s.flash.as_ref().unwrap().text.contains("size"));
    }

    #[test]
    fn cmd_sort_invalid() {
        let mut s = test_state();
        s.dispatch_command("sort bogus");
        assert!(matches!(s.flash.as_ref().unwrap().kind, FlashKind::Error));
    }

    #[test]
    fn cmd_marks_empty() {
        let mut s = test_state();
        let result = s.dispatch_command("marks");
        assert!(matches!(result, CommandResult::Handled));
        assert!(s.flash.as_ref().unwrap().text.contains("no marks"));
    }

    #[test]
    fn cmd_marks_with_entries() {
        let mut s = test_state();
        s.marks.set(
            'a',
            Mark {
                dir: PathBuf::from("/tmp"),
                focus: None,
            },
        );
        let result = s.dispatch_command("marks");
        match result {
            CommandResult::OpenPager { title, lines } => {
                assert_eq!(title, "marks");
                assert_eq!(lines.len(), 1);
                assert!(lines[0].contains("/tmp"));
            }
            _ => panic!("expected OpenPager"),
        }
    }

    #[test]
    fn cmd_set_sort() {
        let mut s = test_state();
        s.dispatch_command("set sort=mtime");
        assert_eq!(s.sort_order, SortMode::Mtime);
    }

    #[test]
    fn cmd_set_unknown_key() {
        let mut s = test_state();
        s.dispatch_command("set foo=bar");
        assert!(matches!(s.flash.as_ref().unwrap().kind, FlashKind::Error));
    }

    #[test]
    fn cmd_shell_not_handled() {
        let mut s = test_state();
        assert!(matches!(
            s.dispatch_command("!ls"),
            CommandResult::NotHandled
        ));
        assert!(matches!(
            s.dispatch_command(";htop"),
            CommandResult::NotHandled
        ));
        assert!(matches!(
            s.dispatch_command("bprev"),
            CommandResult::NotHandled
        ));
        assert!(matches!(
            s.dispatch_command("bnext"),
            CommandResult::NotHandled
        ));
    }

    #[test]
    fn cmd_unknown() {
        let mut s = test_state();
        s.dispatch_command("foobar");
        let flash = s.flash.as_ref().unwrap();
        assert!(matches!(flash.kind, FlashKind::Error));
        assert!(flash.text.contains("foobar"));
    }

    // ── dispatch_prompt ───────────────────────────────────────────

    #[test]
    fn prompt_search_saves_last_search() {
        let mut s = test_state();
        let result = s.dispatch_prompt(&PromptKind::Search { saved_cursor: 0 }, "foo");
        assert!(matches!(result, PromptResult::Handled));
        assert_eq!(s.last_search.as_deref(), Some("foo"));
    }

    #[test]
    fn prompt_search_empty_does_not_save() {
        let mut s = test_state();
        s.last_search = Some("old".to_string());
        s.dispatch_prompt(&PromptKind::Search { saved_cursor: 0 }, "");
        assert_eq!(s.last_search.as_deref(), Some("old"));
    }

    #[test]
    fn prompt_limit_sets_filter() {
        let mut s = test_state();
        s.dispatch_prompt(&PromptKind::Limit, "*.rs");
        assert_eq!(s.temp_filter.as_deref(), Some("*.rs"));
    }

    #[test]
    fn prompt_limit_empty_clears() {
        let mut s = test_state();
        s.temp_filter = Some("old".to_string());
        s.dispatch_prompt(&PromptKind::Limit, "");
        assert!(s.temp_filter.is_none());
    }

    #[test]
    fn prompt_set_env() {
        let mut s = test_state();
        s.dispatch_prompt(&PromptKind::SetEnv, "TEST_SPYC_VAR=hello");
        assert!(matches!(s.flash.as_ref().unwrap().kind, FlashKind::Info));
        // Verify the override was recorded (in the envset store, not the
        // process env — `:s` no longer mutates `environ`).
        assert_eq!(
            crate::envset::var("TEST_SPYC_VAR").as_deref(),
            Some("hello")
        );
    }

    #[test]
    fn prompt_set_env_bad_format() {
        let mut s = test_state();
        s.dispatch_prompt(&PromptKind::SetEnv, "no_equals_sign");
        assert!(matches!(s.flash.as_ref().unwrap().kind, FlashKind::Error));
    }

    #[test]
    fn prompt_pattern_pick() {
        let mut s = test_state();
        // Add some listing entries for the pattern to match against
        s.listing = Listing::empty(PathBuf::from("/tmp/test"));
        use crate::fs::entry::{Entry, EntryKind};
        s.listing.entries = vec![
            Entry {
                path: PathBuf::from("/tmp/test/foo.rs"),
                name: "foo.rs".to_string(),
                kind: EntryKind::File,
                size: 0,
                mtime: std::time::SystemTime::UNIX_EPOCH,
            },
            Entry {
                path: PathBuf::from("/tmp/test/bar.txt"),
                name: "bar.txt".to_string(),
                kind: EntryKind::File,
                size: 0,
                mtime: std::time::SystemTime::UNIX_EPOCH,
            },
        ];
        s.dispatch_prompt(&PromptKind::PatternPick, "*.rs");
        assert!(s.picks.contains(Path::new("/tmp/test/foo.rs")));
        assert!(!s.picks.contains(Path::new("/tmp/test/bar.txt")));
    }

    #[test]
    fn prompt_pane_new_tab_cmd_stashes() {
        let mut s = test_state();
        s.dispatch_prompt(&PromptKind::PaneNewTabCmd, "bash");
        assert_eq!(s.pending_new_tab_cmd.as_deref(), Some("bash"));
        assert!(matches!(s.mode, Mode::Prompting(_)));
    }

    #[test]
    fn prompt_pane_new_tab_cmd_empty_is_noop() {
        let mut s = test_state();
        s.dispatch_prompt(&PromptKind::PaneNewTabCmd, "");
        assert!(s.pending_new_tab_cmd.is_none());
    }

    #[test]
    fn prompt_shell_cmd_not_handled() {
        let mut s = test_state();
        assert!(matches!(
            s.dispatch_prompt(&PromptKind::ShellCmd, "ls"),
            PromptResult::NotHandled
        ));
        assert!(matches!(
            s.dispatch_prompt(&PromptKind::ShellCmdCaptured, "ls"),
            PromptResult::NotHandled
        ));
        assert!(matches!(
            s.dispatch_prompt(&PromptKind::CopyTo, "/tmp"),
            PromptResult::NotHandled
        ));
    }

    #[test]
    fn prompt_remove_confirm_handled() {
        let mut s = test_state();
        assert!(matches!(
            s.dispatch_prompt(&PromptKind::RemoveConfirm, "n"),
            PromptResult::Handled
        ));
    }

    // ── apply() action dispatch ───────────────────────────────────

    #[test]
    fn apply_down_moves_cursor() {
        let mut s = state_with_rows(&["a", "b", "c"]);
        assert!(matches!(s.apply(&Action::Down(1)), ApplyResult::Handled));
        assert_eq!(s.cursor.index, 1);
    }

    #[test]
    fn apply_up_wraps() {
        let mut s = state_with_rows(&["a", "b", "c"]);
        assert!(matches!(s.apply(&Action::Up(1)), ApplyResult::Handled));
        assert_eq!(s.cursor.index, 2);
    }

    #[test]
    fn apply_down_with_count() {
        let mut s = state_with_rows(&["a", "b", "c", "d", "e"]);
        s.apply(&Action::Down(3));
        assert_eq!(s.cursor.index, 3);
    }

    #[test]
    fn apply_page_down() {
        let mut s = state_with_rows(&["a", "b", "c", "d", "e", "f"]);
        s.last_grid = Grid {
            cols: 1,
            rows: 3,
            col_widths: vec![20],
        };
        s.apply(&Action::PageDown);
        assert_eq!(s.cursor.index, 3);
    }

    #[test]
    fn apply_goto_first() {
        let mut s = state_with_rows(&["a", "b", "c"]);
        s.cursor.index = 2;
        s.apply(&Action::GotoFirst);
        assert_eq!(s.cursor.index, 0);
    }

    #[test]
    fn apply_goto_last() {
        let mut s = state_with_rows(&["a", "b", "c"]);
        s.apply(&Action::GotoLast);
        assert_eq!(s.cursor.index, 2);
    }

    #[test]
    fn apply_left_right_columns() {
        let mut s = state_with_rows(&["a", "b", "c", "d", "e", "f"]);
        s.last_grid = Grid {
            cols: 2,
            rows: 3,
            col_widths: vec![10, 10],
        };
        s.apply(&Action::Right(1));
        assert_eq!(s.cursor.index, 3);
        s.apply(&Action::Left(1));
        assert_eq!(s.cursor.index, 0);
    }

    #[test]
    fn apply_toggle_pick() {
        let mut s = state_with_rows(&["a.txt", "b.txt"]);
        s.apply(&Action::TogglePick);
        assert!(s.picks.contains(Path::new("/tmp/test/a.txt")));
    }

    #[test]
    fn apply_pick_toggle_all() {
        let mut s = state_with_rows(&["a", "b", "c"]);
        s.apply(&Action::PickToggleAll);
        assert_eq!(s.picks.len(), 3);
        s.apply(&Action::PickToggleAll);
        assert!(s.picks.is_empty());
    }

    fn dirty_state(names: &[&str], dirty: &[&str]) -> AppState {
        use crate::ui::list_view::{GitChange, GitFileStatus};
        let mut s = state_with_rows(names);
        for d in dirty {
            s.git_files.insert(
                (*d).to_string(),
                GitFileStatus::unstaged(GitChange::Modified),
            );
        }
        s
    }

    #[test]
    fn jump_next_git_change_skips_clean_rows() {
        let mut s = dirty_state(&["a", "b", "c", "d"], &["c"]);
        s.cursor.index = 0;
        assert!(s.jump_to_git_change(true));
        assert_eq!(s.cursor.index, 2); // landed on `c`
    }

    #[test]
    fn jump_next_git_change_wraps_around() {
        let mut s = dirty_state(&["a", "b", "c", "d"], &["a"]);
        s.cursor.index = 2; // past the only dirty row
        assert!(s.jump_to_git_change(true));
        assert_eq!(s.cursor.index, 0); // wrapped back to `a`
    }

    #[test]
    fn jump_prev_git_change_wraps_around() {
        let mut s = dirty_state(&["a", "b", "c", "d"], &["d"]);
        s.cursor.index = 1; // before the only dirty row in reverse
        assert!(s.jump_to_git_change(false));
        assert_eq!(s.cursor.index, 3); // wrapped to `d`
    }

    #[test]
    fn jump_advances_off_the_current_dirty_row() {
        // From a dirty row, pressing `]g` should land on the *next*
        // dirty row, not stay put.
        let mut s = dirty_state(&["a", "b", "c", "d"], &["a", "c"]);
        s.cursor.index = 0;
        assert!(s.jump_to_git_change(true));
        assert_eq!(s.cursor.index, 2);
    }

    #[test]
    fn jump_returns_false_when_no_changes() {
        let mut s = state_with_rows(&["a", "b", "c"]);
        assert!(!s.jump_to_git_change(true));
        assert!(!s.jump_to_git_change(false));
    }

    #[test]
    fn apply_take_adds_to_inventory() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut s = state_with_real_files(tmp.path(), &["a.txt"]);
            s.apply(&Action::Take);
            assert_eq!(s.inventory.len(), 1);
            assert!(s.inventory.contains(&tmp.path().join("a.txt")));
        });
    }

    #[test]
    fn apply_drop_removes_from_inventory() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut s = state_with_real_files(tmp.path(), &["a.txt"]);
            s.take(); // yank first
            s.toggle_inventory_view();
            s.apply(&Action::Drop);
            assert!(s.inventory.is_empty());
        });
    }

    #[test]
    fn apply_toggle_inventory_view() {
        let mut s = test_state();
        s.apply(&Action::ToggleInventoryView);
        assert_eq!(s.view, View::Inventory);
        s.apply(&Action::ToggleInventoryView);
        assert_eq!(s.view, View::Dir);
    }

    #[test]
    fn apply_empty_inventory() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut s = state_with_real_files(tmp.path(), &["a.txt"]);
            s.take(); // yank first
            assert_eq!(s.inventory.len(), 1);
            s.apply(&Action::EmptyInventory);
            assert!(s.inventory.is_empty());
        });
    }

    #[test]
    fn apply_toggle_mask() {
        let mut s = test_state();
        let was_enabled = s.masks.mask1.enabled;
        s.apply(&Action::ToggleMask(1));
        assert_ne!(s.masks.mask1.enabled, was_enabled);
    }

    #[test]
    fn apply_search_next_finds_match() {
        let mut s = state_with_rows(&["alpha", "beta", "gamma"]);
        s.last_search = Some("g".to_string());
        s.apply(&Action::SearchNext);
        assert_eq!(s.cursor.index, 2);
    }

    #[test]
    fn apply_search_prev_finds_match() {
        // Only `alpha` contains `lph`, so the backward sweep from
        // gamma → beta → alpha lands unambiguously on idx 0 under
        // substring matching too.
        let mut s = state_with_rows(&["alpha", "beta", "gamma"]);
        s.cursor.index = 2;
        s.last_search = Some("lph".to_string());
        s.apply(&Action::SearchPrev);
        assert_eq!(s.cursor.index, 0);
    }

    #[test]
    fn apply_start_shell_returns_spawn() {
        let mut s = test_state();
        let result = s.apply(&Action::StartShell);
        assert!(matches!(
            result,
            ApplyResult::Post(PostAction::Spawn { .. })
        ));
    }

    #[test]
    fn apply_prompt_actions_set_mode() {
        let mut s = test_state();
        s.apply(&Action::SearchPrompt);
        assert!(matches!(s.mode, Mode::Prompting(_)));

        s.mode = Mode::Normal;
        s.apply(&Action::ShellCapturedPrompt);
        assert!(matches!(s.mode, Mode::Prompting(_)));

        s.mode = Mode::Normal;
        s.apply(&Action::CommandPrompt);
        assert!(matches!(s.mode, Mode::Prompting(_)));

        s.mode = Mode::Normal;
        s.apply(&Action::JumpPrompt);
        assert!(matches!(s.mode, Mode::Prompting(_)));

        s.mode = Mode::Normal;
        s.apply(&Action::LimitPrompt);
        assert!(matches!(s.mode, Mode::Prompting(_)));

        s.mode = Mode::Normal;
        s.apply(&Action::SetEnvPrompt);
        assert!(matches!(s.mode, Mode::Prompting(_)));
    }

    #[test]
    fn apply_set_mark() {
        let mut s = state_with_rows(&["file.txt"]);
        s.apply(&Action::SetMark('a'));
        assert!(s.marks.get('a').is_some());
    }

    #[test]
    fn apply_date_flashes() {
        let mut s = test_state();
        s.apply(&Action::Date);
        assert!(s.flash.is_some());
        assert!(s.flash.as_ref().unwrap().text.contains("UTC"));
    }

    #[test]
    fn apply_version_flashes() {
        let mut s = test_state();
        s.apply(&Action::Version);
        let flash = s.flash.as_ref().unwrap();
        assert!(flash.text.contains("spyc"));
    }

    #[test]
    fn apply_noop_does_nothing() {
        let mut s = test_state();
        let result = s.apply(&Action::Noop);
        assert!(matches!(result, ApplyResult::Handled));
    }

    #[test]
    fn apply_macro_record_reserved_flashes_hint() {
        let mut s = test_state();
        let result = s.apply(&Action::MacroRecordReserved);
        assert!(matches!(result, ApplyResult::Handled));
        let flash = s.flash.as_ref().unwrap();
        assert!(flash.text.contains("reserved"), "got: {}", flash.text);
        assert!(flash.text.contains('Q'), "should hint at Q: {}", flash.text);
    }

    #[test]
    fn apply_long_list_returns_pager() {
        let mut s = state_with_rows(&["a.txt"]);
        let result = s.apply(&Action::LongList);
        assert!(matches!(result, ApplyResult::OpenPager(_)));
    }

    #[test]
    fn apply_file_type_single_flashes() {
        let mut s = state_with_rows(&["a.txt"]);
        let result = s.apply(&Action::FileType);
        // Single file: flashes info, returns Handled
        assert!(matches!(result, ApplyResult::Handled));
        assert!(s.flash.is_some());
    }

    #[test]
    fn apply_pane_actions_not_handled() {
        let mut s = test_state();
        assert!(matches!(
            s.apply(&Action::TogglePane),
            ApplyResult::NotHandled
        ));
        assert!(matches!(
            s.apply(&Action::PaneFocusDown),
            ApplyResult::NotHandled
        ));
        assert!(matches!(s.apply(&Action::Help), ApplyResult::NotHandled));
        assert!(matches!(s.apply(&Action::Redraw), ApplyResult::NotHandled));
        assert!(matches!(
            s.apply(&Action::ColorToggle),
            ApplyResult::NotHandled
        ));
    }

    #[test]
    fn apply_worktree_new_sets_prompt_or_errors() {
        let mut s = test_state();
        // No git info → error
        s.apply(&Action::WorktreeNew);
        assert!(matches!(s.flash.as_ref().unwrap().kind, FlashKind::Error));

        // With git info → prompt
        s.flash = None;
        s.git_info = Some("main".to_string());
        s.apply(&Action::WorktreeNew);
        assert!(matches!(s.mode, Mode::Prompting(_)));
    }

    #[test]
    fn apply_jump_prev_dir() {
        let mut s = test_state();
        // No prev dir → error
        s.apply(&Action::JumpPrevDir);
        assert!(matches!(s.flash.as_ref().unwrap().kind, FlashKind::Error));
    }

    #[test]
    fn apply_clamps_cursor_after_action() {
        let mut s = state_with_rows(&["a", "b"]);
        s.cursor.index = 10; // out of bounds
        s.apply(&Action::Noop); // any handled action should clamp
        assert_eq!(s.cursor.index, 1); // clamped to last valid
    }

    // ── SPYC_COMMANDS sanity ──────────────────────────────────────

    #[test]
    fn spyc_commands_is_sorted_and_unique() {
        let mut sorted = super::SPYC_COMMANDS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(
            sorted.as_slice(),
            super::SPYC_COMMANDS,
            "SPYC_COMMANDS must be sorted and free of duplicates so \
             tab-completion produces deterministic output",
        );
    }

    #[test]
    fn spyc_commands_covers_every_arm_of_dispatch() {
        // Every literal we list as a `:` command must be reachable
        // through one of the dispatch_command arms (here in state.rs
        // or in App::dispatch_command). This test guards against
        // drift: if you add a new `:foo` command and forget to list
        // it here, completion silently won't offer it.
        let mut s = state_with_rows(&[]);
        for cmd in super::SPYC_COMMANDS {
            // Bare-word dispatch shouldn't flash "unknown command:" —
            // it should either be Handled by AppState (no-arg form) or
            // NotHandled (passes through to App for terminal-owning
            // commands like :grep, :task-to-pane, …). The "unknown"
            // flash only fires from the catch-all at the bottom of
            // state.rs::dispatch_command.
            s.flash = None;
            s.dispatch_command(cmd);
            if let Some(ref f) = s.flash {
                assert!(
                    !f.text.starts_with("unknown command:"),
                    "SPYC_COMMANDS lists `{cmd}` but dispatch_command \
                     reports it as unknown — either add the arm, or \
                     drop the entry from SPYC_COMMANDS",
                );
            }
        }
    }

    /// End-to-end-ish coverage of the git refresh pipeline. Edit a
    /// tracked file → `refresh_listing` surfaces the `M` marker; commit
    /// it → the next refresh clears it. Drives the real `refresh_listing`
    /// → `git_file_statuses_cached` → `git status --porcelain` path on a
    /// throwaway temp repo, so a regression in any of those (or in the
    /// raw-cache / mtime-cache / row-rebuild plumbing) shows up here.
    /// `git_worker_tx` is unset, so the sync spawn path runs — no
    /// timing dependency, no real fs watcher.
    #[test]
    fn refresh_listing_picks_up_edit_and_clears_after_commit() {
        // Canonicalize so macOS `/var` → `/private/var` doesn't trip the
        // repo_root match inside the refresh path.
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap_or_else(|_| tmp.path().to_path_buf());

        let run_git = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .args(args)
                .current_dir(&root)
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@x")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@x")
                // Suppress any user-level .gitconfig so the test is
                // hermetic on machines with unusual defaults.
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_SYSTEM", "/dev/null")
                .status()
                .expect("spawn git");
            assert!(status.success(), "git {args:?} failed");
        };
        run_git(&["init", "-q", "--initial-branch=main"]);
        std::fs::write(root.join("file.txt"), "v1\n").unwrap();
        run_git(&["add", "file.txt"]);
        run_git(&["commit", "-q", "-m", "v1"]);

        let mut s = test_state();
        s.listing.dir = root.clone();
        s.start_dir = root.clone();
        s.update_huge_tree(&root);
        s.git_info = s.compute_git_info_fast();

        // Clean repo: refresh sees no modifications.
        s.refresh_listing();
        assert!(
            s.git_files.is_empty(),
            "clean repo: no markers (got {:?})",
            s.git_files
        );

        // Working-tree edit → `M file.txt` should surface on next refresh.
        std::fs::write(root.join("file.txt"), "v2\n").unwrap();
        // Bypass the in-state 1 s invalidation throttle so this call
        // re-fetches instead of reusing the cached clean snapshot.
        s.last_git_invalidation = None;
        s.refresh_listing();
        assert!(
            s.git_files.contains_key("file.txt"),
            "expected M marker for file.txt after edit; got {:?}",
            s.git_files
        );

        // Commit it → marker should clear (`.git/index` mtime moves, so
        // the mtime-cache invalidates on its own).
        run_git(&["add", "file.txt"]);
        run_git(&["commit", "-q", "-m", "v2"]);
        s.last_git_invalidation = None;
        s.refresh_listing();
        assert!(
            !s.git_files.contains_key("file.txt"),
            "expected marker to clear after commit; got {:?}",
            s.git_files
        );
    }
}
