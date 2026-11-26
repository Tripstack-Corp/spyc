//! A jarred tab's start, off the loop (finding 5). Starting a jar, or taking
//! a restored tab's back, waits on it: the store's check of the first jar,
//! a jar's first run, an upgrade it's parked in. So the tab opens at once on
//! a stand-in that says so, a worker launches its pane, and the pane takes
//! the stand-in's place when it lands ([`App::apply_pane_starts`], in the
//! pre-recv drain). A jar that never started its program leaves the tab
//! `[not started]`, why on its screen, and `^a R` starts it again.
//!
//! A live tab whose jar ended this spyc's lease, as a spyc stopped past its
//! jars' deadline finds, is taken back the same way (S2b): it stays labelled
//! `[detached]` on a stand-in while a worker takes the jar back, and the
//! pane starts from the jar's snapshot and replay, losing nothing the jar's
//! ring held.
//!
//! Every start whose record names a jar (a restored tab's saved one, the
//! one a restart replaces) asks that jar first ([`jar::held`], SPYC-TRAP
//! `jar-never-twice`). One that may still hold the program takes it back,
//! or, for a restart, ends it there and sees the jar go before anything
//! starts; with jars off, a restored tab whose jar runs comes back
//! `[detached]`, saying why and what to do. A retake closes the tab's old
//! connection before it asks for the lease (P1).
//!
//! What's typed into a starting tab is dropped: nothing runs to read it, and
//! keys held back would reach a program that has drawn a prompt they weren't
//! typed for. A tab closed or restarted while it starts takes nothing: the
//! pane that lands for it is ended at once.

use std::path::{Path, PathBuf};

use crate::pane::jar;
use crate::state::sessions::SavedTab;

use super::landing::spawn_worker;
use super::state::ProjectId;
use super::{App, Mode, Pane, Prompt, PromptKind, Wake};

/// A jarred tab's pane, launched on a worker, or why it wasn't.
pub(super) struct PaneStart {
    project: ProjectId,
    /// The tab's id (`TabInfo::id`): a tab restarted meanwhile has a new one.
    tab: String,
    pane: anyhow::Result<Pane>,
    retake: bool,
    /// The jar a restored or lapsed tab takes back.
    socket: Option<std::path::PathBuf>,
}

/// What a tab's start knows of a jar that may still hold its program: the
/// record it asks that jar by.
#[derive(Clone, Copy)]
pub(super) enum Record<'a> {
    /// A new tab: no jar ran its program.
    None,
    /// A restored tab, whose saved jar is taken back while it runs.
    Restoring(&'a SavedTab),
    /// A restart of a tab whose program ran in the jar at this socket: what
    /// that jar still holds is ended there before anything starts.
    Replacing(&'a Path),
}

impl<'a> Record<'a> {
    /// The jar the record names.
    pub(super) fn socket(self) -> Option<&'a Path> {
        match self {
            Self::None => None,
            Self::Restoring(saved) => saved.jar_socket.as_deref(),
            Self::Replacing(socket) => Some(socket),
        }
    }

    const fn restoring(self) -> Option<&'a SavedTab> {
        match self {
            Self::Restoring(saved) => Some(saved),
            Self::None | Self::Replacing(_) => None,
        }
    }
}

/// What a worker does for a tab's jar.
enum Taking {
    /// Start one, or take a restored tab's back, once the program a restart
    /// replaces has ended in its jar at `replacing`.
    Start {
        restoring: Option<Box<SavedTab>>,
        replacing: Option<PathBuf>,
    },
    /// End the program in the jar at this socket, then start the tab's on a
    /// PTY of spyc's: a restart with jars off.
    Native(PathBuf),
    /// Take back the jar at this socket, which ended the tab's lease.
    Retake(PathBuf),
}

impl Taking {
    /// The jar this start asks first.
    fn socket(&self) -> Option<&Path> {
        match self {
            Self::Start {
                restoring,
                replacing,
            } => replacing
                .as_deref()
                .or_else(|| restoring.as_ref()?.jar_socket.as_deref()),
            Self::Native(socket) | Self::Retake(socket) => Some(socket),
        }
    }

    /// What the tab's stand-in says meanwhile.
    const fn note(&self) -> &'static str {
        match self {
            Self::Start {
                replacing: Some(_), ..
            }
            | Self::Native(_) => "Ending this tab's program in its jar, then starting it again…",
            Self::Start {
                restoring: Some(_), ..
            }
            | Self::Retake(_) => "Taking this tab's program back from its jar…",
            Self::Start { .. } => "Starting this tab's jar…",
        }
    }
}

/// What a worker launches a jarred tab's pane from: [`crate::pane::Launch`],
/// owned.
struct JarredLaunch {
    command: String,
    rows: u16,
    cols: u16,
    cwd: std::path::PathBuf,
    context_path: std::path::PathBuf,
    extra_env: Vec<(String, String)>,
    project_env: Vec<(String, String)>,
    ssh_agent: Option<std::path::PathBuf>,
}

impl JarredLaunch {
    /// Launch the pane, its program in the jar `place` (this user's when
    /// `None`) starts or a restored tab's gives back, once the program a
    /// restart replaces has ended in its jar.
    fn run(
        self,
        tab: &str,
        place: Option<jar::Place>,
        taking: &Taking,
        watch: &jar::Watch,
        wake: crate::pane::PaneWake,
    ) -> anyhow::Result<Pane> {
        let end_replaced = |old: &Path| -> anyhow::Result<()> {
            let place = match &place {
                Some(place) => place.clone(),
                None => jar::Place::for_this_user()?,
            };
            jar::end_held(&place, old, Some(watch))
        };
        // SPYC-TRAP(jar-off-the-loop): a jar's spec is made only on this
        // worker; with one, the launch waits on the jar.
        let spec = match taking {
            Taking::Start {
                restoring,
                replacing,
            } => {
                if let Some(old) = replacing {
                    end_replaced(old)?;
                }
                jar::spec_for(place.clone(), tab, restoring.as_deref())?
                    .map(|spec| spec.watched(Some(watch.clone())))
            }
            Taking::Native(old) => {
                end_replaced(old)?;
                None
            }
            Taking::Retake(socket) => Some(
                jar::spec_for(place.clone(), tab, None)?
                    .ok_or_else(|| anyhow::anyhow!("no place for the tab's jar"))?
                    .retaking(socket.clone())
                    .watched(Some(watch.clone())),
            ),
        };
        let env: Vec<(&str, &str)> = self
            .extra_env
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        Pane::launch(
            crate::pane::Launch {
                command: &self.command,
                rows: self.rows,
                cols: self.cols,
                cwd: &self.cwd,
                context_path: &self.context_path,
                extra_env: &env,
                project_env: &self.project_env,
                ssh_agent: self.ssh_agent.as_deref(),
                jar: spec,
                record: taking.socket(),
            },
            wake,
        )
    }
}

impl JarredLaunch {
    /// `launch`, owned for a worker.
    fn from(launch: &crate::pane::Launch) -> Self {
        Self {
            command: launch.command.to_string(),
            rows: launch.rows,
            cols: launch.cols,
            cwd: launch.cwd.to_path_buf(),
            context_path: launch.context_path.to_path_buf(),
            extra_env: launch
                .extra_env
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
            project_env: launch.project_env.to_vec(),
            ssh_agent: launch.ssh_agent.map(Path::to_path_buf),
        }
    }
}

impl App {
    /// What a pane actually runs for `cmd`: a codex pane gets `--no-daemon`
    /// (`agent::codex_without_daemon`) unless `[pane] codex_daemon` lets it
    /// join codex's shared server. The tab keeps `cmd` itself.
    pub(super) fn spawn_command<'a>(&self, cmd: &'a str) -> std::borrow::Cow<'a, str> {
        let codex = crate::agent::detect(cmd).kind() == crate::state::sessions::AgentKind::Codex;
        if codex && !self.state.active_project().config.pane.codex_daemon {
            crate::agent::codex_without_daemon(cmd)
        } else {
            std::borrow::Cow::Borrowed(cmd)
        }
    }

    /// A tab's pane: with jars on, a stand-in, its own pane launched on a
    /// worker; otherwise, or once the user chose to run tabs without a
    /// usable jar (`app::jarless`), `launch`'s, its program on a PTY of
    /// spyc's. Either way, a jar the tab's `record` names is asked first.
    pub(super) fn launch_tab_pane(
        &self,
        launch: crate::pane::Launch,
        jars: bool,
        tab: &str,
        record: Record,
        wake: crate::pane::PaneWake,
    ) -> anyhow::Result<Pane> {
        let jarred = jars && jar::honoured() && !self.runtime.jarless.chosen;
        // SPYC-TRAP(jar-never-twice): the jar the record names, asked before
        // any program starts.
        let held = jar::held(record.socket()).map(Path::to_path_buf);
        let owned = JarredLaunch::from(&launch);
        let taking = match (jarred, held, record) {
            (false, None, _) => {
                let record = record.socket();
                return Pane::launch(crate::pane::Launch { record, ..launch }, wake);
            }
            // C1: with jars off, a restored tab whose jar runs is left to it.
            (false, Some(socket), Record::Restoring(_) | Record::None) => {
                let why = self.left_in_its_jar(jars, &socket);
                let size = (launch.rows, launch.cols);
                return Pane::standing_in(jar::detached(&socket, &why, size), false, size, wake);
            }
            (false, Some(socket), Record::Replacing(_)) => Taking::Native(socket),
            (true, held, record) => Taking::Start {
                restoring: record.restoring().cloned().map(Box::new),
                replacing: held.filter(|_| matches!(record, Record::Replacing(_))),
            },
        };
        self.start_jarred_pane(self.state.active, owned, tab, taking, wake)
    }

    /// What a restored tab whose jar at `socket` runs says when this spyc
    /// runs tabs without jars: why it wasn't started, and what to do.
    fn left_in_its_jar(&self, jars: bool, socket: &Path) -> String {
        let cause = if !jars {
            "`[pane] jars = false` runs tabs without jars"
        } else if !jar::honoured() {
            "this build doesn't run tabs in jars"
        } else {
            "this session runs tabs without jars, as you chose"
        };
        format!(
            "This tab's program still runs in its jar at {}, so it wasn't started again here: \
             {cause}.\n^a R ends it in its jar and starts it here. To take it back instead, \
             turn jars on (`[pane] jars = true`) and restore the session again (spyc -r).",
            socket.display()
        )
    }

    /// Take back each live tab whose jar ended this spyc's lease (decision
    /// G): its pane in a stand-in's hands while a worker does. Whether any.
    pub(crate) fn retake_lapsed_tabs(&mut self) -> bool {
        let lapsed: Vec<_> = self
            .runtime
            .every_tab_set()
            .flat_map(|(project, tabs)| {
                tabs.tabs()
                    .iter()
                    .filter(|e| e.pane.lease_lapsed())
                    .filter_map(move |e| {
                        let socket = e.pane.jar_socket()?.to_path_buf();
                        Some((
                            project,
                            e.info.id.clone(),
                            e.info.command.clone(),
                            e.info.cwd.clone(),
                            socket,
                            e.pane.host.last_size,
                        ))
                    })
            })
            .collect();
        for (project, tab, command, cwd, socket, (rows, cols)) in &lapsed {
            // P1: the old connection closes before the lease is asked for.
            if let Some(entry) = self
                .runtime
                .every_tab_set_mut()
                .find(|(p, _)| p == project)
                .and_then(|(_, tabs)| tabs.tabs_mut().iter_mut().find(|e| &e.info.id == tab))
            {
                entry.pane.host.sever_jar();
            }
            let launch = JarredLaunch {
                command: command.clone(),
                rows: *rows,
                cols: *cols,
                cwd: cwd.clone(),
                context_path: self.view.project().context_path.clone(),
                extra_env: Vec::new(),
                // A retake starts nothing.
                project_env: Vec::new(),
                ssh_agent: None,
            };
            let wake = self.make_pane_wake();
            let taking = Taking::Retake(socket.clone());
            let Ok(stand_in) = self.start_jarred_pane(*project, launch, tab, taking, wake) else {
                continue;
            };
            let entry = self
                .runtime
                .every_tab_set_mut()
                .find(|(p, _)| p == project)
                .and_then(|(_, tabs)| tabs.tabs_mut().iter_mut().find(|e| &e.info.id == tab));
            if let Some(entry) = entry {
                // Its connection is over; ending the pane signals nothing.
                let lapsed = std::mem::replace(&mut entry.pane, stand_in);
                lapsed.shutdown_detached(END_WAIT);
            }
        }
        !lapsed.is_empty()
    }

    /// A jarred tab's stand-in pane, its own pane launched on a worker.
    fn start_jarred_pane(
        &self,
        project: ProjectId,
        launch: JarredLaunch,
        tab: &str,
        taking: Taking,
        wake: crate::pane::PaneWake,
    ) -> anyhow::Result<Pane> {
        let size = (launch.rows, launch.cols);
        let socket = taking.socket().map(Path::to_path_buf);
        let retake = matches!(taking, Taking::Retake(_));
        let (jarred, watch) = jar::starting_watched(taking.note(), socket.as_deref(), size);
        let stand_in = Pane::standing_in(jarred, true, size, wake.clone())?;
        // A startup tab starts before `run()`: its landing wakes the loop
        // once it runs.
        let results = self
            .runtime
            .pane_starts
            .sender_waking(self.loop_waker(Wake::PaneStart));
        let place = self.runtime.jar_place.clone();
        let (tab, again) = (tab.to_string(), tab.to_string());
        let socket_again = socket.clone();
        spawn_worker(
            results,
            move || {
                let pane = launch.run(&tab, place, &taking, &watch, wake);
                PaneStart {
                    project,
                    tab,
                    pane,
                    retake,
                    socket,
                }
            },
            move |panic| PaneStart {
                project,
                tab: again,
                pane: Err(anyhow::anyhow!("the tab's start stopped: {panic}")),
                retake,
                socket: socket_again,
            },
        );
        Ok(stand_in)
    }

    /// Put each landed pane in its tab's place. Whether any landed.
    pub(crate) fn apply_pane_starts(&mut self) -> bool {
        let landed = self.runtime.pane_starts.drain();
        if landed.is_empty() {
            return false;
        }
        for start in landed {
            let origin = Some(self.tab_origin(&start.tab));
            if start.project == self.state.active {
                self.noting_from(origin, |app| app.land_pane_start(start));
            } else if self.state.has_project(start.project) {
                self.noting_from(origin, |app| {
                    app.land_in(start.project, |app| app.land_pane_start(start));
                });
            } else if let Ok(pane) = start.pane {
                pane.shutdown_detached(END_WAIT);
            }
        }
        true
    }

    /// `pane` in the place of the tab `tab`'s stand-in, or, when it never
    /// started its program, a stand-in saying why; one for want of a usable
    /// jar is offered to run without one (`app::jarless`).
    fn land_pane_start(&mut self, start: PaneStart) {
        let PaneStart {
            project,
            tab,
            pane,
            retake,
            socket,
        } = start;
        let wake = self.make_pane_wake();
        let Some(entry) = self
            .runtime
            .project_mut()
            .pane_tabs
            .as_mut()
            .and_then(|tabs| tabs.tabs_mut().iter_mut().find(|e| e.info.id == tab))
            .filter(|e| e.pane.starting())
        else {
            if let Ok(pane) = pane {
                pane.shutdown_detached(END_WAIT);
            }
            return;
        };
        let size = entry.pane.host.last_size;
        let started = pane.is_ok();
        let unusable = pane
            .as_ref()
            .err()
            .and_then(jar::no_usable_jar)
            .map(str::to_string);
        let mut pane = match pane {
            Ok(pane) => pane,
            Err(e) => {
                self.state
                    .note_error(&format!("tab {} didn't start: {e:#}", entry.info.label));
                match Pane::standing_in(
                    not_started(&e, socket.as_deref(), retake, size),
                    false,
                    size,
                    wake,
                ) {
                    Ok(pane) => pane,
                    Err(e) => {
                        self.state.flash_error(format!("pane: {e:#}"));
                        return;
                    }
                }
            }
        };
        let _ = pane.resize(size.0, size.1);
        if pane.adopted() {
            self.state.note_event(&format!(
                "{}: taken back from its jar, still running",
                crate::pane::tabs::strip_exit_suffix(&entry.info.label)
            ));
        }
        settle_landed_tab(&mut entry.info, &pane, started);
        super::project_move::land_carried_status(&mut entry.info, pane.adopted());
        let stand_in = std::mem::replace(&mut entry.pane, pane);
        stand_in.shutdown_detached(END_WAIT);
        self.view.needs_full_repaint = true;
        if let Some(why) = unusable
            && !retake
        {
            self.tab_has_no_usable_jar(project, tab, socket, &why);
        }
    }
}

/// The stand-in for a tab whose start failed with `e`, labelled by what
/// its jar at `socket` says now: a jar that may still hold the program
/// leaves the tab `[detached]`, never `[jar lost]` or `[not started]`,
/// and keeps its record, so a restart asks that jar first (C12).
fn not_started(
    e: &anyhow::Error,
    socket: Option<&Path>,
    retake: bool,
    size: (u16, u16),
) -> jar::Jarred {
    if let Some(socket) = jar::held(socket) {
        if e.downcast_ref::<jar::NoToken>().is_some() {
            return jar::detached(socket, &jar::no_token_screen(socket), size);
        }
        if e.downcast_ref::<jar::OwnedElsewhere>().is_some() {
            return jar::detached(
                socket,
                &format!("spyc: {e:#}\n\n^a x closes this tab."),
                size,
            );
        }
        return jar::ended_at(
            &format!(
                "spyc: {e:#}\n\nIts program still runs in its jar: ^a R ends it there and starts \
                 it again.\n"
            ),
            jar::Lost::Held,
            Some(socket),
            size,
        );
    }
    let lost = if retake {
        jar::Lost::Gone
    } else {
        jar::Lost::Unstarted
    };
    jar::ended(
        &format!("spyc: {e:#}\n\n^a R starts it again\n"),
        lost,
        size,
    )
}

/// How long `^a R` waits for a jar to say who owns it before it asks: a bound
/// only, since the restart's worker asks again when it gets no answer.
const ASK_OWNER: std::time::Duration = std::time::Duration::from_millis(200);

/// How long a stand-in, or a pane nothing will show, has to end.
const END_WAIT: std::time::Duration = std::time::Duration::from_millis(250);

/// The tab's state, for the pane that took its stand-in's place: a program
/// taken back, or left to another spyc, is in its conversation already and
/// gets no `/resume`; one started now gets it once its banner has settled.
fn settle_landed_tab(info: &mut crate::pane::tabs::TabInfo, pane: &Pane, started: bool) {
    use crate::pane::tabs::PendingResumeSend;
    info.label = crate::pane::tabs::strip_exit_suffix(&info.label);
    if !started || pane.adopted() || pane.detached() {
        info.pending_resume_send = None;
    } else if let Some(PendingResumeSend::Text { after, .. }) = &mut info.pending_resume_send {
        *after = std::time::Instant::now() + super::RESTORE_BANNER_SETTLE;
    }
    if info.launched_agent().kind() == crate::state::sessions::AgentKind::Codex
        && info.codex_home.is_none()
    {
        info.codex_home = pane
            .host
            .launch_environment()
            .and_then(|env| crate::agent::codex_home::from_launch(&env, &info.cwd));
    }
}

/// A restart of a tab whose jar may still hold its program (C2): `^a R`
/// asks before it ends that program, and refuses, naming the other spyc,
/// where another spyc owns the jar. A restart nobody confirmed (`:hooks
/// on!`) never ends a program a detached tab's jar holds.
impl App {
    /// The jar the active project's tab at `idx` names, when it may still
    /// hold that tab's program.
    pub(super) fn held_jar_of(&self, idx: usize) -> Option<PathBuf> {
        let entry = self.runtime.project().pane_tabs.as_ref()?.tabs().get(idx)?;
        jar::held(entry.pane.jar_socket()).map(Path::to_path_buf)
    }

    /// `^a R` on the active tab whose jar may still hold its program, with
    /// no program running here: refused where another spyc owns the jar,
    /// else asked. Whether it was either; a live tab is left to the usual
    /// confirm.
    pub(super) fn asked_to_restart_beside_its_jar(&mut self) -> bool {
        let Some(tabs) = self.runtime.project().pane_tabs.as_ref() else {
            return false;
        };
        let pane = tabs.active();
        let live = !pane.is_closed() && pane.exit_status().is_none() && !pane.detached();
        if live || jar::held(pane.jar_socket()).is_none() {
            return false;
        }
        let socket = pane.jar_socket().map(Path::to_path_buf);
        if let Some(why) = socket.as_deref().and_then(|s| self.restart_refused(s)) {
            self.state.flash_error(why);
            return true;
        }
        if !matches!(self.state.mode, Mode::Normal) {
            self.state.flash_info(
                "restart: its program still runs in its jar; ^a R again once nothing else is asked",
            );
            return true;
        }
        let label = self.active_tab_label();
        self.state.mode = Mode::Prompting(Prompt::simple(
            PromptKind::RestartPane,
            format!(
                "restart '{label}'? Its program still runs in its jar: y ends it there, then \
                 starts it again [y/N] "
            ),
        ));
        true
    }

    /// Why `^a R` can't end the program the jar at `socket` holds, if it
    /// can't: nothing can tell whether the jar runs, this spyc has no token
    /// for it, or another spyc owns it, as the jar says within [`ASK_OWNER`].
    /// A jar that doesn't answer in time is asked again by the restart's
    /// worker, which refuses the same way.
    fn restart_refused(&self, socket: &Path) -> Option<String> {
        if jar::place::socket_state(socket) == jar::place::SocketState::Unknown {
            return Some(format!(
                "restart: can't tell whether this tab's jar at {} still runs; nothing was started",
                socket.display()
            ));
        }
        if !jar::token_known(self.runtime.jar_place.as_ref(), socket) {
            return Some(
                "restart: this spyc has no token for this tab's jar, so can't end its program \
                 there; nothing was started"
                    .into(),
            );
        }
        let jar::OtherSpyc(owner) = jar::owned_elsewhere_now(socket, ASK_OWNER)?;
        let pid = owner.map_or_else(String::new, |p| format!(" (pid {p})"));
        Some(format!(
            "restart: another spyc{pid} owns this tab's jar; its program runs on there, and \
             nothing was started here"
        ))
    }

    /// Whether a restart nobody confirmed leaves the tab at `idx` alone: its
    /// jar may still hold its program, and nothing of it runs here.
    pub(super) fn refuses_restart_beside_its_jar(&mut self, idx: usize) -> bool {
        let Some(entry) = self
            .runtime
            .project()
            .pane_tabs
            .as_ref()
            .and_then(|tabs| tabs.tabs().get(idx))
        else {
            return false;
        };
        let away = entry.pane.detached() || entry.pane.is_closed();
        if !away || jar::held(entry.pane.jar_socket()).is_none() {
            return false;
        }
        self.state.flash_info(
            "restart: this tab's program still runs in its jar without this spyc; ^a R ends it \
             there first",
        );
        true
    }
}

impl App {
    /// A tab running `cmd`, a restored one taking back the jar `restoring`
    /// names while it runs.
    pub(super) fn spawn_pane_tab(
        &mut self,
        cmd: &str,
        cwd: &std::path::Path,
        slot: super::pane_tabs::TabSlot,
        home: Option<&std::path::Path>,
        restoring: Option<&crate::state::sessions::SavedTab>,
        hint: Option<crate::state::sessions::AgentKind>,
    ) -> bool {
        let record = restoring.map_or(Record::None, Record::Restoring);
        self.spawn_tab_for(cmd, cwd, slot, home, record, hint)
    }
}

#[cfg(test)]
mod tests {
    /// P1: a retake closes the tab's old connection before it asks for the
    /// lease, so the jar it asks has let that connection go or is letting it.
    /// No test can see the order from outside (the worker's request and the
    /// loop's close race), so this reads it in `retake_lapsed_tabs`. Fails
    /// closed.
    #[test]
    fn a_retake_closes_its_old_connection_before_it_asks() {
        let starts = crate::guard_support::production_half(include_str!("pane_starts.rs"));
        let body = starts
            .split("fn retake_lapsed_tabs(")
            .nth(1)
            .and_then(|body| body.split("\n    fn ").next())
            .expect("premise: retake_lapsed_tabs is there");
        let closed = body.find(".sever_jar()");
        let asked = body.find("self.start_jarred_pane(");
        assert!(
            closed.is_some() && asked.is_some() && closed < asked,
            "the old connection closes before the lease is asked for: {body}"
        );
    }

    /// How many times `src` names `Pane::launch` or `PtyHost::spawn`, called
    /// or not, under whatever name a `use … as` or a `type` gives either:
    /// parsed, so no spelling slips past. What it can't see: a macro that
    /// builds the path, and a call through a trait object or a function
    /// pointer stored elsewhere, which would have to name the function here
    /// first.
    /// Whether `attrs` hold `#[cfg(test)]`: the parsed form of
    /// `guard_support::production_half`, whose text cut can leave a file syn can't read.
    fn test_only(attrs: &[syn::Attribute]) -> bool {
        attrs.iter().any(|a| {
            a.path().is_ident("cfg") && a.parse_args::<syn::Ident>().is_ok_and(|i| i == "test")
        })
    }

    /// An item's attributes, for the kinds that carry `#[cfg(test)]` here.
    fn item_attrs(item: &syn::Item) -> &[syn::Attribute] {
        match item {
            syn::Item::Fn(i) => &i.attrs,
            syn::Item::Mod(i) => &i.attrs,
            syn::Item::Impl(i) => &i.attrs,
            syn::Item::Const(i) => &i.attrs,
            syn::Item::Static(i) => &i.attrs,
            syn::Item::Use(i) => &i.attrs,
            syn::Item::Struct(i) => &i.attrs,
            syn::Item::Enum(i) => &i.attrs,
            syn::Item::Trait(i) => &i.attrs,
            syn::Item::Type(i) => &i.attrs,
            syn::Item::Macro(i) => &i.attrs,
            _ => &[],
        }
    }

    /// How many launches `src` makes: `Pane::launch` and `PtyHost::spawn`,
    /// under any name a `use … as` or a `type` gives either here (a rename
    /// of a rename too), written as `<Pane>::launch` or `<Pane as T>::launch`,
    /// or as `Self::launch` in an `impl` of either. Not seen: a name given in
    /// another file (`pub use Pane as P` re-exported), a path built inside a
    /// macro's tokens, and a launch reached through a value whose type isn't
    /// written (a function pointer passed in from elsewhere).
    fn launches(src: &str, path: &std::path::Path) -> usize {
        use syn::visit::Visit;
        struct Names {
            pane: std::collections::BTreeSet<String>,
            host: std::collections::BTreeSet<String>,
            /// The type of the `impl` being read: `Self` names it.
            in_impl: Option<String>,
            found: usize,
        }
        impl Names {
            fn names(&self, ty: &str, f: &str) -> bool {
                let ty = match (ty, &self.in_impl) {
                    ("Self", Some(own)) => own.as_str(),
                    _ => ty,
                };
                (f == "launch" && self.pane.contains(ty))
                    || (f == "spawn" && self.host.contains(ty))
            }
            /// Learn `alias` as a name of `of`'s type; whether it is new.
            fn learn(&mut self, of: &str, alias: String) -> bool {
                if self.pane.contains(of) {
                    self.pane.insert(alias)
                } else if self.host.contains(of) {
                    self.host.insert(alias)
                } else {
                    false
                }
            }
        }
        fn last(ty: &syn::Type) -> Option<String> {
            match ty {
                syn::Type::Path(t) => t.path.segments.last().map(|s| s.ident.to_string()),
                _ => None,
            }
        }
        impl<'a> Visit<'a> for Names {
            fn visit_item(&mut self, item: &'a syn::Item) {
                if !test_only(item_attrs(item)) {
                    syn::visit::visit_item(self, item);
                }
            }
            fn visit_impl_item_fn(&mut self, f: &'a syn::ImplItemFn) {
                if !test_only(&f.attrs) {
                    syn::visit::visit_impl_item_fn(self, f);
                }
            }
            fn visit_item_impl(&mut self, item: &'a syn::ItemImpl) {
                let own = std::mem::replace(&mut self.in_impl, last(&item.self_ty));
                syn::visit::visit_item_impl(self, item);
                self.in_impl = own;
            }
            fn visit_use_rename(&mut self, rename: &'a syn::UseRename) {
                self.learn(&rename.ident.to_string(), rename.rename.to_string());
            }
            fn visit_item_type(&mut self, item: &'a syn::ItemType) {
                if let Some(of) = last(&item.ty) {
                    self.learn(&of, item.ident.to_string());
                }
            }
            fn visit_expr_path(&mut self, e: &'a syn::ExprPath) {
                // `<Pane>::launch`: the type is the path's `qself`, not a segment.
                if let Some(q) = &e.qself
                    && let (Some(ty), Some(f)) = (last(&q.ty), e.path.segments.last())
                    && self.names(&ty, &f.ident.to_string())
                {
                    self.found += 1;
                }
                syn::visit::visit_expr_path(self, e);
            }
            fn visit_path(&mut self, path: &'a syn::Path) {
                let names: Vec<String> =
                    path.segments.iter().map(|s| s.ident.to_string()).collect();
                if let [.., ty, f] = names.as_slice()
                    && self.names(ty, f)
                {
                    self.found += 1;
                }
                syn::visit::visit_path(self, path);
            }
        }
        let file = syn::parse_file(src).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        if test_only(&file.attrs) {
            return 0;
        }
        let mut names = Names {
            pane: ["Pane".to_string()].into(),
            host: ["PtyHost".to_string()].into(),
            in_impl: None,
            found: 0,
        };
        // Every name first, a rename of a rename among them, then the
        // launches under them.
        loop {
            let known = names.pane.len() + names.host.len();
            names.visit_file(&file);
            if names.pane.len() + names.host.len() == known {
                break;
            }
        }
        names.found = 0;
        names.visit_file(&file);
        names.found
    }

    /// The guard's reading: a launch counts under any name it goes by.
    #[test]
    fn a_launch_under_another_name_is_counted() {
        for (src, n) in [
            ("fn f() { Pane::launch(x); }", 1),
            ("use crate::pane::Pane as P; fn f() { P::launch(x); }", 1),
            ("type H = PtyHost; fn f() { H::spawn(x); }", 1),
            ("fn f() { crate::pane::pty_host::PtyHost::spawn(x); }", 1),
            ("fn f() { other::launch(x); Pane::new(x); }", 0),
            ("fn f() { <crate::pane::Pane>::launch(x); }", 1),
            ("fn f() { <Pane as Launches>::launch(x); }", 1),
            ("impl Pane { fn f() { Self::launch(x); } }", 1),
            ("impl PtyHost { fn f() { Self::spawn(x); } }", 1),
            (
                "impl Other { fn f() { Self::launch(x); Self::spawn(x); } }",
                0,
            ),
            (
                "use crate::pane::Pane as P; use P as Q; fn f() { Q::launch(x); }",
                1,
            ),
            ("type A = Pane; type B = A; fn f() { B::launch(x); }", 1),
            ("fn f() { let go = Pane::launch; go(x); }", 1),
        ] {
            assert_eq!(launches(src, std::path::Path::new("t.rs")), n, "{src}");
        }
    }

    /// SPYC-TRAP `jar-never-twice`: in `src`, a program reaches a PTY
    /// only through `launch_tab_pane` (a tab's, which asks the jar its
    /// record names), a capture's start, or an overlay, which has no record;
    /// and `launch_tab_pane` asks before it launches. `pane/launch.rs`'s one
    /// is `Pane::launch`'s own `PtyHost::spawn`. Read as [`launches`] does,
    /// which says what it can't see. Fails closed.
    #[test]
    fn no_start_with_a_record_reaches_a_pty_around_its_jar() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut launching = std::collections::BTreeMap::new();
        let mut dirs = vec![root.clone()];
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                // Test-only modules, by their names.
                let tests = name == "tests.rs"
                    || name.ends_with("_tests.rs")
                    || name == "test_dir.rs"
                    || name == "test_support.rs";
                if path.is_dir() {
                    if !matches!(name.as_str(), "harness_tests" | "tests") {
                        dirs.push(path);
                    }
                    continue;
                }
                if path.extension().is_none_or(|x| x != "rs") || tests {
                    continue;
                }
                let src = std::fs::read_to_string(&path).unwrap();
                let n = launches(&src, &path);
                if n > 0 {
                    launching.insert(path.strip_prefix(&root).unwrap().display().to_string(), n);
                }
            }
        }
        let expected: std::collections::BTreeMap<String, usize> = [
            ("app/capture_starts.rs", 1),
            ("app/pager_handler/mod.rs", 1),
            ("app/pane_starts.rs", 2),
            ("pane/launch.rs", 1),
        ]
        .into_iter()
        .map(|(f, n)| (f.to_string(), n))
        .collect();
        assert_eq!(launching, expected, "every launch in src/app, by file");
        let starts = crate::guard_support::production_half(include_str!("pane_starts.rs"));
        let tab = starts
            .split("fn launch_tab_pane(")
            .nth(1)
            .and_then(|body| body.split("\n    fn ").next())
            .expect("premise: launch_tab_pane is there");
        let asked = tab.find("jar::held(record.socket())");
        let launched = tab.find("Pane::launch(");
        assert!(
            asked.is_some() && launched.is_some() && asked < launched,
            "launch_tab_pane asks the record's jar before it launches: {tab}"
        );
        assert!(
            tab.contains("crate::pane::Launch { record, ..launch }"),
            "a tab launched here carries its record to the PTY's own check"
        );
        assert!(
            starts.contains("record: taking.socket(),"),
            "a worker's launch carries its record"
        );
    }

    /// SPYC-TRAP `jar-off-the-loop`: outside `src/pane`, only this module's
    /// worker, and `capture_starts`'s, asks for a jar, since a launch given one waits on the jar for as
    /// long as it holds the program. Anywhere else, that wait would freeze the
    /// loop.
    #[test]
    fn only_a_worker_asks_for_a_tabs_jar() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let needles = [
            "jar::spec_for(",
            "jar::wanted(",
            "jar::spawn(",
            "jar::Spec {",
            "for_this_user(",
        ];
        let mut asking = Vec::new();
        let mut dirs = vec![root.clone()];
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    if path != root.join("pane") {
                        dirs.push(path);
                    }
                    continue;
                }
                if path.extension().is_none_or(|x| x != "rs") {
                    continue;
                }
                let src =
                    crate::guard_support::production_half(&std::fs::read_to_string(&path).unwrap());
                if needles.iter().any(|n| src.contains(n)) {
                    asking.push(path.strip_prefix(&root).unwrap().to_path_buf());
                }
            }
        }
        asking.sort();
        assert_eq!(
            asking,
            [
                std::path::PathBuf::from("app/capture_starts.rs"),
                std::path::PathBuf::from("app/pane_starts.rs")
            ],
            "a tab's or capture's jar is asked for off the loop"
        );
    }
}
