//! Jars no spyc owns, offered back at a spyc's start (S2c). Once startup has
//! settled and every restored tab, capture and task has taken its jar back,
//! a worker looks, once, for live jars with no owner whose token this spyc
//! keeps (`jar::orphans`): what a crash left that no `spyc -r` took back.
//! spyc names them in a pop-up (`PromptKind::OrphanJars`) and offers to
//! restore them, each pane as a tab in the project it was in and each task or
//! capture as a background task, or to discard them, ending their programs.
//! Not now leaves them running, for `spyc --jars` to list.

use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent};

use crate::pane::jar::orphans::Orphan;
use crate::state::sessions::{AgentKind, SavedTab, SavedTask};

use super::landing::spawn_worker;
use super::{App, Effect, Mode, Prompt, PromptKind, Wake};

/// Where this session's look for jars no spyc owns is.
#[derive(Default)]
pub(super) struct OrphanJars {
    /// The look has started; it's made once a session.
    looked: bool,
    /// What it found that this spyc doesn't hold, waiting on the answer.
    found: Vec<Orphan>,
}

impl App {
    /// Start the look for jars no spyc owns, once startup has settled and no
    /// jar of this spyc's is still being started or taken back. Whether it
    /// started.
    pub(super) fn settle_orphan_jars(&mut self) -> bool {
        if self.runtime.orphan_jars.looked || !self.startup_settled_for_jars() {
            return false;
        }
        self.runtime.orphan_jars.looked = true;
        let place = self.runtime.jar_place.clone();
        let results = self
            .runtime
            .orphan_results
            .sender_waking(self.loop_waker(Wake::PaneStart));
        spawn_worker(
            results,
            move || crate::pane::jar::orphans::find(place),
            |_| Vec::new(),
        );
        true
    }

    /// Whether this session runs programs in jars, and startup has settled:
    /// no session waits to be restored, no startup command or Lua startup is
    /// pending, and no jar of this spyc's is still being started or taken back.
    pub(super) fn startup_settled_for_jars(&self) -> bool {
        self.jars_wanted()
            && self.state.pending_sessions.is_none()
            && self.state.startup_commands.is_empty()
            && !self.runtime.lua_events.startup_pending
            && !self.jar_start_pending()
    }

    /// Whether this session runs programs in jars.
    fn jars_wanted(&self) -> bool {
        self.state.active_project().config.pane.jars
            && crate::pane::jar::honoured()
            && !self.runtime.jarless.chosen
    }

    /// Whether a tab, capture or task of this spyc's is still starting or
    /// taking back its jar, which the look would take for an orphan.
    fn jar_start_pending(&self) -> bool {
        self.runtime
            .every_tab_set()
            .any(|(_, tabs)| tabs.tabs().iter().any(|e| e.pane.starting()))
            || self
                .runtime
                .pending_capture
                .as_ref()
                .is_some_and(|c| c.jar_start.is_some())
            || self
                .runtime
                .background_tasks
                .tasks
                .iter()
                .any(|t| t.jar_start.is_some())
    }

    /// What the look found, less the jars this spyc holds; offered. Whether
    /// any landed.
    pub(crate) fn apply_orphan_jars(&mut self) -> bool {
        let landed = self.runtime.orphan_results.drain();
        let landed: Vec<Orphan> = landed.into_iter().flatten().collect();
        if landed.is_empty() {
            return false;
        }
        let held = self.held_jar_sockets();
        self.runtime.orphan_jars.found = landed
            .into_iter()
            .filter(|o| !held.contains(&o.socket))
            .collect();
        self.offer_orphan_jars();
        true
    }

    /// The jar sockets this spyc's tabs, capture and tasks hold.
    fn held_jar_sockets(&self) -> Vec<PathBuf> {
        let tabs: Vec<PathBuf> = self
            .runtime
            .every_tab_set()
            .flat_map(|(_, tabs)| tabs.tabs().iter())
            .filter_map(|e| e.pane.jar_socket().map(Path::to_path_buf))
            .collect();
        let capture = self
            .runtime
            .pending_capture
            .iter()
            .filter_map(|c| c.host.jar_socket().map(Path::to_path_buf));
        let tasks = self
            .runtime
            .background_tasks
            .tasks
            .iter()
            .filter_map(|t| t.host.jar_socket().map(Path::to_path_buf));
        tabs.into_iter().chain(capture).chain(tasks).collect()
    }

    /// Ask what to do with the jars found, when nothing else is being asked.
    /// Whether it asked.
    pub(super) fn offer_orphan_jars(&mut self) -> bool {
        let found = &self.runtime.orphan_jars.found;
        if found.is_empty() || !matches!(self.state.mode, Mode::Normal) {
            return false;
        }
        let names: Vec<String> = found.iter().map(described).collect();
        self.state.mode = Mode::Prompting(Prompt::simple(
            PromptKind::OrphanJars,
            format!(
                "{} jar(s) no spyc owns, their programs still running: {}. Restore them \
                 here, or discard them, ending their programs?",
                found.len(),
                names.join(", ")
            ),
        ));
        true
    }

    /// The answer: `r` restores them, `d` discards them, `Esc` leaves them
    /// running, and any other key asks again.
    pub(in crate::app) fn handle_orphan_jars_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        let prompt = std::mem::replace(&mut self.state.mode, Mode::Normal);
        match key.code {
            KeyCode::Char('r' | 'R') => {
                let found = std::mem::take(&mut self.runtime.orphan_jars.found);
                let n = found.len();
                self.restore_orphans(found);
                self.state
                    .flash_info(format!("jars: taking back {n} no spyc owned"));
            }
            KeyCode::Char('d' | 'D') => {
                let found = std::mem::take(&mut self.runtime.orphan_jars.found);
                let n = found.len();
                let place = self.runtime.jar_place.clone();
                std::thread::spawn(move || {
                    for orphan in found {
                        let _ = crate::pane::jar::orphans::discard(place.clone(), &orphan.socket);
                    }
                });
                self.state
                    .flash_info(format!("jars: ending {n} no spyc owned"));
            }
            KeyCode::Esc => {
                self.runtime.orphan_jars.found.clear();
                self.state
                    .flash_info("jars: left running (spyc --jars lists them)");
            }
            _ => self.state.mode = prompt,
        }
        Vec::new()
    }

    /// Take back each of `found`: a pane as a tab in the open project it was
    /// in (else this one), a task or capture as a background task.
    fn restore_orphans(&mut self, found: Vec<Orphan>) {
        let (panes, tasks): (Vec<Orphan>, Vec<Orphan>) = found
            .into_iter()
            .partition(|o| o.is != "task" && o.is != "capture");
        let tasks: Vec<SavedTask> = tasks
            .into_iter()
            .map(|o| SavedTask {
                title: o.label,
                cmd_display: o.command,
                jar_socket: o.socket,
            })
            .collect();
        self.restore_tasks(&tasks);
        for orphan in panes {
            let project = self
                .state
                .open_projects()
                .find(|p| p.anchor().display().to_string() == orphan.project_home)
                .map_or(self.state.active, |p| p.id);
            let saved = saved_tab(orphan);
            self.land_in(project, |app| {
                app.spawn_pane_tab(
                    &saved.command,
                    &saved.cwd,
                    super::pane_tabs::TabSlot::Append,
                    None,
                    Some(&saved),
                    None,
                )
            });
        }
    }
}

/// An orphan as the pop-up names it.
fn described(o: &Orphan) -> String {
    let name = if o.label.is_empty() {
        &o.command
    } else {
        &o.label
    };
    match (o.is.as_str(), o.project.is_empty()) {
        ("task" | "capture", _) => format!("{name} ({})", o.is),
        (_, true) => name.clone(),
        (_, false) => format!("{name} (in {})", o.project),
    }
}

/// A tab to restore around an orphan's jar, as a saved session would have it.
fn saved_tab(o: Orphan) -> SavedTab {
    let command = if o.command.is_empty() {
        o.label.clone()
    } else {
        o.command
    };
    let cwd = [o.cwd.as_str(), o.project_home.as_str()]
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.is_dir())
        .unwrap_or_else(std::env::temp_dir);
    SavedTab {
        label: if o.label.is_empty() {
            command.clone()
        } else {
            o.label
        },
        command,
        cwd,
        agent_kind: agent_kind(&o.agent),
        agent_session_id: None,
        agent_session_name: None,
        codex_home: None,
        codex_home_unresolved: false,
        claude_home: None,
        claim_owner: String::new(),
        pane_id: if o.pane_id.is_empty() {
            uuid::Uuid::now_v7().to_string()
        } else {
            o.pane_id
        },
        agent_hint: None,
        unopened: false,
        jar_socket: Some(o.socket),
    }
}

/// An agent by the name the jar was told.
fn agent_kind(name: &str) -> AgentKind {
    match name {
        "claude" => AgentKind::Claude,
        "codex" => AgentKind::Codex,
        "agy" => AgentKind::Agy,
        "zot" => AgentKind::Zot,
        _ => AgentKind::Other,
    }
}
