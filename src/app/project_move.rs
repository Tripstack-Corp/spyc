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
//!
//! `:attach project NAME` is the way back: another running session's projects
//! come into this spyc. `:attach` lists the sessions there are. This spyc asks
//! that session's spyc to let its session go (`spyc/release`), which saves it
//! as no running spyc's, synced to disk, before it lets any jar go, answers
//! with the file, and quits without saving again or signalling a program. This
//! spyc then opens each of the file's projects beside its own, taking their
//! jars back as `spyc -r` does, with their claims and last reports, saves its
//! own session, and only then removes the other's file. A project it couldn't
//! open stays in that file for `spyc -r`. An agent's MCP proxy follows its jar
//! once the spyc it reached has gone.
//!
//! `:detach tab [N]` and `:attach tab NAME` do the same for one tab. The
//! session a tab moves into holds a copy of its project, with that tab alone
//! and its claims; the project stays here. `:attach tab NAME` brings every
//! tab of a session with one project into the project in view, its jars
//! taken back as a restore takes them, and that session ends as it does for
//! `:attach project`.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::landing::spawn_worker;
use super::message::Wake;
use super::state::ProjectId;
use super::{App, Effect};
use crate::pane::{AgentActivity, ReportedStatus, TabInfo};
use crate::state::scope_registry::ScopeClaim;
use crate::state::sessions::{Held, MANIFEST_VERSION, Manifest, SavedLive};

/// `:detach`, `:detach project [name]` and `:detach tab [N]`.
pub(super) fn cmd_detach(app: &mut App, args: &str) -> Vec<Effect> {
    if app.runtime.handover_starting {
        app.state
            .flash_info("detach: the session's own jar is starting");
        return Vec::new();
    }
    match super::command_table::split_name_args(args.trim()) {
        ("", _) => app.detach(),
        ("project", name) => app.detach_project(name),
        ("tab", n) => app.detach_tab(n),
        _ => app.state.flash_error(format!(
            "detach {}: `:detach`, `:detach project [name]` or `:detach tab [N]`",
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
        let name = match unused_session_name() {
            Ok(name) => name,
            Err(e) => {
                self.state
                    .flash_error(format!("can't detach {label}: {e:#}"));
                return;
            }
        };
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
        let project = self.closed_snapshot(id).project;
        let owners: HashSet<String> = self.in_project(id, |app| {
            let tabs = app.runtime.project().pane_tabs.as_ref();
            tabs.map_or(&[][..], |t| t.tabs())
                .iter()
                .map(|e| e.info.claim_owner.clone())
                .collect()
        });
        self.session_of(project, &owners, name)
    }

    /// `project` as a session of its own named `name`, no running spyc's,
    /// with the scope claims `owners` hold and its tabs' last reports.
    pub(super) fn session_of(
        &self,
        project: crate::state::sessions::SavedProject,
        owners: &HashSet<String>,
        name: &str,
    ) -> Manifest {
        let mut manifest = Manifest {
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
        };
        self.carry_live(&mut manifest);
        manifest
    }

    /// Give each tab `manifest` saves the status its agent last reported here.
    pub(super) fn carry_live(&self, manifest: &mut Manifest) {
        let now = Instant::now();
        let live: HashMap<String, SavedLive> = self
            .runtime
            .every_tab_set()
            .flat_map(|(_, tabs)| tabs.tabs())
            .filter_map(|e| Some((e.info.id.clone(), saved_live(&e.info, now)?)))
            .collect();
        for tab in manifest.projects.iter_mut().flat_map(|p| &mut p.tabs) {
            tab.live = live.get(&tab.pane_id).cloned();
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
        let place = self.runtime.jar_place.clone();
        spawn_worker(
            results,
            move || {
                let argv = vec![exe.display().to_string()];
                let started = crate::pane::jar::session::start(place, &named, argv, &cwd, size)
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

impl App {
    /// `:detach tab [N]`: tab N of the project in view, counted from 1 as
    /// `^a 1` counts, or the active one.
    pub(super) fn detach_tab(&mut self, n: &str) {
        let Some(index) = self.tab_to_move(n) else {
            return;
        };
        let (label, pane_id, owner) = {
            let tabs = self.runtime.project().pane_tabs.as_ref();
            let info = &tabs.expect("tab_to_move found the tab").tabs()[index].info;
            (
                info.label.clone(),
                info.id.clone(),
                info.claim_owner.clone(),
            )
        };
        let name = match unused_session_name() {
            Ok(name) => name,
            Err(e) => {
                self.state
                    .flash_error(format!("can't detach tab {label}: {e:#}"));
                return;
            }
        };
        let Some(manifest) = self.moved_tab_manifest(&pane_id, &owner, &name) else {
            self.state.flash_error(format!(
                "detach tab: {label} has no saved form, so it stays"
            ));
            return;
        };
        // SPYC-TRAP(move-saved-before-release): the session is on disk before
        // any jar is let go; a spyc that dies in between strands nothing.
        if let Err(e) = crate::state::sessions::save_session_durably(&manifest) {
            self.state.flash_error(format!(
                "detach tab {label}: it wasn't saved, so it stays: {e:#}"
            ));
            return;
        }
        let cwd = manifest
            .projects
            .first()
            .map_or_default(|p| p.anchor().to_path_buf());
        self.state
            .note_event(&format!("detaching tab {label} into session {name}"));
        self.let_tab_go(index, pane_id, owner, &name);
        self.start_moved_session(label, name, cwd);
    }

    /// The index of tab `n` of the project in view, the active one for none;
    /// flashed when there's none, or it can't move: still starting, ended, or
    /// outside a jar.
    fn tab_to_move(&mut self, n: &str) -> Option<usize> {
        let Some(tabs) = self.runtime.project().pane_tabs.as_ref() else {
            self.state.flash_error("detach tab: no tab is open");
            return None;
        };
        let index = if n.is_empty() {
            tabs.active_index()
        } else {
            match n.parse::<usize>() {
                Ok(k) if (1..=tabs.len()).contains(&k) => k - 1,
                _ => {
                    let count = tabs.len();
                    self.state.flash_error(format!(
                        "detach tab: no tab {n}; this project's are 1 to {count}"
                    ));
                    return None;
                }
            }
        };
        let entry = &tabs.tabs()[index];
        let why = if entry.pane.starting() {
            "it's still starting"
        } else if entry.pane.is_closed() {
            "its program has ended"
        } else if !entry.pane.host.jarred() {
            "it runs outside a jar and would end"
        } else {
            return Some(index);
        };
        let label = entry.info.label.clone();
        self.state
            .flash_error(format!("can't detach tab {label}: {why}"));
        None
    }

    /// The project in view as a session of its own named `name`, holding only
    /// its tab `pane_id`, with that tab's claims (`owner`'s) and last report.
    /// `None` when the save has no such tab.
    pub(super) fn moved_tab_manifest(
        &mut self,
        pane_id: &str,
        owner: &str,
        name: &str,
    ) -> Option<Manifest> {
        let mut project = self.closed_snapshot(self.state.active).project;
        project.tabs.retain(|t| t.pane_id == pane_id);
        if project.tabs.is_empty() {
            return None;
        }
        project.active_tab = 0;
        project.pane_focused = true;
        let owners: HashSet<String> = std::iter::once(owner.to_string())
            .filter(|o| !o.is_empty())
            .collect();
        Some(self.session_of(project, &owners, name))
    }

    /// Let tab `index` of the project in view go, its program running on: end
    /// the connection to its jar, close its agent's MCP connection as it next
    /// speaks, and close the tab, its claims gone with it, the user left on
    /// the tab they were on; then save this session without it.
    fn let_tab_go(&mut self, index: usize, pane_id: String, owner: String, name: &str) {
        let Some(tabs) = self.runtime.project_mut().pane_tabs.as_mut() else {
            return;
        };
        let users = tabs.active_info().id.clone();
        tabs.switch_to(index);
        tabs.active().host.sever_jar();
        // Severed first, so what closing the tab sends its jar goes nowhere.
        self.close_active_tab_now();
        if let Some(tabs) = self.runtime.project_mut().pane_tabs.as_mut()
            && let Some(back) = tabs.tabs().iter().position(|e| e.info.id == users)
        {
            tabs.switch_to(back);
        }
        crate::mcp::panes_moved(vec![pane_id]);
        if !owner.is_empty() {
            self.drop_claims_of(
                &HashSet::from([owner]),
                &format!("this agent's tab moved to session {name}"),
            );
        }
        if self.autosave_session() {
            self.runtime.autosave_last_saved_fp = Some(self.session_fingerprint());
        }
    }
}

/// `:attach`, `:attach project NAME` and `:attach tab NAME`.
pub(super) fn cmd_attach(app: &mut App, args: &str) -> Vec<Effect> {
    if app.runtime.handover_starting {
        app.state
            .flash_info("attach: the session's own jar is starting");
        return Vec::new();
    }
    match super::command_table::split_name_args(args.trim()) {
        ("", _) => app.list_sessions(),
        ("project", name) if !name.is_empty() => app.attach_project(name),
        ("tab", name) if !name.is_empty() => app.attach_tab(name),
        _ => app.state.flash_error(format!(
            "attach {}: `:attach`, `:attach project NAME` or `:attach tab NAME`",
            args.trim()
        )),
    }
    Vec::new()
}

/// What `:attach` takes from the session it names.
#[derive(Clone, Copy)]
enum Taking {
    /// Its projects, opened beside this spyc's.
    Projects,
    /// Its tabs, into the project in view.
    Tabs,
}

impl Taking {
    const fn word(self) -> &'static str {
        match self {
            Self::Projects => "project",
            Self::Tabs => "tab",
        }
    }
}

/// Another session's spyc, asked to let its session go: the file it saved.
pub(super) struct Released {
    name: String,
    taking: Taking,
    session: Result<u64, String>,
}

impl App {
    /// `:attach`: the other sessions running, each with its projects.
    fn list_sessions(&mut self) {
        let others: Vec<_> = crate::pane::jar::attach::running_here()
            .into_iter()
            .filter(|r| !self.is_this_session(r))
            .collect();
        if others.is_empty() {
            self.state.flash_info("attach: no other session runs");
            return;
        }
        let saved = crate::state::sessions::load_manifests();
        let mut lines: Vec<String> = others
            .iter()
            .map(|r| {
                let projects = saved
                    .iter()
                    .filter(|m| [&r.name, &r.address].contains(&&m.name))
                    .max_by_key(|m| m.epoch_secs)
                    .map_or_else(
                        || "(not saved yet)".to_string(),
                        |m| crate::state::sessions::project_list_label(&m.projects),
                    );
                let terminal = if r.client.is_some() {
                    "attached"
                } else {
                    "detached"
                };
                format!("{:<24} {terminal:<9} {projects}", r.name)
            })
            .collect();
        lines.push(String::new());
        lines.push("`:attach project NAME` brings a session's projects into this spyc".into());
        lines.push("`:attach tab NAME` brings its tabs into the project in view".into());
        self.view.project_mut().pager =
            Some(crate::ui::pager::PagerView::new_plain("sessions", lines));
    }

    /// `:attach project NAME`: ask session `name`'s spyc to let its session
    /// go, for its projects to open here.
    fn attach_project(&mut self, name: &str) {
        let Some((name, socket)) = self.session_to_take(name, Taking::Projects) else {
            return;
        };
        let name = name.as_str();
        if let Some(open) = self.open_here_too(name) {
            self.state.flash_error(format!(
                "attach project: {name}'s project {open} is open here too; \
                 `:attach tab {name}` brings its tabs into the project in view"
            ));
            return;
        }
        self.ask_release(name, socket, Taking::Projects);
    }

    /// `:attach tab NAME`: ask session `name`'s spyc to let its session go,
    /// for its tabs to join the project in view. A session with more than one
    /// project is refused: `:attach project` takes those.
    fn attach_tab(&mut self, name: &str) {
        let Some((name, socket)) = self.session_to_take(name, Taking::Tabs) else {
            return;
        };
        let name = name.as_str();
        let projects = crate::state::sessions::load_manifests()
            .into_iter()
            .filter(|m| m.name == name)
            .max_by_key(|m| m.epoch_secs)
            .map(|m| m.projects.len());
        match projects {
            Some(1) => self.ask_release(name, socket, Taking::Tabs),
            None => self.state.flash_error(format!(
                "attach tab: {name} hasn't saved its session yet; try again in a moment"
            )),
            Some(n) => self.state.flash_error(format!(
                "attach tab: {name} holds {n} projects; `:attach project {name}` brings them"
            )),
        }
    }

    /// The name session `name` goes by and its jar's socket, when it's
    /// another session that runs, found by that name or its address; flashed
    /// when it isn't.
    fn session_to_take(&mut self, name: &str, taking: Taking) -> Option<(String, PathBuf)> {
        let word = taking.word();
        let session = crate::pane::jar::attach::running_here()
            .into_iter()
            .find(|r| r.name == name || r.address == name);
        let own = self.state.session_name.as_deref() == Some(name)
            || session.as_ref().is_some_and(|r| self.is_this_session(r));
        if own {
            self.state
                .flash_error(format!("attach {word}: {name} is this session"));
            return None;
        }
        if session.is_none() {
            self.state.flash_error(format!(
                "attach {word}: no session named {name} runs; `:attach` lists them"
            ));
        }
        session.map(|s| (s.name, s.socket))
    }

    /// Whether `running` is this spyc's session: its own jar's, else one going
    /// by its name.
    fn is_this_session(&self, running: &crate::pane::jar::attach::Running) -> bool {
        match &self.runtime.own_jar {
            Some(jar) => running.address == jar.address(),
            None => self.state.session_name.as_deref() == Some(running.name.as_str()),
        }
    }

    /// Ask session `name`'s spyc, on a worker, to let its session go for this
    /// one to take.
    fn ask_release(&mut self, name: &str, socket: PathBuf, taking: Taking) {
        let word = taking.word();
        self.state
            .note_event(&format!("attaching {name}'s {word}s"));
        let results = self
            .runtime
            .attach_releases
            .sender_waking(self.loop_waker(Wake::PaneStart));
        let (for_work, named) = (name.to_string(), name.to_string());
        spawn_worker(
            results,
            move || Released {
                name: for_work,
                taking,
                session: release(&socket),
            },
            move |why| Released {
                name: named,
                taking,
                session: Err(why),
            },
        );
    }

    /// A project session `name` last saved that is open here as well, which
    /// two spycs can't both have: its label.
    fn open_here_too(&self, name: &str) -> Option<String> {
        let saved = crate::state::sessions::load_manifests()
            .into_iter()
            .filter(|m| m.name == name)
            .max_by_key(|m| m.epoch_secs)?;
        saved
            .projects
            .iter()
            .find(|p| self.project_at_saved(p).is_some())
            .map(|p| p.label.clone())
    }

    /// What the asked spycs let go. Whether there is anything to draw.
    pub(crate) fn apply_attach_releases(&mut self) -> bool {
        // Taken in, a session would be saved here as this spyc's: while `^d`'s
        // jar starts it waits, and one let go after stays saved for `spyc -r`.
        if self.session_out_of_hand() {
            return false;
        }
        let landed = self.runtime.attach_releases.drain();
        let drawn = !landed.is_empty();
        for Released {
            name,
            taking,
            session,
        } in landed
        {
            let manifest = session.and_then(|id| {
                crate::state::sessions::load_manifests()
                    .into_iter()
                    .find(|m| m.id == id)
                    .ok_or_else(|| format!("the session it saved ({id}) isn't there"))
            });
            match (manifest, taking) {
                (Ok(manifest), Taking::Projects) => self.take_session(&name, &manifest),
                (Ok(manifest), Taking::Tabs) => self.take_tabs(&name, &manifest),
                (Err(why), _) => self
                    .state
                    .flash_error(format!("attach {} {name}: {why}", taking.word())),
            }
        }
        drawn
    }

    /// Open each of `manifest`'s projects beside this spyc's, their jars taken
    /// back, with their claims and tasks; save this session, then remove that
    /// one's file, unless a project of it couldn't open.
    pub(super) fn take_session(&mut self, name: &str, manifest: &Manifest) {
        let held = Held::by_running(&crate::state::sessions::load_manifests());
        // A tab that moved away from this spyc and comes back is answered here.
        let panes: Vec<String> = manifest
            .projects
            .iter()
            .flat_map(|p| &p.tabs)
            .map(|t| t.pane_id.clone())
            .collect();
        crate::mcp::panes_returned(&panes);
        let (mut opened, mut stayed) = (Vec::new(), Vec::new());
        for saved in &manifest.projects {
            match self.restore_out_of_view(saved, false, &held) {
                Some(project) => opened.push((project.id, saved.label.clone())),
                None => stayed.push(saved.label.clone()),
            }
        }
        self.take_claims(&manifest.scope_claims);
        self.restore_tasks(&manifest.tasks);
        self.record_open_roots();
        if let Some((first, _)) = opened.first() {
            self.switch_project(*first);
        }
        // SPYC-TRAP(move-saved-before-release): this session holds the
        // projects on disk before the file that held them goes.
        let mut own = self.build_manifest();
        self.carry_live(&mut own);
        let kept = crate::state::sessions::save_session_durably(&own);
        if kept.is_ok() {
            self.runtime.autosave_last_saved_fp = Some(self.session_fingerprint());
        }
        if kept.is_ok()
            && stayed.is_empty()
            && let Err(e) = crate::state::sessions::remove_session(manifest.id)
        {
            crate::spyc_debug!("attach: {name}'s file stays: {e:#}");
        }
        let labels: Vec<&str> = opened.iter().map(|(_, l)| l.as_str()).collect();
        if stayed.is_empty() {
            self.state
                .flash_info(format!("attached {name}'s projects: {}", labels.join(", ")));
        } else {
            self.state.flash_error(format!(
                "attached {name}'s projects but {}, which stayed in its saved session: \
                 `spyc -r` takes them back",
                stayed.join(", ")
            ));
        }
    }

    /// Bring `manifest`'s tabs into the project in view, their jars taken
    /// back, with their claims and tasks; save this session, then remove that
    /// one's file. A tab that can't open stays saved, unopened, in this
    /// project, as a restore keeps one.
    pub(super) fn take_tabs(&mut self, name: &str, manifest: &Manifest) {
        let held = Held::by_running(&crate::state::sessions::load_manifests());
        // A tab that moved away from this spyc and comes back is answered here.
        let panes: Vec<String> = manifest
            .projects
            .iter()
            .flat_map(|p| &p.tabs)
            .map(|t| t.pane_id.clone())
            .collect();
        crate::mcp::panes_returned(&panes);
        let (mut opened, mut unopened) = (Vec::new(), Vec::new());
        for saved in &manifest.projects {
            let (o, u) = self.restore_tabs_here(saved, &held);
            opened.extend(o);
            unopened.extend(u);
        }
        self.take_claims(&manifest.scope_claims);
        self.restore_tasks(&manifest.tasks);
        // SPYC-TRAP(move-saved-before-release): this session holds the tabs,
        // or keeps them unopened, on disk before the file that held them goes.
        let mut own = self.build_manifest();
        self.carry_live(&mut own);
        if crate::state::sessions::save_session_durably(&own).is_ok() {
            self.runtime.autosave_last_saved_fp = Some(self.session_fingerprint());
            if let Err(e) = crate::state::sessions::remove_session(manifest.id) {
                crate::spyc_debug!("attach: {name}'s file stays: {e:#}");
            }
        }
        if unopened.is_empty() {
            self.state
                .flash_info(format!("attached {name}'s tabs: {}", opened.join(", ")));
        } else {
            self.state.flash_error(format!(
                "attached {name}'s tabs but {}, kept unopened in this project",
                unopened.join(", ")
            ));
        }
    }

    /// Take `claims` into this spyc's registry, each keeping its id unless a
    /// claim here has it.
    fn take_claims(&mut self, claims: &[ScopeClaim]) {
        for claim in claims {
            let mut claim = claim.clone();
            let registry = &self.state.scope_registry;
            if registry.iter().any(|c| c.id == claim.id) {
                claim.id = registry.iter().map(|c| c.id).max().unwrap_or(0) + 1;
            }
            self.state.scope_registry.push(claim);
        }
    }

    /// `spyc/release`, from `:attach project` in another spyc: save the
    /// session as no running spyc's, synced, let every jar go, and quit
    /// without saving again or signalling a program. Refused while something
    /// would end with this spyc.
    pub(super) fn release_session(&mut self) -> crate::mcp_cmd::McpResponse {
        use crate::mcp_cmd::McpResponse;
        if let Some(what) = self.what_cant_be_released() {
            return McpResponse::Error {
                message: format!("{what}, so it stays"),
            };
        }
        let saved = match self.save_released() {
            Ok(saved) => saved,
            Err(e) => {
                return McpResponse::Error {
                    message: format!("its session wasn't saved, so it stays: {e:#}"),
                };
            }
        };
        self.runtime.released = true;
        let id = saved.id();
        self.release_jars(saved);
        self.state
            .note_event("released: another spyc takes this session's projects");
        self.state.should_quit = true;
        McpResponse::Ok {
            message: serde_json::json!({ "session": id }).to_string(),
        }
    }

    /// What would end, or not come along, if this spyc let its session go.
    fn what_cant_be_released(&mut self) -> Option<String> {
        if let Some(what) = self.unjarred() {
            return Some(format!("{what} runs outside a jar"));
        }
        // Collected first: each look goes into its project, which needs `self`.
        let ids: Vec<ProjectId> = self.state.open_projects().map(|p| p.id).collect();
        for id in ids {
            if let Some(what) = self.what_cant_move(id) {
                return Some(what);
            }
        }
        None
    }
}

/// Ask the spyc session jar `socket` runs to let its session go: the file it
/// saved.
fn release(socket: &std::path::Path) -> Result<u64, String> {
    let pid = crate::pane::jar::shutdown::program_pid(socket)
        .and_then(|p| u32::try_from(p).ok())
        .ok_or("its jar names no spyc")?;
    crate::mcp::ask_release(pid).map_err(|e| format!("{e:#}"))
}

/// A session name no running session and no saved one has, whose jar's
/// socket can be bound; an error when none can.
fn unused_session_name() -> anyhow::Result<String> {
    let sessions = crate::pane::jar::place::this_users_sessions();
    let name = unused_name();
    crate::pane::jar::session::socket_in(&sessions, &name)?;
    Ok(name)
}

/// A session name no running session and no saved one has.
fn unused_name() -> String {
    let saved: HashSet<String> = crate::state::sessions::load_manifests()
        .into_iter()
        .map(|m| m.name)
        .collect();
    let running: HashSet<String> = crate::pane::jar::attach::running_here()
        .into_iter()
        .flat_map(|r| [r.name, r.address])
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
