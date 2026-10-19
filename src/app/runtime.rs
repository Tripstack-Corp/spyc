//! The **Runtime** half of the MVU split: every OS handle the loop owns —
//! channels, worker endpoints, pty hosts, threads.
//!
//! Extracted from `app/mod.rs`, which sat at 96.5% of its anti-monolith ceiling
//! with over half the file being field docs on this struct and `ViewState`. The
//! docs travelled with the fields; that proximity is the point of them.
//!
//! Fields are `pub(super)`, which from here is exactly the reachability they
//! had as private items of `app`: the whole `app` subtree, which is what the
//! handler, effect and loop-step modules read them through.

use super::{
    AgentStatusCache, BackgroundTasks, Effect, FindPicker, Message, PendingCapture, archive_ops,
    file_ops, git_view_session, graveyard_ops, image_ops, inventory_ops, lua, lua_events,
    pager_stream, preview_ops, worktree_ops,
};
use crate::pane::{Pane, PaneTabs};

/// The per-project half of `Runtime` (#99, `docs/drafts/PROJECTS_PLAN.md`
/// §1c): the bottom pane tabs, the two overlay-subprocess slots, a git view
/// still being built and the pager streams, which belong to one project's
/// columns and pagers. The active project's is reached through
/// [`Runtime::project`] / [`Runtime::project_mut`].
#[derive(Default)]
pub(super) struct ProjectRuntime {
    /// Bottom pane tabs (each owns a `PtyHost`).
    pub(super) pane_tabs: Option<PaneTabs>,
    /// Top-area overlay subprocess (`V`/`D`/`;`) — a `PtyHost`. The LEFT
    /// column's (or single / no-split / full-frame `;cmd`) editor / `$PAGER`.
    pub(super) top_overlay: Option<Pane>,
    /// The RIGHT column's editor / huge-file `$PAGER` overlay PTY in a vertical
    /// split — its own slot so a `V`/`D` in `b` coexists with one in `a` rather
    /// than evicting it (the dual-overlay twin of [`Self::top_overlay`]). Only
    /// ever holds an auto-dismiss editor/pager (never a `;cmd`), so it has no
    /// await-dismiss state. `None` outside a split or when `b` has no overlay.
    pub(super) top_overlay_right: Option<Pane>,
    /// A git view whose model is being built off-thread, before any pager is
    /// mounted. `drain_pending_git_view` mounts the overlay only when a
    /// non-empty model arrives (an empty result just flashes "no changes"), so
    /// `gd` over a clean path doesn't pop an overlay up and tear it back down.
    /// The drain reads the active project's only, so a view opened here mounts
    /// in this project's pager when it is next in view.
    pub(super) pending_git_view: Option<git_view_session::PendingGitView>,
    /// The overlay pager stream (grep / git view), drained into this project's
    /// `pager` by `drain_pager_stream`, id-gated against its `stream_id`. The
    /// drain reads the active project's only, so a stream left running here
    /// waits in its channel until this project is next in view.
    pub(super) pager_stream: Option<Box<dyn pager_stream::PagerStream>>,
    /// The lower-pane stream (agent transcript), drained into this project's
    /// `scroll_pager`. Kept in its own slot so starting a grep or git view
    /// does not kill a transcript still loading. Stashed / restored alongside
    /// the scroll pager by `stash/restore_scrollback_pager_to_active_tab`.
    pub(super) scroll_stream: Option<Box<dyn pager_stream::PagerStream>>,
    /// In-flight streams parked while their scrollback pager is stashed on a
    /// tab of this project that is not the active one, keyed by the pager's
    /// `stream_id`. Kept here rather than on the `pane::TabEntry` because
    /// `PagerStream` is an `app` type and the dependency runs `app → pane`
    /// only. Re-installed into `scroll_stream` by
    /// `restore_active_tab_scrollback_pager`.
    pub(super) stashed_pager_streams:
        std::collections::HashMap<u32, Box<dyn pager_stream::PagerStream>>,
}

/// A parked `wait_for_scope_clear` MCP call (P2): the caller's resolved owner
/// key + queried paths, the deadline it times out at, and the one-shot reply
/// sender the loop fires once its scope clears / the deadline passes. Held in
/// `Runtime` (an OS handle + a clock instant, not domain state).
pub(super) struct PendingScopeWait {
    pub(super) owner: String,
    pub(super) paths: Vec<String>,
    pub(super) deadline: std::time::Instant,
    pub(super) reply: std::sync::mpsc::Sender<crate::mcp_cmd::McpResponse>,
}

/// MVU Phase 5: the **Runtime** cluster — IO handles (channels, worker
/// endpoints, pty hosts, threads) held disjointly from the domain Model
/// (`App.state`) and the render/derived `ViewState`. Fields migrate in
/// over Phase-5 PRs; PR 1 seeds it with the git worker-result receiver
/// (the App-side half of the previously-torn git channel).
pub(super) struct Runtime {
    /// Git worker → main thread results, generation-gated, applied via
    /// `apply_git_worker_result`. The Phase-3a forwarder thread takes this
    /// once in `run()` and bridges it onto the unified `Message` channel.
    pub(super) git_result_rx: Option<std::sync::mpsc::Receiver<super::state::GitWorkerResult>>,
    /// Main thread → git worker requests. The Model records desired
    /// requests in `state.git_cache.pending_git_requests` (it owns no channel); the
    /// run loop drains that outbox through this sender via
    /// `flush_git_requests`. `None` in the test harness.
    pub(super) git_worker_tx: Option<std::sync::mpsc::Sender<super::state::GitWorkerRequest>>,
    /// Commands from the MCP server; `run()` `.take()`s it into the forwarder
    /// thread which re-sends each as `Message::Mcp`.
    pub(super) mcp_cmd_rx: Option<std::sync::mpsc::Receiver<crate::mcp_cmd::McpRequest>>,
    /// Clone of the unified-channel sender; pane wake closures clone it to push
    /// `Wake::Pane`. `None` before `run()` / in the test harness.
    pub(super) pane_wake_tx: Option<std::sync::mpsc::Sender<Message>>,
    /// Monotonic `SinkId` allocator (never reused).
    pub(super) next_sink_id: u64,
    /// P3-2 crash-sufficient autosave: fingerprint of the session-relevant state
    /// at the last successful save, so `settle_autosave` re-saves only on a
    /// genuine change (a clean session arms nothing → idle stays 0 dps).
    pub(super) autosave_last_saved_fp: Option<u64>,
    /// The armed `Deadline::Autosave` fire instant (debounce-window end). `Some`
    /// only while a change awaits its save; cleared on save / when clean. An
    /// OS-ish clock value, correctly in `Runtime` (never the pure Model).
    pub(super) autosave_due: Option<std::time::Instant>,
    /// A deferred terminal scrollback snapshot: when to take it, and which tab
    /// asked for it. `Some` only between `open_pane_scroll_pager` arming the
    /// settle and `settle_pane_scroll` taking the shot, so idle stays 0 dps.
    ///
    /// The tab index is part of it because the settle window is real time the
    /// user can act in — switching tabs mid-window must abandon the snapshot
    /// rather than mount a different pane's history.
    pub(super) pane_scroll_settle: Option<(std::time::Instant, usize)>,
    /// P2 `wait_for_scope_clear`: parked MCP waiters, each holding the one-shot
    /// reply sender the loop fires (from `settle_scope_waiters`) once its scope
    /// clears or its deadline passes. In `Runtime` — it owns OS handles (the
    /// reply channel) + a clock instant, never the pure Model.
    pub(super) scope_waiters: Vec<PendingScopeWait>,
    /// Every open project's runtime, keyed by id — an id-addressed side
    /// table background threads also touch (unlike `AppState::projects`,
    /// which is ordered for the switcher). The active one is reached through
    /// [`Self::project`] / [`Self::project_mut`]; a tab's owner through
    /// [`Self::project_of_pane`] / [`Self::tab_of_pane`].
    pub(super) projects: std::collections::HashMap<super::state::ProjectId, ProjectRuntime>,
    /// Which of `projects` the active-project accessors resolve to. Tracks
    /// `AppState::active`.
    pub(super) active: super::state::ProjectId,
    /// The embedded Lua engine worker — lazy-spawned on first use
    /// (`ensure_lua_worker`), `None` until then / when disabled (`--no-lua`,
    /// `:lua off`) / in the test harness. Owns the interpreter thread; the
    /// non-`Send` `mlua::Lua` lives entirely inside it and never moves to the
    /// main thread.
    pub(super) lua: Option<crate::lua::LuaWorker>,
    /// `init.lua`'s `spyc.map` / `spyc.command` / `spyc.on` registrations,
    /// keyed by trigger → worker-side `fn_id`. Rebuilt from scratch on every
    /// `init.lua` (re)load; empty until then.
    pub(super) lua_registry: lua::LuaRegistry,
    /// The in-flight Lua job (name + watchdog window start), for the runaway
    /// "keep waiting? [y/N]" prompt. `Some` only while a job runs; cleared when
    /// its outcome drains. The `Instant` it carries is an OS-ish clock value,
    /// correctly in `Runtime` (never the pure Model).
    pub(super) lua_inflight: Option<lua::LuaInflight>,
    /// The project the jobs now in the Lua worker were started in, for
    /// `handle_lua_done` to apply a finished script's requests only there.
    /// Set by a submit with nothing in flight (`submit_lua_job`): a key or an
    /// event is refused while a script runs, and an `init.lua` reload queued
    /// behind one must not claim the running script's requests. Unlike
    /// `lua_inflight`, it outlives the first outcome of an event's run of
    /// handlers.
    pub(super) lua_origin: super::state::ProjectId,
    /// Bookkeeping for `spyc.on` event dispatch (the Tier-C seam). Tracks the
    /// last-fired baselines so `settle_lua_events` fires only on a genuine
    /// change, plus the re-entrancy guard that stops a Lua event handler whose
    /// own request re-triggers the same event from looping. Rebuilt from
    /// scratch on `:lua off` (the registries go too).
    pub(super) lua_events: lua_events::LuaEventState,
    /// The one visible strip subscription; dropping it cancels its workers.
    pub(super) agent_hud: Option<super::agent_hud::collector::Collector>,
    /// In-flight foreground `!` capture (owns a `PtyHost`).
    pub(super) pending_capture: Option<PendingCapture>,
    /// Session-scoped scratch dir for `!`-capture output spills — one file per
    /// capture, each holding that capture's full uncapped output (the live
    /// `PendingCapture::buffer` front-trims its head, dropping the start of a
    /// large `git log`). Lazily created on the first capture; removed when
    /// `Runtime` drops at shutdown (and explicitly in `run_teardown`), so
    /// spilled buffers outlive any single pager close (they back the pager's
    /// forward/back history) but never the session. `None` until the first capture.
    pub(super) capture_spill_dir: Option<tempfile::TempDir>,
    /// Backgrounded `!` tasks (each owns a `PtyHost`).
    pub(super) background_tasks: BackgroundTasks,
    /// Active F-finder (holds the walker thread's receiver).
    pub(super) find_picker: Option<FindPicker>,
    /// Monotonic pager-stream id (stale-stream guard), shared across all
    /// stream kinds and every project.
    pub(super) next_stream_id: u32,
    /// Off-render-thread agent-status resolve: the landing slot + in-flight
    /// flag (see `active_agent_status` / `apply_landed_agent_status`).
    pub(super) agent_status_pending: std::sync::Arc<std::sync::Mutex<Option<AgentStatusCache>>>,
    pub(super) agent_status_refreshing: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Option B (`codex_pin`): off-thread home-scoped scan landing slot
    /// (with an in-flight flag). A worker dumps a rollout snapshot here and wakes
    /// the loop with `Message::Wake(Wake::CodexSession)`; `apply_codex_session_pins`
    /// assigns session uuids to unpinned codex tabs.
    pub(super) codex_pin_pending:
        std::sync::Arc<std::sync::Mutex<Option<Vec<super::codex_pin::HomeSnapshot>>>>,
    pub(super) codex_scan_in_flight: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Tier 5: landing slot for off-thread graveyard ops (archive / restore /
    /// purge-all). Each `Effect::Graveyard` worker pushes its
    /// `GraveyardOutcome` here and wakes the loop with `Message::Wake(Wake::Graveyard)`;
    /// `apply_graveyard_outcomes` drains it every pre-recv scan (a `Vec` so
    /// concurrent ops never clobber each other — no in-flight guard needed).
    pub(super) graveyard_results:
        std::sync::Arc<std::sync::Mutex<Vec<graveyard_ops::GraveyardOutcome>>>,
    /// Where an agent-wiring worker (`App::start_wiring`) lands its outcome
    /// before waking the loop with `Wake::Wiring`; `apply_wiring_outcomes`
    /// drains it every pre-recv scan.
    pub(super) wiring_results: super::wiring::WiringResults,
    /// Landing slot for every off-thread image render — a mermaid diagram
    /// (`Effect::RenderMermaid`) and an image file (`Effect::OpenImage`) share
    /// it. The worker pushes an `ImageOutcome` here and wakes with
    /// `Message::Wake(Wake::Image)`; `apply_image_outcomes` drains it each pre-recv scan
    /// and installs or flashes the result. Same shape as `graveyard_results`.
    /// Each outcome is tagged with the project that started it
    /// (`App::spawn_image_work`).
    pub(super) image_results:
        std::sync::Arc<std::sync::Mutex<Vec<(super::state::ProjectId, image_ops::ImageOutcome)>>>,
    /// Landing slot for off-thread archive ops (`Effect::Archive`): mount,
    /// materialize, clean. Same shape as `graveyard_results` — the worker pushes
    /// and wakes with `Message::Wake(Wake::Archive)`, `apply_archive_outcomes` drains.
    /// Each outcome is tagged with the project that started it
    /// (`App::spawn_archive_op`).
    pub(super) archive_results: std::sync::Arc<
        std::sync::Mutex<Vec<(super::state::ProjectId, archive_ops::ArchiveOutcome)>>,
    >,
    /// Cancel flag handed to the in-flight streamed mount, so a long tarball can
    /// be abandoned with `:archive cancel`. Replaced per mount the user starts,
    /// so cancelling one can never cancel the next; a mount started in the
    /// background keeps its own (`App::mount_cancel`).
    pub(super) archive_cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// An effect parked across the mount-size prompt, with the archive it's
    /// waiting for: a `ChangeDir` into a not-yet-mounted archive whose size asked
    /// a question first. A `PromptKind` is plain data and an `Effect` is not, so
    /// the in-flight half waits here — beside `archive_cancel`, the other piece of
    /// in-flight mount state.
    pub(super) archive_mount_then: Option<(std::path::PathBuf, Box<Effect>)>,
    /// Archive reads in flight, by the path each reads, with the requests that
    /// joined each (`App::join_read`): one read of an archive at a time.
    pub(super) archive_reads:
        std::collections::HashMap<std::path::PathBuf, Vec<super::archive::Joined>>,
    /// Landing slot for off-thread file operations, each tagged with the
    /// project that started it (`App::spawn_file_op`).
    pub(super) file_results:
        std::sync::Arc<std::sync::Mutex<Vec<(super::state::ProjectId, file_ops::FileOutcome)>>>,
    /// Landing slot for the off-thread middle-click clipboard read
    /// (`Effect::PasteFromClipboard`). The worker pushes the read's result here
    /// and wakes with `Message::Wake(Wake::ClipboardPaste)`; `apply_clipboard_pastes`
    /// drains it each pre-recv scan. A `Vec`, like `graveyard_results`: a middle
    /// click is cheap to repeat, so reads can overlap and must not clobber.
    pub(super) clipboard_paste_results:
        std::sync::Arc<std::sync::Mutex<Vec<std::io::Result<String>>>>,
    /// Landing slot for `J`'s off-thread default (`spawn_jump_default`).
    /// Drained by `apply_jump_defaults`, which keeps only the answer to
    /// `jump_default_seq`.
    pub(super) jump_default_results:
        std::sync::Arc<std::sync::Mutex<Vec<super::jump_default::JumpDefaultAnswer>>>,
    /// The newest `J` default request; an answer to any other is stale.
    pub(super) jump_default_seq: u64,
    /// Landing slot for off-thread clipboard *writes*. One entry per dispatched
    /// write: `None` succeeded, `Some(msg)` is what to flash.
    ///
    /// The write runs off the loop because the helpers legitimately outlive it —
    /// `xclip`/`xsel` keep running to serve the X11 selection, so the reap poll
    /// spends its whole budget on every yank. A `Vec` because yanks can overlap.
    pub(super) clipboard_copy_results: std::sync::Arc<std::sync::Mutex<Vec<Option<String>>>>,
    /// The watcher-driven listing refresh (`FileOp::RefreshListing`) reads the
    /// dir off-thread; `inflight` keeps a single read in flight at a time, and
    /// `dirty` records a refresh requested while one was running so the result
    /// handler can re-spawn for the latest state. See `App::spawn_listing_refresh`.
    pub(super) listing_refresh_inflight: bool,
    pub(super) listing_refresh_dirty: bool,
    /// Landing slot for off-thread inventory operations.
    pub(super) inventory_results:
        std::sync::Arc<std::sync::Mutex<Vec<inventory_ops::InventoryOutcome>>>,
    /// Landing slot for off-thread MCP worktree create/remove/clean ops. The
    /// worker pushes a `WorktreeOutcome` (result + the MCP reply channel) here
    /// and wakes with `Message::Wake(Wake::WorktreeJob)`; `apply_worktree_outcomes`
    /// drains it each pre-recv scan, re-applies refresh+context, then replies.
    pub(super) worktree_results:
        std::sync::Arc<std::sync::Mutex<Vec<worktree_ops::WorktreeOutcome>>>,
    /// Landing slot for the off-thread vertical-split preview reload
    /// (`kick_preview_reload`). The worker stores its `PreviewOutcome` here
    /// (last-wins `Option` — one preview, so no `Vec` is needed) and wakes the
    /// loop with `Message::Wake(Wake::PreviewReload)`; `apply_preview_reloads` drains it
    /// each pre-recv scan. `preview_reloading` is the in-flight guard that
    /// collapses a burst of saves to one trailing re-render (see `preview_ops`).
    pub(super) preview_results:
        std::sync::Arc<std::sync::Mutex<Option<preview_ops::PreviewOutcome>>>,
    pub(super) preview_reloading: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Terminal graphics-protocol capability (Kitty/iTerm2/Sixel/halfblocks +
    /// font cell size), detected ONCE at startup in `setup_terminal` before the
    /// input reader spawns (the `from_query_stdio` reads stdin — the #444 rule).
    /// `None` ⇒ detection failed / no graphics; the mermaid `View` mode then
    /// reports "no image protocol". Cloned into the render worker by `run_effects`.
    pub(super) picker: Option<ratatui_image::picker::Picker>,
}

impl Runtime {
    /// Build a fresh `Runtime` around `active`'s (empty) `ProjectRuntime`.
    /// `git_result_rx` / `git_worker_tx` / `mcp_cmd_rx` are the one real
    /// difference between the production and test-harness callers (the
    /// harness drives neither the git worker nor the MCP socket), so they
    /// stay parameters rather than being hardcoded here.
    pub(super) fn new(
        git_result_rx: Option<std::sync::mpsc::Receiver<super::state::GitWorkerResult>>,
        git_worker_tx: Option<std::sync::mpsc::Sender<super::state::GitWorkerRequest>>,
        mcp_cmd_rx: Option<std::sync::mpsc::Receiver<crate::mcp_cmd::McpRequest>>,
        active: super::state::ProjectId,
    ) -> Self {
        let mut projects = std::collections::HashMap::new();
        projects.insert(active, ProjectRuntime::default());
        Self {
            git_result_rx,
            git_worker_tx,
            mcp_cmd_rx,
            pane_wake_tx: None,
            next_sink_id: 0,
            autosave_last_saved_fp: None,
            autosave_due: None,
            pane_scroll_settle: None,
            scope_waiters: Vec::new(),
            projects,
            active,
            lua: None,
            lua_registry: lua::LuaRegistry::default(),
            lua_inflight: None,
            lua_origin: active,
            lua_events: lua_events::LuaEventState::default(),
            agent_hud: None,
            pending_capture: None,
            capture_spill_dir: None,
            background_tasks: BackgroundTasks::new(),
            find_picker: None,
            next_stream_id: 0,
            agent_status_pending: std::sync::Arc::new(std::sync::Mutex::new(None)),
            agent_status_refreshing: std::sync::Arc::default(),
            codex_pin_pending: std::sync::Arc::new(std::sync::Mutex::new(None)),
            codex_scan_in_flight: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            graveyard_results: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            wiring_results: std::sync::Arc::default(),
            image_results: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            archive_results: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            archive_cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            archive_mount_then: None,
            archive_reads: std::collections::HashMap::new(),
            file_results: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            clipboard_paste_results: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            jump_default_results: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            jump_default_seq: 0,
            clipboard_copy_results: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            listing_refresh_inflight: false,
            listing_refresh_dirty: false,
            inventory_results: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            worktree_results: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            preview_results: std::sync::Arc::new(std::sync::Mutex::new(None)),
            preview_reloading: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            picker: None,
        }
    }

    /// The active project's runtime (pane tabs, overlay slots). `active`
    /// always names a live entry (#99 step 1).
    pub(super) fn project(&self) -> &ProjectRuntime {
        self.projects
            .get(&self.active)
            .expect("active always names a live project")
    }

    /// Mutable [`Self::project`].
    pub(super) fn project_mut(&mut self) -> &mut ProjectRuntime {
        self.projects
            .get_mut(&self.active)
            .expect("active always names a live project")
    }

    /// Every open project's tab set, by project. What spyc does for its tabs
    /// in the background (draining output, marking exits, resizing, shutting
    /// down) it does for every project's, not only the one in view.
    pub(super) fn every_tab_set(
        &self,
    ) -> impl Iterator<Item = (super::state::ProjectId, &PaneTabs)> {
        self.projects
            .iter()
            .filter_map(|(id, project)| Some((*id, project.pane_tabs.as_ref()?)))
    }

    /// Mutable [`Self::every_tab_set`].
    pub(super) fn every_tab_set_mut(
        &mut self,
    ) -> impl Iterator<Item = (super::state::ProjectId, &mut PaneTabs)> {
        self.projects
            .iter_mut()
            .filter_map(|(id, project)| Some((*id, project.pane_tabs.as_mut()?)))
    }

    /// The project holding the live tab `pane_id`, whichever one that is.
    pub(super) fn project_of_pane(&self, pane_id: &str) -> Option<super::state::ProjectId> {
        self.projects.iter().find_map(|(id, project)| {
            let tabs = project.pane_tabs.as_ref()?;
            tabs.tabs()
                .iter()
                .any(|t| t.info.id == pane_id)
                .then_some(*id)
        })
    }

    /// The tab `pane_id` in whichever project holds it, with its 1-based
    /// number there.
    pub(super) fn tab_of_pane(&self, pane_id: &str) -> Option<(usize, &crate::pane::TabEntry)> {
        self.projects.values().find_map(|project| {
            let tabs = project.pane_tabs.as_ref()?;
            let (index, tab) = tabs
                .tabs()
                .iter()
                .enumerate()
                .find(|(_, t)| t.info.id == pane_id)?;
            Some((index + 1, tab))
        })
    }
}
