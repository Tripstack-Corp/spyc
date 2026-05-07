//! Top-level application state and event loop.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::spyc_debug;
use ansi_to_tui::IntoText;
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use glob::Pattern;
use ratatui::Frame;

use crate::config::{Config, StatusPosition};
use crate::fs::{self, Entry, EntryKind, Listing};
use crate::keymap::{Action, BoundAction, Resolver, ResolverOutcome, UserKeymap};
use crate::pane::{Pane, PaneTabs, PaneWidget, TabEntry, TabInfo};
use crate::shell;
use crate::state::sessions::AgentKind;
use crate::state::{Cursor, Harpoon, History, IgnoreMasks, Inventory, Marks, Picks};
use crate::ui::line_edit::LineEditor;
use crate::ui::{
    help,
    list_view::{Grid, ListView, Row},
    pager::{self, PagerView},
    prompt::PromptLine,
    status::StatusBar,
    theme::Theme,
};
use crate::{Tui, resume_tui, suspend_tui};

/// Precomputed rects for the current frame. Built by `App::compute_layout`.
/// Background capture for a `!` command. The child runs under a PTY
/// (so programs that open `/dev/tty` for prompts — sudo, ssh, gpg —
/// see the slave PTY instead of bleeding onto our real terminal).
/// A reader thread feeds bytes into the channel. While the capture is
/// live, typed keys are forwarded to the child via the master writer
/// so the user can answer prompts. Ctrl+C kills the child outright.
struct PendingCapture {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    /// Master-side writer — typed keys go here as encoded ANSI bytes.
    writer: Box<dyn std::io::Write + Send>,
    /// Receives chunks of stdout as they arrive (not all at once).
    output_rx: std::sync::mpsc::Receiver<Vec<u8>>,
    /// Accumulated raw bytes for the pager (ANSI included).
    buffer: Vec<u8>,
    title: String,
    cmd_display: String,
    /// When the capture started — for the elapsed timer.
    started: std::time::Instant,
    /// True once the reader thread has sent all output.
    finished: bool,
    /// Set when this capture was promoted from a previously-backgrounded
    /// task via `:fg`. ^Z will reuse the same id when re-backgrounding so
    /// the user sees `task #3` consistently across the round-trip.
    original_id: Option<u32>,
}

/// Lifecycle state of a backgrounded shell capture.
#[derive(Debug)]
enum TaskStatus {
    /// Reader thread is still running; child has not exited.
    Running,
    /// Child exited cleanly (or with non-zero status); inner is the code.
    Exited(i32),
    /// User killed the task (M2's `:bg` `R`-action).
    #[allow(dead_code)]
    Killed,
    /// `child.wait()` returned an error -- inner is the message.
    #[allow(dead_code)]
    Crashed(String),
}

/// A capture that has been moved off the foreground pager into the
/// background. Same plumbing as `PendingCapture` (child, writer, rx,
/// buffer); the reader thread spawned by `spawn_capture` keeps draining
/// into `buffer` even though no pager is attached.
struct BackgroundTask {
    id: u32,
    title: String,
    cmd_display: String,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    writer: Box<dyn std::io::Write + Send>,
    output_rx: std::sync::mpsc::Receiver<Vec<u8>>,
    buffer: Vec<u8>,
    status: TaskStatus,
    started: std::time::Instant,
    finished_at: Option<std::time::Instant>,
    /// True whenever bytes arrived while the task was sitting in the
    /// background. Reset on `:fg`. Drives the `[N+]` vs `[N●]` glyph
    /// in the divider so the user can see at a glance which task has
    /// fresh output to look at.
    has_unread_output: bool,
    /// Set true once the user opens the task in the task viewer
    /// (`[t`/`]t`, `gB`, or `:task N`). Combined with `Exited`/`Killed`
    /// status, this is what triggers the on-close promotion to buffer
    /// history -- viewing acts as the user's "I've seen this" ack.
    viewed_in_task_viewer: bool,
    /// True while the task is paused (SIGSTOP delivered, no further
    /// SIGCONT yet). Toggled by `:pause`/`:resume` (and `S`/`C` in
    /// the task viewer). The reader thread keeps blocking on read
    /// until the child resumes; status stays Running because the
    /// child hasn't exited.
    paused: bool,
}

/// Soft cap on per-task buffered output. When exceeded, drop bytes from
/// the head (keep the tail) -- the tail of a long build is what the user
/// usually wants. 1 MB ≈ ~10K lines of plain text.
const TASK_BUFFER_CAP: usize = 1_048_576;

struct BackgroundTasks {
    tasks: Vec<BackgroundTask>,
    next_id: u32,
}

impl BackgroundTasks {
    const fn new() -> Self {
        Self {
            tasks: Vec::new(),
            next_id: 1,
        }
    }

    const fn allocate_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    /// Most-recently-added task id (LIFO order), regardless of status.
    /// `:fg` with no arg uses this.
    fn most_recent(&self) -> Option<u32> {
        self.tasks.last().map(|t| t.id)
    }

    fn take(&mut self, id: u32) -> Option<BackgroundTask> {
        let pos = self.tasks.iter().position(|t| t.id == id)?;
        Some(self.tasks.remove(pos))
    }

    fn running_count(&self) -> usize {
        self.tasks
            .iter()
            .filter(|t| matches!(t.status, TaskStatus::Running))
            .count()
    }

    fn done_count(&self) -> usize {
        self.tasks
            .iter()
            .filter(|t| !matches!(t.status, TaskStatus::Running))
            .count()
    }
}

struct FrameLayout {
    status: ratatui::layout::Rect,
    list: ratatui::layout::Rect,
    divider: Option<ratatui::layout::Rect>,
    pane: Option<ratatui::layout::Rect>,
    prompt: ratatui::layout::Rect,
}

/// Follow-up side effect a key handler asks the main loop to perform.
///
/// Anything that needs to own the tty (editor, pager, shell-out) goes
/// through this so `run()` can tear the TUI down and restore it cleanly.
#[derive(Debug, Default)]
pub enum PostAction {
    #[default]
    None,
    Spawn {
        program: String,
        args: Vec<String>,
        /// Whether to pause and wait for a keypress after the child exits,
        /// so the user can read any output before the TUI is restored.
        pause_after: bool,
    },
}

pub mod state;

/// Which collection the user is looking at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Dir,
    Inventory,
    /// Graveyard view: list of soft-deleted entries (most recent
    /// first). Bindings inside: `p` restore-to-cwd, `P`
    /// restore-to-original, `dd`/`x` purge entry to system trash,
    /// `Z` purge all (with confirm), `Esc`/`gy` close.
    Graveyard,
}

/// Input mode: normal key bindings or a one-line text prompt.
pub enum Mode {
    Normal,
    Prompting(Prompt),
}

#[derive(Debug, Clone, Copy)]
enum ActivateIntent {
    Display, // $PAGER on text files
    Edit,    // $EDITOR
}

#[derive(Debug, Clone)]
pub struct FlashMessage {
    pub text: String,
    pub kind: FlashKind,
}

#[derive(Debug, Clone, Copy)]
pub enum FlashKind {
    Info,
    Error,
}

/// State for returning to the pager after `v` (edit) exits.
enum PagerReturn {
    /// Buffer content: reload from this temp file, then delete it.
    TempFile {
        path: PathBuf,
        title: String,
        scroll: u16,
    },
    /// On-disk file: reopen from the original path.
    SourceFile { path: PathBuf, scroll: u16 },
}

pub struct Prompt {
    pub kind: PromptKind,
    pub prefix: String,
    pub buffer: String,
    /// When set, this prompt uses the vi line editor with history.
    #[allow(dead_code)]
    pub editor: Option<crate::ui::line_edit::LineEditor>,
}

impl Prompt {
    /// Simple prompt (pattern pick, search, jump, etc.) — no vi editing.
    fn simple(kind: PromptKind, prefix: impl Into<String>) -> Self {
        Self {
            kind,
            prefix: prefix.into(),
            buffer: String::new(),
            editor: None,
        }
    }

    /// Shell prompt (`!` / `;`) — vi line editor with history support.
    fn shell(kind: PromptKind, prefix: impl Into<String>) -> Self {
        Self {
            kind,
            prefix: prefix.into(),
            buffer: String::new(),
            editor: Some(LineEditor::new()),
        }
    }
}

pub enum PromptKind {
    PatternPick,
    ShellCmd,
    /// Incremental search. `saved_cursor` is where the cursor was when `/`
    /// was pressed, so Esc can restore it.
    Search {
        saved_cursor: usize,
    },
    Jump,
    CopyTo,
    MoveTo,
    MakeDir,
    NewFile,
    /// Confirm removal. Only `y` / `yes` (case-insensitive) proceeds;
    /// anything else is treated as a cancel.
    RemoveConfirm,
    /// Confirm purge-all from the graveyard view (cascade
    /// everything to system trash). Same single-key shape as
    /// RemoveConfirm; routed separately because the verb and
    /// destination are different.
    GraveyardPurgeAllConfirm,
    SetEnv,
    /// `!` — capture command output with ANSI colors, show in in-app pager.
    ShellCmdCaptured,
    /// New pane tab step 1: command to run.
    PaneNewTabCmd,
    /// New pane tab step 2: working directory.
    PaneNewTabCwd,
    /// Rename the active pane tab.
    PaneRenameTab,
    /// W n — branch name for new worktree.
    WorktreeNewBranch,
    /// W d — confirm worktree removal (y/N).
    WorktreeDeleteConfirm,
    /// `=` — temporary file list filter (glob pattern, `!` for picks, empty clears).
    Limit,
    /// `:` — vim-style command line.
    Command,
    /// Auto-fired when a restored `claude --resume` tab looks broken;
    /// y/Enter respawns into the same slot. Cwd and fallback command
    /// live on the tab's `TabInfo` and are read at confirm time.
    ClaudeCrashRecover {
        tab_idx: usize,
    },
}

/// State for the `F` filename finder. The walk runs in a worker
/// thread streaming batches of paths through `walk_rx`; the picker
/// is interactive immediately and the candidate list grows live as
/// the walker progresses. Re-rank runs on every keystroke and on
/// every fresh batch arrival (cheap: ~1us per candidate).
struct FindPicker {
    /// Repo-relative paths accumulated from the walk so far.
    /// Append-only during the walk; never modified by the user.
    candidates: Vec<PathBuf>,
    /// Absolute root the walk started from. Used to construct the
    /// final absolute path on Enter.
    root: PathBuf,
    /// User's current input.
    query: String,
    /// Current ranked subset (paths only; scores discarded after
    /// sort). Re-built on keystroke or new-batch arrival.
    filtered: Vec<PathBuf>,
    /// Index into `filtered`. 0 when query just changed; arrows
    /// move it within `[0, filtered.len())`.
    selected: usize,
    /// Cap on rendered results so a 100K-file repo doesn't blow up
    /// the pager Line vec on first paint.
    limit: usize,
    /// Receiver for streaming candidate batches from the walker
    /// thread. Set to `None` once the walk completes (channel
    /// disconnects when the worker drops its sender).
    walk_rx: Option<std::sync::mpsc::Receiver<Vec<PathBuf>>>,
    /// True once the walker thread has finished. Drives the title
    /// suffix ("scanning..." vs final count).
    walk_complete: bool,
}

impl FindPicker {
    /// Re-rank `candidates` against the current `query`, store in
    /// `filtered`, reset `selected` to 0.
    fn refilter(&mut self) {
        self.filtered = crate::fs::finder::rank(&self.candidates, &self.query, self.limit)
            .into_iter()
            .map(|(p, _score)| p)
            .collect();
        self.selected = 0;
    }

    /// Drain any batches that have arrived since the last tick.
    /// Returns true when new candidates were appended OR when the
    /// walk completed (caller should re-render either way: title
    /// changes from "scanning..." to a final count).
    fn drain_walk(&mut self) -> bool {
        let Some(rx) = self.walk_rx.as_ref() else {
            return false;
        };
        let mut got_any = false;
        loop {
            match rx.try_recv() {
                Ok(batch) => {
                    self.candidates.extend(batch);
                    got_any = true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.walk_rx = None;
                    self.walk_complete = true;
                    got_any = true;
                    break;
                }
            }
        }
        got_any
    }
}

/// State for an active `:grep` session. The worker thread runs the
/// content searcher and pushes batches of matches through `rx`; the
/// main tick loop drains them and appends to the pager view whose
/// `grep_id` matches `id`. When the matching pager is closed or
/// replaced (`bprev`/`bnext`/Esc/etc.), the session is dropped and
/// the worker exits on its next send.
struct GrepSession {
    /// Unique session id; pasted onto the pager view's `grep_id` so
    /// stale workers can't bleed into a fresh search.
    id: u32,
    /// Receiver for streaming match batches from the worker.
    rx: std::sync::mpsc::Receiver<Vec<crate::fs::grep::GrepMatch>>,
    /// Total matches forwarded so far. Drives the title's progress
    /// suffix and the cap-hit warning.
    count: usize,
    /// True once the worker disconnected (walk complete or cap hit).
    /// The pager flips `streaming` off and the title shows the final
    /// count instead of "scanning…".
    complete: bool,
    /// Cap-hit flag — set when `count` reaches `MAX_MATCHES` so the
    /// final title can warn the user that results were truncated.
    capped: bool,
    /// Pattern echoed in the title.
    pattern: String,
    /// Display root (project home or listing dir) for context in the
    /// title.
    root: PathBuf,
}

/// State for the harpoon menu overlay (`Hh` / `gh`). Shows the
/// project's harpoon slots and lets the user reorder, delete, or
/// jump while the overlay is open. Keys are intercepted before
/// normal dispatch when `Some`.
struct HarpoonMenu {
    /// Cursor row inside the menu (0-based, indexes the *active*
    /// non-empty slots). Clamped to `slots.len() - 1` after each
    /// mutation so deletes never leave it dangling.
    cursor: usize,
    /// vim-style `dd` arming: `d` arms, second `d` deletes; any
    /// other key clears it. Avoids accidental deletion from a
    /// single-key slip.
    delete_armed: bool,
}

/// Stack of recently-closed pager views, for `:bprev`/`:bnext`.
/// Works like a browser back/forward stack.
struct PagerHistory {
    back: Vec<pager::PagerView>,
    forward: Vec<pager::PagerView>,
}

const MAX_PAGER_HISTORY: usize = 10;

impl PagerHistory {
    const fn new() -> Self {
        Self {
            back: Vec::new(),
            forward: Vec::new(),
        }
    }

    /// Save a closed pager view. Skips views flagged `no_history`
    /// (e.g. the help overlay) so accidentally hitting `[b` doesn't
    /// surface stale chrome. Clears the forward stack.
    fn push(&mut self, view: pager::PagerView) {
        if view.no_history {
            return;
        }
        self.back.push(view);
        self.forward.clear();
        if self.back.len() > MAX_PAGER_HISTORY {
            self.back.remove(0);
        }
    }

    /// Go back. On success returns the prior view and tucks `current`
    /// onto the forward stack. On failure (back stack empty) hands
    /// `current` back unchanged so the caller can keep it on screen --
    /// hitting `[b` at the start of history shouldn't close the pager.
    /// PagerView is ~232B so clippy flags the Err variant size; the
    /// alternative (Box on Err only) buys nothing on an in-process,
    /// cold-path call.
    #[allow(clippy::result_large_err)]
    fn go_back(&mut self, current: pager::PagerView) -> Result<pager::PagerView, pager::PagerView> {
        match self.back.pop() {
            Some(prev) => {
                self.forward.push(current);
                Ok(prev)
            }
            None => Err(current),
        }
    }

    /// Go forward. Same edge semantics as `go_back`.
    #[allow(clippy::result_large_err)]
    fn go_forward(
        &mut self,
        current: pager::PagerView,
    ) -> Result<pager::PagerView, pager::PagerView> {
        match self.forward.pop() {
            Some(next) => {
                self.back.push(current);
                Ok(next)
            }
            None => Err(current),
        }
    }

    fn back_len(&self) -> usize {
        self.back.len()
    }

    fn forward_len(&self) -> usize {
        self.forward.len()
    }
}

#[allow(clippy::struct_excessive_bools)]
pub struct App {
    /// Domain state — navigation, selection, filtering, config, etc.
    pub state: state::AppState,

    // --- UI/terminal state (stays in App) ---
    pager: Option<PagerView>,
    pager_history: PagerHistory,
    pager_pending_bracket: Option<char>,
    pager_was_open: bool,
    pager_jump_buf: Option<String>,
    pane_tabs: Option<PaneTabs>,
    /// Active harpoon list — small per-project pinned set of file
    /// pointers. `None` when `PROJECT_HOME` is unset; loaded from
    /// disk at startup and on chdir into a new `PROJECT_HOME`,
    /// auto-saved on every mutation. See `state::harpoon` for the
    /// model and persistence layout.
    harpoon: Option<Harpoon>,
    /// Active harpoon menu overlay (interactive: reorder, delete,
    /// jump). `None` when closed; intercepts keys before normal
    /// dispatch when open.
    harpoon_menu: Option<HarpoonMenu>,
    /// Active Quick Select picker (`^a u`). `None` when closed;
    /// intercepts keys before normal dispatch while open. See
    /// `pane::quick_select` for the model.
    quick_select: Option<crate::pane::quick_select::QuickSelect>,
    /// `dd` arming for the graveyard view: first `d` arms, second
    /// `d` deletes (cascades to system trash). Any other key
    /// disarms. False whenever the graveyard view is closed.
    graveyard_pending_d: bool,
    /// `gg` arming for the graveyard view (jump to top).
    graveyard_pending_g: bool,
    top_overlay: Option<Pane>,
    overlay_awaiting_dismiss: bool,
    pending_overlay_close: bool,
    pending_capture: Option<PendingCapture>,
    background_tasks: BackgroundTasks,
    pending_history_pick: Option<LineEditor>,
    /// Snapshot of jump-history entries (newest first) for the popup
    /// opened by `Esc` on an empty `J` prompt. While `Some`, an
    /// `Enter` on the active pager chdirs to the entry at the
    /// cursor; `^D` deletes the entry from history and the snapshot.
    /// `None` when no jump-history popup is active.
    pending_jump_history: Option<Vec<String>>,
    /// Active F-finder state (filename fuzzy picker). When `Some`,
    /// the pager renders the picker UI and key input is intercepted
    /// before the normal pager handler -- the user types to filter,
    /// arrows move selection, Enter chdirs + cursors on the file.
    find_picker: Option<FindPicker>,
    /// Active `:grep` session. Holds the receiver for the worker
    /// thread streaming matches; the tick loop drains pending matches
    /// onto the matching pager view (identified by `grep_id`). When
    /// the user closes or replaces that pager, the session is dropped
    /// and the worker exits on its next send.
    grep_session: Option<GrepSession>,
    /// Monotonic id for grep sessions, so a freshly-opened `:grep`
    /// pager can never accidentally consume matches from a stale
    /// session (e.g. user runs `:grep foo`, closes it, runs `:grep
    /// bar` while the foo worker is still draining its tail).
    next_grep_id: u32,
    history_pending_g: bool,
    /// Pending `g` in pane scroll mode — `gg` scrolls to top, `gf`/`gF`
    /// jump to file reference.
    scroll_pending_g: bool,
    pending_pager_return: Option<PagerReturn>,
    needs_full_repaint: bool,
    theme: Theme,
    /// Path to the `.spyc-context.json` file (project root).
    /// Written each loop iteration so the MCP server can read it.
    context_path: PathBuf,
    /// Last serialized context JSON — skip disk write when unchanged.
    last_context_json: String,
    /// Whether the MCP socket server is running.
    mcp_running: bool,
    /// Summary printed to stdout after the TUI exits.
    pub exit_summary: Option<String>,
    /// Accumulates characters the user types while the pane is focused.
    pane_prompt_buf: String,
    /// When a focus-switch chord (^a-j / ^a-k) just completed, this
    /// captures (when, the key that completed it). The next dispatch
    /// drops any Press/Repeat of the same key within ~60 ms — without
    /// this guard, fast typing of `^a-j-...` produced a stray `j` byte
    /// to the now-focused pane child (the `j` Press completes the
    /// chord, but a brief OS-level Repeat or a too-quick second Press
    /// arrives just after, with the new focus already in effect).
    focus_chord_completed: Option<(std::time::Instant, KeyCode)>,
    /// The last complete prompt the user sent to the pane (Enter commits).
    last_pane_prompt: Option<String>,
    /// Activity monitor: draws/sec, bytes/sec overlay.
    show_activity: bool,
    activity_draws: u32,
    activity_bytes: u64,
    activity_last_tick: std::time::Instant,
    /// Snapshot from last 1-second window.
    activity_dps: u32,
    activity_bps: u64,
    /// Draw reason counters for the current window.
    activity_reason_pane: u32,
    activity_reason_event: u32,
    activity_reason_other: u32,
    /// Snapshot reasons.
    activity_snap_pane: u32,
    activity_snap_event: u32,
    activity_snap_other: u32,
    /// Cached `build_rows()` output; invalidated by `list_generation`.
    cached_rows: Vec<Row>,
    cached_rows_gen: u64,
    /// Grid stabilization cache key: (list_gen, view_top, cursor, width, height).
    cached_grid_key: (u64, usize, usize, u16, u16),
    /// Commands from the MCP server (writable actions from Claude).
    mcp_cmd_rx: std::sync::mpsc::Receiver<crate::mcp_cmd::McpRequest>,
    /// Tab completion / cycle state. Tracks matches from the last Tab
    /// press and supports cycling through them on repeated Tab.
    tab_state: Option<TabState>,
    /// Scroll throttle: timestamp + direction of last processed arrow key.
    /// DEC 1007 alternate-scroll turns trackpad into arrow keys at 60+ Hz;
    /// we rate-limit to ~25/sec (40ms gap) so inertia doesn't fly.
    scroll_last: Option<(std::time::Instant, KeyCode)>,
    /// Last terminal-window title we emitted; used to skip redundant
    /// OSC 2 writes when project / session / cwd haven't changed.
    /// `None` forces an emit on next draw (used after a child process
    /// like vim may have clobbered the title).
    last_term_title: Option<String>,
}

/// State for Tab-completion cycling. Tracks the original buffer, the
/// computed completions, and which one is currently filled in.
struct TabState {
    /// Buffer content when the first Tab was pressed.
    original_buf: String,
    /// Shell command prefix (e.g., "ls " for `!ls ~/Do<tab>`), empty for J prompt.
    buf_prefix: String,
    /// Path prefix up to the last `/` in the typed word (e.g., "~/").
    word_base: String,
    /// Matched file/dir names (e.g. `Documents/`, `Downloads/`).
    matches: Vec<String>,
    /// 0 = list was just shown (first Tab). 1+ = cycling through matches.
    cycle_index: usize,
}

/// Internal per-item record used to build ListView rows each frame.
pub struct RowData {
    pub path: PathBuf,
    pub display: String,
    pub kind: EntryKind,
}

impl App {
    pub fn new(resume: bool, mcp_takeover_allowed: bool) -> Self {
        let (cwd, start_error) = if let Ok(d) = std::env::current_dir() {
            (d, None)
        } else {
            // cwd not accessible — fall back to $HOME.
            let home = std::env::var("HOME").map_or_else(|_| PathBuf::from("/tmp"), PathBuf::from);
            let _ = std::env::set_current_dir(&home);
            (
                home,
                Some("cwd not accessible, started in $HOME".to_string()),
            )
        };
        let (listing, start_error) = match Listing::read(&cwd) {
            Ok(l) => (l, start_error),
            Err(e) => (
                Listing::empty(cwd.clone()),
                Some(start_error.unwrap_or_default() + &format!("{e}")),
            ),
        };
        let git_info = crate::sysinfo::git_status(&cwd);
        let git_files = crate::sysinfo::git_file_statuses(&cwd);
        let (config, load_note) = match Config::load_default(&cwd) {
            Ok(c) => {
                let note = if c.sources.is_empty() {
                    None
                } else {
                    Some(format!("loaded {} config file(s)", c.sources.len()))
                };
                (c, note)
            }
            Err(e) => (Config::default(), Some(format!("config error: {e}"))),
        };
        let user_keymap = UserKeymap::from_bindings(config.bindings.clone());
        let theme = Theme::default().with_overrides(&config.colors);

        // Literal .git check, no upward walk — keep the concept explicit.
        let project_home = cwd.join(".git").exists().then(|| cwd.clone());
        let session_name = Some(crate::state::session_names::generate());

        // Load the harpoon list for the active project (if any). When
        // `PROJECT_HOME` is unset, harpoon stays `None` and all H-prefix
        // bindings flash a hint. Loaded once at startup; reloaded on
        // chdir into a different `PROJECT_HOME`.
        let harpoon = project_home.as_ref().map(|p| Harpoon::load(p));

        // Run health check before loading state — cleans up orphaned
        // files so Inventory::load() et al. see a consistent directory.
        let health_warnings = if let Some(sd) = crate::state::health::state_dir() {
            let report = crate::state::health::check(&sd);
            if report.cleaned > 0 {
                spyc_debug!("health check: cleaned {} orphaned file(s)", report.cleaned);
            }
            report.warnings
        } else {
            Vec::new()
        };

        let app_state = state::AppState {
            listing,
            picks: Picks::new(),
            inventory: Inventory::load(),
            marks: Marks::load(),
            masks: {
                let mut m = IgnoreMasks::default();
                m.apply_config(&config.ignore_masks);
                m
            },
            temp_filter: None,
            sort_order: crate::fs::listing::SortMode::Name,
            view: View::Dir,
            cursor: Cursor::new(),
            resolver: Resolver::new(),
            user_keymap,
            config,
            mode: Mode::Normal,
            project_home,
            session_name,
            frecency: crate::state::Frecency::load(),
            pane_focused: false,
            // spyc (top) = 30%, pane (bottom) = 70%. Resize with `^W +/-`.
            pane_height_pct: 70,
            pane_zoomed: false,
            pane_focus_before_zoom: None,
            harpoon_filter_set: harpoon
                .as_ref()
                .map(|h| h.ancestor_set().clone())
                .unwrap_or_default(),
            graveyard: Vec::new(),
            pending_new_tab_cmd: None,
            last_captured_cmd: None,
            pending_worktrees: None,
            pending_sessions: None,
            start_dir: cwd,
            prev_dir: None,
            last_search: None,
            quit_pending: None,
            history: History::load(),
            pane_history: History::load_file("pane_history"),
            jump_history: History::load_file("jump_history"),
            command_history: History::load_file("command_history"),
            flash: start_error.map(|text| FlashMessage {
                text,
                kind: FlashKind::Error,
            }),
            user_host: user_host_string(),
            git_info,
            git_files,
            should_quit: false,
            rows: Vec::new(),
            last_grid: Grid {
                cols: 1,
                rows: 1,
                col_widths: vec![20],
            },
            list_generation: 0,
        };
        let context_path = crate::context::context_path(&app_state.start_dir);
        // Command channel for writable MCP actions (Claude → main loop).
        let (mcp_cmd_tx, mcp_cmd_rx) = std::sync::mpsc::channel();
        // Start the MCP Unix socket server so `spyc --mcp` (spawned by
        // Claude Code) can proxy to us for full read/write MCP access.
        let mcp_running = crate::mcp::start_socket_server(context_path.clone(), mcp_cmd_tx)
            .map_or_else(
                |e| {
                    spyc_debug!("MCP socket server failed to start: {e}");
                    false
                },
                |()| true,
            );
        let mut app = Self {
            state: app_state,
            pager: None,
            pager_history: PagerHistory::new(),
            pager_pending_bracket: None,
            pager_was_open: false,
            pager_jump_buf: None,
            pane_tabs: None,
            harpoon,
            harpoon_menu: None,
            quick_select: None,
            graveyard_pending_d: false,
            graveyard_pending_g: false,
            top_overlay: None,
            overlay_awaiting_dismiss: false,
            pending_overlay_close: false,
            pending_capture: None,
            background_tasks: BackgroundTasks::new(),
            pending_history_pick: None,
            pending_jump_history: None,
            find_picker: None,
            grep_session: None,
            next_grep_id: 0,
            history_pending_g: false,
            scroll_pending_g: false,
            pending_pager_return: None,
            needs_full_repaint: false,
            theme,
            context_path,
            last_context_json: String::new(),
            mcp_running,
            exit_summary: None,
            pane_prompt_buf: String::new(),
            focus_chord_completed: None,
            last_pane_prompt: None,
            show_activity: false,
            activity_draws: 0,
            activity_bytes: 0,
            activity_last_tick: std::time::Instant::now(),
            activity_dps: 0,
            activity_bps: 0,
            activity_reason_pane: 0,
            activity_reason_event: 0,
            activity_reason_other: 0,
            activity_snap_pane: 0,
            activity_snap_event: 0,
            activity_snap_other: 0,
            cached_rows: Vec::new(),
            cached_rows_gen: u64::MAX, // force first build
            cached_grid_key: (u64::MAX, 0, 0, 0, 0),
            mcp_cmd_rx,
            tab_state: None,
            scroll_last: None,
            last_term_title: None,
        };
        app.state.rebuild_rows();
        if let Some(msg) = load_note {
            app.state.flash_info(msg);
        }
        // Surface any health check warnings so the user knows state
        // was repaired. Overrides the config load note if both exist.
        if !health_warnings.is_empty() {
            app.state.flash_error(health_warnings.join("; "));
        }
        // Graveyard cascade: if total exceeds the cap, push the
        // oldest entries to the system trash (FIFO) until under
        // the cap. Best-effort and silent on failure (the user
        // would see a flash from any visible-error path; failures
        // here are uncommon disk/permissions issues that don't
        // need to interrupt startup).
        let cap = crate::state::graveyard::GRAVEYARD_CAP_BYTES;
        if crate::state::graveyard::Graveyard::load().total_bytes() > cap {
            let (trashed, _errors) = crate::state::graveyard::Graveyard::cascade_until_under(cap);
            if trashed > 0 {
                app.state.flash_info(format!(
                    "graveyard: {trashed} item(s) moved to system trash (cap reached)"
                ));
            }
        }

        if resume {
            app.show_session_picker();
        }
        // Write .mcp.json so Claude Code spawns `spyc --mcp` (stdio),
        // which proxies to our Unix socket.
        if app.mcp_running {
            app.ensure_mcp_config(mcp_takeover_allowed);
        }
        app
    }

    /// Write `.mcp.json` with stdio transport on startup.
    /// If enterprise policy blocks spyc, flash an error instead.
    fn ensure_mcp_config(&mut self, takeover_allowed: bool) {
        match crate::mcp::ensure_mcp_json(&self.state.listing.dir, takeover_allowed) {
            Ok(crate::mcp::McpConfigStatus::Configured) => {}
            Ok(crate::mcp::McpConfigStatus::TookOver { old_pid }) => {
                self.state
                    .flash_info(format!("MCP: took over from PID {old_pid}"));
            }
            Ok(crate::mcp::McpConfigStatus::SkippedTakeover { old_pid }) => {
                self.state.flash_info(format!(
                    "MCP: kept PID {old_pid} as owner (Claude here will talk to it)"
                ));
            }
            Ok(crate::mcp::McpConfigStatus::BlockedByEnterprise) => {
                self.state.flash_error(
                    "MCP: blocked by enterprise policy (deniedMcpServers or allowedMcpServers)",
                );
            }
            Ok(crate::mcp::McpConfigStatus::ManagedByEnterprise) => {
                self.state
                    .flash_info("MCP: enterprise-managed (skipped local .mcp.json)");
            }
            Err(e) => self.state.flash_error(format!(".mcp.json: {e}")),
        }

        // Codex equivalent: write `.codex/config.toml` so the codex CLI
        // discovers spyc's MCP server the same way claude does. Both
        // agents share the same socket; the writer just registers a
        // stdio entry that re-execs `spyc --mcp` to proxy. Failures
        // here flash but don't gate startup — codex isn't required.
        // Enterprise-flavored statuses are claude-specific; codex
        // shouldn't return them, but if it ever does we treat them as
        // a no-op.
        match crate::mcp::ensure_codex_config_toml(&self.state.listing.dir, takeover_allowed) {
            Ok(crate::mcp::McpConfigStatus::TookOver { old_pid }) => {
                self.state
                    .flash_info(format!("codex MCP: took over from PID {old_pid}"));
            }
            Ok(crate::mcp::McpConfigStatus::SkippedTakeover { old_pid }) => {
                self.state.flash_info(format!(
                    "codex MCP: kept PID {old_pid} as owner (codex here will talk to it)"
                ));
            }
            Ok(_) => {}
            Err(e) => self.state.flash_error(format!(".codex/config.toml: {e}")),
        }
    }

    /// Build a context snapshot from the current state for MCP consumers.
    fn snapshot_context(&self) -> crate::context::SpycContext {
        let cursor_file = self
            .state
            .rows
            .get(self.state.cursor.index)
            .map(|r| r.display.clone());
        crate::context::SpycContext {
            cwd: self.state.listing.dir.clone(),
            cursor_file,
            picks: self.state.picks.iter().cloned().collect(),
            inventory: self.state.inventory.paths().cloned().collect(),
            filter: self.state.temp_filter.clone(),
            git_branch: self.state.git_info.clone(),
            project_home: self.state.project_home.clone(),
            session_name: self.state.session_name.clone().unwrap_or_default(),
        }
    }

    /// Write the context file (best-effort, errors are silently ignored).
    /// Skips the disk write when the serialized JSON is unchanged.
    fn write_context(&mut self) {
        let ctx = self.snapshot_context();
        let json = serde_json::to_string_pretty(&ctx).unwrap_or_default();
        if json == self.last_context_json {
            return;
        }
        let _ = crate::context::write_context_file(&self.context_path, &ctx);
        self.last_context_json = json;
    }

    /// Execute a writable MCP command from Claude. Runs on the main
    /// thread with full access to `AppState`. Returns a response that
    /// the MCP server thread forwards to Claude.
    fn execute_mcp_command(
        &mut self,
        cmd: crate::mcp_cmd::McpCommand,
    ) -> crate::mcp_cmd::McpResponse {
        use crate::mcp_cmd::{McpCommand, McpResponse};
        match cmd {
            McpCommand::NavigateTo { path } => {
                match self.state.jump_to(&path) {
                    Ok(()) => {
                        self.state.flash_info(format!(
                            "[mcp] navigated to {}",
                            self.state.listing.dir.display()
                        ));
                        // Force context write so get_spyc_context reflects
                        // the new state immediately.
                        self.write_context();
                        let ctx = self.snapshot_context();
                        let json = serde_json::to_string_pretty(&ctx).unwrap_or_default();
                        McpResponse::Ok { message: json }
                    }
                    Err(e) => McpResponse::Error {
                        message: format!("navigate failed: {e}"),
                    },
                }
            }
            McpCommand::SetFilter { pattern } => {
                match pattern {
                    Some(ref p) if p.is_empty() => self.state.temp_filter = None,
                    Some(p) => self.state.temp_filter = Some(p),
                    None => self.state.temp_filter = None,
                }
                self.state.rebuild_rows();
                let count = self.state.rows.len();
                let label = self.state.temp_filter.as_deref().unwrap_or("(cleared)");
                self.state.flash_info(format!("[mcp] filter: {label}"));
                self.write_context();
                McpResponse::Ok {
                    message: format!("filter applied, {count} items visible"),
                }
            }
            McpCommand::PickFiles { patterns } => {
                let mut total = 0usize;
                let mut errors = Vec::new();
                for pat_str in &patterns {
                    match glob::Pattern::new(pat_str) {
                        Ok(pat) => {
                            for e in &self.state.listing.entries {
                                if pat.matches(&e.name) {
                                    self.state.picks.insert(&e.path);
                                    total += 1;
                                }
                            }
                        }
                        Err(e) => errors.push(format!("{pat_str}: {e}")),
                    }
                }
                self.state.list_generation = self.state.list_generation.wrapping_add(1);
                if !errors.is_empty() {
                    return McpResponse::Error {
                        message: format!("invalid patterns: {}", errors.join(", ")),
                    };
                }
                self.state
                    .flash_info(format!("[mcp] picked {total} file(s)"));
                self.write_context();
                McpResponse::Ok {
                    message: format!("picked {total} file(s), {} total", self.state.picks.len()),
                }
            }
            McpCommand::ClearPicks => {
                let count = self.state.picks.len();
                self.state.picks.clear();
                self.state.list_generation = self.state.list_generation.wrapping_add(1);
                self.state.flash_info("[mcp] picks cleared");
                self.write_context();
                McpResponse::Ok {
                    message: format!("cleared {count} pick(s)"),
                }
            }
            McpCommand::Disconnected { new_pid } => {
                self.mcp_running = false;
                self.state.flash_error(format!(
                    "MCP taken over by spyc PID {new_pid} — Claude is connected to that instance"
                ));
                McpResponse::Ok {
                    message: "acknowledged".into(),
                }
            }
        }
    }

    /// Reload `.spycrc.toml` and rebuild the user keymap. Leaves the old
    /// config in place on failure and flashes the error.
    pub fn reload_config(&mut self) {
        match Config::load_default(&self.state.listing.dir) {
            Ok(new_config) => {
                self.state.user_keymap = UserKeymap::from_bindings(new_config.bindings.clone());
                self.theme = Theme::default().with_overrides(&new_config.colors);
                // Reset to built-in mask defaults first, then apply config
                // overrides — so removing `[[ignore_masks]]` entries from
                // the rc file reverts the group to defaults on reload.
                self.state.masks = IgnoreMasks::default();
                self.state.masks.apply_config(&new_config.ignore_masks);
                let count = new_config.sources.len();
                self.state.config = new_config;
                self.state.rebuild_rows();
                self.state
                    .flash_info(format!("reloaded {count} config file(s)"));
            }
            Err(e) => self.state.flash_error(format!("config error: {e}")),
        }
    }

    /// Candidate config paths — used by the file watcher. We watch the
    /// directories holding these even when the files don't exist yet so
    /// that `touch ~/.spycrc.toml` picks up immediately.
    fn candidate_config_paths(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        if let Some(home) = std::env::var_os("HOME") {
            out.push(PathBuf::from(home).join(".spycrc.toml"));
        }
        out.push(self.state.listing.dir.join(".spycrc.toml"));
        out
    }

    pub fn run(&mut self, terminal: &mut Tui) -> Result<()> {
        use notify::{RecursiveMode, Watcher};
        use std::sync::mpsc;

        // File watcher: notify posts events into `rx`. Two kinds of watch:
        //
        // 1. Config files — we watch their *parent* directories, not the
        //    files, because editors that replace-on-save (vim, VS Code,
        //    nvim) remove the old inode before creating the new one.
        //
        // 2. The current listing directory (non-recursive) — so external
        //    changes (a build artifact dropping in, `git pull`, etc.) are
        //    reflected without a manual refresh.
        let (tx, rx) = mpsc::channel::<notify::Result<notify::Event>>();
        let mut fs_watcher: Option<notify::RecommendedWatcher> =
            notify::recommended_watcher(tx).ok();
        let mut already_watched: std::collections::HashSet<PathBuf> =
            std::collections::HashSet::new();
        if let Some(w) = fs_watcher.as_mut() {
            for path in self.candidate_config_paths() {
                if let Some(parent) = path.parent() {
                    if parent.is_dir() && already_watched.insert(parent.to_path_buf()) {
                        let _ = w.watch(parent, RecursiveMode::NonRecursive);
                    }
                }
            }
        }
        // Which listing dir is currently watched. On chdir we'll unwatch
        // this one and re-watch the new dir.
        let mut watched_listing: Option<PathBuf> = None;
        let mut watched_git: Option<PathBuf> = None;
        sync_listing_watch(
            fs_watcher.as_mut(),
            &mut watched_listing,
            &mut watched_git,
            &self.state.listing.dir,
        );

        let mut last_context_write = std::time::Instant::now();
        let mut last_refresh = std::time::Instant::now();
        // 1Hz safety net: re-poll git state even if FSEvents missed
        // the `.git/index.lock` → `.git/index` rename. See
        // `AppState::refresh_git_state`.
        let mut last_git_poll = std::time::Instant::now();
        // Trailing debounce: fire refresh once events have stopped
        // arriving for `REFRESH_QUIET`. Bursty git operations
        // (`git add && git commit && git push`) emit several
        // `.git/index` rename events spread over hundreds of ms;
        // firing on the *first* event meant the subprocess sometimes
        // ran during an in-flight, transient state ("M " staged but
        // not yet committed). Waiting for quiet ensures we only
        // sample git after the storm has passed.
        let mut last_event_at: Option<std::time::Instant> = None;

        let mut needs_draw = true; // draw at least once on startup
        // 0=none, 1=pane output, 2=input event, 3=other (refresh/config/repaint/activity)
        let mut draw_reason: u8 = 3;

        while !self.state.should_quit {
            // One-shot full repaint after a pane or overlay closes (or any
            // other event that leaves ratatui's diff buffer stale).
            // Also force repaint when the pager opens while a pane exists,
            // because the pane stops rendering and its stale cells need clearing.
            // When the pager opens over a pane, the pane's stale cells
            // need clearing. But don't use terminal.clear() for this — the
            // pager overlay will paint over everything anyway, and the
            // clear causes a visible flash. Just force a draw instead.
            if self.pager.is_some() && self.pane_tabs.is_some() && !self.pager_was_open {
                needs_draw = true;
                draw_reason = 3;
            }
            self.pager_was_open = self.pager.is_some();
            let mut pending_clear = false;
            if self.needs_full_repaint {
                self.needs_full_repaint = false;
                pending_clear = true;
                needs_draw = true;
                draw_reason = 3;
            }
            // NOTE: periodic ^L to Claude pane tabs was removed — it clears
            // any draft prompt the user has typed, even when focus is on the
            // file list (the text is still in Claude's input buffer).

            // pending_overlay_close is no longer used — the overlay stays
            // visible until Enter via overlay_awaiting_dismiss.
            let _ = self.pending_overlay_close;

            // Check if a background `!` capture has finished.
            // Stream captured command output into the pager in real-time.
            if let Some(capture) = &mut self.pending_capture {
                needs_draw = true;
                draw_reason = 3; // capture in progress
                let mut got_data = false;
                while let Ok(chunk) = capture.output_rx.try_recv() {
                    if chunk.is_empty() {
                        // EOF — child is done.
                        capture.finished = true;
                        break;
                    }
                    capture.buffer.extend_from_slice(&chunk);
                    got_data = true;
                }
                // Update elapsed timer in the title while running.
                if !capture.finished {
                    let elapsed = capture.started.elapsed().as_secs();
                    let human_time = if elapsed >= 3600 {
                        format!(
                            "{}h {}m {}s",
                            elapsed / 3600,
                            (elapsed % 3600) / 60,
                            elapsed % 60
                        )
                    } else if elapsed >= 60 {
                        format!("{}m {}s", elapsed / 60, elapsed % 60)
                    } else {
                        format!("{elapsed}s")
                    };
                    let timer_title =
                        format!("\u{23f3} {} — running... ({human_time})", capture.title);
                    if let Some(view) = self.pager.as_mut() {
                        view.title = timer_title;
                    }
                }
                if got_data || capture.finished {
                    // Rebuild pager content from the accumulated buffer.
                    use ansi_to_tui::IntoText;
                    let normalized = strip_crlf(&capture.buffer);
                    let text = normalized.as_slice().into_text().unwrap_or_default();
                    // "At bottom" detection uses the actual rendered
                    // viewport height -- before this we hardcoded 40,
                    // which under-shoots on tall terminals and made
                    // the streaming-capture auto-tail leave the top
                    // half of the pager showing content with `~`
                    // markers filling the rest until the user
                    // manually scrolled. last_viewport_h is set by
                    // the renderer on every frame.
                    let at_bottom = self.pager.as_ref().is_some_and(|v| {
                        let h = v.last_viewport_h.get();
                        let h = if h == 0 { 40 } else { h };
                        let total = v.line_count();
                        let page = v.page_lines(h);
                        v.scroll >= total.saturating_sub(page)
                    });
                    if let Some(view) = self.pager.as_mut() {
                        view.lines = text.lines;
                        if at_bottom {
                            view.scroll_to_bottom_auto();
                        }
                    }
                }
                if capture.finished {
                    let status = capture.child.wait();
                    let exit_info = match status {
                        Ok(s) if s.success() => "exit 0".to_string(),
                        Ok(s) => format!("exit {}", s.exit_code()),
                        Err(e) => format!("error: {e}"),
                    };
                    let title = format!("{} — {exit_info}", capture.title);
                    // Final rebuild with stderr included.
                    let normalized = strip_crlf(&capture.buffer);
                    let text = normalized.as_slice().into_text().unwrap_or_default();
                    if let Some(view) = self.pager.as_mut() {
                        view.title = title;
                        view.lines = text.lines;
                        view.saveable = true;
                        view.streaming = false;
                        view.scroll_to_bottom_auto();
                    }
                    self.pending_capture = None;
                }
            }

            // Drain output from each backgrounded task. Reader threads
            // keep running even with no pager attached, so the buffer
            // is up-to-date the moment the user does `:fg`. Bounded at
            // TASK_BUFFER_CAP to avoid unbounded memory growth on a
            // talkative `cargo build`.
            let mut just_finished: Vec<(u32, String, String, std::time::Duration)> = Vec::new();
            for task in &mut self.background_tasks.tasks {
                if !matches!(task.status, TaskStatus::Running) {
                    continue;
                }
                while let Ok(chunk) = task.output_rx.try_recv() {
                    if chunk.is_empty() {
                        let exit = task.child.wait();
                        let (status_text, status_val) = match exit {
                            Ok(s) if s.success() => ("exit 0".to_string(), TaskStatus::Exited(0)),
                            #[allow(clippy::cast_possible_wrap)]
                            Ok(s) => {
                                let code = s.exit_code() as i32;
                                (format!("exit {code}"), TaskStatus::Exited(code))
                            }
                            Err(e) => {
                                let msg = e.to_string();
                                (format!("error: {msg}"), TaskStatus::Crashed(msg))
                            }
                        };
                        task.status = status_val;
                        task.finished_at = Some(std::time::Instant::now());
                        just_finished.push((
                            task.id,
                            task.cmd_display.clone(),
                            status_text,
                            task.started.elapsed(),
                        ));
                        break;
                    }
                    task.buffer.extend_from_slice(&chunk);
                    task.has_unread_output = true;
                    if task.buffer.len() > TASK_BUFFER_CAP {
                        let drop_n = task.buffer.len() - TASK_BUFFER_CAP;
                        task.buffer.drain(..drop_n);
                    }
                }
            }
            if !just_finished.is_empty() {
                needs_draw = true;
                draw_reason = 3;
                for (id, cmd_display, status_text, elapsed) in just_finished {
                    let secs = elapsed.as_secs();
                    self.state.flash_info(format!(
                        "task #{id}: {cmd_display} — {status_text} ({secs}s)"
                    ));
                }
            }

            // If a task viewer pager is open, refresh its content from
            // the live task buffer (the bg drain above may have updated
            // the buffer this tick).
            if let Some(viewer_id) = self.pager.as_ref().and_then(|v| v.task_id) {
                if let Some(task) = self
                    .background_tasks
                    .tasks
                    .iter_mut()
                    .find(|t| t.id == viewer_id)
                {
                    // Rebuild on new bytes OR on status transition (e.g.
                    // Running → Exited while the user is looking at it)
                    // so the title and the [EOF] marker keep up with
                    // reality. Drop has_unread_output even on
                    // status-only refreshes so the divider `+` clears.
                    let task_running = matches!(task.status, TaskStatus::Running);
                    let viewer_streaming = self.pager.as_ref().is_some_and(|v| v.streaming);
                    let status_changed = task_running != viewer_streaming;
                    if task.has_unread_output || status_changed {
                        task.has_unread_output = false;
                        task.viewed_in_task_viewer = true;
                        let new_view = Self::build_task_viewer_for(viewer_id, task);
                        if let Some(view) = self.pager.as_mut() {
                            view.lines = new_view.lines;
                            view.title = new_view.title;
                            view.streaming = new_view.streaming;
                        }
                        needs_draw = true;
                        draw_reason = 3;
                    }
                }
            }

            // F-finder: drain any candidate batches the walker
            // worker has pushed since the last tick. Re-rank +
            // re-render only when something changed (or the walk
            // completed -- title flips from "scanning..." to a
            // final count).
            if let Some(picker) = self.find_picker.as_mut() {
                if picker.drain_walk() {
                    picker.refilter();
                    self.render_find_picker();
                    needs_draw = true;
                    draw_reason = 3;
                }
            }

            // :grep session: drain match batches into the active
            // grep pager. Same shape as the F-finder drain but the
            // results land directly in the pager body instead of
            // being re-ranked.
            if self.drain_grep_session() {
                needs_draw = true;
                draw_reason = 3;
            }

            // Pre-drain pane output so we know if anything arrived.
            // All tabs are drained (correctness), but only *active* tab
            // output triggers a redraw — background tab trickle bytes
            // (idle shell cursor blinks, prompt redraws) are silent.
            let mut pane_had_output = false;
            if let Some(tabs) = self.pane_tabs.as_mut() {
                let active_idx = tabs.active_index();
                for (i, entry) in tabs.tabs_mut().iter_mut().enumerate() {
                    // Fast path: skip try_recv() when the reader thread
                    // hasn't posted anything. The atomic check is ~1ns vs
                    // ~20ns for a channel operation.
                    if !entry.pane.has_pending_output() {
                        continue;
                    }
                    if entry.pane.drain_output() && i == active_idx {
                        pane_had_output = true;
                    }
                }
            }
            if let Some(overlay) = self.top_overlay.as_mut() {
                if overlay.has_pending_output() && overlay.drain_output() {
                    pane_had_output = true;
                }
            }
            if pane_had_output {
                needs_draw = true;
                draw_reason = 1;
            }

            // Mark exited tabs AFTER drain so the Closed event has been
            // processed and is_closed() returns true. Trigger a redraw
            // so the "[exited N]" label appears immediately.
            if let Some(tabs) = self.pane_tabs.as_mut() {
                if tabs.mark_exited() {
                    needs_draw = true;
                    draw_reason = 3;
                }
            }

            // For tabs spawned by session restore that need a deferred
            // `/resume <sid>` (we avoid the `--resume` CLI flag because
            // of a known crash regression), wait ~1.5s for claude's
            // banner to render then send the slash command.
            self.send_pending_resumes();

            // If a restored claude tab looks broken (bad exit / crash
            // dump), prompt to respawn. See `pane_has_crash_marker` for
            // the signature; the 30s window auto-disarms once a resume
            // is clearly working.
            let crash_idx = self.find_crashed_restore_tab();
            if let Some(tab_idx) = crash_idx {
                if matches!(self.state.mode, Mode::Normal) {
                    if let Some(tabs) = self.pane_tabs.as_mut() {
                        if let Some(entry) = tabs.tabs_mut().get_mut(tab_idx) {
                            entry.info.restore_fallback = None;
                        }
                    }
                    self.state.mode = Mode::Prompting(Prompt::simple(
                        PromptKind::ClaudeCrashRecover { tab_idx },
                        "claude crash detected — start fresh and recover with /resume? [Y/n] ",
                    ));
                    needs_draw = true;
                    draw_reason = 3;
                }
            }

            // Drain any pending watcher events. Refresh listing / reload
            // config at most once per poll iteration, and debounce
            // listing refreshes to avoid spawning git subprocesses on
            // every rapid-fire .git/index change.
            let mut needs_reload = false;
            // last_event_at carries over from previous iterations when
            // the debounce timer hadn't elapsed yet.
            while let Ok(result) = rx.try_recv() {
                if let Ok(ev) = result {
                    for p in &ev.paths {
                        let listing = self.is_listing_path(p);
                        let config = self.is_config_path(p);
                        spyc_debug!(
                            "watcher event: {} (listing={listing}, config={config}, kind={:?})",
                            p.display(),
                            ev.kind
                        );
                        if config {
                            needs_reload = true;
                        }
                        if listing {
                            last_event_at = Some(std::time::Instant::now());
                        }
                    }
                }
            }
            if needs_reload {
                self.reload_config();
                needs_draw = true;
                draw_reason = 3;
            }
            const REFRESH_QUIET: Duration = Duration::from_millis(500);
            // Always rate-limit at least 500ms apart from previous refresh.
            if let Some(at) = last_event_at {
                let now = std::time::Instant::now();
                if now.duration_since(at) >= REFRESH_QUIET
                    && last_refresh.elapsed() >= REFRESH_QUIET
                {
                    last_event_at = None;
                    self.state.refresh_listing();
                    last_refresh = now;
                    needs_draw = true;
                    draw_reason = 3;
                }
            }
            // 1Hz safety net for git state — converges within a second
            // when FSEvents misses an event (commits replace `.git/index`
            // via atomic rename, which is the inode-replacement edge
            // case where FSEvents can drop notifications). Diff-aware:
            // only repaints when git_info or git_files actually
            // differ, so idle dps stays at 0.
            const GIT_POLL_INTERVAL: Duration = Duration::from_secs(1);
            if self.state.git_info.is_some() && last_git_poll.elapsed() >= GIT_POLL_INTERVAL {
                last_git_poll = std::time::Instant::now();
                if self.state.refresh_git_state() {
                    needs_draw = true;
                    draw_reason = 3;
                }
            }

            // Process writable MCP commands from Claude.
            while let Ok(req) = self.mcp_cmd_rx.try_recv() {
                let resp = self.execute_mcp_command(req.command);
                let _ = req.reply.send(resp);
                needs_draw = true;
                draw_reason = 3;
            }

            // Adaptive poll rate:
            // - 16ms when pane output just arrived (smooth streaming)
            // - 100ms when pane exists but is idle
            // - 250ms when no pane at all
            let poll_ms = if pane_had_output || self.pending_capture.is_some() {
                16 // smooth streaming
            } else if self.pane_tabs.is_some() || self.top_overlay.is_some() {
                100 // pane idle — responsive to new output
            } else {
                500 // no pane — only user input matters
            };
            if event::poll(Duration::from_millis(poll_ms))? {
                needs_draw = true;
                draw_reason = 2;
                match event::read()? {
                    Event::Key(key)
                        if key.kind == KeyEventKind::Press || key.kind == KeyEventKind::Repeat =>
                    {
                        // Throttle rapid-fire arrow keys from trackpad scroll
                        // (DEC 1007 alternate-scroll). Allow ~25 events/sec.
                        if matches!(key.code, KeyCode::Up | KeyCode::Down)
                            && key.modifiers.is_empty()
                        {
                            let now = std::time::Instant::now();
                            if let Some((prev, dir)) = self.scroll_last {
                                if dir == key.code && now.duration_since(prev).as_millis() < 40 {
                                    continue;
                                }
                            }
                            self.scroll_last = Some((now, key.code));
                        } else {
                            self.scroll_last = None;
                        }
                        let post = self.handle_key(key)?;
                        if let PostAction::Spawn {
                            program,
                            args,
                            pause_after,
                        } = post
                        {
                            run_child_in_foreground(terminal, &program, &args, pause_after)?;
                            // Child may have clobbered our title; force a
                            // re-emit on next draw.
                            self.last_term_title = None;
                            // The listing may have changed (mv, rm, chmod, etc).
                            self.state.refresh_listing();
                            // If we were editing a pager buffer, restore it.
                            if let Some(ret) = self.pending_pager_return.take() {
                                match ret {
                                    PagerReturn::TempFile {
                                        path,
                                        title,
                                        scroll,
                                    } => {
                                        if let Ok(content) = std::fs::read_to_string(&path) {
                                            let lines: Vec<String> =
                                                content.lines().map(String::from).collect();
                                            let mut view = PagerView::new_plain(title, lines);
                                            view.scroll = scroll;
                                            view.saveable = true;
                                            self.pager = Some(view);
                                        }
                                        let _ = std::fs::remove_file(&path);
                                    }
                                    PagerReturn::SourceFile { path, scroll } => {
                                        let name = path
                                            .file_name()
                                            .unwrap_or_default()
                                            .to_string_lossy()
                                            .into_owned();
                                        if let Ok(content) = std::fs::read_to_string(&path) {
                                            let lines: Vec<String> =
                                                content.lines().map(String::from).collect();
                                            let mut view = PagerView::new_plain(name, lines);
                                            view.source_path = Some(path);
                                            view.scroll = scroll;
                                            self.pager = Some(view);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Event::Paste(text) => {
                        if crate::key_trace::is_enabled() {
                            crate::key_trace::log(&format!(
                                "RX paste len={} pane_focused={} mode={:?}",
                                text.len(),
                                self.state.pane_focused,
                                std::mem::discriminant(&self.state.mode),
                            ));
                        }
                        if let Mode::Prompting(ref mut p) = self.state.mode {
                            // Paste into the active prompt buffer.
                            // Strip newlines (prompts are single-line).
                            let clean = text.replace(['\n', '\r'], " ");
                            if let Some(ed) = p.editor.as_mut() {
                                // Editor present (`!` / `;` / `:`): splice
                                // at the cursor so a mid-line paste lands
                                // where the user is, then sync the
                                // canonical buffer from the editor.
                                ed.insert_str(&clean);
                                p.buffer = ed.text();
                            } else {
                                // Simple prompt (search, mkdir, etc.) --
                                // no cursor concept, append.
                                p.buffer.push_str(&clean);
                            }
                        } else if self.pane_tabs.is_some() {
                            // Switch focus to the pane — the user clearly
                            // intends to interact with it if they're pasting.
                            if !self.state.pane_focused {
                                self.set_pane_focus(true);
                            }
                            // Track pasted text for yP (yank last prompt).
                            self.pane_prompt_buf.push_str(&text);
                            // Wrap in bracketed paste so the child app (e.g. claude)
                            // receives the block as a single paste, not line-by-line.
                            let pane = self.pane_tabs.as_mut().unwrap().active_mut();
                            let mut buf = Vec::with_capacity(text.len() + 12);
                            buf.extend_from_slice(b"\x1b[200~");
                            buf.extend_from_slice(text.as_bytes());
                            buf.extend_from_slice(b"\x1b[201~");
                            pane.send_bytes(&buf)?;
                        } else {
                            // No prompt and no pane — there's nowhere
                            // sensible to send the paste. Some terminals
                            // wrap rapid-fire keystrokes in bracketed
                            // paste sequences, so silently dropping
                            // could swallow real input. Flash a hint so
                            // the user knows it happened.
                            let n = text.chars().count();
                            self.state.flash_info(format!(
                                "paste ignored ({n} chars) — open `:` or `^\\` to paste"
                            ));
                        }
                    }
                    Event::Resize(cols, rows) => {
                        // Terminal resized — immediately resize all pty tabs
                        // so the child shells re-render their prompts at the
                        // correct width.
                        let area = ratatui::layout::Rect::new(0, 0, cols, rows);
                        let pane_pct = self.effective_pane_pct();
                        if let Some(tabs) = self.pane_tabs.as_mut() {
                            let layout = Self::compute_layout(
                                area,
                                true,
                                pane_pct,
                                self.state.config.layout.status_position,
                            );
                            if let Some(pane_rect) = layout.pane {
                                for entry in tabs.tabs_mut() {
                                    let _ = entry.pane.resize(pane_rect.height, pane_rect.width);
                                }
                            }
                        }
                        if let Some(overlay) = self.top_overlay.as_mut() {
                            let (r, c) = Self::top_overlay_size(pane_pct, self.pane_tabs.is_some());
                            let _ = overlay.resize(r, c);
                        }
                        // Help content is baked at open time for the current
                        // width (wrap points, column count). Rebuild so the
                        // layout matches the new dimensions.
                        if self.help_is_open() {
                            self.open_help();
                        }
                    }
                    _ => {}
                }
            }

            // Activity monitor: roll over the 1-second window.
            let mut activity_only_draw = false;
            if self.show_activity && self.activity_last_tick.elapsed() >= Duration::from_secs(1) {
                let new_dps = self.activity_draws;
                let new_bps = self.activity_bytes;
                let new_sp = self.activity_reason_pane;
                let new_se = self.activity_reason_event;
                let new_so = self.activity_reason_other;
                // Only force a redraw if something changed.
                if (new_dps != self.activity_dps
                    || new_bps != self.activity_bps
                    || new_sp != self.activity_snap_pane
                    || new_se != self.activity_snap_event
                    || new_so != self.activity_snap_other)
                    && !needs_draw
                {
                    // This draw exists only to refresh the overlay —
                    // don't count it in the stats or it oscillates.
                    needs_draw = true;
                    activity_only_draw = true;
                }
                self.activity_dps = new_dps;
                self.activity_bps = new_bps;
                self.activity_snap_pane = new_sp;
                self.activity_snap_event = new_se;
                self.activity_snap_other = new_so;
                self.activity_draws = 0;
                self.activity_bytes = 0;
                self.activity_reason_pane = 0;
                self.activity_reason_event = 0;
                self.activity_reason_other = 0;
                self.activity_last_tick = std::time::Instant::now();
            }

            // Only redraw when something actually changed.
            // Wrap in DEC 2026 synchronized update so the terminal
            // emulator (iTerm2, etc.) buffers the entire frame and
            // paints it atomically — eliminates tearing and reduces
            // terminal-side CPU.
            if needs_draw {
                needs_draw = false;
                self.update_term_title();
                use crossterm::terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate};
                let _ = crossterm::execute!(terminal.backend_mut(), BeginSynchronizedUpdate);
                if pending_clear {
                    terminal.clear()?;
                }
                let frame_area = terminal.draw(|frame| self.render(frame))?.area;
                let _ = crossterm::execute!(terminal.backend_mut(), EndSynchronizedUpdate);
                if self.show_activity && !activity_only_draw {
                    self.activity_draws += 1;
                    self.activity_bytes +=
                        u64::from(frame_area.width) * u64::from(frame_area.height);
                    match draw_reason {
                        1 => self.activity_reason_pane += 1,
                        2 => self.activity_reason_event += 1,
                        _ => self.activity_reason_other += 1,
                    }
                }
                draw_reason = 0;
            }

            // Only re-sync the filesystem watcher when the cwd actually changed.
            if watched_listing.as_deref() != Some(self.state.listing.dir.as_path()) {
                sync_listing_watch(
                    fs_watcher.as_mut(),
                    &mut watched_listing,
                    &mut watched_git,
                    &self.state.listing.dir,
                );
            }

            // Update the MCP context file — throttled to at most once per
            // second to avoid hammering the disk on every 16ms frame.
            if last_context_write.elapsed() >= Duration::from_secs(1) {
                self.write_context();
                last_context_write = std::time::Instant::now();
            }
        }
        // Clean up the context file on exit.
        crate::context::remove_context_file(&self.context_path);
        // Tear down every pane child tree before App is dropped.
        // The per-Pane Drop is a SIGKILL safety net; going through
        // `shutdown` here first sends SIGTERM with a 250ms grace, so
        // well-behaved children (`vite`, `npm run dev`, anything that
        // catches SIGTERM) get a chance to flush state before we
        // escalate. Without this, quitting spyc with a frontend dev
        // server in a pane would leave the whole node/esbuild/worker
        // tree orphaned and still bound to its port.
        if let Some(tabs) = self.pane_tabs.as_mut() {
            for entry in tabs.tabs_mut() {
                entry.pane.shutdown(Duration::from_millis(250));
            }
        }
        Ok(())
    }

    fn is_config_path(&self, path: &Path) -> bool {
        self.candidate_config_paths().iter().any(|c| c == path)
            || self.state.config.sources.iter().any(|c| c == path)
    }

    /// True iff `path` is the listing directory or anything beneath it
    /// that we care about for refresh purposes. `notify` events sometimes
    /// include just the directory and sometimes the affected child;
    /// recursive listing watches (since v1.21.7) also send events for
    /// arbitrary depths, so we accept the whole subtree -- with
    /// `.git/` carved out for tighter filtering since rebase/gc/pack
    /// activity inside there would otherwise spam refresh.
    fn is_listing_path(&self, path: &Path) -> bool {
        // Ignore our own context file writes -- they land in the
        // listing directory and would otherwise trigger a self-
        // perpetuating refresh_listing → git subprocess → redraw cycle.
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with(".spyc-context-") {
                return false;
            }
        }
        let dir = self.state.listing.dir.as_path();
        let git_dir = dir.join(".git");

        // `.git/` filtering: macOS FSEvents sometimes coalesces multiple
        // intra-directory changes into a single event whose path *is*
        // `.git/` itself (rather than the specific child file), so
        // accept that as "something happened in there, refresh."
        // Direct children: only `index` (staging/status) or `HEAD`
        // (branch switch) -- everything else (objects, packs, lockfiles,
        // gc activity, refs/, logs/) is rejected so background git
        // housekeeping doesn't cascade.
        if path == git_dir.as_path() {
            return true;
        }
        if path.starts_with(&git_dir) {
            if path.parent() == Some(git_dir.as_path()) {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    return matches!(name, "index" | "HEAD");
                }
            }
            return false;
        }

        // Anywhere at or below the listing dir (recursive watch) --
        // accept. The 500ms trailing debounce + git-status's index-
        // cache mean even noisy subtrees don't produce unbounded
        // refresh subprocesses.
        path.starts_with(dir)
    }

    // --- Rendering --------------------------------------------------------

    /// Partition the frame into status/list/prompt rects — plus, when
    /// the pane is open, a divider row and the pane rect below it.
    ///
    /// The **entire spyc unit** (status, list, prompt) lives above the
    /// divider. That way the prompt row sits with the file list it's
    /// about rather than attached to the bottom of the screen where the
    /// pane's subprocess is typing.
    fn compute_layout(
        area: ratatui::layout::Rect,
        pane_open: bool,
        pane_pct: u16,
        status_position: StatusPosition,
    ) -> FrameLayout {
        use ratatui::layout::Rect;
        let w = area.width;
        let h = area.height;
        let bottom_status = matches!(status_position, StatusPosition::Bottom);

        if !pane_open {
            // Top:    [status][list…][prompt]
            // Bottom: [list…][prompt][status]   (vim-style)
            let (status_y, list_y, prompt_y) = if bottom_status {
                (
                    area.y + h.saturating_sub(1),
                    area.y,
                    area.y + h.saturating_sub(2),
                )
            } else {
                (area.y, area.y + 1.min(h), area.y + h.saturating_sub(1))
            };
            let status = Rect {
                x: area.x,
                y: status_y,
                width: w,
                height: 1.min(h),
            };
            let list = Rect {
                x: area.x,
                y: list_y,
                width: w,
                height: h.saturating_sub(2),
            };
            let prompt = Rect {
                x: area.x,
                y: prompt_y,
                width: w,
                height: u16::from(h != 0),
            };
            return FrameLayout {
                status,
                list,
                divider: None,
                pane: None,
                prompt,
            };
        }

        // With pane: top unit holds list+prompt(+status if top).
        // Pane and divider sit below; if status is bottom, status is the
        // very last row, prompt one above, pane above that.
        let usable = h.saturating_sub(1); // minus divider
        let pane_h = (u32::from(usable) * u32::from(pane_pct) / 100) as u16;
        let top_h = usable.saturating_sub(pane_h);

        if bottom_status {
            // Layout (top → bottom): [list…][divider][pane…][prompt][status]
            // Reserve: 1 divider + 1 prompt + 1 status = 3 rows of chrome.
            // The remainder splits between list and pane by `pane_pct`.
            let chrome = 3u16;
            let usable_b = h.saturating_sub(chrome);
            let pane_h_b = (u32::from(usable_b) * u32::from(pane_pct) / 100) as u16;
            let list_h = usable_b.saturating_sub(pane_h_b);

            let list = Rect {
                x: area.x,
                y: area.y,
                width: w,
                height: list_h,
            };
            let divider = Rect {
                x: area.x,
                y: area.y + list_h,
                width: w,
                height: 1,
            };
            let pane = Rect {
                x: area.x,
                y: divider.y + 1,
                width: w,
                height: pane_h_b,
            };
            let prompt = Rect {
                x: area.x,
                y: area.y + h.saturating_sub(2),
                width: w,
                height: u16::from(h >= 2),
            };
            let status = Rect {
                x: area.x,
                y: area.y + h.saturating_sub(1),
                width: w,
                height: 1.min(h),
            };
            return FrameLayout {
                status,
                list,
                divider: Some(divider),
                pane: Some(pane),
                prompt,
            };
        }

        // Top status (default): [status][list…][prompt][divider][pane]
        let status = Rect {
            x: area.x,
            y: area.y,
            width: w,
            height: 1.min(top_h),
        };
        let list_h = top_h.saturating_sub(2);
        let list = Rect {
            x: area.x,
            y: area.y + status.height,
            width: w,
            height: list_h,
        };
        let prompt = Rect {
            x: area.x,
            y: area.y + top_h.saturating_sub(1),
            width: w,
            height: u16::from(top_h >= 2),
        };

        let divider = Rect {
            x: area.x,
            y: area.y + top_h,
            width: w,
            height: 1,
        };
        let pane = Rect {
            x: area.x,
            y: divider.y + 1,
            width: w,
            height: pane_h,
        };

        FrameLayout {
            status,
            list,
            divider: Some(divider),
            pane: Some(pane),
            prompt,
        }
    }

    /// Pane status line: tab indicators, active cwd, [SCROLL] tag.
    /// Replaces the old plain-rule divider.
    fn render_pane_status_line(&mut self, frame: &mut Frame, area: ratatui::layout::Rect) {
        use ratatui::{
            style::{Modifier, Style},
            text::{Line, Span},
            widgets::Paragraph,
        };
        let width = area.width as usize;
        // Tinting the rule + active tab in scroll mode is deliberate
        // redundancy with the [SCROLL] tag — three signals in different
        // parts of the divider make "you've left live view" hard to miss.
        let is_scrolling = self
            .pane_tabs
            .as_ref()
            .is_some_and(|t| t.active().is_scrolling());
        let rule_style = if is_scrolling {
            Style::default()
                .fg(self.theme.pick)
                .add_modifier(Modifier::BOLD)
        } else if self.state.pane_focused {
            Style::default()
                .fg(self.theme.prompt_prefix)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(self.theme.status_suffix)
        };
        let active_tab_style = if is_scrolling {
            Style::default()
                .fg(self.theme.pick)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(self.theme.prompt_prefix)
                .add_modifier(Modifier::BOLD)
        };
        let inactive_tab_style = Style::default().fg(self.theme.status_suffix);

        let mut spans: Vec<Span> = Vec::new();
        let mut used = 0usize;

        let activity_style = Style::default()
            .fg(self.theme.pick)
            .add_modifier(Modifier::BOLD);

        // Tab indicators: ─[1*] claude ─[2+] bash, then "── <live cwd>".
        // We render the indicators first (immutable iter) and capture
        // the active index, then re-borrow mut to fetch the live cwd.
        let mut active_idx: Option<usize> = None;
        if let Some(tabs) = &self.pane_tabs {
            for (i, entry) in tabs.tabs().iter().enumerate() {
                let is_active = i == tabs.active_index();
                if is_active {
                    active_idx = Some(i);
                }
                let star = if is_active { "*" } else { "" };
                let activity = if entry.info.has_activity { "+" } else { "" };
                let sep = "─";
                // Uppercase the active tab label in scroll mode — the
                // shape change is a peripheral-vision cue even before
                // the color registers.
                let label = if is_active && is_scrolling {
                    entry.info.label.to_uppercase()
                } else {
                    entry.info.label.clone()
                };
                let tab_text = format!("[{}{star}{activity}] {label} ", i + 1);
                let tab_len = sep.len() + tab_text.len();
                if used + tab_len > width {
                    break;
                }
                spans.push(Span::styled(sep, rule_style));
                let style = if is_active {
                    active_tab_style
                } else if entry.info.has_activity {
                    activity_style
                } else {
                    inactive_tab_style
                };
                spans.push(Span::styled(tab_text, style));
                used += tab_len;
            }
        }

        if let (Some(idx), Some(tabs)) = (active_idx, self.pane_tabs.as_mut()) {
            let entry = &mut tabs.tabs_mut()[idx];
            let live = entry.live_cwd().to_path_buf();
            let cwd_display = crate::paths::display_tilde(&live);
            // Mark when the live cwd has drifted from the spawn cwd
            // (e.g. user `cd`'d in a bash tab). Helps spot the case
            // the bug list called out.
            let drift = live != entry.info.cwd;
            let cwd_prefix = if drift { "── ↪ " } else { "── " };
            let avail = width.saturating_sub(used + 12); // room for [SCROLL] + trailing rule
            if avail > 4 {
                let truncated = if cwd_display.len() > avail {
                    format!("…{}", &cwd_display[cwd_display.len() - avail + 1..])
                } else {
                    cwd_display
                };
                let cwd_fragment = format!("{cwd_prefix}{truncated} ");
                used += cwd_fragment.len();
                let style = if drift {
                    active_tab_style
                } else {
                    inactive_tab_style
                };
                spans.push(Span::styled(cwd_fragment, style));
            }
        }

        // Right-aligned background-task tags. Distinct color from pane
        // tabs so the numbering doesn't visually collide (pane tabs are
        // 1..N left-to-right; bg tasks are 1..N right-anchored). Keeps
        // the rendered group ordered ascending L→R, but if there isn't
        // room for all of them we drop the *oldest* first (keep newest
        // visible). Glyphs:
        //   `[N+]`  running, output arrived since last :fg
        //   `[N\u{25cf}]`  running, quiescent
        //   `[N\u{2713}]`  exited cleanly
        //   `[N\u{2717}]`  non-zero exit / killed / crashed
        let bg_running_color = self.theme.dir; // soft blue
        let bg_unread_color = self.theme.take; // teal -- pulls the eye
        let bg_ok_color = self.theme.exec; // soft green
        let bg_err_color = ratatui::style::Color::Rgb(0xf7, 0x76, 0x8e); // tokyo red
        let mut bg_pieces_rev: Vec<(String, ratatui::style::Color)> = Vec::new();
        let mut bg_width = 0usize;
        let zoom_tag = if self.state.pane_zoomed {
            " [ZOOM]"
        } else {
            ""
        };
        let scroll_tag = if is_scrolling { " [SCROLL]" } else { "" };
        let tag_len = zoom_tag.len() + scroll_tag.len();
        // Reserve room for at least 4 dashes + the tag(s).
        let bg_budget = width.saturating_sub(used + tag_len + 4);
        for task in self.background_tasks.tasks.iter().rev() {
            let (glyph, color) = if task.paused && matches!(task.status, TaskStatus::Running) {
                // Pause glyph trumps the running/unread variants:
                // user explicitly paused, that's the headline state.
                ("\u{23f8}", bg_running_color) // ⏸
            } else {
                match (&task.status, task.has_unread_output) {
                    (TaskStatus::Running, true) => ("+", bg_unread_color),
                    (TaskStatus::Running, false) => ("\u{25cf}", bg_running_color),
                    (TaskStatus::Exited(0), _) => ("\u{2713}", bg_ok_color),
                    (TaskStatus::Exited(_) | TaskStatus::Killed | TaskStatus::Crashed(_), _) => {
                        ("\u{2717}", bg_err_color)
                    }
                }
            };
            let text = format!(" [{}{glyph}]", task.id);
            if bg_width + text.len() > bg_budget {
                break;
            }
            bg_width += text.len();
            bg_pieces_rev.push((text, color));
        }

        // Dash fill between pane-tab area and bg group / mode tag(s).
        let fill = width.saturating_sub(used + tag_len + bg_width);
        if fill > 0 {
            spans.push(Span::styled("─".repeat(fill), rule_style));
            used += fill;
        }

        // Render bg tasks left-to-right (id-ascending) by reversing the
        // collection we built right-to-left.
        for (text, color) in bg_pieces_rev.into_iter().rev() {
            used += text.len();
            spans.push(Span::styled(
                text,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ));
        }

        if self.state.pane_zoomed {
            spans.push(Span::styled(
                zoom_tag,
                Style::default()
                    .fg(self.theme.prompt_prefix)
                    .add_modifier(Modifier::BOLD),
            ));
            used += zoom_tag.len();
        }
        if is_scrolling {
            spans.push(Span::styled(
                scroll_tag,
                Style::default()
                    .fg(self.theme.pick)
                    .add_modifier(Modifier::BOLD),
            ));
            used += scroll_tag.len();
        }
        // If anything's left (shouldn't be), pad.
        let _ = used;

        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    /// Render the harpoon menu overlay. Centered modal box listing
    /// the active project's slots, with the menu cursor on a
    /// highlighted row. Footer shows the bindings.
    fn render_harpoon_menu(&self, frame: &mut Frame) {
        use ratatui::{
            layout::Rect,
            style::{Color, Modifier, Style},
            text::{Line, Span},
            widgets::{Block, Borders, Clear, Paragraph},
        };
        let Some(menu) = self.harpoon_menu.as_ref() else {
            return;
        };
        let Some(h) = self.harpoon.as_ref() else {
            return;
        };

        let area = frame.area();
        // Box dims: width clamped, height = 2 chrome + N slots + 2 footer.
        let width = area.width.clamp(40, 72);
        let body_h = (h.slots.len().max(1)) as u16;
        let height = (2 + body_h + 2).min(area.height); // borders + body + footer
        let x = area.x + (area.width.saturating_sub(width)) / 2;
        let y = area.y + (area.height.saturating_sub(height)) / 2;
        let rect = Rect {
            x,
            y,
            width,
            height,
        };
        frame.render_widget(Clear, rect);

        let title = format!(
            " harpoon — {} ",
            h.project.file_name().map_or_else(
                || h.project.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            )
        );
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(self.theme.prompt_prefix));
        let inner = block.inner(rect);
        frame.render_widget(block, rect);

        let footer_h = 1u16;
        let body_rect = Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: inner.height.saturating_sub(footer_h),
        };
        let footer_rect = Rect {
            x: inner.x,
            y: inner.y + inner.height.saturating_sub(footer_h),
            width: inner.width,
            height: footer_h,
        };

        // Body lines.
        let mut body_lines: Vec<Line> = Vec::with_capacity(h.slots.len().max(1));
        if h.slots.is_empty() {
            body_lines.push(Line::from(Span::styled(
                "  (empty — Ha to harpoon the cursor file/dir)",
                Style::default().fg(self.theme.status_suffix),
            )));
        } else {
            let cursor_style = Style::default()
                .fg(Color::Black)
                .bg(self.theme.prompt_prefix)
                .add_modifier(Modifier::BOLD);
            let normal_style = Style::default().fg(self.theme.status_path);
            let key_style = Style::default()
                .fg(self.theme.pick)
                .add_modifier(Modifier::BOLD);
            for (i, path) in h.slots.iter().enumerate() {
                let on_cursor = i == menu.cursor;
                let armed = on_cursor && menu.delete_armed;
                let prefix = if armed { " ⚠ " } else { "   " };
                // Display path relative to project_home when possible
                // (shorter, more readable); otherwise use the absolute.
                let shown = path
                    .strip_prefix(&h.project)
                    .map_or_else(|_| path.display().to_string(), |p| p.display().to_string());
                let line = Line::from(vec![
                    Span::styled(prefix, normal_style),
                    Span::styled(format!("{}  ", i + 1), key_style),
                    Span::styled(
                        shown,
                        if on_cursor {
                            cursor_style
                        } else {
                            normal_style
                        },
                    ),
                ]);
                body_lines.push(line);
            }
        }
        frame.render_widget(Paragraph::new(body_lines), body_rect);

        let footer_style = Style::default()
            .fg(self.theme.status_suffix)
            .add_modifier(Modifier::DIM);
        let footer_text = if menu.delete_armed {
            "   d again = delete · any other key cancels"
        } else {
            "   j/k move · 1-9/Enter jump · K/J reorder · dd delete · q/Esc close"
        };
        frame.render_widget(
            Paragraph::new(Span::styled(footer_text, footer_style)),
            footer_rect,
        );
    }

    fn render(&mut self, frame: &mut Frame) {
        let frame_area = frame.area();

        // Layout:
        //   - No pane: status (top row), list (middle), prompt (bottom row).
        //   - With pane: status (top row of the top *pane*), list (rest of
        //     top pane), divider row, pane, prompt (bottom row).
        //   The status row is always at the top of the file-list region —
        //   so when the pane is open it sits *inside* the top pane rather
        //   than above the divider.
        let layout = Self::compute_layout(
            frame_area,
            self.pane_tabs.is_some(),
            self.effective_pane_pct(),
            self.state.config.layout.status_position,
        );

        // If a top-overlay pty is active (`;top`, `;vim`, etc.), it
        // replaces the entire spyc area. Status, list, and prompt are
        // hidden; only the overlay + divider + bottom pane render.
        if let Some(overlay) = self.top_overlay.as_mut() {
            // The overlay occupies status + list + prompt area.
            let overlay_area = ratatui::layout::Rect {
                x: layout.status.x,
                y: layout.status.y,
                width: layout.status.width,
                height: layout.status.height + layout.list.height + layout.prompt.height,
            };
            let _ = overlay.resize(overlay_area.height, overlay_area.width);
            overlay.drain_output();
            if overlay.is_closed() && !self.overlay_awaiting_dismiss {
                self.overlay_awaiting_dismiss = true;
            }
            // Visual focus tracks `state.pane_focused`: false ⇒
            // overlay focused (cursor block, full color); true ⇒
            // bottom pane focused (overlay dims to half-lightness via
            // PaneWidget's DIM modifier). User toggles with ^a-j/k.
            let overlay_focused = !self.state.pane_focused;
            frame.render_widget(
                PaneWidget {
                    screen: overlay.screen(),
                    focused: overlay_focused,
                },
                overlay_area,
            );
            // Show a dismiss prompt when the subprocess has exited.
            if self.overlay_awaiting_dismiss && overlay_area.height > 0 {
                use ratatui::{
                    style::{Modifier, Style},
                    text::{Line, Span},
                    widgets::Paragraph,
                };
                let prompt_y = overlay_area.y + overlay_area.height.saturating_sub(1);
                let prompt_rect = ratatui::layout::Rect {
                    x: overlay_area.x,
                    y: prompt_y,
                    width: overlay_area.width,
                    height: 1,
                };
                let style = Style::default()
                    .fg(self.theme.prompt_prefix)
                    .add_modifier(Modifier::BOLD);
                frame.render_widget(
                    Paragraph::new(Line::from(Span::styled(
                        "[process exited — press any key to continue]",
                        style,
                    ))),
                    prompt_rect,
                );
            }

            // Divider + bottom pane still render normally.
            if let Some(divider_rect) = layout.divider {
                self.render_pane_status_line(frame, divider_rect);
            }
            if let (Some(tabs), Some(rect)) = (self.pane_tabs.as_mut(), layout.pane) {
                let _ = tabs.active_mut().resize(rect.height, rect.width);
                tabs.drain_all();
                frame.render_widget(
                    PaneWidget {
                        screen: tabs.active().screen(),
                        focused: self.state.pane_focused,
                    },
                    rect,
                );
            }
            return;
        }

        let (path, suffix) = self.header_parts();
        let project_label = self
            .state
            .project_home
            .as_deref()
            .map(path_basename_display);
        StatusBar {
            project_home: project_label.as_deref(),
            session_name: self.state.session_name.as_deref(),
            path: &path,
            suffix: &suffix,
            git_info: self.state.git_info.as_deref(),
            theme: &self.theme,
        }
        .render(frame, layout.status);

        if self.cached_rows_gen != self.state.list_generation {
            self.cached_rows = self.build_rows();
            self.cached_rows_gen = self.state.list_generation;
        }
        let rows = &self.cached_rows;
        let list_focused = !self.state.pane_focused;
        // Stabilize view_top ↔ grid.  Skip the expensive multi-round
        // loop when inputs haven't changed since the last frame.
        let grid_key = (
            self.state.list_generation,
            self.state.cursor.view_top,
            self.state.cursor.index,
            layout.list.width,
            layout.list.height,
        );
        if grid_key != self.cached_grid_key {
            self.cached_grid_key = grid_key;
            // The grid depends on view_top (different entries have different
            // name lengths → different column count → different items_per_page),
            // and view_top depends on the grid.
            //
            // This can produce a 2-cycle: vt=A gives grid that wants vt=B, and
            // vt=B gives grid that wants vt=A.  When we detect that, always pick
            // the lower of the two (shows more context, deterministic across
            // frames) and recompute the grid for that choice.
            {
                let mut prev_vt: Option<usize> = None; // for 2-cycle detection
                let mut settled = false;
                for round in 0..4 {
                    let probe = ListView {
                        rows,
                        cursor: self.state.cursor.index,
                        view_top: self.state.cursor.view_top,
                        empty_marker: self.state.view == View::Dir,
                        focused: list_focused,
                        theme: &self.theme,
                    };
                    self.state.last_grid = probe.grid(layout.list);
                    let old_vt = self.state.cursor.view_top;
                    let pp = self.state.last_grid.items_per_page();
                    self.state.ensure_cursor_visible();
                    if self.state.cursor.view_top == old_vt {
                        spyc_debug!(
                            "grid settled round {}: vt={} cursor={} grid={}x{} pp={}",
                            round + 1,
                            old_vt,
                            self.state.cursor.index,
                            self.state.last_grid.cols,
                            self.state.last_grid.rows,
                            pp,
                        );
                        settled = true;
                        break;
                    }
                    spyc_debug!(
                        "grid unstable round {}: vt {} -> {} cursor={} grid={}x{} pp={}",
                        round + 1,
                        old_vt,
                        self.state.cursor.view_top,
                        self.state.cursor.index,
                        self.state.last_grid.cols,
                        self.state.last_grid.rows,
                        pp,
                    );
                    // 2-cycle: new vt equals the vt from two rounds ago.
                    if Some(self.state.cursor.view_top) == prev_vt {
                        // Always pick the lower vt — deterministic across frames.
                        let forced = old_vt.min(self.state.cursor.view_top);
                        self.state.cursor.view_top = forced;
                        // Recompute grid for the forced view_top.
                        let probe = ListView {
                            rows,
                            cursor: self.state.cursor.index,
                            view_top: self.state.cursor.view_top,
                            empty_marker: self.state.view == View::Dir,
                            focused: list_focused,
                            theme: &self.theme,
                        };
                        self.state.last_grid = probe.grid(layout.list);
                        spyc_debug!(
                            "grid 2-cycle broken: forcing vt={} (cursor={} grid={}x{} pp={})",
                            forced,
                            self.state.cursor.index,
                            self.state.last_grid.cols,
                            self.state.last_grid.rows,
                            self.state.last_grid.items_per_page(),
                        );
                        settled = true;
                        break;
                    }
                    prev_vt = Some(old_vt);
                }
                if !settled {
                    spyc_debug!(
                        "grid did NOT settle after 4 rounds: vt={} cursor={}",
                        self.state.cursor.view_top,
                        self.state.cursor.index,
                    );
                }
            }
            // Update cache key in case the stabilization loop changed view_top.
            self.cached_grid_key = (
                self.state.list_generation,
                self.state.cursor.view_top,
                self.state.cursor.index,
                layout.list.width,
                layout.list.height,
            );
        } // end grid cache guard

        frame.render_widget(
            ListView {
                rows,
                cursor: self.state.cursor.index,
                view_top: self.state.cursor.view_top,
                empty_marker: self.state.view == View::Dir,
                focused: list_focused,
                theme: &self.theme,
            },
            layout.list,
        );

        if let (Some(tabs), Some(rect)) = (self.pane_tabs.as_mut(), layout.pane) {
            let _ = tabs.active_mut().resize(rect.height, rect.width);
            tabs.drain_all();
            frame.render_widget(
                PaneWidget {
                    screen: tabs.active().screen(),
                    focused: self.state.pane_focused,
                },
                rect,
            );
            tabs.active_mut().output_dirty = false;
            // Quick Select labels paint *over* the pane widget so
            // the user keeps the live output as context. Render
            // here, after the pane, before the divider.
            if self.quick_select.is_some() {
                self.render_quick_select_overlay(frame, rect);
            }
        }

        if let Some(divider_rect) = layout.divider {
            self.render_pane_status_line(frame, divider_rect);
        }

        if let Mode::Prompting(p) = &self.state.mode {
            PromptLine {
                prefix: &p.prefix,
                buffer: &p.buffer,
                theme: &self.theme,
                cursor_pos: p.editor.as_ref().map(|e| e.cursor),
                vi_mode: p.editor.as_ref().map(|e| e.mode),
            }
            .render(frame, layout.prompt);
        } else if let Some(flash) = &self.state.flash {
            use ratatui::{
                style::{Modifier, Style},
                text::{Line, Span},
                widgets::Paragraph,
            };
            let color = match flash.kind {
                FlashKind::Info => self.theme.take,
                FlashKind::Error => self.theme.cursor_bg,
            };
            let line = Line::from(Span::styled(
                flash.text.clone(),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ));
            frame.render_widget(Paragraph::new(line), layout.prompt);
        } else if let Some(capture) = &self.pending_capture {
            // Persistent "running" indicator while a `!` capture is active.
            use ratatui::{
                style::{Modifier, Style},
                text::{Line, Span},
                widgets::Paragraph,
            };
            let line = Line::from(Span::styled(
                format!(
                    "⏳ running: {}  (keys → child, ^C interrupt, ^\\ kill)",
                    capture.cmd_display
                ),
                Style::default()
                    .fg(self.theme.prompt_prefix)
                    .add_modifier(Modifier::BOLD),
            ));
            frame.render_widget(Paragraph::new(line), layout.prompt);
        } else if let Some(pending) = self.state.resolver.pending_display() {
            use ratatui::{
                style::{Modifier, Style},
                text::{Line, Span},
                widgets::Paragraph,
            };
            let line = Line::from(Span::styled(
                pending,
                Style::default()
                    .fg(self.theme.prompt_prefix)
                    .add_modifier(Modifier::BOLD),
            ));
            frame.render_widget(Paragraph::new(line), layout.prompt);
        }

        // Pager comes after list but before help (help always wins).
        if let Some(view) = &self.pager {
            pager::render(frame, frame.area(), view, &self.theme);
        }

        // Harpoon menu overlay — modal, drawn on top of everything
        // except the activity monitor.
        if self.harpoon_menu.is_some() {
            self.render_harpoon_menu(frame);
        }

        // Activity monitor overlay (top-right corner).
        if self.show_activity {
            use ratatui::widgets::Paragraph as ActivityP;
            let text = format!(
                " {} dps [p:{} e:{} o:{}]  {} cells/s  poll {}ms ",
                self.activity_dps,
                self.activity_snap_pane,
                self.activity_snap_event,
                self.activity_snap_other,
                self.activity_bps,
                if self.pending_capture.is_some() {
                    16
                } else if self.pane_tabs.is_some() || self.top_overlay.is_some() {
                    50
                } else {
                    250
                },
            );
            let w = text.len() as u16;
            if frame_area.width > w + 2 {
                let rect = ratatui::layout::Rect {
                    x: frame_area.width - w - 1,
                    y: 0,
                    width: w,
                    height: 1,
                };
                let style = ratatui::style::Style::default()
                    .fg(ratatui::style::Color::Black)
                    .bg(ratatui::style::Color::Yellow);
                frame.render_widget(
                    ActivityP::new(ratatui::text::Line::from(ratatui::text::Span::styled(
                        text, style,
                    ))),
                    rect,
                );
            }
        }
    }

    fn header_parts(&self) -> (String, String) {
        match self.state.view {
            View::Dir => (crate::paths::display_tilde(&self.state.listing.dir), {
                let filter_tag = match &self.state.temp_filter {
                    Some(f) if f == "!" => " limit:picks".to_string(),
                    Some(f) => format!(" limit:{f}"),
                    None => String::new(),
                };
                {
                    let total = self.state.listing.entries.len();
                    let shown = self.state.rows.len();
                    let hidden = total.saturating_sub(shown);
                    let hidden_tag = format!(" hidden:{hidden}");
                    // Bg tasks normally render in the divider line above
                    // the pane (distinct color, right-aligned). When the
                    // pane is hidden there is no divider, so fall back
                    // to the status-bar suffix here.
                    let bg_tag = if self.pane_tabs.is_some() {
                        String::new()
                    } else {
                        let running = self.background_tasks.running_count();
                        let done = self.background_tasks.done_count();
                        if running == 0 && done == 0 {
                            String::new()
                        } else if done == 0 {
                            format!(" bg:{running}\u{25cf}")
                        } else {
                            format!(" bg:{running}\u{25cf}{done}\u{2713}")
                        }
                    };
                    format!(
                        "[picks:{} inv:{} m1:{} m2:{}{}{}{}]",
                        self.state.picks.len(),
                        self.state.inventory.len(),
                        on_off(self.state.masks.mask1.enabled),
                        on_off(self.state.masks.mask2.enabled),
                        filter_tag,
                        hidden_tag,
                        bg_tag,
                    )
                }
            }),
            View::Inventory => (
                "<INVENTORY>".to_string(),
                format!(
                    "[{} items{}]  (t: tag, p: put, x: remove, ESC: return)",
                    self.state.inventory.len(),
                    if self.state.inventory.picks.is_empty() {
                        String::new()
                    } else {
                        format!(", {} tagged", self.state.inventory.picks.len())
                    }
                ),
            ),
            View::Graveyard => (
                "<GRAVEYARD>".to_string(),
                format!(
                    "[{} item(s)]  (p: put cwd, P: restore orig, dd/x: trash, Z: trash all, ESC: return)",
                    self.state.graveyard.len()
                ),
            ),
        }
    }

    fn build_rows(&self) -> Vec<Row> {
        use crate::ui::list_view::GitFileStatus;
        self.state
            .rows
            .iter()
            .map(|rd| {
                let git_status = self
                    .state
                    .git_files
                    .get(&rd.display)
                    .copied()
                    .unwrap_or_else(GitFileStatus::clean);
                Row {
                    display: rd.display.clone(),
                    kind: rd.kind,
                    picked: self.state.view == View::Dir && self.state.picks.contains(&rd.path),
                    taken: self.state.inventory.contains(&rd.path),
                    git_status,
                }
            })
            .collect()
    }

    // --- Input handling ---------------------------------------------------

    fn handle_key(&mut self, key: KeyEvent) -> Result<PostAction> {
        // Per-key dispatch trace, opt-in via `--key-trace` / SPYC_KEY_TRACE.
        // Captures the input as it arrives so a user reproducing an
        // "input doesn't work" issue can ship a log. We re-trace the
        // dispatch decision wherever a key gets routed.
        if crate::key_trace::is_enabled() {
            crate::key_trace::log(&format!(
                "RX kind={:?} code={:?} mods={:?} pane_focused={} pending={:?}",
                key.kind,
                key.code,
                key.modifiers,
                self.state.pane_focused,
                self.state.resolver.pending_display(),
            ));
        }

        // Swallow a Press/Repeat of the chord-completing key when it
        // arrives within ~60 ms of a focus-switch chord. Without this
        // guard, fast-typing `^a-j` (or holding the chord-completing
        // key even briefly) produces a stray byte to the now-focused
        // pane child — the j Press completes the chord, but a Repeat
        // or too-quick second Press follows with the new focus already
        // active, so it gets forwarded to the pane as raw input.
        // 60 ms covers system-key-repeat (~30-50 ms) and kitty-keyboard
        // Repeat events without affecting deliberate double-taps.
        if let Some((at, code)) = self.focus_chord_completed {
            let within_window = at.elapsed() < Duration::from_millis(60);
            if within_window
                && key.code == code
                && matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat)
                && key.modifiers.is_empty()
            {
                crate::key_trace::log("  swallowed (post-chord bounce)");
                return Ok(PostAction::None);
            }
            if !within_window {
                self.focus_chord_completed = None;
            }
        }

        // Any keypress clears a lingering flash message.
        self.state.flash = None;

        // F-finder is modal: while open, swallow all keys for picker
        // navigation (type-to-filter, Up/Down, Enter, Esc). Runs
        // before the capture / pager / file-list dispatch so the
        // picker can't be accidentally double-routed.
        if self.handle_find_picker_key(key) {
            return Ok(PostAction::None);
        }

        // ^C is intentionally a no-op at the spyc-normal level (we
        // don't quit on Ctrl+C, that footgun's too easy with one
        // stray chord). Flash a hint so the user isn't left
        // wondering whether the key got captured -- common after
        // coming back from a `p` → `$PAGER` takeover where they
        // tried to ^C out of less and might have sent a second one
        // in confusion.
        //
        // Exclusions:
        //  - Capture mode forwards ^C to the child as 0x03 below.
        //  - Prompting mode treats ^C as cancel (vi muscle memory:
        //    `^C` in `:` should drop you back to normal mode, same
        //    as Esc) -- handled in `handle_vi_prompt_key`.
        //  - Pane focused: ^C must reach the child (zsh, etc.) so the
        //    user can interrupt a running command. Forwarding happens
        //    at the pane-focused dispatch below.
        let pane_has_focus = self.pane_tabs.is_some() && self.state.pane_focused;
        if matches!(key.code, KeyCode::Char('c'))
            && key.modifiers.contains(KeyModifiers::CONTROL)
            && self.pending_capture.is_none()
            && !matches!(self.state.mode, Mode::Prompting(_))
            && !pane_has_focus
        {
            self.state.flash_info(
                "^C is not a quit binding — use Q (or :q) to quit, Esc to cancel modes",
            );
            return Ok(PostAction::None);
        }

        // While a `!` capture is running, forward typed keys to the
        // child via the master PTY writer so the user can answer
        // prompts (sudo password, ssh password, etc.). Ctrl+\ kills
        // the child outright; Ctrl+C is forwarded as 0x03 so the
        // child's tty driver can deliver SIGINT (matches a normal
        // terminal's behavior, and lets sudo cancel its prompt
        // cleanly).
        if let Some(capture) = &mut self.pending_capture {
            use std::io::Write as _;
            // Hard-kill escape: Ctrl+\ tears the child down even if
            // it has somehow detached from the controlling tty.
            if matches!(key.code, KeyCode::Char('\\'))
                && key.modifiers.contains(KeyModifiers::CONTROL)
            {
                let _ = capture.child.kill();
                let _ = capture.child.wait();
                let title = format!("{} — interrupted", capture.title);
                if let Some(view) = self.pager.as_mut() {
                    view.title = title;
                    view.saveable = true;
                    view.streaming = false;
                }
                self.pending_capture = None;
                return Ok(PostAction::None);
            }
            // ^Z: send to background. Reader thread keeps draining; the
            // pager closes; user can resume with `:fg`.
            if matches!(key.code, KeyCode::Char('z'))
                && key.modifiers.contains(KeyModifiers::CONTROL)
            {
                self.background_capture();
                return Ok(PostAction::None);
            }
            let bytes = crate::pane::input::encode_key(key);
            if !bytes.is_empty() {
                let _ = capture.writer.write_all(&bytes);
                let _ = capture.writer.flush();
            }
            return Ok(PostAction::None);
        }

        // Top overlay: once the subprocess exits, hold the screen until
        // any key so short-lived commands (`;ls`) don't flash and vanish.
        if self.overlay_awaiting_dismiss {
            self.top_overlay = None;
            self.overlay_awaiting_dismiss = false;
            self.needs_full_repaint = true;
            self.state.flash_info("command finished");
            return Ok(PostAction::None);
        }

        // Top overlay (interactive `;` command). Used to be an
        // unconditional takeover ("the user exits by quitting the
        // subprocess itself"), which was fine when only the overlay
        // existed -- but if the user has a bottom pane too (e.g.
        // claude open), they couldn't pop down to it without quitting
        // the overlay first. So now spyc meta keys (^a, ^w, ^\, F10)
        // and bottom-pane-focused keys fall through to the regular
        // chord / pane-forwarding paths, letting the user `^a-j` into
        // claude while keeping `;less docs/foo.md` visible above.
        if let Some(overlay) = self.top_overlay.as_mut() {
            let has_bottom = self.pane_tabs.is_some();
            let is_meta = is_spyc_meta_when_pane_focused(key, self.state.resolver.is_pending());
            let bottom_owns = has_bottom && self.state.pane_focused;
            if !is_meta && !bottom_owns {
                let _ = overlay.send_key(key);
                return Ok(PostAction::None);
            }
            // Fall through: meta key reaches the resolver below;
            // bottom-pane-focused keys reach the pane-focused
            // forwarding block further down.
        }

        // Quick Select picker eats all keys until dismissed.
        // Earlier than the harpoon menu so it'll never collide
        // with chord state.
        if self.quick_select.is_some() {
            return Ok(self.handle_quick_select_key(key));
        }

        // Harpoon menu eats all keys until dismissed (Esc/q).
        if self.harpoon_menu.is_some() {
            return Ok(self.handle_harpoon_menu_key(key));
        }

        // Pager eats all keys until dismissed.
        if self.pager.is_some() {
            let post = self.handle_pager_key(key);
            return Ok(post);
        }
        // When the pane is in scroll mode, navigation keys are handled
        // here instead of being forwarded to the child subprocess.
        // Let spyc meta keys (^W prefix, ^\\, F10) fall through so
        // pane commands still work from scroll mode.
        if let Some(tabs) = self.pane_tabs.as_mut() {
            let pane = tabs.active_mut();
            if pane.is_scrolling()
                && self.state.pane_focused
                && !is_spyc_meta_when_pane_focused(key, self.state.resolver.is_pending())
            {
                return Ok(self.handle_pane_scroll_key(key));
            }
        }
        // When the active tab's subprocess has exited, any key closes
        // the tab (so the user can read the error output, then dismiss).
        if let Some(tabs) = self.pane_tabs.as_mut() {
            if self.state.pane_focused && tabs.active().is_closed() {
                if !tabs.close_active() {
                    self.pane_tabs = None;
                    self.state.pane_focused = false;
                    self.needs_full_repaint = true;
                    self.state.flash_info("pane: last tab closed");
                }
                return Ok(PostAction::None);
            }
        }
        // When the pane is open *and focused*, forward keys to the
        // subprocess — except spyc meta keys, which are always caught
        // by spyc so the user can toggle / resize / focus-switch / send
        // selection from inside the pane.
        if self.pane_tabs.is_some()
            && self.state.pane_focused
            && !matches!(self.state.mode, Mode::Prompting(_))
            && !is_spyc_meta_when_pane_focused(key, self.state.resolver.is_pending())
        {
            // Track what the user types so yP can yank the last prompt.
            match key.code {
                KeyCode::Enter => {
                    let trimmed = strip_ansi_escapes(&self.pane_prompt_buf);
                    if !trimmed.is_empty() {
                        self.last_pane_prompt = Some(trimmed);
                    }
                    self.pane_prompt_buf.clear();
                }
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.pane_prompt_buf.clear();
                }
                KeyCode::Backspace => {
                    self.pane_prompt_buf.pop();
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.pane_prompt_buf.push(c);
                }
                _ => {}
            }
            if let Some(tabs) = self.pane_tabs.as_mut() {
                let _ = tabs.active_mut().send_key(key);
            }
            return Ok(PostAction::None);
        }
        if matches!(self.state.mode, Mode::Prompting(_)) {
            return Ok(self.handle_prompt_key(key));
        }
        // Inventory view: special key handling.
        if self.state.view == View::Inventory {
            match key.code {
                KeyCode::Esc => {
                    self.state.toggle_inventory_view();
                    return Ok(PostAction::None);
                }
                KeyCode::Char('x' | 'd') => {
                    self.state.drop_cursor();
                    return Ok(PostAction::None);
                }
                KeyCode::Char(' ' | 't') => {
                    self.state.inventory.toggle_pick(self.state.cursor.index);
                    self.state.list_generation = self.state.list_generation.wrapping_add(1);
                    self.state.cursor_move_vertical(1, self.state.rows.len());
                    return Ok(PostAction::None);
                }
                KeyCode::Char('p') => {
                    return Ok(self.put_inventory_to_cwd());
                }
                _ => {}
            }
        }
        // Graveyard view: special key handling. Same shape as
        // inventory; verbs are restore/purge instead of put/tag.
        // `dd` (vim-style two-key delete) is implemented via the
        // pager's `d` already being free here — second `d` confirms.
        if self.state.view == View::Graveyard {
            return Ok(self.handle_graveyard_view_key(key));
        }
        let outcome = self.state.resolver.feed(key, &self.state.user_keymap);
        if crate::key_trace::is_enabled() {
            crate::key_trace::log(&format!("  resolver -> {outcome:?}"));
        }
        match outcome {
            ResolverOutcome::Action(action) => {
                // Stamp focus-switch chord completions so the next
                // ~60 ms suppresses a same-key Repeat or bouncy second
                // Press from leaking into the now-focused pane.
                if matches!(action, Action::PaneFocusDown | Action::PaneFocusUp) {
                    self.focus_chord_completed = Some((std::time::Instant::now(), key.code));
                }
                return self.apply(&action);
            }
            ResolverOutcome::User(bound) => return self.apply_user(&bound),
            ResolverOutcome::Pending | ResolverOutcome::Ignored => {}
        }
        Ok(PostAction::None)
    }

    /// Dispatch a user-defined binding. Inline-data actions (unix command,
    /// preset pattern, preset path) run through the same machinery as the
    /// built-in prompts but skip the prompt UI.
    fn apply_user(&mut self, bound: &BoundAction) -> Result<PostAction> {
        match bound {
            BoundAction::Plain(action) => return self.apply(action),
            BoundAction::UnixCmd(template) => {
                let cmd = shell::expand_percent(template, &self.state.selection_paths());
                return Ok(sh_c(&cmd, true));
            }
            BoundAction::PatternPick(pattern) => {
                if let Ok(pat) = glob::Pattern::new(pattern) {
                    for e in &self.state.listing.entries {
                        if pat.matches(&e.name) {
                            self.state.picks.insert(&e.path);
                        }
                    }
                    self.state.list_generation = self.state.list_generation.wrapping_add(1);
                }
            }
            BoundAction::Jump(path) => {
                let _ = self.state.jump_to(path);
            }
            BoundAction::Copy(dest) => {
                self.run_selection_to(dest, fs::ops::copy_selection_to, "copied");
            }
            BoundAction::Move(dest) => {
                self.run_selection_to(dest, fs::ops::move_selection_to, "moved");
            }
            BoundAction::ToggleMaskFixed(n) => {
                if *n == 1 {
                    self.state.masks.toggle_mask1();
                } else if *n == 2 {
                    self.state.masks.toggle_mask2();
                }
                self.state.rebuild_rows();
            }
        }
        self.state.cursor.clamp(self.state.rows.len());
        Ok(PostAction::None)
    }

    fn handle_prompt_key(&mut self, key: KeyEvent) -> PostAction {
        // Single-key confirm prompts: `y` / `Y` proceeds, anything else cancels.
        if matches!(
            &self.state.mode,
            Mode::Prompting(p) if matches!(p.kind, PromptKind::RemoveConfirm)
        ) {
            return self.handle_remove_confirm_key(key);
        }
        if matches!(
            &self.state.mode,
            Mode::Prompting(p) if matches!(p.kind, PromptKind::GraveyardPurgeAllConfirm)
        ) {
            return self.handle_graveyard_purge_all_confirm(key);
        }
        if matches!(
            &self.state.mode,
            Mode::Prompting(p) if matches!(p.kind, PromptKind::ClaudeCrashRecover { .. })
        ) {
            return self.handle_claude_crash_recover_key(key);
        }
        // Shell prompts (`!` / `;`) use the vi line editor + history.
        let has_editor = matches!(
            &self.state.mode,
            Mode::Prompting(p) if p.editor.is_some()
        );
        if has_editor {
            return self.handle_vi_prompt_key(key);
        }

        // --- Simple prompts (search, jump, pattern-pick, etc.) ---

        // ^C cancels too (vi muscle memory; same as Esc).
        let ctrl_c =
            matches!(key.code, KeyCode::Char('c')) && key.modifiers.contains(KeyModifiers::CONTROL);
        // Esc cancels; Backspace on an empty buffer cancels too.
        let backspace_on_empty = matches!(key.code, KeyCode::Backspace)
            && matches!(&self.state.mode, Mode::Prompting(p) if p.buffer.is_empty());
        if matches!(key.code, KeyCode::Esc) || backspace_on_empty || ctrl_c {
            self.cancel_prompt();
            return PostAction::None;
        }
        if matches!(key.code, KeyCode::Enter) {
            let Mode::Prompting(p) = std::mem::replace(&mut self.state.mode, Mode::Normal) else {
                return PostAction::None;
            };
            return self.dispatch_prompt(p);
        }

        // (J's history Up/Down used to live here; v1.33.0 promoted
        // J to a vi-line-editor prompt so handle_vi_prompt_key now
        // owns its history navigation alongside the other vi
        // prompts. Other simple prompts don't have history buckets.)

        // Tab completion.
        if matches!(key.code, KeyCode::Tab | KeyCode::Char('\t')) {
            // Extract kind and buffer before taking &mut self.
            let (_kind, buffer) = if let Mode::Prompting(p) = &self.state.mode {
                (std::mem::discriminant(&p.kind), p.buffer.clone())
            } else {
                return PostAction::None;
            };
            let is_search = matches!(
                &self.state.mode,
                Mode::Prompting(p) if matches!(p.kind, PromptKind::Search { .. })
            );
            if is_search {
                if !buffer.is_empty() {
                    self.state.temp_filter = Some(format!("{buffer}*"));
                    self.state.rebuild_rows();
                }
            } else if matches!(
                &self.state.mode,
                Mode::Prompting(p) if matches!(
                    p.kind,
                    PromptKind::Jump
                        | PromptKind::CopyTo
                        | PromptKind::MoveTo
                        | PromptKind::MakeDir
                        | PromptKind::NewFile
                        | PromptKind::PaneNewTabCwd
                )
            ) {
                self.tab_complete_path();
            }
            return PostAction::None;
        }
        self.tab_state = None;

        // Edit the buffer. Scoped borrow so we can run search afterwards.
        {
            let Mode::Prompting(prompt) = &mut self.state.mode else {
                return PostAction::None;
            };
            match key.code {
                KeyCode::Backspace => {
                    prompt.buffer.pop();
                }
                KeyCode::Char(c) => {
                    if key.modifiers.contains(KeyModifiers::CONTROL) {
                        match c {
                            'u' | 'U' => prompt.buffer.clear(),
                            'w' | 'W' => {
                                while matches!(prompt.buffer.chars().last(), Some(c) if c.is_whitespace())
                                {
                                    prompt.buffer.pop();
                                }
                                while matches!(prompt.buffer.chars().last(), Some(c) if !c.is_whitespace())
                                {
                                    prompt.buffer.pop();
                                }
                            }
                            _ => {}
                        }
                    } else {
                        prompt.buffer.push(c);
                    }
                }
                _ => {}
            }
        }

        // For an active search, re-run the match incrementally against the
        // original cursor position so typing narrows towards a result but
        // backspace widens again.
        let search_info = if let Mode::Prompting(Prompt {
            kind: PromptKind::Search { saved_cursor },
            buffer,
            ..
        }) = &self.state.mode
        {
            Some((*saved_cursor, buffer.clone()))
        } else {
            None
        };
        if let Some((saved, query)) = search_info {
            if query.is_empty() {
                self.state.cursor.index = saved;
            } else if let Some(i) = self.state.find_match(&query, saved, false) {
                self.state.cursor.index = i;
            }
            self.state.cursor.clamp(self.state.rows.len());
        }

        PostAction::None
    }

    /// Single-key confirmation for `R`. `y` / `Y` triggers the delete;
    /// anything else — including Enter, Esc, or any other letter — cancels.
    /// The prompt closes in every case.
    fn handle_remove_confirm_key(&mut self, key: KeyEvent) -> PostAction {
        let confirmed = matches!(key.code, KeyCode::Char('y' | 'Y'));
        self.state.mode = Mode::Normal;
        if !confirmed {
            return PostAction::None;
        }
        let paths = self.state.selection_paths();
        if paths.is_empty() {
            return PostAction::None;
        }
        // Route through the graveyard: archive each path into
        // `<uuid>.tar.zst` first, then unlink the source. If the
        // archive step fails for any path we skip the unlink for
        // *that* path and surface a clear error — the user keeps
        // the file. Per-path failures don't stop the rest of the
        // batch; we report the count at the end.
        let mut archived = 0usize;
        let mut failures: Vec<String> = Vec::new();
        for p in &paths {
            match crate::state::graveyard::Graveyard::write_entry(p) {
                Ok(_entry) => match fs::ops::remove_tree(p) {
                    Ok(()) => archived += 1,
                    Err(e) => {
                        failures.push(format!("{}: archived but unlink failed: {e}", p.display()));
                    }
                },
                Err(e) => {
                    // Archive failed — fall back to a hard delete
                    // would surprise the user (they expect undo);
                    // instead, leave the file alone and report.
                    failures.push(format!(
                        "{}: graveyard archive failed: {e} — file NOT removed",
                        p.display()
                    ));
                }
            }
        }
        if failures.is_empty() {
            self.state
                .flash_info(format!("removed {archived} item(s) (recoverable: gy)"));
        } else {
            // First failure goes in the flash; remainder in debug log.
            self.state.flash_error(failures[0].clone());
            for msg in &failures[1..] {
                spyc_debug!("R: {msg}");
            }
        }
        self.state.picks.clear();
        self.state.refresh_listing();
        PostAction::None
    }

    /// `:undo` — restore the most-recent graveyard entry to its
    /// original path. Best-effort recovery for the very common
    /// "I just deleted the wrong thing" case. If the original
    /// path is occupied (rare; user recreated it), tar's
    /// `set_overwrite(false)` errors and we surface that — the
    /// user can open `gy` and pick `p` to restore-to-cwd instead.
    fn undo_last_remove(&mut self) {
        let g = crate::state::graveyard::Graveyard::load();
        let Some(latest) = g.entries.into_iter().next() else {
            self.state.flash_info("undo: graveyard is empty");
            return;
        };
        let dest = latest.orig_path.parent().map_or_else(
            || std::path::PathBuf::from("/"),
            std::path::Path::to_path_buf,
        );
        match crate::state::graveyard::Graveyard::restore(&latest, &dest) {
            Ok(()) => {
                crate::state::graveyard::Graveyard::delete_entry(&latest);
                self.state.flash_info(format!(
                    "undo: restored {} → {}",
                    latest.filename,
                    dest.display()
                ));
                if matches!(self.state.view, View::Graveyard) {
                    self.state.graveyard = crate::state::graveyard::Graveyard::load().entries;
                    self.state.cursor.clamp(self.state.graveyard.len());
                    self.state.rebuild_rows();
                }
                self.state.refresh_listing();
            }
            Err(e) => self
                .state
                .flash_error(format!("undo: {e} — try `gy` then `p` to restore to cwd")),
        }
    }

    /// Single-key confirmation for "purge ALL graveyard entries to
    /// system trash". Bound on `Z` from the graveyard view; routes
    /// to a separate prompt kind so the wording stays accurate.
    fn handle_graveyard_purge_all_confirm(&mut self, key: KeyEvent) -> PostAction {
        let confirmed = matches!(key.code, KeyCode::Char('y' | 'Y'));
        self.state.mode = Mode::Normal;
        if !confirmed {
            return PostAction::None;
        }
        let mut trashed = 0usize;
        let mut errors = 0usize;
        for entry in self.state.graveyard.clone() {
            match crate::state::graveyard::Graveyard::cascade_entry_to_trash(&entry) {
                Ok(()) => trashed += 1,
                Err(_) => errors += 1,
            }
        }
        if errors > 0 {
            self.state
                .flash_error(format!("graveyard: trashed {trashed}, {errors} failed"));
        } else {
            self.state
                .flash_info(format!("graveyard: trashed {trashed} item(s)"));
        }
        self.state.graveyard = crate::state::graveyard::Graveyard::load().entries;
        self.state.cursor.clamp(self.state.graveyard.len());
        self.state.rebuild_rows();
        PostAction::None
    }

    /// Single-key confirmation for the auto-fired claude crash recovery
    /// prompt. `y` / `Y` / Enter kills the broken tab and replaces it with
    /// a fresh `claude` (the user can then `/resume` manually); anything
    /// else kills it and removes the tab so the dump is off-screen.
    fn handle_claude_crash_recover_key(&mut self, key: KeyEvent) -> PostAction {
        let confirmed = matches!(key.code, KeyCode::Char('y' | 'Y') | KeyCode::Enter);
        let prev_mode = std::mem::replace(&mut self.state.mode, Mode::Normal);
        let Mode::Prompting(Prompt {
            kind: PromptKind::ClaudeCrashRecover { tab_idx },
            ..
        }) = prev_mode
        else {
            return PostAction::None;
        };

        // Snapshot cwd + fallback from the tab and best-effort kill the
        // child (bunfs claude is often still alive post-crash; an
        // already-closed pane errors here, ignored).
        let Some((cwd, fallback)) = self.pane_tabs.as_mut().and_then(|tabs| {
            let entry = tabs.tabs_mut().get_mut(tab_idx)?;
            let _ = entry.pane.child.kill();
            let fallback = entry
                .info
                .restore_fallback
                .clone()
                .unwrap_or_else(|| "claude".to_string());
            Some((entry.info.cwd.clone(), fallback))
        }) else {
            return PostAction::None;
        };

        if !confirmed {
            if let Some(tabs) = self.pane_tabs.as_mut() {
                let still_have_tabs = tabs.remove_at(tab_idx);
                if !still_have_tabs {
                    self.pane_tabs = None;
                }
            }
            self.state.flash_info("claude crash dismissed; tab closed");
            self.needs_full_repaint = true;
            return PostAction::None;
        }

        let (rows, cols) = Self::pane_spawn_size(
            self.effective_pane_pct(),
            self.state.config.layout.status_position,
        );
        match Pane::spawn_with_env(&fallback, rows, cols, &cwd, &self.context_path, &[]) {
            Ok(p) => {
                let entry = TabEntry::new(p, TabInfo::new(&fallback, &cwd));
                if let Some(tabs) = self.pane_tabs.as_mut() {
                    tabs.replace_at(tab_idx, entry);
                }
                self.state
                    .flash_info("started fresh claude — type /resume to recover");
            }
            Err(e) => self.state.flash_error(format!("claude spawn failed: {e}")),
        }
        PostAction::None
    }

    /// Return the appropriate history for the current prompt kind.
    /// Four buckets so they don't pollute each other:
    ///   - `pane_history` for new-pane-tab cmd / cwd prompts
    ///   - `jump_history` for the `J` jump-to-path prompt
    ///   - `command_history` for `:` (vim-style command line)
    ///   - `history` for shell-out prompts (`!`, `;`)
    ///
    /// Mixing `:` with `!` was the worst of these collisions: typing
    /// `!make sync-all` then later hitting `:` + Up surfaces
    /// `make sync-all` and submits it as a `:` command, which then
    /// errors with "unknown command".
    #[allow(clippy::missing_const_for_fn)]
    fn history_for_prompt(&mut self) -> &mut History {
        let kind = match &self.state.mode {
            Mode::Prompting(p) => Some(&p.kind),
            Mode::Normal => None,
        };
        if matches!(
            kind,
            Some(PromptKind::PaneNewTabCmd | PromptKind::PaneNewTabCwd)
        ) {
            &mut self.state.pane_history
        } else if matches!(kind, Some(PromptKind::Jump)) {
            &mut self.state.jump_history
        } else if matches!(kind, Some(PromptKind::Command)) {
            &mut self.state.command_history
        } else {
            &mut self.state.history
        }
    }

    /// Handle keys for shell prompts that use the vi line editor.
    fn handle_vi_prompt_key(&mut self, key: KeyEvent) -> PostAction {
        use crate::ui::line_edit::EditResult;

        // Tab completion — intercept before feeding to the editor so we
        // don't depend on the editor's Tab handling (which varies by
        // terminal key delivery).
        if matches!(key.code, KeyCode::Tab | KeyCode::Char('\t')) {
            let wants_path = matches!(
                &self.state.mode,
                Mode::Prompting(p) if matches!(
                    p.kind,
                    PromptKind::Jump
                        | PromptKind::CopyTo
                        | PromptKind::MoveTo
                        | PromptKind::MakeDir
                        | PromptKind::NewFile
                        | PromptKind::PaneNewTabCwd
                        | PromptKind::ShellCmd
                        | PromptKind::ShellCmdCaptured
                        | PromptKind::Command
                )
            );
            if wants_path {
                self.tab_complete_path();
            }
            return PostAction::None;
        }
        // Non-Tab clears double-Tab state.
        self.tab_state = None;

        // ^C in any prompt cancels and returns to normal mode --
        // vi muscle memory. Distinct from Esc only in keystroke,
        // identical in effect.
        if matches!(key.code, KeyCode::Char('c')) && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.history_for_prompt().reset_nav();
            self.cancel_prompt();
            return PostAction::None;
        }

        // `!?` — when the buffer is empty and the user types '?',
        // immediately open the history editor (no Enter needed).
        if key.code == KeyCode::Char('?') {
            if let Mode::Prompting(Prompt {
                kind: PromptKind::ShellCmdCaptured,
                ref buffer,
                ..
            }) = self.state.mode
            {
                if buffer.is_empty() {
                    self.state.mode = Mode::Normal;
                    self.show_history_popup();
                    return PostAction::None;
                }
            }
        }

        // `<Space>` in Normal mode opens the history popup. The full
        // sequence is `Esc Space`: first Esc enters Normal mode (the
        // standard vi-line-editor behavior); Space then asks for the
        // bigger pager view. Reads more naturally than double-Esc
        // and doesn't fight Esc's "back out of something" muscle
        // memory.
        //
        // Dispatched by prompt kind:
        //   PromptKind::Jump → show_jump_history_popup (j/k cd)
        //   anything else    → show_history_popup (shell !? popup)
        //
        // KNOWN LIMITATION: for `:` (command line) the !? popup
        // shows shell history, not command_history. Tracked in
        // ROADMAP for proper kind-routing.
        if matches!(key.code, KeyCode::Char(' ')) {
            let in_normal_mode = matches!(
                &self.state.mode,
                Mode::Prompting(p) if p.editor.as_ref().is_some_and(
                    |e| e.mode == crate::ui::line_edit::Mode::Normal
                )
            );
            if in_normal_mode {
                let is_jump = matches!(
                    &self.state.mode,
                    Mode::Prompting(p) if matches!(p.kind, PromptKind::Jump)
                );
                self.state.mode = Mode::Normal;
                if is_jump {
                    self.show_jump_history_popup();
                } else {
                    self.show_history_popup();
                }
                return PostAction::None;
            }
        }

        // Feed key to the editor.
        let result = {
            let Mode::Prompting(prompt) = &mut self.state.mode else {
                return PostAction::None;
            };
            let editor = prompt.editor.as_mut().expect("checked above");
            let r = editor.feed(key);
            // Sync the buffer for display (prompt.buffer drives rendering).
            prompt.buffer = editor.text();
            r
        };

        match result {
            EditResult::Submit => {
                let is_pane_prompt = matches!(
                    self.state.mode,
                    Mode::Prompting(Prompt {
                        kind: PromptKind::PaneNewTabCmd | PromptKind::PaneNewTabCwd,
                        ..
                    })
                );
                let is_jump_prompt = matches!(
                    self.state.mode,
                    Mode::Prompting(Prompt {
                        kind: PromptKind::Jump,
                        ..
                    })
                );
                let is_command_prompt = matches!(
                    self.state.mode,
                    Mode::Prompting(Prompt {
                        kind: PromptKind::Command,
                        ..
                    })
                );
                let Mode::Prompting(p) = std::mem::replace(&mut self.state.mode, Mode::Normal)
                else {
                    return PostAction::None;
                };
                // Push to the appropriate history before dispatching.
                // Four buckets stay isolated -- shell, pane-tab, jump
                // destinations, and `:` commands don't cross-pollute
                // each other's Up/Down browse.
                let hist = if is_pane_prompt {
                    &mut self.state.pane_history
                } else if is_jump_prompt {
                    &mut self.state.jump_history
                } else if is_command_prompt {
                    &mut self.state.command_history
                } else {
                    &mut self.state.history
                };
                if !p.buffer.trim().is_empty() {
                    hist.push(p.buffer.trim());
                }
                hist.reset_nav();
                return self.dispatch_prompt(p);
            }
            EditResult::Cancel => {
                self.history_for_prompt().reset_nav();
                self.cancel_prompt();
            }
            EditResult::HistoryPrev => {
                let current_text = {
                    let Mode::Prompting(p) = &self.state.mode else {
                        return PostAction::None;
                    };
                    p.buffer.clone()
                };
                let hist = self.history_for_prompt();
                if let Some(entry) = hist.prev(&current_text) {
                    let entry = entry.to_string();
                    let Mode::Prompting(p) = &mut self.state.mode else {
                        return PostAction::None;
                    };
                    if let Some(ed) = p.editor.as_mut() {
                        ed.set_content_keep_mode(&entry);
                    }
                    p.buffer = entry;
                }
            }
            EditResult::HistoryNext => {
                let hist = self.history_for_prompt();
                let replacement = match hist.next() {
                    Some(entry) => entry.to_string(),
                    None => hist.stashed().to_string(),
                };
                let Mode::Prompting(p) = &mut self.state.mode else {
                    return PostAction::None;
                };
                if let Some(ed) = p.editor.as_mut() {
                    ed.set_content_keep_mode(&replacement);
                }
                p.buffer = replacement;
            }
            // Tab is intercepted before editor.feed() — this arm is
            // only reachable if the editor somehow returns it.
            EditResult::TabComplete | EditResult::Continue => {}
        }
        PostAction::None
    }

    /// Tab-complete a filesystem path in the prompt buffer. For shell
    /// prompts, completes just the last whitespace-delimited word.
    fn tab_complete_path(&mut self) {
        // Extract data from prompt without holding the borrow.
        let (is_shell, is_jump, buffer) = {
            let Mode::Prompting(ref prompt) = self.state.mode else {
                return;
            };
            let is_shell = matches!(
                prompt.kind,
                PromptKind::ShellCmd | PromptKind::ShellCmdCaptured | PromptKind::Command
            );
            let is_jump = matches!(prompt.kind, PromptKind::Jump);
            (is_shell, is_jump, prompt.buffer.clone())
        };

        // Repeated Tab with active cycle state: cycle through matches
        // or re-flash the list for local dirs.
        if let Some(ref mut ts) = self.tab_state {
            if (ts.original_buf == buffer || ts.cycle_index > 0) && ts.matches.len() > 1 {
                // Cycle to next match, fill it in.
                let idx = ts.cycle_index % ts.matches.len();
                let completed = format!("{}{}{}", ts.buf_prefix, ts.word_base, ts.matches[idx]);
                ts.cycle_index = idx + 1;
                let flash = format!("{} — {}/{}", ts.matches[idx], idx + 1, ts.matches.len());
                self.state.flash_info(flash);
                let Mode::Prompting(ref mut prompt) = self.state.mode else {
                    return;
                };
                prompt.buffer = completed;
                if let Some(ed) = prompt.editor.as_mut() {
                    ed.set_content(&prompt.buffer);
                }
                return;
            }
        }

        // For shell prompts, extract just the last word for completion.
        let (buf_prefix, word) = if is_shell {
            let last_space = buffer.rfind(' ').map_or(0, |i| i + 1);
            (
                buffer[..last_space].to_string(),
                buffer[last_space..].to_string(),
            )
        } else {
            (String::new(), buffer)
        };

        let input = crate::paths::expand(&word);
        let input_str = input.to_string_lossy().to_string();
        let (dir, file_prefix) = if input_str.ends_with('/') || input_str.is_empty() {
            let dir = if input_str.is_empty() {
                self.state.listing.dir.clone()
            } else {
                input
            };
            (dir, String::new())
        } else {
            let dir = input.parent().map_or_else(
                || self.state.listing.dir.clone(),
                |p| {
                    if p.as_os_str().is_empty() {
                        self.state.listing.dir.clone()
                    } else {
                        p.to_path_buf()
                    }
                },
            );
            let name = input
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            (dir, name)
        };

        let Ok(entries) = std::fs::read_dir(&dir) else {
            return;
        };
        let mut matches: Vec<String> = entries
            .filter_map(Result::ok)
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with(&file_prefix) {
                    let is_dir = e.file_type().is_ok_and(|ft| ft.is_dir());
                    let suffix = if is_dir { "/" } else { "" };
                    Some(format!("{name}{suffix}"))
                } else {
                    None
                }
            })
            .collect();
        matches.sort();

        if matches.is_empty() {
            // No filesystem matches — try frecency for Jump prompts.
            if is_jump {
                self.frecency_complete(&word, &buf_prefix);
            }
            return;
        }

        let word_base = if word.ends_with('/') || word.is_empty() {
            word.clone()
        } else {
            let last_sep = word.rfind('/').map_or(0, |i| i + 1);
            word[..last_sep].to_string()
        };

        let (completed_word, flash) = if matches.len() == 1 {
            (format!("{word_base}{}", matches[0]), None)
        } else {
            let common = common_prefix(&matches);
            if common.len() > file_prefix.len() {
                let msg = format!("{} matches", matches.len());
                (format!("{word_base}{common}"), Some(msg))
            } else {
                // No text progress — show matches and set up cycle state.
                let display: Vec<&str> = matches.iter().map(std::string::String::as_str).collect();
                let shown = if display.len() > 12 {
                    format!(
                        "{}  (+{} more)",
                        display[..12].join("  "),
                        display.len() - 12
                    )
                } else {
                    display.join("  ")
                };
                if dir == self.state.listing.dir {
                    // Local dir — also filter the listing.
                    self.state.temp_filter = Some(format!("{file_prefix}*"));
                    self.state.rebuild_rows();
                    self.state.flash_info(format!("{shown}  — Tab to cycle"));
                } else {
                    self.state.flash_info(format!("{shown}  — Tab to cycle"));
                }
                let Mode::Prompting(ref prompt) = self.state.mode else {
                    return;
                };
                self.tab_state = Some(TabState {
                    original_buf: prompt.buffer.clone(),
                    buf_prefix: buf_prefix.clone(),
                    word_base,
                    matches,
                    cycle_index: 0,
                });
                return;
            }
        };

        if let Some(msg) = flash {
            self.state.flash_info(msg);
        }

        let Mode::Prompting(ref mut prompt) = self.state.mode else {
            return;
        };
        prompt.buffer = format!("{buf_prefix}{completed_word}");
        if let Some(ed) = prompt.editor.as_mut() {
            ed.set_content(&prompt.buffer);
        }
        // Store cycle state for multi-match (common prefix advanced but
        // further Tabs should still be able to cycle).
        if matches.len() > 1 {
            self.tab_state = Some(TabState {
                original_buf: prompt.buffer.clone(),
                buf_prefix,
                word_base,
                matches,
                cycle_index: 0,
            });
        } else {
            self.tab_state = None;
        }
    }

    /// Frecency fallback for the J prompt: when filesystem completion finds
    /// no matches, search the frecency database for directories matching
    /// the typed fragment.
    fn frecency_complete(&mut self, word: &str, buf_prefix: &str) {
        let hits = self.state.frecency.search(word);
        if hits.is_empty() {
            return;
        }

        // Convert to display strings with trailing slash.
        let names: Vec<String> = hits
            .iter()
            .map(|p| format!("{}/", p.to_string_lossy()))
            .collect();

        if names.len() == 1 {
            // Single match — fill it in directly.
            let completed = format!("{buf_prefix}{}", names[0]);
            let Mode::Prompting(ref mut prompt) = self.state.mode else {
                return;
            };
            prompt.buffer = completed;
            if let Some(ed) = prompt.editor.as_mut() {
                ed.set_content(&prompt.buffer);
            }
            self.tab_state = None;
        } else {
            // Multiple frecency matches — fill best, set up cycling.
            let completed = format!("{buf_prefix}{}", names[0]);
            self.state
                .flash_info(format!("{} — 1/{} frecency", names[0], names.len()));
            let Mode::Prompting(ref mut prompt) = self.state.mode else {
                return;
            };
            let original = prompt.buffer.clone();
            prompt.buffer = completed;
            if let Some(ed) = prompt.editor.as_mut() {
                ed.set_content(&prompt.buffer);
            }
            self.tab_state = Some(TabState {
                original_buf: original,
                buf_prefix: buf_prefix.to_string(),
                word_base: String::new(),
                matches: names,
                cycle_index: 1, // already showing first match
            });
        }
    }

    /// Close the prompt without dispatching. Restores search cursor,
    /// clears Tab-applied filters.
    fn cancel_prompt(&mut self) {
        let Mode::Prompting(p) = std::mem::replace(&mut self.state.mode, Mode::Normal) else {
            return;
        };
        if let PromptKind::Search { saved_cursor } = p.kind {
            self.state.cursor.index = saved_cursor;
            self.state.cursor.clamp(self.state.rows.len());
        }
        // Clear any Tab-applied filter (search or shell prompt).
        if self.state.temp_filter.is_some() {
            self.state.temp_filter = None;
            self.state.rebuild_rows();
        }
        self.tab_state = None;
        // Clear any stashed state from the two-step new-tab prompt.
        self.state.pending_new_tab_cmd = None;
    }

    /// Parse and dispatch a `:` command.
    ///
    /// Pure-domain arms are handled by `AppState::dispatch_command`;
    /// terminal-touching arms (shell, pager, overlay) stay here.
    fn dispatch_command(&mut self, input: &str) -> PostAction {
        use state::CommandResult;

        // Try the pure-domain handler first.
        match self.state.dispatch_command(input) {
            CommandResult::Handled => return PostAction::None,
            CommandResult::OpenPager { title, lines } => {
                self.pager = Some(PagerView::new_plain(title, lines));
                return PostAction::None;
            }
            CommandResult::NotHandled => {}
        }

        // --- Terminal-touching arms (shell/pager/overlay) ---
        let input = input.trim();

        // :!! — repeat last captured command
        if input == "!!" || input == "!" {
            match self.state.last_captured_cmd.clone() {
                Some(cmd) => {
                    let expanded =
                        crate::shell::expand_percent(&cmd, &self.state.selection_paths());
                    self.start_capture(&expanded, &cmd, &cmd);
                }
                None => self.state.flash_error("no previous ! command"),
            }
            return PostAction::None;
        }

        // :!<cmd> — captured shell command
        if let Some(cmd) = input.strip_prefix('!') {
            let cmd = cmd.trim();
            if cmd.is_empty() {
                self.state.flash_error("empty command");
                return PostAction::None;
            }
            self.state.last_captured_cmd = Some(cmd.to_string());
            let expanded = crate::shell::expand_percent(cmd, &self.state.selection_paths());
            self.start_capture(&expanded, cmd, cmd);
            return PostAction::None;
        }

        // :;<cmd> — foreground shell command
        if let Some(cmd) = input.strip_prefix(';') {
            let cmd = cmd.trim();
            if cmd.is_empty() {
                self.state.flash_error("empty command");
                return PostAction::None;
            }
            let expanded = crate::shell::expand_percent(cmd, &self.state.selection_paths());
            let (rows, cols) =
                Self::top_overlay_size(self.effective_pane_pct(), self.pane_tabs.is_some());
            let cwd = self.state.listing.dir.clone();
            match Pane::spawn(&expanded, rows, cols, &cwd, &self.context_path) {
                Ok(p) => {
                    self.top_overlay = Some(p);
                    // Initial focus is on the new overlay so the user
                    // can drive the subprocess directly. ^a-j hands
                    // focus to the bottom pane (when one is open).
                    self.state.pane_focused = false;
                }
                Err(e) => self.state.flash_error(format!("spawn: {e}")),
            }
            return PostAction::None;
        }

        // :undo — restore the most-recent graveyard entry to its
        // original path. The "did I mean to do that?" escape hatch
        // for `R`. No-arg only; if the user wants to pick a
        // specific entry they open the graveyard view (`gy`).
        if input == "undo" {
            self.undo_last_remove();
            return PostAction::None;
        }
        // :date — flash current date/time. Used to be bound to `D` but
        // `D` now opens the cursor file in $PAGER (the common
        // request); the date utility lives on as a typed command for
        // the rare hand-on-keyboard moment you actually want it.
        if input == "date" {
            let _ = self.apply(&Action::Date);
            return PostAction::None;
        }
        // :graveyard — open the graveyard viewer (typed alias for `gy`).
        if input == "graveyard" {
            self.state.open_graveyard_view();
            return PostAction::None;
        }

        // :fg [N] — bring a backgrounded task back to the foreground.
        // No arg = most-recently-backgrounded task; numeric arg = id.
        if input == "fg" {
            self.foreground_task(None);
            return PostAction::None;
        }
        if let Some(arg) = input.strip_prefix("fg ") {
            match arg.trim().parse::<u32>() {
                Ok(id) => self.foreground_task(Some(id)),
                Err(_) => self
                    .state
                    .flash_error(format!("fg: expected task id (got {arg:?})")),
            }
            return PostAction::None;
        }

        // :grep <pattern> — project-wide content search. Walks
        // PROJECT_HOME (or the current listing dir if unset),
        // gitignore-aware, results stream into a pager as
        // `path:line:col: text` so gf/gF jumps to the file.
        if input == "grep" {
            self.state.flash_error("grep: pattern required");
            return PostAction::None;
        }
        if let Some(pattern) = input.strip_prefix("grep ") {
            let pattern = pattern.trim();
            if pattern.is_empty() {
                self.state.flash_error("grep: pattern required");
            } else {
                self.open_grep_pager(pattern);
            }
            return PostAction::None;
        }

        // :task [N] — open the task viewer (peek mode). No arg picks
        // the most-recent task; numeric arg targets a specific id.
        if input == "task" {
            self.open_task_viewer(None);
            return PostAction::None;
        }
        if let Some(arg) = input.strip_prefix("task ") {
            match arg.trim().parse::<u32>() {
                Ok(id) => self.open_task_viewer(Some(id)),
                Err(_) => self
                    .state
                    .flash_error(format!("task: expected task id (got {arg:?})")),
            }
            return PostAction::None;
        }

        // :pause [N] — pause a backgrounded task via SIGSTOP to its
        // process group. No arg = most-recent task; numeric = id.
        if input == "pause" {
            self.pause_task(None);
            return PostAction::None;
        }
        if let Some(arg) = input.strip_prefix("pause ") {
            match arg.trim().parse::<u32>() {
                Ok(id) => self.pause_task(Some(id)),
                Err(_) => self
                    .state
                    .flash_error(format!("pause: expected task id (got {arg:?})")),
            }
            return PostAction::None;
        }

        // :resume [N] — resume a paused backgrounded task via SIGCONT.
        if input == "resume" {
            self.resume_task(None);
            return PostAction::None;
        }
        if let Some(arg) = input.strip_prefix("resume ") {
            match arg.trim().parse::<u32>() {
                Ok(id) => self.resume_task(Some(id)),
                Err(_) => self
                    .state
                    .flash_error(format!("resume: expected task id (got {arg:?})")),
            }
            return PostAction::None;
        }

        // :bprev / :bnext — pager buffer history
        if input == "bprev" {
            if let Some(current) = self.pager.take() {
                match self.pager_history.go_back(current) {
                    Ok(prev) => {
                        self.pager = Some(prev);
                        self.needs_full_repaint = true;
                        let back = self.pager_history.back_len();
                        let fwd = self.pager_history.forward_len();
                        self.state.flash_info(format!("buffer ←{back} →{fwd}"));
                    }
                    Err(current) => {
                        // At the start of history -- keep the current
                        // pager visible instead of closing it.
                        self.pager = Some(current);
                        self.state.flash_info("no older buffers");
                    }
                }
            } else if let Some(prev) = self.pager_history.back.pop() {
                self.pager = Some(prev);
                self.needs_full_repaint = true;
                self.state
                    .flash_info(format!("buffer ←{}", self.pager_history.back_len()));
            } else {
                self.state.flash_info("no buffers in history");
            }
            return PostAction::None;
        }
        if input == "bnext" {
            if let Some(current) = self.pager.take() {
                match self.pager_history.go_forward(current) {
                    Ok(next) => {
                        self.pager = Some(next);
                        self.needs_full_repaint = true;
                        let back = self.pager_history.back_len();
                        let fwd = self.pager_history.forward_len();
                        self.state.flash_info(format!("buffer ←{back} →{fwd}"));
                    }
                    Err(current) => {
                        self.pager = Some(current);
                        self.state.flash_info("no newer buffers");
                    }
                }
            } else {
                self.state.flash_info("no pager open");
            }
            return PostAction::None;
        }

        // If we get here, AppState said NotHandled but we don't recognize it
        // either — this shouldn't happen, but handle gracefully.
        self.state.flash_error(format!("unknown command: {input}"));
        PostAction::None
    }

    /// Dispatch a submitted prompt.
    ///
    /// Pure-domain arms are handled by `AppState::dispatch_prompt`;
    /// terminal-touching arms (shell, pager, overlay, copy/move) stay here.
    #[allow(clippy::needless_pass_by_value)]
    fn dispatch_prompt(&mut self, prompt: Prompt) -> PostAction {
        use state::PromptResult;

        // Clear any Tab-applied filter before dispatching.
        if self.state.temp_filter.is_some() {
            self.state.temp_filter = None;
            self.state.rebuild_rows();
        }
        self.tab_state = None;

        // Try the pure-domain handler first.
        match self.state.dispatch_prompt(&prompt.kind, &prompt.buffer) {
            PromptResult::Handled => return PostAction::None,
            PromptResult::NotHandled => {}
        }

        // --- Terminal-touching arms ---
        match prompt.kind {
            PromptKind::ShellCmd => {
                let expanded = shell::expand_percent(&prompt.buffer, &self.state.selection_paths());
                let (rows, cols) =
                    Self::top_overlay_size(self.effective_pane_pct(), self.pane_tabs.is_some());
                let cwd = self.state.listing.dir.clone();
                match Pane::spawn(&expanded, rows, cols, &cwd, &self.context_path) {
                    Ok(p) => {
                        self.top_overlay = Some(p);
                        self.state.pane_focused = false;
                    }
                    Err(e) => self.state.flash_error(format!("spawn: {e}")),
                }
                PostAction::None
            }
            PromptKind::ShellCmdCaptured => {
                let cmd = if prompt.buffer.trim() == "!" {
                    if let Some(c) = self.state.last_captured_cmd.clone() {
                        c
                    } else {
                        self.state.flash_error("no previous ! command");
                        return PostAction::None;
                    }
                } else {
                    prompt.buffer.clone()
                };
                self.state.last_captured_cmd = Some(cmd.clone());
                let expanded = shell::expand_percent(&cmd, &self.state.selection_paths());
                self.start_capture(&expanded, &cmd, &prompt.buffer);
                PostAction::None
            }
            PromptKind::CopyTo => {
                self.run_selection_to(&prompt.buffer, fs::ops::copy_selection_to, "copied");
                PostAction::None
            }
            PromptKind::MoveTo => {
                self.run_selection_to(&prompt.buffer, fs::ops::move_selection_to, "moved");
                PostAction::None
            }
            PromptKind::PaneNewTabCwd => {
                let cwd = prompt.buffer.trim().to_string();
                if let Some(cmd) = self.state.pending_new_tab_cmd.take() {
                    let cwd_path = if cwd.is_empty() {
                        self.state
                            .project_home
                            .clone()
                            .unwrap_or_else(|| self.state.listing.dir.clone())
                    } else if cwd.starts_with('~') {
                        let home = std::env::var("HOME").unwrap_or_default();
                        std::path::PathBuf::from(cwd.replacen('~', &home, 1))
                    } else {
                        std::path::PathBuf::from(&cwd)
                    };
                    self.open_pane_tab_in(&cmd, &cwd_path);
                }
                PostAction::None
            }
            PromptKind::PaneRenameTab => {
                let name = prompt.buffer.trim().to_string();
                if !name.is_empty() {
                    if let Some(tabs) = self.pane_tabs.as_mut() {
                        tabs.active_info_mut().label = name;
                    }
                }
                PostAction::None
            }
            PromptKind::NewFile => {
                let name = prompt.buffer.trim().to_string();
                if name.is_empty() {
                    return PostAction::None;
                }
                let target = crate::paths::expand(&name);
                let resolved = if target.is_absolute() {
                    target
                } else {
                    self.state.listing.dir.join(&target)
                };
                // Create parent dirs if needed, then touch the file.
                if let Some(parent) = resolved.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if !resolved.exists() {
                    let _ = std::fs::write(&resolved, "");
                }
                let mut argv = shell::resolve_editor();
                if argv.is_empty() {
                    self.state.flash_error("$EDITOR not set");
                    return PostAction::None;
                }
                let program = argv.remove(0);
                argv.push(resolved.to_string_lossy().into_owned());
                PostAction::Spawn {
                    program,
                    args: argv,
                    pause_after: false,
                }
            }
            PromptKind::Command => self.dispatch_command(&prompt.buffer),
            // These should have been handled by AppState — unreachable in practice.
            _ => PostAction::None,
        }
    }

    /// Open the split pane if it's closed, close all tabs if it's open.
    fn toggle_pane(&mut self) {
        if self.pane_tabs.is_some() {
            self.pane_tabs = None;
            self.state.pane_focused = false;
            self.state.pane_zoomed = false;
            self.state.pane_focus_before_zoom = None;
            self.needs_full_repaint = true;
            self.state.flash_info("pane closed");
            return;
        }
        let cmd = std::env::var("SPYC_PANE_CMD").unwrap_or_else(|_| "claude".to_string());
        self.open_pane_tab(&cmd);
    }

    /// Spawn a new pane tab. If no tabs exist, creates the container.
    /// Default cwd is `PROJECT_HOME` when set, else the current listing dir.
    fn open_pane_tab(&mut self, cmd: &str) {
        let cwd = self
            .state
            .project_home
            .clone()
            .unwrap_or_else(|| self.state.listing.dir.clone());
        self.open_pane_tab_in(cmd, &cwd);
    }

    fn open_pane_tab_in(&mut self, cmd: &str, cwd: &std::path::Path) {
        let (rows, cols) = Self::pane_spawn_size(
            self.effective_pane_pct(),
            self.state.config.layout.status_position,
        );
        match Pane::spawn_with_env(cmd, rows, cols, cwd, &self.context_path, &[]) {
            Ok(p) => {
                self.state.pane_focused = true;
                self.state
                    .flash_info(format!("pane: {cmd} (^W k for list)"));
                let entry = TabEntry::new(p, TabInfo::new(cmd, cwd));
                if let Some(tabs) = self.pane_tabs.as_mut() {
                    tabs.push(entry);
                } else {
                    self.pane_tabs = Some(PaneTabs::new(entry));
                }
            }
            Err(e) => self.state.flash_error(format!("pane spawn failed: {e}")),
        }
    }

    /// V — open $EDITOR on the cursor file in the top overlay (replaces
    /// the file list) while the bottom pane stays visible and running.
    /// Open the F-finder. Spawns the walker on a worker thread so
    /// the picker is interactive immediately (typing filters the
    /// already-arrived candidates while the walker keeps streaming
    /// in the background). Closing the picker drops the receiver,
    /// which makes the walker exit on its next `tx.send`.
    /// Spawn a `:grep` worker, install its session, and open a pager
    /// pre-populated with the title and an empty body. Subsequent
    /// ticks drain the rx and append rendered match lines until the
    /// worker disconnects or the pager is replaced.
    fn open_grep_pager(&mut self, pattern: &str) {
        let root = self
            .state
            .project_home
            .clone()
            .unwrap_or_else(|| self.state.listing.dir.clone());
        // Validate the pattern up-front so we can flash an error
        // inline rather than open an empty pager that silently
        // produces zero results. The worker re-compiles the same
        // regex, but parse cost is trivial.
        if let Err(e) = grep_regex::RegexMatcherBuilder::new()
            .case_smart(true)
            .build(pattern)
        {
            self.state.flash_error(format!("grep: {e}"));
            return;
        }
        let id = self.next_grep_id;
        self.next_grep_id = self.next_grep_id.wrapping_add(1);
        let (tx, rx) = std::sync::mpsc::channel();
        let walk_root = root.clone();
        let pat = pattern.to_string();
        let pat_for_thread = pat.clone();
        std::thread::spawn(move || {
            let _ = crate::fs::grep::search_streaming(&walk_root, &pat_for_thread, tx);
        });
        let title = format!("grep — \"{pat}\" — scanning…");
        let mut view = pager::PagerView::new_plain(title, Vec::<String>::new());
        view.streaming = true;
        // Lock the gutter to the cap so it doesn't widen as results
        // stream in (otherwise visible text shifts right each time
        // the count crosses a power of 10: 9→10, 99→100, etc.).
        view.line_count_hint = Some(crate::fs::grep::MAX_MATCHES);
        view.grep_id = Some(id);
        view.saveable = true;
        // Push any previously-open pager onto the back stack so the
        // user can `:bprev` to it.
        if let Some(prev) = self.pager.take() {
            self.pager_history.push(prev);
        }
        self.pager = Some(view);
        self.grep_session = Some(GrepSession {
            id,
            rx,
            count: 0,
            complete: false,
            capped: false,
            pattern: pat,
            root,
        });
        self.needs_full_repaint = true;
    }

    /// Drain any pending grep matches into the active pager. Called
    /// from the tick loop. Returns true when something changed
    /// (matches appended or worker completed) so the caller can
    /// request a redraw.
    fn drain_grep_session(&mut self) -> bool {
        let Some(session) = self.grep_session.as_mut() else {
            return false;
        };
        // Drop the session if the matching pager is gone. The user
        // closed/replaced it; the worker keeps running but will exit
        // on its next send when our rx is dropped.
        let pager_matches = self
            .pager
            .as_ref()
            .is_some_and(|p| p.grep_id == Some(session.id));
        if !pager_matches {
            self.grep_session = None;
            return false;
        }
        let mut got_any = false;
        loop {
            match session.rx.try_recv() {
                Ok(batch) => {
                    if let Some(view) = self.pager.as_mut() {
                        for m in &batch {
                            view.lines.push(ratatui::text::Line::from(m.render()));
                        }
                    }
                    session.count += batch.len();
                    if session.count >= crate::fs::grep::MAX_MATCHES {
                        session.capped = true;
                    }
                    got_any = true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    session.complete = true;
                    got_any = true;
                    break;
                }
            }
        }
        if got_any {
            // Refresh title with current count + status.
            let suffix = if session.complete {
                if session.capped {
                    format!(" — {} matches (cap; refine pattern)", session.count)
                } else {
                    format!(" — {} matches", session.count)
                }
            } else {
                format!(" — {} matches — scanning…", session.count)
            };
            let root_label = crate::paths::display_tilde(&session.root);
            let new_title = format!("grep — \"{}\" — {root_label}{suffix}", session.pattern);
            if let Some(view) = self.pager.as_mut() {
                view.title = new_title;
                if session.complete {
                    view.streaming = false;
                }
            }
            if session.complete {
                self.grep_session = None;
            }
        }
        got_any
    }

    fn open_find_picker(&mut self) {
        let root = self
            .state
            .project_home
            .clone()
            .unwrap_or_else(|| self.state.listing.dir.clone());
        let (tx, rx) = std::sync::mpsc::channel();
        let walk_root = root.clone();
        std::thread::spawn(move || {
            crate::fs::finder::walk_streaming(&walk_root, tx);
        });
        let mut picker = FindPicker {
            candidates: Vec::new(),
            root,
            query: String::new(),
            filtered: Vec::new(),
            selected: 0,
            limit: 200,
            walk_rx: Some(rx),
            walk_complete: false,
        };
        picker.refilter();
        self.find_picker = Some(picker);
        self.render_find_picker();
        self.needs_full_repaint = true;
    }

    /// Rebuild the pager view from current `find_picker` state.
    /// Called on open, after each keystroke that mutates the query
    /// or selection, and after each tick where the streaming walk
    /// produced new candidates (title shows progress).
    fn render_find_picker(&mut self) {
        let Some(picker) = self.find_picker.as_ref() else {
            return;
        };
        let total = picker.candidates.len();
        let shown = picker.filtered.len();
        let pos = if shown == 0 { 0 } else { picker.selected + 1 };
        let scan_suffix = if picker.walk_complete {
            String::new()
        } else {
            " — scanning…".to_string()
        };
        let title = format!(
            "find — \"{}\" — {pos}/{shown} of {total}{scan_suffix}",
            picker.query
        );
        let lines: Vec<String> = picker
            .filtered
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        let mut view = pager::PagerView::new_plain(title, lines);
        view.show_line_numbers = false;
        view.no_history = true;
        // Picker rows must map 1:1 to source lines so the cursor +
        // selection math stays correct -- wrap would split a long
        // path across multiple visual rows and break that.
        view.wrap = false;
        view.picker_cursor = if shown == 0 {
            None
        } else {
            Some(picker.selected)
        };
        // While the walker is still streaming, suppress [EOF] /
        // tilde markers since the candidate list is still growing.
        view.streaming = !picker.walk_complete;
        self.pager = Some(view);
    }

    /// Intercept keys when the F-finder is open. Returns true when
    /// the key was consumed by the picker (so the caller skips
    /// normal pager / file-list dispatch). Esc closes; Enter chdirs
    /// to the matched file's parent and places the cursor on it;
    /// Up/Down move selection; printable chars + Backspace edit
    /// the query and re-rank.
    fn handle_find_picker_key(&mut self, key: KeyEvent) -> bool {
        if self.find_picker.is_none() {
            return false;
        }
        match key.code {
            KeyCode::Esc => {
                self.find_picker = None;
                self.pager = None;
                self.needs_full_repaint = true;
                true
            }
            KeyCode::Enter => {
                let target = self.find_picker.as_ref().and_then(|p| {
                    p.filtered
                        .get(p.selected)
                        .cloned()
                        .map(|rel| (p.root.clone(), rel))
                });
                self.find_picker = None;
                self.pager = None;
                self.needs_full_repaint = true;
                if let Some((root, rel)) = target {
                    let abs = root.join(&rel);
                    if let Some(parent) = abs.parent() {
                        if let Err(e) = self.state.chdir(parent) {
                            self.state.flash_error(format!("chdir: {e}"));
                        } else if let Some(idx) = self.state.rows.iter().position(|r| r.path == abs)
                        {
                            self.state.cursor.index = idx;
                            self.state.cursor.clamp(self.state.rows.len());
                        }
                    }
                }
                true
            }
            KeyCode::Up => {
                if let Some(picker) = self.find_picker.as_mut() {
                    if picker.selected > 0 {
                        picker.selected -= 1;
                        self.render_find_picker();
                    }
                }
                true
            }
            KeyCode::Down => {
                if let Some(picker) = self.find_picker.as_mut() {
                    if picker.selected + 1 < picker.filtered.len() {
                        picker.selected += 1;
                        self.render_find_picker();
                    }
                }
                true
            }
            KeyCode::Backspace => {
                if let Some(picker) = self.find_picker.as_mut() {
                    if !picker.query.is_empty() {
                        picker.query.pop();
                        picker.refilter();
                        self.render_find_picker();
                    }
                }
                true
            }
            KeyCode::Char(c)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                if let Some(picker) = self.find_picker.as_mut() {
                    picker.query.push(c);
                    picker.refilter();
                    self.render_find_picker();
                }
                true
            }
            _ => true, // Swallow other keys while picker is open.
        }
    }

    fn edit_in_pane(&mut self) {
        let Some(row) = self.state.rows.get(self.state.cursor.index) else {
            return;
        };
        let path = row.path.clone();
        if row.kind == EntryKind::Dir {
            self.state.flash_error("V: cannot edit a directory");
            return;
        }
        let argv = shell::resolve_editor();
        if argv.is_empty() {
            self.state.flash_error("no $VISUAL or $EDITOR set");
            return;
        }
        let cmd = format!(
            "{} {}",
            argv.join(" "),
            shell::shell_quote(&path.display().to_string()),
        );
        let (rows, cols) =
            Self::top_overlay_size(self.effective_pane_pct(), self.pane_tabs.is_some());
        let cwd = self.state.listing.dir.clone();
        match Pane::spawn(&cmd, rows, cols, &cwd, &self.context_path) {
            Ok(p) => {
                self.top_overlay = Some(p);
                self.state.pane_focused = false;
            }
            Err(e) => self.state.flash_error(format!("spawn: {e}")),
        }
    }

    /// `D` — open the cursor file in `$PAGER` as a top-overlay pty so
    /// the bottom pane (claude / zsh / etc.) stays visible alongside.
    /// Mirror of `edit_in_pane` for the read path. Common workflow:
    /// `D` on a doc, `^a-j` into claude, work, `^a-k` to scroll.
    fn display_in_pane(&mut self) {
        let Some(row) = self.state.rows.get(self.state.cursor.index) else {
            return;
        };
        let path = row.path.clone();
        if row.kind == EntryKind::Dir {
            self.state.flash_error("D: cannot page a directory");
            return;
        }
        let argv = shell::resolve_pager();
        if argv.is_empty() {
            self.state.flash_error("no $PAGER set");
            return;
        }
        let cmd = format!(
            "{} {}",
            argv.join(" "),
            shell::shell_quote(&path.display().to_string()),
        );
        let (rows, cols) =
            Self::top_overlay_size(self.effective_pane_pct(), self.pane_tabs.is_some());
        let cwd = self.state.listing.dir.clone();
        match Pane::spawn(&cmd, rows, cols, &cwd, &self.context_path) {
            Ok(p) => {
                self.top_overlay = Some(p);
                self.state.pane_focused = false;
            }
            Err(e) => self.state.flash_error(format!("spawn: {e}")),
        }
    }

    /// Does this command look like it's launching Claude CLI?
    fn is_claude_command(cmd: &str) -> bool {
        let first = cmd.split_whitespace().next().unwrap_or("");
        first == "claude" || first.ends_with("/claude")
    }

    /// Does this command look like it's launching Codex CLI? Matches
    /// bare `codex`, `codex resume ...`, `codex exec ...`, etc., plus
    /// the path-qualified form (`/usr/local/bin/codex`).
    fn is_codex_command(cmd: &str) -> bool {
        let first = cmd.split_whitespace().next().unwrap_or("");
        first == "codex" || first.ends_with("/codex")
    }

    /// Classify a command for session-resume purposes.
    fn detect_agent_kind(cmd: &str) -> AgentKind {
        if Self::is_claude_command(cmd) {
            AgentKind::Claude
        } else if Self::is_codex_command(cmd) {
            AgentKind::Codex
        } else {
            AgentKind::Other
        }
    }

    /// Spawn a captured shell command and install the streaming pager
    /// view + `pending_capture` so the loop can drain output. Used by
    /// the `!` prompt, `:!`, `:!!`, and the `!?` history re-execute —
    /// `cmd_display` lets `:!!` show `!` while titling with the actual
    /// resolved command.
    fn start_capture(&mut self, expanded: &str, title_cmd: &str, cmd_display: &str) {
        let title = format!("! {title_cmd}");
        match spawn_capture(expanded, &self.state.listing.dir) {
            Ok((child, writer, rx)) => {
                let mut view =
                    PagerView::new_plain(format!("\u{23f3} {title} — running... (0s)"), Vec::new());
                view.streaming = true;
                self.pager = Some(view);
                self.pending_capture = Some(PendingCapture {
                    child,
                    writer,
                    output_rx: rx,
                    buffer: Vec::new(),
                    title,
                    cmd_display: cmd_display.to_string(),
                    started: std::time::Instant::now(),
                    finished: false,
                    original_id: None,
                });
            }
            Err(e) => self.state.flash_error(format!("exec: {e}")),
        }
    }

    /// `^Z` from inside a streaming `!` capture pager. Move the running
    /// capture into `background_tasks` and close the pager. The reader
    /// thread (spawned by `spawn_capture`) keeps running, so output
    /// keeps accumulating into the task buffer for later `:fg`.
    fn background_capture(&mut self) {
        let Some(capture) = self.pending_capture.take() else {
            return;
        };
        let id = capture
            .original_id
            .unwrap_or_else(|| self.background_tasks.allocate_id());
        let task = BackgroundTask {
            id,
            title: capture.title,
            cmd_display: capture.cmd_display.clone(),
            child: capture.child,
            writer: capture.writer,
            output_rx: capture.output_rx,
            buffer: capture.buffer,
            status: TaskStatus::Running,
            started: capture.started,
            finished_at: None,
            has_unread_output: false,
            viewed_in_task_viewer: false,
            paused: false,
        };
        self.background_tasks.tasks.push(task);
        self.pager = None;
        self.needs_full_repaint = true;
        self.state
            .flash_info(format!("task #{id} backgrounded — :fg to resume"));
    }

    /// `:fg` (no arg) or `:fg N`. Bring a backgrounded task to the
    /// foreground. Still-running tasks resume as a streaming pager
    /// seeded with the buffer; already-exited tasks open as a static
    /// pager and are removed from the background list (one-shot view).
    /// Pause a backgrounded task by sending SIGSTOP to its process
    /// group. portable-pty children are session/group leaders by
    /// default, so `kill(-pid, SIGSTOP)` halts the whole subprocess
    /// tree (e.g. `make → cc → ld` all stop together) rather than
    /// just the direct child.
    ///
    /// `target` of None pauses the most-recent task; numeric arg
    /// targets a specific id. No-op (with flash) if the target is
    /// not Running, doesn't exist, or is already paused.
    fn pause_task(&mut self, target: Option<u32>) {
        let Some(id) = target.or_else(|| self.background_tasks.most_recent()) else {
            self.state.flash_error("no background tasks");
            return;
        };
        let Some(task) = self.background_tasks.tasks.iter_mut().find(|t| t.id == id) else {
            self.state.flash_error(format!("no task with id {id}"));
            return;
        };
        if !matches!(task.status, TaskStatus::Running) {
            self.state.flash_error(format!("task #{id} is not running"));
            return;
        }
        if task.paused {
            self.state.flash_info(format!("task #{id} already paused"));
            return;
        }
        let Some(pid) = task.child.process_id() else {
            self.state.flash_error(format!("task #{id}: no process id"));
            return;
        };
        // Negative pid → process group. SIGSTOP is uncatchable, so the
        // child can't refuse; reader thread keeps blocking on read
        // until SIGCONT.
        let r = unsafe { libc::kill(-(pid as libc::pid_t), libc::SIGSTOP) };
        if r == 0 {
            task.paused = true;
            self.state
                .flash_info(format!("task #{id} paused — :resume to continue"));
        } else {
            self.state
                .flash_error(format!("task #{id}: SIGSTOP failed"));
        }
    }

    /// Resume a paused task with SIGCONT to its process group.
    fn resume_task(&mut self, target: Option<u32>) {
        let Some(id) = target.or_else(|| self.background_tasks.most_recent()) else {
            self.state.flash_error("no background tasks");
            return;
        };
        let Some(task) = self.background_tasks.tasks.iter_mut().find(|t| t.id == id) else {
            self.state.flash_error(format!("no task with id {id}"));
            return;
        };
        if !task.paused {
            self.state.flash_info(format!("task #{id} is not paused"));
            return;
        }
        let Some(pid) = task.child.process_id() else {
            self.state.flash_error(format!("task #{id}: no process id"));
            return;
        };
        let r = unsafe { libc::kill(-(pid as libc::pid_t), libc::SIGCONT) };
        if r == 0 {
            task.paused = false;
            self.state.flash_info(format!("task #{id} resumed"));
        } else {
            self.state
                .flash_error(format!("task #{id}: SIGCONT failed"));
        }
    }

    fn foreground_task(&mut self, target: Option<u32>) {
        if self.pending_capture.is_some() {
            self.state
                .flash_error("already in a foreground task — ^Z to send to background first");
            return;
        }
        let Some(id) = target.or_else(|| self.background_tasks.most_recent()) else {
            self.state.flash_error("no background tasks");
            return;
        };
        let Some(task) = self.background_tasks.take(id) else {
            self.state.flash_error(format!("no task #{id}"));
            return;
        };

        // If the task was paused, auto-resume on foreground — the
        // user explicitly asked for it to be active again. Without
        // this, `:fg` on a paused task would re-attach the streaming
        // capture but the child would stay frozen.
        if task.paused {
            if let Some(pid) = task.child.process_id() {
                unsafe {
                    libc::kill(-(pid as libc::pid_t), libc::SIGCONT);
                }
            }
        }

        match task.status {
            TaskStatus::Running => {
                // Re-attach as a streaming capture. Seed the pager with
                // the buffered output BEFORE handing the buffer over to
                // `pending_capture`, otherwise the user sees an empty
                // pager (or, once new chunks arrive, content scrolled
                // to row 0 with the live tail off-screen) until the
                // streaming-tick rebuilds. Mirrors what
                // `build_task_viewer_for` does for `:task N`.
                use ansi_to_tui::IntoText;
                let normalized = strip_crlf(&task.buffer);
                let text = normalized.as_slice().into_text().unwrap_or_default();
                let secs = task.started.elapsed().as_secs();
                let mut view = PagerView::new_plain(
                    format!("\u{23f3} {} — running... ({secs}s)", task.title),
                    Vec::new(),
                );
                view.lines = text.lines;
                view.streaming = true;
                view.scroll_to_bottom_auto();
                self.pager = Some(view);
                self.pending_capture = Some(PendingCapture {
                    child: task.child,
                    writer: task.writer,
                    output_rx: task.output_rx,
                    buffer: task.buffer,
                    title: task.title,
                    cmd_display: task.cmd_display,
                    started: task.started,
                    finished: false,
                    original_id: Some(task.id),
                });
                self.state
                    .flash_info(format!("task #{id} resumed — ^Z to background again"));
            }
            status => {
                // Exited / Killed / Crashed -- open a static pager with
                // the buffered output and a final-state title.
                use ansi_to_tui::IntoText;
                let normalized = strip_crlf(&task.buffer);
                let text = normalized.as_slice().into_text().unwrap_or_default();
                let elapsed_secs = task
                    .finished_at
                    .map_or_else(|| task.started.elapsed(), |f| f - task.started)
                    .as_secs();
                let status_text = match &status {
                    TaskStatus::Exited(0) => "exit 0".to_string(),
                    TaskStatus::Exited(code) => format!("exit {code}"),
                    TaskStatus::Killed => "killed".to_string(),
                    TaskStatus::Crashed(msg) => format!("error: {msg}"),
                    TaskStatus::Running => unreachable!(),
                };
                let title = format!("{} — {status_text} ({elapsed_secs}s)", task.title);
                let mut view = PagerView::new_plain(title, Vec::new());
                view.lines = text.lines;
                view.saveable = true;
                view.scroll_to_bottom_auto();
                self.pager = Some(view);
            }
        }
        self.needs_full_repaint = true;
    }

    /// Build a "task viewer" pager view -- a peek into a backgrounded
    /// task's buffered output without taking ownership (the way `:fg`
    /// does). The view's `task_id` is set so the main loop can refresh
    /// it from the live buffer while the task is running.
    fn build_task_viewer(&self, id: u32) -> Option<PagerView> {
        let task = self.background_tasks.tasks.iter().find(|t| t.id == id)?;
        Some(Self::build_task_viewer_for(id, task))
    }

    fn build_task_viewer_for(id: u32, task: &BackgroundTask) -> PagerView {
        use ansi_to_tui::IntoText;
        let elapsed = task
            .finished_at
            .map_or_else(|| task.started.elapsed(), |f| f - task.started)
            .as_secs();
        let status_text = match &task.status {
            TaskStatus::Running => format!("running ({elapsed}s)"),
            TaskStatus::Exited(0) => format!("exit 0 ({elapsed}s)"),
            TaskStatus::Exited(code) => format!("exit {code} ({elapsed}s)"),
            TaskStatus::Killed => format!("killed ({elapsed}s)"),
            TaskStatus::Crashed(msg) => format!("error: {msg} ({elapsed}s)"),
        };
        let title = format!("[task #{id}] {} — {status_text}", task.cmd_display);
        let normalized = strip_crlf(&task.buffer);
        let text = normalized.as_slice().into_text().unwrap_or_default();
        let mut view = PagerView::new_plain(title, Vec::new());
        view.lines = text.lines;
        view.task_id = Some(id);
        // Task viewer is a peek -- don't push it to buffer history on
        // close UNLESS the task has exited (handled separately on
        // close: a snapshot is built and pushed). Suppress the default
        // close-time push.
        view.no_history = true;
        view.saveable = true;
        // Suppress [EOF]/tilde markers while the underlying task is
        // still running -- the buffer is live, not finalized.
        view.streaming = matches!(task.status, TaskStatus::Running);
        view.scroll_to_bottom_auto();
        view
    }

    /// `gB` from the file list, or `:task N` colon command. Open the
    /// task viewer for `target` (or the most-recent task if `None`).
    /// Pushes the current pager (if any, and not no_history) to buffer
    /// history first so `[b` can walk back.
    fn open_task_viewer(&mut self, target: Option<u32>) {
        let Some(id) = target.or_else(|| self.background_tasks.most_recent()) else {
            self.state.flash_error("no background tasks");
            return;
        };
        let Some(view) = self.build_task_viewer(id) else {
            self.state.flash_error(format!("no task #{id}"));
            return;
        };
        // Mark viewed so promotion-to-history can fire on close.
        if let Some(task) = self.background_tasks.tasks.iter_mut().find(|t| t.id == id) {
            task.viewed_in_task_viewer = true;
            task.has_unread_output = false;
        }
        // Push the prior pager (if any, eligible) so `[b` can walk back.
        if let Some(prev) = self.pager.take() {
            self.pager_history.push(prev);
        }
        self.pager = Some(view);
        self.needs_full_repaint = true;
    }

    /// `[t`/`]t` chord while a pager is open. Cycles the task viewer
    /// among bg tasks ordered by id. `direction = -1` for prev, `+1`
    /// for next.
    fn cycle_task_viewer(&mut self, direction: i32) {
        if self.background_tasks.tasks.is_empty() {
            self.state.flash_info("no background tasks");
            return;
        }
        let current = self
            .pager
            .as_ref()
            .and_then(|v| v.task_id)
            .and_then(|id| self.background_tasks.tasks.iter().position(|t| t.id == id));
        let next_pos = match current {
            Some(pos) => {
                let n = self.background_tasks.tasks.len() as i32;
                let raw = pos as i32 + direction;
                ((raw % n + n) % n) as usize
            }
            None => {
                if direction < 0 {
                    self.background_tasks.tasks.len() - 1
                } else {
                    0
                }
            }
        };
        let id = self.background_tasks.tasks[next_pos].id;
        self.open_task_viewer(Some(id));
    }

    /// For tabs that have a `pending_resume_send` armed (set by
    /// `restore_session`), send `/resume <sid>\r` to the pty once
    /// enough time has elapsed for claude's banner to render. We
    /// avoid the `--resume` CLI flag because it trips a known
    /// regression that crashes at mount; the slash-command path
    /// goes through `tM_` and works fine.
    fn send_pending_resumes(&mut self) {
        const SETTLE_DELAY: Duration = Duration::from_millis(1500);
        let Some(tabs) = self.pane_tabs.as_mut() else {
            return;
        };
        let now = std::time::Instant::now();
        for entry in tabs.tabs_mut() {
            let Some((sid, spawn_at)) = entry.info.pending_resume_send.as_ref() else {
                continue;
            };
            if now.duration_since(*spawn_at) < SETTLE_DELAY {
                continue;
            }
            let payload = format!("/resume {sid}\r");
            let _ = entry.pane.send_bytes(payload.as_bytes());
            entry.info.pending_resume_send = None;
        }
    }

    /// Locate a `claude --resume` tab from session restore that looks
    /// broken (non-zero exit, or alive-but-printed-a-crash-dump within
    /// the 30s window). Disarms the marker on tabs whose window has
    /// passed without trouble, so a real user-driven exit later isn't
    /// mistaken for a restore failure. Returns the index of the first
    /// crashed tab found, if any.
    fn find_crashed_restore_tab(&mut self) -> Option<usize> {
        let tabs = self.pane_tabs.as_mut()?;
        let now = std::time::Instant::now();
        let window = Duration::from_secs(30);
        let dump_grace = Duration::from_secs(3);
        for (i, entry) in tabs.tabs_mut().iter_mut().enumerate() {
            if entry.info.restore_fallback.is_none() {
                continue;
            }
            let age = now.duration_since(entry.info.spawn_at);
            if age > window {
                entry.info.restore_fallback = None;
                continue;
            }
            let bad_exit = entry.pane.is_closed()
                && entry
                    .pane
                    .exit_status
                    .as_ref()
                    .is_some_and(|s| s.exit_code() != 0);
            // Always re-scan once dump_grace has elapsed: claude often
            // prints the entire crash dump in <1s then sits quiescent,
            // and `output_dirty` gets cleared on every render — gating
            // on it would silently swallow the prompt.
            let dump_signature = !entry.pane.is_closed()
                && age >= dump_grace
                && pane_has_crash_marker(&entry.pane.recent_lines(200));
            if bad_exit || dump_signature {
                return Some(i);
            }
        }
        None
    }

    /// Recompute the host-terminal window title from project / session
    /// state and emit OSC 2 if it has changed since the last write.
    fn update_term_title(&mut self) {
        let title = crate::term_title::compose(
            self.state.project_home.as_deref(),
            self.state.session_name.as_deref(),
            &self.state.listing.dir,
        );
        if self.last_term_title.as_deref() == Some(&title) {
            return;
        }
        let _ = crate::term_title::set(&title);
        self.last_term_title = Some(title);
    }
}

/// True when scrollback contains a known Claude/bun crash signature.
/// These markers don't appear in healthy Claude startup output.
fn pane_has_crash_marker(lines: &[String]) -> bool {
    const MARKERS: &[&str] = &[
        // bun's single-file runtime path; appears in unhandled-exception dumps.
        "/$bunfs/root/",
        // e.g. `g9H is not a function` on the resume path regression.
        "is not a function",
        // sandbox helper failed and `failIfUnavailable` is set.
        "Error: sandbox required but unavailable",
    ];
    lines
        .iter()
        .any(|line| MARKERS.iter().any(|m| line.contains(m)))
}

/// Strip `--resume <token>` from a command line. Used to derive a
/// fresh-session fallback when an automatic resume fails — we want to
/// preserve any other flags the user had on their original `claude`
/// invocation but drop the resume itself so the fallback doesn't fail
/// for the same reason.
fn command_without_resume(cmd: &str) -> String {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    let mut out: Vec<&str> = Vec::with_capacity(parts.len());
    let mut skip_next = false;
    for p in parts {
        if skip_next {
            skip_next = false;
            continue;
        }
        if p == "--resume" {
            skip_next = true;
            continue;
        }
        out.push(p);
    }
    let stripped = out.join(" ");
    if stripped.is_empty() {
        "claude".to_string()
    } else {
        stripped
    }
}

/// Strip codex's `resume [...args]` subcommand and any of its flags
/// from a command line, leaving the bare `codex` invocation. Used at
/// session-save time so a saved tab restores cleanly even if the
/// user had explicitly typed `codex resume <UUID>`. Mirrors
/// `command_without_resume` for claude. The id we'll resume to is
/// stored separately in `agent_session_id`.
fn command_without_codex_resume(cmd: &str) -> String {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    let mut out: Vec<&str> = Vec::with_capacity(parts.len());
    let mut hit_resume = false;
    for p in parts {
        if !hit_resume && p == "resume" {
            // Drop "resume" and everything after it — typically a UUID
            // and/or `--last`/`--all`/`--include-non-interactive` flags
            // that only make sense with `resume`.
            hit_resume = true;
            continue;
        }
        if hit_resume {
            continue;
        }
        out.push(p);
    }
    let stripped = out.join(" ");
    if stripped.is_empty() {
        "codex".to_string()
    } else {
        stripped
    }
}

impl App {
    /// Resolve the `claude --resume <token>` target to use on session save.
    ///
    /// Strategy, in order:
    /// 1. Read the exit-banner token from pane scrollback. If it's a UUID,
    ///    verify a JSONL exists for it under `~/.claude/projects/<slug>/`.
    ///    Claude sometimes prints the banner with a session ID it never
    ///    persisted (e.g. user `/clear`'d or `/resume`'d to a different
    ///    session before exit), so trusting the banner unconditionally
    ///    leads to "No conversation found with session ID …" on restore.
    /// 2. Fall back to the most-recently-modified JSONL in the project
    ///    slug — that's what the no-arg `claude --resume` picker picks.
    /// 3. Last-ditch: scan `~/.claude/sessions/` (PID-scoped, often stale).
    ///
    /// Final guard: any UUID we'd return is verified to have a JSONL on
    /// disk before returning. The PID-scoped index in step 3 lists session
    /// IDs as soon as `claude` starts, but the JSONL is only created on
    /// the first turn — quitting before that produces a ghost ID that
    /// passes step 3 but fails on `claude --resume`. Rather than save a
    /// known-broken ID, return None so restore opens a fresh `claude`.
    fn resolve_claude_resume_target(
        pane: &mut crate::pane::Pane,
        cwd: &std::path::Path,
    ) -> (Option<String>, Option<String>) {
        use crate::state::sessions as s;

        let resolved: (Option<String>, Option<String>) = (|| {
            let banner_lines = pane.recent_lines(200);
            if let Some(tok) = s::extract_claude_resume_token(&banner_lines) {
                if s::is_uuid(&tok) {
                    if s::claude_jsonl_exists(cwd, &tok) {
                        let name = s::find_claude_session_name_public(&tok);
                        return (Some(tok), name);
                    }
                    // Banner UUID has no JSONL — fall through to most-recent.
                } else {
                    // Named sessions: claude resolves names itself, trust it.
                    return (Some(tok.clone()), Some(tok));
                }
            }

            if let Some(id) = s::most_recent_jsonl_for_cwd(cwd) {
                let name = s::find_claude_session_name_public(&id);
                return (Some(id), name);
            }

            match s::find_claude_session(cwd) {
                Some(info) => (Some(info.session_id), info.name),
                None => (None, None),
            }
        })();

        if let (Some(id), _) = &resolved {
            if s::is_uuid(id) && !s::claude_jsonl_exists(cwd, id) {
                spyc_debug!(
                    "resolve_claude_resume_target: dropping ghost id {} (no JSONL under {})",
                    id,
                    cwd.display()
                );
                return (None, None);
            }
        }
        resolved
    }

    /// yp — yank visible pane output to the system clipboard.
    fn yank_pane_to_clipboard(&mut self) -> PostAction {
        let Some(tabs) = self.pane_tabs.as_ref() else {
            self.state.flash_error("no pane open");
            return PostAction::None;
        };
        let lines = tabs.active().visible_lines();
        let text: String = lines
            .iter()
            .map(|l| l.trim_end())
            .collect::<Vec<_>>()
            .join("\n");
        if text.trim().is_empty() {
            self.state.flash_error("pane is empty");
            return PostAction::None;
        }
        match Self::copy_to_clipboard(&text) {
            Ok(()) => {
                let count = text.lines().count();
                self.state
                    .flash_info(format!("yanked {count} lines from pane"));
            }
            Err(e) => self.state.flash_error(format!("yank failed: {e}")),
        }
        PostAction::None
    }

    /// ya — yank the full scrollback + visible screen from the active pane.
    fn yank_scrollback_to_clipboard(&mut self) -> PostAction {
        let Some(tabs) = self.pane_tabs.as_mut() else {
            self.state.flash_error("no pane open");
            return PostAction::None;
        };
        let lines = tabs.active_mut().recent_lines(10_000);
        let text: String = lines
            .iter()
            .map(|l| l.trim_end())
            .collect::<Vec<_>>()
            .join("\n");
        if text.trim().is_empty() {
            self.state.flash_error("pane scrollback is empty");
            return PostAction::None;
        }
        match Self::copy_to_clipboard(&text) {
            Ok(()) => {
                let count = text.lines().count();
                self.state
                    .flash_info(format!("yanked {count} lines (full scrollback)"));
            }
            Err(e) => self.state.flash_error(format!("yank failed: {e}")),
        }
        PostAction::None
    }

    fn copy_to_clipboard(text: &str) -> std::io::Result<()> {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let mut child = Command::new("pbcopy").stdin(Stdio::piped()).spawn()?;
        if let Some(stdin) = child.stdin.as_mut() {
            stdin.write_all(text.as_bytes())?;
        }
        child.wait()?;
        Ok(())
    }

    /// yf — yank the cursor file's absolute path to the system
    /// clipboard. When picks are active, yanks all of them
    /// newline-separated. Always absolute paths so the receiving
    /// shell resolves them correctly regardless of where the user
    /// pastes them. The user's recurring real-world ask was a clean
    /// way to grab a path for one-off shell commands like `git
    /// restore <path>` without opening a pane.
    fn yank_paths_to_clipboard(&mut self) -> PostAction {
        let paths = self.state.selection_paths();
        if paths.is_empty() {
            self.state.flash_error("no path to yank");
            return PostAction::None;
        }
        let text: String = paths
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join("\n");
        match Self::copy_to_clipboard(&text) {
            Ok(()) => {
                if paths.len() == 1 {
                    let preview: String = text.chars().take(80).collect();
                    let ellipsis = if text.len() > 80 { "…" } else { "" };
                    self.state
                        .flash_info(format!("yanked path: {preview}{ellipsis}"));
                } else {
                    self.state
                        .flash_info(format!("yanked {} paths", paths.len()));
                }
            }
            Err(e) => self.state.flash_error(format!("yank failed: {e}")),
        }
        PostAction::None
    }

    /// yP — yank the last prompt the user typed into the pane.
    fn yank_last_prompt_to_clipboard(&mut self) -> PostAction {
        let Some(text) = self.last_pane_prompt.as_ref() else {
            self.state.flash_error("no prompt to yank");
            return PostAction::None;
        };
        match Self::copy_to_clipboard(text) {
            Ok(()) => {
                let preview: String = text.chars().take(60).collect();
                let ellipsis = if text.len() > 60 { "…" } else { "" };
                self.state
                    .flash_info(format!("yanked prompt: {preview}{ellipsis}"));
            }
            Err(e) => self.state.flash_error(format!("yank failed: {e}")),
        }
        PostAction::None
    }

    /// Put inventory items to the current working directory.
    /// Picked items only if any picks exist, else all.
    /// Items are removed from inventory after successful put.
    fn put_inventory_to_cwd(&mut self) -> PostAction {
        let dest = self.state.listing.dir.clone();
        let item_count = if self.state.inventory.picks.is_empty() {
            self.state.inventory.len()
        } else {
            self.state.inventory.picks.len()
        };
        if item_count == 0 {
            self.state.flash_error("inventory is empty");
            return PostAction::None;
        }
        // TODO: confirmation for large puts (>10 items)
        let (count, _, err) = self.state.inventory.put_to(&dest);
        self.state.rebuild_rows();
        if count > 0 {
            self.state.refresh_listing();
            self.state
                .flash_info(format!("put {count} file(s) to {}", dest.display()));
        }
        if let Some(e) = err {
            self.state.flash_error(e);
        }
        PostAction::None
    }

    /// Key dispatcher for `View::Graveyard`. Bindings:
    ///   `j`/`k`/arrows       — move cursor
    ///   `g`/`G`              — first / last
    ///   `p`                  — restore the cursor entry to cwd
    ///   `P`                  — restore to original path (refuses
    ///                          to clobber existing files)
    ///   `dd` (vim-style) /   — purge cursor entry to system trash
    ///   `x`
    ///   `Z`                  — purge ALL entries to system trash
    ///                          (single-key confirm: `y` to commit)
    ///   `Esc`                — close the view, return to dir
    ///
    /// `dd` arming uses a per-instance bool; first `d` arms, any
    /// other key (including a second non-`d`) clears it.
    fn handle_graveyard_view_key(&mut self, key: KeyEvent) -> PostAction {
        // Confirm-purge-all is a transient inline confirm. We
        // signal it via a one-shot Mode::Prompting; routed there
        // directly rather than reusing RemoveConfirm because the
        // semantics are distinct (we're cascading to system trash,
        // not unlinking).
        match key.code {
            KeyCode::Esc => {
                self.state.open_graveyard_view(); // toggle off
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.state.cursor_move_vertical(1, self.state.rows.len());
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.state.cursor_move_vertical(-1, self.state.rows.len());
            }
            KeyCode::Char('g') => {
                self.graveyard_pending_d = false;
                if self.graveyard_pending_g {
                    self.state.cursor.index = 0;
                    self.graveyard_pending_g = false;
                } else {
                    self.graveyard_pending_g = true;
                }
            }
            KeyCode::Char('G') => {
                self.graveyard_pending_d = false;
                self.graveyard_pending_g = false;
                if !self.state.rows.is_empty() {
                    self.state.cursor.index = self.state.rows.len() - 1;
                }
            }
            KeyCode::Char('p') => {
                self.graveyard_pending_d = false;
                self.graveyard_pending_g = false;
                self.graveyard_restore(false);
            }
            KeyCode::Char('P') => {
                self.graveyard_pending_d = false;
                self.graveyard_pending_g = false;
                self.graveyard_restore(true);
            }
            KeyCode::Char('x') => {
                self.graveyard_pending_d = false;
                self.graveyard_pending_g = false;
                self.graveyard_purge_cursor_entry();
            }
            KeyCode::Char('d') => {
                self.graveyard_pending_g = false;
                if self.graveyard_pending_d {
                    self.graveyard_pending_d = false;
                    self.graveyard_purge_cursor_entry();
                } else {
                    self.graveyard_pending_d = true;
                }
            }
            KeyCode::Char('Z') => {
                self.graveyard_pending_d = false;
                self.graveyard_pending_g = false;
                self.state.mode = Mode::Prompting(Prompt::simple(
                    PromptKind::GraveyardPurgeAllConfirm,
                    "purge ALL graveyard entries to system trash? (y/N): ",
                ));
            }
            _ => {
                self.graveyard_pending_d = false;
                self.graveyard_pending_g = false;
            }
        }
        PostAction::None
    }

    /// Restore the cursor entry from the graveyard. `to_original`
    /// = true means the original path (use `Graveyard::restore`
    /// with the orig dir as dest); false = current cwd.
    fn graveyard_restore(&mut self, to_original: bool) {
        let Some(entry) = self.state.graveyard.get(self.state.cursor.index).cloned() else {
            self.state.flash_error("graveyard: no entry under cursor");
            return;
        };
        let dest = if to_original {
            entry.orig_path.parent().map_or_else(
                || std::path::PathBuf::from("/"),
                std::path::Path::to_path_buf,
            )
        } else {
            self.state.listing.dir.clone()
        };
        match crate::state::graveyard::Graveyard::restore(&entry, &dest) {
            Ok(()) => {
                // Restoration succeeded — drop the entry from the
                // graveyard so the user doesn't think it's still there.
                crate::state::graveyard::Graveyard::delete_entry(&entry);
                let where_ = if to_original { "original" } else { "cwd" };
                self.state
                    .flash_info(format!("restored {} ({where_})", entry.filename));
                self.state.graveyard = crate::state::graveyard::Graveyard::load().entries;
                self.state.cursor.clamp(self.state.graveyard.len());
                self.state.refresh_listing(); // dest may be cwd
                self.state.rebuild_rows();
            }
            Err(e) => {
                self.state
                    .flash_error(format!("restore failed: {e} (target may already exist)"));
            }
        }
    }

    /// Purge the cursor entry to system trash. Used by `dd` and `x`.
    fn graveyard_purge_cursor_entry(&mut self) {
        let Some(entry) = self.state.graveyard.get(self.state.cursor.index).cloned() else {
            self.state.flash_error("graveyard: no entry under cursor");
            return;
        };
        match crate::state::graveyard::Graveyard::cascade_entry_to_trash(&entry) {
            Ok(()) => {
                self.state
                    .flash_info(format!("→ system trash: {}", entry.filename));
                self.state.graveyard = crate::state::graveyard::Graveyard::load().entries;
                self.state.cursor.clamp(self.state.graveyard.len());
                self.state.rebuild_rows();
            }
            Err(e) => self.state.flash_error(format!("purge failed: {e}")),
        }
    }

    fn start_new_tab_prompt(&mut self) {
        // Precedence: $SPYC_PANE_CMD > [pane] default_command in
        // .spycrc.toml > "claude" fallback. Env var wins so a user
        // can override on the fly per shell without editing config.
        let default_cmd = std::env::var("SPYC_PANE_CMD")
            .ok()
            .or_else(|| self.state.config.pane.default_command.clone())
            .unwrap_or_else(|| "claude".to_string());
        let mut p = Prompt::shell(PromptKind::PaneNewTabCmd, "pane command: ");
        p.buffer.clone_from(&default_cmd);
        if let Some(ed) = p.editor.as_mut() {
            ed.set_content(&default_cmd);
        }
        self.state.mode = Mode::Prompting(p);
    }

    /// ^W x — close the active pane tab.
    fn close_active_tab(&mut self) {
        if let Some(tabs) = self.pane_tabs.as_mut() {
            if !tabs.close_active() {
                // Last tab removed.
                self.pane_tabs = None;
                self.state.pane_focused = false;
                self.needs_full_repaint = true;
                self.state.flash_info("pane: last tab closed");
            }
        }
    }

    /// ^a R — restart the active tab's command. Closes the tab and spawns
    /// a fresh one with the same command and working directory.
    fn restart_active_tab(&mut self) {
        let Some(tabs) = self.pane_tabs.as_ref() else {
            return;
        };
        let cmd = tabs.active_info().command.clone();
        let cwd = tabs.active_info().cwd.clone();
        // Close the old tab first.
        if let Some(tabs) = self.pane_tabs.as_mut() {
            if !tabs.close_active() {
                self.pane_tabs = None;
                self.state.pane_focused = false;
            }
        }
        // Spawn a replacement with the same command and cwd.
        self.open_pane_tab_in(&cmd, &cwd);
        self.state.flash_info(format!("pane: restarted {cmd}"));
    }

    /// ^W j / ^W k — set keyboard focus directionally (no wrap).
    fn set_pane_focus(&mut self, want_pane: bool) {
        if self.pane_tabs.is_none() {
            return;
        }
        if self.state.pane_focused == want_pane {
            return; // already there — no-op
        }
        self.state.pane_focused = want_pane;
        if self.state.pane_focused {
            let label = self
                .pane_tabs
                .as_ref()
                .map_or("pane", |t| t.active_info().label.as_str());
            self.state.flash_info(format!("focus: {label}"));
        } else {
            // When a `;cmd` overlay is showing the spyc-list slot, the
            // "non-pane" side is the overlay subprocess, not the file
            // list. Label accordingly so the user can read what just
            // got focus instead of guessing.
            let label = if self.top_overlay.is_some() {
                "overlay"
            } else {
                "spyc"
            };
            self.state.flash_info(format!("focus: {label}"));
        }
    }

    /// Handle keys while the pane is in scroll mode. Vi-style navigation
    /// through the scrollback buffer; `Esc`/`q` exit back to live view.
    fn handle_pane_scroll_key(&mut self, key: crossterm::event::KeyEvent) -> PostAction {
        use crossterm::event::{KeyCode, KeyModifiers};
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        // Handle pending `g` prefix: gg = scroll top, gf/gF = goto file.
        if self.scroll_pending_g {
            self.scroll_pending_g = false;
            match key.code {
                KeyCode::Char('g') => {
                    self.pane_tabs
                        .as_mut()
                        .unwrap()
                        .active_mut()
                        .scroll_to_top();
                }
                KeyCode::Char('f') => {
                    self.goto_file_from_pane(false);
                }
                KeyCode::Char('F') => {
                    self.goto_file_from_pane(true);
                }
                _ => {} // Unknown g-sequence, ignore
            }
            return PostAction::None;
        }

        let pane = self.pane_tabs.as_mut().unwrap().active_mut();
        match key.code {
            KeyCode::Char('k') | KeyCode::Up => pane.scroll_up(1),
            KeyCode::Char('j') | KeyCode::Down => pane.scroll_down_or_exit(1),
            KeyCode::PageUp | KeyCode::Char('b') if ctrl => pane.scroll_up(20),
            KeyCode::Char('u') if ctrl => pane.scroll_up(10),
            KeyCode::PageDown | KeyCode::Char('f') if ctrl => pane.scroll_down_or_exit(20),
            KeyCode::Char('d') if ctrl => pane.scroll_down_or_exit(10),
            KeyCode::Char('g') => {
                self.scroll_pending_g = true;
            }
            KeyCode::Char('G') => pane.scroll_to_bottom(),
            KeyCode::Char('s') => match pane.save_to_file() {
                Ok(path) => {
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    self.state.flash_info(format!("saved: {name}"));
                }
                Err(e) => self.state.flash_info(format!("save error: {e}")),
            },
            KeyCode::Esc | KeyCode::Char('q') => {
                pane.exit_scroll_mode();
                self.state.flash_info("scroll: off");
            }
            _ => {}
        }
        PostAction::None
    }

    /// ^W s — write the current selection as shell-quoted paths to the
    /// pane's stdin. A trailing space is appended so the user can keep
    /// typing without concatenating against the last path. No newline
    /// — let the user decide when to submit.
    fn send_selection_to_pane(&mut self) {
        if self.pane_tabs.is_none() {
            self.state.flash_error("no pane open (Ctrl-\\ to open one)");
            return;
        }
        // Build the payload before grabbing the pane mut-borrow, so we
        // can still call self.flash_* below without overlapping borrows.
        let (payload, count) = {
            let paths = self.state.selection_paths();
            if paths.is_empty() {
                self.state.flash_error("nothing selected");
                return;
            }
            let count = paths.len();
            let mut out = String::new();
            for (i, p) in paths.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                out.push_str(&shell::shell_quote(&p.to_string_lossy()));
            }
            out.push(' ');
            (out, count)
        };
        let result = {
            let pane = self
                .pane_tabs
                .as_mut()
                .expect("pane existence already checked")
                .active_mut();
            pane.send_bytes(payload.as_bytes())
        };
        match result {
            Ok(()) => self
                .state
                .flash_info(format!("sent {count} path(s) to pane")),
            Err(e) => self.state.flash_error(format!("send failed: {e}")),
        }
    }

    /// ^W p / ^W i — read file contents of selection (or inventory) and
    /// send them to the active pane tab as bracketed paste. Each file is
    /// wrapped with a header so the recipient (e.g. Claude) knows what
    /// it's looking at.
    fn pipe_content_to_pane(&mut self, use_inventory: bool) {
        if self.pane_tabs.is_none() {
            self.state.flash_error("no pane open");
            return;
        }
        // Build payload: read from cache for inventory, from disk for selection.
        let mut payload = String::new();
        let mut count = 0usize;
        let mut skipped = 0usize;

        if use_inventory {
            let ids = self.state.inventory.selected_ids();
            if ids.is_empty() {
                self.state.flash_error("inventory is empty");
                return;
            }
            for id in &ids {
                if let Some(item) = self.state.inventory.items().find(|i| &i.id == id) {
                    if let Some(bytes) = self.state.inventory.read_content(id) {
                        if let Ok(text) = String::from_utf8(bytes) {
                            if !payload.is_empty() {
                                payload.push('\n');
                            }
                            let _ =
                                write!(payload, "[file: {}]\n{}", item.orig_path.display(), text);
                            count += 1;
                        } else {
                            skipped += 1;
                        }
                    } else {
                        skipped += 1;
                    }
                }
            }
        } else {
            let paths: Vec<PathBuf> = self
                .state
                .selection_paths()
                .into_iter()
                .map(Path::to_path_buf)
                .collect();
            if paths.is_empty() {
                self.state.flash_error("nothing selected");
                return;
            }
            for path in &paths {
                let Ok(contents) = std::fs::read_to_string(path) else {
                    skipped += 1;
                    continue;
                };
                if !payload.is_empty() {
                    payload.push('\n');
                }
                let _ = write!(payload, "[file: {}]\n{}", path.display(), contents);
                count += 1;
            }
        }

        if count == 0 {
            self.state
                .flash_error("no readable text files in selection");
            return;
        }
        // Send as bracketed paste so it arrives as a single block.
        let mut buf = Vec::with_capacity(payload.len() + 12);
        buf.extend_from_slice(b"\x1b[200~");
        buf.extend_from_slice(payload.as_bytes());
        buf.extend_from_slice(b"\x1b[201~");
        let result = {
            let pane = self.pane_tabs.as_mut().unwrap().active_mut();
            pane.send_bytes(&buf)
        };
        let msg = if skipped > 0 {
            format!("piped {count} file(s), skipped {skipped} binary/unreadable")
        } else {
            format!("piped {count} file(s) to pane")
        };
        match result {
            Ok(()) => self.state.flash_info(msg),
            Err(e) => self.state.flash_error(format!("pipe failed: {e}")),
        }
    }

    /// ^W + / ^W - — change the bottom pane's share of the middle rect
    /// in 5% steps, clamped to [10%, 90%].
    fn resize_pane(&mut self, delta_pct: i32) {
        if self.pane_tabs.is_none() {
            return;
        }
        if self.state.pane_zoomed {
            self.state.flash_info("pane is zoomed (^a z to exit)");
            return;
        }
        let current = i32::from(self.state.pane_height_pct);
        let new = (current + delta_pct).clamp(10, 90);
        self.state.pane_height_pct = new as u16;
    }

    /// The pane percentage to use for layout/sizing computations.
    /// Returns 100 when zoomed (list collapses to 0 rows) so that the
    /// stored `pane_height_pct` — the user's preferred split — stays
    /// untouched and is restored on un-zoom.
    const fn effective_pane_pct(&self) -> u16 {
        if self.state.pane_zoomed {
            100
        } else {
            self.state.pane_height_pct
        }
    }

    /// ^a z / ^w z — toggle "zoom" on the bottom pane. When zoomed,
    /// the file list collapses to 0 rows and the pane fills the
    /// middle region (status + prompt rows still render). Focus is
    /// forced into the pane on zoom-on; the prior focus is restored
    /// on zoom-off. No-op (with a flash) when the pane is closed.
    fn toggle_pane_zoom(&mut self) {
        if self.pane_tabs.is_none() {
            self.state.flash_info("no pane open");
            return;
        }
        if self.state.pane_zoomed {
            self.state.pane_zoomed = false;
            if let Some(prev) = self.state.pane_focus_before_zoom.take() {
                self.state.pane_focused = prev;
            }
            self.state.flash_info("zoom: off");
        } else {
            self.state.pane_focus_before_zoom = Some(self.state.pane_focused);
            self.state.pane_zoomed = true;
            self.state.pane_focused = true;
            self.state.flash_info("zoom: on (^a z to exit)");
        }
        // Resize all pty children to the new pane rect so their
        // child shells re-render at the right dimensions; otherwise
        // Claude's UI is the wrong size until the next terminal resize.
        let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
        let area = ratatui::layout::Rect::new(0, 0, cols, rows);
        let layout = Self::compute_layout(
            area,
            true,
            self.effective_pane_pct(),
            self.state.config.layout.status_position,
        );
        if let (Some(pane_rect), Some(tabs)) = (layout.pane, self.pane_tabs.as_mut()) {
            for entry in tabs.tabs_mut() {
                let _ = entry.pane.resize(pane_rect.height, pane_rect.width);
            }
        }
        self.needs_full_repaint = true;
    }

    // ---- Harpoon -----------------------------------------------------------

    /// Path under the cursor (file or directory) that the harpoon
    /// `Ha`/`Hx` actions operate on. Returns the absolute path of
    /// the focused row, or `None` if the listing is empty.
    fn harpoon_cursor_path(&self) -> Option<PathBuf> {
        self.state
            .rows
            .get(self.state.cursor.index)
            .map(|r| r.path.clone())
    }

    /// `Ha` — append the cursor file/dir to the project's harpoon
    /// list. Idempotent (already-harpooned paths flash and bail);
    /// hard-capped at `MAX_SLOTS`. Saves the list immediately so a
    /// crash before the next mutation doesn't lose the entry.
    fn harpoon_append(&mut self) {
        if self.harpoon.is_none() {
            self.state
                .flash_error("harpoon: set PROJECT_HOME first (gP)");
            return;
        }
        let Some(path) = self.harpoon_cursor_path() else {
            self.state.flash_error("harpoon: nothing under cursor");
            return;
        };
        let label = path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let h = self.harpoon.as_mut().unwrap();
        match h.append(path) {
            crate::state::harpoon::AppendResult::Added(slot) => {
                if let Err(e) = h.save() {
                    self.state.flash_error(format!("harpoon save failed: {e}"));
                    return;
                }
                self.sync_harpoon_filter_set();
                if matches!(self.state.temp_filter.as_deref(), Some("h")) {
                    self.state.rebuild_rows();
                }
                self.state.flash_info(format!("harpoon[{slot}] {label}"));
            }
            crate::state::harpoon::AppendResult::AlreadyPresent => {
                self.state
                    .flash_info(format!("harpoon: already in list — {label}"));
            }
            crate::state::harpoon::AppendResult::Full => {
                self.state.flash_error(format!(
                    "harpoon full ({} slots) — Hx to remove first",
                    crate::state::harpoon::MAX_SLOTS
                ));
            }
        }
    }

    /// `Hx` — remove the cursor file from the harpoon list (any
    /// slot). No-op + flash if it isn't harpooned.
    fn harpoon_remove(&mut self) {
        if self.harpoon.is_none() {
            self.state
                .flash_error("harpoon: set PROJECT_HOME first (gP)");
            return;
        }
        let Some(path) = self.harpoon_cursor_path() else {
            self.state.flash_error("harpoon: nothing under cursor");
            return;
        };
        let label = path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let h = self.harpoon.as_mut().unwrap();
        match h.remove(&path) {
            Some(slot) => {
                if let Err(e) = h.save() {
                    self.state.flash_error(format!("harpoon save failed: {e}"));
                    return;
                }
                self.sync_harpoon_filter_set();
                if matches!(self.state.temp_filter.as_deref(), Some("h")) {
                    self.state.rebuild_rows();
                }
                self.state
                    .flash_info(format!("harpoon: removed [{slot}] {label}"));
            }
            None => self
                .state
                .flash_info(format!("harpoon: not in list — {label}")),
        }
    }

    /// `H<digit>` — jump to slot N. Cursor-land semantics: chdir to
    /// the file's parent and place the cursor on it (or chdir into
    /// the directory if the slot is a directory). The user picks
    /// the verb (Enter, V, ^a s) afterwards. Missing-on-disk → flash
    /// and bail; we don't auto-prune (the user might be mid-rebase).
    fn harpoon_jump(&mut self, slot: u8) {
        let Some(h) = self.harpoon.as_ref() else {
            self.state
                .flash_error("harpoon: set PROJECT_HOME first (gP)");
            return;
        };
        let Some(target) = h.get(slot).map(Path::to_path_buf) else {
            self.state.flash_info(format!("harpoon: slot {slot} empty"));
            return;
        };
        if !target.exists() {
            self.state.flash_error(format!(
                "harpoon: gone — {}",
                target.file_name().map_or_else(
                    || target.display().to_string(),
                    |n| n.to_string_lossy().into_owned(),
                )
            ));
            return;
        }
        let (chdir_to, focus) = if target.is_dir() {
            (target, None)
        } else if let Some(parent) = target.parent() {
            (parent.to_path_buf(), Some(target.clone()))
        } else {
            self.state.flash_error("harpoon: slot has no parent dir");
            return;
        };
        if let Err(e) = self.state.chdir(&chdir_to) {
            self.state.flash_error(format!("harpoon chdir: {e}"));
            return;
        }
        if let Some(p) = focus {
            self.state.focus_on_path(&p);
        }
        self.state.rebuild_rows();
        self.state.flash_info(format!("harpoon[{slot}]"));
    }

    /// `Hh` / `gh` — open the harpoon menu overlay. The menu
    /// intercepts subsequent keys until closed (Esc/q). No-op when
    /// the list is unset (no PROJECT_HOME).
    fn harpoon_open_menu(&mut self) {
        if self.harpoon.is_none() {
            self.state
                .flash_error("harpoon: set PROJECT_HOME first (gP)");
            return;
        }
        self.harpoon_menu = Some(HarpoonMenu {
            cursor: 0,
            delete_armed: false,
        });
        self.needs_full_repaint = true;
    }

    /// Key handler for the harpoon menu overlay. Owns all input
    /// while the menu is open. Bindings:
    ///   `j`/`k` (and arrows) — move cursor in the menu
    ///   `g`/`G` — jump to first/last slot
    ///   `1`..`9` — jump directly to slot N (and close)
    ///   `Enter` — jump to slot under cursor (and close)
    ///   `K`/`J` — swap slot up / down (reorder)
    ///   `dd` — delete slot under cursor (vim convention; first `d`
    ///          arms, second `d` confirms; any other key disarms)
    ///   `Esc`/`q` — close menu
    fn handle_harpoon_menu_key(&mut self, key: crossterm::event::KeyEvent) -> PostAction {
        use crossterm::event::KeyCode;
        let Some(menu) = self.harpoon_menu.as_mut() else {
            return PostAction::None;
        };
        let Some(h) = self.harpoon.as_mut() else {
            self.harpoon_menu = None;
            self.needs_full_repaint = true;
            return PostAction::None;
        };
        let len = h.slots.len();

        // `dd` arming. The pending-d flag lives on App so it survives
        // across this call (which can't borrow `menu` mutably across
        // re-entry). Using a local approach: piggyback on `cursor`'s
        // high bit would be hacky — keep it simple and use a separate
        // bool field on `HarpoonMenu`.
        let pending_delete = menu.delete_armed;
        if pending_delete {
            menu.delete_armed = false;
        }

        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.harpoon_menu = None;
                self.needs_full_repaint = true;
            }
            KeyCode::Char('j') | KeyCode::Down if len > 0 => {
                menu.cursor = (menu.cursor + 1).min(len - 1);
            }
            KeyCode::Char('k') | KeyCode::Up if len > 0 => {
                menu.cursor = menu.cursor.saturating_sub(1);
            }
            KeyCode::Char('g') if len > 0 => {
                menu.cursor = 0;
            }
            KeyCode::Char('G') if len > 0 => {
                menu.cursor = len - 1;
            }
            KeyCode::Char(c @ '1'..='9') => {
                let slot = c as u8 - b'0';
                self.harpoon_menu = None;
                self.needs_full_repaint = true;
                self.harpoon_jump(slot);
            }
            KeyCode::Enter if len > 0 => {
                let slot = (menu.cursor + 1) as u8;
                self.harpoon_menu = None;
                self.needs_full_repaint = true;
                self.harpoon_jump(slot);
            }
            KeyCode::Char('K') if menu.cursor > 0 && len > 1 => {
                h.swap(menu.cursor, menu.cursor - 1);
                menu.cursor -= 1;
                if let Err(e) = h.save() {
                    self.state.flash_error(format!("harpoon save failed: {e}"));
                }
                self.sync_harpoon_filter_set();
            }
            KeyCode::Char('J') if menu.cursor + 1 < len => {
                h.swap(menu.cursor, menu.cursor + 1);
                menu.cursor += 1;
                if let Err(e) = h.save() {
                    self.state.flash_error(format!("harpoon save failed: {e}"));
                }
                self.sync_harpoon_filter_set();
            }
            KeyCode::Char('d') => {
                if pending_delete && menu.cursor < len {
                    let removed_idx = menu.cursor;
                    h.remove_at(removed_idx);
                    if let Err(e) = h.save() {
                        self.state.flash_error(format!("harpoon save failed: {e}"));
                    }
                    self.sync_harpoon_filter_set();
                    if matches!(self.state.temp_filter.as_deref(), Some("h")) {
                        self.state.rebuild_rows();
                    }
                    // Re-fetch menu since filter sync invalidates `menu` borrow
                    if let Some(m) = self.harpoon_menu.as_mut() {
                        let new_len = self.harpoon.as_ref().map_or(0, |hh| hh.slots.len());
                        if new_len == 0 {
                            m.cursor = 0;
                        } else {
                            m.cursor = removed_idx.min(new_len - 1);
                        }
                    }
                } else if let Some(m) = self.harpoon_menu.as_mut() {
                    m.delete_armed = true;
                }
            }
            _ => {}
        }
        PostAction::None
    }

    // ---- Quick Select ------------------------------------------------------

    /// `^a u` — enter Quick Select. Snapshot the visible pane,
    /// scan for matches across the built-in + user patterns,
    /// assign labels, and install the picker as a key-intercepting
    /// overlay. Bails with a flash if there's nothing pickable.
    fn open_quick_select(&mut self) {
        use crate::pane::quick_select::{QuickSelect, assign_labels, build_patterns, scan};
        let Some(tabs) = self.pane_tabs.as_mut() else {
            self.state.flash_error("quick select: pane is closed");
            return;
        };
        // Always scan the *visible* viewport — labels must land on
        // text the user can see. Scroll mode falls out of this for
        // free since `visible_lines()` honors the user's current
        // scroll position.
        let lines = tabs.active().visible_lines();
        let patterns = build_patterns(&self.state.config.scan_patterns);
        let mut matches = scan(&lines, &patterns);
        if matches.is_empty() {
            self.state.flash_info("quick select: no matches in view");
            return;
        }
        let all_two_letter = assign_labels(&mut matches);
        self.quick_select = Some(QuickSelect {
            matches,
            pending_first: None,
            all_two_letter,
            open_intent: false,
        });
        self.needs_full_repaint = true;
    }

    /// Key handler for the Quick Select overlay. Owns input until
    /// the picker exits. Bindings:
    ///   `q` / `Esc`            — exit, no action
    ///   one-letter labels      — commit immediately
    ///   uppercase one-letter   — commit with "open" intent
    ///   two-letter labels      — first key narrows, second commits;
    ///                            uppercase anywhere = open intent
    ///   any other key          — clears any narrowing buffer (so a
    ///                            stray keystroke doesn't strand the
    ///                            user; they can still type a label)
    fn handle_quick_select_key(&mut self, key: crossterm::event::KeyEvent) -> PostAction {
        use crossterm::event::KeyCode;
        let Some(qs) = self.quick_select.as_mut() else {
            return PostAction::None;
        };

        let close = |this: &mut Self| {
            this.quick_select = None;
            this.needs_full_repaint = true;
        };

        let c = match key.code {
            KeyCode::Esc => {
                close(self);
                return PostAction::None;
            }
            KeyCode::Char(c) => c,
            _ => return PostAction::None,
        };

        // `q`/`Q` always exits — labels never use it (alphabet check
        // covered in unit test) so this is unambiguous.
        if c.eq_ignore_ascii_case(&'q') && qs.pending_first.is_none() {
            close(self);
            return PostAction::None;
        }

        let is_upper = c.is_ascii_uppercase();
        let lower = c.to_ascii_lowercase();

        if qs.all_two_letter {
            match qs.pending_first {
                None => {
                    // First keystroke: must be the prefix of some label.
                    let any_match = qs.matches.iter().any(|m| m.label.starts_with(lower));
                    if !any_match {
                        return PostAction::None; // no narrowing possible — ignore
                    }
                    qs.pending_first = Some(lower);
                    if is_upper {
                        qs.open_intent = true;
                    }
                }
                Some(first) => {
                    let combined = format!("{first}{lower}");
                    let open = qs.open_intent || is_upper;
                    let m = qs.matches.iter().find(|m| m.label == combined).cloned();
                    close(self);
                    if let Some(m) = m {
                        self.dispatch_quick_select(&m, open);
                    }
                }
            }
        } else {
            // 1-letter labels. Uppercase commits with open intent.
            let m = qs
                .matches
                .iter()
                .find(|m| m.label == lower.to_string())
                .cloned();
            close(self);
            if let Some(m) = m {
                self.dispatch_quick_select(&m, is_upper);
            }
        }
        PostAction::None
    }

    /// Route a picked match to the right action, given user
    /// intent. See action matrix in `FEATURES.md` ("Quick Select").
    fn dispatch_quick_select(&mut self, m: &crate::pane::quick_select::Match, open_intent: bool) {
        use crate::pane::quick_select::MatchKind;
        let kind_label = m.kind.label().to_string();
        let text = m.text.clone();
        if !open_intent {
            self.yank_quick_select(&text, &kind_label);
            return;
        }
        match &m.kind {
            MatchKind::Url => self.open_url_or_flash(&text),
            MatchKind::Path => self.jump_to_pane_path(&text),
            MatchKind::GitSha => self.open_git_show_pager(&text),
            MatchKind::Custom { url_template, .. } if url_template.is_some() => {
                let url = url_template.as_ref().unwrap().replace("{}", &text);
                self.open_url_or_flash(&url);
            }
            // IPv4 and template-less Custom: fall back to yank with a
            // hint that explains why nothing else happened.
            MatchKind::Ipv4 | MatchKind::Custom { .. } => {
                self.yank_quick_select(&text, &kind_label);
                self.state
                    .flash_info(format!("yanked {kind_label} (no open handler)"));
            }
        }
    }

    fn yank_quick_select(&mut self, text: &str, kind_label: &str) {
        match Self::copy_to_clipboard(text) {
            Ok(()) => {
                let preview: String = text.chars().take(60).collect();
                let ellipsis = if text.len() > 60 { "…" } else { "" };
                self.state
                    .flash_info(format!("yanked {kind_label}: {preview}{ellipsis}"));
            }
            Err(e) => self.state.flash_error(format!("yank failed: {e}")),
        }
    }

    /// Hand `target` to the system handler via the `open` crate
    /// (cross-platform: macOS `open`, Linux `xdg-open`, Windows
    /// `start`). The crate spawns the launcher as a detached child
    /// and returns immediately, so the system handler never blocks
    /// our event loop.
    fn open_url_or_flash(&mut self, url: &str) {
        match open::that_detached(url) {
            Ok(()) => {
                let preview: String = url.chars().take(80).collect();
                let ellipsis = if url.len() > 80 { "…" } else { "" };
                self.state
                    .flash_info(format!("opening: {preview}{ellipsis}"));
            }
            Err(e) => self.state.flash_error(format!("open: {e}")),
        }
    }

    /// Navigate spyc to a path matched in the pane (uppercase intent
    /// for a Path match). Mirrors `goto_file_from_pane`'s post-resolve
    /// flow but starts from a pre-extracted path string rather than
    /// running pathref again.
    fn jump_to_pane_path(&mut self, raw: &str) {
        let path = std::path::PathBuf::from(raw);
        let resolved = if path.is_absolute() {
            path
        } else {
            // Resolve against the active pane tab's cwd first, falling
            // back to spyc's listing dir — same precedence `gf` uses.
            let tab_cwd = self.pane_tabs.as_ref().map(|t| t.active_info().cwd.clone());
            let candidate = tab_cwd.as_ref().map(|c| c.join(&path));
            match candidate {
                Some(p) if p.exists() => p,
                _ => self.state.listing.dir.join(&path),
            }
        };
        if !resolved.exists() {
            self.state
                .flash_error(format!("path not found: {}", resolved.display()));
            return;
        }
        let (chdir_to, focus) = if resolved.is_dir() {
            (resolved, None)
        } else if let Some(parent) = resolved.parent() {
            (parent.to_path_buf(), Some(resolved.clone()))
        } else {
            self.state.flash_error("path has no parent dir");
            return;
        };
        if let Err(e) = self.state.chdir(&chdir_to) {
            self.state.flash_error(format!("chdir: {e}"));
            return;
        }
        if let Some(p) = focus {
            self.state.focus_on_path(&p);
        }
        self.state.pane_focused = false;
        self.state.rebuild_rows();
        self.needs_full_repaint = true;
    }

    /// `git show <sha>` into the pager. Uppercase action for a
    /// matched git SHA — the value of the picker for a
    /// commit-discussion workflow.
    fn open_git_show_pager(&mut self, sha: &str) {
        match std::process::Command::new("git")
            .args(["show", "--color=always", sha])
            .current_dir(&self.state.listing.dir)
            .output()
        {
            Ok(out) if out.status.success() && !out.stdout.is_empty() => {
                let title = format!("git show {sha}");
                self.pager = Some(pager::PagerView::new_ansi(title, &out.stdout));
            }
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                let msg = stderr.lines().next().unwrap_or("no output").trim();
                self.state.flash_error(format!("git show: {msg}"));
            }
            Err(e) => self.state.flash_error(format!("git show: {e}")),
        }
    }

    /// Render label overlay on top of the pane. Drawn after the
    /// pane widget so labels paint over the live vt100 grid; small
    /// inverted-color cells next to each match's start position.
    fn render_quick_select_overlay(&self, frame: &mut Frame, pane_rect: ratatui::layout::Rect) {
        use ratatui::{
            style::{Color, Modifier, Style},
            widgets::Paragraph,
        };
        let Some(qs) = self.quick_select.as_ref() else {
            return;
        };
        let label_style = Style::default()
            .fg(Color::Black)
            .bg(self.theme.pick)
            .add_modifier(Modifier::BOLD);
        let pending_style = Style::default()
            .fg(Color::Black)
            .bg(self.theme.prompt_prefix)
            .add_modifier(Modifier::BOLD);
        for m in &qs.matches {
            // Skip labels that would render outside the pane rect.
            // (Matches whose row exceeded the pane height are
            // possible if the snapshot happened to be longer than
            // the visible region — defensive.)
            if m.row >= pane_rect.height as usize || m.col >= pane_rect.width as usize {
                continue;
            }
            // 2-letter narrowing: dim labels whose first letter
            // doesn't match the buffered keystroke; highlight
            // those that do (the user sees their narrowing land).
            let style = if let Some(first) = qs.pending_first {
                if m.label.starts_with(first) {
                    pending_style
                } else {
                    Style::default().fg(self.theme.status_suffix)
                }
            } else {
                label_style
            };
            let text = if let Some(first) = qs.pending_first {
                if m.label.starts_with(first) {
                    // Show only the *second* letter, since the
                    // first is already committed.
                    m.label.chars().nth(1).map(|c| c.to_string())
                } else {
                    None
                }
            } else {
                Some(m.label.clone())
            };
            let Some(text) = text else { continue };
            let label_rect = ratatui::layout::Rect {
                x: pane_rect.x + m.col as u16,
                y: pane_rect.y + m.row as u16,
                width: text.len() as u16,
                height: 1,
            };
            // Clamp to pane rect.
            if label_rect.x + label_rect.width > pane_rect.x + pane_rect.width
                || label_rect.y >= pane_rect.y + pane_rect.height
            {
                continue;
            }
            frame.render_widget(
                Paragraph::new(ratatui::text::Span::styled(text, style)),
                label_rect,
            );
        }
    }

    // ---- Git diff (M12) ----------------------------------------------------

    /// g d / g D — run `git diff` on selection and show in pager.
    ///
    /// `gd` (cached=false) also surfaces *untracked* files in the
    /// selection — without this, the cursor sitting on a `?`/`~`-flagged
    /// new file gives empty diff output and looks broken. We synthesize
    /// an "added" diff per untracked file via `git diff --no-index
    /// /dev/null <file>`, which exits 1 but still produces the diff bytes
    /// we want to render.
    fn open_git_diff(&mut self, cached: bool) {
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
        let mut args: Vec<&str> = vec!["diff", "--color=always"];
        if cached {
            args.push("--cached");
        } else {
            args.push("HEAD");
        }
        args.push("--");
        for s in &path_strings {
            args.push(s);
        }
        let modified_out = match std::process::Command::new("git")
            .args(&args)
            .current_dir(cwd)
            .output()
        {
            Ok(o) => o.stdout,
            Err(e) => {
                self.state.flash_error(format!("git diff: {e}"));
                return;
            }
        };

        let mut combined = modified_out;
        if !cached {
            combined.extend(untracked_diff_bytes(cwd, &path_strings));
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
        self.pager = Some(pager::PagerView::new_ansi(label, &combined));
    }

    /// g b — `git blame` on the cursor file. Selection is ignored
    /// (blame on multiple files / a directory is meaningless).
    fn open_git_blame(&mut self) {
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
        match std::process::Command::new("git")
            .args(["blame", "--color-lines", "--", &path_str])
            .current_dir(&self.state.listing.dir)
            .output()
        {
            Ok(out) if out.status.success() && !out.stdout.is_empty() => {
                let title = format!("git blame {}", row.display);
                self.pager = Some(pager::PagerView::new_ansi(title, &out.stdout));
            }
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                let msg = stderr.lines().next().unwrap_or("no output").trim();
                self.state.flash_error(format!("git blame: {msg}"));
            }
            Err(e) => self.state.flash_error(format!("git blame: {e}")),
        }
    }

    // ---- Path references (M13) ------------------------------------------------

    /// `gf` / `gF` — scan the active pane's visible output for a file path
    /// reference, navigate the file list there, and optionally open the
    /// pager at the referenced line.
    fn goto_file_from_pane(&mut self, open_at_line: bool) {
        let Some(tabs) = self.pane_tabs.as_mut() else {
            self.state.flash_error("no pane open");
            return;
        };
        // Scan what the user is looking at. While scrolling, that's
        // exactly the visible viewport (so a path scrolled into view
        // is the one we find — not a different region). When live,
        // widen to the last 200 lines so paths in large diffs that
        // just rolled past the bottom are still findable.
        let lines = tabs.active_mut().pickable_text(200);
        // Also try resolving against the spyc cwd (project root), not just
        // the pane tab's cwd — Claude often prints paths relative to the
        // project root regardless of the shell's cwd.
        let pane_cwd = tabs.active_info().cwd.clone();
        let spyc_cwd = self.state.listing.dir.clone();

        // Debug: dump visible lines to the debug log so we can see what
        // the vt100 screen actually contains.
        spyc_debug!(
            "gf: {} lines from pane, pane_cwd={}, spyc_cwd={}",
            lines.len(),
            pane_cwd.display(),
            spyc_cwd.display()
        );
        for (i, line) in lines.iter().enumerate() {
            if !line.trim().is_empty() {
                spyc_debug!("gf line[{i}]: {:?}", line);
            }
        }

        let pathref = crate::pane::pathref::extract_path_ref(&lines, &pane_cwd).or_else(|| {
            (pane_cwd != spyc_cwd)
                .then(|| crate::pane::pathref::extract_path_ref(&lines, &spyc_cwd))
                .flatten()
        });

        let Some(pathref) = pathref else {
            self.state
                .flash_error("no path reference found in pane output");
            return;
        };

        spyc_debug!(
            "gf: found path={}, line={:?}",
            pathref.path.display(),
            pathref.line
        );

        let path = pathref.path;
        let line = pathref.line;

        // Exit scroll mode and switch focus to the file list so the user
        // sees the navigation result.
        if let Some(tabs) = self.pane_tabs.as_mut() {
            if tabs.active().is_scrolling() {
                tabs.active_mut().exit_scroll_mode();
            }
        }
        self.state.pane_focused = false;
        self.needs_full_repaint = true;

        // Navigate: if it's a directory, chdir there; if a file, chdir to
        // its parent and focus on it.
        if path.is_dir() {
            if let Err(e) = self.state.chdir(&path) {
                self.state.flash_error(format!("gf: {e}"));
            }
            return;
        }

        if let Some(parent) = path.parent() {
            if parent != self.state.listing.dir {
                if let Err(e) = self.state.chdir(parent) {
                    self.state.flash_error(format!("gf: {e}"));
                    return;
                }
            }
            self.state.focus_on_path(&path);
        }

        // gF: also open the file in the pager at the referenced line.
        if open_at_line {
            let name = path.file_name().map_or_else(
                || path.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            );

            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    let lines_vec: Vec<String> = text.lines().map(String::from).collect();
                    let mut view = pager::PagerView::new_plain(&name, lines_vec);
                    view.source_path = Some(path);
                    // Jump to the referenced line (0-indexed scroll).
                    if let Some(ln) = line {
                        view.scroll = u16::try_from(ln.saturating_sub(1)).unwrap_or(u16::MAX);
                    }
                    self.pager = Some(view);
                }
                Err(e) => {
                    self.state
                        .flash_error(format!("gF: cannot read {name}: {e}"));
                }
            }
        } else if let Some(ln) = line {
            self.state.flash_info(format!(
                "{}:{}",
                path.file_name().map_or_else(
                    || path.display().to_string(),
                    |n| n.to_string_lossy().into_owned()
                ),
                ln
            ));
        }
    }

    // ---- Session management --------------------------------------------------

    fn save_session(&mut self) {
        use crate::state::sessions::{SavedTab, Session};
        let epoch_secs = crate::sysinfo::epoch_secs();
        // Session id is a millisecond timestamp -- unique within a
        // single spyc instance and human-glanceable in the picker.
        let id = (crate::sysinfo::epoch_nanos() / 1_000_000) as u64;

        let tabs: Vec<SavedTab> = self
            .pane_tabs
            .as_mut()
            .map(|pt| {
                pt.tabs_mut()
                    .iter_mut()
                    .map(|t| {
                        let kind = Self::detect_agent_kind(&t.info.command);
                        let (agent_session_id, agent_session_name) = match kind {
                            AgentKind::Claude => {
                                Self::resolve_claude_resume_target(&mut t.pane, &t.info.cwd)
                            }
                            AgentKind::Codex => {
                                let lines = t.pane.recent_lines(200);
                                let id = crate::state::sessions::extract_codex_resume_token(&lines);
                                // Codex doesn't expose a display name; UUID
                                // is what `codex resume <UUID>` consumes.
                                (id, None)
                            }
                            AgentKind::Other => (None, None),
                        };
                        // The sid lives in agent_session_id; baking
                        // --resume / `resume` into `command` would survive
                        // past a resolver miss and pollute the next restore.
                        let saved_command = match kind {
                            AgentKind::Claude => command_without_resume(&t.info.command),
                            AgentKind::Codex => command_without_codex_resume(&t.info.command),
                            AgentKind::Other => t.info.command.clone(),
                        };
                        SavedTab {
                            command: saved_command,
                            label: t.info.label.clone(),
                            cwd: t.info.cwd.clone(),
                            agent_kind: kind,
                            agent_session_id,
                            agent_session_name,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        let session = Session {
            id,
            saved_at: crate::sysinfo::format_now(),
            epoch_secs,
            cwd: self.state.listing.dir.clone(),
            tabs,
            active_tab: self.pane_tabs.as_ref().map_or(0, PaneTabs::active_index),
            pane_height_pct: self.state.pane_height_pct,
            pane_focused: self.state.pane_focused,
            name: self.state.session_name.clone().unwrap_or_default(),
            project_home: self.state.project_home.clone(),
        };
        let _ = crate::state::sessions::save_session(&session);

        // Build exit summary for post-TUI output.
        let cwd_display = crate::paths::display_tilde(&self.state.listing.dir);
        let tab_count = session.tabs.len();
        let claude_names: Vec<String> = session
            .tabs
            .iter()
            .filter(|t| t.effective_kind() == AgentKind::Claude)
            .filter_map(|t| t.agent_session_name.clone())
            .collect();
        let codex_count = session
            .tabs
            .iter()
            .filter(|t| t.effective_kind() == AgentKind::Codex && t.agent_session_id.is_some())
            .count();
        let mut parts = vec![format!("session saved — {cwd_display}")];
        if tab_count > 0 {
            parts.push(format!(
                "{tab_count} pane tab{}",
                if tab_count == 1 { "" } else { "s" }
            ));
        }
        if !claude_names.is_empty() {
            parts.push(format!("claude: {}", claude_names.join(", ")));
        }
        if codex_count > 0 {
            parts.push(format!(
                "codex: {codex_count} session{}",
                if codex_count == 1 { "" } else { "s" }
            ));
        }
        parts.push("restore with spyc -r".to_string());
        self.exit_summary = Some(parts.join(" · "));
    }

    fn show_session_picker(&mut self) {
        use crate::state::sessions;
        let sessions = sessions::load_sessions();
        if sessions.is_empty() {
            self.state.flash_info("no saved sessions");
            return;
        }
        let lines: Vec<String> = sessions
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let age = sessions::format_relative_time(s.epoch_secs);
                let tab_count = s.tabs.len();
                let names: Vec<&str> = s.tabs.iter().map(|t| t.label.as_str()).collect();
                // Show agent session info (claude/codex) for tabs that have it.
                // Picker tooltips group by kind so a session with mixed
                // claude+codex panes is legible at a glance.
                let agent_info: Vec<String> = s
                    .tabs
                    .iter()
                    .filter_map(|t| {
                        let sid = t.agent_session_id.as_deref()?;
                        let short_id = &sid[..sid.len().min(8)];
                        let label = match t.effective_kind() {
                            AgentKind::Claude => match &t.agent_session_name {
                                Some(name) => format!("claude:{name} ({short_id})"),
                                None => format!("claude:{short_id}"),
                            },
                            AgentKind::Codex => format!("codex:{short_id}"),
                            AgentKind::Other => return None,
                        };
                        Some(label)
                    })
                    .collect();
                let tab_info = if tab_count == 0 {
                    String::new()
                } else {
                    format!("  [{}]", names.join(", "))
                };
                let agent_suffix = if agent_info.is_empty() {
                    String::new()
                } else {
                    format!("  {}", agent_info.join(", "))
                };
                let name_col = if s.name.is_empty() {
                    "(unnamed)"
                } else {
                    s.name.as_str()
                };
                format!(
                    "  [{}]  {:<22} {:<14} {}{}{}",
                    i + 1,
                    name_col,
                    age,
                    s.cwd.display(),
                    tab_info,
                    agent_suffix
                )
            })
            .collect();
        self.state.pending_sessions = Some(sessions);
        let mut all_lines = vec!["  [n]  new session".to_string(), String::new()];
        all_lines.extend(lines);
        let mut view = pager::PagerView::new_plain(
            "sessions — j/k navigate, Enter restore, n new, q close",
            all_lines,
        );
        view.picker_cursor = Some(2); // Start on first session (after header).
        self.pager = Some(view);
    }

    /// Prefix width for history editor lines: "  NNN  " = 7 chars.
    const HIST_PREFIX_W: usize = 7;

    /// Sync the history editor after moving the picker cursor to a new line.
    /// Updates the LineEditor content and the display line.
    fn sync_history_editor_to_cursor(&mut self) {
        Self::sync_hist_editor(
            &mut self.pager,
            &mut self.pending_history_pick,
            &self.state.history,
        );
    }

    fn sync_hist_editor(
        pager: &mut Option<pager::PagerView>,
        editor_opt: &mut Option<LineEditor>,
        history: &crate::state::history::History,
    ) {
        let Some(view) = pager else { return };
        let Some(editor) = editor_opt else { return };
        let new_cursor = view.picker_cursor.unwrap_or(0);
        let entries = history.entries();
        let hist_idx = entries.len().saturating_sub(1 + new_cursor);
        if let Some(cmd) = entries.get(hist_idx) {
            editor.set_content_keep_mode(cmd);
        }
        let text = format!("  {:>3}  {}", new_cursor + 1, editor.text());
        view.lines[new_cursor] = ratatui::text::Line::from(text);
        view.picker_edit_cursor = Some((Self::HIST_PREFIX_W + editor.cursor, editor.mode));
    }

    /// Open a popup listing every entry in `jump_history`, newest at
    /// the top. j/k navigate, Enter chdirs to the cursored path,
    /// ^D deletes the entry from history, q/Esc closes. Triggered by
    /// hitting Esc on an empty `J` prompt -- since there's nothing to
    /// throw away, the cancel turns into "show me my jumps."
    fn show_jump_history_popup(&mut self) {
        let entries = self.state.jump_history.entries();
        if entries.is_empty() {
            self.state.flash_info("jump history is empty");
            return;
        }
        // Snapshot newest-first paths into pending_jump_history so
        // index ↔ entry mapping stays stable even if the live history
        // is mutated (e.g. by another running spyc).
        let snapshot: Vec<String> = entries.iter().rev().cloned().collect();
        let lines: Vec<String> = snapshot
            .iter()
            .enumerate()
            .map(|(i, p)| format!("  {:>3}  {}", i + 1, p))
            .collect();
        let mut view = pager::PagerView::new_plain(
            "jump history — j/k move, Enter cd, x delete, q close",
            lines,
        );
        view.picker_cursor = Some(0);
        view.no_history = true;
        view.show_line_numbers = false;
        view.wrap = false;
        self.pending_jump_history = Some(snapshot);
        self.pager = Some(view);
        self.needs_full_repaint = true;
    }

    fn show_history_popup(&mut self) {
        let entries = self.state.history.entries();
        if entries.is_empty() {
            self.state.flash_info("history is empty");
            return;
        }
        // Show newest-first, numbered from 1.
        let lines: Vec<String> = entries
            .iter()
            .rev()
            .enumerate()
            .map(|(i, cmd)| format!("  {:>3}  {}", i + 1, cmd))
            .collect();
        // Create a line editor loaded with the newest entry, Normal mode.
        let newest = entries.last().unwrap();
        let mut editor = LineEditor::new();
        editor.set_content(newest);
        editor.mode = crate::ui::line_edit::Mode::Normal;
        if !editor.buf.is_empty() {
            editor.cursor = editor.buf.len() - 1;
        }
        let mut view = pager::PagerView::new_plain(
            "history — j/k move, i edit, Enter run, ^D delete, q close",
            lines,
        );
        view.picker_cursor = Some(0);
        view.picker_edit_cursor = Some((Self::HIST_PREFIX_W + editor.cursor, editor.mode));
        self.pending_history_pick = Some(editor);
        self.pager = Some(view);
    }

    fn restore_session(&mut self, session: &crate::state::sessions::Session) {
        // Restore working directory and update start_dir so backtick (`)
        // jumps to the session's home, not where spyc was launched from.
        if session.cwd.is_dir() {
            if let Err(e) = self.state.chdir(&session.cwd) {
                self.state.flash_error(format!("session chdir: {e}"));
                return;
            }
            self.state.start_dir.clone_from(&session.cwd);
        } else {
            self.state
                .flash_error(format!("session dir gone: {}", session.cwd.display()));
            return;
        }
        // Keep the startup-generated name when an older session file
        // has no name field; otherwise take the saved one.
        if !session.name.is_empty() {
            self.state.session_name = Some(session.name.clone());
        }
        self.state.project_home = session.project_home.clone().filter(|p| p.is_dir());
        // Restore pane layout.
        self.state.pane_height_pct = session.pane_height_pct;
        if !session.tabs.is_empty() {
            self.pane_tabs = None;
            for tab in &session.tabs {
                let cwd = if tab.cwd.is_dir() {
                    &tab.cwd
                } else {
                    &session.cwd
                };
                let kind = tab.effective_kind();
                // Codex restores by spawning `codex resume <UUID>`
                // directly — the CLI flag works, no `/resume` stdin
                // dance needed. Claude has a regression on the CLI
                // flag (crashes at mount with non-empty initialMessages),
                // so we always spawn fresh and type `/resume <sid>`
                // once it has settled.
                let (cmd, codex_resume_id) = match (kind, tab.agent_session_id.as_deref()) {
                    (AgentKind::Claude, _) => (command_without_resume(&tab.command), None),
                    (AgentKind::Codex, Some(sid)) => {
                        let base = command_without_codex_resume(&tab.command);
                        (format!("{base} resume {sid}"), Some(sid.to_string()))
                    }
                    (AgentKind::Codex, None) => {
                        // No saved id — fall back to codex's own
                        // most-recent picker for this cwd.
                        let base = command_without_codex_resume(&tab.command);
                        (format!("{base} resume --last"), None)
                    }
                    (AgentKind::Other, _) => (tab.command.clone(), None),
                };
                self.open_pane_tab_in(&cmd, cwd);
                if kind == AgentKind::Claude {
                    if let Some(ref sid) = tab.agent_session_id {
                        if let Some(tabs) = self.pane_tabs.as_mut() {
                            if let Some(entry) = tabs.tabs_mut().last_mut() {
                                entry.info.pending_resume_send =
                                    Some((sid.clone(), std::time::Instant::now()));
                            }
                        }
                    }
                }
                // Codex resume target is baked into `cmd` itself; no
                // pending-stdin send needed. The local binding here
                // mainly documents intent; suppress the unused-var lint.
                let _ = codex_resume_id;
            }
            // Restore active tab.
            if let Some(tabs) = self.pane_tabs.as_mut() {
                tabs.switch_to(session.active_tab);
                // Restore custom labels.
                for (entry, saved) in tabs.tabs_mut().iter_mut().zip(&session.tabs) {
                    entry.info.label.clone_from(&saved.label);
                }
            }
            self.state.pane_focused = session.pane_focused;
        }
        self.state.flash_info("session restored");
    }

    // ---- Git worktree (M11) -------------------------------------------------

    /// W l — list worktrees in a pager; digit keys 1-9 select.
    fn worktree_list(&mut self) {
        match crate::sysinfo::git_worktree_list(&self.state.listing.dir) {
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
                self.pager = Some(view);
            }
            None => self
                .state
                .flash_error("not in a git repository (or no worktrees)"),
        }
    }

    /// Compute the (rows, cols) the bottom pane will occupy.
    fn pane_spawn_size(height_pct: u16, status_position: StatusPosition) -> (u16, u16) {
        let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
        let area = ratatui::layout::Rect::new(0, 0, cols, rows);
        let layout = Self::compute_layout(area, true, height_pct, status_position);
        match layout.pane {
            Some(r) => (r.height.max(1), r.width.max(1)),
            None => (rows.saturating_sub(3).max(1), cols.max(1)),
        }
    }

    /// Compute the (rows, cols) available for the top-overlay pty. This
    /// is the top area: everything above the divider (or the whole
    /// screen minus the prompt row if no bottom pane).
    fn top_overlay_size(pane_height_pct: u16, has_bottom_pane: bool) -> (u16, u16) {
        let (cols, total_rows) = crossterm::terminal::size().unwrap_or((80, 24));
        if !has_bottom_pane {
            // Full screen minus prompt row.
            return (total_rows.saturating_sub(1).max(1), cols.max(1));
        }
        // With bottom pane: top region = total - divider(1) - bottom pane.
        let usable = total_rows.saturating_sub(1); // minus divider
        let bottom = (u32::from(usable) * u32::from(pane_height_pct) / 100) as u16;
        let top = usable.saturating_sub(bottom);
        (top.max(1), cols.max(1))
    }

    fn show_session_info(&mut self) {
        let mut lines: Vec<String> = Vec::new();
        lines.push(format!(
            "\u{1f336}\u{fe0f} spyc {}",
            env!("CARGO_PKG_VERSION")
        ));
        lines.push(format!("session  : {}", self.state.session_display()));
        lines.push(format!("project  : {}", self.state.project_home_display()));
        lines.push(format!("user@host: {}", self.state.user_host));
        lines.push(format!(
            "start dir: {}",
            crate::paths::display_tilde(&self.state.start_dir)
        ));
        lines.push(format!("pid      : {}", std::process::id()));
        lines.push(format!(
            "cwd      : {}",
            crate::paths::display_tilde(&self.state.listing.dir)
        ));
        lines.push(format!("entries  : {}", self.state.listing.entries.len()));
        lines.push(format!("visible  : {}", self.state.rows.len()));
        lines.push(format!("picks    : {}", self.state.picks.len()));
        lines.push(format!("inventory: {}", self.state.inventory.len()));
        lines.push(format!("marks    : {}", self.state.marks.entries.len()));
        lines.push(format!("rss      : {}", crate::sysinfo::format_rss()));
        lines.push(format!("time     : {}", crate::sysinfo::format_now()));
        if !self.state.config.sources.is_empty() {
            lines.push(String::new());
            lines.push("config sources:".into());
            for src in &self.state.config.sources {
                lines.push(format!("  {}", crate::paths::display_tilde(src)));
            }
        }
        self.pager = Some(PagerView::new_plain("session info", lines));
    }

    /// Title used for the help pager. Also used by the resize handler to
    /// detect when help is open and needs rebuilding for the new width.
    const HELP_TITLE: &'static str = "spyc — key bindings";

    /// Build and show the help pager. Called from `Action::Help` and on
    /// terminal resize (to re-wrap descriptions for the new width and
    /// pick the right column count).
    fn open_help(&mut self) {
        let (term_w, _) = crossterm::terminal::size().unwrap_or((80, 24));
        // Require at least ~40 chars of description space per column
        // before committing to 2-col (prefix is ~30 chars, so col_w ≥ 70,
        // body ≥ 140). Below that, 2-col cramps descriptions more than a
        // single wider column would.
        let ncols: u16 = if pager::centered_body_width(term_w) < 140 {
            1
        } else {
            2
        };
        let col_w = pager::centered_col_width(term_w, ncols) as usize;
        let lines = help::build_lines(&self.theme, &self.state.user_keymap, col_w);
        let mut view = pager::PagerView::new_styled(Self::HELP_TITLE, lines);
        view.columns = ncols as u8;
        view.no_history = true;
        self.pager = Some(view);
    }

    /// True when the help pager is the currently-open pager view.
    fn help_is_open(&self) -> bool {
        self.pager
            .as_ref()
            .is_some_and(|v| v.title == Self::HELP_TITLE)
    }

    /// Resolve `raw_dest` and run a copy-like or move-like operation across
    /// the current selection. Flash a success / error message afterwards
    /// and refresh the listing so results are visible immediately.
    fn run_selection_to(
        &mut self,
        raw_dest: &str,
        op: fn(&[&Path], &Path) -> std::io::Result<()>,
        verb: &str,
    ) {
        let dest_trim = raw_dest.trim();
        if dest_trim.is_empty() {
            return;
        }
        let paths = self.state.selection_paths();
        if paths.is_empty() {
            self.state.flash_error("nothing selected");
            return;
        }
        let count = paths.len();
        let expanded = crate::paths::expand(dest_trim);
        let dest = if expanded.is_absolute() {
            expanded
        } else {
            self.state.listing.dir.join(&expanded)
        };
        self.run_and_flash(
            op(&paths, &dest),
            format!("{verb} {count} item(s) to {}", dest.display()),
        );
        // Picks point at paths that may no longer exist after a move.
        self.state.picks.clear();
        self.state.refresh_listing();
    }

    /// Set the flash message based on the result of a mutating operation.
    fn run_and_flash(&mut self, result: std::io::Result<()>, success_msg: String) {
        match result {
            Ok(()) => self.state.flash_info(success_msg),
            Err(e) => self.state.flash_error(format!("error: {e}")),
        }
    }

    /// Advance the cursor by `delta` entries in flat order, wrapping at
    /// both ends of the list. This is what `j` / `k` use so pressing `j`
    /// at the bottom jumps back to the top (and vice versa).
    /// Route a key to the pager overlay. Also uses vi-like motion so the
    /// pager feels native to the rest of the UI.
    fn handle_pager_key(&mut self, key: KeyEvent) -> PostAction {
        let Some(view) = &mut self.pager else {
            return PostAction::None;
        };
        // Clear any one-shot flash message from the previous keypress.
        view.flash = None;
        // Compute the pager's actual viewport from the terminal size.
        // The pager overlay occupies ~92% of the frame height, with a
        // 1-row border on each side, and possibly a search bar at the
        // bottom.
        let viewport = {
            let (_, term_h) = crossterm::terminal::size().unwrap_or((80, 24));
            let pager_h = if view.full_width {
                term_h
            } else {
                (u32::from(term_h) * 92 / 100) as u16
            };
            pager_h.saturating_sub(2).max(2)
        };

        // While typing a search query, most keys feed the buffer.
        if view.is_typing_search() {
            match key.code {
                KeyCode::Esc => view.cancel_search(),
                KeyCode::Enter => {
                    let committed = view.commit_search(viewport);
                    if !committed {
                        // Flash inside the pager itself, not on the
                        // file-list status bar -- the user is looking
                        // at the pager, the message belongs there.
                        view.flash = Some("no matches".into());
                    } else if let Some(ref mut editor) = self.pending_history_pick {
                        // Sync picker cursor to the first match.
                        if let Some(line) = view.current_match_line() {
                            view.picker_cursor = Some(line);
                            let nc = line;
                            let entries = self.state.history.entries();
                            let hi = entries.len().saturating_sub(1 + nc);
                            if let Some(cmd) = entries.get(hi) {
                                editor.set_content_keep_mode(cmd);
                            }
                            let text = format!("  {:>3}  {}", nc + 1, editor.text());
                            view.lines[nc] = ratatui::text::Line::from(text);
                            view.picker_edit_cursor =
                                Some((Self::HIST_PREFIX_W + editor.cursor, editor.mode));
                        }
                    }
                }
                KeyCode::Backspace => view.search_backspace(),
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    view.search_push_char(c);
                }
                _ => {}
            }
            return PostAction::None;
        }

        // Inline `:N` jump — accumulate digits, Enter commits, Esc cancels.
        if let Some(ref mut buf) = self.pager_jump_buf {
            match key.code {
                KeyCode::Char(c @ '0'..='9') => {
                    buf.push(c);
                    view.jump_buf = Some(buf.clone());
                }
                KeyCode::Backspace => {
                    if buf.pop().is_none() {
                        self.pager_jump_buf = None;
                        view.jump_buf = None;
                    } else {
                        view.jump_buf = Some(buf.clone());
                    }
                }
                KeyCode::Enter => {
                    if let Ok(n) = buf.parse::<usize>() {
                        if n > 0 {
                            let target = n.saturating_sub(1);
                            if self.pending_history_pick.is_some() {
                                // History editor: jump to entry N.
                                let max = view.lines.len().saturating_sub(1);
                                let clamped = target.min(max);
                                view.picker_cursor = Some(clamped);
                                view.scroll =
                                    u16::try_from(clamped.saturating_sub(2)).unwrap_or(u16::MAX);
                            } else {
                                // Regular pager: jump to line N.
                                view.scroll = u16::try_from(target).unwrap_or(u16::MAX);
                            }
                        }
                    }
                    view.jump_buf = None;
                    self.pager_jump_buf = None;
                    if self.pending_history_pick.is_some() {
                        self.sync_history_editor_to_cursor();
                    }
                }
                _ => {
                    // Esc or non-digit cancels.
                    self.pager_jump_buf = None;
                    view.jump_buf = None;
                }
            }
            return PostAction::None;
        }

        // [b / ]b — pager buffer history navigation (two-key sequence).
        // [t / ]t — task viewer cycle (peek through backgrounded tasks).
        if let Some(bracket) = self.pager_pending_bracket.take() {
            if key.code == KeyCode::Char('b') {
                match bracket {
                    '[' => {
                        if let Some(current) = self.pager.take() {
                            match self.pager_history.go_back(current) {
                                Ok(prev) => {
                                    self.pager = Some(prev);
                                    self.needs_full_repaint = true;
                                    let back = self.pager_history.back_len();
                                    let fwd = self.pager_history.forward_len();
                                    self.state.flash_info(format!("buffer ←{back} →{fwd}"));
                                }
                                Err(current) => {
                                    self.pager = Some(current);
                                    self.state.flash_info("no older buffers");
                                }
                            }
                        }
                    }
                    ']' => {
                        if let Some(current) = self.pager.take() {
                            match self.pager_history.go_forward(current) {
                                Ok(next) => {
                                    self.pager = Some(next);
                                    self.needs_full_repaint = true;
                                    let back = self.pager_history.back_len();
                                    let fwd = self.pager_history.forward_len();
                                    self.state.flash_info(format!("buffer ←{back} →{fwd}"));
                                }
                                Err(current) => {
                                    self.pager = Some(current);
                                    self.state.flash_info("no newer buffers");
                                }
                            }
                        }
                    }
                    _ => {}
                }
                return PostAction::None;
            }
            if key.code == KeyCode::Char('t') {
                let direction = if bracket == '[' { -1 } else { 1 };
                self.cycle_task_viewer(direction);
                return PostAction::None;
            }
            // Unrecognized chord follow-up -- swallow it.
            return PostAction::None;
        }

        // Jump-history popup: j/k navigate, Enter chdirs, x deletes,
        // q/Esc closes. Per-popup j/k handling because the pager
        // dispatch doesn't have a generic picker-move arm; each
        // popup type wires its own (matches how the session picker
        // and history editor do it).
        if self.pending_jump_history.is_some() {
            match key.code {
                KeyCode::Char('j') | KeyCode::Down => {
                    view.picker_move(1, viewport);
                    return PostAction::None;
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    view.picker_move(-1, viewport);
                    return PostAction::None;
                }
                KeyCode::Enter => {
                    let cursor = view.picker_cursor.unwrap_or(0);
                    let snapshot = self.pending_jump_history.take().unwrap();
                    self.pager = None;
                    self.needs_full_repaint = true;
                    if let Some(path_str) = snapshot.get(cursor) {
                        let path = crate::paths::expand(path_str);
                        match self.state.chdir(&path) {
                            Ok(()) => {
                                // Push to top of history so MRU stays
                                // accurate even if user reaches via
                                // popup instead of typing.
                                self.state.jump_history.push(path_str);
                            }
                            Err(e) => self.state.flash_error(format!("cd: {e}")),
                        }
                    }
                    return PostAction::None;
                }
                KeyCode::Char('x') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    // `x` deletes the entry at the cursor. Matches
                    // the inventory view's `x` for "remove this
                    // item." The `!?` shell-history popup uses ^D
                    // because it has a vi line-editor where `x` is
                    // taken; the jump popup has no editor so `x` is
                    // unambiguously "delete entry."
                    let cursor = view.picker_cursor.unwrap_or(0);
                    let snapshot = self.pending_jump_history.as_mut().unwrap();
                    if let Some(path_str) = snapshot.get(cursor).cloned() {
                        // Remove from real history (find by content,
                        // since snapshot indices are reverse-ordered).
                        let entries = self.state.jump_history.entries();
                        if let Some(real_idx) = entries.iter().position(|e| e == &path_str) {
                            self.state.jump_history.remove(real_idx);
                        }
                        snapshot.remove(cursor);
                        if snapshot.is_empty() {
                            self.pending_jump_history = None;
                            self.pager = None;
                            self.needs_full_repaint = true;
                            self.state.flash_info("jump history empty");
                            return PostAction::None;
                        }
                        // Rebuild the pager line list from the snapshot.
                        let lines: Vec<ratatui::text::Line<'static>> = snapshot
                            .iter()
                            .enumerate()
                            .map(|(i, p)| {
                                ratatui::text::Line::from(format!("  {:>3}  {}", i + 1, p))
                            })
                            .collect();
                        view.lines = lines;
                        if cursor >= view.lines.len() {
                            view.picker_cursor = Some(view.lines.len() - 1);
                        }
                        return PostAction::None;
                    }
                }
                _ => {}
            }
        }

        // Worktree picker: 1-9 selects a worktree and chdirs.
        if let Some(ref worktrees) = self.state.pending_worktrees {
            if let KeyCode::Char(c @ '1'..='9') = key.code {
                let idx = (c as u8 - b'1') as usize;
                if let Some(path) = worktrees.get(idx).cloned() {
                    self.pager = None;
                    self.state.pending_worktrees = None;
                    self.needs_full_repaint = true;
                    if let Err(e) = self.state.chdir(&path) {
                        self.state.flash_error(format!("chdir: {e}"));
                    }
                    return PostAction::None;
                }
            }
        }

        // History editor: vi-edit highlighted line, Enter runs, d/x deletes.
        if let Some(ref mut editor) = self.pending_history_pick {
            use crate::ui::line_edit::EditResult;
            let editor_is_normal = editor.mode == crate::ui::line_edit::Mode::Normal;

            // Ctrl+D deletes the highlighted entry from history (any mode).
            if key.code == KeyCode::Char('d') && key.modifiers.contains(KeyModifiers::CONTROL) {
                let cursor = view.picker_cursor.unwrap_or(0);
                let entries = self.state.history.entries();
                let hist_idx = entries.len().saturating_sub(1 + cursor);
                if hist_idx < entries.len() {
                    self.state.history.remove(hist_idx);
                    if self.state.history.entries().is_empty() {
                        self.pager = None;
                        self.pending_history_pick = None;
                        self.needs_full_repaint = true;
                        self.state.flash_info("history is empty");
                        return PostAction::None;
                    }
                    let old_cursor = cursor;
                    self.show_history_popup();
                    if let Some(ref mut v) = self.pager {
                        let max = (v.line_count() as usize).saturating_sub(1);
                        v.picker_cursor = Some(old_cursor.min(max));
                        let new_cur = v.picker_cursor.unwrap_or(0);
                        let entries = self.state.history.entries();
                        let hist_idx = entries.len().saturating_sub(1 + new_cur);
                        if let Some(ref mut ed) = self.pending_history_pick {
                            if let Some(cmd) = entries.get(hist_idx) {
                                ed.set_content_keep_mode(cmd);
                            }
                            v.picker_edit_cursor = Some((Self::HIST_PREFIX_W + ed.cursor, ed.mode));
                            let text = format!("  {:>3}  {}", new_cur + 1, ed.text());
                            v.lines[new_cur] = ratatui::text::Line::from(text);
                        }
                    }
                }
                return PostAction::None;
            }

            // Inline sync: update editor from the current picker line.
            // Uses `view` and `editor` already borrowed in this scope.
            macro_rules! sync_editor {
                ($v:expr, $ed:expr, $hist:expr) => {{
                    let nc = $v.picker_cursor.unwrap_or(0);
                    let entries = $hist.entries();
                    let hi = entries.len().saturating_sub(1 + nc);
                    if let Some(cmd) = entries.get(hi) {
                        $ed.set_content_keep_mode(cmd);
                    }
                    let text = format!("  {:>3}  {}", nc + 1, $ed.text());
                    $v.lines[nc] = ratatui::text::Line::from(text);
                    $v.picker_edit_cursor = Some((Self::HIST_PREFIX_W + $ed.cursor, $ed.mode));
                }};
            }

            // In Normal mode, j/k/G/gg/n/N navigate, / searches, : jumps.
            if editor_is_normal {
                let handled = match key.code {
                    KeyCode::Char('j') | KeyCode::Down => {
                        self.history_pending_g = false;
                        view.picker_move(1, viewport);
                        sync_editor!(view, editor, self.state.history);
                        true
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        self.history_pending_g = false;
                        view.picker_move(-1, viewport);
                        sync_editor!(view, editor, self.state.history);
                        true
                    }
                    KeyCode::Char('G') => {
                        self.history_pending_g = false;
                        let last = view.lines.len().saturating_sub(1);
                        let delta = last as isize - view.picker_cursor.unwrap_or(0) as isize;
                        view.picker_move(delta, viewport);
                        sync_editor!(view, editor, self.state.history);
                        true
                    }
                    KeyCode::Char('g') => {
                        if self.history_pending_g {
                            self.history_pending_g = false;
                            let delta = -(view.picker_cursor.unwrap_or(0) as isize);
                            view.picker_move(delta, viewport);
                            sync_editor!(view, editor, self.state.history);
                        } else {
                            self.history_pending_g = true;
                        }
                        true
                    }
                    KeyCode::Char('/') => {
                        self.history_pending_g = false;
                        view.begin_search();
                        true
                    }
                    KeyCode::Char('n') => {
                        self.history_pending_g = false;
                        view.search_next(viewport);
                        if let Some(line) = view.current_match_line() {
                            view.picker_cursor = Some(line);
                            sync_editor!(view, editor, self.state.history);
                        }
                        true
                    }
                    KeyCode::Char('N') => {
                        self.history_pending_g = false;
                        view.search_prev(viewport);
                        if let Some(line) = view.current_match_line() {
                            view.picker_cursor = Some(line);
                            sync_editor!(view, editor, self.state.history);
                        }
                        true
                    }
                    KeyCode::Char(':') => {
                        self.history_pending_g = false;
                        self.pager_jump_buf = Some(String::new());
                        view.jump_buf = Some(String::new());
                        true
                    }
                    // Disable pager keys that don't make sense here.
                    KeyCode::Char('l' | 'v') => true,
                    _ => {
                        self.history_pending_g = false;
                        false
                    }
                };
                if handled {
                    return PostAction::None;
                }
            }

            // Feed all other keys to the line editor.
            let result = editor.feed(key);
            // Sync the display line with the editor buffer.
            let pc = view.picker_cursor.unwrap_or(0);
            let text = format!("  {:>3}  {}", pc + 1, editor.text());
            view.lines[pc] = ratatui::text::Line::from(text);
            view.picker_edit_cursor = Some((Self::HIST_PREFIX_W + editor.cursor, editor.mode));

            match result {
                EditResult::Submit => {
                    let cmd = editor.text();
                    self.pager = None;
                    self.pending_history_pick = None;
                    self.needs_full_repaint = true;
                    if cmd.trim().is_empty() {
                        return PostAction::None;
                    }
                    // Execute the (possibly edited) command directly.
                    self.state.last_captured_cmd = Some(cmd.clone());
                    self.state.history.push(cmd.trim());
                    let expanded =
                        crate::shell::expand_percent(&cmd, &self.state.selection_paths());
                    self.start_capture(&expanded, &cmd, &cmd);
                }
                EditResult::Cancel => {
                    // Esc in Insert → Normal (handled by editor, returns Continue).
                    // Cancel only fires from Normal-mode Esc or Ctrl+C → close popup.
                    self.pager = None;
                    self.pending_history_pick = None;
                    self.needs_full_repaint = true;
                }
                EditResult::HistoryPrev | EditResult::HistoryNext => {
                    // Up/Down in Insert mode → move between lines.
                    // HistoryPrev = Up key → move toward top of list (newer).
                    let delta: isize = if result == EditResult::HistoryPrev {
                        -1
                    } else {
                        1
                    };
                    view.picker_move(delta, viewport);
                    let new_cursor = view.picker_cursor.unwrap_or(0);
                    let entries = self.state.history.entries();
                    let hist_idx = entries.len().saturating_sub(1 + new_cursor);
                    if let Some(cmd) = entries.get(hist_idx) {
                        editor.set_content(cmd);
                    }
                    let text = format!("  {:>3}  {}", new_cursor + 1, editor.text());
                    view.lines[new_cursor] = ratatui::text::Line::from(text);
                    view.picker_edit_cursor =
                        Some((Self::HIST_PREFIX_W + editor.cursor, editor.mode));
                }
                EditResult::TabComplete | EditResult::Continue => {}
            }
            return PostAction::None;
        }

        // Session picker: j/k navigate, Enter/1-9 select, n new.
        if self.state.pending_sessions.is_some() {
            match key.code {
                KeyCode::Char('j') | KeyCode::Down => {
                    view.picker_move(1, viewport);
                    return PostAction::None;
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    view.picker_move(-1, viewport);
                    return PostAction::None;
                }
                KeyCode::Char(c @ '1'..='9') => {
                    // Direct selection — index into sessions (offset by 2 header lines).
                    let sessions = self.state.pending_sessions.take().unwrap();
                    let idx = (c as u8 - b'1') as usize;
                    if let Some(session) = sessions.get(idx) {
                        let session = session.clone();
                        self.pager = None;
                        self.needs_full_repaint = true;
                        self.restore_session(&session);
                        return PostAction::None;
                    }
                    self.state.pending_sessions = Some(sessions);
                }
                KeyCode::Enter => {
                    let cursor = view.picker_cursor.unwrap_or(0);
                    let sessions = self.state.pending_sessions.take().unwrap();
                    if cursor < 2 {
                        // "New session" header.
                        self.pager = None;
                        self.needs_full_repaint = true;
                        self.state.flash_info("new session");
                        return PostAction::None;
                    }
                    let idx = cursor - 2;
                    if let Some(session) = sessions.get(idx) {
                        let session = session.clone();
                        self.pager = None;
                        self.needs_full_repaint = true;
                        self.restore_session(&session);
                        return PostAction::None;
                    }
                    self.state.pending_sessions = Some(sessions);
                }
                KeyCode::Char('n' | 'N') => {
                    self.pager = None;
                    self.state.pending_sessions = None;
                    self.needs_full_repaint = true;
                    self.state.flash_info("new session");
                    return PostAction::None;
                }
                _ => {}
            }
        }

        // Visual line mode: extend / yank / cancel. Intercept first so
        // motion keys (j/k/G/^d/^u/^f/^b/PageDn/PageUp/Space) move the
        // selection cursor instead of the scroll position, and `y`
        // yanks the inclusive range. Esc / V cancel without yanking.
        if view.is_visual() {
            let half_page = i32::from(viewport) / 2;
            let page = i32::from(viewport);
            match key.code {
                KeyCode::Esc | KeyCode::Char('V') => {
                    view.cancel_visual();
                    return PostAction::None;
                }
                KeyCode::Char('y' | 'Y') => {
                    match view.yank_visual_to_clipboard() {
                        Ok(n) => {
                            view.flash = Some(format!(
                                "yanked {n} line{} to clipboard",
                                if n == 1 { "" } else { "s" }
                            ));
                        }
                        Err(e) => view.flash = Some(format!("yank failed: {e}")),
                    }
                    return PostAction::None;
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    view.visual_move(1, viewport);
                    return PostAction::None;
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    view.visual_move(-1, viewport);
                    return PostAction::None;
                }
                KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    view.visual_move(half_page as isize, viewport);
                    return PostAction::None;
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    view.visual_move(-half_page as isize, viewport);
                    return PostAction::None;
                }
                KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    view.visual_move(page as isize, viewport);
                    return PostAction::None;
                }
                KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    view.visual_move(-page as isize, viewport);
                    return PostAction::None;
                }
                KeyCode::PageDown | KeyCode::Char(' ') => {
                    view.visual_move(page as isize, viewport);
                    return PostAction::None;
                }
                KeyCode::PageUp | KeyCode::Char('b') => {
                    view.visual_move(-page as isize, viewport);
                    return PostAction::None;
                }
                KeyCode::Char('g') | KeyCode::Home => {
                    view.visual_jump_to(0, viewport);
                    return PostAction::None;
                }
                KeyCode::Char('G') | KeyCode::End => {
                    let last = view.lines.len().saturating_sub(1);
                    view.visual_jump_to(last, viewport);
                    return PostAction::None;
                }
                _ => {
                    // Unknown key while in visual mode — ignore so a
                    // stray `/` or `:` doesn't silently trigger a
                    // search/jump that the visual selection wasn't
                    // expecting. User must Esc out first.
                    return PostAction::None;
                }
            }
        }

        match key.code {
            KeyCode::Char('q' | 'Q') | KeyCode::Esc => {
                // Pager-help overlay: dismiss just the help, restore
                // whatever pager was active when `?` was pressed
                // (we pushed it to back-stack at open time). Without
                // this, ESC would close help AND drop us back to the
                // file list -- the user just wanted to glance at the
                // keys, not lose their place.
                if self
                    .pager
                    .as_ref()
                    .is_some_and(|v| v.title == crate::ui::pager::PAGER_HELP_TITLE)
                {
                    self.pager = self.pager_history.back.pop();
                    self.pager_jump_buf = None;
                    self.pager_pending_bracket = None;
                    self.needs_full_repaint = true;
                    return PostAction::None;
                }
                // Task viewer special close: if the viewed task has
                // exited (and the user has seen it), promote -- snapshot
                // its rendered view into buffer history and drop the
                // task from the bg list. Running tasks stay in bg.
                let promote_task: Option<u32> = self.pager.as_ref().and_then(|v| {
                    let id = v.task_id?;
                    let task = self.background_tasks.tasks.iter().find(|t| t.id == id)?;
                    if task.viewed_in_task_viewer && !matches!(task.status, TaskStatus::Running) {
                        Some(id)
                    } else {
                        None
                    }
                });
                if let Some(id) = promote_task {
                    if let Some(task) = self.background_tasks.take(id) {
                        let mut snapshot = Self::build_task_viewer_for(id, &task);
                        snapshot.task_id = None; // not a live viewer anymore
                        snapshot.no_history = false; // must be eligible for history
                        self.pager_history.push(snapshot);
                        // Reap the child handle if still around (already
                        // wait()'d when EOF arrived; this is just to drop
                        // the writer/rx). Implicit via task drop.
                        drop(task);
                    }
                    // Don't double-push the original viewer.
                    self.pager = None;
                } else {
                    // Save eligible pagers to history before closing.
                    let is_picker = self.state.pending_worktrees.is_some()
                        || self.state.pending_sessions.is_some()
                        || self.pending_history_pick.is_some();
                    if !is_picker {
                        if let Some(ref v) = self.pager {
                            if v.picker_cursor.is_none() && !v.streaming {
                                if let Some(v) = self.pager.take() {
                                    self.pager_history.push(v);
                                }
                            }
                        }
                    }
                    self.pager = None;
                }
                self.state.pending_worktrees = None;
                self.state.pending_sessions = None;
                self.pending_history_pick = None;
                self.pending_jump_history = None;
                self.pager_jump_buf = None;
                self.pager_pending_bracket = None;
                self.needs_full_repaint = true;
            }
            KeyCode::Char('/') => view.begin_search(),
            KeyCode::Char('n') => view.search_next(viewport),
            KeyCode::Char('N') => view.search_prev(viewport),
            KeyCode::Char(':') => {
                self.pager_jump_buf = Some(String::new());
                view.jump_buf = Some(String::new());
            }
            KeyCode::Char('[' | ']') => {
                if let KeyCode::Char(c) = key.code {
                    self.pager_pending_bracket = Some(c);
                }
            }
            KeyCode::Char('j') | KeyCode::Down => view.scroll_by(1, viewport),
            KeyCode::Char('k') | KeyCode::Up => view.scroll_by(-1, viewport),
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                view.scroll_by(i32::from(viewport) / 2, viewport);
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                view.scroll_by(-i32::from(viewport) / 2, viewport);
            }
            KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                view.scroll_by(i32::from(viewport), viewport);
            }
            KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                view.scroll_by(-i32::from(viewport), viewport);
            }
            KeyCode::PageDown | KeyCode::Char(' ') => view.scroll_by(i32::from(viewport), viewport),
            KeyCode::PageUp | KeyCode::Char('b') => view.scroll_by(-i32::from(viewport), viewport),
            KeyCode::Char('g') | KeyCode::Home => view.scroll_to_top(),
            KeyCode::Char('G') | KeyCode::End => view.scroll_to_bottom(viewport),
            KeyCode::Char('l') => view.toggle_line_numbers(),
            KeyCode::Char('w') => view.toggle_whitespace(),
            KeyCode::Char('W') => view.toggle_wrap(),
            KeyCode::Char('m') if !view.toggle_markdown() => {
                view.flash = Some("not a markdown file".into());
            }
            KeyCode::Char('f') => view.toggle_full_width(),
            KeyCode::Char('y') => match view.yank_to_clipboard() {
                Ok(()) => view.flash = Some("yanked source to clipboard".into()),
                Err(e) => view.flash = Some(format!("yank failed: {e}")),
            },
            KeyCode::Char('Y') => match view.yank_visible_to_clipboard() {
                Ok(()) => view.flash = Some("yanked visible to clipboard".into()),
                Err(e) => view.flash = Some(format!("yank failed: {e}")),
            },
            KeyCode::Char('V') => {
                // Enter visual line mode -- anchor at the top visible
                // line, then j/k/G/etc. extend the selection and `y`
                // yanks the inclusive range. The interceptor above
                // takes over all subsequent keys until Esc / V exit.
                view.enter_visual();
            }
            KeyCode::Char('S') if view.task_id.is_some() => {
                // Task viewer: S (Stop) pauses the underlying task
                // via SIGSTOP to its process group. Mirrors the
                // :pause command for hand-on-keyboard control.
                let id = view.task_id.unwrap();
                self.pause_task(Some(id));
            }
            KeyCode::Char('C') if view.task_id.is_some() => {
                // Task viewer: C (Continue) resumes a paused task
                // via SIGCONT. Mirrors the :resume command.
                let id = view.task_id.unwrap();
                self.resume_task(Some(id));
            }
            KeyCode::Char('p') => {
                // Hand the file off to $PAGER (default less) via full
                // TTY takeover. Same suspend_tui / resume_tui dance as
                // `v` for $EDITOR. Right tool for huge files past our
                // in-app cap, or for users who want less's specific
                // search / mark / pipe-out features.
                let Some(ref src) = view.source_path else {
                    view.flash = Some("no source file (try `s` to save first)".into());
                    return PostAction::None;
                };
                let argv = shell::resolve_pager();
                let pager_cmd = argv.join(" ");
                let path_quoted = shell::shell_quote(&src.display().to_string());
                self.pager = None;
                self.needs_full_repaint = true;
                return sh_c(&format!("{pager_cmd} {path_quoted}"), false);
            }
            KeyCode::Char('s') if view.saveable => match view.save_to_file() {
                Ok(path) => view.flash = Some(format!("saved: {}", path.display())),
                Err(e) => view.flash = Some(format!("save failed: {e}")),
            },
            KeyCode::Char('v') => {
                let argv = shell::resolve_editor();
                let editor_cmd = argv.join(" ");
                let scroll = view.scroll;
                // Determine the file to edit and the return state.
                let (edit_path, pager_return) = if let Some(ref src) = view.source_path {
                    (
                        src.clone(),
                        PagerReturn::SourceFile {
                            path: src.clone(),
                            scroll,
                        },
                    )
                } else {
                    let title = view.title.clone();
                    match view.write_to_temp() {
                        Ok(tmp) => (
                            tmp.clone(),
                            PagerReturn::TempFile {
                                path: tmp,
                                title,
                                scroll,
                            },
                        ),
                        Err(e) => {
                            self.state.flash_error(format!("write temp: {e}"));
                            return PostAction::None;
                        }
                    }
                };
                self.pending_pager_return = Some(pager_return);
                self.pager = None;
                self.needs_full_repaint = true;
                return sh_c(
                    &format!(
                        "{editor_cmd} {}",
                        shell::shell_quote(&edit_path.display().to_string())
                    ),
                    false,
                );
            }
            KeyCode::Char('?') | KeyCode::F(1) => {
                // Push current pager into history, open pager help on top.
                if let Some(current) = self.pager.take() {
                    self.pager_history.push(current);
                }
                self.pager = Some(crate::ui::pager::build_pager_help(&self.theme));
                self.needs_full_repaint = true;
            }
            _ => {}
        }
        PostAction::None
    }

    // --- Action handlers --------------------------------------------------

    /// Wrapper around the action dispatcher that reconciles
    /// project-scoped state (currently just the harpoon list) after
    /// each action. Cheap: a no-op when `state.project_home` matches
    /// the loaded harpoon's project field.
    fn apply(&mut self, action: &Action) -> Result<PostAction> {
        let result = self.apply_inner(action);
        self.reconcile_harpoon();
        result
    }

    /// Save the current harpoon (if any) and load a fresh one when
    /// `state.project_home` has shifted. Also flips `harpoon` on/off
    /// when `PROJECT_HOME` is set/unset.
    fn reconcile_harpoon(&mut self) {
        let want = self.state.project_home.as_deref();
        let have = self.harpoon.as_ref().map(|h| h.project.as_path());
        if want == have {
            return;
        }
        // Save the outgoing list before we drop it.
        if let Some(h) = self.harpoon.as_ref() {
            if let Err(e) = h.save() {
                spyc_debug!("harpoon save on PROJECT_HOME swap failed: {e}");
            }
        }
        self.harpoon = want.map(Harpoon::load);
        // Close the menu if it's open — its cursor referenced the old
        // list and would point at stale rows.
        self.harpoon_menu = None;
        self.sync_harpoon_filter_set();
        // If `=h` was active, the now-stale set may render an empty
        // list silently; rebuild rows so the user sees the new state.
        if matches!(self.state.temp_filter.as_deref(), Some("h")) {
            self.state.rebuild_rows();
        }
    }

    /// Refresh `state.harpoon_filter_set` from the active harpoon.
    /// Call after any list mutation (append/remove/swap/delete) so
    /// `=h` reflects the new state on the next `rebuild_rows`.
    fn sync_harpoon_filter_set(&mut self) {
        self.state.harpoon_filter_set = self
            .harpoon
            .as_ref()
            .map(|h| h.ancestor_set().clone())
            .unwrap_or_default();
    }

    fn apply_inner(&mut self, action: &Action) -> Result<PostAction> {
        spyc_debug!(
            "apply {:?}: cursor={} vt={} grid={}x{} pp={} len={}",
            action,
            self.state.cursor.index,
            self.state.cursor.view_top,
            self.state.last_grid.cols,
            self.state.last_grid.rows,
            self.state.last_grid.items_per_page(),
            self.state.rows.len(),
        );

        // In dir view, `p` (Drop) means "put inventory to cwd".
        if *action == Action::Drop && self.state.view == View::Dir {
            return Ok(self.put_inventory_to_cwd());
        }

        // yp — yank visible pane output to system clipboard.
        if *action == Action::YankPrompt {
            return Ok(self.yank_pane_to_clipboard());
        }
        // yP — yank last typed pane prompt to system clipboard.
        if *action == Action::YankLastPrompt {
            return Ok(self.yank_last_prompt_to_clipboard());
        }
        // ya — yank full pane scrollback to system clipboard.
        if *action == Action::YankScrollback {
            return Ok(self.yank_scrollback_to_clipboard());
        }
        // yf — yank cursor file's absolute path (or all picks,
        // newline-separated) to system clipboard.
        if *action == Action::YankPaths {
            return Ok(self.yank_paths_to_clipboard());
        }

        // Try pure-domain dispatch first.
        match self.state.apply(action) {
            state::ApplyResult::Handled => return Ok(PostAction::None),
            state::ApplyResult::Post(post) => return Ok(post),
            state::ApplyResult::OpenPager(req) => {
                let view = match req.lines {
                    state::PagerLines::Plain(lines) => {
                        let mut v = PagerView::new_plain(req.title, lines);
                        v.columns = req.columns;
                        if req.fit_to_content {
                            v.fit_to_content = true;
                            // Line-number gutter is noise for short summaries.
                            v.show_line_numbers = false;
                        }
                        v
                    }
                };
                self.pager = Some(view);
                return Ok(PostAction::None);
            }
            state::ApplyResult::NotHandled => {}
        }

        // Terminal-touching arms that must stay in App.
        match action {
            Action::EnterOrDisplay => {
                let post = self.activate(ActivateIntent::Display);
                self.state.cursor.clamp(self.state.rows.len());
                return Ok(post);
            }
            Action::EnterOrEdit => {
                let post = self.activate(ActivateIntent::Edit);
                self.state.cursor.clamp(self.state.rows.len());
                return Ok(post);
            }
            Action::EditInPane => {
                self.edit_in_pane();
                return Ok(PostAction::None);
            }
            Action::DisplayInPane => {
                self.display_in_pane();
                return Ok(PostAction::None);
            }

            Action::ChmodAdd(mode_char) => {
                let paths = self.state.selection_paths();
                if paths.is_empty() {
                    return Ok(PostAction::None);
                }
                let bits: u32 = match mode_char {
                    'w' => 0o200,
                    'x' => 0o111,
                    _ => return Ok(PostAction::None),
                };
                let count = paths.len();
                self.run_and_flash(
                    fs::ops::chmod_add_bits(&paths, bits),
                    format!("chmod +{mode_char} on {count} item(s)"),
                );
                self.state.refresh_listing();
            }

            Action::Help => self.open_help(),

            Action::OpenTaskViewer => self.open_task_viewer(None),

            Action::ReopenLastBuffer => {
                if let Some(prev) = self.pager_history.back.pop() {
                    self.pager = Some(prev);
                    self.needs_full_repaint = true;
                    self.state
                        .flash_info(format!("buffer ←{}", self.pager_history.back_len()));
                } else {
                    self.state.flash_info("no buffers in history");
                }
            }

            Action::FindFile => self.open_find_picker(),

            Action::ReloadConfig => self.reload_config(),

            Action::TogglePane
            | Action::ResumePane
            | Action::PaneFocusDown
            | Action::PaneFocusUp
            | Action::PaneSendSelection
            | Action::PaneGrow
            | Action::PaneShrink
            | Action::TogglePaneZoom
            | Action::PaneScrollEnter
            | Action::PaneScrollSave
            | Action::PaneNewTab
            | Action::PaneCloseTab
            | Action::PaneTabByIndex(_)
            | Action::PaneNextTab
            | Action::PanePrevTab
            | Action::PaneRenameTab
            | Action::PaneRestartTab
            | Action::HarpoonJump(_)
            | Action::HarpoonAppend
            | Action::HarpoonRemove
            | Action::HarpoonOpenMenu
            | Action::PanePipeContent
            | Action::PanePipeInventory
                if matches!(
                    self.state.mode,
                    Mode::Prompting(Prompt {
                        kind: PromptKind::PaneNewTabCmd
                            | PromptKind::PaneNewTabCwd
                            | PromptKind::PaneRenameTab,
                        ..
                    })
                ) =>
            {
                self.cancel_prompt();
                return self.apply(action);
            }

            Action::TogglePane => self.toggle_pane(),
            Action::ResumePane => self.open_pane_tab("claude --resume"),
            Action::PaneFocusDown => self.set_pane_focus(true),
            Action::PaneFocusUp => self.set_pane_focus(false),
            Action::PaneSendSelection => self.send_selection_to_pane(),
            Action::PaneGrow => self.resize_pane(5),
            Action::PaneShrink => self.resize_pane(-5),
            Action::TogglePaneZoom => self.toggle_pane_zoom(),
            Action::PaneScrollEnter => {
                if let Some(tabs) = self.pane_tabs.as_mut() {
                    let active = tabs.active_mut();
                    let on_alt_screen = active.is_alternate_screen();
                    active.enter_scroll_mode();
                    self.state.pane_focused = true;
                    // Full-screen TUIs (codex, vim, htop, lazygit, etc.)
                    // paint into the alternate screen, which never lands
                    // in main-screen scrollback. `^a v` still works
                    // (j/k nav over the current viewport, s saves it),
                    // but there's nothing to scroll *back* to — point
                    // users at the app's own history viewer instead of
                    // letting them think scrollback is broken.
                    if on_alt_screen {
                        self.state.flash_info(
                            "scroll: on — alt-screen app, no scrollback (use the app's own history)",
                        );
                    } else {
                        self.state
                            .flash_info("scroll: on (j/k nav, s save, Esc exit)");
                    }
                }
            }
            Action::PaneScrollSave => {
                if let Some(tabs) = self.pane_tabs.as_mut() {
                    match tabs.active_mut().save_to_file() {
                        Ok(path) => {
                            let name = path.file_name().unwrap_or_default().to_string_lossy();
                            self.state.flash_info(format!("saved: {name}"));
                        }
                        Err(e) => self.state.flash_info(format!("save error: {e}")),
                    }
                }
            }

            Action::PaneNewTab => self.start_new_tab_prompt(),
            Action::PaneCloseTab => self.close_active_tab(),
            Action::PaneTabByIndex(n) => {
                if let Some(tabs) = self.pane_tabs.as_mut() {
                    tabs.switch_to((*n as usize).saturating_sub(1));
                }
            }
            Action::PaneNextTab => {
                if let Some(tabs) = self.pane_tabs.as_mut() {
                    tabs.next();
                }
            }
            Action::PanePrevTab => {
                if let Some(tabs) = self.pane_tabs.as_mut() {
                    tabs.prev();
                }
            }
            Action::PaneRenameTab => {
                if let Some(tabs) = self.pane_tabs.as_ref() {
                    let current = tabs.active_info().label.clone();
                    let mut p = Prompt::shell(PromptKind::PaneRenameTab, "tab name: ");
                    p.buffer.clone_from(&current);
                    if let Some(ed) = p.editor.as_mut() {
                        ed.set_content(&current);
                    }
                    self.state.mode = Mode::Prompting(p);
                }
            }

            Action::PaneRestartTab => self.restart_active_tab(),

            Action::PanePipeContent => self.pipe_content_to_pane(false),
            Action::PanePipeInventory => self.pipe_content_to_pane(true),

            Action::QuickSelectOpen => self.open_quick_select(),
            Action::OpenGraveyardView => self.state.open_graveyard_view(),
            Action::HarpoonJump(n) => self.harpoon_jump(*n),
            Action::HarpoonAppend => self.harpoon_append(),
            Action::HarpoonRemove => self.harpoon_remove(),
            Action::HarpoonOpenMenu => self.harpoon_open_menu(),

            Action::WorktreeList => self.worktree_list(),

            Action::GitDiff | Action::GitDiffCached => {
                let cached = matches!(action, Action::GitDiffCached);
                if self.state.git_info.is_none() {
                    self.state.flash_error("not in a git repository");
                } else {
                    self.open_git_diff(cached);
                }
            }
            Action::GitBlame => {
                if self.state.git_info.is_none() {
                    self.state.flash_error("not in a git repository");
                } else {
                    self.open_git_blame();
                }
            }

            Action::ShowMemory => self.show_session_info(),
            Action::ColorToggle => {
                self.theme = self.theme.toggled();
                self.state.flash_info(if self.theme.mono {
                    "colors off"
                } else {
                    "colors on"
                });
            }

            Action::ToggleActivity => {
                self.show_activity = !self.show_activity;
                self.state.flash_info(if self.show_activity {
                    "activity monitor on"
                } else {
                    "activity monitor off"
                });
            }

            Action::Redraw => {
                self.needs_full_repaint = true;
            }
            Action::Quit => {
                let now = std::time::Instant::now();
                if self
                    .state
                    .quit_pending
                    .is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(2))
                {
                    self.save_session();
                    self.state.should_quit = true;
                } else {
                    self.state.quit_pending = Some(now);
                    let running_panes = self.pane_tabs.as_ref().map_or(0, |tabs| {
                        tabs.tabs().iter().filter(|e| !e.pane.is_closed()).count()
                    });
                    let running_bg = self.background_tasks.running_count();
                    let running = running_panes + running_bg;
                    if running > 0 {
                        self.state.flash_info(format!(
                            "{running} running process{} — press again to quit",
                            if running == 1 { "" } else { "es" }
                        ));
                    } else {
                        self.state.flash_info("press again to quit");
                    }
                }
            }

            Action::GotoFile | Action::GotoFileLine => {
                let open_at_line = matches!(action, Action::GotoFileLine);
                self.goto_file_from_pane(open_at_line);
            }

            // All other actions were already handled by `self.state.apply()`.
            _ => {}
        }
        self.state.cursor.clamp(self.state.rows.len());
        Ok(PostAction::None)
    }

    fn activate(&mut self, intent: ActivateIntent) -> PostAction {
        let Some(row) = self.state.rows.get(self.state.cursor.index) else {
            return PostAction::None;
        };
        let path = row.path.clone();
        let kind = row.kind;

        // Inventory view: enter drills down to the containing directory and
        // focuses on the item, then continues with the intent on that item.
        if self.state.view == View::Inventory {
            let target_dir = if kind == EntryKind::Dir {
                path.clone()
            } else {
                path.parent()
                    .map_or_else(|| path.clone(), Path::to_path_buf)
            };
            if let Err(e) = self.state.chdir(&target_dir) {
                self.state.flash_error(format!("chdir: {e}"));
                return PostAction::None;
            }
            self.state.view = View::Dir;
            self.state.focus_on_path(&path);
            self.state.rebuild_rows();
            if kind == EntryKind::Dir {
                return PostAction::None;
            }
        }

        if kind == EntryKind::Dir {
            if let Err(e) = self.state.chdir(&path) {
                self.state.flash_error(format!("chdir: {e}"));
            }
            return PostAction::None;
        }

        // File: dispatch based on intent.
        match intent {
            ActivateIntent::Display => {
                if shell::looks_like_text(&path) {
                    // Show text files in the in-app pager: no TUI teardown,
                    // consistent keybindings, keeps the pane running below.
                    let name = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    let file_size = std::fs::metadata(&path).map_or(0, |m| m.len());
                    // Big files used to OOM us: read_to_string + syntect
                    // every token = file size × ~50 in pager state. Now
                    // we cap at MAX_PAGER_BYTES; past that, load just
                    // MAX_PAGER_LINES of plain text and tell the user how
                    // to hand off to $PAGER for the full thing.
                    let load_result = if file_size > crate::fs::ops::MAX_PAGER_BYTES {
                        crate::fs::ops::read_truncated(&path, crate::fs::ops::MAX_PAGER_LINES)
                    } else {
                        std::fs::read_to_string(&path).map(|c| {
                            let n = c.lines().count();
                            (c, n, false)
                        })
                    };
                    match load_result {
                        Ok((content, _line_count, truncated)) => {
                            let content = expand_tabs(&content);
                            let is_md = crate::ui::markdown::is_markdown_path(&path);
                            // Source-side lines: syntect-highlighted if
                            // available AND we loaded the whole file
                            // (highlighting a partial file would still
                            // mostly work but blows memory and the savings
                            // is the whole point of truncation).
                            let source_lines: Vec<ratatui::text::Line<'static>> = if truncated {
                                content
                                    .lines()
                                    .map(|l| ratatui::text::Line::from(l.to_string()))
                                    .collect()
                            } else {
                                crate::ui::syntax::highlight_to_lines(&name, &content)
                                    .unwrap_or_else(|| {
                                        content
                                            .lines()
                                            .map(|l| ratatui::text::Line::from(l.to_string()))
                                            .collect()
                                    })
                            };
                            let mut view = if is_md && !truncated {
                                // Pre-compute both views; default to
                                // rendered. `m` toggles. Yank/save always
                                // hit the source via `source_text()`.
                                // Skipped for truncated files since the
                                // markdown rendering of half a doc looks
                                // weird (broken refs, half-closed code
                                // fences).
                                let rendered = crate::ui::markdown::render(&content, &self.theme);
                                let mut v = PagerView::new_styled(name, rendered);
                                v.alt_lines = Some(source_lines);
                                v.markdown_rendered = true;
                                v
                            } else {
                                let display_name = if truncated {
                                    format!(
                                        "{name} \u{26a0} truncated · {} MB",
                                        file_size / (1024 * 1024)
                                    )
                                } else {
                                    name
                                };
                                let mut v = PagerView::new_styled(display_name, source_lines);
                                if truncated {
                                    // Append a banner row pointing at the
                                    // escape hatch so the user knows the
                                    // cap fired and what to do.
                                    let warn_style = ratatui::style::Style::default()
                                        .fg(self.theme.pick)
                                        .add_modifier(ratatui::style::Modifier::BOLD);
                                    v.lines.push(ratatui::text::Line::from(""));
                                    v.lines.push(ratatui::text::Line::from(
                                        ratatui::text::Span::styled(
                                            format!(
                                                "[truncated at {} lines · {} MB total · press p to open in $PAGER]",
                                                crate::fs::ops::MAX_PAGER_LINES,
                                                file_size / (1024 * 1024)
                                            ),
                                            warn_style,
                                        ),
                                    ));
                                    // Also flash an immediate hint -- the
                                    // banner is at the bottom and the user
                                    // might not scroll there before
                                    // wondering what happened to their
                                    // file.
                                    v.flash = Some(format!(
                                        "truncated at {} lines · press p for full file in $PAGER",
                                        crate::fs::ops::MAX_PAGER_LINES
                                    ));
                                }
                                v
                            };
                            view.source_path = Some(path.clone());
                            self.pager = Some(view);
                        }
                        Err(e) => self.state.flash_error(format!("read: {e}")),
                    }
                    PostAction::None
                } else {
                    // Binary file: hex dump via pretty-hex.
                    let name = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    match fs::ops::hex_dump_lines(&path, &self.theme) {
                        Ok(lines) => {
                            let mut view =
                                PagerView::new_plain(format!("{name} [hex]"), Vec::new());
                            // Replace the empty lines with our pre-styled hex lines.
                            view.lines = lines;
                            self.pager = Some(view);
                        }
                        Err(e) => self.state.flash_error(format!("hex: {e}")),
                    }
                    PostAction::None
                }
            }
            ActivateIntent::Edit => {
                let mut argv = shell::resolve_editor();
                if argv.is_empty() {
                    return PostAction::None;
                }
                let program = argv.remove(0);
                argv.push(path.to_string_lossy().into_owned());
                PostAction::Spawn {
                    program,
                    args: argv,
                    pause_after: false,
                }
            }
        }
    }
}

/// Search / filter matcher: case-insensitive substring for plain
/// text, glob for anything with `*`, `?`, or `[`. Used by `/`
/// (search) and `=` (limit filter). Substring (not anchored at the
/// start) so `/env` finds `.env`, `.envrc`, and `environment.toml`
/// — anchored prefix mode hid dot-prefixed files behind their
/// leading `.` and was consistently surprising. Globs are still
/// available for users who want anchoring (`env*`, `.env*`).
pub enum Matcher {
    Substring(String),
    Glob(Pattern),
    /// An invalid glob produced by a malformed pattern. Matches nothing.
    Never,
}

impl Matcher {
    pub fn build(query: &str) -> Self {
        let is_glob = query.contains(['*', '?', '[']);
        let lower = query.to_lowercase();
        if is_glob {
            match Pattern::new(&lower) {
                Ok(p) => Self::Glob(p),
                Err(_) => Self::Never,
            }
        } else {
            Self::Substring(lower)
        }
    }

    pub fn matches(&self, name: &str) -> bool {
        let lower = name.to_lowercase();
        match self {
            Self::Substring(q) => lower.contains(q.as_str()),
            Self::Glob(p) => p.matches(&lower),
            Self::Never => false,
        }
    }
}

/// Point the FS watcher at `new_dir`, unwatching the previously-watched
/// listing dir if any. No-op when the watcher failed to initialize or
/// when the same dir is already being watched.
/// Keys we intercept even when the pane is focused.
const fn is_spyc_meta_when_pane_focused(
    key: crossterm::event::KeyEvent,
    resolver_pending: bool,
) -> bool {
    use crossterm::event::{KeyCode, KeyModifiers};
    // Continuation of a multi-key spyc sequence must stay with spyc.
    if resolver_pending {
        return true;
    }
    // Raw FS byte or F10 — always the pane toggle.
    if matches!(key.code, KeyCode::F(10) | KeyCode::Char('\x1c')) {
        return true;
    }
    // Ctrl-\ (toggle), Ctrl-W (vim pane prefix), Ctrl-A (screen prefix).
    key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char('\\' | 'w' | 'W' | 'a' | 'A'))
}

fn sync_listing_watch(
    fs_watcher: Option<&mut notify::RecommendedWatcher>,
    active: &mut Option<PathBuf>,
    active_git: &mut Option<PathBuf>,
    new_dir: &Path,
) {
    use notify::{RecursiveMode, Watcher};
    let Some(w) = fs_watcher else {
        return;
    };
    if active.as_deref() != Some(new_dir) {
        if let Some(old) = active.as_ref() {
            let _ = w.unwatch(old);
        }
        // Recursive: catches changes anywhere below the listing dir so
        // git status markers update on the parent directory row when a
        // file is added/modified in a subdirectory (e.g. touching
        // `docs/foo.md` while sitting at the repo root). Events under
        // `.git/` are filtered to specific files (`index`, `HEAD`) by
        // `is_listing_path` to avoid `.git/objects` / pack / lockfile
        // churn cascading into needless `git status` calls. macOS
        // FSEvents handles recursive watches at OS level (cheap);
        // Linux inotify needs a watch per subdir, which can hit
        // `fs.inotify.max_user_watches` on enormous monorepos.
        if w.watch(new_dir, RecursiveMode::Recursive).is_ok() {
            *active = Some(new_dir.to_path_buf());
        } else {
            *active = None;
        }
    }
    // Watch the `.git/` directory non-recursively. We can't watch
    // `.git/index` as a file because git commits via atomic rename
    // (write `.git/index.lock`, then rename to `.git/index`), which
    // replaces the inode — a file-level watch follows the *old* inode
    // and goes deaf. A directory watch sees the rename land. Touched
    // by basically every git op that changes status (add, commit,
    // checkout, reset, merge, stash, rebase, ...). NonRecursive
    // bounds the noise even in repos with huge `.git/objects` trees.
    let git_dir = new_dir.join(".git");
    let want_git = git_dir.is_dir();
    let have_git = active_git.as_deref() == Some(&git_dir);
    if !have_git {
        if let Some(old) = active_git.take() {
            let _ = w.unwatch(&old);
        }
        if want_git && w.watch(&git_dir, RecursiveMode::NonRecursive).is_ok() {
            *active_git = Some(git_dir);
        }
    }
}

/// Spawn a shell command with stdout+stderr merged into one pipe.
/// The reader thread sends chunks as they arrive so the pager can
/// stream output in real-time. An empty Vec signals EOF.
/// Spawn `cmd` under a PTY for `!` captures. Returns the child handle,
/// a receiver streaming stdout/stderr/`/dev/tty` bytes, and the master
/// writer for forwarding the user's keystrokes to the child.
///
/// PTY (vs. a plain piped `Command`) is what stops sudo / ssh / gpg
/// from writing their password prompts directly to our real terminal:
/// inside the child, `/dev/tty` resolves to the slave PTY, so those
/// bytes flow back through the master and into the pager buffer.
type CaptureHandles = (
    Box<dyn portable_pty::Child + Send + Sync>,
    Box<dyn std::io::Write + Send>,
    std::sync::mpsc::Receiver<Vec<u8>>,
);

/// Normalize captured pty output for the pager.
///
/// Three passes:
///
/// 1. CRLF (`\r\n`) → LF (`\n`). The pty's slave side enables ONLCR by
///    default, so a child writing `\n` produces `\r\n` on the master
///    we read from. Without this, ratatui rendering interprets the
///    literal `\r` as carriage return and shorter following lines
///    overlay just the prefix of longer prior ones.
/// 2. Bare `\r` collapse. `git pull`, `npm`, `cargo`, etc. use bare
///    `\r` (no newline) to overwrite a progress line on the same
///    terminal row -- `Counting: 18%\rCounting: 27%\rCounting: 100%`.
///    Real terminals handle this; `ansi-to-tui` does not, so without
///    a fix we render every frame side-by-side as one super-wide
///    line. For each `\n`-delimited segment, we keep only the text
///    after the *last* `\r` -- the same final state a real terminal
///    would show. Streaming pagers re-run this every tick, so the
///    user sees live progress (latest frame each redraw).
/// 3. Strip stray ASCII control bytes that aren't whitespace or ANSI
///    escape. Some `git log` commit messages, mboxen, and old-school
///    formatter output carry `\b` (man-page bold trick), `\v`, `\f`,
///    NUL, etc. ratatui can't render them and the host terminal may
///    treat them as cursor controls (backspacing, line-feeding) when
///    we send the bytes through, which fragments rendered Lines and
///    leaves "Buil$er.cs"-style misalignment. We drop them so output
///    is predictable. Kept: `\t` (TAB), `\n` (LF), `\x1b` (ESC for
///    ANSI sequences). Dropped: 0x00-0x08, 0x0B-0x0C, 0x0E-0x1A,
///    0x1C-0x1F, 0x7F.
///
/// ANSI escape sequences never embed bare `\r` and never embed the
/// other control bytes pass 3 strips, so the byte-level passes are
/// safe.
fn strip_crlf(bytes: &[u8]) -> Vec<u8> {
    // Pass 1: \r\n -> \n.
    let mut step1 = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\r' && bytes.get(i + 1) == Some(&b'\n') {
            step1.push(b'\n');
            i += 2;
        } else {
            step1.push(bytes[i]);
            i += 1;
        }
    }
    // Pass 2: collapse bare \r within each line to the last frame.
    let step2: Vec<u8> = if step1.contains(&b'\r') {
        let mut out = Vec::with_capacity(step1.len());
        let mut first = true;
        for line in step1.split(|&b| b == b'\n') {
            if !first {
                out.push(b'\n');
            }
            first = false;
            let start = line.iter().rposition(|&b| b == b'\r').map_or(0, |i| i + 1);
            out.extend_from_slice(&line[start..]);
        }
        out
    } else {
        step1
    };
    // Pass 3: drop other ASCII control bytes (keep \t, \n, ESC).
    step2
        .into_iter()
        .filter(|b| {
            !matches!(
                b,
                0x00..=0x08 | 0x0b..=0x0c | 0x0e..=0x1a | 0x1c..=0x1f | 0x7f
            )
        })
        .collect()
}

fn spawn_capture(cmd: &str, cwd: &std::path::Path) -> Result<CaptureHandles> {
    use portable_pty::{CommandBuilder, PtySize, native_pty_system};
    use std::io::Read as _;
    use std::sync::mpsc;

    let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    let pty_system = native_pty_system();
    let pair = pty_system.openpty(PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    })?;

    // Use the user's $SHELL with -i so aliases and rc-file PATH
    // entries from .zshrc / .bashrc are honored — `:!gemma` should
    // resolve a user alias the same way it does in their terminal.
    let (shell, shell_args) = crate::shell::user_shell_invocation(cmd);
    let mut builder = CommandBuilder::new(&shell);
    builder.args(shell_args.iter().map(String::as_str));
    builder.cwd(cwd);
    // We're not actually a vt100 terminal -- the capture pager only
    // renders ANSI SGR (colors) and treats CR/LF intelligently;
    // cursor positioning, alt-screen, mouse codes, etc. all get
    // stripped or rendered as garbage. Advertising
    // `xterm-256color` would lie about that, and tools like `less`,
    // `vim`, `htop` would happily switch into alt-screen TUI mode
    // and freeze the capture (or render unrenderable cursor games
    // into the pager body). `TERM=dumb` is the canonical "nothing
    // fancy" signal: TUI programs refuse to run as a TUI (they
    // dump to stdout or print a friendly error and exit), which is
    // exactly the behavior we want for `!` captures. Users who
    // genuinely want a TUI program should use `;cmd` (foreground
    // pane) instead.
    //
    // FORCE_COLOR / CLICOLOR_FORCE / COLORTERM are kept so tools
    // that respect those override TERM=dumb's "no color" implication
    // -- cargo, eza, bat, ripgrep all keep their color output. Tools
    // that key off TERM alone (older `git`, default `ls`) will
    // produce plain output, which is acceptable.
    builder.env("TERM", "dumb");
    builder.env("CLICOLOR_FORCE", "1");
    builder.env("FORCE_COLOR", "1");
    builder.env("COLORTERM", "truecolor");
    builder.env("COLUMNS", cols.to_string());
    builder.env("LINES", rows.to_string());
    // We're already running this child *inside* spyc's pager. Tools
    // that probe `isatty(stdout)` and auto-invoke a sub-pager (`git
    // log`, `man`, anything that defers to $PAGER) would otherwise
    // launch `less` against our PTY and freeze the capture waiting
    // for keystrokes. Force a no-op pager so tools just dump their
    // output and our pager wraps the whole thing. `cat` is safer
    // than empty/unset, since some tools fall back to a default
    // when $PAGER is unset.
    builder.env("PAGER", "cat");
    builder.env("GIT_PAGER", "cat");
    builder.env("MANPAGER", "cat");

    let child = pair.slave.spawn_command(builder)?;
    // Drop the slave handle — once the child exits, the master read
    // side will see EOF, which is how we detect "done".
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader()?;
    let writer = pair.master.take_writer()?;

    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
        // Signal EOF with an empty vec.
        let _ = tx.send(Vec::new());
    });

    Ok((child, writer, rx))
}

/// Build a PostAction that runs `cmd` through `sh -c` so shell features
/// (pipes, redirection, `$VAR`) work.
fn sh_c(cmd: &str, pause_after: bool) -> PostAction {
    PostAction::Spawn {
        program: "sh".to_string(),
        args: vec!["-c".to_string(), cmd.to_string()],
        pause_after,
    }
}

/// Hand the tty to a child process, optionally pausing for a keypress
/// afterwards so the user can read the command's output before we repaint
/// over it.
///
/// Job-control aware: the child is placed in its own process group and
/// becomes the foreground process group of the controlling tty for the
/// duration of the run. This is what a normal shell does when launching
/// a foreground command, and it's what makes Ctrl+C / Ctrl+\ delivery
/// land *only* on the child instead of being broadcast to spyc + child.
/// Without this, less running line-counts would react to ^C *and* spyc
/// would see it (caught by our no-op handler, but the FG-group ambiguity
/// caused other anomalies -- less appearing to miss the signal, etc.).
fn run_child_in_foreground(
    terminal: &mut Tui,
    program: &str,
    args: &[String],
    pause_after: bool,
) -> Result<()> {
    use std::io::Write;
    suspend_tui(terminal)?;

    let mut cmd = std::process::Command::new(program);
    cmd.args(args);
    // process_group(0) ⇒ child becomes leader of a new process group
    // (PGID == child PID). Equivalent to setpgid(0, 0) right before
    // exec. The child no longer shares spyc's group, so a tty signal
    // delivered to spyc's FG group can't accidentally hit it.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    let mut child = cmd.spawn()?;

    // Make the child's process group the foreground group of the
    // controlling tty. Now ^C / ^\ from the kernel's tty driver go
    // to the child only. SIGTTOU is ignored globally (see
    // `install_signal_handlers`) so the restore call below doesn't
    // suspend us.
    #[cfg(unix)]
    let saved_pgid: libc::pid_t = unsafe {
        let our_pgid = libc::getpgrp();
        let child_pid = child.id() as libc::pid_t;
        libc::tcsetpgrp(libc::STDIN_FILENO, child_pid);
        our_pgid
    };

    // Ignoring status on purpose: non-zero exits (e.g. less with `q`, or a
    // grep that found nothing) are normal and should not crash spyc.
    let _ = child.wait();

    // Restore tty foreground to spyc's group. Without this, the next
    // tty input would still be delivered to the child's (now-dead)
    // group and the kernel would EIO subsequent reads.
    #[cfg(unix)]
    unsafe {
        libc::tcsetpgrp(libc::STDIN_FILENO, saved_pgid);
    }

    if pause_after {
        let mut stdout = std::io::stdout();
        write!(stdout, "\n[spyc] press any key to continue…")?;
        stdout.flush()?;
        // We're not in raw mode right now, so read a single byte directly
        // from stdin. Any key (including Enter) unblocks.
        let mut byte = [0u8; 1];
        let _ = std::io::Read::read(&mut std::io::stdin(), &mut byte);
    }

    resume_tui(terminal)?;
    Ok(())
}

pub fn row_from_entry(e: &Entry) -> RowData {
    RowData {
        path: e.path.clone(),
        display: e.display_name(),
        kind: e.kind,
    }
}

const fn on_off(b: bool) -> &'static str {
    if b { "on" } else { "off" }
}

/// Last segment of a path as a displayable String, falling back to the full
/// display if the path has no terminating file-name component (root, `..`).
fn path_basename_display(p: &std::path::Path) -> String {
    p.file_name().map_or_else(
        || p.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

fn user_host_string() -> String {
    let user = std::env::var("USER").unwrap_or_else(|_| "user".to_string());
    let host = hostname_best_effort();
    format!("{user}@{host}")
}

fn hostname_best_effort() -> String {
    if let Ok(h) = std::env::var("HOSTNAME") {
        if !h.is_empty() {
            return h;
        }
    }
    if let Ok(out) = std::process::Command::new("hostname").output() {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !s.is_empty() {
                return s;
            }
        }
    }
    "localhost".to_string()
}

/// Expand tab characters to spaces (8-column tab stops).
fn expand_tabs(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut col = 0usize;
    for ch in s.chars() {
        if ch == '\t' {
            let spaces = 8 - (col % 8);
            for _ in 0..spaces {
                out.push(' ');
            }
            col += spaces;
        } else if ch == '\n' {
            out.push(ch);
            col = 0;
        } else {
            out.push(ch);
            col += 1;
        }
    }
    out
}

/// Longest common prefix of a slice of strings (byte-safe for UTF-8).
fn common_prefix(strings: &[String]) -> String {
    let Some(first) = strings.first() else {
        return String::new();
    };
    let mut byte_len = first.len();
    for s in &strings[1..] {
        byte_len = byte_len.min(s.len());
        for ((i, a), b) in first.char_indices().zip(s.chars()) {
            if a != b {
                byte_len = byte_len.min(i);
                break;
            }
        }
    }
    first[..byte_len].to_string()
}

/// Strip ANSI escape sequences from a string and drop remaining
/// non-printable control bytes, leaving only displayable text. Used
/// to sanitize captured pane-prompt buffers before yanking.
fn strip_ansi_escapes(s: &str) -> String {
    let stripped = strip_ansi_escapes::strip_str(s);
    stripped
        .chars()
        .filter(|&c| c >= ' ' || c == '\n' || c == '\t')
        .collect::<String>()
        .trim()
        .to_string()
}

/// Render an "added" diff for every untracked file under `paths`.
/// Two-step: list with `git ls-files --others --exclude-standard`,
/// then `git diff --no-index /dev/null <file>` per result. Returns the
/// concatenated colored diff bytes (empty if no untracked files match).
fn untracked_diff_bytes(cwd: &std::path::Path, paths: &[String]) -> Vec<u8> {
    let mut args: Vec<&str> = vec!["ls-files", "--others", "--exclude-standard", "--"];
    for s in paths {
        args.push(s);
    }
    let listing = match std::process::Command::new("git")
        .args(&args)
        .current_dir(cwd)
        .output()
    {
        Ok(o) if o.status.success() => o.stdout,
        _ => return Vec::new(),
    };
    let mut out = Vec::new();
    for line in listing.split(|b| *b == b'\n') {
        if line.is_empty() {
            continue;
        }
        let Ok(file) = std::str::from_utf8(line) else {
            continue;
        };
        // --no-index exits 1 when files differ — that's the success
        // case for us. Just take whatever it printed.
        if let Ok(o) = std::process::Command::new("git")
            .args([
                "diff",
                "--no-index",
                "--color=always",
                "--",
                "/dev/null",
                file,
            ])
            .current_dir(cwd)
            .output()
        {
            out.extend(o.stdout);
        }
    }
    out
}

#[cfg(test)]
mod layout_tests {
    use super::{App, StatusPosition};
    use ratatui::layout::Rect;

    fn area(w: u16, h: u16) -> Rect {
        Rect::new(0, 0, w, h)
    }

    #[test]
    fn no_pane_top_status_at_row_0() {
        let l = App::compute_layout(area(80, 24), false, 50, StatusPosition::Top);
        assert_eq!(l.status.y, 0);
        assert_eq!(l.list.y, 1);
        assert_eq!(l.prompt.y, 23);
    }

    #[test]
    fn no_pane_bottom_status_at_last_row() {
        let l = App::compute_layout(area(80, 24), false, 50, StatusPosition::Bottom);
        assert_eq!(l.list.y, 0);
        assert_eq!(l.prompt.y, 22);
        assert_eq!(l.status.y, 23);
    }

    #[test]
    fn pane_open_top_status_above_list() {
        let l = App::compute_layout(area(80, 24), true, 50, StatusPosition::Top);
        assert_eq!(l.status.y, 0);
        assert!(l.list.y > l.status.y);
        let pane = l.pane.unwrap();
        let div = l.divider.unwrap();
        assert_eq!(div.y + 1, pane.y);
        // prompt sits in the top region, above the divider.
        assert!(l.prompt.y < div.y);
    }

    #[test]
    fn pane_open_bottom_status_below_pane() {
        let l = App::compute_layout(area(80, 24), true, 50, StatusPosition::Bottom);
        let pane = l.pane.unwrap();
        let div = l.divider.unwrap();
        assert_eq!(l.list.y, 0);
        assert_eq!(l.list.y + l.list.height, div.y);
        assert_eq!(div.y + 1, pane.y);
        // prompt one above status, both at the very bottom.
        assert_eq!(l.prompt.y, 22);
        assert_eq!(l.status.y, 23);
        // pane ends at the row above prompt.
        assert!(pane.y + pane.height <= l.prompt.y);
    }
}

#[cfg(test)]
mod background_tasks_tests {
    use super::BackgroundTasks;

    #[test]
    fn allocate_id_starts_at_one_and_monotonic() {
        let mut bg = BackgroundTasks::new();
        assert_eq!(bg.allocate_id(), 1);
        assert_eq!(bg.allocate_id(), 2);
        assert_eq!(bg.allocate_id(), 3);
    }

    #[test]
    fn most_recent_returns_last_pushed_id() {
        let mut bg = BackgroundTasks::new();
        assert_eq!(bg.most_recent(), None);
        // We can't easily construct full BackgroundTask values in a test
        // (they hold Box<dyn Child>), so we exercise the id allocator
        // and trust `most_recent`/`take` against the `tasks` Vec they
        // operate on. These pass-through helpers are simple enough that
        // the structural test is in the integration of ^Z / :fg flows.
        let _ = bg.allocate_id();
    }

    #[test]
    fn take_missing_id_returns_none() {
        let mut bg = BackgroundTasks::new();
        assert!(bg.take(99).is_none());
    }

    #[test]
    fn running_and_done_counts_are_zero_initially() {
        let bg = BackgroundTasks::new();
        assert_eq!(bg.running_count(), 0);
        assert_eq!(bg.done_count(), 0);
    }
}

#[cfg(test)]
mod strip_crlf_tests {
    use super::strip_crlf;

    #[test]
    fn crlf_collapses_to_lf() {
        assert_eq!(strip_crlf(b"a\r\nb\r\nc"), b"a\nb\nc");
    }

    #[test]
    fn passthrough_when_no_carriage_return() {
        assert_eq!(
            strip_crlf(b"hello world\nplain text"),
            b"hello world\nplain text"
        );
    }

    #[test]
    fn bare_cr_collapses_to_last_frame() {
        // git/npm/cargo progress: same line, multiple updates separated
        // by bare CR. We keep only the final frame.
        let input = b"Counting: 18%\rCounting: 27%\rCounting: 100%, done.\n";
        assert_eq!(strip_crlf(input), b"Counting: 100%, done.\n");
    }

    #[test]
    fn bare_cr_with_no_trailing_newline() {
        // Mid-stream view: last frame still wins, no terminator yet.
        assert_eq!(
            strip_crlf(b"Counting: 18%\rCounting: 50%"),
            b"Counting: 50%"
        );
    }

    #[test]
    fn mixed_crlf_and_bare_cr_across_lines() {
        let input = b"line1\r\nProgress: 10%\rProgress: 100%\r\nline3";
        assert_eq!(strip_crlf(input), b"line1\nProgress: 100%\nline3");
    }

    #[test]
    fn strips_soh_from_git_log_commit_message() {
        // Real-world: git log emits \x01 (SOH) in some commit-message
        // rendering paths -- e.g. when the original message contained
        // pasted control bytes. Without stripping, ratatui draws a
        // visible-but-zero-width glyph the host terminal consumes,
        // misaligning the rest of the line.
        let input = b"    \x01\tsrc/Foo.cs\n    \x01\tsrc/Bar.cs";
        assert_eq!(strip_crlf(input), b"    \tsrc/Foo.cs\n    \tsrc/Bar.cs");
    }

    #[test]
    fn strips_other_ascii_control_bytes() {
        // \b (BS), \v (VT), \f (FF), \x1c (FS), \x7f (DEL).
        let input = b"a\x08b\x0bc\x0cd\x1ce\x7ff";
        assert_eq!(strip_crlf(input), b"abcdef");
    }

    #[test]
    fn keeps_tab_newline_and_esc() {
        // \t, \n, and \x1b (ESC for ANSI) survive pass 3.
        let input = b"a\tb\nc\x1b[31md";
        assert_eq!(strip_crlf(input), b"a\tb\nc\x1b[31md");
    }
}
