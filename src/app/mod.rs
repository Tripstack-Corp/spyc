//! Top-level application state and event loop.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyEventKind};

use crate::Tui;
use crate::config::{Config, StatusPosition};
use crate::fs::{Entry, EntryKind};
use crate::keymap::UserKeymap;
use crate::pane::{Pane, PaneTabs, TabEntry, TabInfo};
use crate::state::IgnoreMasks;
use crate::state::sessions::AgentKind;
use crate::ui::line_edit::LineEditor;
// The loop's message vocabulary lives in its own module; re-exported here so
// every `super::Message` path in the app layer keeps resolving.
use crate::ui::{
    help,
    list_view::Row,
    pager::{self, PagerView},
    theme::Theme,
};
use message::{Message, Wake};

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

mod about;
mod actions;
mod activity;
mod agent_status;
mod archive;
mod archive_ops;
mod archive_route;
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
mod image_gallery;
pub mod image_ops;
mod inventory_ops;
mod key_dispatch;
mod loop_steps;
mod lua;
mod lua_events;
mod matcher;
mod mcp;
mod mermaid_ops;
mod message;
#[cfg(test)]
mod mod_tests;
mod modal;
mod mouse;
mod mouse_mode;
mod navigate;
mod pager_handler;
mod pager_history;
mod pager_stream;
mod pane_scroll;
mod pane_tabs;
mod pane_wake;
mod paste_capture;
mod preview_ops;
mod proc;
mod prompt;
mod quick_select;
mod render;
mod route;
mod run;
/// The event loop's own scratch type. Defined in [`run`] with the loop it
/// serves; re-exported because the `pub(crate)` loop-step methods across this
/// module name it in their signatures.
pub use run::RunCtx;
mod scheduler;
mod session;
mod skill;
mod sources;
pub mod state;
mod status_hooks;
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
pub use matcher::Matcher;
use pager_history::PagerHistory;
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
    /// An operation is in flight. Reads like `Info`, but it names something that
    /// hasn't finished — so whatever the operation lands as replaces it, and a
    /// completion with nothing to say clears it rather than leaving a message
    /// that claims work is still going on.
    Progress,
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
/// A parked `wait_for_scope_clear` MCP call (P2): the caller's resolved owner
/// key + queried paths, the deadline it times out at, and the one-shot reply
/// sender the loop fires once its scope clears / the deadline passes. Held in
/// `Runtime` (an OS handle + a clock instant, not domain state).
struct PendingScopeWait {
    owner: String,
    paths: Vec<String>,
    deadline: std::time::Instant,
    reply: std::sync::mpsc::Sender<crate::mcp_cmd::McpResponse>,
}

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
    /// `Wake::Pane`. `None` before `run()` / in the test harness.
    pane_wake_tx: Option<std::sync::mpsc::Sender<Message>>,
    /// Monotonic `SinkId` allocator (never reused).
    next_sink_id: u64,
    /// P3-2 crash-sufficient autosave: fingerprint of the session-relevant state
    /// at the last successful save, so `settle_autosave` re-saves only on a
    /// genuine change (a clean session arms nothing → idle stays 0 dps).
    autosave_last_saved_fp: Option<u64>,
    /// The armed `Deadline::Autosave` fire instant (debounce-window end). `Some`
    /// only while a change awaits its save; cleared on save / when clean. An
    /// OS-ish clock value, correctly in `Runtime` (never the pure Model).
    autosave_due: Option<std::time::Instant>,
    /// A deferred vt100 scrollback snapshot: when to take it, and which tab
    /// asked for it. `Some` only between `open_pane_scroll_pager` arming the
    /// settle and `settle_pane_scroll` taking the shot, so idle stays 0 dps.
    ///
    /// The tab index is part of it because the settle window is real time the
    /// user can act in — switching tabs mid-window must abandon the snapshot
    /// rather than mount a different pane's history.
    pane_scroll_settle: Option<(std::time::Instant, usize)>,
    /// When the agent status-hook drift check may next run
    /// (`settle_status_hooks`). A throttle, not a deadline — nothing wakes the
    /// loop for it, so idle stays 0 dps. `None` = due now (startup).
    hook_recheck_at: Option<std::time::Instant>,
    /// P2 `wait_for_scope_clear`: parked MCP waiters, each holding the one-shot
    /// reply sender the loop fires (from `settle_scope_waiters`) once its scope
    /// clears or its deadline passes. In `Runtime` — it owns OS handles (the
    /// reply channel) + a clock instant, never the pure Model.
    scope_waiters: Vec<PendingScopeWait>,
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
    /// The in-flight Lua job (name + watchdog window start), for the runaway
    /// "keep waiting? [y/N]" prompt. `Some` only while a job runs; cleared when
    /// its outcome drains. The `Instant` it carries is an OS-ish clock value,
    /// correctly in `Runtime` (never the pure Model).
    lua_inflight: Option<lua::LuaInflight>,
    /// Bookkeeping for `spyc.on` event dispatch (the Tier-C seam). Tracks the
    /// last-fired baselines so `settle_lua_events` fires only on a genuine
    /// change, plus the re-entrancy guard that stops a Lua event handler whose
    /// own request re-triggers the same event from looping. Rebuilt from
    /// scratch on `:lua off` (the registries go too).
    lua_events: lua_events::LuaEventState,
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
    /// the loop with `Message::Wake(Wake::CodexSession)`; `apply_codex_session_pins`
    /// assigns session uuids to unpinned codex tabs.
    codex_pin_pending:
        std::sync::Arc<std::sync::Mutex<Option<Vec<crate::state::codex_transcript::RolloutMeta>>>>,
    codex_scan_in_flight: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Tier 5: landing slot for off-thread graveyard ops (archive / restore /
    /// purge-all). Each `Effect::Graveyard` worker pushes its
    /// `GraveyardOutcome` here and wakes the loop with `Message::Wake(Wake::Graveyard)`;
    /// `apply_graveyard_outcomes` drains it every pre-recv scan (a `Vec` so
    /// concurrent ops never clobber each other — no in-flight guard needed).
    graveyard_results: std::sync::Arc<std::sync::Mutex<Vec<graveyard_ops::GraveyardOutcome>>>,
    /// Landing slot for every off-thread image render — a mermaid diagram
    /// (`Effect::RenderMermaid`) and an image file (`Effect::OpenImage`) share
    /// it. The worker pushes an `ImageOutcome` here and wakes with
    /// `Message::Wake(Wake::Image)`; `apply_image_outcomes` drains it each pre-recv scan
    /// and installs or flashes the result. Same shape as `graveyard_results`.
    image_results: std::sync::Arc<std::sync::Mutex<Vec<image_ops::ImageOutcome>>>,
    /// Landing slot for off-thread archive ops (`Effect::Archive`): mount,
    /// materialize, clean. Same shape as `graveyard_results` — the worker pushes
    /// and wakes with `Message::Wake(Wake::Archive)`, `apply_archive_outcomes` drains.
    archive_results: std::sync::Arc<std::sync::Mutex<Vec<archive_ops::ArchiveOutcome>>>,
    /// Cancel flag handed to the in-flight streamed mount, so a long tarball can
    /// be abandoned with `:archive cancel`. Replaced per mount, so cancelling one
    /// can never cancel the next.
    archive_cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// An effect parked across the mount-size prompt, with the archive it's
    /// waiting for: a `ChangeDir` into a not-yet-mounted archive whose size asked
    /// a question first. A `PromptKind` is plain data and an `Effect` is not, so
    /// the in-flight half waits here — beside `archive_cancel`, the other piece of
    /// in-flight mount state.
    archive_mount_then: Option<(std::path::PathBuf, Box<Effect>)>,
    /// Landing slot for off-thread file operations.
    file_results: std::sync::Arc<std::sync::Mutex<Vec<file_ops::FileOutcome>>>,
    /// Landing slot for the off-thread middle-click clipboard read
    /// (`Effect::PasteFromClipboard`). The worker pushes the read's result here
    /// and wakes with `Message::Wake(Wake::ClipboardPaste)`; `apply_clipboard_pastes`
    /// drains it each pre-recv scan. A `Vec`, like `graveyard_results`: a middle
    /// click is cheap to repeat, so reads can overlap and must not clobber.
    clipboard_paste_results: std::sync::Arc<std::sync::Mutex<Vec<std::io::Result<String>>>>,
    /// Landing slot for off-thread clipboard *writes*. One entry per dispatched
    /// write: `None` succeeded, `Some(msg)` is what to flash.
    ///
    /// The write runs off the loop because the helpers legitimately outlive it —
    /// `xclip`/`xsel` keep running to serve the X11 selection, so the reap poll
    /// spends its whole budget on every yank. A `Vec` because yanks can overlap.
    clipboard_copy_results: std::sync::Arc<std::sync::Mutex<Vec<Option<String>>>>,
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
    /// and wakes with `Message::Wake(Wake::WorktreeJob)`; `apply_worktree_outcomes`
    /// drains it each pre-recv scan, re-applies refresh+context, then replies.
    worktree_results: std::sync::Arc<std::sync::Mutex<Vec<worktree_ops::WorktreeOutcome>>>,
    /// Landing slot for the off-thread vertical-split preview reload
    /// (`kick_preview_reload`). The worker stores its `PreviewOutcome` here
    /// (last-wins `Option` — one preview, so no `Vec` is needed) and wakes the
    /// loop with `Message::Wake(Wake::PreviewReload)`; `apply_preview_reloads` drains it
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

/// The `^a g` image-gallery popup.
///
/// Holds the *received* half (indexed off the agent's transcript) inline, but
/// reads the unsent half from `state.pane.pending_images` by `tab_id` rather
/// than copying it — those are megabytes each, and the popup is a glance.
pub struct ImageGallery {
    /// Which pane tab this gallery belongs to. Pins a late-arriving index so a
    /// tab switch mid-load can't show one agent's images under another's name.
    pub tab_id: String,
    pub received: Vec<crate::state::transcript_images::TranscriptImage>,
    /// Where `received` came from, and where an image's bytes are re-read from.
    /// `None` when there's no readable transcript and only unsent images show.
    pub transcript_path: Option<std::path::PathBuf>,
    pub cursor: usize,
    /// The transcript index is still being built off-thread.
    pub loading: bool,
}

/// A rendered image shown full-screen. Holds the ready-to-blit protocol plus
/// the encoded bytes, so the overlay verbs (`s` save, `y` copy, `b` base64)
/// work without re-rendering, and the [`ImageOrigin`](image_ops::ImageOrigin)
/// that tells them *what* they're acting on. See
/// `docs/archive/MERMAID_PAGER_PLAN.md`.
pub struct ImageView {
    pub protocol: ratatui_image::protocol::Protocol,
    /// The image as it arrived — a mermaid render's PNG, or an image file's own
    /// bytes verbatim (which may be JPEG/GIF/WebP, hence not `png`).
    pub encoded: Vec<u8>,
    /// Extension matching `encoded`'s real format, for `s`.
    pub ext: &'static str,
    /// Natural pixel size, for the footer.
    pub dims: (u32, u32),
    pub origin: image_ops::ImageOrigin,
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

/// A brief spice-heat border-pulse flash — the P3-1 visual bell. `start` times
/// the ~half-second decay; `frame` is the animation step advanced in
/// `settle_visual_bell` (a `&mut` settle — the pure draw can't read the clock)
/// so the perimeter pulse can sweep its pepper→ember→orange→spark gradient.
/// `None` on `view.visual_bell` means no flash is active.
#[derive(Clone, Copy, Debug)]
pub struct VisualBell {
    pub start: std::time::Instant,
    pub frame: u64,
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
    /// A preview an **agent** displaced: `create_worktree(open:true)` /
    /// `open_worktree` put a commander in the right column while the user was
    /// reading there. The right column hosts one thing, so the preview has to
    /// yield — but the user didn't ask for that, so it's set aside with the
    /// split shape it had and comes back when `b` closes. `None` when the user
    /// opened `b` themselves (`^s n` IS the ask) or after a restore.
    pub displaced_preview: Option<(state::VSplit, PagerView)>,
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
    /// `apply_image_outcomes`. Graphics terminals only. See
    /// `docs/archive/MERMAID_PAGER_PLAN.md`.
    pub image_view: Option<ImageView>,
    /// The `^a g` gallery popup, when open.
    pub image_gallery: Option<ImageGallery>,
    pub pager_history: PagerHistory,
    pub pager_pending_bracket: Option<char>,
    pub pager_was_open: bool,
    /// This turn's input went to a pager, so a status-bar flash raised while
    /// running its effects belongs in the pager instead (#166). Consumed at
    /// loop bottom by `relocate_flash_into_pager`.
    pub input_went_to_pager: bool,
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
    /// Active spice-heat border-pulse flash (P3-1 visual bell), or `None`.
    /// Started by `settle_agent_activity` on a Blocked/Done transition when
    /// `[notify].visual` is set (on by default); advanced + cleared by
    /// `settle_visual_bell`. The pure draw reads it to paint the perimeter ring —
    /// armed only on a transition, so idle panes never touch it.
    pub visual_bell: Option<VisualBell>,
    /// Process-lifetime constants for the activity HUD, snapshotted ONCE at
    /// construction so the pure `&self` render pass never reads the OS / env
    /// per frame (the render-purity contract): the pid (for `sample`/lldb),
    /// the terminal's `$TERM`, and its truecolor capability. None of these
    /// change after startup.
    pub hud_pid: u32,
    pub hud_term: String,
    pub hud_truecolor: bool,
    /// Resolved color depth for this session (CLI `--color` > `[layout]
    /// color_depth` > `$COLORTERM` auto-detect). When not `TrueColor`, the
    /// per-frame render downgrades every `Color::Rgb` to the nearest 256-color
    /// index so terminals that can't parse 24-bit SGR (old GNU screen) still get
    /// colors. `hud_truecolor` is derived from this — they never disagree.
    pub color_depth: crate::ui::color_depth::ColorDepth,
    /// Whether spyc is running over SSH (`$SSH_CONNECTION`/`$SSH_TTY`/`$SSH_CLIENT`),
    /// snapshotted once at construction (doesn't change mid-session). Drives the
    /// P3-1 `desktop_via = "auto"` routing — OSC-9 (client-side) over SSH, the OS
    /// notifier locally — so the pure `notification_for_transition` never reads env.
    pub is_ssh: bool,
    /// Tab-completion / cycle state.
    // Module-private (type `TabState` is module-private).
    tab_state: Option<TabState>,
    /// Scroll throttle: timestamp + direction of last processed arrow key.
    pub scroll_last: Option<(std::time::Instant, KeyCode)>,
    /// A forwarded mouse press is awaiting its release.
    ///
    /// Set when a button press is delivered to the pane's child, cleared when the
    /// matching release is. Keyed on "did the press go to the child" rather than on
    /// where the pointer is now, which is what makes the pairing exact: a press
    /// that moved the file-list focus must not produce a release for the child, and
    /// a press the child DID receive must get its release even if the pointer left
    /// the pane first. Children track button state — claude fires its click on the
    /// release — so both an unpaired and a missing release misbehave.
    pub mouse_press_forwarded: bool,
    /// A left press landed on a pager and a spyc-side text selection owns the
    /// drag until the button comes up.
    ///
    /// The exact counterpart of `mouse_press_forwarded`, and mutually exclusive
    /// with it: a gesture belongs to whoever received its press. Without this, a
    /// drag begun on a pager would fall through to the child-forwarding path and
    /// start typing mouse reports into the agent mid-selection.
    ///
    /// Which surface owns the in-flight drag.
    ///
    /// Holds the target so the drag keeps addressing the surface it STARTED in even
    /// if the pointer wanders elsewhere — a selection that retargeted mid-drag would
    /// extend against the wrong buffer's indices. For a pager that's the slot, not
    /// the mount, because a mount can't tell `view.pager` from `view.right_pager`.
    pub mouse_selection: Option<mouse::MouseDragTarget>,
    /// A file-list row selection, kept after the drag ends so the highlight
    /// persists and a follow-up yank can still find it (same contract as the
    /// pager's charwise selection).
    pub list_selection: Option<mouse::ListSelection>,
    /// A spyc-side charwise selection over the pane's visible grid, for a child
    /// that ignores mouse reports — `(anchor, focus)` in SCREEN coordinates,
    /// unordered so a backwards drag keeps its direction.
    ///
    /// Screen coordinates, so it is only meaningful for the frame it was made in:
    /// [`App::drain_pane_output`] clears it when the child paints, because the grid
    /// scrolls out from under a selection anchored this way. That is fine for the
    /// case it exists to serve — reading a static transcript overlay — and is the
    /// simple half of the tradeoff recorded in
    /// `docs/drafts/mouse_selection_plan.md`.
    pub pane_selection: Option<((u16, u16), (u16, u16))>,
    /// A charwise selection over one of the single-line chrome surfaces (the status
    /// bar, the pane divider/tab line) — the row and an unordered column pair.
    pub chrome_selection: Option<mouse::ChromeSelection>,
    /// What each chrome row actually rendered this frame, for a mouse copy.
    ///
    /// `RefCell` because the draw pass is `&self` and this is the renderer recording
    /// what it drew — the same role (and the same reason) as `PagerView`'s
    /// `last_content_area` / `last_body_w` `Cell`s. It holds the DRAWN line, not the
    /// semantic segments: a selection maps screen COLUMNS back to characters, and
    /// only the drawn line has the width-driven truncation the user is looking at.
    pub chrome_rows: std::cell::RefCell<Vec<mouse::ChromeRow>>,
    /// A sustained same-direction wheel-scroll gesture over an agent's own view
    /// (today: codex's `^T`), tracked so a long one can escalate to a page-sized
    /// step and so a short one can't open the view by accident — see
    /// `App::send_agent_view_scroll_keys`. `None` outside a gesture.
    pub pane_scroll_streak: Option<mouse::PaneScrollStreak>,
    /// Set right after spyc sends an agent's `transcript_toggle_key` or
    /// `transcript_close_key`, so a fast follow-up wheel tick — arriving before the
    /// child has redrawn — doesn't act on the STALE screen and send the same key
    /// again. Retired once a scrape confirms the send landed, which is
    /// direction-specific (`mouse::pending_view_confirmed`: marker present for an
    /// `Open`, absent for a `Close`), or after `TOGGLE_SETTLE` if it never does
    /// (self-healing rather than stuck refusing to retry).
    pub pane_view_sent: Option<(std::time::Instant, mouse::PendingViewIntent)>,
    /// Whether an agent-transcript scrollback (`^a v`) renders the agent's
    /// tool-use / tool-result lines. `t` toggles it; the transcript is
    /// re-rendered with the new value. Session-scoped (persists across
    /// re-opens), defaults to shown.
    pub transcript_show_tool_calls: bool,
    /// The scrollback source the user flipped to with `T`, and the pane tab id it
    /// was flipped on — ids are stable and never reused, so another tab's `^a v`
    /// can't inherit this choice. Outlives a re-open (`r` reloads the flipped
    /// source) and is dropped when the scrollback closes, which is the whole life
    /// of the view the flip was made in. `None` = whatever
    /// `decide_scroll_source` picks.
    pub scroll_source_override: Option<(String, pane_scroll::ScrollSourcePick)>,
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
    fn new(
        theme: Theme,
        context_path: PathBuf,
        context_dirty: bool,
        mcp_running: bool,
        color_depth: crate::ui::color_depth::ColorDepth,
    ) -> Self {
        use crate::ui::color_depth::ColorDepth;
        Self {
            pager: None,
            scroll_pager: None,
            pager_right: None,
            right_pager: None,
            displaced_preview: None,
            dim_inactive: true,
            preview_dirty: false,
            image_view: None,
            image_gallery: None,
            pager_history: PagerHistory::new(),
            pager_pending_bracket: None,
            pager_was_open: false,
            input_went_to_pager: false,
            pager_help_stash: None,
            scroll_pager_help_stash: None,
            scroll_source_override: None,
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
            visual_bell: None,
            hud_pid: std::process::id(),
            hud_term: std::env::var("TERM").unwrap_or_else(|_| "?".to_string()),
            // Derived from the resolved depth, not a second COLORTERM read, so the
            // HUD's "truecolor" line and the gradient path always match what we
            // actually emit (forcing `--color 256` also disables the RGB gradient).
            hud_truecolor: color_depth == ColorDepth::TrueColor,
            color_depth,
            is_ssh: std::env::var_os("SSH_CONNECTION").is_some()
                || std::env::var_os("SSH_TTY").is_some()
                || std::env::var_os("SSH_CLIENT").is_some(),
            // `setup_terminal` starts us in 1007 alternate-scroll, so the
            // terminal is NOT in real mouse reporting yet. `settle_mouse_mode`
            // turns it on at the first loop bottom if config asks for it.
            tab_state: None,
            scroll_last: None,
            mouse_press_forwarded: false,
            mouse_selection: None,
            list_selection: None,
            pane_selection: None,
            chrome_selection: None,
            chrome_rows: std::cell::RefCell::new(Vec::new()),
            pane_scroll_streak: None,
            pane_view_sent: None,
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
