//! Top-level application state and event loop.

use std::path::PathBuf;
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
/// task reader threads feed `SinkOutput` (3c); the MCP forwarder feeds `Mcp`;
/// the finder feeds `FindOutput`; and pager-stream workers (grep / git-view /
/// transcript) feed `PagerStreamOutput` (3d). The only
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
    /// MVU Phase 3d: the F-finder walker produced a candidate batch or
    /// completed. Payloadless wake — the candidates ride `FindPicker.walk_rx`,
    /// re-drained by `drain_walk` (a wake after the picker closed no-ops at
    /// the `if let Some(picker)` guard). Collapsed in the coalesce pre-step;
    /// never surfaced as `Input`.
    FindOutput,
    /// A pager-stream worker (the unified `pager_stream` abstraction — grep /
    /// git-view / transcript collapse onto it) produced a batch / its one-shot
    /// model. Payloadless wake — the payload rides the boxed stream's `rx`,
    /// re-drained by `drain_pager_stream` (id-gated against the live pager's
    /// `stream_id`, so a wake for a replaced/closed stream self-discards).
    /// Collapsed in the coalesce pre-step; never surfaced as `Input`.
    PagerStreamOutput,
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
    /// re-scan wakes (drop-safe in coalesce). The redraw + apply both happen
    /// in the pre-recv scan: `apply_landed_agent_status` drains the slot into
    /// the cache and `kick_agent_status_refresh` re-arms — NOT in the draw
    /// (`active_agent_status` is a pure `&self` cache read since #346). Driven
    /// by the scan, not by this message surviving coalesce.
    AgentStatusReady,
    /// Tier 5: an off-thread graveyard op (archive / restore / purge-all,
    /// `Effect::Graveyard`) finished and pushed its outcome onto
    /// `runtime.graveyard_results`. Payloadless wake — the outcome rides the
    /// slot, drained unconditionally by `apply_graveyard_outcomes` in the
    /// pre-recv scan. Collapses to a Timeout like the other re-scan wakes
    /// (drop-safe in coalesce); the redraw is driven by the drain, not by this
    /// message surviving. Same shape as `AgentStatusReady`.
    GraveyardDone,
    /// An off-thread mermaid render+open (`Effect::RenderMermaid`) finished and
    /// pushed its outcome onto `runtime.mermaid_results`. Payloadless wake —
    /// `apply_mermaid_outcomes` drains the slot in the pre-recv scan. Same shape
    /// as `GraveyardDone`.
    MermaidDone,
    /// An off-thread file op (`Effect::FileOp`) finished and pushed its outcome.
    FileOpDone,
    /// An off-thread inventory op (`Effect::Inventory`) finished.
    InventoryDone,
    /// An off-thread MCP worktree op (create/remove/clean) finished and pushed
    /// its outcome onto `runtime.worktree_results`. Payloadless wake —
    /// `apply_worktree_outcomes` drains it in the pre-recv scan, re-applies the
    /// listing/context update, then answers the MCP client. Same shape as
    /// `MermaidDone`.
    WorktreeJobDone,
    /// A Lua script finished on the worker thread (`runtime.lua`). Payloadless
    /// wake — `handle_lua_done` drains the worker's outcome buffer in the
    /// pre-recv scan and translates the requests into effects/actions. Same
    /// shape as `WorktreeJobDone`, except the outcomes ride the worker's own
    /// buffer (`LuaWorker::drain_outcomes`), not a `runtime.*_results` slot.
    LuaDone,
    /// An off-thread vertical-split preview reload (`kick_preview_reload`)
    /// finished and pushed its outcome onto `runtime.preview_results`.
    /// Payloadless wake — `apply_preview_reloads` drains the slot in the
    /// pre-recv scan. Same shape as `MermaidDone`.
    PreviewReloadDone,
    /// Option B (`codex_pin`): an off-thread `~/.codex/sessions` scan landed a
    /// rollout snapshot in `codex_pin_pending`. Payloadless wake — the snapshot
    /// rides the slot, drained by `apply_codex_session_pins` in the pre-recv
    /// scan. Collapses to a Timeout like the other re-scan wakes.
    CodexSessionReady,
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

/// Cadence of the post-Enter verify pass: each tick checks whether
/// `/resume <sid>` is still sitting unsubmitted in the pane tail and
/// re-sends `\r` if so. Claude eats a lone `\r` whenever its async
/// startup work (MCP connects, version check, org-message fetch)
/// remounts the input component — which can happen seconds after the
/// banner looks settled, so the fixed delays above can't close the
/// race on their own.
const RESTORE_RESUME_VERIFY_DELAY: Duration = Duration::from_secs(1);

/// How many retry `\r`s the verify pass may send before giving up
/// (≈5 s of cover past the first Enter). Retries are guarded by the
/// typed command still being visible, so a generous count is safe.
const RESTORE_RESUME_VERIFY_RETRIES: u8 = 5;

/// How many trailing pane lines the verify pass scans for the
/// unsubmitted `/resume <sid>`. The input box lives in the bottom few
/// rows; a margin covers the status line and any popup below it.
const RESTORE_RESUME_VERIFY_TAIL: usize = 15;

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
    /// The contiguous spyc-unit region a top overlay (`;cmd`/`$EDITOR`) or
    /// a `TopPane` pager paints over: everything above the divider when a
    /// pane is open, else the whole frame. NOT `status.y + Σheights` —
    /// with `status_position = "bottom"` the status row is the *last* row,
    /// so that construction anchors the overlay off-screen and panics.
    top_unit: ratatui::layout::Rect,
    /// The right column's content rect when a vertical split is open (the
    /// live-reloading preview), else `None`. Filled by `carve_vsplit`;
    /// `compute_layout`'s single-column branches leave it `None`.
    right: Option<ratatui::layout::Rect>,
    /// The 1-column vertical separator between the left and right columns
    /// when a vertical split is open, else `None`.
    vdivider: Option<ratatui::layout::Rect>,
}

/// Follow-up side effect a key handler asks the main loop to perform.
///
/// Anything that needs to own the tty (editor, pager, shell-out) goes
/// through this so `run()` can tear the TUI down and restore it cleanly.
#[derive(Debug)]
pub enum PostAction {
    Spawn {
        program: String,
        args: Vec<String>,
        /// Whether to pause and wait for a keypress after the child exits,
        /// so the user can read any output before the TUI is restored.
        pause_after: bool,
    },
}

mod actions;
mod activity;
mod agent_status;
mod bootstrap;
mod capture;
mod clipboard;
mod codex_pin;
pub mod command_table;
mod commands;
mod config;
mod effect;
mod file_ops;
mod find_picker;
mod focus;
mod git_state;
mod git_view_session;
mod graveyard;
mod graveyard_ops;
mod grep_session;
#[cfg(test)]
mod harness_tests;
mod harpoon;
mod inventory_ops;
mod key_dispatch;
mod loop_steps;
mod lua;
mod mcp;
mod mermaid_ops;
#[cfg(test)]
mod mod_tests;
mod modal;
mod navigate;
mod pager_handler;
mod pager_history;
mod pager_stream;
mod pane_scroll;
mod pane_tabs;
mod pane_wake;
mod preview_ops;
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
#[cfg(test)]
mod test_harness;
mod update;
mod util;
mod vsplit;
mod watch;
mod worktree_clean;
mod worktree_ops;

use capture::PendingCapture;
#[cfg(unix)]
pub use effect::SigOk;
pub use effect::{ClipMsg, Effect, PaneInput, PaneTarget, PaneTextKind, PaneTextSink};
use find_picker::FindPicker;
use pager_history::PagerHistory;
use pane_wake::SinkId;
use proc::{ForegroundExec, spawn_input_reader};
pub use prompt::{Prompt, PromptKind};
use scheduler::{Deadline, Scheduler, arm_resume_deadlines};
use tasks::{BackgroundTasks, TASK_BUFFER_CAP, TaskStatus};
#[cfg(unix)]
use util::kill_pg;
use util::{
    buffer_to_lines, eof_marker_line, format_elapsed_hms, format_uptime, path_basename_display,
    strip_ansi_escapes, user_host_string,
};

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
        scroll: usize,
        mount: crate::ui::pager::Mount,
        pane_scroll: bool,
    },
    /// On-disk file: reopen from the original path.
    SourceFile {
        path: PathBuf,
        scroll: usize,
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
/// (the App-side half of the previously-torn git channel).
struct Runtime {
    /// Git worker → main thread results, generation-gated, applied via
    /// `apply_git_worker_result`. The Phase-3a forwarder thread takes this
    /// once in `run()` and bridges it onto the unified `Message` channel.
    git_result_rx: Option<std::sync::mpsc::Receiver<state::GitWorkerResult>>,
    /// Main thread → git worker requests. The Model records desired
    /// requests in `state.git_cache.pending_git_requests` (it owns no channel); the
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
    /// The embedded Lua engine worker — lazy-spawned on first use
    /// (`ensure_lua_worker`), `None` until then / when disabled (`--no-lua`,
    /// `:lua off`) / in the test harness. Owns the interpreter thread; the
    /// non-`Send` `mlua::Lua` lives entirely inside it and never moves to the
    /// main thread.
    lua: Option<crate::lua::LuaWorker>,
    /// `init.lua`'s `spyc.map` / `spyc.command` / `spyc.on` registrations,
    /// keyed by trigger → worker-side `fn_id`. Rebuilt from scratch on every
    /// `init.lua` (re)load; empty until then.
    lua_registry: lua::LuaRegistry,
    /// Directories where we wrote an MCP client config we own (`.mcp.json` /
    /// `.codex/config.toml`) when launching an agent pane. Recorded by
    /// `ensure_agent_mcp_config`; `cleanup_written_mcp_configs` removes our
    /// entry from each on teardown so a dead socket isn't left referenced.
    mcp_config_dirs: Vec<PathBuf>,
    /// Bottom pane tabs (each owns a `PtyHost`).
    pane_tabs: Option<PaneTabs>,
    /// Top-area overlay subprocess (`V`/`D`/`;`) — a `PtyHost`. The LEFT
    /// column's (or single / no-split / full-frame `;cmd`) editor / `$PAGER`.
    top_overlay: Option<Pane>,
    /// The RIGHT column's editor / huge-file `$PAGER` overlay PTY in a vertical
    /// split — its own slot so a `V`/`D` in `b` coexists with one in `a` rather
    /// than evicting it (the dual-overlay twin of [`Self::top_overlay`]). Only
    /// ever holds an auto-dismiss editor/pager (never a `;cmd`), so it has no
    /// await-dismiss state. `None` outside a split or when `b` has no overlay.
    top_overlay_right: Option<Pane>,
    /// In-flight foreground `!` capture (owns a `PtyHost`).
    pending_capture: Option<PendingCapture>,
    /// Session-scoped scratch dir for `!`-capture output spills — one file per
    /// capture, each holding that capture's full uncapped output (the live
    /// `PendingCapture::buffer` front-trims its head, dropping the start of a
    /// large `git log`). Lazily created on the first capture; removed when
    /// `Runtime` drops at shutdown (and explicitly in `run_teardown`), so
    /// spilled buffers outlive any single pager close (they back the pager's
    /// forward/back history) but never the session. `None` until the first capture.
    capture_spill_dir: Option<tempfile::TempDir>,
    /// Backgrounded `!` tasks (each owns a `PtyHost`).
    background_tasks: BackgroundTasks,
    /// Active F-finder (holds the walker thread's receiver).
    find_picker: Option<FindPicker>,
    /// Active overlay pager stream (grep / git-view — drains into `view.pager`).
    /// Drained every tick by `drain_pager_stream` (id-gated against
    /// `view.pager.stream_id`).
    pager_stream: Option<Box<dyn pager_stream::PagerStream>>,
    /// Active scroll / lower-pane stream (agent transcript — drains into
    /// `view.scroll_pager`). Kept in its own slot so starting a grep or
    /// git-view (which writes to `pager_stream`) does not kill a
    /// concurrently-loading transcript. Stashed / restored alongside the
    /// scroll pager by `stash/restore_scrollback_pager_to_active_tab`.
    scroll_stream: Option<Box<dyn pager_stream::PagerStream>>,
    /// Monotonic pager-stream id (stale-stream guard), shared across all
    /// stream kinds.
    next_stream_id: u32,
    /// In-flight pager streams parked while their LowerPane scrollback pager is
    /// stashed on a backgrounded tab (keyed by the pager's `stream_id`). Kept
    /// here rather than on the `pane::TabEntry` because `PagerStream` is an
    /// `app` type and the dependency runs `app → pane` only. Re-installed into
    /// `scroll_stream` by `restore_active_tab_scrollback_pager`.
    stashed_pager_streams: std::collections::HashMap<u32, Box<dyn pager_stream::PagerStream>>,
    /// An in-flight git-view whose model is being built off-thread, before any
    /// pager is mounted. `drain_pending_git_view` mounts the overlay only when a
    /// non-empty model arrives (an empty result just flashes "no changes"), so
    /// `gd` over a clean path doesn't pop an overlay up and tear it back down.
    pending_git_view: Option<git_view_session::PendingGitView>,
    /// Off-render-thread agent-status resolve: the landing slot + in-flight
    /// flag (see `active_agent_status` / `apply_landed_agent_status`).
    agent_status_pending: std::sync::Arc<std::sync::Mutex<Option<AgentStatusCache>>>,
    agent_status_refreshing: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Option B (`codex_pin`): off-thread `~/.codex/sessions` scan landing slot
    /// (with an in-flight flag). A worker dumps a rollout snapshot here and wakes
    /// the loop with `Message::CodexSessionReady`; `apply_codex_session_pins`
    /// assigns session uuids to unpinned codex tabs.
    codex_pin_pending:
        std::sync::Arc<std::sync::Mutex<Option<Vec<crate::state::codex_transcript::RolloutMeta>>>>,
    codex_scan_in_flight: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Tier 5: landing slot for off-thread graveyard ops (archive / restore /
    /// purge-all). Each `Effect::Graveyard` worker pushes its
    /// `GraveyardOutcome` here and wakes the loop with `Message::GraveyardDone`;
    /// `apply_graveyard_outcomes` drains it every pre-recv scan (a `Vec` so
    /// concurrent ops never clobber each other — no in-flight guard needed).
    graveyard_results: std::sync::Arc<std::sync::Mutex<Vec<graveyard_ops::GraveyardOutcome>>>,
    /// Landing slot for off-thread mermaid render+open ops (`Effect::RenderMermaid`).
    /// The worker pushes a `MermaidOutcome` here and wakes with
    /// `Message::MermaidDone`; `apply_mermaid_outcomes` drains it each pre-recv
    /// scan and surfaces the result in the pager status line. Same shape as
    /// `graveyard_results`.
    mermaid_results: std::sync::Arc<std::sync::Mutex<Vec<mermaid_ops::MermaidOutcome>>>,
    /// Landing slot for off-thread file operations.
    file_results: std::sync::Arc<std::sync::Mutex<Vec<file_ops::FileOutcome>>>,
    /// The watcher-driven listing refresh (`FileOp::RefreshListing`) reads the
    /// dir off-thread; `inflight` keeps a single read in flight at a time, and
    /// `dirty` records a refresh requested while one was running so the result
    /// handler can re-spawn for the latest state. See `App::spawn_listing_refresh`.
    listing_refresh_inflight: bool,
    listing_refresh_dirty: bool,
    /// Landing slot for off-thread inventory operations.
    inventory_results: std::sync::Arc<std::sync::Mutex<Vec<inventory_ops::InventoryOutcome>>>,
    /// Landing slot for off-thread MCP worktree create/remove/clean ops. The
    /// worker pushes a `WorktreeOutcome` (result + the MCP reply channel) here
    /// and wakes with `Message::WorktreeJobDone`; `apply_worktree_outcomes`
    /// drains it each pre-recv scan, re-applies refresh+context, then replies.
    worktree_results: std::sync::Arc<std::sync::Mutex<Vec<worktree_ops::WorktreeOutcome>>>,
    /// Landing slot for the off-thread vertical-split preview reload
    /// (`kick_preview_reload`). The worker stores its `PreviewOutcome` here
    /// (last-wins `Option` — one preview, so no `Vec` is needed) and wakes the
    /// loop with `Message::PreviewReloadDone`; `apply_preview_reloads` drains it
    /// each pre-recv scan. `preview_reloading` is the in-flight guard that
    /// collapses a burst of saves to one trailing re-render (see `preview_ops`).
    preview_results: std::sync::Arc<std::sync::Mutex<Option<preview_ops::PreviewOutcome>>>,
    preview_reloading: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Terminal graphics-protocol capability (Kitty/iTerm2/Sixel/halfblocks +
    /// font cell size), detected ONCE at startup in `setup_terminal` before the
    /// input reader spawns (the `from_query_stdio` reads stdin — the #444 rule).
    /// `None` ⇒ detection failed / no graphics; the mermaid `View` mode then
    /// reports "no image protocol". Cloned into the render worker by `run_effects`.
    picker: Option<ratatui_image::picker::Picker>,
}

/// A rendered image shown full-screen (the pager `i` view). Holds the
/// ready-to-blit protocol plus the PNG bytes and — for a mermaid diagram — its
/// source, so the image-pager verbs (`s` save, `Y` yank source, later
/// `y`/`b`/`c`) work without re-rendering. Generalizes to image-file preview
/// (where `source` is `None`). See `docs/MERMAID_PAGER_PLAN.md`.
pub struct ImageView {
    pub protocol: ratatui_image::protocol::Protocol,
    pub png: Vec<u8>,
    pub source: Option<String>,
    /// Whether the current render uses the dark theme — tracked so `c` toggles it.
    pub dark: bool,
    /// Transient verb feedback (e.g. "saved: …"), shown in the overlay footer —
    /// the normal flash area is hidden behind the full-screen image.
    pub flash: Option<String>,
}

/// The which-key chord-hint popup's render data — a chord prefix's title and
/// its continuation rows (`keys → label`). Built in `settle_chord_hint` from
/// `Resolver::continuations` once the hint delay elapses while a chord is still
/// pending; read (never mutated) by `render_chord_hint`.
pub struct ChordHint {
    /// The armed prefix, e.g. `"^a"` or `"g"`.
    pub title: String,
    /// `(keys, label)` per continuation, e.g. `("z", "zoom pane (toggle fullscreen)")`.
    pub rows: Vec<(&'static str, &'static str)>,
}

/// MVU end-state: the **ViewState** cluster — render ephemerals + derived
/// caches + UI-layer state. Pure of OS handles (those live in [`Runtime`]) and
/// of domain state (that lives in `AppState`). Owned by `App` as a disjoint
/// field; handlers reach it via `self.view.…`.
#[allow(clippy::struct_excessive_bools)]
pub struct ViewState {
    /// The top-region / centered pager: `Mount::Overlay` (grep, git-view,
    /// help, command output, file viewers) or `Mount::TopPane` (`D`). Drives
    /// the file-list area or the centered overlay.
    pub pager: Option<PagerView>,
    /// The bottom-region pane-scrollback pager (`^a v`): always
    /// `Mount::LowerPane` + `pane_scroll`. Held in a *separate* slot so it
    /// coexists with a top-region [`Self::pager`] (read a `D` doc up top while
    /// scrolling claude's history below) — they occupy different screen regions
    /// and must not evict each other. The focused-region pager is selected by
    /// [`App::active_pager_mut`]; render draws both independently.
    pub scroll_pager: Option<PagerView>,
    /// The RIGHT column's top-region (`D`) pager in a vertical split: a
    /// `Mount::TopPane` `PagerView`, the dual-overlay twin of [`Self::pager`]
    /// for column `b`. Its own slot so a `D` in `b` coexists with a `D`/`V` in
    /// `a` instead of evicting it. Rendered into `layout.right` by
    /// `render_right_split`. `None` outside a split or when `b` has no `D` open.
    /// (The full-frame modals — grep / git-view / help / `;cmd` output — stay in
    /// the single [`Self::pager`] slot; only the column-scoped `D` mirrors here.)
    pub pager_right: Option<PagerView>,
    /// The right-column pager of a vertical split (the live-reloading
    /// preview). Its **own** slot, like [`Self::scroll_pager`], so it coexists
    /// with the top and bottom region pagers — render draws it into
    /// `layout.right`. `None` until the vsplit keys (PR4) open a split; it is
    /// re-read + re-rendered off-thread when its `source_path` changes (PR5).
    pub right_pager: Option<PagerView>,
    /// Whether to fade the inactive split column / list (the focus dim). On by
    /// default; toggled by `^a d` for users who prefer both columns bright.
    pub dim_inactive: bool,
    /// Set when a preview-file change arrived while an off-thread reload was
    /// already in flight (`kick_preview_reload`); the reload's drain re-kicks
    /// once so the FINAL save is the one rendered. Main-thread-only — set in the
    /// fs ingest, read+cleared in `apply_preview_reloads` — so a plain `bool`.
    pub preview_dirty: bool,
    /// Full-screen image overlay (the pager `i` key): a rendered diagram/image
    /// blitted over everything until dismissed (q/Esc), with its own verbs
    /// (`s`/`Y`/`o`/…). `None` when nothing is being viewed. Set by
    /// `apply_mermaid_outcomes`. Graphics terminals only. See
    /// `docs/MERMAID_PAGER_PLAN.md`.
    pub image_view: Option<ImageView>,
    pub pager_history: PagerHistory,
    pub pager_pending_bracket: Option<char>,
    pub pager_was_open: bool,
    /// Stash for the pager that was active when `?` opened the
    /// pager-help overlay. Restored verbatim on Esc/q dismissal so
    /// the user lands back in the same view. Separate from
    /// `pager_history` because the latter silently drops
    /// `no_history=true` views — going through history would lose them.
    pub pager_help_stash: Option<PagerView>,
    /// Stash for the real scrollback (`scroll_pager`) while its dedicated help
    /// (`H` in `^a v`) is shown in that same bottom slot. `H` toggles the help
    /// between scrollback- and pager-keys variants; `Esc`/`q` restores this.
    /// Kept apart from `pager_help_stash` (the top-overlay help) so the two
    /// regions' help flows never clobber each other.
    pub scroll_pager_help_stash: Option<PagerView>,
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
    /// The same row cache + grid key for the **right** column's second
    /// commander (`state.right`), settled independently. Unused (empty / `MAX`
    /// gen) while no second commander is open.
    pub right_cached_rows: Vec<Row>,
    pub right_cached_rows_gen: u64,
    pub right_cached_grid_key: (u64, usize, usize, u16, u16),
    /// Last terminal-window title emitted (OSC 2 dedup). `None` forces
    /// a re-emit on next draw.
    pub last_term_title: Option<String>,
    // --- D3: the remaining UI-layer ephemerals + activity counters ---
    /// Active harpoon menu overlay (interactive: reorder, delete, jump).
    // Module-private (type `HarpoonMenu` is module-private).
    harpoon_menu: Option<HarpoonMenu>,
    /// The which-key chord-hint popup, once the hint delay has elapsed while a
    /// chord is still armed. `None` when no popup is showing (set in
    /// `settle_chord_hint`, cleared the moment the chord resolves/cancels).
    pub chord_hint: Option<ChordHint>,
    /// When the chord-hint popup is due to appear (set on a pending chord in
    /// `handle_key`, consumed by `settle_chord_hint`). `None` when no chord is
    /// arming a popup. Distinct from `chord_hint` so the timer and the shown
    /// popup are tracked independently.
    pub chord_hint_due: Option<std::time::Instant>,
    /// Active Quick Select picker (`^a u`).
    pub quick_select: Option<crate::pane::quick_select::QuickSelect>,
    /// `dd` arming for the graveyard view (first `d` arms, second deletes).
    pub graveyard_pending_d: bool,
    /// `gg` arming for the graveyard view (jump to top).
    pub graveyard_pending_g: bool,
    pub overlay_awaiting_dismiss: bool,
    /// When the current top overlay's child exits, return to spyc **immediately**
    /// instead of holding the "[process exited — press any key]" frame. Set for
    /// interactive overlays — the `V` editor, the `D` huge-file `$PAGER`, the
    /// in-pager editor — where there's no command output to linger on (you `:q`
    /// and want straight back). Left `false` for `;cmd` / `:`-spawned commands,
    /// whose output the await-dismiss preserves (so `;ls` doesn't flash + vanish).
    pub overlay_auto_dismiss: bool,
    /// Which vsplit column the current `V`/`D` overlay/TopPane-pager lives in
    /// (`None` when no split, or no overlay). The overlay is pinned to the
    /// column it opened from: `top_unit` scopes to it and it stays there even
    /// when `^a l`/`^a h` moves keyboard focus to the other column. Set at open,
    /// cleared at teardown.
    pub overlay_column: Option<state::Side>,
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
    /// Last context snapshot written to disk — skip the write when the new
    /// snapshot compares equal (avoids serializing just to diff).
    pub last_context: Option<crate::context::SpycContext>,
    /// `.spyc-context.json` is stale and should be rewritten (debounced +
    /// typing-burst-guarded).
    pub context_dirty: bool,
    /// Whether the MCP socket server is running.
    pub mcp_running: bool,
    /// Whether this instance may take over the MCP socket from another spyc
    /// when it writes a client config. Captured once at startup (the
    /// `App::new` arg) and read at agent-launch time, when we actually write
    /// `.mcp.json` / `.codex/config.toml`.
    pub mcp_takeover_allowed: bool,
    /// When a focus-switch chord just completed: (when, completing key) —
    /// the next dispatch drops a Press/Repeat of that key within ~60 ms.
    pub focus_chord_completed: Option<(std::time::Instant, KeyCode)>,
    /// Activity monitor (`A`): the overlay visibility toggle. The counters
    /// themselves live in [`activity::ActivityMonitor`] (`self.view.activity`).
    pub show_activity: bool,
    /// Activity-monitor counters: live/snapshot double-buffer + peaks + proc
    /// stats. See [`activity::ActivityMonitor`].
    pub activity: activity::ActivityMonitor,
    /// Forward timestamp for the keystroke→echo latency peak, measured against
    /// the next active-pane output.
    pub pane_send_at: Option<std::time::Instant>,
    /// `App::run` process start (activity-monitor uptime).
    pub started_at: std::time::Instant,
    /// Agent-activity (P0) "spicy pulse" animation frame, advanced in
    /// `settle_agent_activity` (a `&mut` settle point — render is pure and
    /// can't read the clock) while ≥1 agent tab is Working. The pure draw maps
    /// it to a warm heat color for the per-tab dot.
    pub agent_anim_frame: u64,
    /// Process-lifetime constants for the activity HUD, snapshotted ONCE at
    /// construction so the pure `&self` render pass never reads the OS / env
    /// per frame (the render-purity contract): the pid (for `sample`/lldb),
    /// the terminal's `$TERM`, and its truecolor capability. None of these
    /// change after startup.
    pub hud_pid: u32,
    pub hud_term: String,
    pub hud_truecolor: bool,
    /// Tab-completion / cycle state.
    // Module-private (type `TabState` is module-private).
    tab_state: Option<TabState>,
    /// Scroll throttle: timestamp + direction of last processed arrow key.
    pub scroll_last: Option<(std::time::Instant, KeyCode)>,
    /// Whether an agent-transcript scrollback (`^a v`) renders the agent's
    /// tool-use / tool-result lines. `t` toggles it; the transcript is
    /// re-rendered with the new value. Session-scoped (persists across
    /// re-opens), defaults to shown.
    pub transcript_show_tool_calls: bool,
    /// Cached terminal dimensions (columns, rows). Read once at startup via
    /// `crossterm::terminal::size()` and refreshed on every `Event::Resize` in
    /// `handle_resize`. Handlers read this instead of calling `terminal::size()`
    /// inline, which avoids the repeated syscall and keeps them OS-call-free.
    pub term_size: (u16, u16),
}

impl ViewState {
    /// Build the initial ViewState. `theme`/`context_path` are the only
    /// caller-specific values; `context_dirty` (write-context-on-startup) and
    /// `mcp_running` differ between the live app (`true` / actual) and the test
    /// harness (`false` / `false`). Everything else starts empty.
    fn new(theme: Theme, context_path: PathBuf, context_dirty: bool, mcp_running: bool) -> Self {
        Self {
            pager: None,
            scroll_pager: None,
            pager_right: None,
            right_pager: None,
            dim_inactive: true,
            preview_dirty: false,
            image_view: None,
            pager_history: PagerHistory::new(),
            pager_pending_bracket: None,
            pager_was_open: false,
            pager_help_stash: None,
            scroll_pager_help_stash: None,
            pager_positions: crate::state::pager_positions::PagerPositions::load(),
            theme,
            needs_full_repaint: false,
            cached_rows: Vec::new(),
            cached_rows_gen: u64::MAX, // force first build
            cached_grid_key: (u64::MAX, 0, 0, 0, 0),
            right_cached_rows: Vec::new(),
            right_cached_rows_gen: u64::MAX, // force first build
            right_cached_grid_key: (u64::MAX, 0, 0, 0, 0),
            last_term_title: None,
            harpoon_menu: None,
            chord_hint: None,
            chord_hint_due: None,
            quick_select: None,
            graveyard_pending_d: false,
            graveyard_pending_g: false,
            overlay_awaiting_dismiss: false,
            overlay_auto_dismiss: false,
            overlay_column: None,
            agent_status_cache: None,
            pending_history_pick: None,
            pending_jump_history: None,
            history_pending_g: false,
            scroll_pending_g: false,
            pending_pager_return: None,
            context_path,
            last_context: None,
            context_dirty,
            mcp_running,
            // Set from the `App::new` arg in bootstrap; the test harness never
            // writes client configs, so the default is fine there.
            mcp_takeover_allowed: false,
            focus_chord_completed: None,
            show_activity: false,
            activity: activity::ActivityMonitor::new(std::time::Instant::now()),
            pane_send_at: None,
            started_at: std::time::Instant::now(),
            agent_anim_frame: 0,
            hud_pid: std::process::id(),
            hud_term: std::env::var("TERM").unwrap_or_else(|_| "?".to_string()),
            hud_truecolor: std::env::var("COLORTERM")
                .is_ok_and(|c| c.contains("truecolor") || c.contains("24bit")),
            tab_state: None,
            scroll_last: None,
            transcript_show_tool_calls: true,
            term_size: crossterm::terminal::size().unwrap_or((80, 24)),
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
    /// A git-deleted file that no longer exists on disk, synthesized into the
    /// `Dir` listing so the deletion is visible (rendered struck-through).
    /// `path` is the would-be location; opening it is guarded, and a future
    /// restore (`gr`) brings it back. `false` for every real on-disk row.
    pub deleted: bool,
}

impl RowData {
    /// The key under which this row's git status lives in `git.files`. That
    /// map keys files by bare basename and directories by `basename/` (see
    /// `git::status::map_to_listing`) — which equals `display` for every kind
    /// EXCEPT executables: `Entry::display_name` decorates those with a
    /// trailing `*` (ls -F style) that the git map never carries, so looking up
    /// by raw `display` silently fails to find any executable's status. Strip
    /// that one suffix so executable files surface their markers like any other
    /// file. (A file genuinely named `foo*` decorates to `foo**`, so stripping
    /// one `*` still yields its real basename key.)
    pub fn git_key(&self) -> &str {
        match self.kind {
            EntryKind::Executable => self.display.strip_suffix('*').unwrap_or(&self.display),
            _ => &self.display,
        }
    }
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
    /// Command sender to the off-thread watch worker (`watch::spawn_watch_worker`).
    /// `None` if the watcher couldn't be created (degrades to poll-only).
    watch_tx: Option<std::sync::mpsc::Sender<watch::WatchCommand>>,
    /// Last listing dir we sent a watch command for; on chdir we send a new
    /// `SyncListing` so the worker re-points its watches. Purely a send-dedup
    /// key — the worker owns the actual watch topology.
    watched_listing: Option<PathBuf>,
    /// Last second-commander (column `b`) listing dir we sent a watch command
    /// for. Re-sent when it changes (open / chdir / close `b`) so the worker
    /// watches `b`'s tree + gitdir too — `b`'s git markers refresh on
    /// fs-events, not just the ≤1 s poll.
    watched_listing_right: Option<PathBuf>,
    /// Last vertical-split preview file we sent a watch command for. The watch
    /// topology is re-sent when this OR `watched_listing` changes, so opening /
    /// swapping / closing the preview re-points the preview-parent watch.
    watched_preview: Option<PathBuf>,
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
            watch_tx: None,
            watched_listing: None,
            watched_listing_right: None,
            watched_preview: None,
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
        let (term_w, _) = self.view.term_size;
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
        match self {
            Self::Substring(q) => ascii_or_lower_contains(name, q),
            Self::Glob(p) => {
                // Glob matching needs an owned &str; skip the lowercasing
                // allocation for the common case of an already-lowercase ASCII
                // name. Non-ASCII (or any uppercase) names fall back to
                // `to_lowercase` to preserve Unicode case-folding semantics.
                if name.is_ascii() && !name.bytes().any(|b| b.is_ascii_uppercase()) {
                    p.matches(name)
                } else {
                    p.matches(&name.to_lowercase())
                }
            }
            Self::Never => false,
        }
    }
}

/// Case-insensitive substring test that avoids allocating a lowercased copy of
/// `name` on the filter/search hot path (called once per listing row per
/// keystroke). `needle` is already lowercased by `Matcher::build`. The ASCII
/// fast path is allocation-free; non-ASCII names fall back to `to_lowercase`
/// so Unicode case folding stays identical to the old behavior.
fn ascii_or_lower_contains(name: &str, needle: &str) -> bool {
    if name.is_ascii() && needle.is_ascii() {
        let (h, n) = (name.as_bytes(), needle.as_bytes());
        if n.is_empty() {
            return true;
        }
        if n.len() > h.len() {
            return false;
        }
        h.windows(n.len())
            .any(|w| w.iter().zip(n).all(|(&a, &b)| a.to_ascii_lowercase() == b))
    } else {
        name.to_lowercase().contains(needle)
    }
}

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
        deleted: false,
    }
}
