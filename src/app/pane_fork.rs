//! `^a F` — fork the active tab: its conversation carries on in a new tab as a
//! branch, and the original line of inquiry stays where it was.
//!
//! What a fork is depends on the agent ([`ForkMode`]). claude and codex branch
//! into a new session that starts from the old one's history, which each agent
//! replays on screen and which `^a v` reads back. A tab with no conversation,
//! such as a shell, forks into a copy. agy and zot can resume a conversation but
//! not branch one, so `^a F` says so instead of opening the same one twice.

use std::collections::HashSet;

use super::App;
use crate::agent::ForkMode;

impl App {
    pub fn fork_active_tab(&mut self) {
        let Some(tabs) = self.runtime.pane_tabs.as_ref() else {
            return;
        };
        let active = tabs.active_index();
        let entry = &tabs.tabs()[active];
        let profile = crate::agent::detect(&entry.info.command);
        let (command, cwd) = match profile.fork_mode() {
            ForkMode::Branch(fork) => {
                // The other tabs' conversations are theirs, so the resolver's
                // fallbacks must not hand this fork one of them. This tab's own
                // pin stays usable when another tab runs it too.
                let own = entry.info.pinned_session_id();
                let claimed: HashSet<String> = tabs
                    .tabs()
                    .iter()
                    .filter_map(|t| t.info.pinned_session_id())
                    .filter(|&id| Some(id) != own)
                    .map(str::to_owned)
                    .collect();
                let (Some(sid), _) = super::session::tab_conversation(entry, &claimed) else {
                    self.state.flash_info(format!(
                        "fork: nothing to branch yet — this {} tab has no saved conversation",
                        profile.name()
                    ));
                    return;
                };
                // The agent finds a session by the directory it was started in,
                // which its own cwd never leaves.
                (fork(&entry.info.command, &sid), entry.info.cwd.clone())
            }
            ForkMode::Duplicate => (entry.info.command.clone(), entry.live_cwd()),
            ForkMode::Unsupported => {
                self.state.flash_info(format!(
                    "fork: {} can't branch a conversation, only resume it in place",
                    profile.name()
                ));
                return;
            }
        };
        if self.open_pane_tab_in(&command, &cwd) {
            let forked = self
                .runtime
                .pane_tabs
                .as_ref()
                .map_or(0, |tabs| tabs.tabs().len());
            self.state
                .flash_info(format!("forked tab {} into tab {forked}", active + 1));
        }
    }
}
