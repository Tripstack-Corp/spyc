//! The Claude Code home a Claude tab's sessions and transcripts are read from
//! (#917): the one its status hook reported, else the one its launch record
//! names, else spyc's own, its `CLAUDE_CONFIG_DIR` else `~/.claude`, read once
//! at startup (`bootstrap::initial_pane`). Every lookup in `state::sessions`
//! takes it as a parameter, so none reads the environment.

use std::path::PathBuf;

use super::App;
use crate::agent::claude_home::Lookup;
use crate::pane::tabs::TabEntry;

impl App {
    /// spyc's own Claude home, which a tab that names none of its own reads.
    pub(super) fn own_claude_home(&self) -> Option<PathBuf> {
        self.state
            .active_project()
            .pane
            .agent_hud
            .claude_home
            .clone()
    }

    /// What decides `entry`'s Claude home. Reads nothing: resolve it in a
    /// worker, or where the caller reads session files anyway.
    pub(super) fn claude_home_lookup(&self, entry: &TabEntry) -> Lookup {
        lookup_for(entry, self.own_claude_home())
    }

    /// [`Self::claude_home_lookup`] for the active tab; spyc's own without one.
    pub(super) fn active_claude_home_lookup(&self) -> Lookup {
        let tabs = self.runtime.project().pane_tabs.as_ref();
        tabs.and_then(|tabs| tabs.tabs().get(tabs.active_index()))
            .map_or_else(
                || Lookup {
                    default: self.own_claude_home(),
                    ..Lookup::default()
                },
                |entry| self.claude_home_lookup(entry),
            )
    }
}

/// [`App::claude_home_lookup`], for a caller holding the tabs mutably.
pub(super) fn lookup_for(entry: &TabEntry, default: Option<PathBuf>) -> Lookup {
    Lookup {
        reported: entry.info.claude_home.clone(),
        launch_record: entry
            .pane
            .host
            .launch_environment_path()
            .map(std::path::Path::to_path_buf),
        default,
    }
}
