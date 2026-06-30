//! App-layer glue for the embedded Lua engine (`src/lua/`).
//!
//! Lazy-spawns the worker on first use, submits a script run with a context
//! snapshot, and — on the pre-recv drain ([`App::handle_lua_done`]) — translates
//! the [`LuaRequest`]s a finished script produced into spyc's existing
//! effect/action vocabulary. Lua never mutates the `App` directly; it only
//! enqueues requests, which this module applies on the main thread, so the MVU
//! contract holds (requests are data; the existing handlers run the effects).

use std::collections::HashMap;

use crate::keymap::user::{BoundAction, UserBinding};
use crate::lua::{LuaJob, LuaOutcome, LuaRequest, LuaWorker, RegKind, Registration};
use crate::mcp_cmd::{McpCommand, McpResponse};

use super::{App, Effect, Message};

/// Cap on how many times `spyc.action(name, count)` re-applies an action — the
/// requests run on the main thread, so an enormous count would block the loop.
const MAX_ACTION_REPEAT: u32 = 1_000;

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
        self.submit_lua_job(LuaJob::RunFile {
            name: name.to_string(),
            lua_dir,
            snapshot,
        });
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
        self.submit_lua_job(LuaJob::RunRegistered { fn_id, snapshot });
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
        self.submit_lua_job(LuaJob::RunRegistered { fn_id, snapshot });
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

    /// Submit `job` to the worker, flashing on a send error (worker gone).
    fn submit_lua_job(&mut self, job: LuaJob) {
        if let Some(worker) = self.runtime.lua.as_ref()
            && let Err(e) = worker.submit(job)
        {
            self.state.flash_error(format!("lua: {e}"));
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
        let mut effects = Vec::new();
        for outcome in outcomes {
            if let Some(err) = outcome.error {
                self.state.flash_error(err);
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
        self.submit_lua_job(LuaJob::Load {
            init_path,
            snapshot,
        });
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
        self.submit_lua_job(LuaJob::Reload {
            init_path,
            snapshot,
        });
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
            // The registries (now backed by dropped worker-side fns) go too.
            app.runtime.lua = None;
            app.runtime.lua_registry.clear();
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
