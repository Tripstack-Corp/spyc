//! `^a F` — fork the active tab: its conversation carries on in a new tab as a
//! branch, and the original line of inquiry stays where it was.
//!
//! What a fork is depends on the agent ([`ForkMode`]). claude and codex branch
//! into a new session that starts from the old one's history, which each agent
//! replays on screen and which `^a v` reads back. A tab with no conversation,
//! such as a shell, forks into a copy. agy and zot can resume a conversation but
//! not branch one, so `^a F` says so instead of opening the same one twice.

use std::collections::HashSet;
use std::path::PathBuf;

use super::App;
use crate::agent::ForkMode;
use crate::pane::TabInfo;

/// The command a fork is built from, and the directory it starts in: the tab's
/// own command and cwd when it launched the agent it runs, or the agent's own
/// program and the shell's directory when a shell is running it (#544), so the
/// fork opens as a tab of that agent rather than a shell handed its flags. An
/// agent finds its sessions by the directory it started in, which a shell
/// keeps while the agent runs.
fn fork_base(info: &TabInfo, live_cwd: PathBuf) -> (String, PathBuf) {
    let agent = info.agent();
    if info.launched_agent().kind() == agent.kind() {
        (info.command.clone(), info.cwd.clone())
    } else {
        (agent.binary().to_string(), live_cwd)
    }
}

/// Whether `^a F` forks a tab running what `info` names at all: the kinds
/// [`App::fork_active_tab`] refuses before it looks for a conversation.
pub(super) fn forks(info: &TabInfo) -> bool {
    match info.agent().fork_mode() {
        ForkMode::Branch(_) => !info.wraps_agent(),
        ForkMode::Duplicate => true,
        ForkMode::Unsupported => false,
    }
}

impl App {
    pub fn fork_active_tab(&mut self) {
        let Some(tabs) = self.runtime.project().pane_tabs.as_ref() else {
            return;
        };
        let active = tabs.active_index();
        let entry = &tabs.tabs()[active];
        let profile = entry.info.agent();
        let home = entry.info.codex_home.clone();
        if profile.kind() == crate::state::sessions::AgentKind::Codex && home.is_none() {
            self.state
                .flash_info("fork: Codex launch home is not identified yet");
            return;
        }
        let (command, cwd) = match profile.fork_mode() {
            ForkMode::Branch(_) if entry.info.wraps_agent() => {
                self.state.flash_info(format!(
                    "fork: this tab runs {} through its own command, and spyc adds no flags to a wrapper",
                    profile.name()
                ));
                return;
            }
            ForkMode::Branch(fork) => {
                // The other tabs' conversations are theirs, so the resolver's
                // fallbacks must not hand this fork one of them. This tab's own
                // pin stays usable when another tab runs it too.
                let own = entry.info.pinned_session_id();
                let claimed: HashSet<String> = tabs
                    .tabs()
                    .iter()
                    .filter(|t| t.info.codex_home == home)
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
                let (base, cwd) = fork_base(&entry.info, entry.live_cwd());
                let command = match fork(&base, &sid) {
                    Ok(command) => command,
                    Err(error) => {
                        self.state.flash_error(format!("fork: {error:#}"));
                        return;
                    }
                };
                (command, cwd)
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
        if self.open_pane_tab_in_home(&command, &cwd, home.as_deref()) {
            let forked = self
                .runtime
                .project()
                .pane_tabs
                .as_ref()
                .map_or(0, |tabs| tabs.tabs().len());
            self.state
                .flash_info(format!("forked tab {} into tab {forked}", active + 1));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fork_base;
    use crate::pane::{RunningAgent, TabInfo};
    use crate::state::sessions::AgentKind;
    use std::path::PathBuf;

    const fn running(kind: AgentKind) -> RunningAgent {
        RunningAgent {
            kind,
            pid: 1,
            started_epoch_secs: 1,
            backgrounded: false,
            on_shared_daemon: false,
        }
    }

    /// A tab launched as its agent forks from its own command, in its own cwd.
    #[test]
    fn a_launched_agent_forks_from_its_command() {
        let info = TabInfo::new("claude --model opus", "/w/app");
        assert_eq!(
            fork_base(&info, PathBuf::from("/elsewhere")),
            ("claude --model opus".to_string(), PathBuf::from("/w/app"))
        );
    }

    /// A shell running an agent forks into that agent, from its own program
    /// and the directory it started in; the shell's command would have been
    /// handed `--resume … --fork-session` (`zsh --resume …`).
    #[test]
    fn a_shell_running_an_agent_forks_into_that_agent() {
        let mut info = TabInfo::new("zsh", "/w");
        info.running = Some(running(AgentKind::Claude));
        assert_eq!(
            fork_base(&info, PathBuf::from("/w/app")),
            ("claude".to_string(), PathBuf::from("/w/app"))
        );
        info.running = Some(running(AgentKind::Codex));
        assert_eq!(fork_base(&info, PathBuf::from("/w/app")).0, "codex");
    }
}
