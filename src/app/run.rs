//! The App event loop: setup, the recv → dispatch → render cycle, and teardown.
//!
//! `run()` plus its scratch-builder `run_setup`, the per-iteration
//! `dispatch_effective` / `render_frame`, `run_teardown`, and the
//! `term_title_effect` helper. `App::new` (the constructor) lives in
//! `bootstrap.rs`.

use crate::spyc_debug;

use super::sources::{coalesce_recv, take_reader_result};
use super::watch::{WatchCommand, spawn_watch_worker};
use super::{
    App, Duration, Effect, Event, ForegroundExec, KeyCode, KeyEventKind, Message, PathBuf, Result,
    Scheduler, Tui, spawn_input_reader,
};

/// Per-iteration draw accumulator for the event loop. `dirty` is an OR
/// across every step in an iteration; `reason` is last-writer-wins,
/// matching the old `draw_reason = N` overwrite semantics. Reset by the
/// render block each iteration.
///
/// `reason` codes: 0 = none, 1 = pane output, 2 = input/git, 3 = other
/// (refresh / config / repaint / activity).
#[derive(Default)]
pub(super) struct Draw {
    dirty: bool,
    reason: u8,
}

impl Draw {
    /// Mark the frame dirty and set the reason (last writer wins).
    pub(super) const fn mark(&mut self, reason: u8) {
        self.dirty = true;
        self.reason = reason;
    }

    /// Mark dirty WITHOUT touching `reason` — for the activity-only
    /// rollover redraw, which must not bump a real `draw_reason` (the
    /// render-stats block reads `reason` and skips counting activity-only
    /// frames).
    pub(super) const fn set_dirty(&mut self) {
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
pub(super) enum DispatchFlow {
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
/// signatures (`fn …(&mut self, ctx: &mut RunCtx)`). Its fields are
/// `pub(super)`, which from here is exactly the reachability they had as
/// private items of `app`: the whole `app` subtree, which is what the sibling
/// loop-step modules read them through.
pub struct RunCtx {
    /// Command sender to the off-thread watch worker (`watch::spawn_watch_worker`).
    /// `None` if the watcher couldn't be created (degrades to poll-only).
    pub(super) watch_tx: Option<std::sync::mpsc::Sender<WatchCommand>>,
    /// Last listing dir we sent a watch command for; on chdir we send a new
    /// `SyncListing` so the worker re-points its watches. Purely a send-dedup
    /// key — the worker owns the actual watch topology.
    pub(super) watched_listing: Option<PathBuf>,
    /// Last second-commander (column `b`) listing dir we sent a watch command
    /// for. Re-sent when it changes (open / chdir / close `b`) so the worker
    /// watches `b`'s tree + gitdir too — `b`'s git markers refresh on
    /// fs-events, not just the ≤1 s poll.
    pub(super) watched_listing_right: Option<PathBuf>,
    /// Last vertical-split preview file we sent a watch command for. The watch
    /// topology is re-sent when this OR `watched_listing` changes, so opening /
    /// swapping / closing the preview re-points the preview-parent watch.
    pub(super) watched_preview: Option<PathBuf>,
    pub(super) scheduler: Scheduler,
    /// Coalesce buffers: the recv arm pushes here; the pre-recv drains process them.
    pub(super) fs_pending: Vec<notify::Event>,
    pub(super) git_pending: Vec<super::state::GitWorkerResult>,
    pub(super) mcp_pending: Vec<crate::mcp_cmd::McpRequest>,
    pub(super) last_context_write: std::time::Instant,
    pub(super) last_refresh: std::time::Instant,
    pub(super) last_git_poll: std::time::Instant,
    /// Trailing-debounce: last listing event; refresh fires after `REFRESH_QUIET` of quiet.
    pub(super) last_event_at: Option<std::time::Instant>,
    /// First listing event since the last refresh (fixed, not bumped) — caps refresh deferral.
    pub(super) first_event_after_refresh: Option<std::time::Instant>,
    /// Last keypress instant — suppresses the MCP context-write for 300ms after a keystroke.
    pub(super) last_input_at: Option<std::time::Instant>,
    pub(super) draw: Draw,
}

#[cfg(test)]
impl RunCtx {
    /// Build a `RunCtx` with no fs watcher (the loop-step unit tests never
    /// spawn one) and fresh empty scratch — so the step methods, which now
    /// take `&mut RunCtx`, can be driven from tests.
    pub(super) fn for_test() -> Self {
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
    /// Build the run()-scoped scratch (`RunCtx`): fs watcher, scheduler,
    /// coalesce buffers, timers, and the `Draw` accumulator; also spawns the
    /// detached git/MCP forwarder threads. Borrows `msg_tx` so `run()` can hand
    /// the original to the input reader. Does NOT spawn the reader or build
    /// `foreground_exec` — those stay `run()` locals for Drop ordering.
    fn run_setup(&mut self, msg_tx: &std::sync::mpsc::Sender<Message>) -> RunCtx {
        // Filesystem watching runs on a dedicated worker thread
        // (`watch::spawn_watch_worker`). notify's recursive `watch()` does a
        // synchronous per-subdir `inotify_add_watch` walk on Linux, so the
        // watcher — and that potentially-blocking walk — must stay off the
        // event-loop/input thread. The worker owns the `RecommendedWatcher`,
        // posts events back as `Message::FsEvent`, and self-terminates when
        // `watch_tx` drops at teardown. We pass it the config-file *parent*
        // dirs (editors replace-on-save, removing the file inode) and then
        // send an initial `SyncListing` to watch the listing dir recursively.
        let config_parents: Vec<PathBuf> = self
            .candidate_config_paths()
            .into_iter()
            .filter_map(|p| p.parent().map(std::path::Path::to_path_buf))
            .collect();
        let watch_tx = spawn_watch_worker(msg_tx, config_parents);

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
        // `Wake::Pane`. Set BEFORE the reader moves `msg_tx` and
        // before the loop processes any user action — so every pane spawned
        // during the session (including session-restore tabs) gets a live
        // wake, not the pre-run no-op.
        self.runtime.pane_wake_tx = Some(msg_tx.clone());

        let mut ctx = RunCtx {
            watch_tx,
            watched_listing: None,
            watched_listing_right: None,
            watched_preview: None,
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
        };
        self.sync_watch(&mut ctx);
        ctx
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
                        self.run_effects(effects, terminal, foreground_exec);
                        // An executor's flash (e.g. `SignalGroup`'s "task #N
                        // paused") lands after the handler, so re-home it here
                        // too — the pager still covers the status bar (#166).
                        if std::mem::take(&mut self.view.input_went_to_pager) {
                            self.relocate_flash_into_pager();
                        }
                    }
                    Event::Paste(text) => {
                        let effects = self.handle_paste(text);
                        self.run_effects(effects, terminal, foreground_exec);
                    }
                    Event::Resize(cols, rows) => self.handle_resize(cols, rows),
                    // Only arrives while `[mouse] capture` is on — with it off the
                    // terminal sends DEC 1007 arrow keys instead. The decision is
                    // pure and lives in `mouse.rs`; this arm is the wire.
                    Event::Mouse(m) => {
                        let effects = self.handle_mouse(m);
                        self.run_effects(effects, terminal, foreground_exec);
                    }
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
            // Buffered or collapsed by the coalesce pre-step, so never surfaced
            // as `effective`: the payload variants ride `ctx.*_pending`, and every
            // `Wake` kind becomes a synthesized Timeout. Four patterns instead of
            // twenty — a new wake can't be missed here, because there is nothing
            // per-kind left to miss.
            Ok(
                Message::FsEvent(_) | Message::GitResult(_) | Message::Mcp(_) | Message::Wake(_),
            ) => {
                // Loud in tests, survivable in production.
                //
                // This is spyc's event loop, and a panic here takes the whole
                // session with it: the user's pane children, their place in a
                // scrollback, an unsaved prompt. That is a brutal price for a
                // condition whose safe answer is obvious — this iteration has
                // nothing to do, so fall through and let the next `recv` proceed,
                // costing one wasted wakeup. An unreachable! here is what turned a
                // missed match arm into "spyc dies when you copy text" (#402).
                //
                // The message is still dropped either way, so nothing is owed: a
                // panic didn't reply to a stranded MCP client either.
                if cfg!(debug_assertions) {
                    unreachable!(
                        "buffered/collapsed message surfaced as `effective` from the coalesce pre-step"
                    );
                }
                spyc_debug!("coalesce leak: a buffered/collapsed message reached dispatch");
            }
        }
        Ok(DispatchFlow::Proceed)
    }

    /// Render one frame iff the accumulator is dirty (extracted verbatim from
    /// the loop's `if ctx.draw.dirty { … }` block). Composes the term-title
    /// effect, wraps the draw in a DEC 2026 synchronized update, honours the
    /// per-iteration `pending_clear`, times the build/whole-frame for the
    /// activity monitor, and counts the draw (skipping `activity_only` frames
    /// so the stats don't oscillate — H6). Resets `ctx.draw` for the next
    /// iteration. `?`-propagates `terminal.draw` / `clear` (`run_effects` is
    /// infallible — a failed foreground spawn flashes, never aborts).
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
        self.run_effects(title_fx, terminal, foreground_exec);
        // Wrap in DEC 2026 synchronized update so the terminal emulator
        // (iTerm2, etc.) buffers the entire frame and paints it atomically —
        // eliminates tearing and reduces terminal-side CPU. EXCEPT while the
        // mermaid image overlay is up: iTerm2 drops inline-image (OSC 1337)
        // escapes emitted inside a synchronized update, so the diagram never
        // paints. The overlay is a single static frame, so skipping the sync
        // wrap there costs nothing.
        use crossterm::terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate};
        let sync_update = self.view.image_view.is_none();
        if sync_update {
            let _ = crossterm::execute!(terminal.backend_mut(), BeginSynchronizedUpdate);
        }
        if pending_clear {
            // SPYC-TRAP(cursor-read-ssh): keep this on `force_full_repaint`, NOT
            // `terminal.clear()` — clear()'s `get_cursor_position()` (`ESC[6n`)
            // round-trip hangs/errs over SSH and races the input reader.
            crate::force_full_repaint(terminal)?;
        }
        let draw_start = std::time::Instant::now();
        let frame_area = terminal
            .draw(|frame| {
                // Time just the buffer build (CPU) so we can separate
                // it from the diff + tty emission measured below.
                let render_start = std::time::Instant::now();
                self.render(frame);
                // Single choke point: quantize RGB → 256-colour for truecolor-blind
                // terminals (old GNU screen). No-op at TrueColor, so capable
                // terminals pay nothing; catches theme + syntect + diff + ANSI
                // passthrough alike since it runs on the finished buffer.
                crate::ui::color_depth::downgrade_buffer(frame.buffer_mut(), self.view.color_depth);
                // Here and not in a renderer: a VS16 emoji reaches the buffer
                // from a flash, a file name and an agent's output alike.
                crate::ui::emoji_diff::pin_emoji_widths(frame.buffer_mut());
                if self.view.show_activity {
                    let us = u64::try_from(render_start.elapsed().as_micros()).unwrap_or(u64::MAX);
                    self.view.activity.peaks_live.render_us =
                        self.view.activity.peaks_live.render_us.max(us);
                }
            })?
            .area;
        // Whole-frame peak (build + diff + emission) = the full
        // main-thread render stall. `frame - render` ≈ diff + emission.
        if self.view.show_activity {
            let us = u64::try_from(draw_start.elapsed().as_micros()).unwrap_or(u64::MAX);
            self.view.activity.peaks_live.frame_us = self.view.activity.peaks_live.frame_us.max(us);
        }
        if sync_update {
            let _ = crossterm::execute!(terminal.backend_mut(), EndSynchronizedUpdate);
        }
        if self.view.show_activity && !activity_only {
            self.view.activity.live.draws += 1;
            self.view.activity.live.bytes +=
                u64::from(frame_area.width) * u64::from(frame_area.height);
            match ctx.draw.reason {
                1 => self.view.activity.live.reason_pane += 1,
                2 => self.view.activity.live.reason_event += 1,
                _ => self.view.activity.live.reason_other += 1,
            }
        }
        ctx.draw.reason = 0;
        Ok(())
    }

    /// Exit teardown: remove the MCP context file, then SIGTERM-grace every
    /// pane child tree before `App` is dropped (the per-Pane `Drop` is a
    /// SIGKILL safety net; going through `shutdown` first gives well-behaved
    /// children — `vite`, `npm run dev`, anything that catches SIGTERM —
    /// 250ms to flush before we escalate, so quitting with a dev server in a
    /// pane doesn't orphan its process tree).
    ///
    /// Stays **synchronous** (unlike the interactive `^a x` close, which
    /// off-threads via `Pane::shutdown_detached`): the process is about to
    /// exit, and a detached reaper would die with it before killing children
    /// in their own `setsid` process groups — orphaning the tree.
    ///
    /// Called from `main` *after* `restore_terminal`, so it runs on the normal
    /// screen: a child that doesn't exit promptly gets a `spyc: waiting for …`
    /// line naming it, instead of a silent freeze behind the alt-screen. Must
    /// run on every exit path (the PR8b guarantee) — `main` calls it
    /// unconditionally regardless of the run loop's result.
    pub fn run_teardown(&mut self) {
        crate::context::remove_context_file(&self.view.context_path);
        // Remove the MCP client configs we wrote on agent launch — our socket
        // is about to die, so a lingering entry would point at nothing.
        self.cleanup_written_mcp_configs();
        // Drop every archive staging tree this session created. The startup
        // sweep is the backstop for a process that never gets here.
        self.clean_all_staging();
        // Drop the `!`-capture spill dir now (it also auto-removes on Runtime
        // drop, but do it explicitly so the temp files don't linger if the
        // process exits without unwinding). Capture buffers are session-scoped.
        self.runtime.capture_spill_dir.take();
        if let Some(tabs) = self.runtime.pane_tabs.as_mut() {
            for entry in tabs.tabs_mut() {
                let label = entry.info.label.clone();
                let pid = entry.pane.process_id();
                entry
                    .pane
                    .shutdown_reporting(Duration::from_millis(250), || match pid {
                        Some(p) => eprintln!("spyc: waiting for {label} (pid {p}) to exit…"),
                        None => eprintln!("spyc: waiting for {label} to exit…"),
                    });
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

        // All run()-scoped scratch (the fs-watch worker's command sender + its
        // topology dedup key, scheduler, coalesce buffers, debounce timers,
        // last-keypress instant, the Draw accumulator) lives in `ctx`.
        // `run_setup` also spawns the fs-watch worker + the git/MCP forwarder
        // threads and installs `pane_wake_tx` (all needing a `msg_tx` clone).
        // Declared BEFORE `reader_handle` so `ctx` — and thus `watch_tx` —
        // drops AFTER the reader thread is joined; dropping `watch_tx` signals
        // the watch worker to exit and drop its `RecommendedWatcher` (H8).
        let mut ctx = self.run_setup(&msg_tx);

        // Load `~/.config/spyc/init.lua` now that `pane_wake_tx` is wired (so
        // `ensure_lua_worker` can spawn). Submits a `Load` job whose
        // registrations land via the first `handle_lua_done` drain below; a
        // no-op when there's no init.lua. Done here, not in `bootstrap`, because
        // the worker needs the wake channel `run_setup` just installed.
        self.load_init_lua();

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

        // Enforce the graveyard cap OFF the startup path. The single directory
        // walk plus each eviction's decompress + system-trash call is disk IO
        // proportional to the graveyard's size, and running it inside
        // `App::new` meant a full graveyard reached the user as a launch that
        // appeared to hang — no TUI yet, so nothing on screen explained it.
        //
        // Emitted here rather than in `bootstrap` for the same reason as
        // `load_init_lua` above: the worker wakes the loop through the channel
        // `run_setup` installs, and `run_effects` needs the `foreground_exec`
        // built just above. The op decides for itself whether there is anything
        // to evict, so the main thread never touches the directory.
        self.run_effects(
            vec![Effect::Graveyard(
                super::graveyard_ops::GraveyardOp::CascadeToCap {
                    cap: crate::state::graveyard::GRAVEYARD_CAP_BYTES,
                },
            )],
            terminal,
            &foreground_exec,
        );

        // (Pre-v1.50.84 the loop carried `last_pane_render` and
        // `last_active_drain` timestamps to throttle pane renders /
        // parses while the user was typing. Both became unnecessary
        // once parsing moved to a per-Pane worker thread — they
        // were just delaying the moment the main thread noticed
        // the worker had finished an echo, which manifested as
        // off-by-one input lag. Removed.)

        // Single exit funnel: every termination path (normal quit, reader
        // death, a `DispatchFlow::Exit`, or a `?` error from dispatch/render)
        // `break`s its result out of this loop so `run_teardown` ALWAYS runs.
        // The early `return`s that used to live here skipped teardown,
        // orphaning pane children (no graceful SIGTERM) and leaking the
        // context file on reader-death / handler-error exits.
        let exit_result: Result<()> = loop {
            if self.state.should_quit {
                break Ok(());
            }
            // MVU Phase 3d: authoritative reader-death exit. With the poll
            // floor gone, the loop blocks on `recv()`; the reader sends a
            // `ReaderExited` wake on death to kick that recv, but this
            // level-triggered check is what actually exits — it can't be
            // consumed by `coalesce_pending` (which drops the wake), so it
            // catches death even when the edge wake races a real message.
            if reader_done.load(std::sync::atomic::Ordering::Acquire) {
                break take_reader_result(&read_err);
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

            // Land what the workers finished, then settle timers and
            // reconciles, all before the loop blocks (see `pre_recv.rs`).
            self.drain_landed_work(&mut ctx, terminal, &foreground_exec);
            self.settle_before_recv(&mut ctx, terminal, &foreground_exec);

            // MVU Phase 3c/3d: the poll floor AND the `MAX_IDLE_CAP` poll
            // ceiling are both GONE. Every event source now wakes the channel —
            // input, fs, git (3a), panes (`PaneOutput`), captures/tasks
            // (`SinkOutput`), MCP, finder, grep (3d), and reader-death. The
            // loop blocks on `recv()` when no deadline is armed; an armed
            // deadline only SHORTENS the wait (it never lengthens it, and
            // there's no ceiling clamp — the 1s GitPoll drives a 1s wait, a
            // farther-out deadline a longer wait). Fresh clock JUST before recv so a
            // deadline-driven sleep lands on the deadline, not deadline +
            // body-cost.
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
            ) {
                Ok(DispatchFlow::Continue) => {
                    // The scroll-throttle skips this iteration's render. But
                    // `step_pager_repaint` above already consumed
                    // `needs_full_repaint` into `pending_clear` (clearing the
                    // flag) — dropping it here would leave the requested screen
                    // clear unperformed and stale cells on screen. `ctx.draw`
                    // survives the `continue` (it's reset only inside
                    // `render_frame`), but the consumed clear does not, so
                    // re-arm it for the next rendered frame.
                    if pending_clear {
                        self.view.needs_full_repaint = true;
                    }
                    continue;
                }
                Ok(DispatchFlow::Exit(result)) => break result,
                Ok(DispatchFlow::Proceed) => {}
                Err(e) => break Err(e),
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

            // which-key chord-hint popup: show it once the hint delay elapses
            // while a chord is still armed, and (dis)arm its wake. Before
            // `render_frame` so the popup lands in this frame.
            self.settle_chord_hint(now_post, &mut ctx);

            // Re-arm the post-recv advisory deadlines — ActivityRollover +
            // CaptureTick (see `arm_post_recv_deadlines`).
            self.arm_post_recv_deadlines(now_post, &mut ctx);

            // Render the frame iff dirty (see `render_frame`): title effect,
            // DEC 2026 synchronized-update wrap, optional clear, the timed
            // draw, and the activity stats.
            if let Err(e) = self.render_frame(
                terminal,
                &foreground_exec,
                pending_clear,
                activity_only_draw,
                &mut ctx,
            ) {
                break Err(e);
            }

            self.sync_watch(&mut ctx);

            // Event-driven MCP context-file write — debounced + typing-burst
            // suppressed, with ContextWrite deadline arming (see
            // `maybe_write_context`).
            self.maybe_write_context(now_post, &mut ctx);
        };
        // `run_teardown` is intentionally NOT called here — `main` runs it
        // after `restore_terminal`, so its "waiting for …" lines land on the
        // normal screen instead of behind the alt-screen.
        exit_result
    }

    /// Recompute the host-terminal window title from project / session
    /// state and emit OSC 2 if it has changed since the last write.
    /// Compose the terminal title and, if it changed, return the
    /// `SetTerminalTitle` effect to emit (the run loop runs it via
    /// `run_effects`, the sole executor of the `term_title::set` IO). The
    /// compose + dedup stay loop-side: `last_term_title` is advanced here,
    /// and the foreground-exec after-work resets it to `None` to force a
    /// re-emit. Returns `None` when the title is unchanged.
    pub(super) fn term_title_effect(&mut self) -> Option<Effect> {
        let title = crate::term_title::compose(
            self.state.project_home.as_deref(),
            self.state.session_name.as_deref(),
            &self.state.cur().listing.dir,
        );
        if self.view.last_term_title.as_deref() == Some(&title) {
            return None;
        }
        self.view.last_term_title = Some(title.clone());
        Some(Effect::SetTerminalTitle { title })
    }
}
