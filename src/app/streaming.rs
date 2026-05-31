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

use super::{App, TASK_BUFFER_CAP, TaskStatus, eof_marker_line, strip_crlf};

impl App {
    /// Drain the foreground `!` capture into its pager and finalize on EOF.
    /// Returns `true` while a capture exists (PR2 preserves the pre-3c
    /// unconditional redraw-while-streaming; PR3 gates it on actual output).
    pub(crate) fn drain_pending_capture(&mut self) -> bool {
        let had_capture = self.pending_capture.is_some();
        if let Some(capture) = &mut self.pending_capture {
            // MVU Phase 3c: clear-before-read (no-op until the slot is set).
            capture.host.clear_wake_pending();
            let mut got_data = false;
            let mut chunks: Vec<Vec<u8>> = Vec::new();
            let drain = capture.host.drain(|bytes| chunks.push(bytes.to_vec()));
            for chunk in chunks {
                capture.buffer.extend_from_slice(&chunk);
                got_data = true;
            }
            if drain.newly_closed {
                capture.finished = true;
            }
            // Update elapsed timer in the title while running.
            if !capture.finished {
                let elapsed = capture.started.elapsed().as_secs();
                let human_time = if elapsed >= 3600 {
                    format!(
                        "{}h {}m {}s",
                        elapsed / 3600,
                        (elapsed % 3600) / 60,
                        elapsed % 60
                    )
                } else if elapsed >= 60 {
                    format!("{}m {}s", elapsed / 60, elapsed % 60)
                } else {
                    format!("{elapsed}s")
                };
                let timer_title = format!("\u{23f3} {} — running... ({human_time})", capture.title);
                if let Some(view) = self.pager.as_mut() {
                    view.title = timer_title;
                }
            }
            if got_data || capture.finished {
                // Rebuild pager content from the accumulated buffer.
                use ansi_to_tui::IntoText;
                let normalized = strip_crlf(&capture.buffer);
                let text = normalized.as_slice().into_text().unwrap_or_default();
                // "At bottom" detection uses the actual rendered viewport
                // height -- before this we hardcoded 40, which under-shoots
                // on tall terminals and made the streaming-capture auto-tail
                // leave the top half of the pager showing content with `~`
                // markers filling the rest until the user manually scrolled.
                // last_viewport_h is set by the renderer on every frame.
                let at_bottom = self.pager.as_ref().is_some_and(|v| {
                    let h = v.last_viewport_h.get();
                    let h = if h == 0 { 40 } else { h };
                    let total = v.line_count();
                    let page = v.page_lines(h);
                    v.scroll >= total.saturating_sub(page)
                });
                if let Some(view) = self.pager.as_mut() {
                    view.lines = text.lines;
                    if at_bottom {
                        view.scroll_to_bottom_auto();
                    }
                }
            }
            if capture.finished {
                // Reader thread already saw EOF; capture.host may have already
                // harvested exit_status during drain. If not (race window),
                // wait() now — safe because the child has exited.
                let (exit_info, ok) = if let Some(s) = capture.host.exit_status.as_ref() {
                    if s.success() {
                        ("exit 0".to_string(), true)
                    } else {
                        (format!("exit {}", s.exit_code()), false)
                    }
                } else {
                    match capture.host.child.wait() {
                        Ok(s) if s.success() => ("exit 0".to_string(), true),
                        Ok(s) => (format!("exit {}", s.exit_code()), false),
                        Err(e) => (format!("error: {e}"), false),
                    }
                };
                // Status glyph mirrors the bottom-status-bar conventions
                // (✓ for exit 0, ✗ for everything else) so the pager title
                // tells the user at a glance whether their command succeeded.
                let glyph = if ok { "\u{2713}" } else { "\u{2717}" }; // ✓ / ✗
                let title = format!("{glyph} {} — {exit_info}", capture.title);
                // Final rebuild with stderr included.
                use ansi_to_tui::IntoText;
                let normalized = strip_crlf(&capture.buffer);
                let text = normalized.as_slice().into_text().unwrap_or_default();
                if let Some(view) = self.pager.as_mut() {
                    view.title = title;
                    view.lines = text.lines;
                    // Anchor an EOF marker to the bottom of content so it's
                    // visible even when output exceeds viewport_h.
                    view.lines.push(eof_marker_line(&exit_info));
                    view.eof_in_content = true;
                    view.saveable = true;
                    view.streaming = false;
                    view.scroll_to_bottom_auto();
                }
                self.pending_capture = None;
            }
        }
        had_capture
    }

    /// Drain every running background task into its buffer and harvest exit
    /// on EOF. Returns `true` when a task finished this tick (the divider /
    /// toast need a redraw).
    pub(crate) fn drain_background_tasks(&mut self) -> bool {
        let mut just_finished: Vec<(u32, String, String, std::time::Duration)> = Vec::new();
        for task in &mut self.background_tasks.tasks {
            if !matches!(task.status, TaskStatus::Running) {
                continue;
            }
            // MVU Phase 3c: clear-before-read (no-op until the slot is set).
            task.host.clear_wake_pending();
            let mut chunks: Vec<Vec<u8>> = Vec::new();
            let drain = task.host.drain(|bytes| chunks.push(bytes.to_vec()));
            for chunk in chunks {
                task.buffer.extend_from_slice(&chunk);
                task.has_unread_output = true;
                if task.buffer.len() > TASK_BUFFER_CAP {
                    let drop_n = task.buffer.len() - TASK_BUFFER_CAP;
                    task.buffer.drain(..drop_n);
                }
            }
            if drain.newly_closed {
                // Reader thread observed EOF this tick. Host's drain already
                // tried try_wait — re-attempt here in case it raced, then
                // build the status_text + TaskStatus.
                let exit = task
                    .host
                    .exit_status
                    .take()
                    .map_or_else(|| task.host.child.wait(), Ok);
                let (status_text, status_val) = match exit {
                    Ok(s) if s.success() => ("exit 0".to_string(), TaskStatus::Exited(0)),
                    #[allow(clippy::cast_possible_wrap)]
                    Ok(s) => {
                        let code = s.exit_code() as i32;
                        (format!("exit {code}"), TaskStatus::Exited(code))
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        (format!("error: {msg}"), TaskStatus::Crashed(msg))
                    }
                };
                task.status = status_val;
                task.finished_at = Some(std::time::Instant::now());
                just_finished.push((
                    task.id,
                    task.cmd_display.clone(),
                    status_text,
                    task.started.elapsed(),
                ));
            }
        }
        if just_finished.is_empty() {
            return false;
        }
        for (id, cmd_display, status_text, elapsed) in just_finished {
            let secs = elapsed.as_secs();
            self.state.flash_info(format!(
                "task #{id}: {cmd_display} — {status_text} ({secs}s)"
            ));
        }
        true
    }

    /// If a task-viewer pager is open, refresh its content from the live task
    /// buffer (the bg drain above may have updated it this tick). Returns
    /// `true` when it rebuilt.
    pub(crate) fn refresh_task_viewer(&mut self) -> bool {
        if let Some(viewer_id) = self.pager.as_ref().and_then(|v| v.task_id)
            && let Some(task) = self
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
            let viewer_streaming = self.pager.as_ref().is_some_and(|v| v.streaming);
            let status_changed = task_running != viewer_streaming;
            if task.has_unread_output || status_changed {
                task.has_unread_output = false;
                task.viewed_in_task_viewer = true;
                let new_view = Self::build_task_viewer_for(viewer_id, task);
                if let Some(view) = self.pager.as_mut() {
                    view.lines = new_view.lines;
                    view.title = new_view.title;
                    view.streaming = new_view.streaming;
                }
                return true;
            }
        }
        false
    }
}
