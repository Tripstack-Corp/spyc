//! Top-level application state and event loop.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::spyc_debug;
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use glob::Pattern;
use ratatui::Frame;

use crate::config::{Config, StatusPosition};
use crate::fs::{self, Entry, EntryKind, Listing};
use crate::keymap::{Resolver, UserKeymap};
use crate::pane::{Pane, PaneTabs, TabEntry, TabInfo};
use crate::shell;
use crate::state::sessions::AgentKind;
use crate::state::{Cursor, Harpoon, History, IgnoreMasks, Inventory, Marks, Picks};
use crate::ui::line_edit::LineEditor;
use crate::ui::{
    help,
    list_view::Row,
    pager::{self, PagerView},
    theme::Theme,
};
use crate::{Tui, resume_tui, suspend_tui};

/// Unified message stream consumed by `App::run` (MVU Phase 1,
/// `docs/MVU_PLAN.md`). As of Phase 3d the loop is **fully event-driven** —
/// every source wakes this one channel and `run()` blocks on `recv()` with
/// no poll floor: the parkable crossterm reader feeds `Input` (+ `ReaderExited`
/// on death); the notify watcher closure feeds `FsEvent`; the git forwarder
/// feeds `GitResult` (3a); pane parser workers feed `PaneOutput` (3b); capture/
/// task reader threads feed `SinkOutput` (3c); the MCP forwarder feeds `Mcp`
/// and the finder/grep workers feed `FindOutput`/`GrepOutput` (3d). The only
/// remaining timed wakes are armed `Tick` deadlines (git poll, activity
/// rollover, capture-timer, …) — and they only SHORTEN the wait; nothing armed
/// means an unbounded block until a real message.
enum Message {
    /// A crossterm input event. The reader Press-filters `Key` events
    /// (only `Press`/`Repeat` are forwarded); `Paste`/`Resize`/`Focus`/
    /// `Mouse` pass through unchanged.
    Input(Event),
    /// MVU Phase 3a: a filesystem change from the notify watcher closure.
    /// Carries a bare `notify::Event` — the closure drops `Err` at the
    /// boundary, preserving the prior Ok-only drain contract. The recv arm
    /// only *buffers* it into `fs_pending`; the unchanged pre-recv drain
    /// stamps the debounce against `now_pre` (see `ingest_fs_event`).
    FsEvent(notify::Event),
    /// MVU Phase 3a: a git-worker result, routed via the forwarder thread
    /// onto the unified channel. The recv arm only *buffers* it into
    /// `git_pending`; the unchanged pre-recv drain applies it
    /// (generation-gated) via `ingest_git_result`.
    GitResult(state::GitWorkerResult),
    /// MVU Phase 3b: a pane PTY output WAKEUP — never carries bytes. A
    /// lost-wakeup-safe edge from a parser worker's 0→1 `wake_pending` CAS
    /// (the worker bumps `parser_gen` first). The loop treats it purely as
    /// "wake and re-scan": it re-enters the pre-recv pane scan, which clears
    /// each `wake_pending` and re-reads `parser_gen` via `drain_output`. The
    /// `tab` labels which pane woke us (carried for 3c/Phase-5; in 3b the
    /// scan re-drains all panes, so a stale id self-discards). Buffered +
    /// collapsed in the coalesce pre-step, NEVER surfaced as `Input`.
    PaneOutput { tab: SinkId },
    /// MVU Phase 3c: a foreground `!` capture or a background task produced
    /// output or hit EOF — the same lost-wakeup-safe edge as `PaneOutput`,
    /// fired by the shared `PtyHost` reader thread (captures/tasks have no
    /// parser worker; the main loop drains them). Carries no bytes and no
    /// exit status: the woken pre-recv drain re-scans the capture + all
    /// running tasks and observes `newly_closed`, harvesting exit inline
    /// (the reader can't call `child.wait()` — `portable_pty` needs
    /// `&mut self`). `sink` is a trace label only (the drain re-scans all,
    /// so a stale id after a `:fg`/`^Z`/demote/promote self-discards).
    /// Buffered/collapsed; never surfaced as `Input`.
    SinkOutput { sink: SinkId },
    /// MVU Phase 3d: a grep worker produced a batch or completed. Payloadless
    /// wake — the matches ride `GrepSession.rx`, re-drained by
    /// `drain_grep_session` (which id-gates against the live pager's
    /// `grep_id`, so a wake for a replaced/closed session self-discards).
    /// Collapsed in the coalesce pre-step; never surfaced as `Input`.
    GrepOutput,
    /// MVU Phase 3d: the F-finder walker produced a candidate batch or
    /// completed. Payloadless wake — the candidates ride `FindPicker.walk_rx`,
    /// re-drained by `drain_walk` (a wake after the picker closed no-ops at
    /// the `if let Some(picker)` guard). Collapsed in the coalesce pre-step;
    /// never surfaced as `Input`.
    FindOutput,
    /// MVU Phase 3d: a writable MCP request forwarded from the socket server.
    /// Unlike the wake variants, this carries a payload (the command + its
    /// one-shot reply Sender). Buffered into `mcp_pending` by the recv
    /// pre-step + the coalesce drain; executed + replied at the pre-recv MCP
    /// drain (`execute_mcp_command` writes the context file synchronously,
    /// then `reply.send` — preserving single-connection read-after-write).
    /// MUST NOT be dropped in coalesce (the reply Sender would strand the
    /// client). Never surfaced as `Input`.
    Mcp(crate::mcp_cmd::McpRequest),
    /// MVU Phase 3d: the input reader thread exited (fatal read/poll error or
    /// clean stop). Payloadless death-wake, sent AFTER `reader_done.store`
    /// (store-then-send → the loop-top Acquire-load sees the error). With the
    /// poll floor gone, this is what kicks a blocking `recv()`; the loop-top
    /// `reader_done` check then exits. Collapses to a Timeout like the other
    /// wakes; never surfaced as `Input`.
    ReaderExited,
    /// MVU Phase 2: a timer/deadline elapsed. Derived by the loop's own
    /// `Scheduler`, NOT a thread. The loop never actually sends itself a
    /// `Tick` — `recv_timeout` returning `Err(Timeout)` IS the tick handler
    /// (it re-evaluates every timer predicate against the fresh `now`). The
    /// variant exists so later subscriptions can push real `Tick`s onto the
    /// single channel without re-touching the enum.
    #[allow(dead_code)]
    Tick(Deadline),
    /// MVU Phase 6: an off-thread `active_agent_status` resolve landed a result
    /// in `agent_status_pending`. Like the git/MCP forwarders, the worker must
    /// WAKE the loop — the event-driven loop blocks on a bare `recv()` at idle,
    /// so a landed result would otherwise sit unread (and unrendered) until an
    /// unrelated event. Payloadless: collapses to a Timeout like the other
    /// re-scan wakes (drop-safe in coalesce). The redraw is driven by the
    /// pre-recv scan's `agent_status_pending` check, not by this message
    /// surviving — the actual apply stays in `active_agent_status` (render).
    AgentStatusReady,
}

/// How long to wait after spawning a restored Claude pane before
/// typing `/resume <sid>`. Banner / version-check / MCP-auth lines
/// can take well over a second to settle on cold starts; bumping
/// from the original 1500 ms reduces the race window where Claude
/// is still drawing when our keystrokes land.
const RESTORE_BANNER_SETTLE: Duration = Duration::from_secs(2);

/// Additional pause between typing `/resume <sid>` and pressing
/// Enter. A combined send (text + `\r` in one write) intermittently
/// landed in Claude's prompt mid-render — the chars stuck, the
/// trailing `\r` got dropped, and the user was left staring at an
/// unsubmitted command. Splitting the two writes a few hundred ms
/// apart gives the prompt time to settle in between.
const RESTORE_RESUME_ENTER_DELAY: Duration = Duration::from_millis(300);

/// Precomputed rects for the current frame. Built by `App::compute_layout`.
/// `pub` so the `pub fn compute_layout` (now in the `render` child
/// module) doesn't expose a more-private return type; fields stay
/// private — only `app` and its descendants read them.
pub struct FrameLayout {
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

mod actions;
mod agent_status;
mod capture;
mod commands;
mod effect;
mod find_picker;
mod git_state;
mod grep_session;
mod key_dispatch;
mod loop_steps;
mod pager_handler;
mod pager_history;
mod pane_wake;
mod prompt;
mod render;
mod route;
mod scheduler;
mod session;
mod sources;
pub mod state;
mod streaming;
mod tasks;

use capture::PendingCapture;
#[cfg(unix)]
pub use effect::SigOk;
pub use effect::{ClipMsg, Effect, PaneInput, PaneTarget, PaneTextKind, PaneTextSink};
use find_picker::FindPicker;
use grep_session::GrepSession;
use pager_history::PagerHistory;
use pane_wake::SinkId;
pub use prompt::{Prompt, PromptKind};
use scheduler::{Deadline, Scheduler, arm_resume_deadlines};
use sources::{coalesce_pending, sync_listing_watch, take_reader_result};
use tasks::{BackgroundTask, BackgroundTasks, TASK_BUFFER_CAP, TaskStatus};

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
        mount: crate::ui::pager::Mount,
        pane_scroll: bool,
    },
    /// On-disk file: reopen from the original path.
    SourceFile {
        path: PathBuf,
        scroll: u16,
        mount: crate::ui::pager::Mount,
        pane_scroll: bool,
    },
}

/// True when the active prompt is a file/directory-path entry
/// (copy-to, move-to, mkdir). These prompts get vi editing via
/// `Prompt::shell` but skip history nav — they share the
/// shell-command history slot, which has nothing useful for a
/// path prompt and was surfacing `make sync-all` on Up arrow.
const fn is_path_prompt_kind(mode: &Mode) -> bool {
    matches!(
        mode,
        Mode::Prompting(Prompt {
            kind: PromptKind::CopyTo | PromptKind::MoveTo | PromptKind::MakeDir,
            ..
        })
    )
}

/// Which persistent history bucket a prompt kind browses and records
/// into. Kept as a single pure mapping so the *browse* path
/// (`history_for_prompt`) and the *record-on-submit* path can't drift.
/// They did drift once: the `^a c` "pane cwd:" prompt recorded into the
/// same bucket as "pane command:", so directory paths leaked into the
/// command history's Up/Down browse. One mapping, two callers, no skew.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum HistoryBucket {
    /// Shell prompts (`!`, `;`, path prompts) — the default bucket.
    Shell,
    /// "pane command:" — commands tabs were launched with.
    PaneCmd,
    /// "pane cwd:" — working directories tabs were launched in.
    PaneCwd,
    /// `J` jump-to-path destinations.
    Jump,
    /// `:` vim-style command line.
    Command,
}

const fn history_bucket_for(kind: Option<&PromptKind>) -> HistoryBucket {
    match kind {
        Some(PromptKind::PaneNewTabCmd) => HistoryBucket::PaneCmd,
        Some(PromptKind::PaneNewTabCwd) => HistoryBucket::PaneCwd,
        Some(PromptKind::Jump) => HistoryBucket::Jump,
        Some(PromptKind::Command) => HistoryBucket::Command,
        _ => HistoryBucket::Shell,
    }
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

/// TTL cache for the active pane's status-line session short-id.
/// Keyed by the active pane's `(kind, cwd, spawn_epoch_secs)` — the
/// *spawn-time* cwd from `TabInfo`, which is immutable, so switching
/// tabs re-keys but a chdir *inside* a pane does not. Anything the key
/// doesn't capture (e.g. a custom session title) is bounded by the
/// `AGENT_STATUS_TTL` re-resolve.
struct AgentStatusCache {
    computed_at: std::time::Instant,
    kind: AgentKind,
    cwd: std::path::PathBuf,
    spawn_epoch_secs: u64,
    /// Cached status string (e.g. `"claude:9a7c4dc6"`) or `None`
    /// when no session resolved. Cached either way to avoid
    /// re-running the JSON walk for "no session" repeatedly.
    status: Option<String>,
}

const AGENT_STATUS_TTL: std::time::Duration = std::time::Duration::from_secs(30);

/// MVU Phase 5: the **Runtime** cluster — IO handles (channels, worker
/// endpoints, pty hosts, threads) held disjointly from the domain Model
/// (`App.state`) and the render/derived `ViewState`. Fields migrate in
/// over Phase-5 PRs; PR 1 seeds it with the git worker-result receiver
/// (the App-side half of the previously-torn git channel). The `App::split()`
/// three-borrow helper arrives in PR 4, where the first multi-cluster site
/// (`route_snapshot`) needs the three clusters borrowed at once.
struct Runtime {
    /// Git worker → main thread results, generation-gated, applied via
    /// `apply_git_worker_result`. The Phase-3a forwarder thread takes this
    /// once in `run()` and bridges it onto the unified `Message` channel.
    /// The Sender half stays on `state.git_worker_tx`.
    git_result_rx: Option<std::sync::mpsc::Receiver<state::GitWorkerResult>>,
    /// Commands from the MCP server; `run()` `.take()`s it into the forwarder
    /// thread which re-sends each as `Message::Mcp`.
    mcp_cmd_rx: Option<std::sync::mpsc::Receiver<crate::mcp_cmd::McpRequest>>,
    /// Clone of the unified-channel sender; pane wake closures clone it to push
    /// `Message::PaneOutput`. `None` before `run()` / in the test harness.
    pane_wake_tx: Option<std::sync::mpsc::Sender<Message>>,
    /// Monotonic `SinkId` allocator (never reused).
    next_sink_id: u64,
    /// Bottom pane tabs (each owns a `PtyHost`).
    pane_tabs: Option<PaneTabs>,
    /// Top-area overlay subprocess (`V`/`D`/`;`) — a `PtyHost`.
    top_overlay: Option<Pane>,
    /// In-flight foreground `!` capture (owns a `PtyHost`).
    pending_capture: Option<PendingCapture>,
    /// Backgrounded `!` tasks (each owns a `PtyHost`).
    background_tasks: BackgroundTasks,
    /// Active F-finder (holds the walker thread's receiver).
    find_picker: Option<FindPicker>,
    /// Active `:grep` session (holds the worker receiver).
    grep_session: Option<GrepSession>,
    /// Monotonic grep-session id (stale-session guard).
    next_grep_id: u32,
    /// Off-render-thread agent-status resolve: the landing slot + in-flight
    /// flag (see `active_agent_status` / `apply_landed_agent_status`).
    agent_status_pending: std::sync::Arc<std::sync::Mutex<Option<AgentStatusCache>>>,
    agent_status_refreshing: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

/// MVU end-state: the **ViewState** cluster — render ephemerals + derived
/// caches + UI-layer state. Pure of OS handles (those live in [`Runtime`]) and
/// of domain state (that lives in `AppState`). Owned by `App` as a disjoint
/// field; handlers reach it via `self.view.…`.
#[allow(clippy::struct_excessive_bools)]
pub struct ViewState {
    pub pager: Option<PagerView>,
    pub pager_history: PagerHistory,
    pub pager_pending_bracket: Option<char>,
    pub pager_was_open: bool,
    pub pager_jump_buf: Option<String>,
    /// Stash for the pager that was active when `?` opened the
    /// pager-help overlay. Restored verbatim on Esc/q dismissal so
    /// the user lands back in the same view. Separate from
    /// `pager_history` because the latter silently drops
    /// `no_history=true` views — going through history would lose them.
    pub pager_help_stash: Option<PagerView>,
    /// Per-file scroll memory for the pager (loaded once at startup;
    /// see [`state::pager_positions`]).
    pub pager_positions: crate::state::pager_positions::PagerPositions,
    /// Color/style overrides.
    pub theme: Theme,
    /// One-shot full buffer clear/redraw request.
    pub needs_full_repaint: bool,
    /// Cached `build_rows()` output; invalidated by `list_generation`.
    pub cached_rows: Vec<Row>,
    pub cached_rows_gen: u64,
    /// Grid stabilization cache key: (list_gen, view_top, cursor, width, height).
    pub cached_grid_key: (u64, usize, usize, u16, u16),
    /// Last terminal-window title emitted (OSC 2 dedup). `None` forces
    /// a re-emit on next draw.
    pub last_term_title: Option<String>,
    // --- D3: the remaining UI-layer ephemerals + activity counters ---
    /// Active harpoon menu overlay (interactive: reorder, delete, jump).
    // Module-private (type `HarpoonMenu` is module-private).
    harpoon_menu: Option<HarpoonMenu>,
    /// Active Quick Select picker (`^a u`).
    pub quick_select: Option<crate::pane::quick_select::QuickSelect>,
    /// `dd` arming for the graveyard view (first `d` arms, second deletes).
    pub graveyard_pending_d: bool,
    /// `gg` arming for the graveyard view (jump to top).
    pub graveyard_pending_g: bool,
    pub overlay_awaiting_dismiss: bool,
    pub pending_overlay_close: bool,
    /// TTL cache for the active pane's status-line session short-id.
    // Module-private (type `AgentStatusCache` is module-private); the
    // `app::*` descendant modules still reach it via `self.view.…`.
    agent_status_cache: Option<AgentStatusCache>,
    pub pending_history_pick: Option<LineEditor>,
    /// Snapshot of jump-history entries for the `J`-prompt popup.
    pub pending_jump_history: Option<Vec<String>>,
    pub history_pending_g: bool,
    /// Pending `g` in pane scroll mode (`gg`/`gf`/`gF`).
    pub scroll_pending_g: bool,
    // Module-private (type `PagerReturn` is module-private).
    pending_pager_return: Option<PagerReturn>,
    /// Path to the `.spyc-context.json` file (written each loop for MCP).
    pub context_path: PathBuf,
    /// Last serialized context JSON — skip the disk write when unchanged.
    pub last_context_json: String,
    /// `.spyc-context.json` is stale and should be rewritten (debounced +
    /// typing-burst-guarded).
    pub context_dirty: bool,
    /// Whether the MCP socket server is running.
    pub mcp_running: bool,
    /// When a focus-switch chord just completed: (when, completing key) —
    /// the next dispatch drops a Press/Repeat of that key within ~60 ms.
    pub focus_chord_completed: Option<(std::time::Instant, KeyCode)>,
    /// Activity monitor (`A`): draws/sec, bytes/sec overlay toggle + counters.
    pub show_activity: bool,
    pub activity_draws: u32,
    pub activity_bytes: u64,
    pub activity_last_tick: std::time::Instant,
    pub activity_dps: u32,
    pub activity_bps: u64,
    pub activity_reason_pane: u32,
    pub activity_reason_event: u32,
    pub activity_reason_other: u32,
    pub activity_snap_pane: u32,
    pub activity_snap_event: u32,
    pub activity_snap_other: u32,
    /// Peak frame/render time (µs) over the window + snapshots.
    pub activity_frame_peak_us: u64,
    pub activity_frame_peak_snap: u64,
    pub activity_render_peak_us: u64,
    pub activity_render_peak_snap: u64,
    /// Peak keystroke→echo latency (µs) + snapshot; `pane_send_at` is the
    /// forward timestamp measured against the next active-pane output.
    pub activity_echo_peak_us: u64,
    pub activity_echo_snap: u64,
    pub pane_send_at: Option<std::time::Instant>,
    pub activity_watcher_events: u32,
    pub activity_watcher_events_snap: u32,
    pub activity_mcp_reqs: u32,
    pub activity_mcp_reqs_snap: u32,
    pub activity_git_results: u32,
    pub activity_git_results_snap: u32,
    /// Roundtrip (ms) of the most recent git worker request.
    pub activity_git_last_ms: u32,
    /// `App::run` process start (activity-monitor uptime).
    pub started_at: std::time::Instant,
    /// Cached proc stats refreshed once per 1 s A-monitor tick.
    pub activity_proc_rss_kb: u64,
    pub activity_proc_threads: u32,
    /// Tab-completion / cycle state.
    // Module-private (type `TabState` is module-private).
    tab_state: Option<TabState>,
    /// Scroll throttle: timestamp + direction of last processed arrow key.
    pub scroll_last: Option<(std::time::Instant, KeyCode)>,
}

impl ViewState {
    /// Build the initial ViewState. `theme`/`context_path` are the only
    /// caller-specific values; `context_dirty` (write-context-on-startup) and
    /// `mcp_running` differ between the live app (`true` / actual) and the test
    /// harness (`false` / `false`). Everything else starts empty.
    fn new(theme: Theme, context_path: PathBuf, context_dirty: bool, mcp_running: bool) -> Self {
        Self {
            pager: None,
            pager_history: PagerHistory::new(),
            pager_pending_bracket: None,
            pager_was_open: false,
            pager_jump_buf: None,
            pager_help_stash: None,
            pager_positions: crate::state::pager_positions::PagerPositions::load(),
            theme,
            needs_full_repaint: false,
            cached_rows: Vec::new(),
            cached_rows_gen: u64::MAX, // force first build
            cached_grid_key: (u64::MAX, 0, 0, 0, 0),
            last_term_title: None,
            harpoon_menu: None,
            quick_select: None,
            graveyard_pending_d: false,
            graveyard_pending_g: false,
            overlay_awaiting_dismiss: false,
            pending_overlay_close: false,
            agent_status_cache: None,
            pending_history_pick: None,
            pending_jump_history: None,
            history_pending_g: false,
            scroll_pending_g: false,
            pending_pager_return: None,
            context_path,
            last_context_json: String::new(),
            context_dirty,
            mcp_running,
            focus_chord_completed: None,
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
            activity_frame_peak_us: 0,
            activity_frame_peak_snap: 0,
            activity_render_peak_us: 0,
            activity_render_peak_snap: 0,
            activity_echo_peak_us: 0,
            activity_echo_snap: 0,
            pane_send_at: None,
            activity_watcher_events: 0,
            activity_watcher_events_snap: 0,
            activity_mcp_reqs: 0,
            activity_mcp_reqs_snap: 0,
            activity_git_results: 0,
            activity_git_results_snap: 0,
            activity_git_last_ms: 0,
            started_at: std::time::Instant::now(),
            activity_proc_rss_kb: 0,
            activity_proc_threads: 0,
            tab_state: None,
            scroll_last: None,
        }
    }
}

pub struct App {
    /// Domain state — navigation, selection, filtering, config, etc.
    pub state: state::AppState,
    /// Render ephemerals + caches (see [`ViewState`]).
    view: ViewState,
    /// IO-handle cluster (channels, PtyHosts, worker endpoints, off-thread
    /// slots) — see [`Runtime`].
    runtime: Runtime,
    /// Summary printed to stdout after the TUI exits (read by `main`).
    pub exit_summary: Option<String>,
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

/// Per-iteration draw accumulator for the event loop. `dirty` is an OR
/// across every step in an iteration; `reason` is last-writer-wins,
/// matching the old `draw_reason = N` overwrite semantics. Reset by the
/// render block each iteration.
///
/// `reason` codes: 0 = none, 1 = pane output, 2 = input/git, 3 = other
/// (refresh / config / repaint / activity).
#[derive(Default)]
struct Draw {
    dirty: bool,
    reason: u8,
}

impl Draw {
    /// Mark the frame dirty and set the reason (last writer wins).
    const fn mark(&mut self, reason: u8) {
        self.dirty = true;
        self.reason = reason;
    }

    /// Mark dirty WITHOUT touching `reason` — for the activity-only
    /// rollover redraw, which must not bump a real `draw_reason` (the
    /// render-stats block reads `reason` and skips counting activity-only
    /// frames).
    const fn set_dirty(&mut self) {
        self.dirty = true;
    }
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
        // Defer the initial git-status read to the background worker
        // (kicked off after AppState is built, below). Previously
        // these two `git status` spawns blocked the first paint by
        // 200-500 ms on a ~110k-file repo. Cache-miss handling in
        // the chdir / event-loop path will populate `git_info` and
        // `git_files` once the worker reports back.
        let git_info: Option<String> = None;
        let git_files: std::collections::HashMap<String, crate::ui::list_view::GitFileStatus> =
            std::collections::HashMap::new();
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

        // Always anchor PROJECT_HOME on the launch dir. Previously
        // this was gated on `cwd.join(".git").exists()`, which meant
        // launching spyc one level above the actual repo (e.g. from
        // `~/src/workspace` containing a Java monorepo at
        // `~/src/workspace/inner-repo`) left `project_home` None —
        // and downstream code (session save, harpoon, MCP context)
        // had no project anchor at all. Honoring the launch dir
        // gives every spyc invocation a project anchor; users who
        // want a different anchor can override with `:project <path>`
        // or `gP`. Cleared with `:project clear`.
        let project_home = Some(cwd.clone());
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
            sort_reversed: false,
            view: View::Dir,
            cursor: Cursor::new(),
            resolver: Resolver::new(),
            user_keymap,
            config,
            mode: Mode::Normal,
            project_home,
            session_name,
            frecency: crate::state::Frecency::load(),
            focus: state::Focus::FileList,
            // spyc (top) = 30%, pane (bottom) = 70%. Resize with `^W +/-`.
            pane_height_pct: 70,
            pane_zoomed: false,
            pane_focus_before_zoom: None,
            pane_hidden: false,
            harpoon_filter_set: harpoon
                .as_ref()
                .map(|h| h.ancestor_set().clone())
                .unwrap_or_default(),
            // MVU Phase 5: domain fields relocated from `App`. Note
            // `harpoon` is moved (consumed) AFTER `harpoon_filter_set`
            // borrowed it just above.
            harpoon,
            pane_prompt_buf: String::new(),
            last_pane_prompt: None,
            pane_snapshot: state::PaneSnapshot::default(),
            pending_delete_preview: None,
            // Populated on the first successful `refresh_git_state`
            // call. See `AppState::git_poll_cache` doc for why this
            // starts None.
            git_poll_cache: None,
            // The very first chdir of App::run will set both based
            // on the actual tree size. Bootstrap defaults are fine —
            // the small-tree cadence is conservative until proven
            // huge.
            is_huge_tree: false,
            huge_tree_anchor: None,
            huge_tree_decisions: std::collections::HashMap::new(),
            current_repo_root: None,
            current_gitdir: None,
            git_status_raw_cache: None,
            git_worker_tx: None,
            git_generation: 0,
            last_git_invalidation: None,
            last_git_request_at: None,
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
            pane_cwd_history: History::load_file("pane_cwd_history"),
            jump_history: History::load_file("jump_history"),
            command_history: History::load_file("command_history"),
            flash: start_error.map(|text| FlashMessage {
                text,
                kind: FlashKind::Error,
            }),
            user_host: user_host_string(),
            git: state::GitState {
                info: git_info,
                files: git_files,
            },
            should_quit: false,
            rows: Vec::new(),
            grid_dims: crate::ui::list_view::GridDims {
                cols: 1,
                rows_per_col: 1,
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
        // Background git-status worker. Owns the spawn of
        // `git status --porcelain` on cache miss so the chdir UI
        // returns immediately. Lives for the lifetime of the
        // process; the OS reaps the thread on exit. We hold the
        // sender on `state.git_worker_tx` and the receiver on
        // `runtime.git_result_rx`. See `state::GitWorkerRequest` /
        // `state::GitWorkerResult` for the contract.
        let (git_req_tx, git_req_rx) = std::sync::mpsc::channel::<state::GitWorkerRequest>();
        let (git_res_tx, git_res_rx) = std::sync::mpsc::channel::<state::GitWorkerResult>();
        std::thread::spawn(move || {
            while let Ok(req) = git_req_rx.recv() {
                // Stat the cache-key mtimes BEFORE reading status. An
                // index write racing this read then lands in the *next*
                // poll's diff: an older key paired with newer status is
                // safe (forces one redundant refresh), whereas the
                // reverse order — newer key, older status — would make
                // the 1 Hz poll short-circuit on a stale snapshot
                // forever, hiding staged/working changes until an
                // unrelated later write moved the mtime.
                let (index_mtime, head_mtime) = crate::sysinfo::resolve_gitdir(&req.repo_root)
                    .map_or((None, None), |gd| {
                        let i = std::fs::metadata(gd.join("index"))
                            .and_then(|m| m.modified())
                            .ok();
                        let h = std::fs::metadata(gd.join("HEAD"))
                            .and_then(|m| m.modified())
                            .ok();
                        (i, h)
                    });
                let raw = crate::sysinfo::git_status_porcelain_raw(&req.canonical);
                let _ = git_res_tx.send(state::GitWorkerResult {
                    generation: req.generation,
                    repo_root: req.repo_root,
                    raw,
                    index_mtime,
                    head_mtime,
                });
            }
        });
        let mut app_state = app_state;
        app_state.git_worker_tx = Some(git_req_tx);
        let mut app = Self {
            state: app_state,
            // Write context once on startup so claude sees initial state
            // (context_dirty: true).
            view: ViewState::new(theme, context_path, true, mcp_running),
            exit_summary: None,
            runtime: Runtime {
                git_result_rx: Some(git_res_rx),
                mcp_cmd_rx: Some(mcp_cmd_rx),
                pane_wake_tx: None,
                next_sink_id: 0,
                pane_tabs: None,
                top_overlay: None,
                pending_capture: None,
                background_tasks: BackgroundTasks::new(),
                find_picker: None,
                grep_session: None,
                next_grep_id: 0,
                agent_status_pending: std::sync::Arc::new(std::sync::Mutex::new(None)),
                agent_status_refreshing: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(
                    false,
                )),
            },
        };
        app.state.rebuild_rows();
        // Evaluate huge-tree status at startup so the first 1 Hz poll
        // / first event-driven refresh uses the right cadence and
        // `git status` flag. Without this, spyc launched directly
        // in a 110k-file project root would run small-tree cadence
        // until the user navigated somewhere.
        let initial_cwd = app.state.listing.dir.clone();
        app.state.update_huge_tree(&initial_cwd);
        // Now that the worker is wired and we know is_huge_tree,
        // kick off the first git read in the background. The branch
        // string is computed sync from `.git/HEAD` so it's available
        // on the first paint; only the per-file markers and dirty
        // flag wait for the worker.
        app.state.git.info = app.state.compute_git_info_fast();
        let _ = app.state.git_file_statuses_cached(&initial_cwd);
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
        if app.view.mcp_running {
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
    /// Refresh `activity_proc_rss_kb` / `activity_proc_threads`. Called once
    /// per A-monitor 1 s tick. `proc_rss_threads` reads the OS directly
    /// (sysinfo for rss + libproc for the macOS thread count) — a fast
    /// syscall, not a `ps` fork-exec, so it runs inline (the off-thread
    /// machinery #227 added for the slow `ps` spawn is no longer needed).
    fn refresh_process_stats(&mut self) {
        if let Some((rss, threads)) = crate::sysinfo::proc_rss_threads() {
            self.view.activity_proc_rss_kb = rss;
            self.view.activity_proc_threads = threads;
        }
    }

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
            git_branch: self.state.git.info.clone(),
            project_home: self.state.project_home.clone(),
            session_name: self.state.session_name.clone().unwrap_or_default(),
        }
    }

    /// Write the context file (best-effort, errors are silently ignored).
    /// Skips the disk write when the serialized JSON is unchanged.
    fn write_context(&mut self) {
        let ctx = self.snapshot_context();
        let json = serde_json::to_string_pretty(&ctx).unwrap_or_default();
        if json == self.view.last_context_json {
            return;
        }
        // MVU Phase 3d: only advance the dedup cache when the write actually
        // landed. If `write_context_file` fails, `last_context_json` stays
        // behind disk, so a later identical-state mutation still writes
        // instead of dedup-skipping into a stale file. (The 500ms cap used to
        // mask this by re-running the debounced writer; it's gone now.)
        if crate::context::write_context_file(&self.view.context_path, &ctx).is_ok() {
            self.view.last_context_json = json;
        }
    }

    /// Execute a writable MCP command from Claude. Runs on the main
    /// thread with full access to `AppState`. Returns a response that
    /// the MCP server thread forwards to Claude.
    /// Persist the current pager's scroll position to disk if it's a
    /// file-backed view (`source_path` is set). Call before any
    /// assignment that drops or replaces `self.view.pager` so the user's
    /// reading position survives close + reopen. No-op for command
    /// output, help, picker UIs, etc. — those views intentionally
    /// don't carry a `source_path`.
    fn remember_pager_position(&mut self) {
        if let Some(view) = self.view.pager.as_ref()
            && let Some(path) = view.source_path.clone()
        {
            let scroll = view.scroll;
            self.view.pager_positions.record(&path, scroll);
        }
    }

    /// Close the active pager, persisting its scroll position first.
    /// Drop-in replacement for the raw `pager = None` assignment
    /// everywhere the user's reading position should survive close
    /// + reopen.
    fn clear_pager(&mut self) {
        self.remember_pager_position();
        self.view.pager = None;
    }

    /// Tear down a `^a-v` scrollback pager: snap the pty back to
    /// live, clear the pager, force a repaint, and flash the
    /// status change. Mirrors the Esc/q close path so chord-driven
    /// and focus-switch escapes land in the same final state. No-op
    /// when no pane_scroll pager is open — safe to call from
    /// `Action::PaneFocusUp` / `PaneFocusDown` unconditionally.
    fn close_pane_scroll_pager(&mut self) {
        if !self.view.pager.as_ref().is_some_and(|v| v.pane_scroll) {
            return;
        }
        if let Some(tabs) = self.runtime.pane_tabs.as_mut() {
            tabs.active_mut().exit_scroll_mode();
        }
        self.clear_pager();
        self.view.needs_full_repaint = true;
        self.state.flash_info("scroll: off");
    }

    /// Assign a new pager view, persisting the outgoing view's
    /// scroll position first. Drop-in replacement for the bare
    /// `pager = Some(view)` pattern at open / replace sites
    /// — covers the case where the user has one file open, opens
    /// another, then later wants to come back to the first one.
    fn set_pager(&mut self, view: PagerView) {
        self.remember_pager_position();
        self.view.pager = Some(view);
    }

    /// Apply a git-worker result if it's still relevant — matching
    /// generation, repo_root, and huge flag all required. Refills the
    /// raw-status cache, then recomputes `git_files` (parsed against
    /// the *current* listing dir's prefix, which may differ from the
    /// dir the worker saw if the user has drilled around within the
    /// project) and `git_info`. Returns true iff state changed.
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
                        // Write synchronously: an MCP client commonly
                        // calls get_spyc_context right after a mutation,
                        // and that reads the on-disk context file. The
                        // debounced gate can lag seconds behind under a
                        // typing burst, so the follow-up read would see
                        // stale state. (Non-MCP edits stay debounced.)
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
                self.view.mcp_running = false;
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
                self.view.theme = Theme::default().with_overrides(&new_config.colors);
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

        // MVU Phase 3a: the single message channel. The parkable input
        // reader, the notify watcher closure, and the git forwarder all
        // feed `msg_tx`; the loop `recv_timeout`s on `msg_rx`. Created
        // first so the watcher/forwarder can clone a sender before the
        // reader takes ownership of the original.
        let (msg_tx, msg_rx) = mpsc::channel::<Message>();

        // File watcher: notify posts events onto the unified channel via a
        // closure `EventHandler` that wraps each `Ok(Event)` as
        // `Message::FsEvent`, dropping `Err` at the boundary (preserving
        // the prior Ok-only drain contract). Two kinds of watch:
        //
        // 1. Config files — we watch their *parent* directories, not the
        //    files, because editors that replace-on-save (vim, VS Code,
        //    nvim) remove the old inode before creating the new one.
        //
        // 2. The current listing directory (non-recursive) — so external
        //    changes (a build artifact dropping in, `git pull`, etc.) are
        //    reflected without a manual refresh.
        let watcher_tx = msg_tx.clone();
        let mut fs_watcher: Option<notify::RecommendedWatcher> =
            notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                if let Ok(ev) = res {
                    let _ = watcher_tx.send(Message::FsEvent(ev));
                }
            })
            .ok();
        let mut already_watched: std::collections::HashSet<PathBuf> =
            std::collections::HashSet::new();
        if let Some(w) = fs_watcher.as_mut() {
            for path in self.candidate_config_paths() {
                if let Some(parent) = path.parent()
                    && parent.is_dir()
                    && already_watched.insert(parent.to_path_buf())
                {
                    let _ = w.watch(parent, RecursiveMode::NonRecursive);
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
            self.state.current_gitdir.as_deref(),
        );

        let mut last_context_write = std::time::Instant::now();
        let mut last_refresh = std::time::Instant::now();
        // 1Hz safety net: re-poll git state even if FSEvents missed
        // the `.git/index.lock` → `.git/index` rename. See
        // `AppState::refresh_git_state`.
        let mut last_git_poll = std::time::Instant::now();
        // MVU Phase 2: advisory deadline scheduler — computes the
        // recv_timeout wait from armed timers; the loop still fires each
        // timer via its own predicate against the threaded `now`.
        let mut scheduler = Scheduler::new();
        // Trailing debounce: fire refresh once events have stopped
        // arriving for `REFRESH_QUIET`. Bursty git operations
        // (`git add && git commit && git push`) emit several
        // `.git/index` rename events spread over hundreds of ms;
        // firing on the *first* event meant the subprocess sometimes
        // ran during an in-flight, transient state ("M " staged but
        // not yet committed). Waiting for quiet ensures we only
        // sample git after the storm has passed.
        let mut last_event_at: Option<std::time::Instant> = None;
        // First listing event since the last refresh — fixed when the
        // window opens, NOT bumped on each subsequent event. Lets the
        // trailing-debounce fire after `max_refresh_defer` of continuous
        // activity (cargo / claude / agent writes saturating the
        // watcher) instead of starving indefinitely. Cleared on refresh.
        let mut first_event_after_refresh: Option<std::time::Instant> = None;

        // MVU Phase 3a: buffers for messages the recv arm coalesces. The
        // recv arms ONLY push here (zero state mutation); the unchanged
        // pre-recv drains above process them against `now_pre`. This keeps
        // the timing-sensitive debounce / generation-gate logic exactly
        // where it was — `recv` only changes *when* the loop wakes, not
        // *how* events are handled.
        let mut fs_pending: Vec<notify::Event> = Vec::new();
        let mut git_pending: Vec<state::GitWorkerResult> = Vec::new();
        // MVU Phase 3d: writable MCP requests buffered by the recv pre-step,
        // executed + replied at the pre-recv drain (preserving the synchronous
        // write_context → reply read-after-write ordering).
        let mut mcp_pending: Vec<crate::mcp_cmd::McpRequest> = Vec::new();

        // Last keypress instant. MVU Phase 3b PR2 retired its use as a
        // poll-cadence trigger (the typing-burst hack — panes now wake the
        // loop via `Message::PaneOutput`, so the first PTY echo after a
        // keystroke arrives on the channel, not via a tightened poll). It
        // survives for its OTHER consumer: the context-write debounce
        // suppressor, which holds off the MCP context mtime bump for 300 ms
        // after a keystroke so claude's input echo isn't yanked mid-type.
        let mut last_input_at: Option<std::time::Instant> = None;

        // MVU Phase 3a: the git-status worker (spawned in `new()`) keeps
        // sending onto its own channel; this forwarder bridges its results
        // onto the unified channel as `Message::GitResult`, so `recv` wakes
        // on a fresh git status instead of waiting out the poll. `.take()`
        // here is the sole consumer of `git_result_rx`. The thread parks in
        // `recv()` until the worker's sender drops at App teardown (the loop
        // has long exited by then) or `msg_rx` drops (send errors → break).
        // Because this sender keeps the channel Connected after the input
        // reader dies, reader-death is detected via `reader_done` below, NOT
        // channel disconnection.
        if let Some(git_rx) = self.runtime.git_result_rx.take() {
            let gtx = msg_tx.clone();
            std::thread::spawn(move || {
                while let Ok(r) = git_rx.recv() {
                    if gtx.send(Message::GitResult(r)).is_err() {
                        break;
                    }
                }
            });
        }

        // MVU Phase 3d: the MCP socket server (spawned in `new()`) keeps
        // sending requests onto its own channel; this forwarder bridges them
        // onto the unified channel as `Message::Mcp`, so `recv` wakes on an
        // MCP request instead of waiting out the poll. `.take()` here is the
        // sole consumer of `mcp_cmd_rx`. Same shape as the git forwarder; the
        // socket server (`start_socket_server`) is unchanged and never names
        // `Message`. The request carries its one-shot reply Sender, executed
        // + replied on the main loop (read-after-write preserved).
        if let Some(mcp_rx) = self.runtime.mcp_cmd_rx.take() {
            let mtx = msg_tx.clone();
            std::thread::spawn(move || {
                while let Ok(req) = mcp_rx.recv() {
                    if mtx.send(Message::Mcp(req)).is_err() {
                        break;
                    }
                }
            });
        }

        // MVU Phase 3b: install the channel sender so pane wake closures
        // (built by `make_pane_wake` at every spawn site) can push
        // `Message::PaneOutput`. Set BEFORE the reader moves `msg_tx` and
        // before the loop processes any user action — so every pane spawned
        // during the session (including session-restore tabs) gets a live
        // wake, not the pre-run no-op.
        self.runtime.pane_wake_tx = Some(msg_tx.clone());

        // MVU Phase 1: the parkable input reader runs on its own thread
        // and feeds `msg_tx`; the loop `recv_timeout`s on `msg_rx` instead
        // of calling `event::poll`/`event::read` directly. `reader_handle`
        // is a run()-scoped local so its `Drop` (stop + unpark + join)
        // tears the thread down when run() returns. Phase 3a/3b: the watcher,
        // git forwarder, and pane workers (via `pane_wake_tx`) also feed the
        // channel; tasks / MCP stay polled until 3c/3d.
        let reader_handle = spawn_input_reader(msg_tx);
        let read_err = reader_handle.read_err.clone();
        // Phase 3a: extra senders (watcher/forwarder) keep the channel
        // Connected after the reader thread dies, so the `Err(Disconnected)`
        // arm no longer fires on reader death. Gate the loop-exit on this
        // flag instead (set true when the reader returns).
        let reader_done = reader_handle.reader_done.clone();
        let foreground_exec = ForegroundExec {
            park: reader_handle.park.clone(),
            acked: reader_handle.acked.clone(),
            reader_done: reader_handle.reader_done.clone(),
            reader: reader_handle
                .handle
                .as_ref()
                .expect("input reader handle present")
                .thread()
                .clone(),
        };

        // (Pre-v1.50.84 the loop carried `last_pane_render` and
        // `last_active_drain` timestamps to throttle pane renders /
        // parses while the user was typing. Both became unnecessary
        // once parsing moved to a per-Pane worker thread — they
        // were just delaying the moment the main thread noticed
        // the worker had finished an echo, which manifested as
        // off-by-one input lag. Removed.)

        // Draw at least once on startup (dirty: true, reason: 3 = other).
        let mut draw = Draw {
            dirty: true,
            reason: 3,
        };

        while !self.state.should_quit {
            // MVU Phase 3d: authoritative reader-death exit. With the poll
            // floor gone, the loop blocks on `recv()`; the reader sends a
            // `ReaderExited` wake on death to kick that recv, but this
            // level-triggered check is what actually exits — it can't be
            // consumed by `coalesce_pending` (which drops the wake), so it
            // catches death even when the edge wake races a real message.
            if reader_done.load(std::sync::atomic::Ordering::Acquire) {
                return take_reader_result(&read_err);
            }
            // One-shot full repaint after a pane or overlay closes (or any
            // other event that leaves ratatui's diff buffer stale).
            // Also force repaint when the pager opens while a pane exists,
            // because the pane stops rendering and its stale cells need clearing.
            // When the pager opens over a pane, the pane's stale cells
            // need clearing. But don't use terminal.clear() for this — the
            // pager overlay will paint over everything anyway, and the
            // clear causes a visible flash. Just force a draw instead.
            let (pager_redraw, pending_clear) = self.step_pager_repaint();
            if pager_redraw {
                draw.mark(3);
            }
            // NOTE: periodic ^L to Claude pane tabs was removed — it clears
            // any draft prompt the user has typed, even when focus is on the
            // file list (the text is still in Claude's input buffer).

            // pending_overlay_close is no longer used — the overlay stays
            // visible until Enter via overlay_awaiting_dismiss.
            let _ = self.view.pending_overlay_close;

            // MVU Phase 3c: drain the streaming pull sources (extracted to
            // streaming.rs). Each returns whether it needs a redraw. The
            // poll floor still backstops them this PR; PR3 deletes it once
            // these wake the channel.
            if self.drain_pending_capture() {
                draw.mark(3);
            }
            if self.drain_background_tasks() {
                draw.mark(3);
            }
            if self.refresh_task_viewer() {
                draw.mark(3);
            }

            // MVU Phase 6: drain any off-thread agent-status resolve that
            // landed (it woke us via `Message::AgentStatusReady`). Done HERE,
            // in the always-run scan — not in `active_agent_status` — because
            // the status bar (and thus `active_agent_status`) is skipped on the
            // overlay / top-pager render paths; draining only there would leave
            // the slot full and this nudge would busy-spin. Applying the result
            // updates the cache so the next render shows the short-id.
            if self.apply_landed_agent_status() {
                draw.mark(3);
            }

            // F-finder: drain any candidate batches the walker
            // worker has pushed since the last tick. Re-rank +
            // re-render only when something changed (or the walk
            // completed -- title flips from "scanning..." to a
            // final count).
            if let Some(picker) = self.runtime.find_picker.as_mut()
                && picker.drain_walk()
            {
                picker.refilter();
                self.render_find_picker();
                draw.mark(3);
            }

            // :grep session: drain match batches into the active
            // grep pager. Same shape as the F-finder drain but the
            // results land directly in the pager body instead of
            // being re-ranked.
            if self.drain_grep_session() {
                draw.mark(3);
            }

            // Pre-recv pane-output scan: drain every tab + overlay, flip the
            // background-tab divider glyph, mark exited tabs (see
            // `drain_pane_output` — clear_wake/drain_output CAS lives there).
            let (pane_draw, pane_reason) = self.drain_pane_output();
            if pane_draw {
                draw.mark(pane_reason);
            }

            // MVU Phase 5: snapshot the active pane's routing flags into the
            // Model, AFTER the drain + `mark_exited` finalized `is_closed` and
            // BEFORE `recv` (see `snapshot_pane_routing`).
            self.snapshot_pane_routing();

            // MVU Phase 2: one clock read for all PRE-recv timers
            // (send_pending_resumes / find_crashed_restore_tab /
            // watcher-stamp / refresh / git poll), matching their old
            // pre-recv local reads. POST-recv timers (activity rollover,
            // context-write) use `now_post` captured after recv returns.
            let now_pre = std::time::Instant::now();

            // Session-restore: deferred `/resume` sends + crash-recovery prompt
            // (see `handle_restore_resumes`).
            if self.handle_restore_resumes(now_pre, &mut scheduler) {
                draw.mark(3);
            }

            // Drain buffered FsEvents + run the trailing-debounce listing
            // refresh (see `ingest_fs_and_maybe_refresh`).
            if self.ingest_fs_and_maybe_refresh(
                now_pre,
                &mut scheduler,
                &mut fs_pending,
                &mut last_event_at,
                &mut first_event_after_refresh,
                &mut last_refresh,
            ) {
                draw.mark(3);
            }
            // 1 Hz safety-net git poll + GitPoll deadline arming (see
            // `poll_git_cadence`).
            if self.poll_git_cadence(now_pre, &mut last_git_poll, &mut scheduler) {
                draw.mark(3);
            }

            // Execute writable MCP commands buffered into `mcp_pending` (see
            // `drain_mcp_pending` — kept at this early loop position for the
            // 5s read-after-write timeout contract).
            if self.drain_mcp_pending(&mut mcp_pending) {
                draw.mark(3);
            }

            // Drain the git-worker results buffered into `git_pending` — the
            // SOLE apply/count/take site (see `drain_git_pending`).
            if self.drain_git_pending(&mut git_pending) {
                draw.mark(2);
            }

            // MVU Phase 3c: the last poll floor is GONE. Every event source
            // now wakes the channel — panes via `PaneOutput`, captures/tasks
            // via `SinkOutput`, fs/git (3a) directly. The only remaining
            // pull sources are MCP + finder/grep (3d), serviced by the
            // MVU Phase 3d: the last poll (`MAX_IDLE_CAP`) is GONE — every
            // source now wakes the channel (input, fs, git, panes, captures/
            // tasks, MCP, finder, grep, and reader-death). The loop blocks on
            // `recv()` when no deadline is armed; an armed deadline only
            // SHORTENS the wait (it never lengthens it, and there's no ceiling
            // clamp — a 1s GitPoll now drives a 1s wait, a 10s huge-tree poll a
            // 10s wait). Fresh clock JUST before recv so a deadline-driven
            // sleep lands on the deadline, not deadline + body-cost.
            let wait_now = std::time::Instant::now();
            // If the pre-recv drains already dirtied the frame, DON'T block —
            // a zero-timeout recv falls straight through to the draw this
            // iteration. Blocking here delayed already-drained pane output
            // (e.g. a keystroke echo) until the next message/deadline arrived,
            // a visible per-keystroke render lag. (Draw-before-you-block.)
            let wait = if draw.dirty {
                Some(Duration::ZERO)
            } else {
                scheduler.next().map(|when| {
                    when.saturating_duration_since(wait_now)
                        .max(Duration::from_millis(1))
                })
            };
            let recvd = match wait {
                // Deadline armed → bounded wait.
                Some(d) => msg_rx.recv_timeout(d),
                // Nothing armed → block until a real message. A dead reader
                // can't strand the loop: it sends `ReaderExited` on death
                // (kicking this recv), and the loop-top `reader_done` check is
                // the authoritative exit.
                None => msg_rx
                    .recv()
                    .map_err(|_| std::sync::mpsc::RecvTimeoutError::Disconnected),
            };

            // MVU Phase 3a: having received, *coalesce* — buffer every
            // immediately-available FsEvent/GitResult/Mcp into the pending Vecs
            // (drained at the top of the next iteration) and surface only an
            // Input (or a Tick/Timeout/Disconnected) to the dispatch match.
            // This collapses a burst into a single wakeup and bounds Input
            // latency to one iteration. Input is NEVER handled inside the
            // coalesce loop (an `Effect::ForegroundExec` parks the reader /
            // re-inits the TUI), only surfaced for the arm — and the coalesce stops at
            // the first one, so Input stays one-per-iteration and FIFO.
            let effective = match recvd {
                Ok(Message::FsEvent(ev)) => {
                    fs_pending.push(ev);
                    coalesce_pending(&msg_rx, &mut fs_pending, &mut git_pending, &mut mcp_pending)
                        .map_or(Err(std::sync::mpsc::RecvTimeoutError::Timeout), |ev| {
                            Ok(Message::Input(ev))
                        })
                }
                Ok(Message::GitResult(r)) => {
                    git_pending.push(r);
                    coalesce_pending(&msg_rx, &mut fs_pending, &mut git_pending, &mut mcp_pending)
                        .map_or(Err(std::sync::mpsc::RecvTimeoutError::Timeout), |ev| {
                            Ok(Message::Input(ev))
                        })
                }
                // MVU Phase 3d: buffer the MCP request (carries its reply
                // Sender), collapse companions, synthesize Timeout so the
                // pre-recv MCP drain executes it + replies.
                Ok(Message::Mcp(req)) => {
                    mcp_pending.push(req);
                    coalesce_pending(&msg_rx, &mut fs_pending, &mut git_pending, &mut mcp_pending)
                        .map_or(Err(std::sync::mpsc::RecvTimeoutError::Timeout), |ev| {
                            Ok(Message::Input(ev))
                        })
                }
                // MVU Phase 3b: a pane wake carries no payload to buffer —
                // collapse any companion wakes/fs/git, then synthesize a
                // Timeout so control re-enters the loop top and the pre-recv
                // pane scan does the clear+drain. NEVER drained inline, NEVER
                // surfaced as Input (except a coalesced real keystroke).
                Ok(Message::PaneOutput { tab } | Message::SinkOutput { sink: tab }) => {
                    // A pane (3b) or capture/task (3c) wake. `tab`/`sink`
                    // labels which source woke us — logged for wake-path
                    // traceability; the pre-recv drains re-scan all sources,
                    // so the id isn't used to target. Collapse companions →
                    // synthesize Timeout so control re-enters the pre-recv
                    // drains (pane scan + capture/task drains).
                    spyc_debug!("sink wake: {tab:?}");
                    coalesce_pending(&msg_rx, &mut fs_pending, &mut git_pending, &mut mcp_pending)
                        .map_or(Err(std::sync::mpsc::RecvTimeoutError::Timeout), |ev| {
                            Ok(Message::Input(ev))
                        })
                }
                // MVU Phase 3d / Phase 6: a grep/finder wake, a reader
                // death-wake, or an agent-status-resolved wake — all
                // payloadless, collapse-to-Timeout. For grep/finder the
                // pre-recv drains re-run; for ReaderExited the synthesized
                // Timeout re-enters the loop, where the loop-top reader_done
                // check exits; for AgentStatusReady the pre-recv scan's
                // pending-check marks the frame dirty so render applies the landed
                // short-id (the worker can't redraw, only wake — see the field
                // doc on `agent_status_pending`).
                Ok(
                    Message::GrepOutput
                    | Message::FindOutput
                    | Message::ReaderExited
                    | Message::AgentStatusReady,
                ) => coalesce_pending(&msg_rx, &mut fs_pending, &mut git_pending, &mut mcp_pending)
                    .map_or(Err(std::sync::mpsc::RecvTimeoutError::Timeout), |ev| {
                        Ok(Message::Input(ev))
                    }),
                other => other,
            };
            match effective {
                Ok(Message::Input(ev)) => {
                    draw.mark(2);
                    match ev {
                        Event::Key(key)
                            if key.kind == KeyEventKind::Press
                                || key.kind == KeyEventKind::Repeat =>
                        {
                            // Arm the typing-burst window — see the poll-ms
                            // computation above. Cheap; just stores an
                            // Instant.
                            last_input_at = Some(std::time::Instant::now());
                            // Mark the context file dirty: every keypress
                            // is potentially a state-mutating action
                            // (cursor move, pick toggle, chdir, etc.).
                            // The end-of-iteration write is debounced and
                            // serializes-then-skips when JSON is unchanged,
                            // so a no-op keystroke (e.g. pressing keys in
                            // a chord prefix) won't actually touch disk.
                            self.view.context_dirty = true;
                            // Throttle rapid-fire arrow keys from trackpad scroll
                            // (DEC 1007 alternate-scroll). Allow ~25 events/sec.
                            if matches!(key.code, KeyCode::Up | KeyCode::Down)
                                && key.modifiers.is_empty()
                            {
                                let now = std::time::Instant::now();
                                if let Some((prev, dir)) = self.view.scroll_last
                                    && dir == key.code
                                    && now.duration_since(prev).as_millis() < 40
                                {
                                    continue;
                                }
                                self.view.scroll_last = Some((now, key.code));
                            } else {
                                self.view.scroll_last = None;
                            }
                            // MVU Phase 4: the handler returns a list of
                            // effects; `run_effects` is the sole executor
                            // (the ForegroundExec arm carries the former
                            // inline spawn + its after-work).
                            let effects = self.handle_key(key)?;
                            self.run_effects(effects, terminal, &foreground_exec)?;
                        }
                        Event::Paste(text) => self.handle_paste(text)?,
                        Event::Resize(cols, rows) => self.handle_resize(cols, rows),
                        _ => {}
                    }
                }
                // No input this tick: re-poll the still-polled sources; no
                // draw (matches the old `event::poll(...) == false`). The
                // loop never sends itself a Tick (the scheduler is advisory)
                // so Tick is identical to Timeout; the variant exists for
                // later subscriptions. This arm is also where a buffer-only
                // coalesce lands (it synthesizes Timeout) and where a dead
                // reader is detected every ~wait — see the reader_done gate.
                Ok(Message::Tick(_)) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    // MVU Phase 3a: the watcher closure + git forwarder each
                    // hold a `msg_tx` clone, so the channel stays Connected
                    // after the input reader dies — the `Disconnected` arm
                    // below no longer fires on reader death (it would spin on
                    // Timeout forever and never surface the fatal read error).
                    // Detect reader death here instead, preserving the prior
                    // `event::read()?` contract: propagate a recorded fatal
                    // error, else exit cleanly.
                    if reader_done.load(std::sync::atomic::Ordering::Acquire) {
                        return take_reader_result(&read_err);
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    // Defensive fallback: every `msg_tx` clone dropped (no
                    // watcher, no forwarder, reader gone). Same contract —
                    // propagate a recorded fatal error; else a clean stop.
                    return take_reader_result(&read_err);
                }
                // Phase 3a: FsEvent/GitResult are buffered + coalesced in the
                // pre-step above, never surfaced as `effective`.
                Ok(
                    Message::FsEvent(_)
                    | Message::GitResult(_)
                    | Message::Mcp(_)
                    | Message::PaneOutput { .. }
                    | Message::SinkOutput { .. }
                    | Message::GrepOutput
                    | Message::FindOutput
                    | Message::ReaderExited
                    | Message::AgentStatusReady,
                ) => {
                    unreachable!(
                        "buffered/collapsed message surfaced as `effective` from the coalesce pre-step"
                    )
                }
            }

            // MVU Phase 2: clock for POST-recv timers (activity rollover,
            // context-write), captured AFTER the recv sleep — matching
            // their old live `.elapsed()` read position (a stale top-of-loop
            // clock would defer them by up to the full wait).
            let now_post = std::time::Instant::now();

            // Activity monitor: roll over the 1-second window (snapshot +
            // reset + proc-stat refresh; see `roll_activity_window`). Returns
            // whether an overlay-only redraw is warranted this tick.
            let activity_only_draw = self.roll_activity_window(now_post, draw.dirty);
            if activity_only_draw {
                draw.set_dirty();
            }

            // Re-arm the post-recv advisory deadlines — ActivityRollover +
            // CaptureTick (see `arm_post_recv_deadlines`).
            self.arm_post_recv_deadlines(now_post, &mut scheduler);

            // Only redraw when something actually changed.
            // Wrap in DEC 2026 synchronized update so the terminal
            // emulator (iTerm2, etc.) buffers the entire frame and
            // paints it atomically — eliminates tearing and reduces
            // terminal-side CPU.
            if draw.dirty {
                draw.dirty = false;
                // Title compose + dedup stay loop-side; only the
                // `term_title::set` IO runs through the sole executor.
                let title_fx: Vec<Effect> = self.term_title_effect().into_iter().collect();
                self.run_effects(title_fx, terminal, &foreground_exec)?;
                use crossterm::terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate};
                let _ = crossterm::execute!(terminal.backend_mut(), BeginSynchronizedUpdate);
                if pending_clear {
                    terminal.clear()?;
                }
                let draw_start = std::time::Instant::now();
                let frame_area = terminal
                    .draw(|frame| {
                        // Time just the buffer build (CPU) so we can separate
                        // it from the diff + tty emission measured below.
                        let render_start = std::time::Instant::now();
                        self.render(frame);
                        if self.view.show_activity {
                            let us = u64::try_from(render_start.elapsed().as_micros())
                                .unwrap_or(u64::MAX);
                            self.view.activity_render_peak_us =
                                self.view.activity_render_peak_us.max(us);
                        }
                    })?
                    .area;
                // Whole-frame peak (build + diff + emission) = the full
                // main-thread render stall. `frame - render` ≈ diff + emission.
                if self.view.show_activity {
                    let us = u64::try_from(draw_start.elapsed().as_micros()).unwrap_or(u64::MAX);
                    self.view.activity_frame_peak_us = self.view.activity_frame_peak_us.max(us);
                }
                let _ = crossterm::execute!(terminal.backend_mut(), EndSynchronizedUpdate);
                if self.view.show_activity && !activity_only_draw {
                    self.view.activity_draws += 1;
                    self.view.activity_bytes +=
                        u64::from(frame_area.width) * u64::from(frame_area.height);
                    match draw.reason {
                        1 => self.view.activity_reason_pane += 1,
                        2 => self.view.activity_reason_event += 1,
                        _ => self.view.activity_reason_other += 1,
                    }
                }
                draw.reason = 0;
            }

            // Only re-sync the filesystem watcher when the cwd actually changed.
            if watched_listing.as_deref() != Some(self.state.listing.dir.as_path()) {
                sync_listing_watch(
                    fs_watcher.as_mut(),
                    &mut watched_listing,
                    &mut watched_git,
                    &self.state.listing.dir,
                    self.state.current_gitdir.as_deref(),
                );
            }

            // Event-driven MCP context-file write — debounced + typing-burst
            // suppressed, with ContextWrite deadline arming (see
            // `maybe_write_context`).
            self.maybe_write_context(
                now_post,
                last_input_at,
                &mut last_context_write,
                &mut scheduler,
            );
        }
        // Clean up the context file on exit.
        crate::context::remove_context_file(&self.view.context_path);
        // Tear down every pane child tree before App is dropped.
        // The per-Pane Drop is a SIGKILL safety net; going through
        // `shutdown` here first sends SIGTERM with a 250ms grace, so
        // well-behaved children (`vite`, `npm run dev`, anything that
        // catches SIGTERM) get a chance to flush state before we
        // escalate. Without this, quitting spyc with a frontend dev
        // server in a pane would leave the whole node/esbuild/worker
        // tree orphaned and still bound to its port.
        if let Some(tabs) = self.runtime.pane_tabs.as_mut() {
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
        if let Some(name) = path.file_name().and_then(|n| n.to_str())
            && name.starts_with(".spyc-context-")
        {
            return false;
        }
        let dir = self.state.listing.dir.as_path();

        // `.git/` filtering against the repo's *resolved* gitdir
        // (cached on chdir). For a normal repo that's `<root>/.git`;
        // for a linked worktree it's `<main>/.git/worktrees/<name>/`
        // (the `.git` here is a *file*, and the real index/HEAD live
        // outside the working tree). macOS FSEvents sometimes coalesces
        // intra-directory changes into a single event whose path *is*
        // the gitdir itself, so accept that as "something happened in
        // there, refresh." Direct children: only `index` (staging/
        // status) or `HEAD` (branch switch) -- everything else (objects,
        // packs, lockfiles, gc activity, refs/, logs/) is rejected so
        // background git housekeeping doesn't cascade.
        if let Some(git_dir) = self.state.current_gitdir.as_deref() {
            if path == git_dir {
                return true;
            }
            if path.starts_with(git_dir) {
                if path.parent() == Some(git_dir)
                    && let Some(name) = path.file_name().and_then(|n| n.to_str())
                {
                    return matches!(name, "index" | "HEAD");
                }
                return false;
            }
        }

        // Anywhere at or below the listing dir (recursive watch) --
        // accept. The 500ms trailing debounce + git-status's index-
        // cache mean even noisy subtrees don't produce unbounded
        // refresh subprocesses.
        path.starts_with(dir)
    }

    /// Status-bar agent segment text for the active pane, or `None`
    /// when no pane is open or the pane isn't a known agent.
    ///
    /// Renders as `claude:<8-hex>` / `gemini:<8-hex>` / `codex` (the
    /// short-id resolution for codex is a follow-up — its rollout
    /// filenames encode the UUID but we don't parse them yet).
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
                    let bg_tag = if self.runtime.pane_tabs.is_some() {
                        String::new()
                    } else {
                        let running = self.runtime.background_tasks.running_count();
                        let done = self.runtime.background_tasks.done_count();
                        if running == 0 && done == 0 {
                            String::new()
                        } else if done == 0 {
                            format!(" bg:{running}\u{25cf}")
                        } else {
                            format!(" bg:{running}\u{25cf}{done}\u{2713}")
                        }
                    };
                    let sort_tag = format!(
                        " sort:{}{}",
                        self.state.sort_order,
                        if self.state.sort_reversed {
                            "\u{2191}"
                        } else {
                            ""
                        },
                    );
                    format!(
                        "[picks:{} inv:{} m1:{} m2:{}{}{}{}{}]",
                        self.state.picks.len(),
                        self.state.inventory.len(),
                        on_off(self.state.masks.mask1.enabled),
                        on_off(self.state.masks.mask2.enabled),
                        filter_tag,
                        hidden_tag,
                        sort_tag,
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
        let delete_preview: Option<&Vec<PathBuf>> = self.state.pending_delete_preview.as_ref();
        self.state
            .rows
            .iter()
            .map(|rd| {
                let git_status = self
                    .state
                    .git
                    .files
                    .get(&rd.display)
                    .copied()
                    .unwrap_or_else(GitFileStatus::clean);
                let pending_delete =
                    delete_preview.is_some_and(|v| v.iter().any(|p| p == &rd.path));
                Row {
                    display: rd.display.clone(),
                    kind: rd.kind,
                    picked: self.state.view == View::Dir && self.state.picks.contains(&rd.path),
                    taken: self.state.inventory.contains(&rd.path),
                    git_status,
                    pending_delete,
                }
            })
            .collect()
    }

    // --- Input handling ---------------------------------------------------

    /// Build the routing snapshot used by `route::route_key`.
    /// Pure read of the fields the router cares about.
    fn route_snapshot(&self) -> route::RouteSnapshot {
        route::RouteSnapshot {
            is_prompting: matches!(self.state.mode, Mode::Prompting(_)),
            has_top_overlay: self.runtime.top_overlay.is_some(),
            pager_mount: self.view.pager.as_ref().map(|v| v.mount),
            has_pane_tabs: self.runtime.pane_tabs.is_some(),
            pane_focused: self.state.pane_focused(),
            // MVU Phase 5: read from the Model snapshot (refreshed at
            // loop-top), not the live host — decouples routing from Runtime.
            pane_scrolling: self.state.pane_snapshot.is_scrolling,
            pane_closed: self.state.pane_snapshot.is_closed,
            resolver_pending: self.state.resolver.is_pending(),
        }
    }

    /// Tab-complete a filesystem path in the prompt buffer. For shell
    /// prompts, completes just the last whitespace-delimited word.
    fn tab_complete_path(&mut self) {
        // Extract data from prompt without holding the borrow.
        let (is_shell, is_jump, is_command, buffer) = {
            let Mode::Prompting(ref prompt) = self.state.mode else {
                return;
            };
            let is_shell = matches!(
                prompt.kind,
                PromptKind::ShellCmd | PromptKind::ShellCmdCaptured | PromptKind::Command
            );
            let is_jump = matches!(prompt.kind, PromptKind::Jump);
            let is_command = matches!(prompt.kind, PromptKind::Command);
            (is_shell, is_jump, is_command, prompt.buffer.clone())
        };

        // Repeated Tab with active cycle state: cycle through matches
        // or re-flash the list for local dirs.
        if let Some(ref mut ts) = self.view.tab_state
            && (ts.original_buf == buffer || ts.cycle_index > 0)
            && ts.matches.len() > 1
        {
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

        // `:` prompt with no whitespace yet — complete the spyc command
        // name from the command registry (`COMMAND_TABLE`) rather than
        // falling through to filesystem completion (which would try to match
        // a file starting with "pa" in cwd, almost never useful here).
        if is_command && !buffer.contains(char::is_whitespace) {
            self.tab_complete_spyc_command(&buffer);
            return;
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
                self.view.tab_state = Some(TabState {
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
            self.view.tab_state = Some(TabState {
                original_buf: prompt.buffer.clone(),
                buf_prefix,
                word_base,
                matches,
                cycle_index: 0,
            });
        } else {
            self.view.tab_state = None;
        }
    }

    /// Tab-complete a `:` command base name from the command registry
    /// ([`crate::app::state::COMMAND_TABLE`]). Single match: fill the name
    /// plus a trailing space (so the user can keep typing args, or hit Enter
    /// for the no-arg form — `dispatch_command` trims). Common-prefix advance:
    /// fill the shared prefix and flash a count. Otherwise show all matches
    /// and stage cycle state for repeated Tab.
    fn tab_complete_spyc_command(&mut self, prefix: &str) {
        let matches: Vec<String> = crate::app::state::completion_command_names()
            .filter(|c| c.starts_with(prefix))
            .map(str::to_string)
            .collect();
        if matches.is_empty() {
            return;
        }

        if matches.len() == 1 {
            let buffer = format!("{} ", matches[0]);
            let Mode::Prompting(ref mut prompt) = self.state.mode else {
                return;
            };
            if let Some(ed) = prompt.editor.as_mut() {
                ed.set_content(&buffer);
            }
            prompt.buffer = buffer;
            self.view.tab_state = None;
            return;
        }

        let common = common_prefix(&matches);
        if common.len() > prefix.len() {
            // Filled some chars but more matches remain — stage cycle
            // state so a follow-up Tab on the same buffer can rotate.
            let display: Vec<&str> = matches.iter().map(String::as_str).collect();
            let shown = if display.len() > 12 {
                format!(
                    "{}  (+{} more)",
                    display[..12].join("  "),
                    display.len() - 12
                )
            } else {
                display.join("  ")
            };
            self.state.flash_info(format!("{shown}  — Tab to cycle"));
            let Mode::Prompting(ref mut prompt) = self.state.mode else {
                return;
            };
            if let Some(ed) = prompt.editor.as_mut() {
                ed.set_content(&common);
            }
            prompt.buffer = common;
            self.view.tab_state = Some(TabState {
                original_buf: prompt.buffer.clone(),
                buf_prefix: String::new(),
                word_base: String::new(),
                matches,
                cycle_index: 0,
            });
            return;
        }

        // No textual progress — leave the buffer alone, show all
        // matches, and stage cycle state. The cycle path on the next
        // Tab will compare `original_buf == buffer` (true since we
        // didn't change the buffer) and rotate.
        let display: Vec<&str> = matches.iter().map(String::as_str).collect();
        let shown = if display.len() > 12 {
            format!(
                "{}  (+{} more)",
                display[..12].join("  "),
                display.len() - 12
            )
        } else {
            display.join("  ")
        };
        self.state.flash_info(format!("{shown}  — Tab to cycle"));
        self.view.tab_state = Some(TabState {
            original_buf: prefix.to_string(),
            buf_prefix: String::new(),
            word_base: String::new(),
            matches,
            cycle_index: 0,
        });
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
            self.view.tab_state = None;
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
            self.view.tab_state = Some(TabState {
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
        self.view.tab_state = None;
        // Clear any stashed state from the two-step new-tab prompt.
        self.state.pending_new_tab_cmd = None;
    }

    /// Dispatch a submitted prompt.
    ///
    /// Pure-domain arms are handled by `AppState::dispatch_prompt`;
    /// terminal-touching arms (shell, pager, overlay, copy/move) stay here.
    #[allow(clippy::needless_pass_by_value)]
    fn dispatch_prompt(&mut self, prompt: Prompt) -> Vec<Effect> {
        use state::PromptResult;

        // Clear any Tab-applied filter before dispatching.
        if self.state.temp_filter.is_some() {
            self.state.temp_filter = None;
            self.state.rebuild_rows();
        }
        self.view.tab_state = None;

        // Try the pure-domain handler first.
        match self.state.dispatch_prompt(&prompt.kind, &prompt.buffer) {
            PromptResult::Handled => {
                // Some pure-domain prompts shift PROJECT_HOME (e.g.
                // `WorktreeNewBranch` chdirs into the new worktree
                // and re-anchors). `apply`'s post-action
                // reconciliation only fires for `Action` dispatches,
                // not prompt submissions — call directly so harpoon
                // reloads on prompts that move us between project
                // roots. The call is cheap when project_home is
                // unchanged (compares paths and returns early).
                self.reconcile_harpoon();
                return Vec::new();
            }
            PromptResult::NotHandled => {}
        }

        // --- Terminal-touching arms ---
        match prompt.kind {
            PromptKind::ShellCmd => {
                let expanded = shell::expand_percent(&prompt.buffer, &self.state.selection_paths());
                let (rows, cols) = Self::top_overlay_size(
                    self.effective_pane_pct(),
                    self.runtime.pane_tabs.is_some(),
                );
                let cwd = self.state.listing.dir.clone();
                let wake = self.make_pane_wake();
                match Pane::spawn(&expanded, rows, cols, &cwd, &self.view.context_path, wake) {
                    Ok(p) => {
                        self.runtime.top_overlay = Some(p);
                        self.state.focus = state::Focus::Overlay;
                    }
                    Err(e) => self.state.flash_error(format!("spawn: {e}")),
                }
                Vec::new()
            }
            PromptKind::ShellCmdCaptured => {
                let cmd = if prompt.buffer.trim() == "!" {
                    if let Some(c) = self.state.last_captured_cmd.clone() {
                        c
                    } else {
                        self.state.flash_error("no previous ! command");
                        return Vec::new();
                    }
                } else {
                    prompt.buffer.clone()
                };
                self.state.last_captured_cmd = Some(cmd.clone());
                let expanded = shell::expand_percent(&cmd, &self.state.selection_paths());
                self.start_capture(&expanded, &cmd, &prompt.buffer);
                Vec::new()
            }
            PromptKind::CopyTo => {
                self.run_selection_to(&prompt.buffer, fs::ops::copy_selection_to, "copied");
                Vec::new()
            }
            PromptKind::MoveTo => {
                self.run_selection_to(&prompt.buffer, fs::ops::move_selection_to, "moved");
                Vec::new()
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
                Vec::new()
            }
            PromptKind::PaneRenameTab => {
                let name = prompt.buffer.trim().to_string();
                if !name.is_empty()
                    && let Some(tabs) = self.runtime.pane_tabs.as_mut()
                {
                    tabs.active_info_mut().label = name;
                }
                Vec::new()
            }
            PromptKind::NewFile => {
                let name = prompt.buffer.trim().to_string();
                if name.is_empty() {
                    return Vec::new();
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
                    return Vec::new();
                }
                let program = argv.remove(0);
                argv.push(resolved.to_string_lossy().into_owned());
                PostAction::Spawn {
                    program,
                    args: argv,
                    pause_after: false,
                }
                .into()
            }
            PromptKind::Command => self.dispatch_command(&prompt.buffer),
            // These should have been handled by AppState — unreachable in practice.
            _ => Vec::new(),
        }
    }

    /// Hide / show the bottom pane (or spawn it on first use).
    ///
    /// Three states:
    ///   1. No tabs yet (`pane_tabs.is_none()`): spawn the default
    ///      command. Same as before.
    ///   2. Tabs exist and visible: flip `pane_hidden = true`. The
    ///      child ptys keep running; render skips the pane area;
    ///      `pane_focused` parks at false so keystrokes go to the
    ///      file list. SIGWINCH is held off — the next show pass
    ///      restores the prior pane geometry.
    ///   3. Tabs exist and hidden: flip `pane_hidden = false`,
    ///      restore focus to the pane. Children pick up wherever
    ///      they left off.
    ///
    /// Why this is hide-don't-kill: previously toggle was destructive
    /// (`pane_tabs = None`, `Drop for PtyHost` → SIGKILL on the
    /// claude process group). Daily-drivers reported losing their
    /// in-flight conversation every time they wanted the whole
    /// screen for a few seconds. Explicit kill of a tab still goes
    /// through `^a-x` (`PaneCloseTab`); destroying the *whole*
    /// pane container is now a multi-step intentional act (close
    /// each tab via `^a-x`), not a one-keystroke side effect.
    fn toggle_pane(&mut self) {
        if self.runtime.pane_tabs.is_some() {
            self.state.pane_hidden = !self.state.pane_hidden;
            self.view.needs_full_repaint = true;
            if self.state.pane_hidden {
                // Park focus on the list while hidden. Keystrokes
                // can't drive an off-screen pane sensibly. Zoom is
                // mutually exclusive with hidden — clear it so a
                // re-show doesn't try to render zoomed onto a
                // newly-resized area.
                self.state.focus = state::Focus::FileList;
                self.state.pane_zoomed = false;
                self.state.pane_focus_before_zoom = None;
                self.state.flash_info("pane hidden — F10/^a-\\ to show");
            } else {
                // Re-show: focus the pane so the next keystroke
                // lands in the child. Matches the "I'm opening this
                // because I want to interact with it" intent.
                self.state.focus = state::Focus::Pane;
                self.state.flash_info("pane shown");
            }
            return;
        }
        // No pane container exists — `^a-\` is a pure hide/show
        // toggle, not a create binding. Previously this silently
        // spawned the default command ($SPYC_PANE_CMD or `claude`),
        // which surprised users expecting a no-op (reported by
        // Justin: "I see ^a-c defaults to claude, but POLA"). Point
        // the user at the explicit creation binding instead.
        self.state.flash_info("no pane — ^a-c to create one");
    }

    /// Spawn a new pane tab. If no tabs exist, creates the container.
    /// Default cwd is the current listing dir — i.e. "open here",
    /// matching the user's mental model of where they're browsing.
    /// Reported by Justin: F9 (`ResumePane`) used to spawn at
    /// `PROJECT_HOME` (typically the dir spyc was launched from)
    /// instead of the dir he had since navigated into. `^a-c`
    /// already pre-fills its cwd prompt with `listing.dir`; this
    /// brings the bare-spawn path in line.
    ///
    /// Users who want a specific anchor should use `^a-c` and edit
    /// the prompt, or invoke `:project` to move PROJECT_HOME.
    fn open_pane_tab(&mut self, cmd: &str) {
        let cwd = self.state.listing.dir.clone();
        self.open_pane_tab_in(cmd, &cwd);
    }

    fn open_pane_tab_in(&mut self, cmd: &str, cwd: &std::path::Path) {
        let (rows, cols) = Self::pane_spawn_size(
            self.effective_pane_pct(),
            self.state.config.layout.status_position,
        );
        let wake = self.make_pane_wake();
        match Pane::spawn_with_env(cmd, rows, cols, cwd, &self.view.context_path, &[], wake) {
            Ok(p) => {
                self.state.focus = state::Focus::Pane;
                self.state
                    .flash_info(format!("pane: {cmd} (^W k for list)"));
                let entry = TabEntry::new(p, TabInfo::new(cmd, cwd));
                if let Some(tabs) = self.runtime.pane_tabs.as_mut() {
                    tabs.push(entry);
                } else {
                    self.runtime.pane_tabs = Some(PaneTabs::new(entry));
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
        let id = self.runtime.next_grep_id;
        self.runtime.next_grep_id = self.runtime.next_grep_id.wrapping_add(1);
        let (tx, rx) = std::sync::mpsc::channel();
        let walk_root = root.clone();
        let pat = pattern.to_string();
        let pat_for_thread = pat.clone();
        // MVU Phase 3d: wake the loop on each batch (via WakingSender) and
        // once more after the worker returns — that final wake drives the
        // last drain_grep_session, which sees the rx disconnect and marks the
        // session complete (title loses "scanning…") with no poll floor.
        let wake = self.make_grep_wake();
        let final_wake = std::sync::Arc::clone(&wake);
        let tx = crate::fs::WakingSender::new(tx, wake);
        std::thread::spawn(move || {
            let _ = crate::fs::grep::search_streaming(&walk_root, &pat_for_thread, tx);
            final_wake();
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
        // user can `:bprev` to it. Save its scroll first so the
        // position survives a crash before the user `:bprev`s back.
        self.remember_pager_position();
        if let Some(prev) = self.view.pager.take() {
            self.view.pager_history.push(prev);
        }
        self.set_pager(view);
        self.runtime.grep_session = Some(GrepSession {
            id,
            rx,
            count: 0,
            complete: false,
            capped: false,
            pattern: pat,
            root,
        });
        self.view.needs_full_repaint = true;
    }

    /// Drain any pending grep matches into the active pager. Called
    /// from the tick loop. Returns true when something changed
    /// (matches appended or worker completed) so the caller can
    /// request a redraw.
    fn drain_grep_session(&mut self) -> bool {
        let Some(session) = self.runtime.grep_session.as_mut() else {
            return false;
        };
        // Drop the session if the matching pager is gone. The user
        // closed/replaced it; the worker keeps running but will exit
        // on its next send when our rx is dropped.
        let pager_matches = self
            .view
            .pager
            .as_ref()
            .is_some_and(|p| p.grep_id == Some(session.id));
        if !pager_matches {
            self.runtime.grep_session = None;
            return false;
        }
        let mut got_any = false;
        loop {
            match session.rx.try_recv() {
                Ok(batch) => {
                    if let Some(view) = self.view.pager.as_mut() {
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
            if let Some(view) = self.view.pager.as_mut() {
                view.title = new_title;
                if session.complete {
                    view.streaming = false;
                }
            }
            if session.complete {
                self.runtime.grep_session = None;
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
        // MVU Phase 3d: wake the loop on each candidate batch (via
        // WakingSender) and once more after the walk returns — that final
        // wake drives the last drain_walk, which sees the rx disconnect and
        // flips `walk_complete` (title → final count) without the poll floor.
        let wake = self.make_find_wake();
        let final_wake = std::sync::Arc::clone(&wake);
        let tx = crate::fs::WakingSender::new(tx, wake);
        std::thread::spawn(move || {
            crate::fs::finder::walk_streaming(&walk_root, tx);
            final_wake();
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
        self.runtime.find_picker = Some(picker);
        self.render_find_picker();
        self.view.needs_full_repaint = true;
    }

    /// Rebuild the pager view from current `find_picker` state.
    /// Called on open, after each keystroke that mutates the query
    /// or selection, and after each tick where the streaming walk
    /// produced new candidates (title shows progress).
    fn render_find_picker(&mut self) {
        let Some(picker) = self.runtime.find_picker.as_ref() else {
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
        self.set_pager(view);
    }

    /// Intercept keys when the F-finder is open. Returns true when
    /// the key was consumed by the picker (so the caller skips
    /// normal pager / file-list dispatch). Esc closes; Enter chdirs
    /// to the matched file's parent and places the cursor on it;
    /// Up/Down move selection; printable chars + Backspace edit
    /// the query and re-rank.
    fn handle_find_picker_key(&mut self, key: KeyEvent) -> bool {
        if self.runtime.find_picker.is_none() {
            return false;
        }
        match key.code {
            KeyCode::Esc => {
                self.runtime.find_picker = None;
                self.clear_pager();
                self.view.needs_full_repaint = true;
                true
            }
            KeyCode::Enter => {
                let target = self.runtime.find_picker.as_ref().and_then(|p| {
                    p.filtered
                        .get(p.selected)
                        .cloned()
                        .map(|rel| (p.root.clone(), rel))
                });
                self.runtime.find_picker = None;
                self.clear_pager();
                self.view.needs_full_repaint = true;
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
                if let Some(picker) = self.runtime.find_picker.as_mut()
                    && picker.selected > 0
                {
                    picker.selected -= 1;
                    self.render_find_picker();
                }
                true
            }
            KeyCode::Down => {
                if let Some(picker) = self.runtime.find_picker.as_mut()
                    && picker.selected + 1 < picker.filtered.len()
                {
                    picker.selected += 1;
                    self.render_find_picker();
                }
                true
            }
            KeyCode::Backspace => {
                if let Some(picker) = self.runtime.find_picker.as_mut()
                    && !picker.query.is_empty()
                {
                    picker.query.pop();
                    picker.refilter();
                    self.render_find_picker();
                }
                true
            }
            KeyCode::Char(c)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                if let Some(picker) = self.runtime.find_picker.as_mut() {
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
        if row.kind == EntryKind::Dir
            || (row.kind == EntryKind::Symlink && crate::fs::target_is_dir(&path))
        {
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
            Self::top_overlay_size(self.effective_pane_pct(), self.runtime.pane_tabs.is_some());
        let cwd = self.state.listing.dir.clone();
        let wake = self.make_pane_wake();
        match Pane::spawn(&cmd, rows, cols, &cwd, &self.view.context_path, wake) {
            Ok(p) => {
                self.runtime.top_overlay = Some(p);
                self.state.focus = state::Focus::Overlay;
            }
            Err(e) => self.state.flash_error(format!("spawn: {e}")),
        }
    }

    /// `D` — open the cursor file in spyc's in-app pager mounted in
    /// the top-pane slot, so the bottom pane (claude / zsh / etc.)
    /// stays visible. Mirror of `edit_in_pane` for the read path.
    /// Common workflow: `D` on a doc, `^a-j` into claude, work,
    /// `^a-k` back to scroll.
    ///
    /// v1.5 Phase 5 swapped the implementation from
    /// "spawn `\$PAGER` as a pty top overlay" to "use the in-app
    /// pager." The pager is more capable on every axis we care about
    /// (search, jump, syntax highlighting, range yank, markdown
    /// render, hex dump for binaries), and uses the existing
    /// `Mount::TopPane` rail laid in Phase 1.
    ///
    /// **Huge-file fallback:** files past `MAX_PAGER_BYTES` are
    /// still handed to `\$PAGER` as a top overlay because `less`
    /// streams from disk while the in-app pager loads the (already
    /// truncated) buffer into memory. Streaming wins for multi-GB
    /// logs.
    fn display_in_pane(&mut self) {
        let Some(row) = self.state.rows.get(self.state.cursor.index) else {
            return;
        };
        let path = row.path.clone();
        if row.kind == EntryKind::Dir
            || (row.kind == EntryKind::Symlink && crate::fs::target_is_dir(&path))
        {
            self.state.flash_error("D: cannot page a directory");
            return;
        }
        let file_size = std::fs::metadata(&path).map_or(0, |m| m.len());
        if file_size > crate::fs::ops::MAX_PAGER_BYTES {
            // Huge file: $PAGER's stream-from-disk wins over our
            // in-memory pager. Fall back to the pre-v1.5 behavior
            // (spawn $PAGER as a top overlay).
            self.spawn_pager_overlay_for_path(&path);
            return;
        }
        let Some(mut view) = self.build_pager_view_for_file(&path) else {
            return;
        };
        view.mount = crate::ui::pager::Mount::TopPane;
        // Don't push to buffer history: this is a fresh open, not a
        // page the user navigated away from and might want to revisit
        // via `[b` / `]b`.
        view.no_history = true;
        self.set_pager(view);
        self.state.focus = state::Focus::Pager(pager::Mount::TopPane);
        self.view.needs_full_repaint = true;
    }

    /// Build a `PagerView` from a file on disk. Handles text (with
    /// markdown rendering / syntax highlighting / truncation banner
    /// for big files) and binary (hex dump). Flashes a read error
    /// and returns `None` on failure. The returned view has
    /// `mount = Overlay` (the default); callers override for
    /// `TopPane` / `LowerPane` mounts. Extracted from the old
    /// inline body of `ActivateIntent::Display` so both `Enter` /
    /// `d` (overlay) and `D` (top pane) share the same loading
    /// path.
    fn build_pager_view_for_file(&mut self, path: &Path) -> Option<PagerView> {
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        if shell::looks_like_text(path) {
            let file_size = std::fs::metadata(path).map_or(0, |m| m.len());
            // Big files used to OOM us: read_to_string + syntect every
            // token = file size × ~50 in pager state. Cap at
            // MAX_PAGER_BYTES; past that, load just MAX_PAGER_LINES of
            // plain text and tell the user how to hand off to $PAGER
            // for the full thing.
            let load_result = if file_size > crate::fs::ops::MAX_PAGER_BYTES {
                crate::fs::ops::read_truncated(path, crate::fs::ops::MAX_PAGER_LINES)
            } else {
                std::fs::read_to_string(path).map(|c| {
                    let n = c.lines().count();
                    (c, n, false)
                })
            };
            let (content, _line_count, truncated) = match load_result {
                Ok(t) => t,
                Err(e) => {
                    self.state.flash_error(format!("read: {e}"));
                    return None;
                }
            };
            let content = expand_tabs(&content);
            let is_md = crate::ui::markdown::is_markdown_path(path);
            // JSON pretty-print: try parse + canonical re-emit. On a
            // successful parse with output differing from the raw
            // bytes, `lines` holds the pretty version and `alt_lines`
            // holds the raw (`m` toggles). Re-uses the alt-view
            // machinery currently named for markdown (`alt_lines`,
            // `markdown_rendered`); a rename to a generic name is
            // queued for the folding work in v1.50.73.
            let json_pretty: Option<String> = if !truncated && crate::ui::json::is_json_path(path) {
                crate::ui::json::pretty_print(&content)
            } else {
                None
            };
            // Source-side lines: syntect-highlighted if available AND
            // we loaded the whole file (highlighting a partial file
            // would still mostly work but blows memory, and the
            // savings is the whole point of truncation).
            let source_lines: Vec<ratatui::text::Line<'static>> = if truncated {
                content
                    .lines()
                    .map(|l| ratatui::text::Line::from(l.to_string()))
                    .collect()
            } else {
                crate::ui::syntax::highlight_to_lines(&name, &content).unwrap_or_else(|| {
                    content
                        .lines()
                        .map(|l| ratatui::text::Line::from(l.to_string()))
                        .collect()
                })
            };
            let mut view = if let Some(pretty) = json_pretty {
                // Pretty differs from raw: build a styled view of the
                // pretty bytes, stash the (already-highlighted) raw
                // lines as alt for the `m` toggle.
                let pretty_lines: Vec<ratatui::text::Line<'static>> =
                    crate::ui::syntax::highlight_to_lines(&name, &pretty).unwrap_or_else(|| {
                        pretty
                            .lines()
                            .map(|l| ratatui::text::Line::from(l.to_string()))
                            .collect()
                    });
                let mut v = PagerView::new_styled(name.clone(), pretty_lines);
                if pretty != content {
                    v.alt_lines = Some(source_lines);
                    // `markdown_rendered = true` semantically means
                    // "the processed/alt-form is in `lines`". Same
                    // interpretation for JSON: pretty is "rendered".
                    v.markdown_rendered = true;
                }
                v
            } else if is_md && !truncated {
                // Pre-compute both views; `m` toggles. Yank/save
                // always hit the source via `source_text()`. Which
                // view shows first is configurable via
                // `[markdown] open_as_rendered`. Skipped for
                // truncated files since markdown rendering of half a
                // doc looks weird (broken refs, half-closed code
                // fences).
                // Hint the markdown renderer at the actual pager body
                // width so wide tables expand instead of wrapping into
                // the 80-col prose budget. Centered overlay pager
                // claims 90% of the terminal minus block borders;
                // matches the `pager_inner_area` math.
                //
                // Subtract the projected line-number gutter so a wide
                // table doesn't overflow the right edge of the
                // viewport. The gutter is `ilog10(lines) + 2` cells
                // wide (see `pager::render`); we don't yet know the
                // RENDERED line count (it can exceed the source's
                // because of soft-break-as-hard-break + table
                // expansion), so use 4× the source count as a
                // conservative estimate, which buys ~1 digit of
                // safety on the gutter.
                let (term_w, _) = crossterm::terminal::size().unwrap_or((80, 24));
                let body_w = crate::ui::pager::centered_body_width(term_w) as usize;
                let source_line_count = content.lines().count().max(1);
                let gutter_w = (source_line_count.saturating_mul(4)).max(1).ilog10() as usize + 2;
                let pager_w = body_w.saturating_sub(2 + gutter_w);
                let rendered =
                    crate::ui::markdown::render(&content, &self.view.theme, Some(pager_w));
                if self.state.config.markdown.open_as_rendered {
                    let mut v = PagerView::new_styled(name, rendered);
                    v.alt_lines = Some(source_lines);
                    v.markdown_rendered = true;
                    v
                } else {
                    // Source first: `lines` holds source, `alt_lines`
                    // holds the rendered view, `markdown_rendered`
                    // is false. `m` swap is symmetric.
                    let mut v = PagerView::new_styled(name, source_lines);
                    v.alt_lines = Some(rendered);
                    v.markdown_rendered = false;
                    v
                }
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
                    // Append a banner row pointing at the escape
                    // hatch so the user knows the cap fired and what
                    // to do.
                    let warn_style = ratatui::style::Style::default()
                        .fg(self.view.theme.pick)
                        .add_modifier(ratatui::style::Modifier::BOLD);
                    v.lines.push(ratatui::text::Line::from(""));
                    v.lines
                        .push(ratatui::text::Line::from(ratatui::text::Span::styled(
                            format!(
                                "[truncated at {} lines · {} MB total · press p to open in $PAGER]",
                                crate::fs::ops::MAX_PAGER_LINES,
                                file_size / (1024 * 1024)
                            ),
                            warn_style,
                        )));
                    // Also flash an immediate hint — the banner is at
                    // the bottom and the user might not scroll there
                    // before wondering what happened to their file.
                    v.flash = Some(format!(
                        "truncated at {} lines · press p for full file in $PAGER",
                        crate::fs::ops::MAX_PAGER_LINES
                    ));
                }
                v
            };
            view.source_path = Some(path.to_path_buf());
            // Restore the scroll position from the previous visit (if
            // any). Clamp to `lines.len() - 1` so a saved row that's
            // now past the end (file shrank) lands at the new last
            // line rather than blanking the viewport.
            if let Some(saved) = self.view.pager_positions.get(path) {
                let last = view.lines.len().saturating_sub(1);
                view.scroll = saved.min(u16::try_from(last).unwrap_or(u16::MAX));
            }
            Some(view)
        } else {
            // Binary file: hex dump via pretty-hex.
            match fs::ops::hex_dump_lines(path, &self.view.theme) {
                Ok(lines) => {
                    let mut view = PagerView::new_plain(format!("{name} [hex]"), Vec::new());
                    view.lines = lines;
                    Some(view)
                }
                Err(e) => {
                    self.state.flash_error(format!("hex: {e}"));
                    None
                }
            }
        }
    }

    /// Pre-v1.5 `D` behavior: spawn `\$PAGER` as a top overlay pty.
    /// Now used only as the huge-file fallback path from
    /// `display_in_pane` — files past `MAX_PAGER_BYTES` benefit from
    /// `less`'s stream-from-disk over our in-memory pager.
    fn spawn_pager_overlay_for_path(&mut self, path: &Path) {
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
            Self::top_overlay_size(self.effective_pane_pct(), self.runtime.pane_tabs.is_some());
        let cwd = self.state.listing.dir.clone();
        let wake = self.make_pane_wake();
        match Pane::spawn(&cmd, rows, cols, &cwd, &self.view.context_path, wake) {
            Ok(p) => {
                self.runtime.top_overlay = Some(p);
                self.state.focus = state::Focus::Overlay;
            }
            Err(e) => self.state.flash_error(format!("spawn: {e}")),
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
            Ok(host) => {
                // MVU Phase 3c: install the capture's channel wake (a generic
                // SinkOutput edge — the loop re-scans on it). Survives the
                // ^Z/:fg round-trip with the host, so no re-install there.
                // Self-wake sweeps any bytes that landed during the install
                // window now the floor is gone.
                let wake = self.make_sink_wake();
                host.set_wake(wake);
                host.fire_wake_now();
                let mut view =
                    PagerView::new_plain(format!("\u{23f3} {title} — running... (0s)"), Vec::new());
                view.streaming = true;
                self.set_pager(view);
                self.runtime.pending_capture = Some(PendingCapture {
                    host,
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
        let Some(capture) = self.runtime.pending_capture.take() else {
            return;
        };
        let id = capture
            .original_id
            .unwrap_or_else(|| self.runtime.background_tasks.allocate_id());
        let task = BackgroundTask {
            id,
            title: capture.title,
            cmd_display: capture.cmd_display.clone(),
            host: capture.host,
            buffer: capture.buffer,
            status: TaskStatus::Running,
            started: capture.started,
            finished_at: None,
            has_unread_output: false,
            viewed_in_task_viewer: false,
            paused: false,
        };
        self.runtime.background_tasks.tasks.push(task);
        self.clear_pager();
        self.view.needs_full_repaint = true;
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
    fn pause_task(&mut self, target: Option<u32>) -> Vec<Effect> {
        let Some(id) = target.or_else(|| self.runtime.background_tasks.most_recent()) else {
            self.state.flash_error("no background tasks");
            return Vec::new();
        };
        let Some(task) = self
            .runtime
            .background_tasks
            .tasks
            .iter_mut()
            .find(|t| t.id == id)
        else {
            self.state.flash_error(format!("no task with id {id}"));
            return Vec::new();
        };
        if !matches!(task.status, TaskStatus::Running) {
            self.state.flash_error(format!("task #{id} is not running"));
            return Vec::new();
        }
        if task.paused {
            self.state.flash_info(format!("task #{id} already paused"));
            return Vec::new();
        }
        let Some(pid) = task.host.process_id() else {
            self.state.flash_error(format!("task #{id}: no process id"));
            return Vec::new();
        };
        // SIGSTOP to the process group (negative pid → group; uncatchable,
        // so the child can't refuse; the reader thread keeps blocking on
        // read until SIGCONT). The signal, the `paused` toggle, and the
        // flash all run in `run_effects` (the sole side-effect executor).
        vec![Effect::SignalGroup {
            pid,
            sig: rustix::process::Signal::STOP,
            on_ok: SigOk::Pause(id),
            on_err: format!("task #{id}: SIGSTOP failed"),
        }]
    }

    /// Send SIGINT to a running task's process group. Mirrors what
    /// pressing ^C in a normal terminal does: child's tty driver
    /// delivers SIGINT and the child decides whether to exit
    /// (default) or trap. Returns the human-readable result for the
    /// caller to flash in the right place (pager footer, status
    /// bar, etc.). `None` target = most-recent task.
    fn interrupt_task(&self, target: Option<u32>) -> Result<String, String> {
        let id = target
            .or_else(|| self.runtime.background_tasks.most_recent())
            .ok_or_else(|| "no background tasks".to_string())?;
        let task = self
            .runtime
            .background_tasks
            .tasks
            .iter()
            .find(|t| t.id == id)
            .ok_or_else(|| format!("no task with id {id}"))?;
        if !matches!(task.status, TaskStatus::Running) {
            return Err("process already stopped".to_string());
        }
        let pid = task
            .host
            .process_id()
            .ok_or_else(|| format!("task #{id}: no process id"))?;
        // Negative pid → process group, same convention as
        // pause_task / resume_task. SIGINT lets the child clean up
        // (matches a real ^C); use ^\ in the live capture path for a
        // hard kill.
        kill_pg(pid, rustix::process::Signal::INT)
            .map(|()| format!("task #{id}: sent SIGINT"))
            .map_err(|_| format!("task #{id}: SIGINT failed"))
    }

    /// Resume a paused task with SIGCONT to its process group.
    fn resume_task(&mut self, target: Option<u32>) -> Vec<Effect> {
        let Some(id) = target.or_else(|| self.runtime.background_tasks.most_recent()) else {
            self.state.flash_error("no background tasks");
            return Vec::new();
        };
        let Some(task) = self
            .runtime
            .background_tasks
            .tasks
            .iter_mut()
            .find(|t| t.id == id)
        else {
            self.state.flash_error(format!("no task with id {id}"));
            return Vec::new();
        };
        if !task.paused {
            self.state.flash_info(format!("task #{id} is not paused"));
            return Vec::new();
        }
        let Some(pid) = task.host.process_id() else {
            self.state.flash_error(format!("task #{id}: no process id"));
            return Vec::new();
        };
        // SIGCONT to the process group; the signal, the `paused` toggle,
        // and the flash all run in `run_effects`.
        vec![Effect::SignalGroup {
            pid,
            sig: rustix::process::Signal::CONT,
            on_ok: SigOk::Resume(id),
            on_err: format!("task #{id}: SIGCONT failed"),
        }]
    }

    /// `:task-to-pane [N]` — promote a backgrounded `!` task to a
    /// new pane tab. v1.5 Phase 6b. The pty keeps running through
    /// the transition; we resize it to the bottom-pane geometry
    /// (capture spawned at terminal-rows × terminal-cols, the new
    /// tab slot is usually smaller), replay the captured buffer
    /// through a fresh vt100 parser, wake the child if it was
    /// paused, and register the resulting `Pane` in `pane_tabs`.
    /// `BackgroundTask` lifecycle metadata (status, finished_at,
    /// has_unread_output, viewed_in_task_viewer) is dropped — the
    /// task is now a pane.
    ///
    /// Already-exited tasks are not promoted: `:fg` is the right
    /// route for static output (one-shot view of the captured
    /// buffer), and a dead pty would just immediately tear down
    /// the new tab. Flash-and-skip; the task stays in the bg
    /// list.
    fn promote_task_to_pane(&mut self, target: Option<u32>) {
        let Some(id) = target.or_else(|| self.runtime.background_tasks.most_recent()) else {
            self.state.flash_error("no background tasks");
            return;
        };
        let Some(mut task) = self.runtime.background_tasks.take(id) else {
            self.state.flash_error(format!("no task #{id}"));
            return;
        };
        if !matches!(task.status, TaskStatus::Running) {
            // Re-add so :fg still works. flash_error so the user
            // sees this as a non-action rather than silent.
            self.runtime.background_tasks.tasks.push(task);
            self.state.flash_error(format!(
                "task #{id} already exited; :fg to view its output instead"
            ));
            return;
        }

        // Capture-side TERM was `dumb`; promoting doesn't change
        // that — the child's TERM is set at spawn time. Plain shells
        // and SGR-color output render fine; alt-screen TUIs (vim,
        // htop, lazygit) won't suddenly start working. Document via
        // the flash hint below.

        let (rows, cols) = Self::pane_spawn_size(
            self.effective_pane_pct(),
            self.state.config.layout.status_position,
        );
        // Resize before building the parser so vt100's grid sizes
        // match the child's view. ioctl is best-effort — if it
        // fails, the geometry is just slightly off until the next
        // resize event.
        let _ = task.host.resize(rows, cols);

        // Replay the captured output through a fresh parser so
        // the visible grid reflects what the user saw in the
        // task viewer. Same scrollback budget Pane::spawn_with_env
        // uses (10K rows).
        let mut parser = vt100::Parser::new(rows, cols, 10_000);
        parser.process(&task.buffer);

        // Wake a paused task — it's user-facing now, no point
        // leaving SIGSTOP'd.
        if task.paused
            && let Some(pid) = task.host.process_id()
        {
            #[cfg(unix)]
            let _ = kill_pg(pid, rustix::process::Signal::CONT);
        }

        // If the task viewer was open on this task, close it —
        // the task no longer exists in `background_tasks`, so the
        // viewer's task_id is stale.
        if self
            .view
            .pager
            .as_ref()
            .is_some_and(|v| v.task_id == Some(id))
        {
            self.clear_pager();
        }

        let cmd = task.cmd_display.clone();
        // BackgroundTask doesn't track the original cwd (capture
        // was spawned in `state.listing.dir` at the time but we
        // didn't save it); use current listing dir as a best-effort
        // tab-info field. The child is already running at its own
        // cwd regardless.
        let cwd = self.state.listing.dir.clone();
        let label = task.title.clone();
        let wake = self.make_pane_wake();
        // MVU Phase 3c: clear the task's reader-thread sink wake BEFORE adopt
        // installs the parser-worker PaneWake — the reader is shared, so a
        // leftover sink wake would fire a spurious SinkOutput for what is now
        // a pane (double-wake alongside PaneOutput).
        task.host.clear_wake_slot();
        let pane = Pane::adopt(task.host, parser, wake);
        let mut info = TabInfo::new(&cmd, &cwd);
        info.label.clone_from(&label);
        let entry = TabEntry::new(pane, info);

        self.state.focus = state::Focus::Pane;
        if let Some(tabs) = self.runtime.pane_tabs.as_mut() {
            tabs.push(entry);
            // Switch active tab to the promoted one so the user
            // lands looking at it.
            let last = tabs.len().saturating_sub(1);
            tabs.switch_to(last);
        } else {
            self.runtime.pane_tabs = Some(PaneTabs::new(entry));
        }
        self.view.needs_full_repaint = true;
        self.state.flash_info(format!("task #{id} → tab '{label}'"));
    }

    /// `:pane-to-task` — demote the active pane tab to a
    /// background task. v1.5 Phase 6c. Inverse of
    /// `:task-to-pane`: same `PtyHost` moves between containers,
    /// the pty keeps running through the transition.
    ///
    /// Buffer recovery is the open design call — vim's `^z` to
    /// background loses visual context, and we follow the same
    /// rule: the new task buffer starts empty, fresh output
    /// accumulates from the demote point. We don't seed from
    /// `screen.contents()` because the task buffer is ANSI bytes
    /// (so rebuild via `ansi-to-tui` works) and `screen.contents()`
    /// is plain text — seeding would erase color. Acceptable;
    /// can revisit if users hit it.
    fn demote_pane_to_task(&mut self) {
        let Some(tabs) = self.runtime.pane_tabs.as_mut() else {
            self.state.flash_error("no pane to demote");
            return;
        };
        let Some(entry) = tabs.take_active() else {
            self.state.flash_error("no pane to demote");
            return;
        };
        let TabEntry { pane, info, .. } = entry;
        // MVU Phase 3c PR3: refuse to demote an already-exited pane. Its
        // reader thread has already returned, so the resulting task would
        // never wake or finalize — and with the poll floor gone, nothing
        // would ever flip it out of `Running` (a stuck, invisible task).
        // The tab's already been taken, so dropping `pane` here closes it.
        if pane.is_closed() {
            self.state.flash_info("pane already exited — closed it");
            if self
                .runtime
                .pane_tabs
                .as_ref()
                .is_some_and(PaneTabs::is_empty)
            {
                self.runtime.pane_tabs = None;
                self.state.focus = state::Focus::FileList;
            }
            return;
        }
        // Stop the parser worker and reclaim the byte receiver so
        // the background task's drain still sees raw bytes.
        let host = pane.take_host();
        // MVU Phase 3c: a pane host carries no sink wake (the pane woke via
        // its now-stopped parser worker), so install one for the task — its
        // reader thread is still alive and will fire SinkOutput on bytes/EOF.
        // Self-wake sweeps any bytes delivered during the install window.
        let wake = self.make_sink_wake();
        host.set_wake(wake);
        host.fire_wake_now();
        // If we just took the last tab, drop the container so
        // layout / status / focus revert to "no pane open" state.
        if self
            .runtime
            .pane_tabs
            .as_ref()
            .is_some_and(PaneTabs::is_empty)
        {
            self.runtime.pane_tabs = None;
            self.state.focus = state::Focus::FileList;
        }

        let id = self.runtime.background_tasks.allocate_id();
        let label = info.label.clone();
        let task = BackgroundTask {
            id,
            title: label.clone(),
            cmd_display: info.command,
            host,
            buffer: Vec::new(),
            status: TaskStatus::Running,
            started: info.spawn_at,
            finished_at: None,
            has_unread_output: false,
            viewed_in_task_viewer: false,
            paused: false,
        };
        self.runtime.background_tasks.tasks.push(task);
        self.view.needs_full_repaint = true;
        self.state.flash_info(format!(
            "tab '{label}' → task #{id} (:fg or :task-to-pane to bring back)"
        ));
    }

    fn foreground_task(&mut self, target: Option<u32>) {
        if self.runtime.pending_capture.is_some() {
            self.state
                .flash_error("already in a foreground task — ^Z to send to background first");
            return;
        }
        let Some(id) = target.or_else(|| self.runtime.background_tasks.most_recent()) else {
            self.state.flash_error("no background tasks");
            return;
        };
        let Some(task) = self.runtime.background_tasks.take(id) else {
            self.state.flash_error(format!("no task #{id}"));
            return;
        };

        // If the task was paused, auto-resume on foreground — the
        // user explicitly asked for it to be active again. Without
        // this, `:fg` on a paused task would re-attach the streaming
        // capture but the child would stay frozen.
        if task.paused
            && let Some(pid) = task.host.process_id()
        {
            let _ = kill_pg(pid, rustix::process::Signal::CONT);
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
                self.set_pager(view);
                self.runtime.pending_capture = Some(PendingCapture {
                    host: task.host,
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
                let glyph = if matches!(&status, TaskStatus::Exited(0)) {
                    "\u{2713}" // ✓
                } else {
                    "\u{2717}" // ✗
                };
                let title = format!("{glyph} {} — {status_text} ({elapsed_secs}s)", task.title);
                let mut view = PagerView::new_plain(title, Vec::new());
                view.lines = text.lines;
                view.saveable = true;
                view.scroll_to_bottom_auto();
                self.set_pager(view);
            }
        }
        self.view.needs_full_repaint = true;
    }

    /// Build a "task viewer" pager view -- a peek into a backgrounded
    /// task's buffered output without taking ownership (the way `:fg`
    /// does). The view's `task_id` is set so the main loop can refresh
    /// it from the live buffer while the task is running.
    fn build_task_viewer(&self, id: u32) -> Option<PagerView> {
        let task = self
            .runtime
            .background_tasks
            .tasks
            .iter()
            .find(|t| t.id == id)?;
        Some(Self::build_task_viewer_for(id, task))
    }

    fn build_task_viewer_for(id: u32, task: &BackgroundTask) -> PagerView {
        use ansi_to_tui::IntoText;
        let elapsed = task
            .finished_at
            .map_or_else(|| task.started.elapsed(), |f| f - task.started)
            .as_secs();
        let (glyph, status_text) = match &task.status {
            TaskStatus::Running => ("\u{23f3}", format!("running ({elapsed}s)")), // ⏳
            TaskStatus::Exited(0) => ("\u{2713}", format!("exit 0 ({elapsed}s)")), // ✓
            TaskStatus::Exited(code) => ("\u{2717}", format!("exit {code} ({elapsed}s)")), // ✗
            TaskStatus::Killed => ("\u{2717}", format!("killed ({elapsed}s)")),
            TaskStatus::Crashed(msg) => ("\u{2717}", format!("error: {msg} ({elapsed}s)")),
        };
        let title = format!("{glyph} [task #{id}] {} — {status_text}", task.cmd_display);
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
        let running = matches!(task.status, TaskStatus::Running);
        // Suppress [EOF]/tilde markers while the underlying task is
        // still running -- the buffer is live, not finalized.
        view.streaming = running;
        // Once the task has exited, anchor an EOF marker to the bottom
        // of content so it's visible even when output exceeds the
        // viewport. The render-time [EOF] only appears in unused
        // viewport rows below content.
        if !running {
            view.lines.push(eof_marker_line(&status_text));
            view.eof_in_content = true;
        }
        view.scroll_to_bottom_auto();
        view
    }

    /// `gB` from the file list, or `:task N` colon command. Open the
    /// task viewer for `target` (or the most-recent task if `None`).
    /// Pushes the current pager (if any, and not no_history) to buffer
    /// history first so `[b` can walk back.
    fn open_task_viewer(&mut self, target: Option<u32>) {
        let Some(id) = target.or_else(|| self.runtime.background_tasks.most_recent()) else {
            self.state.flash_error("no background tasks");
            return;
        };
        let Some(view) = self.build_task_viewer(id) else {
            self.state.flash_error(format!("no task #{id}"));
            return;
        };
        // Mark viewed so promotion-to-history can fire on close.
        if let Some(task) = self
            .runtime
            .background_tasks
            .tasks
            .iter_mut()
            .find(|t| t.id == id)
        {
            task.viewed_in_task_viewer = true;
            task.has_unread_output = false;
        }
        // Push the prior pager (if any, eligible) so `[b` can walk back.
        self.remember_pager_position();
        if let Some(prev) = self.view.pager.take() {
            self.view.pager_history.push(prev);
        }
        self.set_pager(view);
        self.view.needs_full_repaint = true;
    }

    /// `[t`/`]t` chord while a pager is open. Cycles the task viewer
    /// among bg tasks ordered by id. `direction = -1` for prev, `+1`
    /// for next.
    fn cycle_task_viewer(&mut self, direction: i32) {
        if self.runtime.background_tasks.tasks.is_empty() {
            self.state.flash_info("no background tasks");
            return;
        }
        let current = self
            .view
            .pager
            .as_ref()
            .and_then(|v| v.task_id)
            .and_then(|id| {
                self.runtime
                    .background_tasks
                    .tasks
                    .iter()
                    .position(|t| t.id == id)
            });
        let next_pos = match current {
            Some(pos) => {
                let n = self.runtime.background_tasks.tasks.len() as i32;
                let raw = pos as i32 + direction;
                ((raw % n + n) % n) as usize
            }
            None => {
                if direction < 0 {
                    self.runtime.background_tasks.tasks.len() - 1
                } else {
                    0
                }
            }
        };
        let id = self.runtime.background_tasks.tasks[next_pos].id;
        self.open_task_viewer(Some(id));
    }

    /// For tabs that have a `pending_resume_send` armed (set by
    /// `restore_session`), drive the two-phase keystroke injection
    /// that recovers a Claude conversation. We avoid the `--resume`
    /// CLI flag because it trips a known regression that crashes at
    /// mount; the slash-command path goes through `tM_` and works
    /// fine.
    ///
    /// Two phases:
    /// - `Text` (after banner-settle): write `/resume <sid>` with no
    ///   trailing Enter and transition to `Enter`.
    /// - `Enter` (after a small additional delay): write `\r`.
    ///
    /// Splitting the writes avoids an intermittent race where
    /// Claude's TUI was still mid-render when the original combined
    /// `/resume <sid>\r` arrived: the chars landed in the prompt
    /// but the trailing `\r` got dropped, leaving the command
    /// sitting unsubmitted. Reported by a user who could "just hit
    /// Enter" to recover. The delay between phases lets the prompt
    /// absorb the typed chars before we tell it to submit.
    fn send_pending_resumes(&mut self, now: std::time::Instant) {
        use crate::pane::tabs::PendingResumeSend;
        let Some(tabs) = self.runtime.pane_tabs.as_mut() else {
            return;
        };
        for entry in tabs.tabs_mut() {
            match entry.info.pending_resume_send.take() {
                Some(PendingResumeSend::Text { sid, after }) if now >= after => {
                    let _ = entry.pane.send_bytes(format!("/resume {sid}").as_bytes());
                    entry.info.pending_resume_send = Some(PendingResumeSend::Enter {
                        after: now + RESTORE_RESUME_ENTER_DELAY,
                    });
                }
                Some(PendingResumeSend::Enter { after }) if now >= after => {
                    let _ = entry.pane.send_bytes(b"\r");
                    // Cleared by take().
                }
                other => {
                    // Not yet due, or empty — put back what we took.
                    entry.info.pending_resume_send = other;
                }
            }
        }
    }

    /// Locate a `claude --resume` tab from session restore that looks
    /// broken (non-zero exit, or alive-but-printed-a-crash-dump within
    /// the 30s window). Disarms the marker on tabs whose window has
    /// passed without trouble, so a real user-driven exit later isn't
    /// mistaken for a restore failure. Returns the index of the first
    /// crashed tab found, if any.
    fn find_crashed_restore_tab(&mut self, now: std::time::Instant) -> Option<usize> {
        let tabs = self.runtime.pane_tabs.as_mut()?;
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
                && entry.pane.exit_status().is_some_and(|s| s.exit_code() != 0);
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
    /// Compose the terminal title and, if it changed, return the
    /// `SetTerminalTitle` effect to emit (the run loop runs it via
    /// `run_effects`, the sole executor of the `term_title::set` IO). The
    /// compose + dedup stay loop-side: `last_term_title` is advanced here,
    /// and the foreground-exec after-work resets it to `None` to force a
    /// re-emit. Returns `None` when the title is unchanged.
    fn term_title_effect(&mut self) -> Option<Effect> {
        let title = crate::term_title::compose(
            self.state.project_home.as_deref(),
            self.state.session_name.as_deref(),
            &self.state.listing.dir,
        );
        if self.view.last_term_title.as_deref() == Some(&title) {
            return None;
        }
        self.view.last_term_title = Some(title.clone());
        Some(Effect::SetTerminalTitle { title })
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
pub fn command_without_resume(cmd: &str) -> String {
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
pub fn command_without_codex_resume(cmd: &str) -> String {
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

/// Parse `gemini --list-sessions` stdout for the line whose
/// bracketed UUID matches `uuid`, returning the leading `<n>.`
/// index. The expected format is:
///
/// ```text
/// Available sessions for this project (2):
///   1. let's do a code review of this app (1 day ago) [76422c62-...-d149]
///   2. Analyze project for bugs and provide recommendations. (...) [4a7cd126-...-7544]
/// ```
///
/// Pure helper so the parser has unit tests; the IO side
/// (`gemini_resume_index_for`) just spawns the process and feeds
/// stdout in.
fn parse_gemini_list_sessions_for_uuid(text: &str, uuid: &str) -> Option<u32> {
    for line in text.lines() {
        let trimmed = line.trim_start();
        let Some((idx_str, rest)) = trimmed.split_once('.') else {
            continue;
        };
        let Ok(idx) = idx_str.trim().parse::<u32>() else {
            continue;
        };
        let Some(open) = rest.rfind('[') else {
            continue;
        };
        let Some(close) = rest.rfind(']') else {
            continue;
        };
        if open >= close {
            continue;
        }
        if rest[open + 1..close].eq_ignore_ascii_case(uuid) {
            return Some(idx);
        }
    }
    None
}

/// Strip Gemini's `--resume <id>` (or `-r <id>`) and `--session-id
/// <UUID>` flags from a command line, leaving a clean baseline that
/// session restore can re-decorate. The resume index is unstable
/// across runs (it's just a position in `--list-sessions` output) so
/// we always recompute it at restore time from the saved UUID; baking
/// the old index into the saved command would silently resume the
/// wrong conversation.
pub fn command_without_gemini_resume(cmd: &str) -> String {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    let mut out: Vec<&str> = Vec::with_capacity(parts.len());
    let mut skip_next = false;
    for p in parts {
        if skip_next {
            skip_next = false;
            continue;
        }
        if p == "--resume" || p == "-r" || p == "--session-id" {
            skip_next = true;
            continue;
        }
        if let Some(_value) = p.strip_prefix("--resume=") {
            continue;
        }
        if let Some(_value) = p.strip_prefix("--session-id=") {
            continue;
        }
        out.push(p);
    }
    let stripped = out.join(" ");
    if stripped.is_empty() {
        "gemini".to_string()
    } else {
        stripped
    }
}

/// Strip Antigravity's `--conversation <UUID>`, `-c <UUID>`, and `--continue` flags from a command line.
pub fn command_without_agy_resume(cmd: &str) -> String {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    let mut out: Vec<&str> = Vec::with_capacity(parts.len());
    let mut skip_next = false;
    for p in parts {
        if skip_next {
            skip_next = false;
            continue;
        }
        if p == "--conversation" || p == "-c" {
            skip_next = true;
            continue;
        }
        if p == "--continue" {
            continue;
        }
        if let Some(_value) = p.strip_prefix("--conversation=") {
            continue;
        }
        if let Some(_value) = p.strip_prefix("-c=") {
            continue;
        }
        out.push(p);
    }
    let stripped = out.join(" ");
    if stripped.is_empty() {
        "agy".to_string()
    } else {
        stripped
    }
}

impl App {
    /// Resolve the `claude --resume <token>` target to use on session save.
    ///
    /// Multi-pane safety: when several Claude tabs share a cwd, we
    /// can't blindly use "most-recent JSONL for this cwd" — they'd
    /// all save the same ID and collapse onto a single conversation
    /// at restore. The caller threads `pane_spawn_epoch_secs` and a
    /// `claimed` set; the resolver picks a unique session record per
    /// pane by matching `startedAt` to the pane's spawn time.
    ///
    /// Strategy, in order:
    /// 1. Read the exit-banner token from pane scrollback. If it's a
    ///    UUID, verify a JSONL exists for it under
    ///    `~/.claude/projects/<slug>/`. Claude sometimes prints the
    ///    banner with a session ID it never persisted (e.g. user
    ///    `/clear`'d or `/resume`'d before exit), so an unconditional
    ///    trust leads to "No conversation found …" on restore. The
    ///    banner is unambiguously this pane, so it bypasses `claimed`.
    /// 2. Walk `~/.claude/sessions/` records matching the cwd, skip
    ///    any already in `claimed`, pick the one whose `startedAt` is
    ///    closest to this pane's spawn time, verify JSONL on disk.
    /// 3. Last-ditch: most-recently-modified JSONL in the project
    ///    slug, but only if it isn't already in `claimed`. Without
    ///    the claimed-check this is what was producing the bug.
    pub(crate) fn resolve_claude_resume_target(
        pane: &crate::pane::Pane,
        cwd: &std::path::Path,
        pane_spawn_epoch_secs: u64,
        claimed: &std::collections::HashSet<String>,
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
                    // Banner UUID has no JSONL — fall through.
                } else {
                    // Named sessions: claude resolves names itself, trust it.
                    return (Some(tok.clone()), Some(tok));
                }
            }

            // Step 2: pick the per-pane match by spawn-time proximity.
            // Filter to JSONL-on-disk first so the picker only sees
            // resumable candidates.
            let candidates: Vec<_> = s::find_claude_sessions(cwd)
                .into_iter()
                .filter(|c| s::claude_jsonl_exists(cwd, &c.session_id))
                .collect();
            if let Some(c) =
                s::pick_closest_unclaimed_session(candidates, pane_spawn_epoch_secs, claimed)
            {
                return (Some(c.session_id), c.name);
            }

            // Step 3: final fallback. Most-recent JSONL — but only if
            // unclaimed; otherwise leave this pane unresumable rather
            // than collapse it onto another pane's conversation.
            if let Some(id) = s::most_recent_jsonl_for_cwd(cwd)
                && !claimed.contains(&id)
            {
                let name = s::find_claude_session_name_public(&id);
                return (Some(id), name);
            }
            (None, None)
        })();

        if let (Some(id), _) = &resolved
            && s::is_uuid(id)
            && !s::claude_jsonl_exists(cwd, id)
        {
            spyc_debug!(
                "resolve_claude_resume_target: dropping ghost id {} (no JSONL under {})",
                id,
                cwd.display()
            );
            return (None, None);
        }
        resolved
    }

    /// Resolve the Gemini resume target to save for a pane.
    ///
    /// Gemini's CLI doesn't print an exit banner with a resume token,
    /// so we pull the candidate set from
    /// `~/.gemini/tmp/<project>/chats/*.jsonl` (each file's first line
    /// is JSON metadata with `sessionId` and `startTime`) and pick the
    /// unclaimed record whose start time is closest to this pane's
    /// `spawn_epoch_secs`. Multi-pane safety: the `claimed` set
    /// prevents two panes in the same project from collapsing onto
    /// one conversation. Returns the UUID; Gemini doesn't expose a
    /// human-readable session name from the CLI, so the second slot
    /// is always `None`.
    pub(crate) fn resolve_gemini_resume_target(
        cwd: &std::path::Path,
        pane_spawn_epoch_secs: u64,
        claimed: &std::collections::HashSet<String>,
    ) -> (Option<String>, Option<String>) {
        use crate::state::sessions as s;
        let candidates = s::find_gemini_sessions(cwd);
        s::pick_closest_unclaimed_session(candidates, pane_spawn_epoch_secs, claimed)
            .map_or((None, None), |c| (Some(c.session_id), None))
    }

    /// At restore time, translate a saved Gemini session UUID into
    /// the index `gemini --resume <N>` consumes. Runs `gemini
    /// --list-sessions` synchronously in `cwd` and delegates parsing
    /// to `parse_gemini_list_sessions_for_uuid`. Returns `None` when
    /// the binary errors, the UUID isn't in the listing, or the
    /// output format drifts. Failure is recoverable: the caller falls
    /// back to spawning `gemini` bare and lets the user pick.
    pub(crate) fn gemini_resume_index_for(cwd: &std::path::Path, uuid: &str) -> Option<u32> {
        let out = std::process::Command::new("gemini")
            .arg("--list-sessions")
            .current_dir(cwd)
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let text = std::str::from_utf8(&out.stdout).ok()?;
        parse_gemini_list_sessions_for_uuid(text, uuid)
    }

    // MVU Phase 5: `yank_pane_to_clipboard` / `yank_scrollback_to_clipboard`
    // are gone — their live-pane read + guards + clipboard IO moved into
    // `run_effects`'s `Effect::ReadPaneText` executor. The `yp`/`ya` action
    // arms in `actions.rs` now emit `ReadPaneText { kind, then: Clipboard }`
    // directly, so the handler stays pure-Model (no Runtime read).

    /// yf — yank the cursor file's absolute path to the system
    /// clipboard. When picks are active, yanks all of them
    /// newline-separated. Always absolute paths so the receiving
    /// shell resolves them correctly regardless of where the user
    /// pastes them. The user's recurring real-world ask was a clean
    /// way to grab a path for one-off shell commands like `git
    /// restore <path>` without opening a pane.
    fn yank_paths_to_clipboard(&mut self) -> Vec<Effect> {
        let paths = self.state.selection_paths();
        if paths.is_empty() {
            self.state.flash_error("no path to yank");
            return Vec::new();
        }
        let text: String = paths
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let ok = if paths.len() == 1 {
            ClipMsg::SinglePath
        } else {
            ClipMsg::MultiPath { count: paths.len() }
        };
        vec![Effect::CopyToClipboard { text, ok }]
    }

    /// yP — yank the last prompt the user typed into the pane.
    fn yank_last_prompt_to_clipboard(&mut self) -> Vec<Effect> {
        let Some(text) = self.state.last_pane_prompt.as_ref() else {
            self.state.flash_error("no prompt to yank");
            return Vec::new();
        };
        vec![Effect::CopyToClipboard {
            text: text.clone(),
            ok: ClipMsg::Prompt,
        }]
    }

    /// Put inventory items to the current working directory.
    /// Picked items only if any picks exist, else all.
    /// Items are removed from inventory after successful put.
    fn put_inventory_to_cwd(&mut self) -> Vec<Effect> {
        let dest = self.state.listing.dir.clone();
        let item_count = if self.state.inventory.picks.is_empty() {
            self.state.inventory.len()
        } else {
            self.state.inventory.picks.len()
        };
        if item_count == 0 {
            self.state.flash_error("inventory is empty");
            return Vec::new();
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
        Vec::new()
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
    fn handle_graveyard_view_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        // Confirm-purge-all is a transient inline confirm. We
        // signal it via a one-shot Mode::Prompting; routed there
        // directly rather than reusing RemoveConfirm because the
        // semantics are distinct (we're cascading to system trash,
        // not unlinking).
        match key.code {
            KeyCode::Char('?') | KeyCode::F(1) => {
                // Reported: graveyard view had no `?` help, so the
                // restore / purge bindings were undiscoverable
                // from within the view. The pager-mounted help
                // overlay coexists fine with the underlying
                // graveyard view — Esc on the help returns to the
                // same cursor position.
                self.view.graveyard_pending_d = false;
                self.view.graveyard_pending_g = false;
                self.open_help();
            }
            KeyCode::Esc => {
                self.state.open_graveyard_view(); // toggle off
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let rpc = self.state.grid_dims.rows_per_col as usize;
                self.state
                    .cursor_move_vertical(1, rpc, self.state.rows.len());
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let rpc = self.state.grid_dims.rows_per_col as usize;
                self.state
                    .cursor_move_vertical(-1, rpc, self.state.rows.len());
            }
            KeyCode::Char('g') => {
                self.view.graveyard_pending_d = false;
                if self.view.graveyard_pending_g {
                    self.state.cursor.index = 0;
                    self.view.graveyard_pending_g = false;
                } else {
                    self.view.graveyard_pending_g = true;
                }
            }
            KeyCode::Char('G') => {
                self.view.graveyard_pending_d = false;
                self.view.graveyard_pending_g = false;
                if !self.state.rows.is_empty() {
                    self.state.cursor.index = self.state.rows.len() - 1;
                }
            }
            KeyCode::Char('p') => {
                self.view.graveyard_pending_d = false;
                self.view.graveyard_pending_g = false;
                self.graveyard_restore(false);
            }
            KeyCode::Char('P') => {
                self.view.graveyard_pending_d = false;
                self.view.graveyard_pending_g = false;
                self.graveyard_restore(true);
            }
            KeyCode::Char('x') => {
                self.view.graveyard_pending_d = false;
                self.view.graveyard_pending_g = false;
                self.graveyard_purge_cursor_entry();
            }
            KeyCode::Char('d') => {
                self.view.graveyard_pending_g = false;
                if self.view.graveyard_pending_d {
                    self.view.graveyard_pending_d = false;
                    self.graveyard_purge_cursor_entry();
                } else {
                    self.view.graveyard_pending_d = true;
                }
            }
            KeyCode::Char('Z') => {
                self.view.graveyard_pending_d = false;
                self.view.graveyard_pending_g = false;
                self.state.mode = Mode::Prompting(Prompt::simple(
                    PromptKind::GraveyardPurgeAllConfirm,
                    "purge ALL graveyard entries to system trash? (y/N): ",
                ));
            }
            _ => {
                self.view.graveyard_pending_d = false;
                self.view.graveyard_pending_g = false;
            }
        }
        Vec::new()
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
        let default_cmd = crate::envset::var("SPYC_PANE_CMD")
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
        if let Some(tabs) = self.runtime.pane_tabs.as_mut()
            && !tabs.close_active()
        {
            // Last tab removed.
            self.runtime.pane_tabs = None;
            self.state.focus = state::Focus::FileList;
            self.view.needs_full_repaint = true;
            self.state.flash_info("pane: last tab closed");
        }
    }

    /// ^a R — restart the active tab's command. Closes the tab and spawns
    /// a fresh one with the same command and working directory.
    fn restart_active_tab(&mut self) {
        let Some(tabs) = self.runtime.pane_tabs.as_ref() else {
            return;
        };
        let cmd = tabs.active_info().command.clone();
        let cwd = tabs.active_info().cwd.clone();
        // Close the old tab first.
        if let Some(tabs) = self.runtime.pane_tabs.as_mut()
            && !tabs.close_active()
        {
            self.runtime.pane_tabs = None;
            self.state.focus = state::Focus::FileList;
        }
        // Spawn a replacement with the same command and cwd.
        self.open_pane_tab_in(&cmd, &cwd);
        self.state.flash_info(format!("pane: restarted {cmd}"));
    }

    /// ^W j / ^W k — set keyboard focus directionally (no wrap).
    fn set_pane_focus(&mut self, want_pane: bool) {
        if self.runtime.pane_tabs.is_none() {
            return;
        }
        if self.state.pane_focused() == want_pane {
            return; // already there — no-op
        }
        // Branch order is arbitrary in Phase 0: every non-Pane arm yields
        // `pane_focused() == false`, so it is invisible to all current
        // consumers (router, render DIM, flash, ^C gate). The Overlay/Pager
        // distinction is carried only for future MVU phases.
        self.state.focus = if want_pane {
            state::Focus::Pane
        } else if self.runtime.top_overlay.is_some() {
            state::Focus::Overlay
        } else if let Some(v) = self.view.pager.as_ref() {
            state::Focus::Pager(v.mount)
        } else {
            state::Focus::FileList
        };
        if self.state.pane_focused() {
            let label = self
                .runtime
                .pane_tabs
                .as_ref()
                .map_or("pane", |t| t.active_info().label.as_str());
            self.state.flash_info(format!("focus: {label}"));
        } else {
            // When a `;cmd` overlay is showing the spyc-list slot, the
            // "non-pane" side is the overlay subprocess, not the file
            // list. Label accordingly so the user can read what just
            // got focus instead of guessing.
            let label = if self.runtime.top_overlay.is_some() {
                "overlay"
            } else {
                "spyc"
            };
            self.state.flash_info(format!("focus: {label}"));
        }
    }

    /// Handle keys while the pane is in scroll mode. Vi-style navigation
    /// through the scrollback buffer; `Esc`/`q` exit back to live view.
    /// `^a-v` — open the active pane's scrollback (cell grid +
    /// off-screen history) in a `PagerView` mounted in the lower
    /// pane slot. Search, jump, visual range yank, line numbers
    /// — every pager feature works on the captured snapshot.
    ///
    /// Snapshots the pty state at entry; live output keeps flowing
    /// to the parser but the pager view is frozen until the user
    /// closes it. Esc / `q` exits, the pty pane snaps back to live.
    ///
    /// Alt-screen apps (codex, vim, htop, lazygit) still flash the
    /// "no scrollback" hint and skip opening the pager — there's
    /// genuinely nothing to scroll back through, and the app's own
    /// history viewer is the right tool.
    /// `:dump-scrollback` diagnostic. Runs the same drain +
    /// snapshot path as `^a-v`, then writes the captured lines as
    /// plain text to `/tmp/spyc-scrollback.txt`. Tail the file to
    /// confirm whether content visible on the live pane (HUD
    /// overlays, etc.) is actually reaching our vt100 emulator at
    /// snapshot time.
    fn dump_scrollback_snapshot(&mut self) {
        let Some(tabs) = self.runtime.pane_tabs.as_mut() else {
            self.state.flash_error("dump-scrollback: no pane open");
            return;
        };
        let active = tabs.active_mut();
        for _ in 0..3 {
            active.drain_output();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        active.drain_output();
        let lines = active.with_screen_mut(crate::ui::scrollback::lines_from_scrollback);
        let path = std::path::Path::new("/tmp/spyc-scrollback.txt");
        let mut out = String::new();
        for line in &lines {
            for span in &line.spans {
                out.push_str(&span.content);
            }
            out.push('\n');
        }
        match std::fs::write(path, &out) {
            Ok(()) => {
                self.state
                    .flash_info(format!("wrote {} lines to {}", lines.len(), path.display()));
            }
            Err(e) => {
                self.state
                    .flash_error(format!("dump-scrollback: write failed: {e}"));
            }
        }
    }

    /// Stash the active scrollback pager (if any) onto the
    /// currently-active tab's slot. Tab-switch handlers call this
    /// **before** flipping the active-tab pointer; the companion
    /// `restore_active_tab_scrollback_pager` runs **after** the flip
    /// to surface the destination tab's stashed pager if it has one.
    /// Together: scroll back on tab 1, `^a-n`, the pager visually
    /// disappears (replaced by tab 2's live pty); `^a-p` back to
    /// tab 1, the pager comes back at the same scroll / search /
    /// selection state.
    ///
    /// Only acts on scrollback pagers (`pane_scroll == true`).
    /// Content-bound pagers (Overlay file viewer, TopPane Markdown,
    /// etc.) are App-level and persist across tab switches.
    fn stash_scrollback_pager_to_active_tab(&mut self) {
        if !self.view.pager.as_ref().is_some_and(|v| v.pane_scroll) {
            return;
        }
        let view = self.view.pager.take();
        if let Some(tabs) = self.runtime.pane_tabs.as_mut() {
            tabs.active_entry_mut().stashed_scrollback_pager = view;
        }
    }

    /// Restore the active tab's stashed scrollback pager into
    /// `self.view.pager` if one is stashed AND no other pager is currently
    /// displayed. A non-scrollback pager (Overlay file viewer, etc.)
    /// up at the time of the tab switch is left alone; the stash
    /// surfaces on the next switch back where no overlay is in the
    /// way. See `stash_scrollback_pager_to_active_tab` for the
    /// outgoing half of the pair.
    fn restore_active_tab_scrollback_pager(&mut self) {
        if self.view.pager.is_some() {
            return;
        }
        if let Some(tabs) = self.runtime.pane_tabs.as_mut()
            && let Some(view) = tabs.active_entry_mut().stashed_scrollback_pager.take()
        {
            self.set_pager(view);
        }
    }

    fn open_pane_scroll_pager(&mut self) {
        let Some(tabs) = self.runtime.pane_tabs.as_ref() else {
            return;
        };
        let active_info = tabs.active_info();
        let label = active_info.label.clone();
        let command = active_info.command.clone();
        let cwd = active_info.cwd.clone();
        let spawn = active_info.spawn_epoch_secs;

        // Agent-aware scrollback. An agent's `AgentProfile` may carry a
        // `TranscriptSpec`: read its structured on-disk transcript — the
        // source of truth (codex/agy confine history to a scroll region
        // vt100 can't capture; claude's terminal output works too but
        // the transcript is cleaner) — and render the real conversation,
        // taking priority over the alt-screen guard + vt100 path below.
        // `config_key` gates the view (`None` = always-on, e.g. codex).
        // `miss_message` distinguishes "flash + stop" (codex — no usable
        // terminal capture) from "fall through to vt100" (claude/agy).
        let profile = crate::agent::detect(&command);
        if let Some(spec) = profile.transcript() {
            let enabled = match spec.config_key {
                None => spec.default_enabled,
                Some(key) => self
                    .state
                    .config
                    .pane
                    .transcript_enabled(key, spec.default_enabled),
            };
            if enabled {
                if let Some(path) = (spec.resolve)(cwd.as_path(), spawn) {
                    let lines = (spec.render)(path.as_path(), &self.view.theme);
                    if !lines.is_empty() {
                        self.mount_scroll_pager(format!(" {label} (transcript)"), lines);
                        return;
                    }
                }
                if let Some(msg) = spec.miss_message {
                    self.state.flash_info(msg);
                    return;
                }
            }
        }

        let tabs = self
            .runtime
            .pane_tabs
            .as_mut()
            .expect("pane_tabs presence checked above");
        let active = tabs.active_mut();
        if active.is_alternate_screen() {
            // Alt-screen apps (vim, less, htop, ...) do virtual
            // scrolling inside a fixed grid — old content lives in
            // app memory, not the terminal — so spyc has nothing to
            // show.
            self.state
                .flash_info("scroll: alt-screen app — use its own scrollback / history keys");
            return;
        }
        // Drain pending bytes before snapshotting. Bytes that hit
        // the OS pipe between the last render tick and this keypress
        // may still be in flight on the reader/parser threads; a few
        // short yields let them flush so the snapshot includes the
        // most-recent paint.
        for _ in 0..3 {
            active.drain_output();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        active.drain_output();
        // Empty scrollback ⇒ a fresh process, or an app that keeps
        // its own history (scroll region / virtual scroll). Flash a
        // hint; still open the pager so search/yank of the visible
        // screen works.
        let scrollback_rows = active.with_screen_mut(crate::ui::scrollback::scrollback_len);
        let lines = active.with_screen_mut(crate::ui::scrollback::lines_from_scrollback);
        if scrollback_rows == 0 {
            self.state
                .flash_info("no scrollback captured — this app keeps its own history");
        }
        self.mount_scroll_pager(format!(" {label} (history)"), lines);
    }

    /// Mount a lower-pane scroll/transcript pager from pre-built
    /// lines. Shared by the vt100-scrollback path and the codex
    /// on-disk transcript path. Enters the active pane's scroll mode
    /// (divider cues + key routing flip to the pager) and parks the
    /// view at the bottom on first render.
    fn mount_scroll_pager(&mut self, title: String, lines: Vec<ratatui::text::Line<'static>>) {
        if let Some(tabs) = self.runtime.pane_tabs.as_mut() {
            tabs.active_mut().enter_scroll_mode();
        }
        let mut view = crate::ui::pager::PagerView::new_styled(title, lines);
        view.mount = crate::ui::pager::Mount::LowerPane;
        view.pane_scroll = true;
        // Gutter off so existing content doesn't jump horizontally
        // when the pager opens. Toggle with `l`.
        view.show_line_numbers = false;
        view.no_history = true;
        // Wrap long lines (compiler errors, diffs, transcript turns)
        // — no horizontal scroll, so truncation would hide content.
        view.wrap = true;
        // Park at the bottom on first render via the deferred flag;
        // the LowerPane render branch knows the real viewport height
        // and scrolls there, avoiding a one-frame jump.
        view.pending_scroll_to_bottom.set(true);
        self.set_pager(view);
        self.state.focus = state::Focus::Pane;
        self.view.needs_full_repaint = true;
        self.state
            .flash_info("scroll: on (/, n/N, :N, V, y, Esc exit)");
    }

    fn handle_pane_scroll_key(&mut self, key: crossterm::event::KeyEvent) -> Vec<Effect> {
        use crossterm::event::{KeyCode, KeyModifiers};
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        // Handle pending `g` prefix: gg = scroll top, gf/gF = goto file.
        if self.view.scroll_pending_g {
            self.view.scroll_pending_g = false;
            return match key.code {
                KeyCode::Char('g') => {
                    self.runtime
                        .pane_tabs
                        .as_mut()
                        .unwrap()
                        .active_mut()
                        .scroll_to_top();
                    Vec::new()
                }
                // gf/gF while scrolling a pane — same path as the file-list
                // action: emit a `ReadPaneText`/`GotoFile` effect so the
                // pickable read + navigation run in `run_effects` (PR 5b).
                KeyCode::Char('f') => vec![Effect::ReadPaneText {
                    kind: PaneTextKind::Pickable(200),
                    then: PaneTextSink::GotoFile {
                        open_at_line: false,
                    },
                }],
                KeyCode::Char('F') => vec![Effect::ReadPaneText {
                    kind: PaneTextKind::Pickable(200),
                    then: PaneTextSink::GotoFile { open_at_line: true },
                }],
                _ => Vec::new(), // Unknown g-sequence, ignore
            };
        }

        let pane = self.runtime.pane_tabs.as_mut().unwrap().active_mut();
        match key.code {
            KeyCode::Char('k') | KeyCode::Up => pane.scroll_up(1),
            KeyCode::Char('j') | KeyCode::Down => pane.scroll_down_or_exit(1),
            KeyCode::PageUp | KeyCode::Char('b') if ctrl => pane.scroll_up(20),
            KeyCode::Char('u') if ctrl => pane.scroll_up(10),
            KeyCode::PageDown | KeyCode::Char('f') if ctrl => pane.scroll_down_or_exit(20),
            KeyCode::Char('d') if ctrl => pane.scroll_down_or_exit(10),
            KeyCode::Char('g') => {
                self.view.scroll_pending_g = true;
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
        Vec::new()
    }

    /// ^W s — write the current selection as shell-quoted paths to the
    /// pane's stdin. A trailing space is appended so the user can keep
    /// typing without concatenating against the last path. No newline
    /// — let the user decide when to submit.
    fn send_selection_to_pane(&mut self) -> Vec<Effect> {
        if self.runtime.pane_tabs.is_none() {
            self.state.flash_error("no pane open (Ctrl-\\ to open one)");
            return Vec::new();
        }
        // Build the payload before grabbing the pane mut-borrow, so we
        // can still call self.flash_* below without overlapping borrows.
        // Clone project_home up front so the immutable borrow doesn't
        // overlap with the selection_paths borrow below.
        let project_home = self.state.project_home.clone();
        let (payload, count) = {
            let paths = self.state.selection_paths();
            if paths.is_empty() {
                self.state.flash_error("nothing selected");
                return Vec::new();
            }
            let count = paths.len();
            let mut out = String::new();
            for (i, p) in paths.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                // Anchor paths on PROJECT_HOME so what lands in the
                // pane matches what an agent / shell session running
                // inside that project would type. Outside-project
                // paths stay absolute rather than walking up with
                // `../../..`, which is rarely what the user wants.
                let display = project_home
                    .as_deref()
                    .and_then(|home| p.strip_prefix(home).ok())
                    .map_or_else(
                        || p.to_path_buf(),
                        |rel| {
                            if rel.as_os_str().is_empty() {
                                // path == project_home itself.
                                std::path::PathBuf::from(".")
                            } else {
                                rel.to_path_buf()
                            }
                        },
                    );
                out.push_str(&shell::shell_quote(&display.to_string_lossy()));
            }
            out.push(' ');
            (out, count)
        };
        vec![Effect::SendToPane {
            target: PaneTarget::Active,
            input: PaneInput::Bytes(payload.into_bytes()),
            on_ok: Some(format!("sent {count} path(s) to pane")),
            err_prefix: Some("send failed"),
        }]
    }

    /// ^W p / ^W i — read file contents of selection (or inventory) and
    /// send them to the active pane tab as bracketed paste. Each file is
    /// wrapped with a header so the recipient (e.g. Claude) knows what
    /// it's looking at.
    fn pipe_content_to_pane(&mut self, use_inventory: bool) -> Vec<Effect> {
        if self.runtime.pane_tabs.is_none() {
            self.state.flash_error("no pane open");
            return Vec::new();
        }
        // Build payload: read from cache for inventory, from disk for selection.
        let mut payload = String::new();
        let mut count = 0usize;
        let mut skipped = 0usize;

        if use_inventory {
            let ids = self.state.inventory.selected_ids();
            if ids.is_empty() {
                self.state.flash_error("inventory is empty");
                return Vec::new();
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
                return Vec::new();
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
            return Vec::new();
        }
        // Send as bracketed paste so it arrives as a single block.
        let mut buf = Vec::with_capacity(payload.len() + 12);
        buf.extend_from_slice(b"\x1b[200~");
        buf.extend_from_slice(payload.as_bytes());
        buf.extend_from_slice(b"\x1b[201~");
        let msg = if skipped > 0 {
            format!("piped {count} file(s), skipped {skipped} binary/unreadable")
        } else {
            format!("piped {count} file(s) to pane")
        };
        vec![Effect::SendToPane {
            target: PaneTarget::Active,
            input: PaneInput::Bytes(buf),
            on_ok: Some(msg),
            err_prefix: Some("pipe failed"),
        }]
    }

    /// ^W + / ^W - — change the bottom pane's share of the middle rect
    /// in 5% steps, clamped to [10%, 90%].
    fn resize_pane(&mut self, delta_pct: i32) {
        if self.runtime.pane_tabs.is_none() {
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
        if self.runtime.pane_tabs.is_none() {
            self.state.flash_info("no pane open");
            return;
        }
        if self.state.pane_zoomed {
            self.state.pane_zoomed = false;
            if let Some(prev) = self.state.pane_focus_before_zoom.take() {
                self.state.focus = if prev {
                    state::Focus::Pane
                } else {
                    state::Focus::FileList
                };
            }
            self.state.flash_info("zoom: off");
        } else {
            self.state.pane_focus_before_zoom = Some(self.state.pane_focused());
            self.state.pane_zoomed = true;
            self.state.focus = state::Focus::Pane;
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
        if let (Some(pane_rect), Some(tabs)) = (layout.pane, self.runtime.pane_tabs.as_mut()) {
            for entry in tabs.tabs_mut() {
                let _ = entry.pane.resize(pane_rect.height, pane_rect.width);
            }
        }
        self.view.needs_full_repaint = true;
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
        if self.state.harpoon.is_none() {
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
        let h = self.state.harpoon.as_mut().unwrap();
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
        if self.state.harpoon.is_none() {
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
        let h = self.state.harpoon.as_mut().unwrap();
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
        let Some(h) = self.state.harpoon.as_ref() else {
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
        if self.state.harpoon.is_none() {
            self.state
                .flash_error("harpoon: set PROJECT_HOME first (gP)");
            return;
        }
        self.view.harpoon_menu = Some(HarpoonMenu {
            cursor: 0,
            delete_armed: false,
        });
        self.view.needs_full_repaint = true;
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
    fn handle_harpoon_menu_key(&mut self, key: crossterm::event::KeyEvent) -> Vec<Effect> {
        use crossterm::event::KeyCode;
        let Some(menu) = self.view.harpoon_menu.as_mut() else {
            return Vec::new();
        };
        let Some(h) = self.state.harpoon.as_mut() else {
            self.view.harpoon_menu = None;
            self.view.needs_full_repaint = true;
            return Vec::new();
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
                self.view.harpoon_menu = None;
                self.view.needs_full_repaint = true;
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
                self.view.harpoon_menu = None;
                self.view.needs_full_repaint = true;
                self.harpoon_jump(slot);
            }
            KeyCode::Enter if len > 0 => {
                let slot = (menu.cursor + 1) as u8;
                self.view.harpoon_menu = None;
                self.view.needs_full_repaint = true;
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
                    if let Some(m) = self.view.harpoon_menu.as_mut() {
                        let new_len = self.state.harpoon.as_ref().map_or(0, |hh| hh.slots.len());
                        if new_len == 0 {
                            m.cursor = 0;
                        } else {
                            m.cursor = removed_idx.min(new_len - 1);
                        }
                    }
                } else if let Some(m) = self.view.harpoon_menu.as_mut() {
                    m.delete_armed = true;
                }
            }
            _ => {}
        }
        Vec::new()
    }

    // ---- Quick Select ------------------------------------------------------

    /// `^a u` — enter Quick Select. Snapshot the visible pane,
    /// scan for matches across the built-in + user patterns,
    /// assign labels, and install the picker as a key-intercepting
    /// overlay. Bails with a flash if there's nothing pickable.
    fn open_quick_select(&mut self) {
        use crate::pane::quick_select::{QuickSelect, assign_labels, build_patterns, scan};
        let Some(tabs) = self.runtime.pane_tabs.as_mut() else {
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
        self.view.quick_select = Some(QuickSelect {
            matches,
            pending_first: None,
            all_two_letter,
            open_intent: false,
        });
        self.view.needs_full_repaint = true;
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
    fn handle_quick_select_key(&mut self, key: crossterm::event::KeyEvent) -> Vec<Effect> {
        use crossterm::event::KeyCode;
        let Some(qs) = self.view.quick_select.as_mut() else {
            return Vec::new();
        };

        let close = |this: &mut Self| {
            this.view.quick_select = None;
            this.view.needs_full_repaint = true;
        };

        let c = match key.code {
            KeyCode::Esc => {
                close(self);
                return Vec::new();
            }
            KeyCode::Char(c) => c,
            _ => return Vec::new(),
        };

        // `q`/`Q` always exits — labels never use it (alphabet check
        // covered in unit test) so this is unambiguous.
        if c.eq_ignore_ascii_case(&'q') && qs.pending_first.is_none() {
            close(self);
            return Vec::new();
        }

        let is_upper = c.is_ascii_uppercase();
        let lower = c.to_ascii_lowercase();

        if qs.all_two_letter {
            match qs.pending_first {
                None => {
                    // First keystroke: must be the prefix of some label.
                    let any_match = qs.matches.iter().any(|m| m.label.starts_with(lower));
                    if !any_match {
                        return Vec::new(); // no narrowing possible — ignore
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
        Vec::new()
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
        match crate::clipboard::copy(text) {
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
    /// for a Path match). Mirrors `goto_file_navigate`'s post-resolve
    /// flow but starts from a pre-extracted path string rather than
    /// running pathref again.
    fn jump_to_pane_path(&mut self, raw: &str) {
        let path = std::path::PathBuf::from(raw);
        let resolved = if path.is_absolute() {
            path
        } else {
            // Resolve against the active pane tab's cwd first, falling
            // back to spyc's listing dir — same precedence `gf` uses.
            let tab_cwd = self
                .runtime
                .pane_tabs
                .as_ref()
                .map(|t| t.active_info().cwd.clone());
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
        self.state.focus = state::Focus::FileList;
        self.state.rebuild_rows();
        self.view.needs_full_repaint = true;
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

    /// Render label overlay on top of the pane. Drawn after the
    /// pane widget so labels paint over the live vt100 grid; small
    /// inverted-color cells next to each match's start position.
    fn render_quick_select_overlay(&self, frame: &mut Frame, pane_rect: ratatui::layout::Rect) {
        use ratatui::{
            style::{Color, Modifier, Style},
            widgets::Paragraph,
        };
        let Some(qs) = self.view.quick_select.as_ref() else {
            return;
        };
        let label_style = Style::default()
            .fg(Color::Black)
            .bg(self.view.theme.pick)
            .add_modifier(Modifier::BOLD);
        let pending_style = Style::default()
            .fg(Color::Black)
            .bg(self.view.theme.prompt_prefix)
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
                    Style::default().fg(self.view.theme.status_suffix)
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
        self.view.pager = Some(pager::PagerView::new_ansi(label, &combined));
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

    // ---- Path references (M13) ------------------------------------------------

    /// `gf` / `gF` — scan the active pane's visible output for a file path
    /// reference, navigate the file list there, and optionally open the
    /// pager at the referenced line.
    /// Resolve a path reference from already-read pane `lines` and navigate to
    /// it (chdir + focus); `open_at_line` (gF) also opens the file in the pager
    /// at the referenced line. The pickable read + the pane's cwd are supplied
    /// by the `ReadPaneText` / `GotoFile` executor (PR 5b) so the live-pane read
    /// lives in `run_effects` — this half stays pure of the Runtime handle.
    fn goto_file_navigate(&mut self, lines: Vec<String>, pane_cwd: PathBuf, open_at_line: bool) {
        // Also try resolving against the spyc cwd (project root), not just
        // the pane tab's cwd — Claude often prints paths relative to the
        // project root regardless of the shell's cwd.
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
        if let Some(tabs) = self.runtime.pane_tabs.as_mut()
            && tabs.active().is_scrolling()
        {
            tabs.active_mut().exit_scroll_mode();
        }
        self.state.focus = state::Focus::FileList;
        self.view.needs_full_repaint = true;

        // Navigate: if it's a directory, chdir there; if a file, chdir to
        // its parent and focus on it.
        if path.is_dir() {
            if let Err(e) = self.state.chdir(&path) {
                self.state.flash_error(format!("gf: {e}"));
            }
            return;
        }

        if let Some(parent) = path.parent() {
            if parent != self.state.listing.dir
                && let Err(e) = self.state.chdir(parent)
            {
                self.state.flash_error(format!("gf: {e}"));
                return;
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
                    self.set_pager(view);
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

    /// Run the canonical quit lifecycle: first call arms a 2-second
    /// confirm window (and flashes any running-process count); a
    /// second call inside that window persists the session and sets
    /// `should_quit`. Shared by `Action::Quit` (the Q / ^D keybindings)
    /// and the `:q` / `:quit` command — both paths must save and warn
    /// identically.
    fn request_quit(&mut self) {
        let now = std::time::Instant::now();
        if self
            .state
            .quit_pending
            .is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(2))
        {
            // If a file pager is open, capture its scroll before
            // shutdown so reopening the file in the next session
            // resumes where we left off. Bypassed close paths
            // (typically session save → quit, no Esc) would
            // otherwise drop the in-memory scroll on the floor.
            self.remember_pager_position();
            self.save_session();
            self.state.should_quit = true;
        } else {
            self.state.quit_pending = Some(now);
            let running_panes = self.runtime.pane_tabs.as_ref().map_or(0, |tabs| {
                tabs.tabs().iter().filter(|e| !e.pane.is_closed()).count()
            });
            let running_bg = self.runtime.background_tasks.running_count();
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

    /// Prefix width for history editor lines: "  NNN  " = 7 chars.
    const HIST_PREFIX_W: usize = 7;

    /// Sync the history editor after moving the picker cursor to a new line.
    /// Updates the LineEditor content and the display line.
    fn sync_history_editor_to_cursor(&mut self) {
        Self::sync_hist_editor(
            &mut self.view.pager,
            &mut self.view.pending_history_pick,
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
    /// ^D deletes the entry from history, q/Esc closes. Two triggers:
    /// `?` on an empty `J` prompt (spy parity, the short reflex), or
    /// `<Space>` while the `J` line editor is in Normal mode
    /// (a vi-style alternative for users already exploring the
    /// prompt's editor).
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
        self.view.pending_jump_history = Some(snapshot);
        self.set_pager(view);
        self.view.needs_full_repaint = true;
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
        self.view.pending_history_pick = Some(editor);
        self.set_pager(view);
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
                self.set_pager(view);
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
        let lines = help::build_lines(&self.view.theme, &self.state.user_keymap, col_w);
        let mut view = pager::PagerView::new_styled(Self::HELP_TITLE, lines);
        view.columns = ncols as u8;
        view.no_history = true;
        self.set_pager(view);
    }

    /// True when the help pager is the currently-open pager view.
    fn help_is_open(&self) -> bool {
        self.view
            .pager
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

    // --- Action handlers --------------------------------------------------

    fn activate(&mut self, intent: ActivateIntent) -> Vec<Effect> {
        let Some(row) = self.state.rows.get(self.state.cursor.index) else {
            return Vec::new();
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
                return Vec::new();
            }
            self.state.view = View::Dir;
            self.state.focus_on_path(&path);
            self.state.rebuild_rows();
            if kind == EntryKind::Dir {
                return Vec::new();
            }
        }

        // Symlinks are classified by lstat (`DirEntry::metadata`),
        // so a symlink-to-dir comes through as `Symlink`, not `Dir`.
        // Resolve through to the target for navigation so Enter does
        // the obvious thing on `node_modules/foo -> .pnpm/...`. We
        // *don't* generalize this to every op — `R`, picks, etc.
        // intentionally operate on the link itself.
        let descend = kind == EntryKind::Dir
            || (kind == EntryKind::Symlink && crate::fs::target_is_dir(&path));

        if descend {
            if let Err(e) = self.state.chdir(&path) {
                self.state.flash_error(format!("chdir: {e}"));
            }
            return Vec::new();
        }

        // File: dispatch based on intent.
        match intent {
            ActivateIntent::Display => {
                if let Some(view) = self.build_pager_view_for_file(&path) {
                    self.set_pager(view);
                }
                Vec::new()
            }
            ActivateIntent::Edit => {
                let mut argv = shell::resolve_editor();
                if argv.is_empty() {
                    return Vec::new();
                }
                let program = argv.remove(0);
                argv.push(path.to_string_lossy().into_owned());
                PostAction::Spawn {
                    program,
                    args: argv,
                    pause_after: false,
                }
                .into()
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
/// Place the OS terminal cursor at the focused pty pane's vt100
/// cursor position so alt-screen TUIs (nvim, less, htop, lazygit)
/// render a visible cursor. Without this they show no cursor at
/// all: spyc hides the host cursor at startup
/// (`main.rs::setup_terminal`), and the v1.41.18-era pane-widget
/// guard correctly stops us from painting a reverse-block over the
/// child's cursor shape in alt-screen — but the host cursor stays
/// hidden unless something asks ratatui to position it.
///
/// No-ops when the pane is missing or the child has hidden the
/// cursor via DEC ?25l (vt100 surfaces this as `hide_cursor()`).
/// Skips the call when the cursor would land outside the pane's
/// drawable rect, which can happen briefly during a resize.
fn place_pty_cursor_from_screen(
    frame: &mut Frame,
    screen: &vt100::Screen,
    rect: ratatui::layout::Rect,
) {
    if screen.hide_cursor() {
        return;
    }
    let (cy, cx) = screen.cursor_position();
    if u32::from(cy) >= u32::from(rect.height) || u32::from(cx) >= u32::from(rect.width) {
        return;
    }
    let x = rect.x + cx;
    let y = rect.y + cy;
    frame.set_cursor_position((x, y));
}

// `place_pty_cursor` removed in v1.50.84 — every pane render call-site
// now folds the cursor placement into the same `with_screen` closure
// as the widget render (single mutex window). See
// `place_pty_cursor_from_screen` for the cursor logic.

/// How long after a focus-switch chord (`^a-j` / `^a-k`) a same-key
/// Press/Repeat is treated as a stray bounce and dropped. Covers
/// system key-repeat (~30-50 ms) and kitty-keyboard Repeat events.
const POST_CHORD_BOUNCE_WINDOW: std::time::Duration = std::time::Duration::from_millis(60);

/// Whether `key` is a stray bounce of a just-completed focus-switch
/// chord that should be swallowed (rather than leaked to the now-
/// focused pane child).
///
/// `resolver_pending` is the resolver's state *before* this key is
/// fed: when a chord is already mid-flight (the user pressed `^a`
/// again), `key` is a legitimate chord completion, not a bounce — so
/// we must not swallow it. Without this clause, rapid repeated
/// `^a-j` / `^a-k` lost every chord after the first (the second `j`/`k`
/// landed inside the bounce window and was dropped before reaching the
/// resolver).
fn is_post_chord_bounce(
    stamp: Option<(std::time::Instant, KeyCode)>,
    key: KeyEvent,
    resolver_pending: bool,
) -> bool {
    let Some((at, code)) = stamp else {
        return false;
    };
    at.elapsed() < POST_CHORD_BOUNCE_WINDOW
        && key.code == code
        && matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat)
        && key.modifiers.is_empty()
        && !resolver_pending
}

/// Decide whether the watcher-driven `refresh_listing` should fire
/// this loop iteration.
///
/// A pure trailing-edge debounce (`now - last_event_at >= refresh_quiet`)
/// gets starved under continuous fs activity — cargo writing into
/// `target/`, claude/agent file streams, IDE autosave bursts — because
/// every new event resets `last_event_at` and the quiet window never
/// arrives. So we ALSO cap the wait at `max_defer` from the *first*
/// event of the current busy stretch, ensuring per-file markers can't
/// stay stale forever just because the FS won't go quiet.
fn should_fire_refresh(
    last_event_at: Option<std::time::Instant>,
    last_refresh: std::time::Instant,
    first_event_after_refresh: Option<std::time::Instant>,
    now: std::time::Instant,
    refresh_quiet: Duration,
    max_defer: Duration,
) -> bool {
    let Some(at) = last_event_at else {
        return false;
    };
    let trailing_quiet = now.duration_since(at) >= refresh_quiet;
    let max_wait_exceeded =
        first_event_after_refresh.is_some_and(|first| now.duration_since(first) >= max_defer);
    let rate_ok = now.duration_since(last_refresh) >= refresh_quiet;
    (trailing_quiet || max_wait_exceeded) && rate_ok
}

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

/// Hard cap on subdirs we will walk when deciding whether to use
/// `RecursiveMode::Recursive` for the listing watcher.
///
/// On Linux, `notify`'s recursive mode is not OS-native — it walks
/// the subtree and calls `inotify_add_watch` per directory,
/// synchronously, on the calling thread. In a tree like `$HOME`
/// with `anaconda3/` and similar deep package directories, that walk
/// runs for many seconds while the main event loop is blocked,
/// hanging the TUI. Same shape as the `MAX_ENTRIES` cap added in
/// PR #28 for `Listing::read`.
///
/// When the listing dir's subtree exceeds this cap (counted with
/// early termination, see `count_subdirs_capped`), we fall back to a
/// non-recursive watch. The 1 Hz git poll declared at the top of
/// `App::run` covers parent-row dirty-flag refresh independently of
/// the watcher, so the only feature regression is up to one second
/// of lag on "child modified → parent row dirties" — visible only
/// on the largest trees, where instant updates aren't reliable in
/// practice anyway.
///
/// macOS FSEvents is OS-level (no per-subdir walk) and is unaffected
/// by this cap: `pick_recursive_mode` returns `Recursive`
/// unconditionally on non-Linux platforms.
///
/// The chosen value is empirical, not derived: 256 is comfortably
/// above the subdir count of typical project repos (the spyc tree
/// itself, ratatui, cargo, …) and below `$HOME`-shaped trees with
/// package managers in residence (`anaconda3/`, multiple
/// `node_modules/`, `.cache/`, etc.). If real-world reports show a
/// project that ends up over the cap or a giant tree that ends up
/// under it, this is the constant to revisit.
///
/// Trades the old worst case of "blocks the event loop forever on
/// `inotify_add_watch`" for a new worst case of "walks at most
/// `MAX_RECURSIVE_WATCH_DIRS + 1` `read_dir` calls per chdir" —
/// hot-cache typical chdirs are sub-millisecond; cold-cache giant
/// trees bail at the budget in ~50 ms.
#[cfg(target_os = "linux")]
const MAX_RECURSIVE_WATCH_DIRS: usize = 256;

/// Subdir count threshold for "this is a huge working tree" — drives
/// adaptive backoff of the git poll cadence and the `git status`
/// untracked-enumeration mode (see `AppState::is_huge_tree`,
/// `AppState::chdir`). Chosen to match `MAX_RECURSIVE_WATCH_DIRS`:
/// a tree that already trips Linux's recursive-watch downgrade is
/// almost certainly the same tree where the 1 Hz `git status` poll
/// hurts. Single constant on all platforms because the huge-tree
/// signal is needed everywhere — the recursive-watch gating
/// constant stays Linux-only because only Linux's `notify` backend
/// pays the per-subdir walk cost.
pub const HUGE_TREE_SUBDIR_THRESHOLD: usize = 256;

/// Count subdirs under `root`, terminating as soon as the running
/// count exceeds `cap`. Two callers:
///
/// - **Linux** `pick_recursive_mode` (gating `RecursiveMode::Recursive`
///   watch registration; see `MAX_RECURSIVE_WATCH_DIRS`).
/// - **All platforms** `AppState::chdir` (setting the
///   `is_huge_tree` flag that drives adaptive backoff of the git
///   poll cadence and the `git status` untracked-enumeration mode
///   on huge working trees).
///
/// Traversal is DFS (via `Vec::pop`), which differs from `notify`'s
/// internal BFS. For an "is the count over `cap`" decision the order
/// doesn't matter; the DFS form keeps stack memory bounded by `cap`
/// (we stop pushing immediately on overflow).
///
/// Does not follow symlinks: `DirEntry::file_type()` is `lstat`-based
/// on Unix, so a symlink-to-dir reports as a symlink (not a dir) and
/// is not pushed onto the walk stack. This matches `notify`'s default
/// behavior — its recursive walker does not chase symlinks either, so
/// the count we produce here tracks what `notify` would have walked.
pub fn count_subdirs_capped(root: &Path, cap: usize) -> usize {
    let mut count = 0usize;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in rd.filter_map(Result::ok) {
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_dir() {
                count += 1;
                if count > cap {
                    return count;
                }
                stack.push(entry.path());
            }
        }
    }
    count
}

// `CaptureHandles` retired in v1.5 Phase 6a — `spawn_capture` now
// returns a `PtyHost` directly, the same shape `Pane` and
// `BackgroundTask` use. PTY (vs. a plain piped `Command`) is what
// stops sudo / ssh / gpg from writing their password prompts directly
// to our real terminal: inside the child, `/dev/tty` resolves to the
// slave PTY, so those bytes flow back through the master and into
// the pager buffer.

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
/// Build the EOF marker line appended to captures / finished tasks
/// so the "command finished" indicator stays visible at the bottom
/// of the pager even when content fills the viewport. `tail` is
/// rendered after the literal `[EOF — `; pass the exit string
/// (`"exit 0"`, `"killed"`, `"error: ..."`) or any other short
/// status the caller wants surfaced.
/// Format a `Duration` in seconds as a compact human string for
/// the activity-monitor uptime field. Forms:
/// - `< 1 m`: `Ns`
/// - `< 1 h`: `Nm Ns`
/// - `< 1 d`: `Nh NNm`
/// - `>= 1 d`: `Nd Nh`
fn format_uptime(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else if secs < 86_400 {
        format!("{}h {:02}m", secs / 3600, (secs % 3600) / 60)
    } else {
        format!("{}d {}h", secs / 86_400, (secs % 86_400) / 3600)
    }
}

fn eof_marker_line(tail: &str) -> ratatui::text::Line<'static> {
    use ratatui::style::{Modifier, Style};
    use ratatui::text::{Line, Span};
    Line::from(Span::styled(
        format!("[EOF — {tail}]"),
        Style::default().add_modifier(Modifier::DIM),
    ))
}

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

fn spawn_capture(cmd: &str, cwd: &std::path::Path) -> Result<crate::pane::pty_host::PtyHost> {
    let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    // TERM=dumb so TUI programs refuse to run as a TUI inside our
    // capture (the pager only renders SGR colors and CRLF, not
    // cursor positioning / alt-screen). FORCE_COLOR / CLICOLOR_FORCE
    // / COLORTERM ride alongside so tools that respect those (cargo,
    // eza, bat, ripgrep) keep their color output anyway. PAGER=cat /
    // GIT_PAGER=cat / MANPAGER=cat stops tools that auto-invoke a
    // sub-pager (git log, man) from launching `less` against our
    // pty and freezing the capture.
    let env: &[(&str, &str)] = &[
        ("CLICOLOR_FORCE", "1"),
        ("FORCE_COLOR", "1"),
        ("COLORTERM", "truecolor"),
        ("PAGER", "cat"),
        ("GIT_PAGER", "cat"),
        ("MANPAGER", "cat"),
    ];
    crate::pane::pty_host::PtyHost::spawn(crate::pane::pty_host::PtySpec {
        command: cmd,
        rows,
        cols,
        cwd,
        env,
        term: "dumb",
        // No SIGWINCH nudge for captures — they aren't interactive
        // shells with rc-file prompts to redraw.
        nudge_winch: false,
        // Captures don't enable the per-byte debug dump (Pane does).
        debug_dump: false,
    })
}

/// Build a `ForegroundExec` effect that runs `cmd` through `sh -c` so shell features
/// (pipes, redirection, `$VAR`) work.
fn sh_c(cmd: &str, pause_after: bool) -> Vec<Effect> {
    PostAction::Spawn {
        program: "sh".to_string(),
        args: vec!["-c".to_string(), cmd.to_string()],
        pause_after,
    }
    .into()
}

/// Owns the parkable crossterm input-reader thread (MVU Phase 1). The
/// reader becomes the SOLE caller of `event::poll`/`event::read`. Modeled
/// on `ParserWorker` (src/pane/mod.rs): a stop flag set on `Drop`, then
/// `unpark` + `join`. See `docs/MVU_PLAN.md` Phase 1 for the
/// park/ack/drain handshake the executor below relies on.
struct ReaderHandle {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    park: std::sync::Arc<std::sync::atomic::AtomicBool>,
    acked: std::sync::Arc<std::sync::atomic::AtomicBool>,
    reader_done: std::sync::Arc<std::sync::atomic::AtomicBool>,
    read_err: std::sync::Arc<std::sync::Mutex<Option<std::io::Error>>>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Drop for ReaderHandle {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Release);
        if let Some(h) = self.handle.take() {
            h.thread().unpark(); // in case it's parked
            let _ = h.join();
        }
    }
}

/// Spawn the parkable input reader. It uses a FINITE `event::poll(10ms)`
/// loop — never a bare `event::read()`, which would pin crossterm's
/// process-global reader mutex indefinitely — so a parked reader holds
/// no lock and issues no tty read, leaving a foreground child's stdin
/// uncontended. On park it drains crossterm's buffered events to empty
/// (dropping them) BEFORE acking, so nothing is stranded across the
/// child's tty ownership.
fn spawn_input_reader(tx: std::sync::mpsc::Sender<Message>) -> ReaderHandle {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering::{Acquire, Release};

    let stop = Arc::new(AtomicBool::new(false));
    let park = Arc::new(AtomicBool::new(false));
    let acked = Arc::new(AtomicBool::new(false));
    let reader_done = Arc::new(AtomicBool::new(false));
    let read_err = Arc::new(std::sync::Mutex::new(None));

    let handle = {
        let stop = stop.clone();
        let park = park.clone();
        let acked = acked.clone();
        let reader_done = reader_done.clone();
        let read_err = read_err.clone();
        std::thread::Builder::new()
            .name("spyc-input-reader".to_string())
            .spawn(move || {
                // Park-latency bound — NOT the loop cadence. 10ms <= the
                // 16ms typing tier, so input still surfaces within one
                // main-loop tick.
                const READER_POLL: Duration = Duration::from_millis(10);
                loop {
                    if stop.load(Acquire) {
                        reader_done.store(true, Release);
                        return;
                    }
                    if park.load(Acquire) {
                        // Reached only at loop top, i.e. AFTER any poll/read
                        // returned: we hold no crossterm lock and issue no
                        // tty read. Drain buffered events to empty (dropped)
                        // so none is stranded across the child's tty
                        // ownership, then ack and park. Spurious unparks are
                        // safe — the loop top re-checks `park`.
                        while matches!(event::poll(Duration::ZERO), Ok(true)) {
                            if event::read().is_err() {
                                break;
                            }
                        }
                        acked.store(true, Release);
                        std::thread::park();
                        continue;
                    }
                    match event::poll(READER_POLL) {
                        Ok(true) => match event::read() {
                            Ok(ev) => {
                                // Press-filter for Key (verbatim from the
                                // old inline guard); everything else
                                // (Paste/Resize/Focus/Mouse) forwarded.
                                let forward = match &ev {
                                    Event::Key(k) => {
                                        k.kind == KeyEventKind::Press
                                            || k.kind == KeyEventKind::Repeat
                                    }
                                    _ => true,
                                };
                                if forward && tx.send(Message::Input(ev)).is_err() {
                                    reader_done.store(true, Release); // main loop gone
                                    return;
                                }
                            }
                            Err(e) => {
                                *read_err.lock().unwrap() = Some(e);
                                reader_done.store(true, Release);
                                // MVU Phase 3d death-wake: store THEN send, so
                                // the loop-top Acquire-load sees the error. With
                                // no poll floor, this kicks the blocking recv.
                                let _ = tx.send(Message::ReaderExited);
                                return;
                            }
                        },
                        Ok(false) => {} // poll timeout — re-check stop/park
                        Err(e) => {
                            *read_err.lock().unwrap() = Some(e);
                            reader_done.store(true, Release);
                            let _ = tx.send(Message::ReaderExited); // death-wake (see above)
                            return;
                        }
                    }
                }
            })
            .expect("spawn spyc-input-reader thread")
    };

    ReaderHandle {
        stop,
        park,
        acked,
        reader_done,
        read_err,
        handle: Some(handle),
    }
}

/// Parking-aware wrapper around `run_child_in_foreground` (MVU Phase 1).
/// Synchronously parks + acks + drains the input reader BEFORE the child
/// takes the tty, and re-arms it after — so only the child reads stdin
/// during the takeover (no keystroke leakage either direction).
struct ForegroundExec {
    park: std::sync::Arc<std::sync::atomic::AtomicBool>,
    acked: std::sync::Arc<std::sync::atomic::AtomicBool>,
    reader_done: std::sync::Arc<std::sync::atomic::AtomicBool>,
    reader: std::thread::Thread,
}

impl ForegroundExec {
    /// Quiesce the reader: clear any stale ack, request park, then wait
    /// (bounded, ~200 ms) for the ack — at which point the reader has
    /// returned from poll/read AND drained crossterm's buffers, so the
    /// tty is clean for the child. Returns early if the reader has
    /// already exited (`reader_done`). Bounded so a descheduled reader
    /// can't freeze the UI on an editor/pager launch.
    fn park_and_wait(&self) {
        use std::sync::atomic::Ordering::{Acquire, Release};
        self.acked.store(false, Release);
        self.park.store(true, Release);
        let deadline = std::time::Instant::now() + Duration::from_millis(200);
        while !self.acked.load(Acquire) {
            if self.reader_done.load(Acquire) {
                break; // reader exited — provably not reading the tty
            }
            if std::time::Instant::now() >= deadline {
                spyc_debug!("FG park ack timed out; proceeding");
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Re-arm the reader after the child returns (clear park BEFORE
    /// unpark — ordering matters so a spurious wake can't strand it).
    fn unpark_reader(&self) {
        self.park.store(false, std::sync::atomic::Ordering::Release);
        self.reader.unpark();
    }

    fn run(
        &self,
        terminal: &mut Tui,
        program: &str,
        args: &[String],
        pause_after: bool,
    ) -> Result<()> {
        self.park_and_wait();
        // The existing takeover, byte-for-byte unchanged.
        let r = run_child_in_foreground(terminal, program, args, pause_after);
        self.unpark_reader();
        r
    }
}

#[cfg(test)]
mod foreground_exec_tests {
    use super::ForegroundExec;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering::{Acquire, Release};
    use std::time::{Duration, Instant};

    /// The ForegroundExec park handshake acks (proving the reader
    /// quiesced before a child would take the tty) and a second cycle
    /// re-acks (proving unpark resumed the parked reader). Driven by a
    /// stub reader mirroring the real reader's park branch — CI-safe, no
    /// tty / no App::run.
    #[test]
    fn park_handshake_acks_and_resumes() {
        let stop = Arc::new(AtomicBool::new(false));
        let park = Arc::new(AtomicBool::new(false));
        let acked = Arc::new(AtomicBool::new(false));
        let reader_done = Arc::new(AtomicBool::new(false));
        let handle = {
            let (stop, park, acked) = (stop.clone(), park.clone(), acked.clone());
            std::thread::spawn(move || {
                loop {
                    if stop.load(Acquire) {
                        return;
                    }
                    if park.load(Acquire) {
                        acked.store(true, Release);
                        std::thread::park();
                        continue;
                    }
                    std::thread::sleep(Duration::from_millis(1));
                }
            })
        };
        let fe = ForegroundExec {
            park,
            acked: acked.clone(),
            reader_done,
            reader: handle.thread().clone(),
        };

        fe.park_and_wait();
        assert!(acked.load(Acquire), "reader should ack the park");

        fe.unpark_reader();
        fe.park_and_wait();
        assert!(acked.load(Acquire), "reader should re-ack after unpark");

        stop.store(true, Release);
        handle.thread().unpark();
        let _ = handle.join();
    }

    /// `park_and_wait` short-circuits (does not burn the full ~200 ms
    /// deadline) when the reader has already exited, and records no ack.
    #[test]
    fn wait_short_circuits_on_reader_done() {
        let park = Arc::new(AtomicBool::new(false));
        let acked = Arc::new(AtomicBool::new(false));
        let reader_done = Arc::new(AtomicBool::new(true)); // already exited
        let dummy = std::thread::spawn(std::thread::park);
        let fe = ForegroundExec {
            park,
            acked: acked.clone(),
            reader_done,
            reader: dummy.thread().clone(),
        };

        let start = Instant::now();
        fe.park_and_wait();
        assert!(
            start.elapsed() < Duration::from_millis(150),
            "must short-circuit on reader_done, not wait the full deadline"
        );
        assert!(
            !acked.load(Acquire),
            "no ack when the reader is already done"
        );

        dummy.thread().unpark();
        let _ = dummy.join();
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
    let saved_pgid = {
        use std::os::fd::AsFd;
        let our_pgid = rustix::process::getpgrp();
        if let Some(child_pid) = rustix::process::Pid::from_raw(child.id() as i32) {
            let stdin = std::io::stdin();
            let _ = rustix::termios::tcsetpgrp(stdin.as_fd(), child_pid);
        }
        our_pgid
    };

    // Ignoring status on purpose: non-zero exits (e.g. less with `q`, or a
    // grep that found nothing) are normal and should not crash spyc.
    let _ = child.wait();

    // Restore tty foreground to spyc's group. Without this, the next
    // tty input would still be delivered to the child's (now-dead)
    // group and the kernel would EIO subsequent reads.
    #[cfg(unix)]
    {
        use std::os::fd::AsFd;
        let stdin = std::io::stdin();
        let _ = rustix::termios::tcsetpgrp(stdin.as_fd(), saved_pgid);
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

/// `kill(-pid, sig)` — signal the process group leadered by `pid`.
/// portable-pty calls `setsid` on spawn, so the child IS the group
/// leader; negative-pid targets reach grandchildren too. Returns the
/// underlying syscall result so background-task callers can flash
/// the user-facing success/failure message.
///
/// `Pid::from_raw` rejects zero (which would mean "current process
/// group" — a footgun if the child id was somehow 0); on that path
/// we synthesize an `ESRCH` so the caller flashes the same "failed"
/// branch as a real kill failure.
#[cfg(unix)]
fn kill_pg(pid: u32, sig: rustix::process::Signal) -> rustix::io::Result<()> {
    match rustix::process::Pid::from_raw(pid as i32) {
        Some(rpid) => rustix::process::kill_process_group(rpid, sig),
        None => Err(rustix::io::Errno::SRCH),
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
    if let Ok(h) = std::env::var("HOSTNAME")
        && !h.is_empty()
    {
        return h;
    }
    if let Ok(out) = std::process::Command::new("hostname").output()
        && out.status.success()
    {
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !s.is_empty() {
            return s;
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
impl App {
    /// Test-only `App` constructor for workflow-harness tests
    /// (`docs/TEST_IMPROVEMENT_PLAN.md` Phase 1). Builds a deterministic
    /// `App` with **no** terminal, **no** MCP socket server, **no**
    /// git-status worker thread, and **no** real-env cwd — unlike
    /// `App::new`. Drive it with `apply(&Action)` / `handle_key(KeyEvent)`
    /// and assert on `self.state.*`, `self.runtime.pane_tabs`, `self.view.pager`, etc.
    ///
    /// Wrap callers in `crate::state::with_state_root(tmp, || …)` so the
    /// history / pager-position / inventory state dir is an isolated temp.
    pub(crate) fn test_app(cwd: std::path::PathBuf) -> Self {
        // No MCP server / git worker is spawned. The harness never drives
        // `run()`'s drain loop, and `apply` / `handle_key` don't read these
        // receivers, so both `mcp_cmd_rx` and `git_result_rx` are `None`
        // (Phase 3a/3d: `run()` is the only `.take()` site).
        let context_path = crate::context::context_path(&cwd);
        let mut app = Self {
            state: state::AppState::test_default(cwd),
            view: ViewState::new(Theme::default(), context_path, false, false),
            exit_summary: None,
            runtime: Runtime {
                git_result_rx: None,
                mcp_cmd_rx: None,
                pane_wake_tx: None,
                next_sink_id: 0,
                pane_tabs: None,
                top_overlay: None,
                pending_capture: None,
                background_tasks: BackgroundTasks::new(),
                find_picker: None,
                grep_session: None,
                next_grep_id: 0,
                agent_status_pending: std::sync::Arc::new(std::sync::Mutex::new(None)),
                agent_status_refreshing: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(
                    false,
                )),
            },
        };
        app.state.rebuild_rows();
        app
    }

    /// Seed the listing with fake file rows (no real fs), mirroring the
    /// `state_with_rows` pattern; clamps the cursor into range.
    pub(crate) fn seed_rows(&mut self, names: &[&str]) {
        let dir = self.state.listing.dir.clone();
        self.state.rows = names
            .iter()
            .map(|n| RowData {
                path: dir.join(n),
                display: (*n).to_string(),
                kind: EntryKind::File,
            })
            .collect();
        self.state.cursor.clamp(self.state.rows.len());
    }

    /// Flash message text, if any — compact assertion helper.
    pub(crate) fn flash_text(&self) -> Option<&str> {
        self.state.flash.as_ref().map(|f| f.text.as_str())
    }
}

#[cfg(test)]
mod harness_tests {
    use super::*;
    use crate::keymap::Action;

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::empty())
    }

    /// Acceptance: a fresh harness starts with a deterministic cwd,
    /// listing, cursor, focus, and no pane/pager.
    #[test]
    fn fresh_harness_is_deterministic() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let app = App::test_app(std::path::PathBuf::from("/tmp/harness"));
            assert_eq!(app.state.focus, state::Focus::FileList);
            assert!(!app.state.pane_focused());
            assert!(matches!(app.state.mode, Mode::Normal));
            assert_eq!(app.state.cursor.index, 0);
            assert!(app.runtime.pane_tabs.is_none());
            assert!(app.view.pager.is_none());
            assert!(app.flash_text().is_none());
            assert_eq!(
                app.state.listing.dir,
                std::path::PathBuf::from("/tmp/harness")
            );
        });
    }

    /// Acceptance: the harness can apply an `Action` and observe the
    /// resulting state (cursor movement here) plus a `PostAction`.
    #[test]
    fn apply_action_moves_cursor() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(std::path::PathBuf::from("/tmp/harness"));
            app.seed_rows(&["a", "b", "c"]);
            assert_eq!(app.state.cursor.index, 0);
            let post = app.apply(&Action::Down(1)).unwrap();
            assert_eq!(app.state.cursor.index, 1);
            assert!(post.is_empty());
            app.apply(&Action::Up(1)).unwrap();
            assert_eq!(app.state.cursor.index, 0);
        });
    }

    /// PR 5b: `gf`/`gF` emit a `ReadPaneText`/`GotoFile` effect (the pickable
    /// read + navigation run in `run_effects`); `gF` sets `open_at_line`.
    #[test]
    fn goto_file_actions_emit_read_pane_text_pickable() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(std::path::PathBuf::from("/tmp/harness"));
            match app.apply(&Action::GotoFile).unwrap().as_slice() {
                [
                    Effect::ReadPaneText {
                        kind: PaneTextKind::Pickable(200),
                        then:
                            PaneTextSink::GotoFile {
                                open_at_line: false,
                            },
                    },
                ] => {}
                other => panic!("gf: expected ReadPaneText Pickable(200)+GotoFile, got {other:?}"),
            }
            match app.apply(&Action::GotoFileLine).unwrap().as_slice() {
                [
                    Effect::ReadPaneText {
                        kind: PaneTextKind::Pickable(200),
                        then: PaneTextSink::GotoFile { open_at_line: true },
                    },
                ] => {}
                other => panic!("gF: expected open_at_line=true, got {other:?}"),
            }
        });
    }

    /// Acceptance: a `KeyEvent` routes through the full `handle_key`
    /// path (resolver → route → dispatch) with no pane/overlay open.
    #[test]
    fn handle_key_routes_j_to_cursor_down() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(std::path::PathBuf::from("/tmp/harness"));
            app.seed_rows(&["a", "b", "c"]);
            app.handle_key(key('j')).unwrap();
            assert_eq!(app.state.cursor.index, 1, "j should move the cursor down");
        });
    }

    /// PR4: the term-title compose + dedup stay loop-side. First call
    /// emits a `SetTerminalTitle` effect; an unchanged title dedups to
    /// `None` (so `term_title::set` only runs when the title changed).
    #[test]
    fn term_title_effect_emits_then_dedups() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(std::path::PathBuf::from("/tmp/harness"));
            assert!(
                matches!(
                    app.term_title_effect(),
                    Some(Effect::SetTerminalTitle { .. })
                ),
                "first call emits the title effect"
            );
            assert!(
                app.term_title_effect().is_none(),
                "unchanged title is deduped to None"
            );
        });
    }

    /// PR4: the send/pipe pre-pane guards still short-circuit with no
    /// effect (and flash inline) when no pane is open.
    #[test]
    fn send_and_pipe_no_pane_emit_no_effect() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(std::path::PathBuf::from("/tmp/harness"));
            assert!(
                app.send_selection_to_pane().is_empty(),
                "no-pane send emits nothing"
            );
            assert!(
                app.pipe_content_to_pane(false).is_empty(),
                "no-pane pipe emits nothing"
            );
        });
    }

    fn esc() -> KeyEvent {
        KeyEvent::new(KeyCode::Esc, KeyModifiers::empty())
    }

    /// Routing: while a prompt is open, a printable key edits the prompt
    /// buffer and does NOT move the list cursor (prompt wins).
    #[test]
    fn prompt_input_wins_over_list() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(std::path::PathBuf::from("/tmp/harness"));
            app.seed_rows(&["a", "b", "c"]);
            app.state.mode = Mode::Prompting(Prompt::simple(PromptKind::Jump, "jump: "));
            app.handle_key(key('x')).unwrap();
            assert_eq!(
                app.state.cursor.index, 0,
                "cursor must not move while prompting"
            );
            match &app.state.mode {
                Mode::Prompting(p) => assert_eq!(p.buffer, "x"),
                Mode::Normal => panic!("prompt should still be open"),
            }
        });
    }

    /// Routing: an Overlay-mounted in-app pager consumes normal keys —
    /// `j` is handled by the pager, the list cursor stays put.
    #[test]
    fn overlay_pager_consumes_keys_not_list() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(std::path::PathBuf::from("/tmp/harness"));
            app.seed_rows(&["a", "b", "c"]);
            let lines: Vec<String> = (0..50).map(|i| format!("line {i}")).collect();
            app.view.pager = Some(PagerView::new_plain("t", lines));
            app.handle_key(key('j')).unwrap();
            assert_eq!(
                app.state.cursor.index, 0,
                "list cursor must not move with a pager open"
            );
            assert!(app.view.pager.is_some(), "pager stays open on j");
        });
    }

    /// Routing: Esc on an open overlay pager closes it.
    #[test]
    fn esc_closes_overlay_pager() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(std::path::PathBuf::from("/tmp/harness"));
            app.view.pager = Some(PagerView::new_plain("t", vec!["a".to_string()]));
            app.handle_key(esc()).unwrap();
            assert!(app.view.pager.is_none(), "Esc should close the pager");
        });
    }
}

#[cfg(test)]
mod guard_tests {
    /// Anti-monolith guardrail. `app/mod.rs` was a ~12k-line monolith;
    /// REFACTOR_PLAN Phases 1–2 decomposed it into focused `src/app/`
    /// modules. This test fails if `mod.rs` creeps back toward that —
    /// new render/key/command/action/session logic belongs in the
    /// matching child module (or a new one), not appended here.
    ///
    /// If you hit this: extract a module, don't bump the ceiling. The
    /// ceiling sits well below the old monolith and comfortably above
    /// what legitimately stays in `mod.rs` (the `App` struct, `run`
    /// event loop, and small glue), so tripping it means something that
    /// should be its own module landed here instead. See AGENTS.md →
    /// "Keep `src/app/` modularized".
    #[test]
    fn mod_rs_stays_decomposed() {
        const CEILING: usize = 8_500;
        let src = include_str!("mod.rs");
        let lines = src.lines().count();
        assert!(
            lines <= CEILING,
            "src/app/mod.rs is {lines} lines, over the {CEILING}-line \
             anti-monolith ceiling. Extract logic into a src/app/ child \
             module instead of growing mod.rs (see AGENTS.md). Don't just \
             raise CEILING."
        );
    }
}

#[cfg(test)]
mod refresh_debounce_tests {
    use super::should_fire_refresh;
    use std::time::{Duration, Instant};

    const QUIET: Duration = Duration::from_millis(500);
    const MAX_DEFER: Duration = Duration::from_secs(1);

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    /// MVU Phase 2 pins the RefreshQuiet armed instant to the
    /// `should_fire_refresh` predicate edge: it must be true AT the armed
    /// instant and false 1 ms before, so the scheduler can never drive the
    /// recv wait to zero by arming before the predicate can fire. The
    /// `fire_at` here is the exact formula used to arm `Deadline::RefreshQuiet`.
    #[test]
    fn fires_exactly_at_the_armed_edge() {
        let base = Instant::now();
        let last_refresh = base;
        let last_event = at(base, 100);
        let first_event = at(base, 100);
        // App::run arms at: max(last_refresh+QUIET, min(last_event+QUIET,
        // first+MAX_DEFER)) = max(base+500, min(base+600, base+1100)) = base+600.
        let fire_at = (last_refresh + QUIET).max((last_event + QUIET).min(first_event + MAX_DEFER));
        assert_eq!(fire_at, at(base, 600));
        assert!(should_fire_refresh(
            Some(last_event),
            last_refresh,
            Some(first_event),
            fire_at,
            QUIET,
            MAX_DEFER
        ));
        // 1 ms before the edge (base+599): the predicate must NOT fire.
        assert!(!should_fire_refresh(
            Some(last_event),
            last_refresh,
            Some(first_event),
            at(base, 599),
            QUIET,
            MAX_DEFER
        ));
    }

    #[test]
    fn no_pending_event_never_fires() {
        let base = Instant::now();
        assert!(!should_fire_refresh(
            None,
            base,
            None,
            at(base, 5_000),
            QUIET,
            MAX_DEFER
        ));
    }

    #[test]
    fn fires_after_trailing_quiet() {
        let base = Instant::now();
        // Last event at t=0, now at t=600 ms → 600 ms of quiet → fire.
        assert!(should_fire_refresh(
            Some(base),
            base,
            Some(base),
            at(base, 600),
            QUIET,
            MAX_DEFER
        ));
    }

    #[test]
    fn waits_during_trailing_quiet_window() {
        let base = Instant::now();
        // Last event at t=400, now at t=500 → only 100 ms of quiet → wait.
        assert!(!should_fire_refresh(
            Some(at(base, 400)),
            base,
            Some(base),
            at(base, 500),
            QUIET,
            MAX_DEFER
        ));
    }

    #[test]
    fn max_defer_breaks_starvation_under_continuous_activity() {
        // The regression: events keep arriving so trailing-quiet is
        // never met, but the first-event-of-this-stretch was >= max_defer
        // ago → fire anyway so markers don't stay stale forever.
        let base = Instant::now();
        let still_active = at(base, 1_100); // last event 100 ms ago — NOT quiet
        let now = at(base, 1_200);
        let first_event = base; // 1.2 s ago, > MAX_DEFER (1 s)
        assert!(should_fire_refresh(
            Some(still_active),
            base,
            Some(first_event),
            now,
            QUIET,
            MAX_DEFER
        ));
    }

    #[test]
    fn rate_limit_blocks_back_to_back_fires() {
        let base = Instant::now();
        // Trailing quiet met but last_refresh was only 100 ms ago → wait.
        assert!(!should_fire_refresh(
            Some(base),
            at(base, 400), // last_refresh 100 ms before now
            Some(base),
            at(base, 500),
            QUIET,
            MAX_DEFER
        ));
    }

    #[test]
    fn rate_limit_also_gates_max_defer_path() {
        // Even when max-defer would fire, we still respect the rate
        // limit so we never refresh twice within `refresh_quiet`.
        let base = Instant::now();
        let now = at(base, 1_200);
        assert!(!should_fire_refresh(
            Some(at(base, 1_100)), // not quiet
            at(base, 900),         // last_refresh 300 ms ago — too recent
            Some(base),            // first_event 1.2 s ago — max_defer hit
            now,
            QUIET,
            MAX_DEFER
        ));
    }
}

#[cfg(test)]
mod post_chord_bounce_tests {
    use super::{POST_CHORD_BOUNCE_WINDOW, is_post_chord_bounce};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::time::{Duration, Instant};

    fn press(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    #[test]
    fn swallows_same_key_bounce_in_window_when_idle() {
        // `^a-j` just completed; a stray `j` within the window with the
        // resolver idle is a bounce → swallow.
        let stamp = Some((Instant::now(), KeyCode::Char('j')));
        assert!(is_post_chord_bounce(stamp, press('j'), false));
    }

    #[test]
    fn does_not_swallow_when_resolver_pending() {
        // The regression: a fresh `^a` made the resolver pending, so the
        // incoming `j` completes a NEW chord and must reach the resolver.
        let stamp = Some((Instant::now(), KeyCode::Char('j')));
        assert!(!is_post_chord_bounce(stamp, press('j'), true));
    }

    #[test]
    fn does_not_swallow_different_key() {
        let stamp = Some((Instant::now(), KeyCode::Char('j')));
        assert!(!is_post_chord_bounce(stamp, press('k'), false));
    }

    #[test]
    fn does_not_swallow_with_modifiers() {
        // A second `^a` (Ctrl-A) must never be swallowed as a bounce.
        let stamp = Some((Instant::now(), KeyCode::Char('a')));
        let ctrl_a = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL);
        assert!(!is_post_chord_bounce(stamp, ctrl_a, false));
    }

    #[test]
    fn does_not_swallow_after_window_expires() {
        let past = Instant::now()
            .checked_sub(POST_CHORD_BOUNCE_WINDOW + Duration::from_millis(40))
            .unwrap();
        let stamp = Some((past, KeyCode::Char('j')));
        assert!(!is_post_chord_bounce(stamp, press('j'), false));
    }

    #[test]
    fn no_stamp_never_swallows() {
        assert!(!is_post_chord_bounce(None, press('j'), false));
    }
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
mod history_bucket_tests {
    use super::{HistoryBucket, PromptKind, history_bucket_for};

    #[test]
    fn pane_command_and_cwd_use_distinct_buckets() {
        // The bug this guards: both pane prompts shared one bucket, so
        // directories typed at "pane cwd:" leaked into the "pane
        // command:" Up/Down browse.
        assert_eq!(
            history_bucket_for(Some(&PromptKind::PaneNewTabCmd)),
            HistoryBucket::PaneCmd
        );
        assert_eq!(
            history_bucket_for(Some(&PromptKind::PaneNewTabCwd)),
            HistoryBucket::PaneCwd
        );
        assert_ne!(HistoryBucket::PaneCmd, HistoryBucket::PaneCwd);
    }

    #[test]
    fn jump_and_command_stay_isolated() {
        assert_eq!(
            history_bucket_for(Some(&PromptKind::Jump)),
            HistoryBucket::Jump
        );
        assert_eq!(
            history_bucket_for(Some(&PromptKind::Command)),
            HistoryBucket::Command
        );
    }

    #[test]
    fn shell_and_path_prompts_fall_back_to_shell_bucket() {
        assert_eq!(
            history_bucket_for(Some(&PromptKind::ShellCmd)),
            HistoryBucket::Shell
        );
        assert_eq!(
            history_bucket_for(Some(&PromptKind::CopyTo)),
            HistoryBucket::Shell
        );
        // Normal mode (no prompt) also resolves to the default bucket.
        assert_eq!(history_bucket_for(None), HistoryBucket::Shell);
    }
}

#[cfg(test)]
mod format_uptime_tests {
    use super::format_uptime;

    #[test]
    fn seconds_only() {
        assert_eq!(format_uptime(0), "0s");
        assert_eq!(format_uptime(59), "59s");
    }

    #[test]
    fn minutes_and_seconds() {
        assert_eq!(format_uptime(60), "1m 0s");
        assert_eq!(format_uptime(125), "2m 5s");
    }

    #[test]
    fn hours_and_minutes() {
        assert_eq!(format_uptime(3600), "1h 00m");
        assert_eq!(format_uptime(3725), "1h 02m");
    }

    #[test]
    fn days_and_hours() {
        assert_eq!(format_uptime(86_400), "1d 0h");
        assert_eq!(format_uptime(90_000), "1d 1h");
    }
}

#[cfg(test)]
mod eof_marker_tests {
    use super::eof_marker_line;

    fn flat(line: &ratatui::text::Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn exit_zero_renders_with_tail() {
        let line = eof_marker_line("exit 0");
        assert_eq!(flat(&line), "[EOF — exit 0]");
    }

    #[test]
    fn killed_status_renders() {
        let line = eof_marker_line("killed (12s)");
        assert_eq!(flat(&line), "[EOF — killed (12s)]");
    }

    #[test]
    fn marker_is_dim() {
        use ratatui::style::Modifier;
        let line = eof_marker_line("exit 1");
        assert!(line.spans[0].style.add_modifier.contains(Modifier::DIM));
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

#[cfg(test)]
mod gemini_helpers_tests {
    use super::{command_without_gemini_resume, parse_gemini_list_sessions_for_uuid};

    // ── command_without_gemini_resume ─────────────────────────────

    #[test]
    fn strips_long_resume_with_value() {
        assert_eq!(command_without_gemini_resume("gemini --resume 5"), "gemini");
    }

    #[test]
    fn strips_short_resume_with_value() {
        assert_eq!(command_without_gemini_resume("gemini -r latest"), "gemini");
    }

    #[test]
    fn strips_resume_with_equals_form() {
        assert_eq!(command_without_gemini_resume("gemini --resume=3"), "gemini");
    }

    #[test]
    fn strips_session_id_flag() {
        assert_eq!(
            command_without_gemini_resume(
                "gemini --session-id 11111111-1111-1111-1111-111111111111"
            ),
            "gemini"
        );
    }

    #[test]
    fn preserves_unrelated_flags() {
        assert_eq!(
            command_without_gemini_resume("gemini -y --model flash --resume 2"),
            "gemini -y --model flash"
        );
    }

    #[test]
    fn empty_input_falls_back_to_gemini() {
        assert_eq!(command_without_gemini_resume(""), "gemini");
    }

    // ── parse_gemini_list_sessions_for_uuid ────────────────────────

    #[test]
    fn parses_real_world_listing() {
        let stdout = "Available sessions for this project (2):
  1. let's do a code review of this app (1 day ago) [76422c62-ea2f-4334-8e3d-45fba862d149]
  2. Analyze project for bugs and provide recommendations. (1 day ago) [4a7cd126-f849-47c2-8035-80a07c807544]
The 'metricReader' option is deprecated. Please use 'metricReaders' instead.
";
        assert_eq!(
            parse_gemini_list_sessions_for_uuid(stdout, "76422c62-ea2f-4334-8e3d-45fba862d149"),
            Some(1)
        );
        assert_eq!(
            parse_gemini_list_sessions_for_uuid(stdout, "4a7cd126-f849-47c2-8035-80a07c807544"),
            Some(2)
        );
    }

    #[test]
    fn returns_none_for_unknown_uuid() {
        let stdout = "  1. only session [11111111-1111-1111-1111-111111111111]\n";
        assert!(
            parse_gemini_list_sessions_for_uuid(stdout, "22222222-2222-2222-2222-222222222222")
                .is_none()
        );
    }

    #[test]
    fn matches_uuid_case_insensitively() {
        // Defensive: gemini emits lowercase but match either way.
        let stdout = "  3. example [76422C62-EA2F-4334-8E3D-45FBA862D149]\n";
        assert_eq!(
            parse_gemini_list_sessions_for_uuid(stdout, "76422c62-ea2f-4334-8e3d-45fba862d149"),
            Some(3)
        );
    }

    #[test]
    fn skips_lines_without_brackets_or_index() {
        // The header / trailing deprecation warning must not derail
        // the per-line parse.
        let stdout = "Available sessions for this project (1):\n  1. example (1 day ago) [11111111-1111-1111-1111-111111111111]\nThe 'metricReader' option is deprecated.\n";
        assert_eq!(
            parse_gemini_list_sessions_for_uuid(stdout, "11111111-1111-1111-1111-111111111111"),
            Some(1)
        );
    }

    #[test]
    fn rejects_malformed_index() {
        // If the leading token can't parse as a number we just skip
        // the line, never returning the wrong index.
        let stdout = "  X. malformed [11111111-1111-1111-1111-111111111111]\n";
        assert!(
            parse_gemini_list_sessions_for_uuid(stdout, "11111111-1111-1111-1111-111111111111")
                .is_none()
        );
    }
}

#[cfg(test)]
mod agy_helpers_tests {
    use super::command_without_agy_resume;

    #[test]
    fn strips_conversation_with_value() {
        assert_eq!(
            command_without_agy_resume("agy --conversation 11111111-1111-1111-1111-111111111111"),
            "agy"
        );
    }

    #[test]
    fn strips_c_with_value() {
        assert_eq!(
            command_without_agy_resume("agy -c 11111111-1111-1111-1111-111111111111"),
            "agy"
        );
    }

    #[test]
    fn strips_conversation_equals_value() {
        assert_eq!(
            command_without_agy_resume("agy --conversation=11111111-1111-1111-1111-111111111111"),
            "agy"
        );
    }

    #[test]
    fn strips_c_equals_value() {
        assert_eq!(
            command_without_agy_resume("agy -c=11111111-1111-1111-1111-111111111111"),
            "agy"
        );
    }

    #[test]
    fn strips_continue_flag() {
        assert_eq!(command_without_agy_resume("agy --continue"), "agy");
    }

    #[test]
    fn preserves_unrelated_flags() {
        assert_eq!(
            command_without_agy_resume("agy --print \"hello\" --continue"),
            "agy --print \"hello\""
        );
    }

    #[test]
    fn empty_input_falls_back_to_agy() {
        assert_eq!(command_without_agy_resume(""), "agy");
    }
}

#[cfg(test)]
mod listing_watch_tests {
    use super::count_subdirs_capped;
    use std::fs;

    #[test]
    fn empty_dir_counts_zero() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(count_subdirs_capped(tmp.path(), 10), 0);
    }

    #[test]
    fn count_under_cap_returns_total() {
        let tmp = tempfile::tempdir().unwrap();
        for i in 0..5 {
            fs::create_dir(tmp.path().join(format!("sub{i}"))).unwrap();
        }
        assert_eq!(count_subdirs_capped(tmp.path(), 10), 5);
    }

    #[test]
    fn count_descends_into_nested_subdirs() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a");
        fs::create_dir(&a).unwrap();
        let b = a.join("b");
        fs::create_dir(&b).unwrap();
        fs::create_dir(b.join("c")).unwrap();
        // a, a/b, a/b/c = 3 dirs.
        assert_eq!(count_subdirs_capped(tmp.path(), 10), 3);
    }

    #[test]
    fn count_stops_early_when_cap_exceeded() {
        let tmp = tempfile::tempdir().unwrap();
        // 20 sibling dirs, cap=5 → return as soon as we step past cap.
        for i in 0..20 {
            fs::create_dir(tmp.path().join(format!("sub{i}"))).unwrap();
        }
        let count = count_subdirs_capped(tmp.path(), 5);
        assert!(
            count > 5,
            "expected count to exceed cap on overflow, got {count}"
        );
        // Early termination: we return as soon as count > cap (i.e. at
        // cap + 1), not after walking everything.
        assert_eq!(count, 6, "should return cap + 1, not walk further");
    }

    #[test]
    #[cfg(unix)]
    fn count_does_not_follow_symlinks_to_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("real");
        let link_parent = tmp.path().join("link_parent");
        fs::create_dir(&real).unwrap();
        fs::create_dir(real.join("inside")).unwrap();
        fs::create_dir(&link_parent).unwrap();
        std::os::unix::fs::symlink(&real, link_parent.join("link_to_real")).unwrap();
        // Walked: real, real/inside, link_parent. The symlink under
        // link_parent reports as a symlink (not a dir) via lstat-based
        // file_type(), so we don't recurse through it — same behavior
        // as notify's default recursive walker.
        assert_eq!(count_subdirs_capped(tmp.path(), 10), 3);
    }
}
