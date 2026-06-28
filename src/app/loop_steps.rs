//! MVU Phase 6 PR-C: small, self-contained steps lifted out of the `App::run`
//! event-loop body, in the established descendant-module `impl App` pattern
//! (like `streaming.rs` / `sources.rs` / `agent_status.rs`). Each is a verbatim
//! move of a contiguous loop slice — behavior-equivalent; the loop still calls
//! them in the same order, so the loop's ordering invariants are unchanged.
//! Methods that drive a redraw return a `bool`/tuple the caller folds into
//! `needs_draw` / `draw_reason` (kept loop-side so the draw-reason precedence
//! stays visible at the call sites).

use std::time::{Duration, Instant};

use super::{
    App, ChordHint, Deadline, Mode, Prompt, PromptKind, RunCtx, arm_resume_deadlines, state,
};

impl App {
    /// One-shot full-repaint bookkeeping at the top of each iteration.
    /// Returns `(needs_draw, pending_clear)`. Force a redraw when the pager
    /// opens over a live pane (the pane stops rendering and its stale cells
    /// need clearing — but via a forced draw, not `terminal.clear()`, which
    /// would flash). A pending `needs_full_repaint` additionally requests a
    /// screen clear (`pending_clear`) consumed by the render block.
    pub(crate) const fn step_pager_repaint(&mut self) -> (bool, bool) {
        // Force a draw when the pager opens over a live pane (the pane stops
        // rendering, so its stale cells need clearing — via a forced draw, not
        // `terminal.clear()`, which would flash).
        let pager_just_opened = self.view.pager.is_some()
            && self.runtime.pane_tabs.is_some()
            && !self.view.pager_was_open;
        self.view.pager_was_open = self.view.pager.is_some();
        // A pending full repaint also requests a screen clear.
        let pending_clear = self.view.needs_full_repaint;
        self.view.needs_full_repaint = false;
        (pager_just_opened || pending_clear, pending_clear)
    }

    /// MVU Phase 5: snapshot the active pane's routing flags into the Model,
    /// AFTER the drain + `mark_exited` finalized `is_closed` and BEFORE `recv`.
    /// `route_snapshot` reads this instead of the live host, decoupling key
    /// routing from the Runtime. Behavior-equivalent: the only mutators of
    /// these flags (scroll-mode key handlers / child-exit drain) run either
    /// before this point or strictly after `route_snapshot`, so the value
    /// matches what the old live read observed.
    pub(crate) fn snapshot_pane_routing(&mut self) {
        self.state.pane.pane_snapshot =
            self.runtime
                .pane_tabs
                .as_ref()
                .map_or_else(state::PaneSnapshot::default, |t| state::PaneSnapshot {
                    is_scrolling: t.active().is_scrolling(),
                    is_closed: t.active().is_closed(),
                });
        // Re-derive `state.focus` from the live surfaces here, at the same
        // loop-top seam and for the same reason: so the value `route_snapshot`
        // reads next is always current. Most pager opens never set focus and
        // closes leave it stale; this makes `state.focus` authoritative without
        // bookkeeping at every open/close site. Behavior-preserving today (only
        // the non-`Pane` discriminant moves; `pane_focused()` is unchanged).
        self.recompute_focus();
    }

    /// 1 Hz safety-net git poll — converges within a second when FSEvents
    /// misses an atomic-rename `.git/index` replace. Diff-aware
    /// (`refresh_git_state` only repaints on a real change, so idle dps stays
    /// 0). Also arms/disarms the advisory `GitPoll` deadline. Returns whether a
    /// redraw is needed.
    pub(crate) fn poll_git_cadence(&mut self, now_pre: Instant, ctx: &mut RunCtx) -> bool {
        let mut needs_draw = false;
        let git_poll_interval = Duration::from_secs(1);
        if self.state.any_git_repo()
            && now_pre.duration_since(ctx.last_git_poll) >= git_poll_interval
        {
            ctx.last_git_poll = now_pre;
            if self.state.refresh_git_state() {
                needs_draw = true;
            }
        }
        // Arm/disarm GitPoll to reflect git_info presence (advisory — the
        // predicate above fires it against now_pre).
        if self.state.any_git_repo() {
            ctx.scheduler
                .arm(Deadline::GitPoll, ctx.last_git_poll + git_poll_interval);
        } else {
            ctx.scheduler.disarm(Deadline::GitPoll);
        }
        needs_draw
    }

    /// Execute writable MCP commands the recv arm buffered into `mcp_pending`,
    /// replying on each one-shot Sender. Kept at its early loop position so a
    /// queued request never sits behind a TUI-tearing ForegroundExec and
    /// breaches the client's 5 s timeout. `execute_mcp_command` writes the
    /// context file synchronously, then `reply.send` — preserving
    /// single-connection read-after-write. Returns whether a redraw is needed.
    pub(crate) fn drain_mcp_pending(&mut self, ctx: &mut RunCtx) -> bool {
        let mut needs_draw = false;
        for req in std::mem::take(&mut ctx.mcp_pending) {
            // `mcp_reqs` is bumped per-tool in the `ToolCalled` arm (one is sent
            // for every tools/call), so it's not counted here — that would
            // double-count writable commands, which arrive as their own command
            // *in addition to* a `ToolCalled`.
            let crate::mcp_cmd::McpRequest { command, reply } = req;
            // Heavy worktree ops (create/remove/clean) run off the loop: validate
            // synchronously (cheap, reads App state incl. the occupied guard),
            // then hand the gix/copy IO to a worker that replies once the main
            // loop has re-applied refresh+context (`apply_worktree_outcomes`).
            // Everything else is served synchronously, replying inline.
            if let Some(planned) = self.plan_worktree_job(&command) {
                match planned {
                    Ok(job) => self.spawn_worktree_job(
                        job,
                        super::worktree_ops::WorktreeCompletion::Mcp(reply),
                    ),
                    Err(resp) => {
                        let _ = reply.send(resp);
                    }
                }
            } else {
                let resp = self.execute_mcp_command(command);
                let _ = reply.send(resp);
            }
            needs_draw = true;
        }
        needs_draw
    }

    /// Drain the git-worker results the recv arm buffered into `git_pending`.
    /// Stale results (the user navigated past them) are discarded via the
    /// generation counter inside `ingest_git_result`; matching results refill
    /// the raw-status cache and recompute git_files / git_info against the
    /// *current* listing dir. SOLE apply/count/take site. Sets `context_dirty`
    /// when a result lands (git_branch / dirty flag may have changed). Returns
    /// whether a redraw is needed (the caller uses `draw_reason = 2`).
    pub(crate) fn drain_git_pending(&mut self, ctx: &mut RunCtx) -> bool {
        let mut needs_draw = false;
        for result in std::mem::take(&mut ctx.git_pending) {
            if self.ingest_git_result(result) {
                needs_draw = true;
                self.view.context_dirty = true;
            }
        }
        needs_draw
    }

    /// Activity monitor: roll over the 1-second window — snapshot the per-second
    /// counters, reset accumulators, refresh proc stats. Returns
    /// `activity_only_draw`: true when a counter changed and the frame wasn't
    /// already dirty, so the overlay refresh draws without being counted in the
    /// stats (which would oscillate). The caller folds it into `needs_draw`.
    pub(crate) fn roll_activity_window(&mut self, now_post: Instant, needs_draw: bool) -> bool {
        if !self.view.show_activity
            || now_post.duration_since(self.view.activity.last_tick) < Duration::from_secs(1)
        {
            return false;
        }
        // Snapshot the live counters/peaks and reset the accumulators. `roll`
        // reports whether any *counter* changed; if so and the frame wasn't
        // already dirty, draw once to refresh the overlay — but don't count
        // that draw in the stats (it would oscillate).
        let changed = self.view.activity.roll(now_post);
        let activity_only_draw = changed && !needs_draw;
        // Refresh process stats (RSS / thread count) on the same 1 s cadence.
        // Hidden inside the A-monitor tick so callers without it open pay zero
        // cost.
        self.refresh_process_stats();
        activity_only_draw
    }

    /// Re-arm the post-recv advisory deadlines. ActivityRollover (when the
    /// A-monitor is shown — it's a status-corner overlay, so with no pane the
    /// floor is None and this deadline is the only thing waking the rollover)
    /// and CaptureTick (the ~1 Hz elapsed-timer tick for a streaming capture /
    /// `:task` viewer).
    pub(crate) fn arm_post_recv_deadlines(&self, now_post: Instant, ctx: &mut RunCtx) {
        if self.view.show_activity {
            ctx.scheduler.arm(
                Deadline::ActivityRollover,
                self.view.activity.last_tick + Duration::from_secs(1),
            );
        } else {
            ctx.scheduler.disarm(Deadline::ActivityRollover);
        }
        if self.capture_tick_should_arm() {
            ctx.scheduler
                .arm(Deadline::CaptureTick, now_post + Duration::from_secs(1));
        } else {
            ctx.scheduler.disarm(Deadline::CaptureTick);
        }
    }

    /// Settle the which-key chord-hint popup. When the hint delay has elapsed
    /// (`chord_hint_due` reached) and a chord is *still* pending, build the
    /// popup from the resolver's continuations and dirty the frame; then
    /// arm/disarm the `ChordHint` wake from `chord_hint_due` so the loop sleeps
    /// exactly until the popup is due (and not at all once it has shown or the
    /// chord resolved). POST-recv, alongside the other advisory deadlines.
    pub(crate) fn settle_chord_hint(&mut self, now_post: Instant, ctx: &mut RunCtx) {
        if let Some(due) = self.view.chord_hint_due
            && now_post >= due
        {
            self.view.chord_hint_due = None;
            if self.state.resolver.is_pending() {
                let title = self
                    .state
                    .resolver
                    .pending_display()
                    .map(|s| s.trim_end_matches('-').to_string())
                    .unwrap_or_default();
                let rows: Vec<(&'static str, &'static str)> = self
                    .state
                    .resolver
                    .continuations()
                    .into_iter()
                    .map(|e| match e {
                        crate::keymap::ChordEntry::Act(keys, action) => (keys, action.describe()),
                        crate::keymap::ChordEntry::Sub(keys, label) => (keys, label),
                    })
                    .collect();
                if !rows.is_empty() {
                    self.view.chord_hint = Some(ChordHint { title, rows });
                    ctx.draw.mark(3);
                }
            }
        }
        match self.view.chord_hint_due {
            Some(due) => ctx.scheduler.arm(Deadline::ChordHint, due),
            None => ctx.scheduler.disarm(Deadline::ChordHint),
        }
    }

    /// MVU Phase 6 PR-C: pre-recv session-restore handling. Send any deferred
    /// `/resume <sid>` for restored tabs whose banner has settled, arm the
    /// RestoreSettle/ResumeEnter deadlines, and — if a restored claude tab
    /// looks crashed (bad exit / crash dump) while in Normal mode — open the
    /// crash-recovery prompt. Returns whether a redraw is needed (the prompt).
    pub(crate) fn handle_restore_resumes(&mut self, now_pre: Instant, ctx: &mut RunCtx) -> bool {
        self.send_pending_resumes(now_pre);
        // Arm RestoreSettle/ResumeEnter at the earliest pending resume across
        // all tabs so the wait can wake for it.
        arm_resume_deadlines(&mut ctx.scheduler, self.runtime.pane_tabs.as_ref());

        let crash_idx = self.find_crashed_restore_tab(now_pre);
        if let Some(tab_idx) = crash_idx
            && matches!(self.state.mode, Mode::Normal)
        {
            if let Some(tabs) = self.runtime.pane_tabs.as_mut()
                && let Some(entry) = tabs.tabs_mut().get_mut(tab_idx)
            {
                entry.info.restore_fallback = None;
            }
            self.state.mode = Mode::Prompting(Prompt::simple(
                PromptKind::ClaudeCrashRecover { tab_idx },
                "claude crash detected — start fresh and recover with /resume? [Y/n] ",
            ));
            return true;
        }
        false
    }

    /// MVU Phase 6 PR-C: post-recv MCP context-file write. Event-driven via
    /// `context_dirty`, throttled by a 150 ms min-interval and SUPPRESSED
    /// during the 300 ms typing-burst window (so claude's input echo isn't
    /// yanked by an mtime change mid-keystroke). Also arms/disarms the advisory
    /// `ContextWrite` deadline at the predicate edge. No redraw (the write is
    /// invisible).
    pub(crate) fn maybe_write_context(&mut self, now_post: Instant, ctx: &mut RunCtx) {
        let typing_burst = ctx
            .last_input_at
            .is_some_and(|t| now_post.duration_since(t) < Duration::from_millis(300));
        if self.view.context_dirty
            && !typing_burst
            && now_post.duration_since(ctx.last_context_write) >= Duration::from_millis(150)
        {
            self.write_context();
            ctx.last_context_write = now_post;
            self.view.context_dirty = false;
            ctx.scheduler.disarm(Deadline::ContextWrite);
        }
        // Arm ContextWrite at the predicate edge (the later of the 150 ms
        // min-interval and the 300 ms typing-burst suppressor) while dirty;
        // disarm once written.
        if self.view.context_dirty {
            let edge = (ctx.last_context_write + Duration::from_millis(150)).max(
                ctx.last_input_at
                    .map_or(ctx.last_context_write, |t| t + Duration::from_millis(300)),
            );
            ctx.scheduler.arm(Deadline::ContextWrite, edge);
        } else {
            ctx.scheduler.disarm(Deadline::ContextWrite);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Monitor off → the rollover is a no-op (no overlay-only draw, counters
    /// untouched) even when the window has elapsed.
    #[test]
    fn roll_activity_window_off_is_noop() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            app.view.show_activity = false;
            app.view.activity.live.draws = 7;
            app.view.activity.last_tick =
                Instant::now().checked_sub(Duration::from_secs(5)).unwrap();
            let only = app.roll_activity_window(Instant::now(), false);
            assert!(!only);
            assert_eq!(
                app.view.activity.live.draws, 7,
                "monitor off → counters untouched"
            );
        });
    }

    /// Monitor on, window elapsed, a counter changed, frame not already dirty
    /// → signals an overlay-only draw and resets the accumulators.
    #[test]
    fn roll_activity_window_signals_and_resets_on_change() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            app.view.show_activity = true;
            app.view.activity.live.draws = 5; // differs from snap (0) → changed
            app.view.activity.last_tick =
                Instant::now().checked_sub(Duration::from_secs(2)).unwrap();
            let only = app.roll_activity_window(Instant::now(), false);
            assert!(
                only,
                "changed counter + not already dirty → overlay-only draw"
            );
            assert_eq!(app.view.activity.live.draws, 0, "accumulators reset");
            assert_eq!(
                app.view.activity.snap.draws, 5,
                "snapshot captured the window value"
            );
        });
    }

    /// Frame already dirty → no extra overlay-only draw (avoids oscillation),
    /// but the rollover still snapshots + resets.
    #[test]
    fn roll_activity_window_no_signal_when_already_dirty() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            app.view.show_activity = true;
            app.view.activity.live.draws = 5;
            app.view.activity.last_tick =
                Instant::now().checked_sub(Duration::from_secs(2)).unwrap();
            let only = app.roll_activity_window(Instant::now(), true);
            assert!(!only);
            assert_eq!(
                app.view.activity.live.draws, 0,
                "rollover resets even when already dirty"
            );
        });
    }

    /// No git repo → no poll (no redraw) and the GitPoll deadline is disarmed
    /// (nothing else armed, so the scheduler has no next deadline).
    #[test]
    fn poll_git_cadence_disarms_without_git() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            assert!(
                app.state.left.git.info.is_none(),
                "tmpdir is not a git repo"
            );
            let mut ctx = RunCtx::for_test();
            let needs = app.poll_git_cadence(Instant::now(), &mut ctx);
            assert!(!needs, "no git → no redraw");
            assert!(ctx.scheduler.next().is_none(), "no git → GitPoll disarmed");
        });
    }
}
