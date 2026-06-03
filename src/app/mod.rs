//! Top-level application state and event loop.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::spyc_debug;
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use glob::Pattern;
use ratatui::Frame;

use crate::config::{Config, StatusPosition};
use crate::fs::{Entry, EntryKind, Listing};
use crate::keymap::{Resolver, UserKeymap};
use crate::pane::{Pane, PaneTabs, TabEntry, TabInfo};
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
mod clipboard;
mod commands;
mod config;
mod effect;
mod find_picker;
mod git_state;
mod graveyard;
mod grep_session;
mod harpoon;
mod key_dispatch;
mod loop_steps;
mod mcp;
mod navigate;
mod pager_handler;
mod pager_history;
mod pane_scroll;
mod pane_tabs;
mod pane_wake;
mod prompt;
mod quick_select;
mod render;
mod route;
mod scheduler;
mod session;
mod sources;
pub mod state;
mod streaming;
mod tasks;
mod update;

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
use sources::{coalesce_recv, sync_listing_watch, take_reader_result};
use tasks::{BackgroundTasks, TASK_BUFFER_CAP, TaskStatus};

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
    git_result_rx: Option<std::sync::mpsc::Receiver<state::GitWorkerResult>>,
    /// Main thread → git worker requests. The Model records desired
    /// requests in `state.pending_git_requests` (it owns no channel); the
    /// run loop drains that outbox through this sender via
    /// `flush_git_requests`. `None` in the test harness.
    git_worker_tx: Option<std::sync::mpsc::Sender<state::GitWorkerRequest>>,
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

/// Outcome of dispatching one coalesced `effective` message (the result of
/// `App::dispatch_effective`). Lets the dispatch logic live in a method while
/// the loop keeps owning the actual control flow:
/// - `Continue` → re-enter the loop top immediately (the scroll-throttle
///   early-out), skipping this iteration's post-recv timers / render /
///   context-write — exactly what the old inline `continue;` did.
/// - `Proceed` → fall through to the post-recv timers + render.
/// - `Exit(_)` → return from `run()` with this result: reader death, where
///   `take_reader_result` distinguishes a clean stop (`Ok`) from a recorded
///   fatal read error (`Err`). Kept distinct from `?`-propagated handler
///   errors (which exit via the method's own `Result`).
enum DispatchFlow {
    Continue,
    Proceed,
    Exit(Result<()>),
}

/// Run()-scoped scratch for the event loop. Ephemeral: built by
/// `App::run_setup`, dropped when `run()` returns. Deliberately NOT
/// persistent App state (kept off `ViewState`/`Runtime`) so non-loop paths
/// — `test_app`, `route_snapshot`, render, MCP execute — never see it.
///
/// Owns the fs watcher + its watch topology (so the watcher's notify thread
/// is torn down with the run scope), the advisory `Scheduler`, the coalesce
/// buffers, the debounce timers, the last-keypress instant, and the per-
/// iteration `Draw` accumulator. It does NOT own the input reader handle,
/// `foreground_exec`, or the channel: those stay bare `run()` locals so
/// their Drop/borrow ordering is unchanged (the reader's `Drop` joins the
/// thread; `RunCtx` — and thus the watcher — must drop AFTER that join, so
/// `run()` declares `ctx` BEFORE `reader_handle`). The git/MCP forwarder
/// threads are detached (no handle kept) and self-terminate on channel drop.
///
/// `pub` only because the `pub(crate)` loop-step methods name it in their
/// signatures (`fn …(&mut self, ctx: &mut RunCtx)`); its fields stay
/// module-private (the `app` module itself is private, so this is
/// crate-internal in practice — matching `ViewState`/`App`).
pub struct RunCtx {
    fs_watcher: Option<notify::RecommendedWatcher>,
    /// Listing dir currently watched; on chdir we unwatch it and watch the new one.
    watched_listing: Option<PathBuf>,
    watched_git: Option<PathBuf>,
    scheduler: Scheduler,
    /// Coalesce buffers: the recv arm pushes here; the pre-recv drains process them.
    fs_pending: Vec<notify::Event>,
    git_pending: Vec<state::GitWorkerResult>,
    mcp_pending: Vec<crate::mcp_cmd::McpRequest>,
    last_context_write: std::time::Instant,
    last_refresh: std::time::Instant,
    last_git_poll: std::time::Instant,
    /// Trailing-debounce: last listing event; refresh fires after `REFRESH_QUIET` of quiet.
    last_event_at: Option<std::time::Instant>,
    /// First listing event since the last refresh (fixed, not bumped) — caps refresh deferral.
    first_event_after_refresh: Option<std::time::Instant>,
    /// Last keypress instant — suppresses the MCP context-write for 300ms after a keystroke.
    last_input_at: Option<std::time::Instant>,
    draw: Draw,
}

#[cfg(test)]
impl RunCtx {
    /// Build a `RunCtx` with no fs watcher (the loop-step unit tests never
    /// spawn one) and fresh empty scratch — so the step methods, which now
    /// take `&mut RunCtx`, can be driven from tests.
    fn for_test() -> Self {
        Self {
            fs_watcher: None,
            watched_listing: None,
            watched_git: None,
            scheduler: Scheduler::new(),
            fs_pending: Vec::new(),
            git_pending: Vec::new(),
            mcp_pending: Vec::new(),
            last_context_write: std::time::Instant::now(),
            last_refresh: std::time::Instant::now(),
            last_git_poll: std::time::Instant::now(),
            last_event_at: None,
            first_event_after_refresh: None,
            last_input_at: None,
            draw: Draw::default(),
        }
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
            git_worker_available: false,
            pending_git_requests: Vec::new(),
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
        // process; the OS reaps the thread on exit. Both channel ends
        // live on the Runtime (`runtime.git_worker_tx` sender,
        // `runtime.git_result_rx` receiver); the Model holds no channel —
        // it records desired requests in `state.pending_git_requests`,
        // which the run loop flushes to the worker via `flush_git_requests`.
        // See `state::GitWorkerRequest` / `state::GitWorkerResult`.
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
        app_state.git_worker_available = true;
        let mut app = Self {
            state: app_state,
            // Write context once on startup so claude sees initial state
            // (context_dirty: true).
            view: ViewState::new(theme, context_path, true, mcp_running),
            exit_summary: None,
            runtime: Runtime {
                git_result_rx: Some(git_res_rx),
                git_worker_tx: Some(git_req_tx),
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
        // The bootstrap cache-miss queued a request into the Model's
        // outbox (git_worker_available is now true); flush it onto the
        // worker channel so the first per-file markers land as early as
        // they did when the send was inline.
        app.flush_git_requests();
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

    /// Build the event loop's run()-scoped scratch (`RunCtx`): the fs
    /// watcher + initial watch topology, the advisory scheduler, the
    /// coalesce buffers, the debounce timers, the last-keypress instant, and
    /// the `Draw` accumulator. Also spawns the detached git/MCP forwarder
    /// threads and installs `pane_wake_tx` (each needs a `msg_tx` clone).
    /// Takes `&msg_tx` (does not consume it) so `run()` can hand the original
    /// to the input reader afterward. Does NOT spawn the reader or build
    /// `foreground_exec` — those stay bare `run()` locals for Drop ordering.
    fn run_setup(&mut self, msg_tx: &std::sync::mpsc::Sender<Message>) -> RunCtx {
        use notify::{RecursiveMode, Watcher};

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

        RunCtx {
            fs_watcher,
            watched_listing,
            watched_git,
            // MVU Phase 2: advisory deadline scheduler — computes the
            // recv_timeout wait from armed timers; the loop still fires each
            // timer via its own predicate against the threaded `now`.
            scheduler: Scheduler::new(),
            // MVU Phase 3a/3d: buffers the recv arm pushes into (zero state
            // mutation); the pre-recv drains process them against `now_pre`,
            // keeping the timing-sensitive debounce / generation-gate logic
            // exactly where it was — recv only changes *when* the loop wakes.
            fs_pending: Vec::new(),
            git_pending: Vec::new(),
            mcp_pending: Vec::new(),
            last_context_write: std::time::Instant::now(),
            last_refresh: std::time::Instant::now(),
            // 1Hz safety net: re-poll git state even if FSEvents missed
            // the `.git/index.lock` → `.git/index` rename.
            last_git_poll: std::time::Instant::now(),
            // Trailing debounce: fire refresh once events stop arriving for
            // `REFRESH_QUIET`. Bursty git ops emit several `.git/index`
            // rename events over hundreds of ms; firing on the *first* meant
            // sampling a transient state. Waiting for quiet avoids that.
            last_event_at: None,
            // First listing event since the last refresh — fixed (not bumped
            // per event) so the debounce can still fire after `max_refresh_defer`
            // of continuous activity instead of starving. Cleared on refresh.
            first_event_after_refresh: None,
            // Last keypress instant. MVU Phase 3b PR2 retired its poll-cadence
            // use; it survives for the context-write debounce suppressor,
            // which holds off the MCP context mtime bump for 300 ms after a
            // keystroke so claude's input echo isn't yanked mid-type.
            last_input_at: None,
            // Draw at least once on startup (dirty: true, reason: 3 = other).
            draw: Draw {
                dirty: true,
                reason: 3,
            },
        }
    }

    /// Dispatch one coalesced `effective` message. Extracted verbatim from
    /// the loop's `match effective { … }` (the Key/Paste/Resize input arm, the
    /// Tick/Timeout reader-death gate, the Disconnected fallback, and the
    /// `unreachable!` for buffered variants). Returns a [`DispatchFlow`] so the
    /// loop keeps the actual control flow: the scroll-throttle early-out maps
    /// to `Continue` (AFTER recording `last_input_at` + `context_dirty`, H3),
    /// reader death maps to `Exit(take_reader_result(..))` (H4), and handler
    /// `?`-errors propagate through this method's own `Result` (H5).
    fn dispatch_effective(
        &mut self,
        effective: Result<Message, std::sync::mpsc::RecvTimeoutError>,
        terminal: &mut Tui,
        foreground_exec: &ForegroundExec,
        reader_done: &std::sync::atomic::AtomicBool,
        read_err: &std::sync::Mutex<Option<std::io::Error>>,
        ctx: &mut RunCtx,
    ) -> Result<DispatchFlow> {
        match effective {
            Ok(Message::Input(ev)) => {
                ctx.draw.mark(2);
                match ev {
                    Event::Key(key)
                        if key.kind == KeyEventKind::Press || key.kind == KeyEventKind::Repeat =>
                    {
                        // Arm the typing-burst window — see the poll-ms
                        // computation above. Cheap; just stores an
                        // Instant.
                        ctx.last_input_at = Some(std::time::Instant::now());
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
                                // Early-out: skip the rest of this iteration
                                // (the old inline `continue;`).
                                return Ok(DispatchFlow::Continue);
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
                        self.run_effects(effects, terminal, foreground_exec)?;
                    }
                    Event::Paste(text) => self.handle_paste(text)?,
                    Event::Resize(cols, rows) => self.handle_resize(cols, rows),
                    _ => {}
                }
            }
            // No input this tick: re-poll the still-polled sources; no redraw
            // (matches the old `event::poll(...) == false`). The loop never
            // sends itself a Tick (the scheduler is advisory) so Tick is
            // identical to Timeout; the variant exists for later
            // subscriptions. This arm is also where a buffer-only coalesce
            // lands (it synthesizes Timeout) and where a dead reader is
            // detected every ~wait — see the reader_done gate.
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
                    return Ok(DispatchFlow::Exit(take_reader_result(read_err)));
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                // Defensive fallback: every `msg_tx` clone dropped (no
                // watcher, no forwarder, reader gone). Same contract —
                // propagate a recorded fatal error; else a clean stop.
                return Ok(DispatchFlow::Exit(take_reader_result(read_err)));
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
        Ok(DispatchFlow::Proceed)
    }

    /// Render one frame iff the accumulator is dirty (extracted verbatim from
    /// the loop's `if ctx.draw.dirty { … }` block). Composes the term-title
    /// effect, wraps the draw in a DEC 2026 synchronized update, honors the
    /// per-iteration `pending_clear`, times the build/whole-frame for the
    /// activity monitor, and counts the draw (skipping `activity_only` frames
    /// so the stats don't oscillate — H6). Resets `ctx.draw` for the next
    /// iteration. `?`-propagates `run_effects` / `terminal.draw` / `clear`.
    fn render_frame(
        &mut self,
        terminal: &mut Tui,
        foreground_exec: &ForegroundExec,
        pending_clear: bool,
        activity_only: bool,
        ctx: &mut RunCtx,
    ) -> Result<()> {
        // Only redraw when something actually changed.
        if !ctx.draw.dirty {
            return Ok(());
        }
        ctx.draw.dirty = false;
        // Title compose + dedup stay loop-side; only the
        // `term_title::set` IO runs through the sole executor.
        let title_fx: Vec<Effect> = self.term_title_effect().into_iter().collect();
        self.run_effects(title_fx, terminal, foreground_exec)?;
        // Wrap in DEC 2026 synchronized update so the terminal emulator
        // (iTerm2, etc.) buffers the entire frame and paints it atomically —
        // eliminates tearing and reduces terminal-side CPU.
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
                    let us = u64::try_from(render_start.elapsed().as_micros()).unwrap_or(u64::MAX);
                    self.view.activity_render_peak_us = self.view.activity_render_peak_us.max(us);
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
        if self.view.show_activity && !activity_only {
            self.view.activity_draws += 1;
            self.view.activity_bytes += u64::from(frame_area.width) * u64::from(frame_area.height);
            match ctx.draw.reason {
                1 => self.view.activity_reason_pane += 1,
                2 => self.view.activity_reason_event += 1,
                _ => self.view.activity_reason_other += 1,
            }
        }
        ctx.draw.reason = 0;
        Ok(())
    }

    /// Loop teardown (extracted verbatim from the tail of `run()`): remove the
    /// MCP context file, then SIGTERM-grace every pane child tree before `App`
    /// is dropped (the per-Pane `Drop` is a SIGKILL safety net; going through
    /// `shutdown` first gives well-behaved children — `vite`, `npm run dev`,
    /// anything that catches SIGTERM — 250ms to flush before we escalate, so
    /// quitting with a dev server in a pane doesn't orphan its process tree).
    fn run_teardown(&mut self) {
        crate::context::remove_context_file(&self.view.context_path);
        if let Some(tabs) = self.runtime.pane_tabs.as_mut() {
            for entry in tabs.tabs_mut() {
                entry.pane.shutdown(Duration::from_millis(250));
            }
        }
    }

    pub fn run(&mut self, terminal: &mut Tui) -> Result<()> {
        use std::sync::mpsc;

        // MVU Phase 3a: the single message channel. The parkable input
        // reader, the notify watcher closure, and the git forwarder all
        // feed `msg_tx`; the loop `recv_timeout`s on `msg_rx`. Created
        // first so the watcher/forwarder can clone a sender before the
        // reader takes ownership of the original.
        let (msg_tx, msg_rx) = mpsc::channel::<Message>();

        // All run()-scoped scratch (watcher + topology, scheduler, coalesce
        // buffers, debounce timers, last-keypress instant, the Draw
        // accumulator) lives in `ctx`. `run_setup` also spawns the git/MCP
        // forwarder threads and installs `pane_wake_tx` (all needing a
        // `msg_tx` clone). Declared BEFORE `reader_handle` so the watcher
        // (owned by `ctx`) drops AFTER the reader thread is joined (H8).
        let mut ctx = self.run_setup(&msg_tx);

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
            // clear causes a visible flash. Just force a ctx.draw instead.
            let (pager_redraw, pending_clear) = self.step_pager_repaint();
            if pager_redraw {
                ctx.draw.mark(3);
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
                ctx.draw.mark(3);
            }
            if self.drain_background_tasks() {
                ctx.draw.mark(3);
            }
            if self.refresh_task_viewer() {
                ctx.draw.mark(3);
            }

            // MVU Phase 6: drain any off-thread agent-status resolve that
            // landed (it woke us via `Message::AgentStatusReady`). Done HERE,
            // in the always-run scan — not in `active_agent_status` — because
            // the status bar (and thus `active_agent_status`) is skipped on the
            // overlay / top-pager render paths; draining only there would leave
            // the slot full and this nudge would busy-spin. Applying the result
            // updates the cache so the next render shows the short-id.
            if self.apply_landed_agent_status() {
                ctx.draw.mark(3);
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
                ctx.draw.mark(3);
            }

            // :grep session: drain match batches into the active
            // grep pager. Same shape as the F-finder drain but the
            // results land directly in the pager body instead of
            // being re-ranked.
            if self.drain_grep_session() {
                ctx.draw.mark(3);
            }

            // Pre-recv pane-output scan: drain every tab + overlay, flip the
            // background-tab divider glyph, mark exited tabs (see
            // `drain_pane_output` — clear_wake/drain_output CAS lives there).
            let (pane_draw, pane_reason) = self.drain_pane_output();
            if pane_draw {
                ctx.draw.mark(pane_reason);
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
            if self.handle_restore_resumes(now_pre, &mut ctx) {
                ctx.draw.mark(3);
            }

            // Drain buffered FsEvents + run the trailing-debounce listing
            // refresh (see `ingest_fs_and_maybe_refresh`).
            if self.ingest_fs_and_maybe_refresh(now_pre, &mut ctx) {
                ctx.draw.mark(3);
            }
            // 1 Hz safety-net git poll + GitPoll deadline arming (see
            // `poll_git_cadence`).
            if self.poll_git_cadence(now_pre, &mut ctx) {
                ctx.draw.mark(3);
            }

            // Execute writable MCP commands buffered into `ctx.mcp_pending` (see
            // `drain_mcp_pending` — kept at this early loop position for the
            // 5s read-after-write timeout contract).
            if self.drain_mcp_pending(&mut ctx) {
                ctx.draw.mark(3);
            }

            // Drain the git-worker results buffered into `ctx.git_pending` — the
            // SOLE apply/count/take site (see `drain_git_pending`).
            if self.drain_git_pending(&mut ctx) {
                ctx.draw.mark(2);
            }

            // Flush the Model's git-request outbox onto the worker channel
            // before the loop blocks on `recv`. The pure-domain refresh paths
            // (refresh_listing / refresh_git_state / chdir) only *record*
            // requests in `state.pending_git_requests` — the Model owns no
            // channel — so this is where they're actually dispatched. Placed
            // after every pre-recv refresh (and after the prior iteration's
            // message dispatch) so a cache-miss reaches the worker without
            // waiting for the next event.
            self.flush_git_requests();

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
            // a zero-timeout recv falls straight through to the ctx.draw this
            // iteration. Blocking here delayed already-drained pane output
            // (e.g. a keystroke echo) until the next message/deadline arrived,
            // a visible per-keystroke render lag. (Draw-before-you-block.)
            let wait = if ctx.draw.dirty {
                Some(Duration::ZERO)
            } else {
                ctx.scheduler.next().map(|when| {
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

            // MVU Phase 3a: having received, *coalesce* — buffer the burst into
            // the pending Vecs and surface only an Input (or Tick/Timeout/
            // Disconnected) to the dispatch match below. See `coalesce_recv`
            // (in sources.rs, next to `coalesce_pending`).
            let effective = coalesce_recv(recvd, &msg_rx, &mut ctx);
            // Dispatch the coalesced message (Input → Key/Paste/Resize, or a
            // Tick/Timeout/Disconnected reader-death check). The returned
            // DispatchFlow keeps control flow loop-side: Continue is the
            // scroll-throttle early-out, Exit is reader death.
            match self.dispatch_effective(
                effective,
                terminal,
                &foreground_exec,
                &reader_done,
                &read_err,
                &mut ctx,
            )? {
                DispatchFlow::Continue => continue,
                DispatchFlow::Exit(result) => return result,
                DispatchFlow::Proceed => {}
            }

            // MVU Phase 2: clock for POST-recv timers (activity rollover,
            // context-write), captured AFTER the recv sleep — matching
            // their old live `.elapsed()` read position (a stale top-of-loop
            // clock would defer them by up to the full wait).
            let now_post = std::time::Instant::now();

            // Activity monitor: roll over the 1-second window (snapshot +
            // reset + proc-stat refresh; see `roll_activity_window`). Returns
            // whether an overlay-only redraw is warranted this tick.
            let activity_only_draw = self.roll_activity_window(now_post, ctx.draw.dirty);
            if activity_only_draw {
                ctx.draw.set_dirty();
            }

            // Re-arm the post-recv advisory deadlines — ActivityRollover +
            // CaptureTick (see `arm_post_recv_deadlines`).
            self.arm_post_recv_deadlines(now_post, &mut ctx);

            // Render the frame iff dirty (see `render_frame`): title effect,
            // DEC 2026 synchronized-update wrap, optional clear, the timed
            // draw, and the activity stats.
            self.render_frame(
                terminal,
                &foreground_exec,
                pending_clear,
                activity_only_draw,
                &mut ctx,
            )?;

            // Only re-sync the filesystem watcher when the cwd actually changed.
            if ctx.watched_listing.as_deref() != Some(self.state.listing.dir.as_path()) {
                sync_listing_watch(
                    ctx.fs_watcher.as_mut(),
                    &mut ctx.watched_listing,
                    &mut ctx.watched_git,
                    &self.state.listing.dir,
                    self.state.current_gitdir.as_deref(),
                );
            }

            // Event-driven MCP context-file write — debounced + typing-burst
            // suppressed, with ContextWrite deadline arming (see
            // `maybe_write_context`).
            self.maybe_write_context(now_post, &mut ctx);
        }
        self.run_teardown();
        Ok(())
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
        crate::agent::resume::parse_gemini_list_sessions_for_uuid(text, uuid)
    }

    // MVU Phase 5: `yank_pane_to_clipboard` / `yank_scrollback_to_clipboard`
    // are gone — their live-pane read + guards + clipboard IO moved into
    // `run_effects`'s `Effect::ReadPaneText` executor. The `yp`/`ya` action
    // arms in `actions.rs` now emit `ReadPaneText { kind, then: Clipboard }`
    // directly, so the handler stays pure-Model (no Runtime read).

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
                git_worker_tx: None,
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
    use crossterm::event::KeyModifiers;

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
        const CEILING: usize = 4_000;
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
