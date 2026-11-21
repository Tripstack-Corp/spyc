//! `:detach project [name]` (S5): a project leaves this spyc for a session of
//! its own, its tabs running on in their jars. spyc saves the project as a
//! session under a new name, synced to disk before any jar is let go, so a
//! crash at any point leaves the project's tabs running and a file that says
//! where they are (finding 6). Then it ends its connection to each of the
//! project's jars as a crash would, drops the project without signalling a
//! program, and starts the session's own jar with a spyc in it at once. That
//! spyc restores the session by its name and takes the jars back
//! (`detach::restore_own_session`), as after `^d`, and `spyc attach NAME`
//! reaches it.
//!
//! What a save doesn't keep goes with the project too: its tabs' scope claims,
//! and each tab's last status report (`SavedTab::live`), since an agent
//! blocked on a prompt reports nothing until it's answered. An agent's MCP
//! proxy follows its jar to the new spyc (R30a): this spyc closes the moved
//! panes' connections on their next message, unanswered (`mcp::panes_moved`).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::landing::spawn_worker;
use super::message::Wake;
use super::state::ProjectId;
use super::{App, Effect};
use crate::pane::jar::session::Begin;
use crate::pane::{AgentActivity, ReportedStatus, TabInfo};
use crate::state::sessions::{MANIFEST_VERSION, Manifest, SavedLive};

/// `:detach`, and `:detach project [name]`.
pub(super) fn cmd_detach(app: &mut App, args: &str) -> Vec<Effect> {
    match super::command_table::split_name_args(args.trim()) {
        ("", _) => app.detach(),
        ("project", name) => app.detach_project(name),
        _ => app.state.flash_error(format!(
            "detach {}: `:detach`, or `:detach project [name]`",
            args.trim()
        )),
    }
    Vec::new()
}

/// A detached project's session jar, started or not.
pub(super) struct MovedStart {
    label: String,
    name: String,
    started: Result<(), String>,
}

impl App {
    /// `:detach project [name]`: the project in view, or the open one named.
    pub(super) fn detach_project(&mut self, named: &str) {
        let Some(id) = self.project_to_move(named) else {
            return;
        };
        let label = self.state.label_of(id).unwrap_or_default();
        if let Some(what) = self.what_cant_move(id) {
            self.state
                .flash_error(format!("can't detach {label}: {what}"));
            return;
        }
        let name = unused_session_name();
        let manifest = self.moved_manifest(id, &name);
        // SPYC-TRAP(move-saved-before-release): the session is on disk before
        // any jar is let go; a spyc that dies in between strands nothing.
        if let Err(e) = crate::state::sessions::save_session_durably(&manifest) {
            self.state.flash_error(format!(
                "detach {label}: it wasn't saved, so it stays: {e:#}"
            ));
            return;
        }
        let cwd = manifest
            .projects
            .first()
            .map_or_default(|p| p.anchor().to_path_buf());
        self.state
            .note_event(&format!("detaching project {label} into session {name}"));
        self.let_project_go(id, &name);
        self.start_moved_session(label, name, cwd);
    }

    /// The project `named` names, the one in view for none, unless it is the
    /// only one open; flashed when there's none to move.
    fn project_to_move(&mut self, named: &str) -> Option<ProjectId> {
        let id = if named.is_empty() {
            Some(self.state.active)
        } else {
            self.state
                .open_projects()
                .find(|p| p.label() == named)
                .map(|p| p.id)
        };
        let Some(id) = id else {
            self.state
                .flash_error(format!("detach project: no open project is named {named}"));
            return None;
        };
        if self.state.open_projects().nth(1).is_none() {
            self.state.flash_error(
                "detach project: it's the only project open, and ^d detaches all of spyc",
            );
            return None;
        }
        Some(id)
    }

    /// What in project `id` would end, or not come along, if it moved: an
    /// editor, or a tab that runs outside a jar or hasn't started yet.
    fn what_cant_move(&mut self, id: ProjectId) -> Option<String> {
        self.in_project(id, |app| {
            let project = app.runtime.project();
            if project.top_overlay.is_some() || project.top_overlay_right.is_some() {
                return Some("its editor would end; quit it first".into());
            }
            let tabs = project.pane_tabs.as_ref()?.tabs();
            if let Some(e) = tabs.iter().find(|e| e.pane.starting()) {
                return Some(format!("the tab {} is still starting", e.info.label));
            }
            tabs.iter()
                .find(|e| !e.pane.is_closed() && !e.pane.host.jarred())
                .map(|e| format!("the tab {} runs outside a jar and would end", e.info.label))
        })
    }

    /// Project `id` as a session of its own named `name`: as a save writes it,
    /// with its tabs' scope claims and last status reports, and no running
    /// spyc's, so the spyc its jar starts restores it whole.
    pub(super) fn moved_manifest(&mut self, id: ProjectId, name: &str) -> Manifest {
        let mut project = self.closed_snapshot(id).project;
        let now = Instant::now();
        let (owners, live): (HashSet<String>, HashMap<String, SavedLive>) =
            self.in_project(id, |app| {
                let tabs = app.runtime.project().pane_tabs.as_ref();
                let tabs = tabs.map_or(&[][..], |t| t.tabs());
                let owners = tabs.iter().map(|e| e.info.claim_owner.clone()).collect();
                let live = tabs
                    .iter()
                    .filter_map(|e| Some((e.info.id.clone(), saved_live(&e.info, now)?)))
                    .collect();
                (owners, live)
            });
        for tab in &mut project.tabs {
            tab.live = live.get(&tab.pane_id).cloned();
        }
        Manifest {
            version: MANIFEST_VERSION,
            id: u64::try_from(crate::sysinfo::epoch_nanos() / 1_000_000).unwrap_or(u64::MAX),
            epoch_secs: crate::sysinfo::epoch_secs(),
            saved_at: crate::sysinfo::format_now(),
            name: name.to_string(),
            pid: None,
            process_started: None,
            active_project: Some(project.id),
            scope_claims: self
                .state
                .scope_registry
                .iter()
                .filter(|c| !c.owner.is_empty() && owners.contains(&c.owner))
                .cloned()
                .collect(),
            projects: vec![project],
            closed_projects: Vec::new(),
            tasks: Vec::new(),
        }
    }

    /// Let project `id` go, its programs running on: end the connection to
    /// each of its jars, close its agents' MCP connections as they next speak,
    /// and drop it, its claims gone with it, and save this session without it.
    fn let_project_go(&mut self, id: ProjectId, name: &str) {
        if id == self.state.active
            && let Some(to) = self.landing_after(id)
        {
            self.switch_project(to);
        }
        let panes: Vec<String> = self.in_project(id, |app| {
            let tabs = app.runtime.project().pane_tabs.as_ref();
            tabs.map_or(&[][..], |t| t.tabs())
                .iter()
                .map(|e| {
                    e.pane.host.sever_jar();
                    e.info.id.clone()
                })
                .collect()
        });
        crate::mcp::panes_moved(panes);
        // Severed first, so what dropping a tab sends its jar goes nowhere.
        self.forget_project(id, &format!("this agent's project moved to session {name}"));
        if self.autosave_session() {
            self.runtime.autosave_last_saved_fp = Some(self.session_fingerprint());
        }
    }

    /// Start session `name`'s own jar, its spyc running at once in `cwd`, on a
    /// worker: it waits for the jar's socket.
    fn start_moved_session(&mut self, label: String, name: String, cwd: PathBuf) {
        let exe = match crate::paths::installed_spyc() {
            Ok(exe) => exe,
            Err(e) => {
                self.state.flash_error(format!(
                    "{label}'s session {name} didn't start ({e:#}); \
                     its tabs run on in their jars, and `spyc -r` takes them back"
                ));
                return;
            }
        };
        let size = crossterm::terminal::size().map_or((24, 80), |(cols, rows)| (rows, cols));
        let results = self
            .runtime
            .moved_starts
            .sender_waking(self.loop_waker(Wake::PaneStart));
        let (for_work, named) = (label.clone(), name.clone());
        spawn_worker(
            results,
            move || {
                let argv = vec![exe.display().to_string()];
                let started =
                    crate::pane::jar::session::start(&named, argv, &cwd, size, Begin::Now)
                        .map(drop)
                        .map_err(|e| format!("{e:#}"));
                MovedStart {
                    label: for_work,
                    name: named,
                    started,
                }
            },
            move |why| MovedStart {
                label,
                name,
                started: Err(why),
            },
        );
    }

    /// What the detached projects' session jars did. Whether there is
    /// anything to draw.
    pub(crate) fn apply_moved_starts(&mut self) -> bool {
        let landed = self.runtime.moved_starts.drain();
        let drawn = !landed.is_empty();
        for MovedStart {
            label,
            name,
            started,
        } in landed
        {
            match started {
                Ok(()) => self.state.flash_info(format!(
                    "{label} runs on as session {name}: `spyc attach {name}` reaches it"
                )),
                Err(why) => self.state.flash_error(format!(
                    "{label}'s session {name} didn't start ({why}); \
                     its tabs run on in their jars, and `spyc -r` takes them back"
                )),
            }
        }
        drawn
    }
}

/// A session name no running session and no saved one has.
fn unused_session_name() -> String {
    let saved: HashSet<String> = crate::state::sessions::load_manifests()
        .into_iter()
        .map(|m| m.name)
        .collect();
    let running: HashSet<String> = crate::pane::jar::attach::running_here()
        .into_iter()
        .map(|r| r.name)
        .collect();
    (0..64)
        .map(|_| crate::state::session_names::generate())
        .find(|n| !saved.contains(n) && !running.contains(n))
        .unwrap_or_else(|| {
            format!(
                "{}_{}",
                crate::state::session_names::generate(),
                crate::sysinfo::epoch_secs()
            )
        })
}

/// The status `info`'s agent last reported, as a moved project carries it.
fn saved_live(info: &TabInfo, now: Instant) -> Option<SavedLive> {
    let live = info.reported.as_ref()?;
    Some(SavedLive {
        status: live.status.name().to_string(),
        note: live.note.clone(),
        blocked_on: live.blocked_on.clone(),
        left_ms: u64::try_from(live.expiry.saturating_duration_since(now).as_millis())
            .unwrap_or(u64::MAX),
    })
}

/// A tab of a moved project, landed: one taken back from its jar takes the
/// status its agent last reported in the spyc it came from; one started again
/// drops it.
pub(super) fn land_carried_status(info: &mut TabInfo, adopted: bool) {
    let Some(live) = info.carried_live.take().filter(|_| adopted) else {
        return;
    };
    let Some(status) = AgentActivity::reportable(&live.status) else {
        return;
    };
    let now = Instant::now();
    let mut report = ReportedStatus::new(status, now, now + Duration::from_millis(live.left_ms));
    report.note = live.note;
    report.blocked_on = live.blocked_on;
    info.last_reported = Some(report.clone());
    info.reported = Some(report);
    info.activity = status;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn carrying(status: &str) -> TabInfo {
        let mut info = TabInfo::new("claude", std::path::Path::new("/"));
        info.carried_live = Some(SavedLive {
            status: status.into(),
            note: Some("Bash".into()),
            blocked_on: None,
            left_ms: 60_000,
        });
        info
    }

    #[test]
    fn a_tab_taken_back_from_its_jar_takes_the_report_it_carried() {
        let mut info = carrying("blocked");
        land_carried_status(&mut info, true);
        assert_eq!(info.activity, AgentActivity::Blocked);
        let live = info.reported.as_ref().unwrap();
        assert_eq!(live.note.as_deref(), Some("Bash"));
        assert!(info.carried_live.is_none());
    }

    #[test]
    fn a_tab_started_again_drops_the_report_it_carried() {
        let mut info = carrying("blocked");
        land_carried_status(&mut info, false);
        assert_eq!(info.activity, AgentActivity::Unknown);
        assert!(info.reported.is_none());
        assert!(info.carried_live.is_none());
    }
}
