//! The `:command` registry — the single source of truth for the colon-command
//! surface (names, executable handler, completion visibility).
//!
//! Lives in the `app` layer (not `state`) because a [`CommandSpec`] carries an
//! `App`-level handler fn-pointer ([`CmdHandler::App`]) for the terminal-
//! touching commands. The Model (`AppState::dispatch_command`) consults only
//! [`command_layer`], whose signature is `App`-free, so the dependency stays
//! one-way (Model → routing metadata; App → handlers).
//!
//! **The compile-time guarantee:** a `CmdHandler::App` entry can't be
//! constructed without naming its handler fn, so a registered command with no
//! implementation is a *build error* — not the runtime "unknown command" flash
//! it used to be (which bit `:undo` in v1.41.1).
//!
//! Extracted from `app/state.rs` (MVU last-mile, Part A).

use super::{App, Effect, commands};

/// Which dispatch layer owns a named `:command`. An `App`-free routing
/// discriminant consulted by the Model (`AppState::dispatch_command`); the
/// concrete behavior lives in [`CmdHandler`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmdLayer {
    /// Resolved entirely by `AppState::dispatch_command` (pure-domain —
    /// `Handled` / `OpenPager` / `Quit` / `Post`).
    Pure,
    /// `AppState::dispatch_command` returns `NotHandled`; the terminal-
    /// touching handler runs at the App layer ([`CmdHandler::App`]).
    App,
}

/// The executable handler for a registered `:command`.
pub enum CmdHandler {
    /// Handled by `AppState::dispatch_command` (the pure method). Those arms
    /// are already test-guarded by `command_table_dispatches_without_unknown`,
    /// so no fn-pointer is needed here.
    Pure,
    /// Handled by this fn at the App layer. Receives the trimmed argument
    /// string (everything after the command name); returns the effects to run.
    App(fn(&mut App, &str) -> Vec<Effect>),
}

impl CmdHandler {
    const fn layer(&self) -> CmdLayer {
        match self {
            Self::Pure => CmdLayer::Pure,
            Self::App(_) => CmdLayer::App,
        }
    }
}

/// A registered `:command` base name.
///
/// **THE single source of truth for the `:command` surface** (MVU Phase 6).
/// Replaces the former hand-synced trio that bred the "unknown command"
/// footgun (bit `:undo` in v1.41.1, `:limit`): the `NotHandled` allowlist in
/// [`AppState::dispatch_command`], the matchers in `App::dispatch_command`,
/// and the old `SPYC_COMMANDS` completion list. Now the routing, the App-layer
/// handlers, and tab-completion all DERIVE from this table. With the handler
/// fn-pointer carried inline, a missing App-layer arm is a compile error.
///
/// Special prefixes `!` / `;` (free-form shell args) are intentionally absent:
/// they're symbol-dispatched in both layers and don't name-complete.
pub struct CommandSpec {
    pub name: &'static str,
    pub handler: CmdHandler,
    /// Offered in `:`-command tab-completion. (Every command is today; the
    /// flag lets a future internal-only command opt out without a 2nd list.)
    pub completion: bool,
}

impl CommandSpec {
    /// The dispatch layer owning this command (derived from its handler).
    pub const fn layer(&self) -> CmdLayer {
        self.handler.layer()
    }
}

const fn pure(name: &'static str) -> CommandSpec {
    CommandSpec {
        name,
        handler: CmdHandler::Pure,
        completion: true,
    }
}

const fn app(name: &'static str, handler: fn(&mut App, &str) -> Vec<Effect>) -> CommandSpec {
    CommandSpec {
        name,
        handler: CmdHandler::App(handler),
        completion: true,
    }
}

/// The command registry. Sorted by `name` for deterministic completion
/// output (enforced by `command_table_is_sorted_and_unique`).
pub const COMMAND_TABLE: &[CommandSpec] = &[
    app("activity", commands::cmd_activity),
    app("bnext", commands::cmd_bnext),
    app("bprev", commands::cmd_bprev),
    pure("cd"),
    app("chmod", commands::cmd_chmod),
    app("date", commands::cmd_date),
    app("dump-scrollback", commands::cmd_dump_scrollback),
    app("fg", commands::cmd_fg),
    app("filetype", commands::cmd_filetype),
    app("graveyard", commands::cmd_graveyard),
    app("grep", commands::cmd_grep),
    pure("limit"),
    app("longlist", commands::cmd_longlist),
    pure("marks"),
    pure("name"),
    app("pane-to-task", commands::cmd_pane_to_task),
    app("pause", commands::cmd_pause),
    pure("project"),
    pure("q"),
    pure("quit"),
    app("resume", commands::cmd_resume),
    pure("set"),
    pure("sort"),
    pure("startdir"),
    app("task", commands::cmd_task),
    app("task-to-pane", commands::cmd_task_to_pane),
    app("undo", commands::cmd_undo),
    pure("version"),
    pure("whoami"),
    app("why-status", commands::cmd_why_status),
];

/// The registered spec for `name` (the first whitespace-delimited word of a
/// typed `:command`), or `None` if `name` is not registered.
pub fn lookup(name: &str) -> Option<&'static CommandSpec> {
    COMMAND_TABLE.iter().find(|c| c.name == name)
}

/// The layer owning `name`, or `None` if `name` is not registered.
pub fn command_layer(name: &str) -> Option<CmdLayer> {
    lookup(name).map(CommandSpec::layer)
}

/// Split a trimmed `:command` into `(name, args)`: the command name up to the
/// first ASCII space and the trimmed remainder (`""` when there are no args).
///
/// Splits on a literal space — not arbitrary whitespace — to exactly preserve
/// the former `strip_prefix("name ")` matching: a tab-separated `fg\t3` is one
/// unregistered token (→ "unknown command"), not `fg` + `3`.
pub fn split_name_args(input: &str) -> (&str, &str) {
    match input.split_once(' ') {
        Some((name, rest)) => (name, rest.trim()),
        None => (input, ""),
    }
}

/// Completion-visible command names, in table (sorted) order.
pub fn completion_command_names() -> impl Iterator<Item = &'static str> {
    COMMAND_TABLE
        .iter()
        .filter(|c| c.completion)
        .map(|c| c.name)
}

#[cfg(test)]
mod tests {
    #[test]
    fn command_table_is_sorted_and_unique() {
        let names: Vec<&str> = super::COMMAND_TABLE.iter().map(|c| c.name).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(
            sorted, names,
            "COMMAND_TABLE must be sorted by name and free of duplicates so \
             tab-completion produces deterministic output",
        );
    }
}
