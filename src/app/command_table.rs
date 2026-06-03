//! The `:command` registry — the single source of truth for the colon-command
//! surface (names, dispatch layer, completion visibility).
//!
//! Lives in the `app` layer (not `state`) so a future [`CommandSpec`] can carry
//! an `App`-level handler fn-pointer without the Model depending on the `App`
//! aggregate. The Model (`AppState::dispatch_command`) consults only
//! [`command_layer`], whose signature is `App`-free, so the dependency stays
//! one-way (Model → this table's routing metadata; App → this table's handlers).
//!
//! Extracted from `app/state.rs` (MVU last-mile, Part A).

/// Which dispatch layer owns a named `:command`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmdLayer {
    /// Resolved entirely by `AppState::dispatch_command` (pure-domain —
    /// `Handled` / `OpenPager` / `Quit` / `Post`).
    Pure,
    /// `AppState::dispatch_command` returns `NotHandled`; the terminal-
    /// touching arm lives in `App::dispatch_command`.
    App,
}

/// A registered `:command` base name.
///
/// **THE single source of truth for the `:command` surface** (MVU Phase 6).
/// Replaces the former hand-synced trio that bred the "unknown command"
/// footgun (bit `:undo` in v1.41.1, `:limit`): the `NotHandled` allowlist in
/// [`AppState::dispatch_command`], the matchers in `App::dispatch_command`,
/// and the old `SPYC_COMMANDS` completion list. Now the `NotHandled` routing
/// and tab-completion both DERIVE from this table. `command_table_*` (tests)
/// catch the two failure modes reachable from `AppState`: a `Pure` entry whose
/// arm is missing (would flash "unknown command"), and a layer misclassified
/// vs how `dispatch_command` routes it. (The existence of an `App`-layer arm in
/// `App::dispatch_command` isn't unit-tested — that needs a live terminal — so
/// it stays covered by the manual smoke; the registry just guarantees the name
/// is *routed* to the App layer rather than silently dropped.)
///
/// Special prefixes `!` / `;` (free-form shell args) are intentionally absent:
/// they're symbol-dispatched in both layers and don't name-complete.
pub struct CommandSpec {
    pub name: &'static str,
    pub layer: CmdLayer,
    /// Offered in `:`-command tab-completion. (Every command is today; the
    /// flag lets a future internal-only command opt out without a 2nd list.)
    pub completion: bool,
}

const fn cmd(name: &'static str, layer: CmdLayer) -> CommandSpec {
    CommandSpec {
        name,
        layer,
        completion: true,
    }
}

/// The command registry. Sorted by `name` for deterministic completion
/// output (enforced by `command_table_is_sorted_and_unique`).
pub const COMMAND_TABLE: &[CommandSpec] = &[
    cmd("bnext", CmdLayer::App),
    cmd("bprev", CmdLayer::App),
    cmd("cd", CmdLayer::Pure),
    cmd("date", CmdLayer::App),
    cmd("dump-scrollback", CmdLayer::App),
    cmd("fg", CmdLayer::App),
    cmd("graveyard", CmdLayer::App),
    cmd("grep", CmdLayer::App),
    cmd("limit", CmdLayer::Pure),
    cmd("marks", CmdLayer::Pure),
    cmd("name", CmdLayer::Pure),
    cmd("pane-to-task", CmdLayer::App),
    cmd("pause", CmdLayer::App),
    cmd("project", CmdLayer::Pure),
    cmd("q", CmdLayer::Pure),
    cmd("quit", CmdLayer::Pure),
    cmd("resume", CmdLayer::App),
    cmd("set", CmdLayer::Pure),
    cmd("sort", CmdLayer::Pure),
    cmd("startdir", CmdLayer::Pure),
    cmd("task", CmdLayer::App),
    cmd("task-to-pane", CmdLayer::App),
    cmd("undo", CmdLayer::App),
    cmd("version", CmdLayer::Pure),
    cmd("whoami", CmdLayer::Pure),
];

/// The layer owning `name` (the first whitespace-delimited word of a typed
/// `:command`), or `None` if `name` is not registered.
pub fn command_layer(name: &str) -> Option<CmdLayer> {
    COMMAND_TABLE
        .iter()
        .find(|c| c.name == name)
        .map(|c| c.layer)
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
