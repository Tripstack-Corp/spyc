//! App-layer glue for the embedded Lua engine (`src/lua/`).
//!
//! Lazy-spawns the worker on first use, submits a script run with a context
//! snapshot, and — on the pre-recv drain ([`App::handle_lua_done`]) — translates
//! the [`LuaRequest`]s a finished script produced into spyc's existing
//! effect/action vocabulary. Lua never mutates the `App` directly; it only
//! enqueues requests, which this module applies on the main thread, so the MVU
//! contract holds (requests are data; the existing handlers run the effects).

use crate::keymap::user::BoundAction;
use crate::lua::{LuaJob, LuaOutcome, LuaRequest, LuaWorker};
use crate::mcp_cmd::{McpCommand, McpResponse};

use super::{App, Effect, Message};

/// Cap on how many times `spyc.action(name, count)` re-applies an action — the
/// requests run on the main thread, so an enormous count would block the loop.
const MAX_ACTION_REPEAT: u32 = 1_000;

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

    /// Trigger a `map KEY lua <name>` binding: submit a run of
    /// `<config_root>/lua/<name>.lua` with the current context snapshot.
    pub(super) fn apply_lua_binding(&mut self, name: &str) -> Vec<Effect> {
        if !self.ensure_lua_worker() {
            self.state
                .flash_error("lua is disabled (--no-lua / :lua off)");
            return Vec::new();
        }
        let Some(lua_dir) = crate::state::config_root().map(|r| r.join("lua")) else {
            self.state
                .flash_error("lua: no config dir ($HOME / $XDG_CONFIG_HOME unset)");
            return Vec::new();
        };
        // Best-effort serialize: refuse while a script is already running rather
        // than stacking runs when a key is mashed.
        if self.runtime.lua.as_ref().is_some_and(LuaWorker::is_busy) {
            self.state.flash_error("lua: a script is already running");
            return Vec::new();
        }
        let snapshot = self.snapshot_context();
        if let Some(worker) = self.runtime.lua.as_ref()
            && let Err(e) = worker.submit(LuaJob::RunFile {
                name: name.to_string(),
                lua_dir,
                snapshot,
            })
        {
            self.state.flash_error(format!("lua: {e}"));
        }
        Vec::new()
    }

    /// Drain finished Lua runs (pre-recv scan). Returns `(needs_draw, effects)`,
    /// mirroring `apply_file_outcomes`: an error flashes; a failed/aborted run
    /// applied nothing; a successful run's requests translate to effects/actions.
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
            for req in outcome.requests {
                effects.extend(self.apply_lua_request(req));
            }
        }
        (true, effects)
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
}

/// `:lua [status|on|off]` — inspect or toggle the engine. The panic button when
/// a user's own config wedges things; `--no-lua` is the startup equivalent.
pub(super) fn cmd_lua(app: &mut App, args: &str) -> Vec<Effect> {
    match args.trim() {
        "" | "status" => {
            let state = if !crate::lua::enabled() {
                "disabled"
            } else if app.runtime.lua.as_ref().is_some_and(LuaWorker::is_busy) {
                "running a script"
            } else if app.runtime.lua.is_some() {
                "ready"
            } else {
                "enabled (not yet spawned)"
            };
            app.state.flash_info(format!("lua: {state}"));
        }
        "off" => {
            crate::lua::set_enabled(false);
            // Drop the worker: aborts any in-flight run and joins the thread.
            app.runtime.lua = None;
            app.state.flash_info("lua: disabled");
        }
        "on" => {
            crate::lua::set_enabled(true);
            app.state.flash_info("lua: enabled");
        }
        other => {
            app.state
                .flash_error(format!("lua: unknown subcommand '{other}' (status|on|off)"));
        }
    }
    Vec::new()
}
