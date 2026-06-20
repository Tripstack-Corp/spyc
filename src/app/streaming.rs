//! MVU Phase 3c: the streaming pull-source drains (foreground `!` capture +
//! background tasks + the task-viewer refresh), extracted from the `App::run`
//! loop body. These are `impl App` methods living in a child module, reading
//! App's private state directly via the descendant-module rule.
//!
//! Each returns whether it produced a redraw-worthy change this tick; the
//! run loop sets `needs_draw`/`draw_reason` from the return. Behavior is
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
                // Final rebuild with stderr included.
                let lines = buffer_to_lines(&capture.buffer);
                if let Some(view) = self.view.pager.as_mut() {
                    view.title = title;
                    view.lines = lines;
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
            for (i, entry) in tabs.tabs_mut().iter_mut().enumerate() {
                // Clear the wake edge BEFORE the gen load (clear-before-read).
                // SOLE clear site — see the fn doc.
                entry.pane.clear_wake();
                if entry.pane.drain_output() {
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
}
