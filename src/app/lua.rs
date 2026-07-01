//! App-layer glue for the embedded Lua engine (`src/lua/`).
//!
//! Lazy-spawns the worker on first use, submits a script run with a context
//! snapshot, and — on the pre-recv drain ([`App::handle_lua_done`]) — translates
//! the [`LuaRequest`]s a finished script produced into spyc's existing
//! effect/action vocabulary. Lua never mutates the `App` directly; it only
//! enqueues requests, which this module applies on the main thread, so the MVU
//! contract holds (requests are data; the existing handlers run the effects).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent};

use crate::keymap::user::{BoundAction, UserBinding};
use crate::lua::{LuaJob, LuaOutcome, LuaRequest, LuaWorker, RegKind, Registration};
use crate::mcp_cmd::{McpCommand, McpResponse};

use super::{App, Deadline, Effect, Message, Mode, Prompt, PromptKind, RunCtx};

/// Cap on how many times `spyc.action(name, count)` re-applies an action — the
/// requests run on the main thread, so an enormous count would block the loop.
const MAX_ACTION_REPEAT: u32 = 1_000;

/// Soft threshold after which an in-flight Lua job raises the interactive
/// "keep waiting? [y/N]" runaway prompt. The worker's hard wall-clock ceiling
/// (`DEFAULT_CEILING`, 30s) is the backstop even if the user keeps choosing to
/// wait. Possible only because the interpreter is off-thread, so the loop is
/// free to render the modal and read the answer while the script runs.
const LUA_RUNAWAY_SOFT: Duration = Duration::from_secs(1);

/// Tracks the currently-running Lua job (main side) for the runaway watchdog.
/// Held in `Runtime` (an `Instant` is fine there — it's not the pure Model);
/// `None` whenever no job is in flight. Jobs are serial + busy-guarded, so at
/// most one exists at a time, and one drained outcome means it finished.
pub struct LuaInflight {
    /// Human display name for the modal ("init.lua", a script name, etc.).
    pub name: String,
    /// When the (current) watchdog window started — `Instant::now()` at submit,
    /// reset on a "keep waiting" so the prompt re-fires after another window.
    pub started_at: Instant,
    /// Whether the runaway prompt has already been raised for this window
    /// (so the watchdog raises it at most once per window).
    pub prompted: bool,
}

/// Pure runaway-watchdog decision: should the "keep waiting?" prompt be raised
/// now? True iff a job is in flight, hasn't been prompted for this window, and
/// has run at least `soft`. Clock-free (the caller computes `elapsed`) so it's
/// unit-testable like `route.rs` / `focus.rs`.
pub(super) const fn lua_runaway_due(prompted: bool, elapsed: Duration, soft: Duration) -> bool {
    !prompted && elapsed.as_millis() >= soft.as_millis()
}

/// Sentinel prefix for a `spyc.map`-registered key binding: a synthetic
/// `BoundAction::Lua("@map:<idx>")` keymap entry whose `<idx>` indexes
/// [`LuaRegistry::maps`]. Distinguishes a registered-callback binding from a
/// `map KEY lua <name>` file binding (which names a script under `lua/`).
const MAP_SENTINEL: &str = "@map:";

/// The runtime registries `init.lua` populates via `spyc.map` / `spyc.command`
/// / `spyc.on`. Each entry maps a trigger to the worker-side `fn_id` of the Lua
/// callback to invoke. Held in `Runtime` (it tracks live worker state); rebuilt
/// from scratch on every `init.lua` (re)load.
#[derive(Default)]
pub struct LuaRegistry {
    /// `spyc.map` callbacks, indexed by position; the `@map:<idx>` keymap
    /// sentinel addresses this. Each push also appends a synthetic keymap
    /// binding, so the index and the binding stay in lockstep.
    pub maps: Vec<u64>,
    /// `spyc.command` callbacks, keyed by `:`-command name.
    pub commands: HashMap<String, u64>,
    /// `spyc.on` callbacks, keyed by event name. Recorded but not yet
    /// dispatched (the Tier-C event seam).
    pub events: HashMap<String, Vec<u64>>,
}

impl LuaRegistry {
    /// Drop every registered callback id. Paired with a `Reload` job (which
    /// re-runs init.lua) so the next drain re-populates from the fresh
    /// registrations.
    fn clear(&mut self) {
        self.maps.clear();
        self.commands.clear();
        self.events.clear();
    }
}

impl App {
    /// Ensure the Lua worker is running (lazy spawn). Returns whether one is
    /// available — false when Lua is disabled (`--no-lua` / `:lua off`) or no
    /// wake channel exists yet (pre-`run()` / the test harness).
    fn ensure_lua_worker(&mut self) -> bool {
        if !crate::lua::enabled() {
            return false;
        }
        if self.runtime.lua.is_some() {
            return true;
        }
        let Some(tx) = self.runtime.pane_wake_tx.clone() else {
            return false;
        };
        self.runtime.lua = Some(LuaWorker::spawn(move || {
            let _ = tx.send(Message::LuaDone);
        }));
        true
    }

    /// Trigger a `map KEY lua <name>` binding. A `@map:<idx>` sentinel name
    /// fires the `spyc.map`-registered callback at that index; any other name
    /// runs the file `<config_root>/lua/<name>.lua`. Both submit with the
    /// current context snapshot.
    pub(super) fn apply_lua_binding(&mut self, name: &str) -> Vec<Effect> {
        if !self.ensure_lua_worker() {
            self.state
                .flash_error("lua is disabled (--no-lua / :lua off)");
            return Vec::new();
        }
        if let Some(idx) = name.strip_prefix(MAP_SENTINEL) {
            return self.fire_registered_map(idx);
        }
        let Some(lua_dir) = crate::state::config_root().map(|r| r.join("lua")) else {
            self.state
                .flash_error("lua: no config dir ($HOME / $XDG_CONFIG_HOME unset)");
            return Vec::new();
        };
        if self.lua_busy_flash() {
            return Vec::new();
        }
        let snapshot = self.snapshot_context();
        self.submit_lua_job(
            name.to_string(),
            LuaJob::RunFile {
                name: name.to_string(),
                lua_dir,
                snapshot,
            },
        );
        Vec::new()
    }

    /// Fire a `spyc.map`-registered callback by its `@map:<idx>` index.
    fn fire_registered_map(&mut self, idx: &str) -> Vec<Effect> {
        let Some(fn_id) = idx
            .parse::<usize>()
            .ok()
            .and_then(|i| self.runtime.lua_registry.maps.get(i).copied())
        else {
            self.state.flash_error("lua: stale mapped binding");
            return Vec::new();
        };
        if self.lua_busy_flash() {
            return Vec::new();
        }
        let snapshot = self.snapshot_context();
        self.submit_lua_job(
            "mapped key".to_string(),
            LuaJob::RunRegistered { fn_id, snapshot },
        );
        Vec::new()
    }

    /// Dispatch a Lua-registered `:`-command (the COMMAND_TABLE fallthrough).
    /// `Some(effects)` when a registered callback was submitted; `None` when no
    /// Lua command of that name exists (so the caller flashes "unknown").
    pub(super) fn dispatch_lua_command(&mut self, name: &str) -> Option<Vec<Effect>> {
        let fn_id = *self.runtime.lua_registry.commands.get(name)?;
        if !self.ensure_lua_worker() {
            self.state
                .flash_error("lua is disabled (--no-lua / :lua off)");
            return Some(Vec::new());
        }
        if self.lua_busy_flash() {
            return Some(Vec::new());
        }
        let snapshot = self.snapshot_context();
        self.submit_lua_job(
            format!(":{name}"),
            LuaJob::RunRegistered { fn_id, snapshot },
        );
        Some(Vec::new())
    }

    /// Best-effort serialize: flash + return true when a script is already
    /// running rather than stacking runs when a trigger fires repeatedly.
    fn lua_busy_flash(&mut self) -> bool {
        if self.runtime.lua.as_ref().is_some_and(LuaWorker::is_busy) {
            self.state.flash_error("lua: a script is already running");
            true
        } else {
            false
        }
    }

    /// Submit `job` to the worker, flashing on a send error (worker gone). On a
    /// successful submit, record `lua_inflight` (the `name` is the runaway
    /// modal's display label) so the watchdog can raise the "keep waiting?"
    /// prompt if the job runs past `LUA_RUNAWAY_SOFT`.
    fn submit_lua_job(&mut self, name: String, job: LuaJob) {
        if let Some(worker) = self.runtime.lua.as_ref() {
            match worker.submit(job) {
                Ok(()) => {
                    self.runtime.lua_inflight = Some(LuaInflight {
                        name,
                        started_at: Instant::now(),
                        prompted: false,
                    });
                }
                Err(e) => self.state.flash_error(format!("lua: {e}")),
            }
        }
    }

    /// Drain finished Lua runs (pre-recv scan). Returns `(needs_draw, effects)`,
    /// mirroring `apply_file_outcomes`: an error flashes; a failed/aborted run
    /// applied nothing; a successful run's requests translate to effects/actions
    /// and its registrations (from a `Load`/`Reload`) install live bindings.
    pub(super) fn handle_lua_done(&mut self) -> (bool, Vec<Effect>) {
        let Some(worker) = self.runtime.lua.as_ref() else {
            return (false, Vec::new());
        };
        let outcomes: Vec<LuaOutcome> = worker.drain_outcomes();
        if outcomes.is_empty() {
            return (false, Vec::new());
        }
        // The job finished (jobs are serial + busy-guarded, so one outcome ⇒
        // the in-flight job is done). If the inflight slot is already `None`,
        // the user aborted via the runaway prompt (which flashed + cleared it),
        // so suppress the abort error the outcome carries — it's not news.
        let user_aborted = self.runtime.lua_inflight.is_none();
        self.runtime.lua_inflight = None;
        // Dismiss the runaway modal if it's still up (the job beat the user to
        // the answer).
        if matches!(
            &self.state.mode,
            Mode::Prompting(p) if matches!(p.kind, PromptKind::LuaRunaway)
        ) {
            self.state.mode = Mode::Normal;
            self.view.needs_full_repaint = true;
        }
        let mut effects = Vec::new();
        for outcome in outcomes {
            if let Some(err) = outcome.error {
                if !user_aborted {
                    self.state.flash_error(err);
                }
                continue;
            }
            for Registration { kind, fn_id } in outcome.registrations {
                self.apply_lua_registration(kind, fn_id);
            }
            for req in outcome.requests {
                effects.extend(self.apply_lua_request(req));
            }
        }
        (true, effects)
    }

    /// Install one `init.lua` registration as a live binding: a `Map` appends a
    /// synthetic `@map:<idx>` keymap entry (which the existing resolver routes
    /// straight back to `apply_lua_binding`); a `Command` records a runtime
    /// `:`-command; an `Event` is recorded but not yet dispatched.
    fn apply_lua_registration(&mut self, kind: RegKind, fn_id: u64) {
        match kind {
            RegKind::Map(key) => {
                let chord = match crate::config::dsl::parse_key(&key) {
                    Ok(c) => c,
                    Err(e) => {
                        self.state
                            .flash_error(format!("lua: spyc.map bad key '{key}': {e}"));
                        return;
                    }
                };
                let idx = self.runtime.lua_registry.maps.len();
                self.runtime.lua_registry.maps.push(fn_id);
                self.state.user_keymap.push(UserBinding {
                    chord,
                    action: BoundAction::Lua(format!("{MAP_SENTINEL}{idx}")),
                });
            }
            RegKind::Command(name) => {
                self.runtime.lua_registry.commands.insert(name, fn_id);
            }
            RegKind::Event(event) => {
                self.runtime
                    .lua_registry
                    .events
                    .entry(event)
                    .or_default()
                    .push(fn_id);
            }
        }
    }

    /// Translate one finished-script request into spyc's existing vocabulary,
    /// reusing `execute_mcp_command` for the overlapping mutations, `apply` for
    /// built-in actions, and `dispatch_command` for `:` commands.
    fn apply_lua_request(&mut self, req: LuaRequest) -> Vec<Effect> {
        match req {
            LuaRequest::Action { name, count } => self.run_lua_action(&name, count),
            LuaRequest::Command(line) => self.dispatch_command(&line),
            LuaRequest::Navigate(path) => {
                self.run_lua_mcp(McpCommand::NavigateTo { path });
                Vec::new()
            }
            LuaRequest::Pick(patterns) => {
                self.run_lua_mcp(McpCommand::PickFiles { patterns });
                Vec::new()
            }
            LuaRequest::ClearPicks => {
                self.run_lua_mcp(McpCommand::ClearPicks);
                Vec::new()
            }
            LuaRequest::Filter(pattern) => {
                self.run_lua_mcp(McpCommand::SetFilter { pattern });
                Vec::new()
            }
            LuaRequest::ReportStatus(status) => {
                self.run_lua_mcp(McpCommand::ReportStatus {
                    pane_id: None,
                    pane: None,
                    status,
                    ttl_ms: None,
                });
                Vec::new()
            }
            LuaRequest::Notify(msg) => {
                self.state.flash_info(msg);
                Vec::new()
            }
            LuaRequest::Warn(msg) => {
                self.state.flash_error(msg);
                Vec::new()
            }
        }
    }

    /// Run a built-in action named by its `.spycrc` DSL verb (the single source
    /// of truth, reused from `config::dsl`), `count` times (clamped). Only
    /// `Plain` actions are reachable; verbs needing inline args
    /// (`unix`/`jump`/`command`) are rejected — `spyc.cmd(":…")` is the escape
    /// hatch for those.
    fn run_lua_action(&mut self, name: &str, count: Option<u32>) -> Vec<Effect> {
        match crate::config::dsl::parse_action(name, "") {
            Ok(BoundAction::Plain(action)) => {
                let repeats = count.unwrap_or(1).clamp(1, MAX_ACTION_REPEAT);
                let mut fx = Vec::new();
                for _ in 0..repeats {
                    match self.apply(&action) {
                        Ok(e) => fx.extend(e),
                        Err(e) => {
                            self.state.flash_error(format!("lua: action '{name}': {e}"));
                            break;
                        }
                    }
                }
                fx
            }
            Ok(_) => {
                self.state.flash_error(format!(
                    "lua: action '{name}' needs arguments not available via spyc.action — use spyc.cmd"
                ));
                Vec::new()
            }
            Err(e) => {
                self.state
                    .flash_error(format!("lua: unknown action '{name}': {e}"));
                Vec::new()
            }
        }
    }

    /// Run an MCP-vocabulary mutation, surfacing an error response as a flash.
    fn run_lua_mcp(&mut self, cmd: McpCommand) {
        if let McpResponse::Error { message } = self.execute_mcp_command(cmd) {
            self.state.flash_error(format!("lua: {message}"));
        }
    }

    /// Resolve `<config_root>/init.lua` if Lua is enabled and the file exists.
    fn init_lua_path(&self) -> Option<std::path::PathBuf> {
        if !crate::lua::enabled() {
            return None;
        }
        let path = crate::state::config_root()?.join("init.lua");
        path.is_file().then_some(path)
    }

    /// Load `~/.config/spyc/init.lua` (if present + Lua enabled) by submitting a
    /// `Load` job. Called once from `run()` after the wake channel is wired, so
    /// `ensure_lua_worker` can spawn. A no-op (silent) when there's no init.lua
    /// — the common case — so a configless run pays nothing.
    pub(super) fn load_init_lua(&mut self) {
        let Some(init_path) = self.init_lua_path() else {
            return;
        };
        if !self.ensure_lua_worker() {
            return;
        }
        let snapshot = self.snapshot_context();
        self.submit_lua_job(
            "init.lua".to_string(),
            LuaJob::Load {
                init_path,
                snapshot,
            },
        );
    }

    /// Re-run `init.lua` after a config reload (`^R` / `:lua reload`). The
    /// loaded keymap already dropped the old synthetic `@map:` entries, so we
    /// clear the registries and submit a `Reload` job; the next
    /// [`App::handle_lua_done`] re-applies the fresh registrations (re-appending
    /// the synthetic keymap entries).
    ///
    /// `verbose` flashes when there's no init.lua / Lua is off — wanted for an
    /// explicit `:lua reload`, but silent for the implicit `^R` config reload
    /// (a configless user shouldn't get a Lua flash every `^R`).
    pub(super) fn reload_init_lua(&mut self, verbose: bool) {
        self.runtime.lua_registry.clear();
        let Some(init_path) = self.init_lua_path() else {
            if verbose {
                if crate::lua::enabled() {
                    self.state.flash_info("lua: no init.lua to reload");
                } else {
                    self.state
                        .flash_error("lua is disabled (--no-lua / :lua off)");
                }
            }
            return;
        };
        if !self.ensure_lua_worker() {
            return;
        }
        let snapshot = self.snapshot_context();
        self.submit_lua_job(
            "init.lua".to_string(),
            LuaJob::Reload {
                init_path,
                snapshot,
            },
        );
    }

    /// Runaway watchdog (PRE-recv). While a Lua job is in flight and the
    /// runaway prompt hasn't been raised for the current window, arm a
    /// `LuaRunaway` deadline at `started_at + LUA_RUNAWAY_SOFT`; when that
    /// instant has passed, raise the "keep waiting? [y/N]" modal and mark the
    /// window prompted. Idle stays 0 dps: the deadline is armed ONLY while a
    /// job is in-flight + un-prompted. Returns whether a redraw is needed.
    pub(crate) fn settle_lua_runaway(&mut self, now_pre: Instant, ctx: &mut RunCtx) -> bool {
        let Some(inflight) = self.runtime.lua_inflight.as_ref() else {
            ctx.scheduler.disarm(Deadline::LuaRunaway);
            return false;
        };
        if inflight.prompted {
            ctx.scheduler.disarm(Deadline::LuaRunaway);
            return false;
        }
        let elapsed = now_pre.saturating_duration_since(inflight.started_at);
        if lua_runaway_due(inflight.prompted, elapsed, LUA_RUNAWAY_SOFT) {
            // Only raise the modal in Normal mode — never stomp an open prompt
            // (a confirm dialogue, the `:` line, …). Re-armed below so it
            // re-fires once the user is back in Normal mode.
            if matches!(self.state.mode, Mode::Normal) {
                let secs = elapsed.as_secs().max(1);
                let name = inflight.name.clone();
                self.runtime
                    .lua_inflight
                    .as_mut()
                    .expect("inflight present (checked above)")
                    .prompted = true;
                self.state.mode = Mode::Prompting(Prompt::simple(
                    PromptKind::LuaRunaway,
                    format!("lua '{name}' running {secs}s — keep waiting? [y/N] "),
                ));
                ctx.scheduler.disarm(Deadline::LuaRunaway);
                return true;
            }
            // A prompt is up: re-arm a short retry so the modal appears as soon
            // as the user clears it (rather than waiting another full window).
            ctx.scheduler
                .arm(Deadline::LuaRunaway, now_pre + LUA_RUNAWAY_SOFT);
            return false;
        }
        ctx.scheduler
            .arm(Deadline::LuaRunaway, inflight.started_at + LUA_RUNAWAY_SOFT);
        false
    }
}

impl App {
    /// Single-key confirm for the runaway "keep waiting? [y/N]" prompt. `y`/`Y`
    /// re-arms the watchdog window (so it re-prompts after another
    /// `LUA_RUNAWAY_SOFT` if the script is still running) and keeps waiting;
    /// the worker's hard ceiling (timed from the ORIGINAL job start, untouched
    /// here) remains the backstop. Anything else — `n`/`N`/Esc/Enter/a stray
    /// key — requests the abort: the worker's instruction hook unwinds the
    /// script at its next check, and the eventual `LuaDone` carries the abort
    /// error (suppressed by `handle_lua_done`, since we flash here + clear the
    /// inflight slot).
    pub(super) fn handle_lua_runaway_confirm_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        let keep_waiting = matches!(key.code, KeyCode::Char('y' | 'Y'));
        self.state.mode = Mode::Normal;
        self.view.needs_full_repaint = true;
        if keep_waiting {
            // Re-arm: reset the window so the prompt re-fires after another
            // soft threshold while the same job keeps running.
            if let Some(inflight) = self.runtime.lua_inflight.as_mut() {
                inflight.started_at = Instant::now();
                inflight.prompted = false;
            }
            return Vec::new();
        }
        // Abort the in-flight script (hook unwinds at its next check).
        let name = self
            .runtime
            .lua_inflight
            .as_ref()
            .map_or_else(|| "script".to_string(), |i| i.name.clone());
        if let Some(worker) = self.runtime.lua.as_ref() {
            worker.request_abort();
        }
        self.runtime.lua_inflight = None;
        self.state.flash_info(format!("lua: '{name}' aborted"));
        Vec::new()
    }
}

/// `:lua [status|on|off|reload]` — inspect, toggle, or reload the engine. The
/// panic button when a user's own config wedges things; `--no-lua` is the
/// startup equivalent. `reload` re-runs `init.lua` so edits take without `^R`.
pub(super) fn cmd_lua(app: &mut App, args: &str) -> Vec<Effect> {
    match args.trim() {
        "" | "status" => {
            let maps = app.runtime.lua_registry.maps.len();
            let cmds = app.runtime.lua_registry.commands.len();
            let state = if !crate::lua::enabled() {
                "disabled".to_string()
            } else if app.runtime.lua.as_ref().is_some_and(LuaWorker::is_busy) {
                "running a script".to_string()
            } else if app.runtime.lua.is_some() {
                format!("ready — {maps} map(s), {cmds} command(s)")
            } else {
                "enabled (not yet spawned)".to_string()
            };
            app.state.flash_info(format!("lua: {state}"));
        }
        "off" => {
            crate::lua::set_enabled(false);
            // Drop the worker: aborts any in-flight run and joins the thread.
            // The registries (now backed by dropped worker-side fns) go too,
            // along with the runaway-watchdog slot for the killed job.
            app.runtime.lua = None;
            app.runtime.lua_registry.clear();
            app.runtime.lua_inflight = None;
            app.state.flash_info("lua: disabled");
        }
        "on" => {
            crate::lua::set_enabled(true);
            app.state.flash_info("lua: enabled");
            // Re-load init.lua so `spyc.map`/`spyc.command` re-register after a
            // prior `:lua off` cleared them.
            app.load_init_lua();
        }
        "reload" => {
            app.reload_init_lua(true);
        }
        other => {
            app.state.flash_error(format!(
                "lua: unknown subcommand '{other}' (status|on|off|reload)"
            ));
        }
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    /// The pure runaway decision: prompt only once a job has run past the soft
    /// threshold and hasn't already been prompted for this window.
    #[test]
    fn lua_runaway_due_boundaries() {
        let soft = Duration::from_secs(1);
        // Below the threshold → not due, regardless of prompted.
        assert!(!lua_runaway_due(false, Duration::from_millis(999), soft));
        assert!(!lua_runaway_due(true, Duration::from_millis(999), soft));
        // Exactly at the threshold → due (>= boundary), un-prompted only.
        assert!(lua_runaway_due(false, Duration::from_secs(1), soft));
        assert!(!lua_runaway_due(true, Duration::from_secs(1), soft));
        // Past the threshold → due iff un-prompted.
        assert!(lua_runaway_due(false, Duration::from_secs(5), soft));
        assert!(!lua_runaway_due(true, Duration::from_secs(5), soft));
        // Zero elapsed → never due.
        assert!(!lua_runaway_due(false, Duration::ZERO, soft));
    }

    /// Regression: a `spyc.command`-registered `:`-command (from init.lua) must
    /// route through the App-layer Lua dispatch, NOT be swallowed by the pure
    /// `AppState::dispatch_command`'s "unknown command" arm. The pure half can't
    /// see the runtime Lua registry, so it flashed "unknown command" + returned
    /// `Handled`, short-circuiting the App layer — so init.lua `:`-commands
    /// reported "unknown command". `App::dispatch_command` now resolves a Lua
    /// command name (that isn't a built-in) before the pure short-circuit.
    #[test]
    fn lua_registered_command_is_not_reported_unknown() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            // Register a `:blame` command as init.lua's `spyc.command` would.
            app.runtime
                .lua_registry
                .commands
                .insert("blame".to_string(), 0);
            app.state.flash = None;
            let _ = app.dispatch_command("blame");
            // The test harness has no worker (no `pane_wake_tx`), so the Lua
            // path flashes "lua is disabled" — the point is it ROUTED to Lua,
            // not "unknown command".
            let unknown = app
                .state
                .flash
                .as_ref()
                .is_some_and(|f| f.text.contains("unknown command"));
            assert!(
                !unknown,
                "a registered Lua `:`-command must route to Lua, not flash 'unknown command'"
            );
        });
    }

    /// An un-prompted in-flight job whose window has elapsed raises the modal,
    /// marks the window prompted, and disarms the deadline.
    #[test]
    fn watchdog_raises_modal_after_soft_threshold() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            let now = Instant::now();
            app.runtime.lua_inflight = Some(LuaInflight {
                name: "slow".to_string(),
                // Started well past the soft threshold.
                started_at: now.checked_sub(Duration::from_secs(3)).unwrap(),
                prompted: false,
            });
            let mut ctx = RunCtx::for_test();
            let drew = app.settle_lua_runaway(now, &mut ctx);
            assert!(drew, "raising the modal needs a redraw");
            assert!(
                matches!(
                    &app.state.mode,
                    Mode::Prompting(p) if matches!(p.kind, PromptKind::LuaRunaway)
                ),
                "the runaway modal must be up"
            );
            assert!(
                app.runtime
                    .lua_inflight
                    .as_ref()
                    .is_some_and(|i| i.prompted),
                "the window must be marked prompted (raise at most once)"
            );
            // Modal up → the deadline is disarmed (re-armed on the user's
            // answer, not while the modal blocks).
            assert!(ctx.scheduler.next().is_none(), "deadline disarmed once up");
        });
    }

    /// No in-flight job → the watchdog is inert and disarms the deadline (idle
    /// stays 0 dps).
    #[test]
    fn watchdog_inert_without_inflight() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            let mut ctx = RunCtx::for_test();
            assert!(!app.settle_lua_runaway(Instant::now(), &mut ctx));
            assert!(
                ctx.scheduler.next().is_none(),
                "no job → LuaRunaway disarmed, nothing to wake for"
            );
        });
    }

    /// A young in-flight job arms the deadline at `started_at + soft` but does
    /// not raise the modal yet.
    #[test]
    fn watchdog_arms_but_waits_below_threshold() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            let now = Instant::now();
            app.runtime.lua_inflight = Some(LuaInflight {
                name: "fresh".to_string(),
                started_at: now,
                prompted: false,
            });
            let mut ctx = RunCtx::for_test();
            let drew = app.settle_lua_runaway(now, &mut ctx);
            assert!(!drew, "below threshold → no modal yet");
            assert!(matches!(app.state.mode, Mode::Normal), "no modal yet");
            assert!(
                ctx.scheduler.next().is_some(),
                "the soft-threshold deadline must be armed"
            );
        });
    }

    /// Answering `n` to the runaway prompt clears the inflight slot, flashes,
    /// and returns to Normal mode (the abort request is a no-op in the harness,
    /// where no worker is spawned, but the main-side bookkeeping still runs).
    #[test]
    fn answering_no_aborts_and_clears_inflight() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            app.runtime.lua_inflight = Some(LuaInflight {
                name: "loop".to_string(),
                started_at: Instant::now(),
                prompted: true,
            });
            app.state.mode = Mode::Prompting(Prompt::simple(
                PromptKind::LuaRunaway,
                "lua 'loop' running 1s — keep waiting? [y/N] ",
            ));
            let _ = app.handle_lua_runaway_confirm_key(key(KeyCode::Char('n')));
            assert!(matches!(app.state.mode, Mode::Normal), "n closes the modal");
            assert!(
                app.runtime.lua_inflight.is_none(),
                "n clears the inflight slot (the job is being aborted)"
            );
            assert_eq!(app.flash_text(), Some("lua: 'loop' aborted"));
        });
    }

    /// Answering `y` keeps waiting: the modal closes, the inflight slot
    /// survives, and the window is re-armed (prompted reset) so the prompt can
    /// re-fire after another soft threshold.
    #[test]
    fn answering_yes_rearms_and_keeps_waiting() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            app.runtime.lua_inflight = Some(LuaInflight {
                name: "slow".to_string(),
                started_at: Instant::now().checked_sub(Duration::from_secs(5)).unwrap(),
                prompted: true,
            });
            app.state.mode = Mode::Prompting(Prompt::simple(
                PromptKind::LuaRunaway,
                "lua 'slow' running 5s — keep waiting? [y/N] ",
            ));
            let _ = app.handle_lua_runaway_confirm_key(key(KeyCode::Char('y')));
            assert!(matches!(app.state.mode, Mode::Normal), "y closes the modal");
            let inflight = app
                .runtime
                .lua_inflight
                .as_ref()
                .expect("y keeps waiting → inflight survives");
            assert!(!inflight.prompted, "the window is re-armed for a re-prompt");
        });
    }

    /// When the job finishes after the user already aborted (inflight already
    /// cleared), the abort error the outcome carries is suppressed — it's not
    /// news. (`handle_lua_done` no-ops with no worker; this asserts the
    /// inflight-clearing contract via the watchdog instead.)
    #[test]
    fn abort_clears_inflight_so_done_suppresses_error() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            app.runtime.lua_inflight = Some(LuaInflight {
                name: "loop".to_string(),
                started_at: Instant::now(),
                prompted: true,
            });
            app.state.mode = Mode::Prompting(Prompt::simple(
                PromptKind::LuaRunaway,
                "lua 'loop' running 1s — keep waiting? [y/N] ",
            ));
            let _ = app.handle_lua_runaway_confirm_key(key(KeyCode::Char('n')));
            // The abort cleared the slot; a later `handle_lua_done` would see
            // `lua_inflight == None` and treat the outcome as a user-abort.
            assert!(app.runtime.lua_inflight.is_none());
        });
    }
}
