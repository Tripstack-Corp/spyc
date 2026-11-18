//! `^d` and `:detach` (S4): the terminal goes, and spyc stays. spyc in its
//! own jar (`pane::jar::session`) writes its terminal's reset and has the jar
//! end the client (`settle_detach`). A plain spyc hands itself over first: it
//! leaves the loop, saves the session as no running spyc's, starts the
//! session's own jar holding a spyc for the first terminal to attach, ends each
//! jar's connection as a crash would, and becomes `spyc attach` for that jar
//! (`hand_over`, from `lib.rs`). The spyc the jar starts restores the session
//! by its name and takes the jars back (`restore_own_session`).

use anyhow::Context;

use super::App;
use super::proc::ForegroundExec;
use super::tasks::TaskStatus;
use crate::Tui;

/// A plain spyc's handover, left for `lib.rs` once the loop ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handover {
    /// The session's name: its jar's.
    pub name: String,
}

impl App {
    /// Quit, or with `detach`, detach: the keys and commands that leave spyc.
    pub(super) fn leave(&mut self, detach: bool) {
        if detach {
            self.detach();
        } else {
            self.request_quit();
        }
    }

    /// `^d` / `:detach`.
    pub(super) fn detach(&mut self) {
        if self.runtime.own_jar.is_some() {
            self.runtime.detach_pending = true;
            return;
        }
        if let Some(what) = self.unjarred() {
            self.state.flash_error(format!(
                "can't detach: {what} runs outside a jar and would end with spyc"
            ));
            return;
        }
        let Some(name) = self.state.session_name.clone() else {
            self.state
                .flash_error("can't detach: the session has no name");
            return;
        };
        self.runtime.handover = Some(Handover { name });
        self.state.should_quit = true;
    }

    /// What runs outside a jar, which a handover would end.
    fn unjarred(&self) -> Option<String> {
        for (_, tabs) in self.runtime.every_tab_set() {
            if let Some(entry) = tabs
                .tabs()
                .iter()
                .find(|e| !e.pane.is_closed() && !e.pane.host.jarred())
            {
                return Some(format!("the tab {}", entry.info.label));
            }
        }
        self.runtime
            .background_tasks
            .tasks
            .iter()
            .find(|t| matches!(t.status, TaskStatus::Running) && !t.host.jarred())
            .map(|t| format!("task {}", t.id))
    }

    /// The handover the loop ended for, if it did.
    pub const fn take_handover(&mut self) -> Option<Handover> {
        self.runtime.handover.take()
    }

    /// Hand the session over, once the loop has ended and the terminal is
    /// restored: save it, start its jar, leave every jar running, and become
    /// `spyc attach` for it. Returns only when that fails, saying what runs on.
    pub fn hand_over(&mut self, handover: &Handover) -> anyhow::Result<()> {
        use std::os::unix::process::CommandExt;
        let name = &handover.name;
        // Saved as no running spyc's: this process stays alive as `spyc
        // attach`, and a session whose spyc runs is restored as a copy.
        let mut manifest = self.build_manifest();
        manifest.pid = None;
        manifest.process_started = None;
        if let Err(e) = crate::state::sessions::save_session(&manifest) {
            eprintln!("spyc: {name} wasn't saved ({e:#}); its tabs run on in their jars");
        }
        let exe = std::env::current_exe().context("where spyc runs from")?;
        let cwd = std::env::current_dir().context("spyc's directory")?;
        let size = crossterm::terminal::size().map_or((24, 80), |(cols, rows)| (rows, cols));
        let started =
            crate::pane::jar::session::start(name, vec![exe.display().to_string()], &cwd, size);
        self.release_jars();
        started.with_context(|| {
            format!("{name}'s tabs run on in their jars: `spyc -r` takes them back")
        })?;
        let err = std::process::Command::new(&exe)
            .args(["attach", name])
            .exec();
        Err(err).with_context(|| format!("become `spyc attach {name}`, which you can run"))
    }

    /// Leave every jar running for the next spyc, its connection ended as a
    /// crash would, and clear what this spyc kept for itself.
    fn release_jars(&mut self) {
        for (_, tabs) in self.runtime.every_tab_set() {
            for entry in tabs.tabs() {
                entry.pane.host.sever_jar();
            }
        }
        for task in &self.runtime.background_tasks.tasks {
            task.host.sever_jar();
        }
        for path in self.view.context_paths() {
            crate::context::remove_context_file(path);
        }
        self.clean_all_staging();
        self.runtime.capture_spill_dir.take();
    }

    /// spyc in its own jar: a `^d` writes the terminal's reset, then has the
    /// jar end the client, which the jar does once the reset has reached it.
    pub(super) fn settle_detach(&mut self, terminal: &mut Tui) {
        if !std::mem::take(&mut self.runtime.detach_pending) {
            return;
        }
        let Some(jar) = &self.runtime.own_jar else {
            return;
        };
        let reset = crate::terminal::detach_reset(terminal);
        if !jar.detach_client() {
            self.state.flash_error("detach: spyc's jar didn't answer");
        } else if let Err(e) = reset {
            self.state
                .flash_error(format!("detach: the terminal's reset: {e:#}"));
        }
    }

    /// spyc in its own jar, at the loop's start: hear the jar, and restore the
    /// session of its name.
    pub(super) fn start_own_jar(&mut self, terminal: &mut Tui, foreground_exec: &ForegroundExec) {
        self.watch_own_jar();
        self.restore_own_session(terminal, foreground_exec);
    }

    /// spyc in its own jar, at its start: the session of the jar's name comes
    /// back, saved by the spyc that handed it over, its tabs taken back from
    /// their jars. A session whose spyc still runs isn't taken.
    fn restore_own_session(&mut self, terminal: &mut Tui, foreground_exec: &ForegroundExec) {
        let Some(name) = self.runtime.own_jar.as_ref().map(|j| j.name().to_string()) else {
            return;
        };
        let Some(manifest) = crate::state::sessions::load_manifests()
            .into_iter()
            .filter(|m| m.name == name && !m.running())
            .max_by_key(|m| m.epoch_secs)
        else {
            return;
        };
        let effects = self.restore_session(&manifest);
        self.run_effects(effects, terminal, foreground_exec);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detach_is_refused_while_a_tab_runs_outside_a_jar() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(&tmp.path().join("state"), || {
            let dir = std::fs::canonicalize(tmp.path()).unwrap();
            let mut app = App::test_app(dir.clone());
            assert!(app.open_pane_tab_in("sh", &dir));
            app.detach();
            assert_eq!(app.take_handover(), None);
            assert!(!app.state.should_quit, "a refused detach doesn't quit");
            let flash = app.flash_text().unwrap_or_default();
            assert!(flash.contains("outside a jar"), "{flash}");
        });
    }

    #[test]
    fn with_everything_in_jars_detach_leaves_the_loop_for_a_handover() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(&tmp.path().join("state"), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            app.state.session_name = Some("ANISE".into());
            app.detach();
            assert!(app.state.should_quit);
            assert_eq!(
                app.take_handover(),
                Some(Handover {
                    name: "ANISE".into()
                })
            );
        });
    }
}
