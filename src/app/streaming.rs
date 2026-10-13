//! MVU Phase 3c: the streaming pull-source drains (foreground `!` capture +
//! background tasks + the task-viewer refresh), extracted from the `App::run`
//! loop body. These are `impl App` methods living in a child module, reading
//! App's private state directly via the descendant-module rule.
//!
//! Each returns whether it produced a redraw-worthy change this tick; the
//! run loop sets `needs_draw`/`draw_reason` from the return. Behaviour is
//! unchanged from the pre-3c inline blocks except for the `clear_wake_pending`
//! calls (the clear-before-read half of the lost-wakeup protocol — a no-op
//! until a capture/task installs its wake slot in this phase).

use super::{App, TASK_BUFFER_CAP, TaskStatus, buffer_to_lines, eof_marker_line};
use crate::pane::pty_host::ExitOutcome;

/// MVU Phase 5 PR8: a child-exit event, finalized by [`App::apply_exit_event`].
///
/// **Design B** (chosen over the plan's waiter-thread sketch): the child stays
/// on the main thread and is reaped *synchronously* in the pre-recv scan via
/// the bounded [`crate::pane::pty_host::PtyHost::reap_exit`] (EOF ⇒ the child
/// has already exited, so the reap returns immediately — never speculative).
/// These are therefore loop-synthesized **synchronous** events, NOT channel
/// `Message`s: the producer is the loop's own reap, so a channel round-trip
/// would only add latency + reorder the exit behind a concurrent keystroke.
/// They realize the plan's `CaptureExit` / `TaskExited`; `PaneExited` (and
/// `PaneTarget::Sink`) are deferred — both need a pane-stored `SinkId` (which
/// doesn't exist yet; the id today is only a wake-closure trace label), and a
/// pane exit has no inline blocking reap to relocate (it's a non-blocking
/// `try_wait` + a `[exited N]` tab label in `tabs::mark_exited`). Phase 6 can
/// promote these into the unified `update(&mut Model, Message)` if exits ever
/// become worker-produced.
enum ExitEvent {
    /// The foreground `!` capture's child exited — write the ✓/✗ pager
    /// title + `[EOF]` frame and clear `pending_capture`.
    CaptureExit { status: ExitOutcome },
    /// Background task `id`'s child exited — flash the toast + set its
    /// terminal `TaskStatus`.
    TaskExited { id: u32, status: ExitOutcome },
}

/// Front-trim a streaming output buffer to `TASK_BUFFER_CAP`, dropping the
/// oldest bytes. Shared by the background-task and foreground-`!`-capture
/// drains so both stay memory-bounded — and so the per-tick `into_text()`
/// rebuild that re-parses the whole buffer stays O(CAP), not O(total output).
fn cap_stream_buffer(buf: &mut Vec<u8>) {
    if buf.len() > TASK_BUFFER_CAP {
        let drop_n = buf.len() - TASK_BUFFER_CAP;
        buf.drain(..drop_n);
    }
}

/// Backstop for the on-disk capture spill (`PendingCapture::full_log`): far
/// above any normal command (a full `git log` is ~1.5 MiB) — purely so a
/// runaway `!yes` can't fill the disk. The spill keeps the HEAD, so for
/// newest-first output like `git log` it's the newest commits that stay.
const CAPTURE_SPILL_MAX: u64 = 64 * 1_048_576;

/// Bytes to render in the FINISHED capture pager: the full on-disk spill (head
/// intact) when present and readable, else the front-trimmed live `buffer`. The
/// `bool` is whether the spill hit [`CAPTURE_SPILL_MAX`] (→ a "tail omitted"
/// marker). The read/parse happens once, at exit — not per streaming tick — so
/// being O(total) here is fine where the live path's O(CAP) cap is load-bearing.
fn finished_capture_lines(
    spill: Option<&super::capture::CaptureSpill>,
    capped_buffer: &[u8],
) -> (Vec<ratatui::text::Line<'static>>, bool) {
    let spilled = spill.and_then(|s| std::fs::read(&s.path).ok());
    let lines = buffer_to_lines(spilled.as_deref().unwrap_or(capped_buffer));
    let hit_cap = spill.is_some_and(|s| s.bytes >= CAPTURE_SPILL_MAX);
    (lines, hit_cap)
}

impl App {
    /// MVU Phase 5 PR8: finalize a child exit. The single handler the two
    /// streaming drains dispatch into once they've detected `newly_closed`
    /// and bounded-reaped the status — the update-shaped seam Phase 6 lifts
    /// into the unified `update`. Each arm reproduces the pre-PR8 inline
    /// finalize byte-for-byte; only the reap moved a frame upstream.
    fn apply_exit_event(&mut self, ev: ExitEvent) {
        match ev {
            ExitEvent::CaptureExit { status } => {
                let Some(capture) = self.runtime.pending_capture.take() else {
                    return;
                };
                // Status glyph mirrors the bottom-status-bar conventions
                // (✓ for exit 0, ✗ for everything else) so the pager title
                // tells the user at a glance whether their command succeeded.
                let (exit_info, ok) = match status {
                    ExitOutcome::Exited { code, success } => {
                        if success {
                            ("exit 0".to_string(), true)
                        } else {
                            (format!("exit {code}"), false)
                        }
                    }
                    ExitOutcome::Errored(e) => (format!("error: {e}"), false),
                };
                let glyph = if ok { "\u{2713}" } else { "\u{2717}" }; // ✓ / ✗
                let title = format!("{glyph} {} — {exit_info}", capture.title);
                // Final rebuild from the FULL on-disk spill (head intact) rather
                // than the front-trimmed live buffer, so a large capture like
                // `git log` shows its start instead of beginning mid-output.
                let (lines, spill_capped) =
                    finished_capture_lines(capture.full_log.as_ref(), &capture.buffer);
                if let Some(view) = self.view.pager.as_mut() {
                    view.title = title;
                    view.lines = lines;
                    // If the spill hit its backstop, say so before the EOF frame.
                    if spill_capped {
                        view.lines.push(eof_marker_line(&format!(
                            "capture exceeded {} MiB — tail omitted",
                            CAPTURE_SPILL_MAX / 1_048_576
                        )));
                    }
                    // Anchor an EOF marker to the bottom of content so it's
                    // visible even when output exceeds viewport_h.
                    view.lines.push(eof_marker_line(&exit_info));
                    view.eof_in_content = true;
                    view.saveable = true;
                    view.streaming = false;
                    view.scroll_to_bottom_auto();
                }
            }
            ExitEvent::TaskExited { id, status } => {
                let Some(task) = self
                    .runtime
                    .background_tasks
                    .tasks
                    .iter_mut()
                    .find(|t| t.id == id)
                else {
                    return;
                };
                let (status_text, status_val) = match status {
                    ExitOutcome::Exited { code, success } => {
                        if success {
                            ("exit 0".to_string(), TaskStatus::Exited(0))
                        } else {
                            #[allow(clippy::cast_possible_wrap)]
                            let code = code as i32;
                            (format!("exit {code}"), TaskStatus::Exited(code))
                        }
                    }
                    ExitOutcome::Errored(msg) => {
                        (format!("error: {msg}"), TaskStatus::Crashed(msg))
                    }
                };
                task.status = status_val;
                task.finished_at = Some(std::time::Instant::now());
                let secs = task.started.elapsed().as_secs();
                let cmd_display = task.cmd_display.clone();
                self.state.flash_info(format!(
                    "task #{id}: {cmd_display} — {status_text} ({secs}s)"
                ));
            }
        }
    }

    /// Drain the foreground `!` capture into its pager and finalize on EOF.
    /// Returns whether a redraw is needed. MVU Phase 3c PR3: with the poll
    /// floor gone, this is gated on actual change — new output, the exit
    /// frame, or the elapsed-timer's displayed seconds ticking over — so a
    /// quiet running capture redraws ~1 Hz (driven by the MAX_IDLE_CAP wake)
    /// rather than every iteration, and idle draws stay at 0.
    pub(crate) fn drain_pending_capture(&mut self) -> bool {
        let mut redraw = false;
        // MVU Phase 5 PR8: the reaped exit, dispatched to `apply_exit_event`
        // after the `&mut self.runtime.pending_capture` borrow below ends.
        let mut exit = None;
        if let Some(capture) = &mut self.runtime.pending_capture {
            // MVU Phase 3c: clear-before-read (paired with the reader CAS).
            capture.host.clear_wake_pending();
            let mut got_data = false;
            let mut chunks: Vec<Vec<u8>> = Vec::new();
            let drain = capture.host.drain(|bytes| chunks.push(bytes.to_vec()));
            for chunk in chunks {
                // Tee the raw bytes to the on-disk spill (the full, uncapped
                // record) BEFORE the in-memory buffer front-trims its head, so
                // the finished pager can show what the live preview dropped —
                // e.g. `git log`'s newest commits. Bounded by CAPTURE_SPILL_MAX;
                // a write error gives up on the spill (fall back to the buffer).
                if let Some(spill) = capture.full_log.as_mut()
                    && spill.bytes < CAPTURE_SPILL_MAX
                {
                    use std::io::Write;
                    if spill.file.write_all(&chunk).is_ok() {
                        spill.bytes += chunk.len() as u64;
                    } else {
                        capture.full_log = None;
                    }
                }
                capture.buffer.extend_from_slice(&chunk);
                // Cap the capture buffer exactly like a background task's
                // (drain_background_tasks). Without this, a chatty `!cargo
                // build` / `!make test` grows `capture.buffer` without bound,
                // and the per-tick `into_text()` rebuild below re-parses the
                // ENTIRE accumulated buffer every chunk — O(total output)
                // memory and O(n^2) cumulative ANSI parse on the input thread.
                // Front-trimming to TASK_BUFFER_CAP bounds both to parity with
                // the (already-capped) task path: steady-state O(CAP) per tick.
                cap_stream_buffer(&mut capture.buffer);
                got_data = true;
            }
            redraw |= got_data;
            if drain.newly_closed {
                capture.finished = true;
            }
            // Update elapsed timer in the title while running; redraw only
            // when the displayed seconds string actually changes.
            if !capture.finished {
                let elapsed = capture.started.elapsed().as_secs();
                let human_time = super::format_elapsed_hms(elapsed);
                let timer_title = format!("\u{23f3} {} — running... ({human_time})", capture.title);
                if let Some(view) = self.view.pager.as_mut() {
                    if view.title != timer_title {
                        redraw = true;
                    }
                    view.title = timer_title;
                }
            }
            if got_data || capture.finished {
                // Rebuild pager content from the accumulated buffer.
                let lines = buffer_to_lines(&capture.buffer);
                // "At bottom" detection uses the actual rendered viewport
                // height -- before this we hardcoded 40, which under-shoots
                // on tall terminals and made the streaming-capture auto-tail
                // leave the top half of the pager showing content with `~`
                // markers filling the rest until the user manually scrolled.
                // last_viewport_h is set by the renderer on every frame.
                let at_bottom = self.view.pager.as_ref().is_some_and(|v| {
                    let h = v.last_viewport_h.get();
                    let h = if h == 0 { 40 } else { h };
                    let total = v.line_count();
                    let page = v.page_lines(h);
                    v.scroll >= total.saturating_sub(page)
                });
                if let Some(view) = self.view.pager.as_mut() {
                    view.lines = lines;
                    if at_bottom {
                        view.scroll_to_bottom_auto();
                    }
                }
            }
            if capture.finished {
                // MVU Phase 5 PR8: bounded reap (the reader already saw EOF,
                // so this returns immediately — see `PtyHost::reap_exit`).
                // Defer the ✓/✗ title + [EOF] finalize to `apply_exit_event`
                // once this `&mut self.runtime.pending_capture` borrow ends.
                exit = Some(capture.host.reap_exit());
            }
        }
        if let Some(status) = exit {
            self.apply_exit_event(ExitEvent::CaptureExit { status });
            redraw = true; // the exit frame (✓/✗ title + [EOF]) must draw
        }
        redraw
    }

    /// Drain every running background task into its buffer and harvest exit
    /// on EOF. Returns `true` when a redraw is needed: a task finished (the
    /// toast + divider), OR a backgrounded task's `[N+]` divider just
    /// appeared (its `has_unread_output` flipped false→true this tick). MVU
    /// Phase 3c PR3: with the floor gone, the divider-appearance redraw must
    /// be driven here; further chunks don't change the glyph, so only the
    /// transition redraws (idle draws stay at 0 for a chatty quiet-divider task).
    pub(crate) fn drain_background_tasks(&mut self) -> bool {
        // MVU Phase 5 PR8: reaped exits, dispatched to `apply_exit_event`
        // after the `&mut self.runtime.background_tasks.tasks` loop borrow ends.
        let mut exited: Vec<(u32, ExitOutcome)> = Vec::new();
        let mut divider_appeared = false;
        for task in &mut self.runtime.background_tasks.tasks {
            if !matches!(task.status, TaskStatus::Running) {
                continue;
            }
            // MVU Phase 3c: clear-before-read (paired with the reader CAS).
            task.host.clear_wake_pending();
            let was_unread = task.has_unread_output;
            let mut chunks: Vec<Vec<u8>> = Vec::new();
            let drain = task.host.drain(|bytes| chunks.push(bytes.to_vec()));
            for chunk in chunks {
                task.buffer.extend_from_slice(&chunk);
                task.has_unread_output = true;
                cap_stream_buffer(&mut task.buffer);
            }
            // The `[N+]` glyph reflects has_unread_output yes/no, so only its
            // false→true transition needs a redraw — not every chunk.
            if !was_unread && task.has_unread_output {
                divider_appeared = true;
            }
            if drain.newly_closed {
                // MVU Phase 5 PR8: bounded reap now, while the host is
                // borrowed (the reader saw EOF → the child is gone, so this
                // returns immediately). The `TaskExited` finalize (flash +
                // `TaskStatus`) runs after the loop releases the tasks borrow.
                // The task stays `Running` until then — still within this same
                // pre-recv scan, before the drain returns — so the subsequent
                // `refresh_task_viewer` observes the finalized status.
                exited.push((task.id, task.host.reap_exit()));
            }
        }
        let finished_any = !exited.is_empty();
        for (id, status) in exited {
            self.apply_exit_event(ExitEvent::TaskExited { id, status });
        }
        divider_appeared || finished_any
    }

    /// If a task-viewer pager is open, refresh its content from the live task
    /// buffer (the bg drain above may have updated it this tick). Returns
    /// `true` when it rebuilt.
    pub(crate) fn refresh_task_viewer(&mut self) -> bool {
        if let Some(viewer_id) = self.view.pager.as_ref().and_then(|v| v.task_id)
            && let Some(task) = self
                .runtime
                .background_tasks
                .tasks
                .iter_mut()
                .find(|t| t.id == viewer_id)
        {
            // Rebuild on new bytes OR on status transition (e.g. Running →
            // Exited while the user is looking at it) so the title and the
            // [EOF] marker keep up with reality. Drop has_unread_output even
            // on status-only refreshes so the divider `+` clears.
            let task_running = matches!(task.status, TaskStatus::Running);
            let viewer_streaming = self.view.pager.as_ref().is_some_and(|v| v.streaming);
            let status_changed = task_running != viewer_streaming;
            if task.has_unread_output || status_changed {
                task.has_unread_output = false;
                task.viewed_in_task_viewer = true;
                let new_view = Self::build_task_viewer_for(viewer_id, task);
                if let Some(view) = self.view.pager.as_mut() {
                    view.lines = new_view.lines;
                    view.title = new_view.title;
                    view.streaming = new_view.streaming;
                    // The task buffer front-trims at TASK_BUFFER_CAP, so the
                    // rebuilt line count can shrink — drop/clamp a stale
                    // visual selection or search before the next yank/render.
                    view.clamp_state_to_lines();
                }
                return true;
            }
        }
        false
    }

    /// MVU Phase 6 PR-C: the pre-recv pane-output scan. Drains every tab + the
    /// top overlay (Acquire-load on each worker's `parser_gen`), flips the
    /// background-tab `has_activity` divider glyph, and marks exited tabs.
    /// Returns `(needs_draw, draw_reason)` — `draw_reason` is 1 for pane output
    /// and 3 for a newly-exited tab (last-writer-wins, matching the inline
    /// order); the caller folds them into the loop's `needs_draw`/`draw_reason`.
    ///
    /// CAS-CRITICAL: `clear_wake()` is the SOLE clear site and stays
    /// immediately before `drain_output()` (clear-before-read) — it must NOT
    /// move into `drain_output`, which render's `drain_all` also calls every
    /// frame and would re-clear the edge mid-stream, defeating the worker's
    /// 0→1 wake-coalescing CAS.
    pub(crate) fn drain_pane_output(&mut self) -> (bool, u8) {
        let mut needs_draw = false;
        let mut draw_reason = 0u8;
        let mut pane_had_output = false;
        if let Some(tabs) = self.runtime.pane_tabs.as_mut() {
            let active_idx = tabs.active_index();
            // Stamp the moment of output for the agent-activity signal (P0).
            // `&mut` settle step, so reading the clock here is fine (render is
            // the pure half). The Working↔Idle derive is `settle_agent_activity`.
            let now = std::time::Instant::now();
            for (i, entry) in tabs.tabs_mut().iter_mut().enumerate() {
                // Clear the wake edge BEFORE the gen load (clear-before-read).
                // SOLE clear site — see the fn doc.
                entry.pane.clear_wake();
                if entry.pane.drain_output() {
                    // Output stamp for EVERY tab (active + background) — the
                    // activity dot tracks live output, not just unseen output.
                    entry.info.last_output_at = Some(now);
                    if crate::shell::command_is_shell(&entry.info.command) {
                        super::shell_agents::schedule_look(&mut entry.info, now);
                    }
                    // P1-2 scrape fallback: mark dirty so `settle_scrape_quiet`
                    // scans the screen after `SCRAPE_QUIET_WINDOW` of silence.
                    if !entry.info.agent().detection_rules().is_empty() {
                        entry.info.scrape_dirty = true;
                    }
                    if i == active_idx {
                        pane_had_output = true;
                    } else {
                        entry.info.has_activity = true;
                        needs_draw = true;
                        draw_reason = 1;
                    }
                }
            }
        }
        if let Some(overlay) = self.runtime.top_overlay.as_mut() {
            overlay.clear_wake(); // clear-before-read (see the tab scan)
            if overlay.drain_output() {
                pane_had_output = true;
            }
        }
        // The right-column overlay (`b`'s `V`/`D`) has its own slot and reader,
        // so it needs the same clear-wake + drain here — this is the SOLE
        // clear_wake site. Without it `b`'s editor wake edge never re-arms: after
        // its first output the reader stops pushing wakes, so the loop only
        // redraws on the next keystroke (laggy editor + stale frames).
        if let Some(overlay) = self.runtime.top_overlay_right.as_mut() {
            overlay.clear_wake();
            if overlay.drain_output() {
                pane_had_output = true;
            }
        }
        if pane_had_output {
            needs_draw = true;
            draw_reason = 1;
            // A held snapshot supplies a drag's displayed grid and copied bytes
            // while the parser continues consuming output. A released selection
            // returns to the live grid, so output retires its screen coordinates.
            if !matches!(
                self.view.mouse_selection,
                Some(super::mouse::MouseDragTarget::Pane)
            ) {
                self.clear_pane_selection();
            }
            // Echo-latency probe (A-monitor only): this active-pane output is
            // the agent's echo of the last forwarded keystroke. Measure
            // forward→echo so we can see the pane round-trip vs render cost.
            if self.view.show_activity
                && let Some(sent) = self.view.pane_send_at.take()
            {
                let us = u64::try_from(sent.elapsed().as_micros()).unwrap_or(u64::MAX);
                self.view.activity.peaks_live.echo_us =
                    self.view.activity.peaks_live.echo_us.max(us);
            }
        }
        // Mark exited tabs AFTER drain so the Closed event has been processed
        // and `is_closed()` returns true; the "[exited N]" label appears
        // immediately.
        if let Some(tabs) = self.runtime.pane_tabs.as_mut()
            && tabs.mark_exited()
        {
            needs_draw = true;
            draw_reason = 3;
        }
        (needs_draw, draw_reason)
    }
}

#[cfg(test)]
mod cap_buffer_tests {
    use super::{TASK_BUFFER_CAP, cap_stream_buffer};

    #[test]
    fn leaves_small_buffer_untouched() {
        let mut buf = vec![b'x'; 100];
        cap_stream_buffer(&mut buf);
        assert_eq!(buf.len(), 100);
    }

    #[test]
    fn front_trims_oversized_buffer_to_cap_keeping_the_tail() {
        // Oldest bytes (b'A') are dropped; the most-recent CAP bytes survive.
        let mut buf = vec![b'A'; TASK_BUFFER_CAP];
        buf.extend(std::iter::repeat_n(b'B', 4096));
        cap_stream_buffer(&mut buf);
        assert_eq!(buf.len(), TASK_BUFFER_CAP);
        // The retained window is the tail: all the new B's plus the most
        // recent A's, with the oldest 4096 A's dropped from the front.
        assert_eq!(buf[buf.len() - 4096..], [b'B'; 4096]);
        assert_eq!(buf[0], b'A');
    }

    #[test]
    fn exactly_cap_is_not_trimmed() {
        let mut buf = vec![b'z'; TASK_BUFFER_CAP];
        cap_stream_buffer(&mut buf);
        assert_eq!(buf.len(), TASK_BUFFER_CAP);
    }

    // The finished pager reads the FULL on-disk spill (head intact), not the
    // front-trimmed live buffer — the fix for `!git log` losing its newest
    // commits off the front. Falls back to the buffer when there's no spill,
    // and flags a spill that hit CAPTURE_SPILL_MAX.
    #[test]
    fn finished_capture_prefers_full_spill_over_capped_buffer() {
        use super::{CAPTURE_SPILL_MAX, finished_capture_lines};
        use crate::app::capture::CaptureSpill;

        let joined = |lines: &[ratatui::text::Line<'static>]| -> String {
            lines
                .iter()
                .map(|l| {
                    l.spans
                        .iter()
                        .map(|s| s.content.as_ref())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\n")
        };

        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"HEAD_LINE\nmiddle\nTAIL_LINE\n").unwrap();
        let spill = CaptureSpill {
            file: std::fs::File::open(tmp.path()).unwrap(),
            path: tmp.path().to_path_buf(),
            bytes: 27,
        };

        // Live buffer holds only the front-trimmed tail; the full spill wins.
        let (lines, capped) = finished_capture_lines(Some(&spill), b"TAIL_LINE\n");
        let text = joined(&lines);
        assert!(
            text.contains("HEAD_LINE"),
            "spill head must be present: {text:?}"
        );
        assert!(text.contains("TAIL_LINE"), "spill tail must be present");
        assert!(!capped, "27 bytes is well under CAPTURE_SPILL_MAX");

        // No spill → fall back to the capped buffer (only the tail).
        let (fallback, _) = finished_capture_lines(None, b"TAIL_LINE\n");
        let ftext = joined(&fallback);
        assert!(
            ftext.contains("TAIL_LINE") && !ftext.contains("HEAD_LINE"),
            "no spill → capped buffer only: {ftext:?}"
        );

        // A spill that reached the cap is flagged (→ truncation marker).
        let at_cap = CaptureSpill {
            file: std::fs::File::open(tmp.path()).unwrap(),
            path: tmp.path().to_path_buf(),
            bytes: CAPTURE_SPILL_MAX,
        };
        let (_, capped2) = finished_capture_lines(Some(&at_cap), b"");
        assert!(capped2, "spill at CAPTURE_SPILL_MAX is flagged");
    }
}

/// The `!` capture and background-task drains, driven the way the loop drives
/// them: a real child under `/bin/sh` (unit tests never get the user's shell),
/// drained until the rule under test is observable. Each test names what the
/// user sees, so none of them reads the drain's locals.
#[cfg(test)]
mod drain_tests {
    use std::time::{Duration, Instant};

    use super::{CAPTURE_SPILL_MAX, TASK_BUFFER_CAP};
    use crate::app::capture::CaptureSpill;
    use crate::app::{App, TaskStatus};

    const DEADLINE: Duration = Duration::from_secs(20);

    fn with_app(test: impl FnOnce(&mut App)) {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let dir = tmp.path().join("w");
            std::fs::create_dir(&dir).unwrap();
            test(&mut App::test_app(dir));
        });
    }

    fn pager_text(app: &App) -> Vec<String> {
        let view = app.view.pager.as_ref().expect("a pager is open");
        view.lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    /// The pager's text without blank lines, so a trailing newline's empty row
    /// doesn't decide a test about content.
    fn pager_rows(app: &App) -> Vec<String> {
        pager_text(app)
            .into_iter()
            .filter(|l| !l.is_empty())
            .collect()
    }

    /// Run `drain` until `done` holds, returning each drain's redraw answer.
    fn drain_until(
        app: &mut App,
        what: &str,
        mut drain: impl FnMut(&mut App) -> bool,
        done: impl Fn(&App) -> bool,
    ) -> Vec<bool> {
        let deadline = Instant::now() + DEADLINE;
        let mut answers = Vec::new();
        while !done(app) {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            answers.push(drain(app));
            std::thread::sleep(Duration::from_millis(10));
        }
        answers
    }

    fn drain_capture_until(app: &mut App, what: &str, done: impl Fn(&App) -> bool) -> Vec<bool> {
        drain_until(app, what, App::drain_pending_capture, done)
    }

    fn drain_capture_to_exit(app: &mut App) -> Vec<bool> {
        drain_capture_until(app, "the capture to finish", |a| {
            a.runtime.pending_capture.is_none()
        })
    }

    fn drain_tasks_until(app: &mut App, what: &str, done: impl Fn(&App) -> bool) -> Vec<bool> {
        drain_until(app, what, App::drain_background_tasks, done)
    }

    /// `{prefix}{n}` on a line of its own for each `n` in `range`.
    fn numbered_lines(prefix: &str, range: std::ops::RangeInclusive<u32>) -> String {
        use std::fmt::Write;
        range.fold(String::new(), |mut text, n| {
            writeln!(text, "{prefix}{n}").expect("writing to a String cannot fail");
            text
        })
    }

    fn write_to_capture(app: &mut App, bytes: &[u8]) {
        let capture = app
            .runtime
            .pending_capture
            .as_mut()
            .expect("a live capture");
        capture.host.write_all(bytes).unwrap();
    }

    fn write_to_task(app: &mut App, bytes: &[u8]) {
        app.runtime.background_tasks.tasks[0]
            .host
            .write_all(bytes)
            .unwrap();
    }

    #[test]
    fn output_reaches_the_pager_while_the_command_is_still_running() {
        with_app(|app| {
            app.start_capture("cat", "cat", "cat");
            write_to_capture(app, b"hello\n");
            let answers = drain_capture_until(app, "the output", |a| {
                pager_rows(a).iter().any(|l| l == "hello")
            });
            assert_eq!(answers.last(), Some(&true), "new output must draw");
            assert!(app.runtime.pending_capture.is_some(), "still running");
            let view = app.view.pager.as_ref().unwrap();
            assert!(view.streaming, "a running capture is still streaming");
            assert!(view.title.starts_with('\u{23f3}'), "{}", view.title);
        });
    }

    /// A capture with nothing new to show must not draw every loop iteration:
    /// it draws when the elapsed time in its title changes, and only then.
    #[test]
    fn a_quiet_capture_draws_only_when_its_clock_changes() {
        with_app(|app| {
            app.start_capture("cat", "cat", "cat");
            app.runtime.pending_capture.as_mut().unwrap().started = Instant::now();
            assert!(!app.drain_pending_capture(), "nothing new to show");
            assert!(!app.drain_pending_capture(), "still nothing new to show");

            let five_seconds_ago = Instant::now()
                .checked_sub(Duration::from_secs(5))
                .expect("the clock is past five seconds");
            app.runtime.pending_capture.as_mut().unwrap().started = five_seconds_ago;
            assert!(app.drain_pending_capture(), "the clock moved on");
            let title = &app.view.pager.as_ref().unwrap().title;
            assert!(title.ends_with("(5s)"), "{title}");
            assert!(!app.drain_pending_capture(), "the same second again");
        });
    }

    #[test]
    fn a_clean_exit_closes_the_pager_with_a_tick_and_an_eof_line() {
        with_app(|app| {
            app.start_capture("printf 'one\\ntwo\\n'", "printf", "printf");
            let answers = drain_capture_to_exit(app);
            assert_eq!(answers.last(), Some(&true), "the exit frame must draw");
            let view = app.view.pager.as_ref().unwrap();
            assert_eq!(view.title, "\u{2713} ! printf — exit 0");
            assert!(!view.streaming, "a finished capture stops streaming");
            assert!(view.saveable, "a finished capture can be saved");
            assert_eq!(pager_rows(app), ["one", "two", "[EOF — exit 0]"]);
        });
    }

    /// The exit draws even when it brings no output of its own: here the
    /// output has already been shown when the command exits.
    #[test]
    fn a_failing_exit_closes_the_pager_with_a_cross_and_its_code() {
        with_app(|app| {
            app.start_capture("printf 'x\\n'; sleep 0.2; exit 3", "fails", "fails");
            drain_capture_until(app, "the output", |a| {
                pager_rows(a).iter().any(|l| l == "x")
            });
            let answers = drain_capture_to_exit(app);
            assert_eq!(answers.last(), Some(&true), "the exit frame must draw");
            let view = app.view.pager.as_ref().unwrap();
            assert_eq!(view.title, "\u{2717} ! fails — exit 3");
            assert_eq!(pager_rows(app), ["x", "[EOF — exit 3]"]);
        });
    }

    /// Output past the live buffer's cap loses its start from that buffer, but
    /// not from the finished pager: a `!git log` opens at its newest commit.
    #[test]
    fn a_capture_past_the_buffer_cap_still_opens_at_its_first_line() {
        with_app(|app| {
            // ~1.3 MB, past the 1 MiB cap. `cat` of a file writes in large
            // blocks; `seq` on a tty flushes per line, and 8-byte chunks take
            // hundreds of drains to cross a 512-chunk reader channel.
            let numbers = numbered_lines("", 1..=180_000);
            let dir = app.state.cur().listing.dir.clone();
            std::fs::write(dir.join("numbers"), numbers).unwrap();
            app.start_capture("cat numbers", "cat numbers", "cat numbers");
            let deadline = Instant::now() + DEADLINE;
            while app.runtime.pending_capture.is_some() {
                assert!(Instant::now() < deadline, "the capture never finished");
                app.drain_pending_capture();
                if let Some(capture) = app.runtime.pending_capture.as_ref() {
                    assert!(
                        capture.buffer.len() <= TASK_BUFFER_CAP,
                        "the live buffer outgrew its cap: {}",
                        capture.buffer.len()
                    );
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            let rows = pager_rows(app);
            assert_eq!(rows.first().map(String::as_str), Some("1"), "the head");
            assert_eq!(rows.len(), 180_001, "every line, plus the EOF line");
            assert_eq!(rows[179_999], "180000", "the tail");
        });
    }

    /// When the full copy on disk can't be written, the capture carries on and
    /// the pager shows the live buffer instead.
    #[test]
    fn an_unwritable_spill_falls_back_to_the_live_buffer() {
        with_app(|app| {
            app.start_capture("printf 'kept\\n'", "kept", "kept");
            let capture = app.runtime.pending_capture.as_mut().unwrap();
            let path = capture
                .full_log
                .as_ref()
                .expect("a spill file")
                .path
                .clone();
            // Read-only, so the drain's write fails.
            capture.full_log = Some(CaptureSpill {
                file: std::fs::File::open(&path).unwrap(),
                path,
                bytes: 0,
            });
            drain_capture_to_exit(app);
            assert_eq!(pager_rows(app), ["kept", "[EOF — exit 0]"]);
        });
    }

    /// The on-disk copy stops at its backstop, and the finished pager says the
    /// tail was cut rather than ending silently.
    #[test]
    fn a_spill_at_its_backstop_stops_growing_and_says_so() {
        with_app(|app| {
            app.start_capture("printf 'TAIL\\n'", "big", "big");
            let capture = app.runtime.pending_capture.as_mut().unwrap();
            let spill = capture.full_log.as_mut().expect("a spill file");
            std::io::Write::write_all(&mut spill.file, b"HEAD\n").unwrap();
            spill.bytes = CAPTURE_SPILL_MAX;
            drain_capture_to_exit(app);
            assert_eq!(
                pager_rows(app),
                [
                    "HEAD",
                    "[EOF — capture exceeded 64 MiB — tail omitted]",
                    "[EOF — exit 0]",
                ]
            );
        });
    }

    /// A reader at the bottom follows new output; a reader who scrolled up to
    /// read is left where they are.
    #[test]
    fn the_pager_follows_output_only_for_a_reader_at_the_bottom() {
        with_app(|app| {
            app.start_capture("cat", "cat", "cat");
            app.view.pager.as_ref().unwrap().last_viewport_h.set(10);
            let lines = numbered_lines("line ", 1..=30);
            write_to_capture(app, lines.as_bytes());
            drain_capture_until(app, "thirty lines", |a| {
                pager_rows(a).iter().filter(|l| *l == "line 30").count() == 2
            });
            let view = app.view.pager.as_ref().unwrap();
            assert!(
                view.scroll + view.page_lines(10) >= view.line_count(),
                "the newest line is on screen: scroll {} of {} lines",
                view.scroll,
                view.line_count()
            );

            app.view.pager.as_mut().unwrap().scroll = 0;
            write_to_capture(app, b"more\n");
            drain_capture_until(app, "more output", |a| {
                pager_rows(a).iter().any(|l| l == "more")
            });
            assert_eq!(
                app.view.pager.as_ref().unwrap().scroll,
                0,
                "a reader who scrolled up stays put"
            );
        });
    }

    /// After `^Z` the capture's output belongs to its task: the pager drain
    /// leaves it alone, the task's `[N+]` divider appears once (not on every
    /// chunk), and its exit is reported with its code.
    #[test]
    fn a_backgrounded_capture_flags_new_output_once_and_reports_its_exit() {
        with_app(|app| {
            app.start_capture("cat; exit 3", "cat", "cat");
            app.background_capture();
            assert!(!app.drain_pending_capture(), "no capture is live");

            let task_has = |a: &App, text: &str| {
                String::from_utf8_lossy(&a.runtime.background_tasks.tasks[0].buffer).contains(text)
            };
            write_to_task(app, b"first\n");
            let answers = drain_tasks_until(app, "the first output", |a| task_has(a, "first"));
            assert_eq!(answers.last(), Some(&true), "the [N+] divider appears");
            assert!(app.runtime.background_tasks.tasks[0].has_unread_output);

            write_to_task(app, b"second\n");
            let answers = drain_tasks_until(app, "the second output", |a| task_has(a, "second"));
            assert!(
                answers.iter().all(|drew| !drew),
                "more output doesn't change the divider: {answers:?}"
            );

            write_to_task(app, b"\x04"); // ^D ends `cat`; the shell exits 3
            let answers = drain_tasks_until(app, "the task to exit", |a| {
                !matches!(
                    a.runtime.background_tasks.tasks[0].status,
                    TaskStatus::Running
                )
            });
            assert_eq!(answers.last(), Some(&true), "the exit must draw");
            assert!(matches!(
                app.runtime.background_tasks.tasks[0].status,
                TaskStatus::Exited(3)
            ));
            let flash = app.flash_text().unwrap_or_default();
            assert!(flash.starts_with("task #1: cat — exit 3 ("), "{flash}");
        });
    }

    #[test]
    fn a_background_task_that_succeeds_is_marked_exited_zero() {
        with_app(|app| {
            app.start_capture("true", "true", "true");
            app.background_capture();
            drain_tasks_until(app, "the task to exit", |a| {
                !matches!(
                    a.runtime.background_tasks.tasks[0].status,
                    TaskStatus::Running
                )
            });
            assert!(matches!(
                app.runtime.background_tasks.tasks[0].status,
                TaskStatus::Exited(0)
            ));
            let flash = app.flash_text().unwrap_or_default();
            assert!(flash.starts_with("task #1: true — exit 0 ("), "{flash}");
        });
    }
}
