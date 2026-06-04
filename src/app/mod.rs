//! Top-level application state and event loop.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind};
use glob::Pattern;
use ratatui::Frame;

use crate::Tui;
use crate::config::{Config, StatusPosition};
use crate::fs::{Entry, EntryKind};
use crate::keymap::UserKeymap;
use crate::pane::{Pane, PaneTabs, TabEntry, TabInfo};
use crate::state::IgnoreMasks;
use crate::state::sessions::AgentKind;
use crate::ui::line_edit::LineEditor;
use crate::ui::{
    help,
    list_view::Row,
    pager::{self, PagerView},
    theme::Theme,
};

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
mod bootstrap;
mod capture;
mod clipboard;
pub mod command_table;
mod commands;
mod config;
mod effect;
mod find_picker;
mod focus;
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
mod proc;
mod prompt;
mod quick_select;
mod render;
mod route;
mod run;
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
use proc::{ForegroundExec, spawn_input_reader};
pub use prompt::{Prompt, PromptKind};
use scheduler::{Deadline, Scheduler, arm_resume_deadlines};
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
