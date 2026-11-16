//! A jarred tab's start, off the loop (finding 5). Starting a jar, or taking
//! a restored tab's back, waits on it: the store's check of the first jar,
//! a jar's first run, an upgrade it's parked in. So the tab opens at once on
//! a stand-in that says so, a worker launches its pane, and the pane takes
//! the stand-in's place when it lands ([`App::apply_pane_starts`], in the
//! pre-recv drain). A jar that never started its program leaves the tab
//! `[not started]`, why on its screen, and `^a R` starts it again.
//!
//! What's typed into a starting tab is dropped: nothing runs to read it, and
//! keys held back would reach a program that has drawn a prompt they weren't
//! typed for. A tab closed or restarted while it starts takes nothing: the
//! pane that lands for it is ended at once.

use crate::pane::jar;
use crate::state::sessions::SavedTab;

use super::landing::spawn_worker;
use super::state::ProjectId;
use super::{App, Pane, Wake};

/// A jarred tab's pane, launched on a worker, or why it wasn't.
pub(super) struct PaneStart {
    project: ProjectId,
    /// The tab's id (`TabInfo::id`): a tab restarted meanwhile has a new one.
    tab: String,
    pane: anyhow::Result<Pane>,
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
}

impl JarredLaunch {
    /// Launch the pane, its program in the jar `place` (this user's when
    /// `None`) starts or a restored tab's gives back.
    fn run(
        self,
        tab: &str,
        place: Option<jar::Place>,
        restoring: Option<&SavedTab>,
        wake: crate::pane::PaneWake,
    ) -> anyhow::Result<Pane> {
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
                // SPYC-TRAP(jar-off-the-loop): a jar's spec is made only on this
                // worker; with one, the launch waits on the jar.
                jar: jar::spec_for(place, tab, restoring),
            },
            wake,
        )
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
    /// worker; otherwise `launch`'s, its program on a PTY of spyc's.
    pub(super) fn launch_tab_pane(
        &self,
        launch: crate::pane::Launch,
        jars: bool,
        tab: &str,
        restoring: Option<&SavedTab>,
        wake: crate::pane::PaneWake,
    ) -> anyhow::Result<Pane> {
        if !(jars && jar::honoured()) {
            return Pane::launch(launch, wake);
        }
        let owned = JarredLaunch {
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
        };
        self.start_jarred_pane(owned, tab, restoring, wake)
    }

    /// A jarred tab's stand-in pane, its own pane launched on a worker.
    fn start_jarred_pane(
        &self,
        launch: JarredLaunch,
        tab: &str,
        restoring: Option<&SavedTab>,
        wake: crate::pane::PaneWake,
    ) -> anyhow::Result<Pane> {
        let size = (launch.rows, launch.cols);
        let socket = restoring.and_then(|s| s.jar_socket.clone());
        let note = if restoring.is_some() {
            "Taking this tab's program back from its jar…"
        } else {
            "Starting this tab's jar…"
        };
        let stand_in = Pane::standing_in(
            jar::starting(note, socket.as_deref(), size),
            true,
            size,
            wake.clone(),
        )?;
        // A startup tab starts before `run()`: its landing wakes the loop
        // once it runs.
        let results = self
            .runtime
            .pane_starts
            .sender_waking(self.loop_waker(Wake::PaneStart));
        let project = self.state.active;
        let place = self.runtime.jar_place.clone();
        let restoring = restoring.cloned();
        let (tab, again) = (tab.to_string(), tab.to_string());
        spawn_worker(
            results,
            move || {
                let pane = launch.run(&tab, place, restoring.as_ref(), wake);
                PaneStart { project, tab, pane }
            },
            move |panic| PaneStart {
                project,
                tab: again,
                pane: Err(anyhow::anyhow!("the tab's start stopped: {panic}")),
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
            if start.project == self.state.active {
                self.land_pane_start(start.tab, start.pane);
            } else if self.state.has_project(start.project) {
                self.land_in(start.project, |app| {
                    app.land_pane_start(start.tab, start.pane);
                });
            } else if let Ok(pane) = start.pane {
                pane.shutdown_detached(END_WAIT);
            }
        }
        true
    }

    /// `pane` in the place of the tab `tab`'s stand-in, or, when it never
    /// started its program, a stand-in saying why.
    fn land_pane_start(&mut self, tab: String, pane: anyhow::Result<Pane>) {
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
        let mut pane = match pane {
            Ok(pane) => pane,
            Err(e) => {
                let why = format!("spyc: {e:#}\n\n^a R starts it again\n");
                match Pane::standing_in(jar::unstarted(&why, size), false, size, wake) {
                    Ok(pane) => pane,
                    Err(e) => {
                        self.state.flash_error(format!("pane: {e:#}"));
                        return;
                    }
                }
            }
        };
        let _ = pane.resize(size.0, size.1);
        settle_landed_tab(&mut entry.info, &pane, started);
        let stand_in = std::mem::replace(&mut entry.pane, pane);
        stand_in.shutdown_detached(END_WAIT);
        self.view.needs_full_repaint = true;
    }
}

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

#[cfg(test)]
mod tests {
    /// SPYC-TRAP `jar-off-the-loop`: outside `src/pane`, only this module's
    /// worker asks for a jar, since a launch given one waits on the jar for as
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
        assert_eq!(
            asking,
            [std::path::PathBuf::from("app/pane_starts.rs")],
            "a tab's jar is asked for off the loop"
        );
    }
}
