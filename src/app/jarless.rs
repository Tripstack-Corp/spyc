//! Jars on, and no usable `spyc-jar` (none beside spyc, one that can't run,
//! or another build's): a tab whose jar can't start for it ends
//! `[not started]`, saying to check spyc's installation, and spyc offers,
//! once a session, to run tabs without jars (`PromptKind::JarlessFallback`).
//! Taken, each such tab starts again on a PTY of spyc's own, and every tab
//! after starts there too until spyc quits; declined, they stay
//! `[not started]`. A tab's program never runs outside a jar unless the user
//! says so: here, or with `[pane] jars = false` (SPYC-TRAP `jar-or-nothing`).
//!
//! A restored tab whose jar still runs isn't started again: its program
//! would run twice.

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent};

use super::state::ProjectId;
use super::{App, Effect, Mode, Prompt, PromptKind};
use crate::pane::jar::place::{SocketState, socket_state};

/// This session's answer, and the tabs waiting on it.
#[derive(Default)]
pub(super) struct Jarless {
    /// The user chose to run tabs without jars.
    pub(super) chosen: bool,
    asked: bool,
    /// Why no jar is usable, as the store or the jar's start said.
    why: String,
    waiting: Vec<Waiting>,
}

/// A tab that didn't start for want of a usable jar.
struct Waiting {
    project: ProjectId,
    tab: String,
    /// A restored tab's jar, which may still run.
    socket: Option<PathBuf>,
}

impl App {
    /// The tab `tab` of `project` didn't start for want of a usable jar
    /// (`why`): start it again without one if the user chose that, or hold
    /// it for the offer. Runs in `project`.
    pub(super) fn tab_has_no_usable_jar(
        &mut self,
        project: ProjectId,
        tab: String,
        socket: Option<PathBuf>,
        why: &str,
    ) {
        let waiting = Waiting {
            project,
            tab,
            socket,
        };
        if self.runtime.jarless.chosen {
            self.start_without_jar(&waiting);
            return;
        }
        if self.runtime.jarless.asked {
            return;
        }
        why.clone_into(&mut self.runtime.jarless.why);
        self.runtime.jarless.waiting.push(waiting);
        self.offer_jarless();
    }

    /// Ask whether to run tabs without jars, when a tab waits on the answer
    /// and nothing else is being asked. Whether it asked.
    pub(super) fn offer_jarless(&mut self) -> bool {
        let jarless = &self.runtime.jarless;
        if jarless.asked || jarless.waiting.is_empty() || !matches!(self.state.mode, Mode::Normal) {
            return false;
        }
        self.state.mode = Mode::Prompting(Prompt::simple(
            PromptKind::JarlessFallback,
            format!(
                "Tabs can't run in jars: no usable spyc-jar ({}). Check spyc's installation: \
                 spyc-jar belongs beside spyc. Run this session's tabs without jars? Their \
                 programs then end when spyc does; `[pane] jars = false` runs every session \
                 that way.",
                jarless.why
            ),
        ));
        true
    }

    /// The answer: `y` runs tabs without jars, `n` or `Esc` leaves them
    /// `[not started]`, and any other key asks again.
    pub(in crate::app) fn handle_jarless_fallback_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        let prompt = std::mem::replace(&mut self.state.mode, Mode::Normal);
        match key.code {
            KeyCode::Char('y' | 'Y') => {
                self.runtime.jarless.asked = true;
                self.runtime.jarless.chosen = true;
                let waiting = std::mem::take(&mut self.runtime.jarless.waiting);
                let kept = waiting
                    .iter()
                    .filter(|w| !self.start_without_jar(w))
                    .count();
                let note = match kept {
                    0 => String::new(),
                    n => format!("; {n} restored tab(s) left: their jars still run"),
                };
                self.state
                    .flash_info(format!("pane: tabs run without jars this session{note}"));
            }
            KeyCode::Char('n' | 'N') | KeyCode::Esc => {
                self.runtime.jarless.asked = true;
                self.runtime.jarless.waiting.clear();
                self.state.flash_info(
                    "pane: tabs need a usable spyc-jar; `[pane] jars = false` runs them without one",
                );
            }
            _ => self.state.mode = prompt,
        }
        Vec::new()
    }

    /// Start `waiting`'s tab again, without a jar. Whether it did: a
    /// restored tab whose jar may still run isn't, nor one closed meanwhile.
    fn start_without_jar(&mut self, waiting: &Waiting) -> bool {
        if waiting
            .socket
            .as_deref()
            .is_some_and(|s| matches!(socket_state(s), SocketState::Live | SocketState::Unknown))
        {
            return false;
        }
        let tab = waiting.tab.clone();
        self.land_in(waiting.project, |app| {
            let idx = app
                .runtime
                .project()
                .pane_tabs
                .as_ref()?
                .tabs()
                .iter()
                .position(|e| e.info.id == tab)?;
            app.restart_tab_now(idx);
            Some(())
        })
        .flatten()
        .is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;
    use std::time::{Duration, Instant};

    fn lines(path: &std::path::Path) -> usize {
        std::fs::read_to_string(path).map_or(0, |t| t.lines().count())
    }

    /// Finding 7: a restored tab whose jar may still run (here one nothing
    /// can tell about) isn't started again without a jar, even once the user
    /// chose that: its program would run twice.
    #[test]
    fn a_tab_whose_jar_may_still_run_is_not_started_without_one() {
        let dir = tempfile::tempdir().unwrap();
        crate::state::with_state_root(dir.path(), || {
            let mut app = App::test_app(dir.path().to_path_buf());
            let ran = dir.path().join("ran");
            let program = format!("sh -c 'echo once >> {}; exec cat'", ran.display());
            assert!(app.open_pane_tab_in(&program, dir.path()));
            let until = Instant::now() + Duration::from_secs(20);
            while lines(&ran) == 0 {
                assert!(Instant::now() < until, "the program never ran");
                std::thread::sleep(Duration::from_millis(20));
            }
            let socket = dir.path().join("unknown.sock");
            std::fs::write(&socket, "").unwrap();
            let tab = app
                .runtime
                .project()
                .pane_tabs
                .as_ref()
                .unwrap()
                .active_info()
                .id
                .clone();
            app.tab_has_no_usable_jar(app.state.active, tab, Some(socket), "none here");
            app.handle_jarless_fallback_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));
            assert!(
                app.flash_text().is_some_and(|f| f.contains("still run")),
                "{:?}",
                app.flash_text()
            );
            std::thread::sleep(Duration::from_millis(300));
            assert_eq!(lines(&ran), 1, "the program ran twice");
            if let Some(tabs) = app.runtime.project_mut().pane_tabs.as_mut() {
                tabs.active_mut()
                    .shutdown_reporting(Duration::from_millis(500), || {});
            }
        });
    }
}
