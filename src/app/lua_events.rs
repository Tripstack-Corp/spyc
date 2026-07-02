//! Tier-C `spyc.on` event dispatch: firing registered Lua callbacks on
//! low-frequency events (the capstone of the Lua platform, `docs/LUA_SCRIPTING_PLAN.md`).
//!
//! Four events fire — `startup` (once, after init.lua loads), `dir_changed`,
//! `project_changed`, and `agent_status` — each passing the callback an event
//! table. Events fire from the impure App layer POST-transition: `dir_changed`
//! / `project_changed` diff against last-fired baselines in
//! [`App::settle_lua_events`] (loop-bottom); `agent_status` fires on a semantic
//! transition from `settle_agent_activity`; `startup` fires once the init.lua
//! `Load` drains. Each submits a `LuaJob::RunRegistered { event: Some(payload) }`
//! that drains through the SAME [`App::handle_lua_done`](super::App::handle_lua_done)
//! path as a map/command trigger — no new `Message`/coalesce variant.
//!
//! High-frequency events (`cursor_moved` / `pane_output`) are intentionally not
//! wired: they'd fire at key-repeat / firehose speed and blow the repaint budget.

use std::collections::HashMap;

use crate::lua::{LuaJob, LuaWorker};

use super::App;

/// The four event names `spyc.on` can register a live-dispatched hook for.
/// Any other name registers fine (the seam accepts it) but never fires — only
/// these are wired to a dispatch site.
pub(super) const EV_STARTUP: &str = "startup";
pub(super) const EV_DIR_CHANGED: &str = "dir_changed";
pub(super) const EV_PROJECT_CHANGED: &str = "project_changed";
pub(super) const EV_AGENT_STATUS: &str = "agent_status";

/// Bookkeeping for `spyc.on` event dispatch (the Tier-C seam). Held in
/// `Runtime` (it tracks live worker + clock state, never the pure Model). The
/// diff baselines let `settle_lua_events` fire only on a genuine change; the
/// re-entrancy guard stops a handler whose own request re-triggers the event.
/// Fields are `pub(super)` so the trigger paths in `lua.rs`
/// (`handle_lua_done` / `load_init_lua` / `reload_init_lua` / `cmd_lua`) can
/// arm/clear them alongside this module's dispatch.
#[derive(Default)]
pub struct LuaEventState {
    /// Set when `load_init_lua`/`reload_init_lua` submits the `Load`/`Reload`
    /// that carries the `spyc.on` registrations, cleared once that outcome
    /// drains — the `startup` event fires exactly then (after the handlers are
    /// installed), so a startup handler is actually registered when it fires.
    pub(super) startup_pending: bool,
    /// `true` while `handle_lua_done` is applying a finished run's requests
    /// (and the effects they translate to) this iteration. `settle_lua_events`
    /// treats a cwd/project change seen under this flag as Lua-caused: it
    /// silently re-baselines instead of firing, so an `on("dir_changed")`
    /// handler that itself navigates can't loop.
    pub(super) applying_requests: bool,
    /// Last cwd `dir_changed` observed — `None` until the first settle seeds
    /// it (so the initial cwd is a baseline, not a spurious first fire).
    last_cwd: Option<std::path::PathBuf>,
    /// Whether `last_cwd` has been seeded (distinguishes "no cwd yet" from a
    /// genuine `None` — the focused column always has a cwd, so this is really
    /// a first-run guard for the whole `settle_lua_events`).
    seeded: bool,
    /// Last PROJECT_HOME `project_changed` observed (seeded alongside `seeded`).
    last_project_home: Option<std::path::PathBuf>,
    /// Last semantic agent status fired per tab id (`SPYC_PANE_ID`), so
    /// `agent_status` fires only on a real working↔blocked↔done↔idle
    /// transition, never on an output tick / animation frame. Entries for
    /// closed tabs are pruned each settle.
    pub(super) last_agent_status: HashMap<String, crate::pane::AgentActivity>,
}

/// The `state` string an `agent_status` event carries for a tab's semantic
/// activity — `None` for `Unknown` (a non-agent tab / no signal yet), which
/// never fires an event. Mirrors the `report_status` MCP vocabulary.
const fn agent_state_label(a: crate::pane::AgentActivity) -> Option<&'static str> {
    use crate::pane::AgentActivity::{Blocked, Done, Idle, Unknown, Working};
    match a {
        Working => Some("working"),
        Blocked => Some("blocked"),
        Done => Some("done"),
        Idle => Some("idle"),
        Unknown => None,
    }
}

impl App {
    /// Fire every `spyc.on(event, …)` callback registered for `event`, passing
    /// `payload` (marshaled to a Lua table via `json_to_lua`) as the callback's
    /// single argument. A no-op when no handler is registered for `event` /
    /// Lua is disabled.
    ///
    /// **Busy-skip:** if a script is already running, the event is DROPPED (not
    /// queued) — an event fired during a running script re-fires on the next
    /// occurrence, and single-worker serialization stays intact. This also
    /// bounds runaway event queueing.
    ///
    /// The re-entrancy guard lives in the CALLER (`settle_lua_events`), not
    /// here: only `dir_changed`/`project_changed` can be caused by a Lua
    /// request (a handler that navigates), and `settle_lua_events` suppresses
    /// those. `startup` / `agent_status` can't be self-triggered by a request,
    /// so they always fire.
    ///
    /// No user-facing flash — events are automatic, so an event that can't fire
    /// (busy) is silent, never a "lua busy" line.
    pub(super) fn fire_lua_event(&mut self, event: &str, payload: serde_json::Value) {
        let fn_ids = match self.runtime.lua_registry.events.get(event) {
            Some(ids) if !ids.is_empty() => ids.clone(),
            _ => return,
        };
        if !self.ensure_lua_worker() {
            return;
        }
        // Skip (drop) rather than queue when a script is running: bounds runaway
        // event queueing and keeps the single-worker guarantee.
        if self.runtime.lua.as_ref().is_some_and(LuaWorker::is_busy) {
            return;
        }
        let snapshot = self.snapshot_context();
        // One job per handler. Serial worker → they run back-to-back; each
        // finished run drains through `handle_lua_done` like any trigger.
        for fn_id in fn_ids {
            self.submit_lua_job(
                format!("on:{event}"),
                LuaJob::RunRegistered {
                    fn_id,
                    snapshot: snapshot.clone(),
                    event: Some(payload.clone()),
                },
            );
        }
    }

    /// Fire the low-frequency `spyc.on` state-change events — `dir_changed` (the
    /// focused column's cwd) and `project_changed` (PROJECT_HOME) — by diffing
    /// against the last-fired baselines. Called at loop-bottom (the impure
    /// settle point, post-transition), so it catches a change from ANY source
    /// (key, MCP, session restore, or a Lua request) without instrumenting each
    /// mutation site.
    ///
    /// Re-entrancy: a change CAUSED by a Lua run this iteration (the
    /// `applying_requests` flag `handle_lua_done` set) is re-baselined silently
    /// — never fired — so an `on("dir_changed")` handler that navigates can't
    /// loop. The flag is cleared here (it spans this whole iteration, incl. the
    /// deferred `run_effects(lua_fx)`).
    ///
    /// Idle stays 0 dps: this only submits a job on an actual change; when
    /// nothing changed it does no work and arms no timer.
    pub(crate) fn settle_lua_events(&mut self) {
        // Nothing to do without any event registration (the common case) — no
        // baseline churn, no clock, no work.
        if self.runtime.lua_registry.events.is_empty() {
            self.runtime.lua_events.applying_requests = false;
            return;
        }
        let suppress = self.runtime.lua_events.applying_requests;
        self.runtime.lua_events.applying_requests = false;

        let cwd = self.state.cur().listing.dir.clone();
        let project = self.state.project_home.clone();

        // First settle seeds the baselines without firing (the initial cwd /
        // PROJECT_HOME are a baseline, not a change).
        if !self.runtime.lua_events.seeded {
            self.runtime.lua_events.seeded = true;
            self.runtime.lua_events.last_cwd = Some(cwd);
            self.runtime.lua_events.last_project_home = project;
            return;
        }

        if self.runtime.lua_events.last_cwd.as_deref() != Some(cwd.as_path()) {
            self.runtime.lua_events.last_cwd = Some(cwd.clone());
            if !suppress {
                self.fire_lua_event(
                    EV_DIR_CHANGED,
                    serde_json::json!({ "cwd": cwd.to_string_lossy() }),
                );
            }
        }
        if self.runtime.lua_events.last_project_home != project {
            self.runtime.lua_events.last_project_home = project.clone();
            if !suppress {
                let home = project.map(|p| p.to_string_lossy().into_owned());
                self.fire_lua_event(
                    EV_PROJECT_CHANGED,
                    serde_json::json!({ "project_home": home }),
                );
            }
        }
    }

    /// Whether any `spyc.on('agent_status', …)` handler is registered — the
    /// gate `settle_agent_activity` checks before doing any per-tab status
    /// tracking, so a configless / no-handler run pays nothing.
    pub(crate) fn lua_wants_agent_status_event(&self) -> bool {
        self.runtime
            .lua_registry
            .events
            .get(EV_AGENT_STATUS)
            .is_some_and(|v| !v.is_empty())
    }

    /// Fire `agent_status` for a tab whose SEMANTIC status just transitioned
    /// (working↔blocked↔done↔idle). Called from `settle_agent_activity` with the
    /// tab's newly-computed effective status, keyed by its stable `SPYC_PANE_ID`
    /// so a repeat of the same status (an output tick / animation frame) does
    /// NOT fire — only a genuine transition does. Payload `{pane, state}` where
    /// `pane` is the `SPYC_PANE_ID` and `state` ∈ working|blocked|done|idle.
    ///
    /// A no-op when no `agent_status` handler is registered (skips all the
    /// baseline bookkeeping in the common case). The re-entrancy guard doesn't
    /// apply — a status transition is driven by pane output / a report, never
    /// by a Lua request, so a handler can't cause one.
    pub(crate) fn fire_agent_status_event(
        &mut self,
        pane_id: &str,
        status: crate::pane::AgentActivity,
    ) {
        if !self
            .runtime
            .lua_registry
            .events
            .contains_key(EV_AGENT_STATUS)
        {
            return;
        }
        let Some(label) = agent_state_label(status) else {
            // `Unknown` — a non-agent tab / no signal yet — never fires; drop
            // any prior baseline for the id so a later real status is a change.
            self.runtime.lua_events.last_agent_status.remove(pane_id);
            return;
        };
        let previously = self
            .runtime
            .lua_events
            .last_agent_status
            .get(pane_id)
            .copied();
        if previously == Some(status) {
            return;
        }
        self.runtime
            .lua_events
            .last_agent_status
            .insert(pane_id.to_string(), status);
        self.fire_lua_event(
            EV_AGENT_STATUS,
            serde_json::json!({ "pane": pane_id, "state": label }),
        );
    }

    /// Drop `agent_status` baselines for tabs that no longer exist, so a closed
    /// pane's id doesn't linger (and a reused-looking transition can't be
    /// missed). Called once per `settle_agent_activity` after the per-tab pass.
    pub(crate) fn prune_agent_status_baselines(
        &mut self,
        live_ids: &std::collections::HashSet<String>,
    ) {
        if self.runtime.lua_events.last_agent_status.is_empty() {
            return;
        }
        self.runtime
            .lua_events
            .last_agent_status
            .retain(|id, _| live_ids.contains(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lua::LuaWorker;
    use crate::pane::AgentActivity;
    use std::sync::mpsc;
    use std::time::Duration;

    use crate::app::{Effect, Message};

    /// Attach a REAL Lua worker to `app` (the test harness has none) wired to a
    /// wake channel the test can block on, so an integration test drives a real
    /// event→callback→request→apply cycle. Returns the wake receiver.
    fn attach_lua_worker(app: &mut App) -> mpsc::Receiver<Message> {
        let (tx, rx) = mpsc::channel::<Message>();
        app.runtime.pane_wake_tx = Some(tx.clone());
        app.runtime.lua = Some(LuaWorker::spawn(move || {
            let _ = tx.send(Message::LuaDone);
        }));
        rx
    }

    /// Block until the worker signals a finished job, then drain it. Panics on
    /// timeout so a wedged worker fails the test instead of hanging.
    fn wait_and_drain(app: &mut App, rx: &mpsc::Receiver<Message>) -> Vec<Effect> {
        rx.recv_timeout(Duration::from_secs(5))
            .expect("a lua job completed");
        app.handle_lua_done().1
    }

    /// Write `init.lua` under a temp config root and load it (installing its
    /// `spyc.on` registrations), returning the config-root tempdir to keep the
    /// file alive for the test's lifetime.
    fn load_init(app: &mut App, rx: &mpsc::Receiver<Message>, body: &str) -> tempfile::TempDir {
        let cfg = tempfile::tempdir().unwrap();
        std::fs::write(cfg.path().join("init.lua"), body).unwrap();
        crate::state::with_config_root(cfg.path(), || {
            app.load_init_lua();
        });
        // Drain the Load: installs the `on` registration + fires `startup` (a
        // no-op unless a startup handler was registered).
        let _ = wait_and_drain(app, rx);
        cfg
    }

    /// Loop-integration e2e: a registered `on("dir_changed")` handler enqueues a
    /// request; firing the event runs the callback, whose request drains through
    /// `handle_lua_done` and applies. Drives the whole
    /// event→callback→request→apply cycle end to end (the coverage gap behind
    /// the earlier drain/dispatch bugs).
    #[test]
    fn dir_changed_event_runs_callback_and_applies_its_request() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            let rx = attach_lua_worker(&mut app);
            let _cfg = load_init(
                &mut app,
                &rx,
                "spyc.on('dir_changed', function(ev) spyc.notify('cd:' .. ev.cwd) end)",
            );
            assert!(
                !app.lua_wants_agent_status_event()
                    && app.runtime.lua_registry.events.contains_key(EV_DIR_CHANGED),
                "the dir_changed handler is registered"
            );

            // Fire the event directly with a payload, then drive the cycle.
            app.state.flash = None;
            app.fire_lua_event(EV_DIR_CHANGED, serde_json::json!({ "cwd": "/some/where" }));
            let _ = wait_and_drain(&mut app, &rx);
            assert_eq!(
                app.flash_text(),
                Some("cd:/some/where"),
                "the callback ran with the event payload and its notify applied"
            );
        });
    }

    /// `settle_lua_events` fires `dir_changed` on a genuine cwd change, and the
    /// callback's request applies — proving the loop-level diff path (not just a
    /// direct `fire_lua_event`) works end to end.
    #[test]
    fn settle_fires_dir_changed_on_a_real_cwd_change() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            let rx = attach_lua_worker(&mut app);
            let _cfg = load_init(
                &mut app,
                &rx,
                "spyc.on('dir_changed', function(ev) spyc.notify('at:' .. ev.cwd) end)",
            );
            // First settle seeds the baseline (no fire).
            app.settle_lua_events();
            assert!(!app.runtime.lua.as_ref().unwrap().is_busy());

            // Change the focused column's cwd, then settle → fires.
            app.state.left.listing.dir = std::path::PathBuf::from("/moved/here");
            app.state.flash = None;
            app.settle_lua_events();
            let _ = wait_and_drain(&mut app, &rx);
            assert_eq!(app.flash_text(), Some("at:/moved/here"));
        });
    }

    /// Re-entrancy: an `on("dir_changed")` handler whose OWN request changes the
    /// cwd must not infinite-loop. The `applying_requests` guard makes
    /// `settle_lua_events` re-baseline (not re-fire) a Lua-caused change, so the
    /// event fires a BOUNDED number of times.
    #[test]
    fn dir_changed_handler_that_navigates_does_not_loop() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            let rx = attach_lua_worker(&mut app);
            // The handler navigates to a fixed dir on every dir_changed. Without
            // the guard, each navigate would re-fire dir_changed forever.
            let _cfg = load_init(
                &mut app,
                &rx,
                "spyc.on('dir_changed', function(ev) spyc.navigate('/tmp') end)",
            );
            app.settle_lua_events(); // seed baseline

            // Simulate a user cwd change → fires the handler.
            app.state.left.listing.dir = std::path::PathBuf::from("/user/moved");
            let mut fires = 0;
            // Drive several loop iterations. Each: settle (may fire), drain (runs
            // the handler → its navigate applies via effects/requests, setting
            // applying_requests), then a follow-up settle re-baselines silently.
            for _ in 0..6 {
                app.settle_lua_events();
                if app.runtime.lua.as_ref().unwrap().is_busy()
                    || rx.try_recv().is_ok()
                    || app.runtime.lua_inflight.is_some()
                {
                    // A job was submitted → wait + drain (the handler ran).
                    let _ = rx.recv_timeout(Duration::from_secs(5));
                    let (_d, fx) = app.handle_lua_done();
                    // Apply any ChangeDir the navigate produced, mutating cwd
                    // (mirrors run_effects(lua_fx) in the real loop).
                    for e in &fx {
                        if let Effect::ChangeDir { path, .. } = e {
                            app.state.left.listing.dir = path.clone();
                        }
                    }
                    fires += 1;
                }
            }
            // The navigate is a Lua-caused change → re-baselined, never re-fired.
            // So exactly one fire (the user's change), not an unbounded cascade.
            assert!(
                fires <= 1,
                "the re-entrancy guard must bound event fires; got {fires}"
            );
        });
    }

    /// `agent_status` fires on a genuine semantic transition, and NOT on a
    /// repeat of the same status (an output tick / animation frame). Drives
    /// `fire_agent_status_event` directly (the transition-detection core).
    #[test]
    fn agent_status_fires_on_transition_not_repeat() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            let rx = attach_lua_worker(&mut app);
            let _cfg = load_init(
                &mut app,
                &rx,
                "spyc.on('agent_status', function(ev) spyc.notify(ev.pane .. '=' .. ev.state) end)",
            );
            assert!(app.lua_wants_agent_status_event());

            // First observation of a status is a transition (nil → working).
            app.state.flash = None;
            app.fire_agent_status_event("pane-1", AgentActivity::Working);
            let _ = wait_and_drain(&mut app, &rx);
            assert_eq!(app.flash_text(), Some("pane-1=working"));

            // A REPEAT of the same status fires nothing (no new job).
            app.state.flash = None;
            app.fire_agent_status_event("pane-1", AgentActivity::Working);
            assert!(
                !app.runtime.lua.as_ref().unwrap().is_busy()
                    && rx.try_recv().is_err()
                    && app.runtime.lua_inflight.is_none(),
                "a repeat status must not fire"
            );

            // A transition to a NEW status fires again.
            app.fire_agent_status_event("pane-1", AgentActivity::Blocked);
            let _ = wait_and_drain(&mut app, &rx);
            assert_eq!(app.flash_text(), Some("pane-1=blocked"));

            // `Unknown` never fires (drops the baseline) — a non-agent tab.
            app.state.flash = None;
            app.fire_agent_status_event("pane-1", AgentActivity::Unknown);
            assert!(
                rx.try_recv().is_err() && app.runtime.lua_inflight.is_none(),
                "Unknown must not fire an event"
            );
        });
    }

    /// An event with no registered handler is a silent no-op (never submits a
    /// job, never flashes), so a configless run pays nothing per event.
    #[test]
    fn firing_an_unregistered_event_is_a_noop() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            let _rx = attach_lua_worker(&mut app);
            app.state.flash = None;
            app.fire_lua_event(EV_DIR_CHANGED, serde_json::json!({ "cwd": "/x" }));
            assert!(
                !app.runtime.lua.as_ref().unwrap().is_busy() && app.runtime.lua_inflight.is_none(),
                "no handler → no job submitted"
            );
            assert!(app.flash_text().is_none(), "events never flash");
        });
    }

    /// A `startup` handler fires exactly once, when its `Load` registrations
    /// install — the callback runs and its request applies.
    #[test]
    fn startup_event_fires_once_after_load() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            let rx = attach_lua_worker(&mut app);
            // load_init's own drain fires startup (the Load installed the
            // handler, then handle_lua_done fired `startup`).
            app.state.flash = None;
            let _cfg = load_init(
                &mut app,
                &rx,
                "spyc.on('startup', function() spyc.notify('booted') end)",
            );
            // The startup fire submitted a second job (the handler run); drain it.
            let _ = wait_and_drain(&mut app, &rx);
            assert_eq!(app.flash_text(), Some("booted"));
            // startup_pending is cleared — it won't fire again.
            assert!(!app.runtime.lua_events.startup_pending);
        });
    }
}
