//! Agents launched inside shell tabs (#544). A shell tab carries the agent env
//! (`SPYC_MCP_SOCK` + `SPYC_PANE_ID`), so an agent typed into it can reach spyc
//! and name the tab. And because spyc learns which agent runs only after it has
//! started, while agents read their MCP config (and codex and agy their hooks)
//! at startup, a shell tab in a project that has consented to status hooks gets
//! every supported agent's config written up front.

use std::time::{Duration, Instant};

use super::scheduler::Deadline;
use super::{App, RunCtx, state};
use crate::pane::{RunningAgent, TabInfo};

/// How long after a shell tab's output to look at its foreground: long enough
/// for a program the user just launched to have started.
const SETTLE: Duration = Duration::from_millis(300);

/// The least time between two looks at one tab while its output streams. An
/// agent's TUI redraws constantly, so looking on every burst would cost a scan
/// per frame; this costs at most one a second, and none while the tab is idle.
const MIN_INTERVAL: Duration = Duration::from_secs(1);

/// Output in a shell tab may mean it started, stopped or ended an agent: look
/// at its foreground `SETTLE` from now, but not within `MIN_INTERVAL` of the
/// last look. A look already scheduled stands.
pub(super) fn schedule_look(info: &mut TabInfo, now: Instant) {
    if info.fg_check_due.is_none() {
        let earliest = info.fg_checked_at.map_or(now, |at| at + MIN_INTERVAL);
        info.fg_check_due = Some((now + SETTLE).max(earliest));
    }
}

/// What the tab's pty foreground holds, and whether `running`'s process still
/// exists. The shell is its own group's leader, so a foreground group other
/// than the shell's pid is a job it is running.
fn look(pane: &crate::pane::Pane, running: Option<RunningAgent>) -> (Seen, bool) {
    let seen = match (pane.process_id(), pane.foreground_pgrp()) {
        (Some(shell), Some(fg)) if fg != shell => seen(&crate::proc_group::members(fg)),
        _ => Seen::Shell,
    };
    let alive = running.is_some_and(|r| crate::proc_group::alive(r.pid));
    (seen, alive)
}

/// Everything a tab learned about an agent that has gone or been replaced: its
/// reports and scrape, its activity, and its session. None of it describes
/// the next one.
fn forget_agent(info: &mut TabInfo) {
    info.reported = None;
    info.last_reported = None;
    info.last_report_ignored = None;
    info.recent_hook_events.clear();
    info.scrape_status = None;
    info.scrape_dirty = false;
    info.scrape_pending_at = None;
    info.activity = crate::pane::AgentActivity::Unknown;
    info.notified = crate::pane::AgentActivity::Unknown;
    info.live_session_id = None;
    info.codex_session_id = None;
    info.codex_home = None;
    info.pending_resume_send = None;
    info.status_hooks_restart_needed = false;
}

impl App {
    /// Look at every shell tab whose look is due, and apply what changed.
    /// Armed only while a look is pending, so an idle spyc does no work.
    /// `&mut` settle step, PRE-recv. Returns whether any tab changed.
    pub(crate) fn settle_shell_agents(&mut self, now: Instant, ctx: &mut RunCtx) -> bool {
        let mut changes = Vec::new();
        let mut next: Option<Instant> = None;
        if let Some(tabs) = self.runtime.pane_tabs.as_mut() {
            for (idx, entry) in tabs.tabs_mut().iter_mut().enumerate() {
                match entry.info.fg_check_due {
                    Some(due) if due <= now => {
                        entry.info.fg_check_due = None;
                        entry.info.fg_checked_at = Some(now);
                        let (seen, alive) = look(&entry.pane, entry.info.running);
                        changes.push((idx, transition(entry.info.running, seen, alive)));
                    }
                    Some(due) => next = Some(next.map_or(due, |n| n.min(due))),
                    None => {}
                }
            }
        }
        match next {
            Some(at) => ctx.scheduler.arm(Deadline::ShellAgentLook, at),
            None => ctx.scheduler.disarm(Deadline::ShellAgentLook),
        }
        let mut changed = false;
        for (idx, change) in changes {
            changed |= self.apply_shell_agent_change(idx, change);
        }
        changed
    }

    fn apply_shell_agent_change(&mut self, idx: usize, change: Change) -> bool {
        let Some(info) = self
            .runtime
            .pane_tabs
            .as_mut()
            .and_then(|tabs| tabs.tabs_mut().get_mut(idx))
            .map(|entry| &mut entry.info)
        else {
            return false;
        };
        match change {
            Change::Unchanged => false,
            Change::Backgrounded | Change::Foregrounded => {
                if let Some(running) = info.running.as_mut() {
                    running.backgrounded = change == Change::Backgrounded;
                }
                true
            }
            Change::Exited => {
                forget_agent(info);
                info.running = None;
                let id = info.id.clone();
                self.state.codex_recovery.remove(&id);
                true
            }
            Change::Started(running) => {
                forget_agent(info);
                info.running = Some(running);
                let cwd = info.cwd.clone();
                info.status_hooks_at_spawn = crate::agent::profile_for(running.kind)
                    .status_hooks()
                    .is_some_and(|support| support.installed(&cwd));
                let id = info.id.clone();
                self.state.codex_recovery.remove(&id);
                // As a launched agent tab does: install where consented, ask
                // where the project hasn't answered.
                self.maybe_offer_status_hooks_for(running.kind, &cwd);
                true
            }
        }
    }

    /// Write, for each agent spyc supports, what it reads at startup: its MCP
    /// entry and its status hooks, each recorded for cleanup on exit exactly as
    /// an agent tab's are. Only where that agent's hook root has consented; a
    /// shell tab anywhere else writes nothing. Quiet, because opening a shell
    /// is not launching an agent.
    pub(super) fn provision_shell_tab(&mut self, cwd: &std::path::Path) {
        if !self.view.mcp_running {
            return;
        }
        for profile in crate::agent::REGISTRY {
            let Some(support) = profile.status_hooks() else {
                continue;
            };
            let dir = (support.config_dir)(cwd);
            let root = state::find_repo_root(&dir).unwrap_or_else(|| dir.clone());
            if crate::state::hook_consent::consent_for(&root) != Some(true) {
                continue;
            }
            self.write_mcp_config(profile.kind(), cwd, false);
            self.install_status_hooks(cwd, profile.kind());
        }
    }
}

/// Programs that run a script named for what it does: `node …/codex.js` is
/// codex, though the process is node.
const INTERPRETERS: &[&str] = &[
    "node", "bun", "deno", "python", "python3", "ruby", "perl", "sh", "bash", "zsh", "dash",
];

/// The names a group member answers to: its argv[0] and kernel name, and for
/// an interpreter the script it runs, without its extension.
fn member_names(member: &crate::proc_group::Member) -> Vec<String> {
    let base = |s: &str| {
        std::path::Path::new(s)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let mut names = vec![base(&member.comm)];
    if let Some(argv0) = member.argv.first() {
        let program = base(argv0);
        if INTERPRETERS.contains(&program.as_str())
            && let Some(script) = member.argv.get(1).filter(|a| !a.starts_with('-'))
        {
            let script = base(script);
            let stem = script.split('.').next().unwrap_or(&script).to_string();
            names.push(stem);
        }
        names.push(program);
    }
    names
}

/// Which member of a foreground group is a known agent, and which agent. When
/// several are (an agent running another as a tool), the earliest started is
/// the one the user ran. Printed text plays no part: only what the processes are.
pub(super) fn identify(
    members: &[crate::proc_group::Member],
) -> Option<(
    crate::state::sessions::AgentKind,
    &crate::proc_group::Member,
)> {
    members
        .iter()
        .filter_map(|member| {
            let names = member_names(member);
            crate::agent::REGISTRY
                .iter()
                .find(|profile| names.iter().any(|name| name == profile.binary()))
                .map(|profile| (profile.kind(), member))
        })
        .min_by_key(|(_, member)| (member.started_epoch_secs, member.pid))
}

/// What a foreground group holds: the agent `identify` finds, and for codex
/// whether it was started without `--no-daemon`.
pub(super) fn seen(members: &[crate::proc_group::Member]) -> Seen {
    identify(members).map_or(Seen::Shell, |(kind, member)| Seen::Agent {
        kind,
        pid: member.pid,
        started_epoch_secs: member.started_epoch_secs,
        on_shared_daemon: kind == crate::state::sessions::AgentKind::Codex
            && crate::agent::codex_may_use_daemon(&member.argv),
    })
}

/// What a shell tab's foreground holds when it is looked at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Seen {
    /// The shell itself, or a program that isn't an agent.
    Shell,
    Agent {
        kind: crate::state::sessions::AgentKind,
        pid: u32,
        started_epoch_secs: u64,
        on_shared_daemon: bool,
    },
}

/// How a shell tab's agent changed between two looks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Change {
    Unchanged,
    /// A new agent is in the foreground, replacing any earlier one.
    Started(crate::pane::RunningAgent),
    /// The agent is gone.
    Exited,
    /// The agent left the foreground without exiting (`^Z`, `bg`).
    Backgrounded,
    /// A backgrounded agent is back (`fg`).
    Foregrounded,
}

/// `prev` is the agent last seen, `seen` what the foreground holds now, and
/// `prev_alive` whether `prev`'s process still exists: an agent that left the
/// foreground and still exists was backgrounded, not exited.
pub(super) const fn transition(
    prev: Option<crate::pane::RunningAgent>,
    seen: Seen,
    prev_alive: bool,
) -> Change {
    match (prev, seen) {
        (None, Seen::Shell) => Change::Unchanged,
        (Some(prev), Seen::Agent { pid, .. }) if prev.pid == pid => {
            if prev.backgrounded {
                Change::Foregrounded
            } else {
                Change::Unchanged
            }
        }
        (
            _,
            Seen::Agent {
                kind,
                pid,
                started_epoch_secs,
                on_shared_daemon,
            },
        ) => Change::Started(crate::pane::RunningAgent {
            kind,
            pid,
            started_epoch_secs,
            backgrounded: false,
            on_shared_daemon,
        }),
        (Some(prev), Seen::Shell) if prev_alive => {
            if prev.backgrounded {
                Change::Unchanged
            } else {
                Change::Backgrounded
            }
        }
        (Some(_), Seen::Shell) => Change::Exited,
    }
}

#[cfg(test)]
mod tests {
    use super::{Change, Seen, identify, transition};
    use crate::pane::RunningAgent;
    use crate::proc_group::Member;
    use crate::state::sessions::AgentKind;

    fn member(pid: u32, comm: &str, argv: &[&str], started: u64) -> Member {
        Member {
            pid,
            comm: comm.into(),
            argv: argv.iter().map(|a| (*a).to_string()).collect(),
            started_epoch_secs: started,
        }
    }

    #[test]
    fn an_agent_is_known_by_its_argv0_or_its_kernel_name() {
        let by_argv0 = [member(10, "2.1.285", &["claude"], 1)];
        assert_eq!(
            identify(&by_argv0).map(|(k, m)| (k, m.pid)),
            Some((AgentKind::Claude, 10))
        );
        let by_comm = [member(
            11,
            "agy",
            &["/Users/x/.local/bin/agy", "-m", "y"],
            1,
        )];
        assert_eq!(identify(&by_comm).map(|(k, _)| k), Some(AgentKind::Agy));
    }

    /// The npm shim's leader is node; its script, and the native child it
    /// starts, are codex. Either is enough.
    #[test]
    fn a_shimmed_agent_is_known_through_its_interpreter() {
        let leader_only = [member(
            20,
            "node",
            &["node", "/usr/lib/node_modules/@openai/codex/bin/codex.js"],
            5,
        )];
        assert_eq!(
            identify(&leader_only).map(|(k, _)| k),
            Some(AgentKind::Codex)
        );
        let wrapped = [
            member(20, "sh", &["sh", "-c", "sleep 1"], 5),
            member(21, "codex", &["/vendor/bin/codex"], 6),
        ];
        assert_eq!(
            identify(&wrapped).map(|(k, m)| (k, m.pid)),
            Some((AgentKind::Codex, 21))
        );
    }

    /// A shell running a plain command, or a script that isn't an agent, runs
    /// no agent. Names are matched whole: `claude-helper` is not claude.
    #[test]
    fn other_programs_are_not_agents() {
        let plain = [
            member(30, "make", &["make", "test"], 1),
            member(31, "python3", &["python3", "-m", "http.server"], 1),
            member(32, "claude-helper", &["claude-helper"], 1),
        ];
        assert_eq!(identify(&plain), None);
        assert_eq!(identify(&[]), None);
    }

    /// An agent running another as a tool: the outer one, started first, is
    /// the tab's.
    #[test]
    fn the_earliest_started_agent_is_the_tabs() {
        let nested = [
            member(41, "codex", &["codex", "exec", "x"], 200),
            member(40, "claude", &["claude"], 100),
        ];
        assert_eq!(
            identify(&nested).map(|(k, m)| (k, m.pid)),
            Some((AgentKind::Claude, 40))
        );
    }

    fn running(pid: u32, backgrounded: bool) -> RunningAgent {
        RunningAgent {
            kind: AgentKind::Claude,
            pid,
            started_epoch_secs: 100,
            backgrounded,
            on_shared_daemon: false,
        }
    }

    fn seen(kind: AgentKind, pid: u32) -> Seen {
        Seen::Agent {
            kind,
            pid,
            started_epoch_secs: 200,
            on_shared_daemon: false,
        }
    }

    #[test]
    fn a_shell_with_no_agent_stays_one() {
        assert_eq!(transition(None, Seen::Shell, false), Change::Unchanged);
    }

    #[test]
    fn an_agent_appearing_in_the_foreground_starts() {
        let Change::Started(r) = transition(None, seen(AgentKind::Codex, 7), false) else {
            panic!("not started");
        };
        assert_eq!(
            (r.kind, r.pid, r.started_epoch_secs, r.backgrounded),
            (AgentKind::Codex, 7, 200, false)
        );
    }

    #[test]
    fn the_same_agent_still_running_is_unchanged() {
        assert_eq!(
            transition(Some(running(7, false)), seen(AgentKind::Claude, 7), true),
            Change::Unchanged
        );
    }

    /// Exit and relaunch between two looks, of the same agent or another: the
    /// new process is a new agent, with none of the old one's session.
    #[test]
    fn a_different_process_is_a_new_agent() {
        let Change::Started(r) =
            transition(Some(running(7, false)), seen(AgentKind::Agy, 8), false)
        else {
            panic!("not replaced");
        };
        assert_eq!((r.kind, r.pid), (AgentKind::Agy, 8));
        let Change::Started(r) =
            transition(Some(running(7, false)), seen(AgentKind::Claude, 9), false)
        else {
            panic!("a relaunched claude is not the old one");
        };
        assert_eq!(r.pid, 9);
    }

    /// The shell has the terminal back. A process that still exists was
    /// stopped or backgrounded; one that doesn't has exited.
    #[test]
    fn leaving_the_foreground_is_an_exit_only_once_the_process_is_gone() {
        assert_eq!(
            transition(Some(running(7, false)), Seen::Shell, true),
            Change::Backgrounded
        );
        assert_eq!(
            transition(Some(running(7, true)), Seen::Shell, true),
            Change::Unchanged
        );
        assert_eq!(
            transition(Some(running(7, false)), Seen::Shell, false),
            Change::Exited
        );
        assert_eq!(
            transition(Some(running(7, true)), Seen::Shell, false),
            Change::Exited
        );
    }

    #[test]
    fn a_backgrounded_agent_brought_back_is_foregrounded() {
        assert_eq!(
            transition(Some(running(7, true)), seen(AgentKind::Claude, 7), true),
            Change::Foregrounded
        );
    }

    /// A codex started without `--no-daemon` may be on the shared daemon,
    /// and the agent it starts carries that; other agents never are.
    #[test]
    fn a_codex_without_no_daemon_is_marked_and_kept_so() {
        let daemon_flag = |members: &[Member]| match super::seen(members) {
            Seen::Agent {
                on_shared_daemon, ..
            } => on_shared_daemon,
            Seen::Shell => panic!("no agent"),
        };
        assert!(daemon_flag(&[member(
            1,
            "codex",
            &["codex", "-m", "o3"],
            1
        )]));
        assert!(!daemon_flag(&[member(
            1,
            "codex",
            &["codex", "--no-daemon"],
            1
        )]));
        assert!(!daemon_flag(&[member(1, "claude", &["claude"], 1)]));
        let Change::Started(r) = transition(
            None,
            super::seen(&[member(9, "codex", &["codex"], 1)]),
            false,
        ) else {
            panic!("not started");
        };
        assert!(r.on_shared_daemon);
    }
}
