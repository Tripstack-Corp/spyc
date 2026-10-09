//! The two phases `App::run` goes through on every iteration before it
//! blocks on `recv`, in order: land what the workers finished, then run the
//! timer-driven settles and reconciles. Each body is the loop slice it
//! replaced, so the ordering the comments describe still holds, and each step
//! still marks its own redraw reason where it runs.

use crate::Tui;

use super::{App, ForegroundExec, RunCtx};

impl App {
    /// Land every off-thread result that arrived since the last iteration:
    /// streaming sources, worker outcomes, pager streams, pane output. Each
    /// slot is drained regardless of which wake survived coalescing.
    pub(super) fn drain_landed_work(
        &mut self,
        ctx: &mut RunCtx,
        terminal: &mut Tui,
        foreground_exec: &ForegroundExec,
    ) {
        // MVU Phase 3c: drain the streaming pull sources (extracted to
        // streaming.rs). Each returns whether it needs a redraw. These
        // wake the channel themselves now (`SinkOutput`), so the poll
        // floor that once backstopped them is gone (see below).
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
        // landed (it woke us via `Message::Wake(Wake::AgentStatus)`). Done HERE,
        // in the always-run scan — not in `active_agent_status` — because
        // the status bar (and thus `active_agent_status`) is skipped on the
        // overlay / top-pager render paths; draining only there would leave
        // the slot full and this nudge would busy-spin. Applying the result
        // updates the cache so the next render shows the short-id.
        if self.apply_landed_agent_status() {
            ctx.draw.mark(3);
        }
        // Kick the off-thread refresh HERE (the &mut settle point), not from
        // the &self draw pass — the resolver spawns a worker that walks
        // `~/.claude/sessions`, which render must never do (render-purity
        // contract). No-ops fast when the cache is fresh or a walk is
        // already in flight.
        self.kick_agent_status_refresh();

        // Option B: drain a landed codex-session scan and pin uuids to
        // unpinned codex tabs, then re-arm a scan if any tab still needs one
        // (within its pin window). Pins don't change the frame, so neither
        // marks a redraw — same off-thread shape as agent-status above.
        self.apply_codex_session_pins();
        self.kick_codex_session_scan();

        // Tier 5: drain any off-thread graveyard op (archive / restore /
        // purge-all) that landed (it woke us via `Message::Wake(Wake::Graveyard)`).
        // Always drained here — the slot holds the outcome regardless of
        // which wake survived coalescing; the apply does the flash + refresh.
        if self.apply_graveyard_outcomes() {
            ctx.draw.mark(3);
        }

        // Archive mount / materialize / cleanup outcomes — same
        // unconditional pre-recv drain as the graveyard above. Mounting can
        // return effects (a materialize kicked by a mount, a pager open), so
        // the drain hands them back to be run.
        let (archive_draw, archive_fx) = self.apply_archive_outcomes();
        if archive_draw {
            ctx.draw.mark(3);
        }
        if !archive_fx.is_empty() {
            self.run_effects(archive_fx, terminal, foreground_exec);
        }

        let (file_draw, file_fx) = self.apply_file_outcomes();
        if file_draw {
            ctx.draw.mark(3);
        }
        if !file_fx.is_empty() {
            self.run_effects(file_fx, terminal, foreground_exec);
        }

        // A middle-click clipboard read that landed (woke us via
        // `Message::Wake(Wake::ClipboardPaste)`). Routing runs here, against the
        // focus of *now* — see `apply_clipboard_pastes`.
        // A clipboard write that landed (woke us via
        // `Message::Wake(Wake::ClipboardCopy)`). Only failures have anything to say.
        if self.apply_clipboard_writes() {
            ctx.draw.mark(3);
        }

        let (paste_draw, paste_fx) = self.apply_clipboard_pastes();
        if paste_draw {
            ctx.draw.mark(3);
        }
        if !paste_fx.is_empty() {
            self.run_effects(paste_fx, terminal, foreground_exec);
        }

        // `J`'s default path landed (woke via `Message::Wake(Wake::JumpDefault)`).
        if self.apply_jump_defaults() {
            ctx.draw.mark(3);
        }

        // Drain finished Lua runs (woke via `Message::Wake(Wake::Lua)`): apply the
        // requests the script produced, running any effects they translate to.
        let (lua_draw, lua_fx) = self.handle_lua_done();
        if lua_draw {
            ctx.draw.mark(3);
        }
        if !lua_fx.is_empty() {
            self.run_effects(lua_fx, terminal, foreground_exec);
        }

        // Drain any off-thread inventory ops (yank/remove/clear/put) that landed.
        if self.apply_inventory_outcomes() {
            ctx.draw.mark(3);
        }

        // Mermaid render+open results (woke us via `Message::Wake(Wake::Image)`) —
        // surface success/failure in the pager status line.
        if self.apply_image_outcomes() {
            ctx.draw.mark(3);
        }

        // Vsplit preview reloads (woke us via `Message::Wake(Wake::PreviewReload)`) —
        // install the rebuilt right-column view, preserving scroll. Always
        // drained here; the apply re-kicks if a save landed mid-render.
        if self.apply_preview_reloads() {
            ctx.draw.mark(3);
        }

        // Off-thread MCP worktree ops (woke via `Message::Wake(Wake::WorktreeJob)`) —
        // re-apply refresh+context on the loop, then reply to the client.
        if self.apply_worktree_outcomes() {
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

        // Resolve an in-flight git-view (deferred mount): mount the overlay
        // on first content, or flash "no changes" on an empty result. Runs
        // before the unified drain so a just-mounted stream is a no-op there
        // this tick.
        if self.drain_pending_git_view() {
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
    }

    /// The pre-`recv` timers, debounces and reconciles, against one clock read.
    /// Runs after [`Self::drain_landed_work`], which several of them depend on.
    pub(super) fn settle_before_recv(
        &mut self,
        ctx: &mut RunCtx,
        terminal: &mut Tui,
        foreground_exec: &ForegroundExec,
    ) {
        // MVU Phase 2: one clock read for all PRE-recv timers
        // (send_pending_resumes / watcher-stamp / refresh / git poll), matching their old
        // pre-recv local reads. POST-recv timers (activity rollover,
        // context-write) use `now_post` captured after recv returns.
        let now_pre = std::time::Instant::now();

        // Session-restore: deferred `/resume` sends (see
        // `handle_restore_resumes`).
        self.handle_restore_resumes(now_pre, ctx);

        // Drain buffered FsEvents + run the trailing-debounce listing
        // refresh (see `ingest_fs_and_maybe_refresh`).
        if self.ingest_fs_and_maybe_refresh(now_pre, ctx) {
            ctx.draw.mark(3);
        }
        // 1 Hz safety-net git poll + GitPoll deadline arming (see
        // `poll_git_cadence`).
        if self.poll_git_cadence(now_pre, ctx) {
            ctx.draw.mark(3);
        }

        // P1-2 viewport scan: bounded Codex delay; other agents wait for quiet.
        // Runs BEFORE settle_agent_activity so scrape_status is fresh.
        // Armed only while a dirty tab exists ⇒ idle stays 0 dps.
        if self.settle_scrape_quiet(now_pre, ctx) {
            ctx.draw.mark(3);
        }

        // Agent-activity (P0): derive each agent tab's Working/Idle from the
        // `last_output_at` stamped in `drain_pane_output`, advance the spicy
        // pulse frame, and arm AgentIdle (flip Working→Idle) + AgentAnim
        // (pulse while working). Idle when all tabs quiet ⇒ both disarmed ⇒
        // 0 dps preserved (see `settle_agent_activity`).
        let (agent_draw, agent_fx) = self.settle_agent_activity(now_pre, ctx);
        if agent_draw {
            ctx.draw.mark(3);
        }
        if !agent_fx.is_empty() {
            self.run_effects(agent_fx, terminal, foreground_exec);
        }

        // P3-1 visual bell: advance/decay the spice-heat border-pulse flash
        // (armed only after a Blocked/Done transition when `[notify].visual`
        // is set; a no-op otherwise, so idle stays 0 dps).
        if self.settle_visual_bell(now_pre, ctx) {
            ctx.draw.mark(3);
        }

        // Lua runaway watchdog: while a Lua job is in-flight + un-prompted,
        // arm `LuaRunaway` at its soft threshold and raise the interactive
        // "keep waiting? [y/N]" modal once it elapses. Idle stays 0 dps
        // (armed only while a job runs).
        if self.settle_lua_runaway(now_pre, ctx) {
            ctx.draw.mark(3);
        }

        // A deferred vt100 scrollback snapshot: take it once the settle
        // window has elapsed. Armed only between `^a v` (or a
        // `spyc_history` wheel gesture) and the shot, so idle stays 0 dps —
        // and the wait happens on the loop instead of in a sleep, which is
        // what keeps a wheel tick from stalling it.
        if self.settle_pane_scroll(now_pre, ctx) {
            ctx.draw.mark(3);
        }

        // P3-2 crash-sufficient autosave: debounce a session save after any
        // session-relevant change (tab/cwd/vsplit/project-home/geometry),
        // firing ~AUTOSAVE_DEBOUNCE after the last change so a SIGKILL loses
        // at most that window. Armed only while dirty ⇒ idle stays 0 dps; it
        // never needs a redraw, so nothing to mark.
        self.settle_autosave(now_pre, ctx);

        // An `$EDITOR` (or an agent) writing a member spyc extracted leaves no
        // trace but the file's own stats, so notice it here and record it —
        // the status badge reads the Model, and the draw pass can't stat
        // anything. Only looks at members handed to an editor, so an idle loop
        // (which never gets here) costs nothing either way.
        if self.settle_archive_edits() {
            ctx.draw.mark(3);
        }

        // Fire the low-frequency `spyc.on` state-change events (dir_changed
        // / project_changed) by diffing against the last-fired baselines —
        // AFTER `handle_lua_done` + its `run_effects(lua_fx)` above, so a
        // Lua-caused change is re-baselined (not re-fired) via the
        // re-entrancy guard, and `settle_agent_activity` (agent_status) has
        // already run this iteration. Only enqueues on a real change → no
        // redraw here (a handler's effects land on a later drain), 0 dps
        // idle.
        self.settle_lua_events();

        // `spyc -c` commands, once startup has settled. A no-op after the
        // first run, since the queue is empty.
        if let Some(startup_fx) = self.settle_startup_commands() {
            ctx.draw.mark(3);
            self.run_effects(startup_fx, terminal, foreground_exec);
        }

        // Reconcile the terminal's mouse mode against `[mouse] capture`. One
        // bool compare — emits nothing when they agree, so idle stays 0 dps
        // and this covers startup / `:mouse` / config reload /
        // return-from-foreground through a single path.
        let mouse_fx = self.settle_mouse_mode();
        if !mouse_fx.is_empty() {
            self.run_effects(mouse_fx, terminal, foreground_exec);
        }

        // Bring the process cwd to the focused column (#495). A focus
        // change or an agent's `open_worktree` into `b` installs a column
        // without the `chdir` a navigation does. One `getcwd`; nothing
        // emitted when they agree.
        let cwd_fx = self.settle_process_cwd();
        if !cwd_fx.is_empty() {
            self.run_effects(cwd_fx, terminal, foreground_exec);
        }

        // Re-install an agent pane's status hooks if something removed them
        // since it launched (a sibling spyc's teardown, a `git clean`).
        // Throttled and piggybacked on an iteration that was happening
        // anyway — no deadline, so idle stays 0 dps. Only the flash it
        // raises is visible, hence the redraw on a heal.
        if self.settle_status_hooks(now_pre) {
            ctx.draw.mark(3);
        }

        // Execute writable MCP commands buffered into `ctx.mcp_pending` (see
        // `drain_mcp_pending` — kept at this early loop position for the
        // 5s read-after-write timeout contract).
        if self.drain_mcp_pending(ctx) {
            ctx.draw.mark(3);
        }

        // P2 `wait_for_scope_clear`: resolve parked scope-waiters (a
        // `release_scope` may have landed in the drain just above) + honour
        // their deadlines. Armed only while a waiter is parked ⇒ 0 dps idle.
        self.settle_scope_waiters(now_pre, ctx);

        // Drain the git-worker results buffered into `ctx.git_pending` — the
        // SOLE apply/count/take site (see `drain_git_pending`).
        if self.drain_git_pending(ctx) {
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
    }
}

#[cfg(test)]
mod tests {
    /// A guard that pins a call into one of these phases holds only while the
    /// loop runs the phase, and in this order: the settles read what the
    /// drains landed, and both finish before the loop blocks.
    #[test]
    fn the_loop_runs_both_phases_before_it_waits() {
        let run = crate::guard_support::production_half(include_str!("run.rs"));
        let at = |needle: &str| {
            run.find(needle)
                .unwrap_or_else(|| panic!("run.rs lost `{needle}`"))
        };
        let drain = at("self.drain_landed_work(&mut ctx, terminal, &foreground_exec);");
        let settle = at("self.settle_before_recv(&mut ctx, terminal, &foreground_exec);");
        let wait = at("let wait_now = std::time::Instant::now();");
        assert!(
            drain < settle && settle < wait,
            "drain, then settle, then wait"
        );
    }
}
