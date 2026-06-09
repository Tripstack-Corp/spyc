//! The App event loop: setup, the recv → dispatch → render cycle, and teardown.
//!
//! `run()` plus its scratch-builder `run_setup`, the per-iteration
//! `dispatch_effective` / `render_frame`, `run_teardown`, and the
//! `term_title_effect` helper. `App::new` (the constructor) lives in
//! `bootstrap.rs`.

use super::sources::{coalesce_recv, sync_listing_watch, take_reader_result};
use super::{
    App, DispatchFlow, Draw, Duration, Effect, Event, ForegroundExec, KeyCode, KeyEventKind,
    Message, PathBuf, Result, RunCtx, Scheduler, Tui, spawn_input_reader,
};

impl App {
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
            self.state.git_cache.current_gitdir.as_deref(),
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
                    Event::Paste(text) => {
                        let effects = self.handle_paste(text);
                        self.run_effects(effects, terminal, foreground_exec)?;
                    }
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
                | Message::PagerStreamOutput
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

            // Overlay dismissal goes through overlay_awaiting_dismiss (stays
            // visible until Enter); pending_overlay_close is inert here.
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

            // The unified pager-stream drain (the `pager_stream` abstraction
            // grep / git-view / transcript collapse onto). A no-op while no
            // stream is active; id-gated against the live pager's `stream_id`.
            if self.drain_pager_stream() {
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
            // requests in `state.git_cache.pending_git_requests` — the Model owns no
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
                    self.state.git_cache.current_gitdir.as_deref(),
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
    pub(super) fn term_title_effect(&mut self) -> Option<Effect> {
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
