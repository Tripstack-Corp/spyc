//! Domain state for the application — everything testable without a terminal.
//!
//! `AppState` holds navigation, selection, filtering, bookmarks, input mode,
//! config, history, and cached info. Event handlers that operate on pure
//! domain logic live here; the `App` shell in `mod.rs` owns terminal state
//! (pager widget, pane tabs, pty handles) and delegates to `AppState`.

// The pure Model must never panic on domain state: a bare `.unwrap()` here is a
// build error (use `.expect("invariant")`, or handle the `None`/`Err`). Scoped
// on purpose — see AGENTS.md → "Rust house style"; the `tests/` module opts out.
#![deny(clippy::unwrap_used)]

use std::path::{Path, PathBuf};

#[cfg(test)]
use crate::config::Config;
#[cfg(test)]
use crate::fs::Listing;
use crate::keymap::Resolver;
#[cfg(test)]
use crate::keymap::UserKeymap;
#[cfg(test)]
use crate::state::{Cursor, IgnoreMasks, Picks};
use crate::state::{Frecency, History, Inventory, Marks};
#[cfg(test)]
use crate::ui::list_view::GridDims;

use super::{Effect, FlashKind, FlashMessage, Mode, RowData, View};

pub(super) mod agent_hud;
mod pane_layout;
pub use pane_layout::PaneLayout;
mod apply;
pub mod archive;
mod commander;
pub use commander::Commander;
mod dispatch;
mod git;
mod git_cache;
mod homes;
pub use git_cache::{GitCache, GitState, GitStatusCache, GitWorkerRequest, GitWorkerResult};
mod listing;
mod navigation;
mod pending_images;
pub use pending_images::{MAX_PENDING_IMAGES_PER_TAB, PendingImage};
mod project;
pub use project::{Project, ProjectId};
pub mod restore;
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
    /// Effects for the event loop to execute (e.g. a `ForegroundExec`
    /// for `$SHELL`, or an off-thread `FileOp` that opens a pager when it
    /// completes). Empty == nothing to do.
    ///
    /// `apply` has no direct `OpenPager` variant: opening a pager needs the
    /// content's IO (e.g. `L`'s per-file `symlink_metadata`), which must not
    /// run in this pure dispatcher — those arms emit an `Effect::FileOp`
    /// instead. The marks / command path produces `Update::OpenPager`
    /// directly (see `CommandResult`).
    Post(Vec<Effect>),
    /// Caller must handle this action (terminal-touching).
    NotHandled,
}

/// The unified result of any pure-domain producer (`apply` /
/// `dispatch_command` / `dispatch_prompt`), normalizing the three
/// per-producer result enums above (`ApplyResult` / `CommandResult` /
/// `PromptResult`) into one shape the App-side bridges match uniformly:
/// - `Handled(effects)` carries the producer's `Post` effects (empty `Vec`
///   for a plain `Handled` — the App side runs whatever's there);
/// - `OpenPager` / `Quit` stay typed (App-only capability + a compile-time
///   wiring guard — dropping the arm is a build error, not a silent regress);
/// - `Defer` is the old `NotHandled` — the App-side executor takes over.
///
/// The `From` impls below are the single, tested mapping site. The producers
/// deliberately keep their own narrower result enums (each exposes only the
/// variants it can actually return); the bridges in `actions` / `commands` /
/// `prompt` widen to `Update` via `From` at the call site. This is the settled
/// shape — an earlier plan to have producers return `Update` directly and
/// delete the three was dropped as churn for no behavioural gain.
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

/// Description of a pager to open, without importing UI types.
#[derive(Debug)]
pub struct PagerRequest {
    pub title: String,
    pub lines: Vec<String>,
    pub columns: u8,
    /// When true, the pager height auto-shrinks to fit content (top edge
    /// stays anchored to the standard centred position; the box just
    /// grows shorter from the bottom). Line-number gutter is suppressed
    /// since it's noise for short summaries.
    pub fit_to_content: bool,
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

/// Archives currently mounted (`docs/archive/ARCHIVE_BROWSING_PLAN.md`). Pure
/// data — an index plus the user's pending changes — so it belongs in the Model
/// even though the bytes it describes live on disk.
pub type Mounts = crate::archive::Mounts;

/// MVU Phase 5: a cheap Model-side snapshot of the active pane's
/// routing-relevant flags, refreshed once per loop iteration (after the
/// tab drain + `mark_exited`, before `recv`). `route_snapshot` reads these
/// instead of the live `PtyHost` so key routing decouples from the
/// Runtime. Behaviour-equivalent: refreshed from the live pane at the same
/// loop point the old live read would have observed, and the only mutator
/// of `is_scrolling`/`is_closed` (key handlers / child-exit drain) runs
/// either before this refresh or after `route_snapshot`. (Render keeps its
/// own live reads; the richer per-tab snapshot for render purity is Phase 6.)
#[derive(Default, Clone, Copy)]
pub struct PaneSnapshot {
    pub is_scrolling: bool,
    pub is_closed: bool,
}

/// Bottom-pane layout + prompt-echo state: the split percentage, zoom
/// (with the focus captured at zoom-on), the hide toggle, the once-per-loop
/// routing snapshot, and the `^a c` prompt-echo buffers. Grouped out of
/// Which region `^a z` has zoomed. Zoom enlarges the *focused* region:
/// `BottomPane` collapses the file list (the historical behaviour),
/// `TopList` collapses the pane (the list fills the frame). `None` is the
/// normal split. The target is derived from focus at zoom-on, so focus is
/// unchanged across the toggle and needs no separate save/restore.
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ZoomTarget {
    /// Normal split — neither region is zoomed.
    #[default]
    None,
    /// The bottom pty pane fills the middle; the file list collapses to 0 rows.
    BottomPane,
    /// The file list fills the frame; the pane collapses off-screen (like
    /// `pane_hidden`, but the pty stays alive and the prior split is kept).
    /// Also the **left** column of a vertical split zoomed (the list is that
    /// column's content).
    TopList,
    /// The **right** column of a vertical split (the preview) fills the frame;
    /// the left column + pane collapse. Like `TopList` for layout (pane
    /// collapsed, body full), but the body renders `view.right_pager` instead
    /// of the file list. The split is kept, so un-zoom restores it.
    RightColumn,
}

/// Which of the two vertical regions a left/right split addresses. Labelled
/// `a` (left) / `b` (right) in the UI — numbers stay for PTY tabs, letters
/// for file panes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    /// The column across the split.
    pub const fn other(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }
}

/// How a vertical split is laid out. `TopOnly` splits just the file-list
/// region (the PTY/agent pane stays full-width below both columns);
/// `FullHeight` runs the divider the whole frame height (the PTY pane shrinks
/// to the left column's width).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VsplitMode {
    TopOnly,
    FullHeight,
}

impl VsplitMode {
    /// The other mode — the `^s f` flip.
    pub const fn flipped(self) -> Self {
        match self {
            Self::TopOnly => Self::FullHeight,
            Self::FullHeight => Self::TopOnly,
        }
    }
}

/// `[layout] vsplit_mode` — the shape a fresh split opens in.
impl From<crate::config::VsplitMode> for VsplitMode {
    fn from(m: crate::config::VsplitMode) -> Self {
        match m {
            crate::config::VsplitMode::FullHeight => Self::FullHeight,
            crate::config::VsplitMode::TopOnly => Self::TopOnly,
        }
    }
}

/// The vertical (left/right) split. Pure domain state: the right column's
/// width share, the layout mode, and which column owns input. Held as
/// `Option<VSplit>` on the Model — `None` is the single-column default. The
/// pager *content* for the right region lives in `ViewState`, not here.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VSplit {
    /// The right column's share of the split, percent. Clamped to `[20, 80]`.
    pub width_pct: u16,
    pub mode: VsplitMode,
    /// Which column currently owns the keyboard.
    pub focus: Side,
}

pub struct AppState {
    pub inventory: Inventory,
    pub marks: Marks,
    /// Archives mounted this session, keyed by the archive file's own path. A
    /// column whose `listing.dir` sits under one of them is browsing an index
    /// rather than a directory (`docs/archive/ARCHIVE_BROWSING_PLAN.md`).
    pub mounts: Mounts,
    pub resolver: Resolver,
    /// `:mouse on|off` for this session, overriding `[mouse] capture`.
    ///
    /// Separate from `config` on purpose: `reload_config` replaces the whole
    /// `Config`, so a runtime toggle stored there is silently reverted by any
    /// save of a watched config file — including the automatic fs-watch reload.
    /// `:mouse auto` clears it and hands control back to the config.
    pub mouse_capture_override: Option<bool>,
    /// `:set flags=` for the rest of the run. Outside `Config` for the same
    /// reason as `mouse_capture_override`: a config reload would revert it.
    pub status_flags_override: Option<crate::ui::status_flags::FlagsMode>,
    pub mode: Mode,
    pub session_name: Option<String>,
    /// Stable per-process session file id (millis timestamp), assigned once at
    /// startup and carried through restore, so every save (quit + P3-2 autosave)
    /// overwrites the same `<id>.json` instead of spawning a new file per save.
    pub session_id: Option<u64>,
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
    /// Rows about to be deleted, populated while a `RemoveConfirm`
    /// prompt is active. Drives the warning-colour row highlight in
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
    /// P2 merge/scope coordination registry (`docs/archive/AGENT_AWARENESS_PLAN.md`):
    /// what each agent tab has declared it's touching, via the `register_scope`
    /// MCP tool. In-memory + session-persisted (rides P3-2's autosave and
    /// `spyc -r`), never file-backed — every agent pane in one spyc shares this
    /// one registry. Advisory only: nothing here blocks a merge.
    pub scope_registry: Vec<crate::state::scope_registry::ScopeClaim>,
    /// Pure question correlation keyed by ephemeral pane id; never persisted.
    pub codex_recovery:
        std::collections::HashMap<String, crate::agent::codex_recovery::CodexRecovery>,
    pub user_host: String,
    pub pending_new_tab_cmd: Option<String>,
    pub pending_worktrees: Option<Vec<PathBuf>>,
    pub pending_sessions: Option<Vec<crate::state::sessions::Manifest>>,
    /// `spyc -c` commands not yet run: they wait for startup to settle
    /// (`App::settle_startup_commands`).
    pub startup_commands: Vec<String>,
    pub frecency: Frecency,
    /// Every open project, in switcher order; looked up by `active` via
    /// [`Self::active_project`] / [`Self::active_project_mut`]
    /// (`docs/drafts/PROJECTS_PLAN.md`, #99). Action handlers reach the active
    /// project's **focused** column via [`Self::cur`] / [`Self::cur_mut`];
    /// render draws `left`/`right` explicitly off [`Self::active_project`].
    pub projects: Vec<Project>,
    /// Which of `projects` is drawn and driven by the keyboard — or, while
    /// [`Self::in_background`] is set, the project an agent's MCP call acts on.
    pub active: ProjectId,
    /// Set while `App::in_project` has made `active` a project the user isn't
    /// looking at, to serve an MCP call from that project's agent. What belongs
    /// to the user's view stays with it: the process cwd, frecency and the
    /// harpoon menu.
    pub in_background: bool,
    /// Whether the background git-status worker is wired. Set by `App::new`
    /// once the worker thread spawns; `false` in tests and during the
    /// AppState bootstrap before the worker exists. When `true`, a
    /// cache-miss in `git_file_statuses_cached` enqueues a request into the
    /// column's `pending_git_requests` — the Model holds no channel, so the
    /// App run loop flushes that outbox to the Runtime-owned `git_worker_tx`.
    /// When `false`, the synchronous `git status` spawn path runs inline. One
    /// worker serves every project's columns, so the flag is app-wide.
    pub git_worker_available: bool,
}

impl AppState {
    /// The active project — the one drawn and driven by the keyboard. `active`
    /// always names a live entry in `projects` (#99); the scan is linear, as a
    /// spyc holds a handful of projects.
    pub fn active_project(&self) -> &Project {
        self.projects
            .iter()
            .find(|p| p.id == self.active)
            .expect("active always names a live project")
    }

    /// Mutable [`Self::active_project`]. Note this borrows all of `self`, same
    /// caveat as [`Self::cur_mut`].
    pub fn active_project_mut(&mut self) -> &mut Project {
        self.projects
            .iter_mut()
            .find(|p| p.id == self.active)
            .expect("active always names a live project")
    }

    /// The open project `id`; `None` once it has closed.
    pub fn project(&self, id: ProjectId) -> Option<&Project> {
        self.projects.iter().find(|p| p.id == id)
    }

    pub fn has_project(&self, id: ProjectId) -> bool {
        self.project(id).is_some()
    }

    /// Every open project, in switcher order.
    pub fn open_projects(&self) -> impl Iterator<Item = &Project> {
        self.projects.iter()
    }

    /// Feed `key` to the resolver. `resolver` lives flat on `AppState` while
    /// `user_keymap` lives on the active project; `active_project()` borrows
    /// all of `self`, so it can't be called while `resolver` is already
    /// borrowed mutably for `feed` — take it out, call `feed`, then put it
    /// back (`Resolver` is cheap and `Default`).
    pub fn feed_resolver(
        &mut self,
        key: crossterm::event::KeyEvent,
    ) -> crate::keymap::ResolverOutcome {
        let mut resolver = std::mem::take(&mut self.resolver);
        let outcome = resolver.feed(key, &self.active_project().user_keymap);
        self.resolver = resolver;
        outcome
    }

    /// Old `pane_focused` axis, derived from the active project's `focus`
    /// field. True exactly when the bottom pty pane owns the keyboard. Every
    /// former read of `self.state.pane_focused` is now
    /// `self.state.pane_focused()`; behaviour-equivalent because the
    /// router, the render DIM cue, the `^C` gate and the paste filter
    /// only ever consumed the bool, never the non-Pane discriminant.
    pub fn pane_focused(&self) -> bool {
        matches!(self.active_project().focus, Focus::Pane)
    }

    /// The **focused** file-commander — the column action handlers operate on.
    /// Resolves to `right` only when a right column exists *and* the vsplit
    /// focus is on it; otherwise `left`. While `right` is `None` (the
    /// single-column default, and all of Stage 1) this is always `left`, which
    /// is what makes the `self.left.…` → `self.cur().…` conversion
    /// behaviour-preserving. Render does **not** use this — it draws `left` /
    /// `right` explicitly as fixed visual identities.
    pub fn cur(&self) -> &Commander {
        let p = self.active_project();
        match p.right.as_ref() {
            Some(right) if matches!(p.vsplit.map(|v| v.focus), Some(Side::Right)) => right,
            _ => &p.left,
        }
    }

    /// Mutable [`Self::cur`]. Note this borrows all of `self`, so a handler that
    /// also touches a global field while holding `cur_mut()` must sequence the
    /// reads first (evaluate the RHS, then assign through `cur_mut()`).
    pub fn cur_mut(&mut self) -> &mut Commander {
        let p = self.active_project_mut();
        match p.right.as_mut() {
            Some(right) if matches!(p.vsplit.map(|v| v.focus), Some(Side::Right)) => right,
            _ => &mut p.left,
        }
    }

    /// Every live column's listing dir in every open project — `left`, plus
    /// `right` when the split is open — with the project it is in. Deliberately
    /// every column, not the focused one nor only the active project's: callers
    /// ask "is any column standing here?" of the global mount table, which is
    /// what makes evicting or unmounting an archive safe.
    pub fn every_column_dir(&self) -> Vec<(ProjectId, std::path::PathBuf)> {
        self.projects
            .iter()
            .flat_map(|p| {
                std::iter::once(&p.left)
                    .chain(p.right.as_ref())
                    .map(move |c| (p.id, c.listing.dir.clone()))
            })
            .collect()
    }

    /// The working directory a freshly-spawned pane tab opens in, honouring
    /// `[pane] new_tab_cwd`. `WorktreeRoot` (the default) anchors the pane to
    /// the focused column's worktree/repo root (`gw`'s target); `ProjectHome`
    /// anchors to the sticky session project root; `BrowseDir` opens at the
    /// focused column's current listing dir. `WorktreeRoot`/`ProjectHome` fall
    /// back to the browse dir when their target is unresolved. Goes through
    /// `cur()` so a focused second commander is honoured — and so a pane
    /// launched from the pane view follows the last-focused column, the one
    /// `^a k` returns to (`columns_are_addressed_through_handles`).
    /// How the status bar writes its state suffix: `:set flags=`, else
    /// `[layout] status_flags`.
    pub fn status_flags(&self) -> crate::ui::status_flags::FlagsMode {
        self.status_flags_override
            .unwrap_or(self.active_project().config.layout.status_flags)
    }

    pub fn default_pane_cwd(&self) -> std::path::PathBuf {
        let p = self.active_project();
        match p.config.pane.new_tab_cwd {
            crate::config::NewTabCwd::WorktreeRoot => self
                .cur()
                .git_cache
                .current_repo_root
                .clone()
                .unwrap_or_else(|| self.cur().listing.dir.clone()),
            crate::config::NewTabCwd::ProjectHome => p
                .home
                .clone()
                .unwrap_or_else(|| self.cur().listing.dir.clone()),
            crate::config::NewTabCwd::BrowseDir => self.cur().listing.dir.clone(),
        }
    }

    /// The commander for an EXPLICIT column (not focus-based, unlike `cur`).
    /// `Right` with no second commander falls back to `left` (safe no-op for
    /// the per-column git refresh loop). Used to drive git per-Side.
    pub fn col(&self, side: Side) -> &Commander {
        let p = self.active_project();
        match side {
            Side::Right => p.right.as_ref().unwrap_or(&p.left),
            Side::Left => &p.left,
        }
    }

    pub fn col_mut(&mut self, side: Side) -> &mut Commander {
        let p = self.active_project_mut();
        match side {
            Side::Right => p.right.as_mut().unwrap_or(&mut p.left),
            Side::Left => &mut p.left,
        }
    }

    /// The commander on `side` if that column is open. Unlike [`Self::col`],
    /// `Right` with no second commander is `None`, not `left`.
    pub fn get_col(&self, side: Side) -> Option<&Commander> {
        let p = self.active_project();
        match side {
            Side::Left => Some(&p.left),
            Side::Right => p.right.as_ref(),
        }
    }

    /// Whether the column on `side` is open. `Left` always is.
    pub fn has_col(&self, side: Side) -> bool {
        match side {
            Side::Left => true,
            Side::Right => self.active_project().right.is_some(),
        }
    }

    /// Every open commander, `left` first, mutably.
    pub fn columns_mut(&mut self) -> impl Iterator<Item = &mut Commander> {
        let p = self.active_project_mut();
        std::iter::once(&mut p.left).chain(p.right.as_mut())
    }

    /// Open `commander` as the second column, replacing one already open.
    pub fn open_second(&mut self, commander: Commander) {
        self.active_project_mut().right = Some(commander);
    }

    /// Close the second column. `false` when none was open.
    pub fn close_second(&mut self) -> bool {
        self.active_project_mut().right.take().is_some()
    }

    /// The `Side` `cur()` resolves to — `Right` iff a second commander is open
    /// AND focused, else `Left`. The side whose git a focus-scoped op targets.
    pub fn focused_side(&self) -> Side {
        let p = self.active_project();
        match (p.right.is_some(), p.vsplit.map(|v| v.focus)) {
            (true, Some(Side::Right)) => Side::Right,
            _ => Side::Left,
        }
    }

    /// The sides with a live commander: always `Left`, plus `Right` when a
    /// second commander is open. Holds no borrow of `self` (`use<>`), so a
    /// loop over it may mutate the columns it names.
    pub fn active_sides(&self) -> impl Iterator<Item = Side> + use<> {
        let has_right = self.active_project().right.is_some();
        std::iter::once(Side::Left).chain(has_right.then_some(Side::Right))
    }

    /// True when ANY active column is inside a git repo — gates the 1 Hz git
    /// poll (per-column git, so either column being in a repo means polling is
    /// worthwhile).
    pub fn any_git_repo(&self) -> bool {
        self.active_sides().any(|s| self.col(s).git.info.is_some())
    }

    /// The root that *project-scoped search tools* (grep `F`, find, MCP search)
    /// walk for a given column — the column's own **worktree root** when it's
    /// inside a repo, else `PROJECT_HOME`, else its listing dir. This is what
    /// makes those tools *follow the focused worktree*: column `b` sitting in a
    /// separate worktree scopes its tools to that worktree rather than the
    /// overall project anchor. In the common case (launched at the repo root,
    /// `PROJECT_HOME == repo root == worktree root`) all three agree, so
    /// single-column behaviour is unchanged. Always returns *some* dir (search
    /// of the current dir is always sensible); harpoon, which needs a stable
    /// anchor, uses the fallible [`Self::harpoon_root`] instead.
    pub fn tool_root(&self, side: Side) -> std::path::PathBuf {
        self.col(side)
            .git_cache
            .current_repo_root
            .clone()
            .or_else(|| self.active_project().home.clone())
            .unwrap_or_else(|| self.col(side).listing.dir.clone())
    }

    /// The directory a **worktree** operation anchors on: normally the focused
    /// column's own dir, but the repo/project root when that dir is a place with
    /// no directory behind it.
    ///
    /// Inside an archive mount the column's dir is the *container file's* path —
    /// `~/src/pkg.tar.gz` — and git discovery from there fails outright
    /// (`path is not a directory`), taking every worktree tool down with it while
    /// the user browses an archive. A worktree op is about the repo, not about
    /// where the cursor happens to be resting, so the mount case falls back to
    /// [`Self::tool_root`]; a mount clears the column's git state, so that
    /// resolves to `PROJECT_HOME` — the project the user is working on.
    ///
    /// Asked of the mount registry rather than by stat'ing the path: the Model
    /// already knows which paths are virtual, and it can answer without IO.
    pub fn worktree_anchor(&self) -> std::path::PathBuf {
        let dir = &self.cur().listing.dir;
        if self.mounts.contains(dir) {
            self.tool_root(self.focused_side())
        } else {
            dir.clone()
        }
    }

    /// The anchor a column's **harpoon** list keys off: its worktree root when
    /// in a repo, else `PROJECT_HOME`. Unlike [`Self::tool_root`] there is *no*
    /// listing-dir fallback — harpoon stays disabled (`None`) outside a repo
    /// with no project, rather than spawning a bookmark file per random dir.
    /// `b` in a separate worktree keys off that worktree, so its absolute-path
    /// bookmarks never jump into `a`'s copy of a file.
    pub fn harpoon_root(&self, side: Side) -> Option<std::path::PathBuf> {
        self.col(side)
            .git_cache
            .current_repo_root
            .clone()
            .or_else(|| self.active_project().home.clone())
    }

    // --- Cursor/navigation (Phase 1) ---

    /// Set the active project's `start_dir` (the `` ` `` jump target) by
    /// reference, reusing the existing allocation where possible. Named so
    /// call sites read as one short statement instead of a multi-line
    /// `active_project_mut().start_dir.clone_from(...)` chain.
    pub fn set_start_dir(&mut self, dir: &PathBuf) {
        self.active_project_mut().start_dir.clone_from(dir);
    }

    /// `:startdir` with no argument — report the active project's current
    /// `` ` `` jump target.
    pub fn flash_start_dir(&mut self) {
        let dir = self.active_project().start_dir.display().to_string();
        self.flash_info(format!("start dir: {dir}"));
    }

    /// Set the active project's focus to `Pane` when `want_pane`, else back
    /// to the file list — the two outcomes session restore (and similar
    /// "did this bring a pane along" paths) ever lands keyboard focus on.
    pub fn set_focus_pane_or_list(&mut self, want_pane: bool) {
        self.active_project_mut().focus = if want_pane {
            Focus::Pane
        } else {
            Focus::FileList
        };
    }

    /// Record that `pane_id`'s agent self-reported a status this turn — see
    /// `AgentHudState::status_reported`.
    pub fn agent_hud_status_reported(&mut self, pane_id: &str) {
        self.active_project_mut()
            .pane
            .agent_hud
            .status_reported(pane_id);
    }

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

    /// Announce work that is starting, to be replaced or cleared when it lands.
    ///
    /// A flash has no lifetime of its own — it shows until something else
    /// replaces it — so an in-flight message left behind by a completion that had
    /// nothing to say goes on claiming the work is still running.
    pub fn flash_progress<S: Into<String>>(&mut self, text: S) {
        self.flash = Some(FlashMessage {
            text: text.into(),
            kind: FlashKind::Progress,
        });
    }

    /// Drop an in-flight message now that its operation has finished.
    ///
    /// Only ever clears a [`FlashKind::Progress`], so a real message that landed
    /// in the meantime — a warning about the archive, an error — survives.
    pub fn clear_progress_flash(&mut self) {
        if self
            .flash
            .as_ref()
            .is_some_and(|f| matches!(f.kind, FlashKind::Progress))
        {
            self.flash = None;
        }
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
            Err(e) => self.flash_info(format!("save error: {e:#}")),
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
        self.active_project()
            .home
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
/// → no stack blow-up on a deep tree either), with early termination once the
/// running count reaches `cap`. Returns a value `<= cap`; a return of exactly
/// `cap` means "at least `cap`" (the caller shows `N+`).
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
        let left = Commander {
            listing: Listing::empty(cwd.clone()),
            picks: Picks::new(),
            masks: IgnoreMasks::default(),
            temp_filter: None,
            sort_order: crate::fs::listing::SortMode::Name,
            sort_reversed: false,
            view: View::Dir,
            cursor: Cursor::new(),
            rows: Vec::new(),
            grid_dims: GridDims::new(1, 20),
            page: crate::ui::list_view::PageSpan::default(),
            list_generation: 0,
            git: GitState::default(),
            git_cache: GitCache::default(),
            harpoon: None,
            harpoon_filter_set: std::collections::HashSet::new(),
            pending_ghosts: std::collections::HashSet::new(),
        };
        let pane = PaneLayout {
            pane_height_pct: 30,
            ..Default::default()
        };
        let project = Project::new(
            None,
            left,
            cwd,
            Config::default(),
            UserKeymap::default(),
            pane,
        );
        let active = project.id;
        Self {
            mounts: Mounts::default(),
            projects: vec![project],
            active,
            in_background: false,
            git_worker_available: false,
            inventory: Inventory::new(),
            marks: Marks::default(),
            resolver: Resolver::new(),
            mouse_capture_override: None,
            status_flags_override: None,
            mode: Mode::Normal,
            session_name: None,
            session_id: None,
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
            pending_delete_preview: None,
            graveyard: Vec::new(),
            scope_registry: Vec::new(),
            codex_recovery: std::collections::HashMap::new(),
            user_host: "test@host".to_string(),
            pending_new_tab_cmd: None,
            pending_worktrees: None,
            pending_sessions: None,
            startup_commands: Vec::new(),
            frecency: Frecency::default(),
        }
    }
}

#[cfg(test)]
mod tests;
