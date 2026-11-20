//! S3: spyc upgrades the jars it owns to the build it starts jars from, so
//! agents started on one install run on under the next with nothing restarted
//! (R33: same PID, program and connections). Once startup has settled and the
//! jar store has this build's entry, the loop looks at one tab at a time, on a
//! worker: a jar on this build already is left alone, any other is asked to
//! upgrade, then asked again what it runs, which must be this build.
//!
//! Never a jar mid-turn (its tab working, waiting on the user or on its
//! background work), one still starting, or one another spyc owns (only a tab
//! this spyc holds is looked at); never two at once. A busy refusal (no safe
//! point in time) is asked again after a growing pause, a bounded number of
//! times, then left for the next start. A rejected or failed upgrade stops
//! upgrades for the session: the rest run on their build, as before.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::pane::jar::store::Binary;
use crate::pane::jar::{Signals, UPGRADE_WAIT, UpgradeAnswer};
use crate::pane::tabs::AgentActivity;

use super::landing::spawn_worker;
use super::run::RunCtx;
use super::scheduler::Deadline;
use super::{App, Wake};

/// How long a busy jar is left before it is asked again: one pause a try.
const BUSY_RETRY: [Duration; 3] = [
    Duration::from_secs(10),
    Duration::from_secs(30),
    Duration::from_secs(90),
];

/// Where this session's upgrades of its jars are.
#[derive(Default)]
pub(super) struct JarUpgrades {
    /// The tab whose jar is being looked at or upgraded, while one is.
    in_flight: Option<String>,
    /// Tabs whose jar runs this build, or was given up on this session.
    settled: HashSet<String>,
    /// Busy refusals so far, and when to ask again, by tab.
    busy: HashMap<String, (usize, Instant)>,
    /// Why upgrades stopped this session.
    stopped: Option<String>,
}

/// What one tab's look came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Looked {
    /// Its jar runs this build already.
    Current,
    /// Its jar runs this build now.
    Upgraded(String),
    /// Its jar's answer, or none, short of that.
    Short(UpgradeAnswer),
}

/// One tab, as the choice of which jar to look at next sees it.
#[derive(Debug, Clone, Copy)]
struct Candidate<'a> {
    id: &'a str,
    /// Its agent is mid-turn: working, or waiting on the user or on work.
    mid_turn: bool,
    /// Its jar is still starting or being taken back.
    starting: bool,
}

/// The tab to look at next, and when the soonest busy jar may be asked again
/// if none is ready before.
fn choose<'a>(
    tabs: &[Candidate<'a>],
    ups: &JarUpgrades,
    now: Instant,
) -> (Option<&'a str>, Option<Instant>) {
    let mut soonest: Option<Instant> = None;
    for tab in tabs {
        if ups.settled.contains(tab.id) || tab.mid_turn || tab.starting {
            continue;
        }
        if let Some((_, at)) = ups.busy.get(tab.id)
            && *at > now
        {
            soonest = Some(soonest.map_or(*at, |s| s.min(*at)));
            continue;
        }
        return (Some(tab.id), None);
    }
    (None, soonest)
}

impl App {
    /// Look at the next tab's jar, when startup has settled, nothing is in
    /// flight, and the store has this build. Arms the busy retry's deadline
    /// when only busy jars are left.
    pub(super) fn settle_jar_upgrades(&mut self, now: Instant, ctx: &mut RunCtx) {
        ctx.scheduler.disarm(Deadline::JarUpgradeRetry);
        let ups = &self.runtime.jar_upgrades;
        if ups.stopped.is_some() || ups.in_flight.is_some() || !self.startup_settled_for_jars() {
            return;
        }
        let Some(Binary::Entry(entry)) = crate::pane::jar::store::ready() else {
            return;
        };
        let Some((id, label, signals, socket)) = self.next_jar_to_look_at(now, ctx) else {
            return;
        };
        let build = entry
            .parent()
            .and_then(std::path::Path::file_name)
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        self.runtime.jar_upgrades.in_flight = Some(id.clone());
        let results = self
            .runtime
            .jar_upgrade_results
            .sender_waking(self.loop_waker(Wake::PaneStart));
        spawn_worker(
            results,
            move || (id, label, look(&signals, &socket, &entry, &build)),
            |why| {
                (
                    String::new(),
                    String::new(),
                    Looked::Short(UpgradeAnswer::Refused(why)),
                )
            },
        );
    }

    /// The next tab to look at, with what the worker needs; arms the busy
    /// retry when only busy jars wait.
    fn next_jar_to_look_at(
        &self,
        now: Instant,
        ctx: &mut RunCtx,
    ) -> Option<(String, String, Signals, PathBuf)> {
        let entries: Vec<_> = self
            .runtime
            .every_tab_set()
            .flat_map(|(_, tabs)| tabs.tabs().iter())
            .filter(|e| !e.pane.is_closed() && e.pane.host.jar_signals().is_some())
            .collect();
        let tabs: Vec<Candidate> = entries
            .iter()
            .map(|e| Candidate {
                id: &e.info.id,
                mid_turn: matches!(
                    e.info.activity,
                    AgentActivity::Working | AgentActivity::Blocked | AgentActivity::Waiting
                ),
                starting: e.pane.starting(),
            })
            .collect();
        let (chosen, retry) = choose(&tabs, &self.runtime.jar_upgrades, now);
        if let Some(at) = retry {
            ctx.scheduler.arm(Deadline::JarUpgradeRetry, at);
        }
        let entry = entries
            .iter()
            .find(|e| Some(e.info.id.as_str()) == chosen)?;
        Some((
            entry.info.id.clone(),
            entry.info.label.clone(),
            entry.pane.host.jar_signals()?,
            entry.pane.jar_socket()?.to_path_buf(),
        ))
    }

    /// What the looks found. Whether there is anything to draw.
    pub(crate) fn apply_jar_upgrades(&mut self) -> bool {
        let landed = self.runtime.jar_upgrade_results.drain();
        let mut drawn = false;
        for (id, label, looked) in landed {
            self.runtime.jar_upgrades.in_flight = None;
            drawn |= self.land_jar_upgrade(id, &label, looked);
        }
        drawn
    }

    fn land_jar_upgrade(&mut self, id: String, label: &str, looked: Looked) -> bool {
        let ups = &mut self.runtime.jar_upgrades;
        match looked {
            Looked::Current | Looked::Short(UpgradeAnswer::Lost) => {
                ups.busy.remove(&id);
                ups.settled.insert(id);
                false
            }
            Looked::Upgraded(build) => {
                ups.busy.remove(&id);
                ups.settled.insert(id);
                self.state
                    .flash_info(format!("{label}: its jar runs {build} now"));
                true
            }
            Looked::Short(UpgradeAnswer::Busy(why)) => {
                let tries = ups.busy.get(&id).map_or(0, |(n, _)| *n);
                if let Some(pause) = BUSY_RETRY.get(tries) {
                    ups.busy.insert(id, (tries + 1, Instant::now() + *pause));
                } else {
                    crate::spyc_debug!("jar upgrade: {label} stayed busy ({why}); next start");
                    ups.busy.remove(&id);
                    ups.settled.insert(id);
                }
                false
            }
            Looked::Short(answer) => {
                let why = match answer {
                    UpgradeAnswer::Refused(why) => why,
                    UpgradeAnswer::NoAnswer => {
                        format!("no answer within {} s", UPGRADE_WAIT.as_secs())
                    }
                    other => format!("{other:?}"),
                };
                ups.stopped = Some(why.clone());
                self.state.flash_error(format!(
                    "jar upgrade stopped: {label}'s jar: {why}; the other jars run on their build"
                ));
                true
            }
        }
    }
}

/// On the worker: what the jar at `socket` runs; when not `build`, its
/// upgrade to `entry`, confirmed by what it runs after.
fn look(
    signals: &Signals,
    socket: &std::path::Path,
    entry: &std::path::Path,
    build: &str,
) -> Looked {
    let runs = |socket: &std::path::Path| {
        spyc_jar::client::describe(socket)
            .ok()
            .and_then(|fields| fields.into_iter().find(|(k, _)| k == "build"))
            .map(|(_, v)| v)
    };
    if runs(socket).as_deref() == Some(build) {
        return Looked::Current;
    }
    match signals.upgrade(entry, UPGRADE_WAIT) {
        UpgradeAnswer::Upgraded(_) => match runs(socket) {
            Some(now) if now == build => Looked::Upgraded(now),
            now => Looked::Short(UpgradeAnswer::Refused(format!(
                "it resumed as {}, not {build}",
                now.unwrap_or_else(|| "nothing it would name".into())
            ))),
        },
        other => Looked::Short(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(id: &str) -> Candidate<'_> {
        Candidate {
            id,
            mid_turn: false,
            starting: false,
        }
    }

    #[test]
    fn a_tab_mid_turn_or_starting_waits_and_the_next_idle_one_goes() {
        let now = Instant::now();
        let ups = JarUpgrades::default();
        let tabs = [
            Candidate {
                mid_turn: true,
                ..tab("working")
            },
            Candidate {
                starting: true,
                ..tab("starting")
            },
            tab("idle"),
        ];
        assert_eq!(choose(&tabs, &ups, now), (Some("idle"), None));
        assert_eq!(choose(&tabs[..2], &ups, now), (None, None));
    }

    #[test]
    fn a_settled_tab_is_never_looked_at_again() {
        let mut ups = JarUpgrades::default();
        ups.settled.insert("a".into());
        let tabs = [tab("a"), tab("b")];
        assert_eq!(choose(&tabs, &ups, Instant::now()), (Some("b"), None));
        ups.settled.insert("b".into());
        assert_eq!(choose(&tabs, &ups, Instant::now()), (None, None));
    }

    #[test]
    fn a_busy_jar_waits_out_its_pause_and_says_when() {
        let now = Instant::now();
        let mut ups = JarUpgrades::default();
        ups.busy
            .insert("a".into(), (1, now + Duration::from_secs(10)));
        ups.busy
            .insert("b".into(), (1, now + Duration::from_secs(5)));
        let tabs = [tab("a"), tab("b")];
        assert_eq!(
            choose(&tabs, &ups, now),
            (None, Some(now + Duration::from_secs(5)))
        );
        assert_eq!(
            choose(&tabs, &ups, now + Duration::from_secs(6)),
            (Some("b"), None)
        );
    }
}
