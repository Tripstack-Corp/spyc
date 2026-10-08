//! `spyc -c <cmd>`: `:` commands from the command line, run in order once
//! startup has settled — `init.lua` loaded, a `-r` session picked, and every
//! startup question (status hooks, project tabs, the skill update) answered.
//! Run before then, a restore would overwrite what they did, and a prompt
//! would take keys meant for it.

use crate::app::{App, Effect, Mode};
use crate::lua::LuaWorker;

/// Whether startup has settled enough for `spyc -c` commands to run.
pub(super) const fn startup_has_settled(
    lua_loading: bool,
    picking_session: bool,
    prompting: bool,
) -> bool {
    !lua_loading && !picking_session && !prompting
}

impl App {
    /// Queue `spyc -c` commands; [`Self::settle_startup_commands`] runs them.
    /// A leading `:` is optional, as typed at the prompt.
    pub fn queue_startup_commands(&mut self, commands: Vec<String>) {
        self.state.startup_commands = commands;
    }

    /// Run the queued `spyc -c` commands once startup has settled, each as if
    /// typed at `:`. Called at loop bottom; `Some` with their effects when
    /// they ran, since one can change the frame without returning any.
    pub(super) fn settle_startup_commands(&mut self) -> Option<Vec<Effect>> {
        if self.state.startup_commands.is_empty() {
            return None;
        }
        // The load, and the `startup` handlers it fires as it lands.
        let lua_loading = self.runtime.lua_events.startup_pending
            || self.runtime.lua_inflight.is_some()
            || self.runtime.lua.as_ref().is_some_and(LuaWorker::is_busy);
        let settled = startup_has_settled(
            lua_loading,
            self.state.pending_sessions.is_some(),
            matches!(self.state.mode, Mode::Prompting(_)),
        );
        if !settled {
            return None;
        }
        let mut effects = Vec::new();
        for command in std::mem::take(&mut self.state.startup_commands) {
            let command = command.trim();
            effects.extend(self.dispatch_command(command.strip_prefix(':').unwrap_or(command)));
        }
        Some(effects)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::listing::SortMode;

    fn app_with(commands: &[&str]) -> (tempfile::TempDir, App) {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = App::test_app(tmp.path().to_path_buf());
        app.queue_startup_commands(commands.iter().map(ToString::to_string).collect());
        (tmp, app)
    }

    #[test]
    fn startup_commands_run_in_order_once_startup_settles() {
        let (tmp, mut app) = app_with(&["sort size", ":sort mtime"]);
        crate::state::with_state_root(tmp.path(), || {
            let _ = app.settle_startup_commands();
            assert_eq!(
                app.state.cur().sort_order,
                SortMode::Mtime,
                "the last one wins, its `:` dropped"
            );
            assert!(app.state.startup_commands.is_empty(), "each runs once");
        });
    }

    #[test]
    fn startup_commands_wait_for_the_session_picker() {
        let (tmp, mut app) = app_with(&["sort mtime"]);
        crate::state::with_state_root(tmp.path(), || {
            app.state.pending_sessions = Some(Vec::new());
            let _ = app.settle_startup_commands();
            assert_eq!(
                app.state.cur().sort_order,
                SortMode::Name,
                "not under the picker"
            );

            app.state.pending_sessions = None;
            let _ = app.settle_startup_commands();
            assert_eq!(app.state.cur().sort_order, SortMode::Mtime);
        });
    }

    #[test]
    fn startup_commands_wait_for_an_open_prompt() {
        let (tmp, mut app) = app_with(&["sort mtime"]);
        crate::state::with_state_root(tmp.path(), || {
            app.state.mode = Mode::Prompting(crate::app::Prompt::simple(
                crate::app::PromptKind::ClosePane,
                "close running tab? [y/N] ",
            ));
            let _ = app.settle_startup_commands();
            assert_eq!(
                app.state.cur().sort_order,
                SortMode::Name,
                "not under a prompt"
            );

            app.state.mode = Mode::Normal;
            let _ = app.settle_startup_commands();
            assert_eq!(app.state.cur().sort_order, SortMode::Mtime);
        });
    }

    #[test]
    fn startup_commands_wait_for_init_lua() {
        let (tmp, mut app) = app_with(&["sort mtime"]);
        crate::state::with_state_root(tmp.path(), || {
            app.runtime.lua_events.startup_pending = true;
            let _ = app.settle_startup_commands();
            assert_eq!(
                app.state.cur().sort_order,
                SortMode::Name,
                "not while init.lua loads"
            );

            app.runtime.lua_events.startup_pending = false;
            let _ = app.settle_startup_commands();
            assert_eq!(app.state.cur().sort_order, SortMode::Mtime);
        });
    }

    /// The loop is what runs them: `main` queues the flag, and the loop bottom
    /// settles the queue.
    #[test]
    fn the_flag_reaches_the_loop() {
        let lib = crate::guard_support::production_half(include_str!("../lib.rs"));
        assert!(lib.contains("app.queue_startup_commands(cli.cmd)"));
        let run = crate::guard_support::production_half(include_str!("pre_recv.rs"));
        assert!(run.contains("self.settle_startup_commands()"));
    }

    #[test]
    fn settled_needs_all_three() {
        assert!(startup_has_settled(false, false, false));
        assert!(!startup_has_settled(true, false, false));
        assert!(!startup_has_settled(false, true, false));
        assert!(!startup_has_settled(false, false, true));
    }
}
