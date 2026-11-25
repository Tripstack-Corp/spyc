//! `^d` and `:detach` (S4): the terminal goes, and spyc stays. spyc in its
//! own jar (`pane::jar::session`) writes its terminal's reset and has the jar
//! end the client (`settle_detach`). A plain spyc hands itself over first: it
//! leaves the loop, saves the session as no running spyc's, starts the
//! session's own jar holding a spyc for the first terminal to attach, ends each
//! jar's connection as a crash would, and becomes `spyc attach` for that jar
//! (`hand_over`, from `lib.rs`). The spyc the jar starts restores the session
//! by its name and takes the jars back (`restore_own_session`).
//!
//! `:upgrade` is the same handover to a newly installed spyc. In a plain spyc
//! it is `^d`'s, whose jar runs the binary now installed. In spyc's own jar
//! it is made in place: the session saved and its jars left running, spyc
//! `exec`s the new binary, which keeps the pid the jar knows its program by,
//! and the terminal stays attached while the new spyc restores the session.
//!
//! `:name NEW` renames the session (`name_session`). In its own jar the
//! socket, its address, stays; the name is written beside it
//! (`pane::jar::session::write_name`), where `spyc attach` and a spyc
//! restarted in that jar read it. A name another running session goes by, or
//! has for its address, is refused, so `spyc attach NAME` means one.

use anyhow::Context;

use super::App;
use super::proc::ForegroundExec;
use super::tasks::TaskStatus;
use crate::Tui;

/// A handover, left for `lib.rs` once the loop ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handover {
    /// The session's name: its jar's.
    pub name: String,
    /// An upgrade in spyc's own jar: `exec` the new binary there.
    pub in_place: bool,
}

/// The binary spyc runs from, as it was when spyc started: an install puts a
/// new file at the path, with another inode or a later modification time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryStamp {
    path: std::path::PathBuf,
    inode: u64,
    modified: Option<std::time::SystemTime>,
}

impl BinaryStamp {
    /// The file at `path` now.
    pub fn of(path: &std::path::Path) -> Option<Self> {
        use std::os::unix::fs::MetadataExt;
        let meta = std::fs::metadata(path).ok()?;
        Some(Self {
            path: path.to_path_buf(),
            inode: meta.ino(),
            modified: meta.modified().ok(),
        })
    }

    /// The binary spyc runs from, now.
    pub fn installed() -> Option<Self> {
        crate::paths::installed_spyc()
            .ok()
            .and_then(|p| Self::of(&p))
    }

    /// Whether `now` is another file than this one: a new install.
    pub fn replaced_by(&self, now: &Self) -> bool {
        self.inode != now.inode || self.modified != now.modified
    }
}

/// `:upgrade`: hand the session to the spyc installed since this one started.
pub(super) const fn cmd_upgrade(app: &mut App, _args: &str) -> Vec<super::Effect> {
    app.runtime.upgrade_pending = true;
    Vec::new()
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
        self.state.note_event(&format!(
            "detached: spyc moves into {name}'s own jar, its tabs running on"
        ));
        self.runtime.handover = Some(Handover {
            name,
            in_place: false,
        });
        self.state.should_quit = true;
    }

    /// `:upgrade`, with the loop: only to a binary installed since spyc
    /// started, in place in spyc's own jar, else by `^d`'s handover.
    fn upgrade(&mut self) {
        let (newer, now) = self.newer_install();
        if !newer {
            let at = now.map_or_else(String::new, |n| format!(" at {}", n.path.display()));
            self.state
                .flash_error(format!("upgrade: no newer spyc is installed{at}"));
            return;
        }
        if let Some(at) = &now {
            self.state.note_event(&format!(
                "upgrading to the spyc installed at {}",
                at.path.display()
            ));
        }
        match (&self.runtime.own_jar, self.state.session_name.clone()) {
            (Some(_), Some(name)) => {
                self.runtime.handover = Some(Handover {
                    name,
                    in_place: true,
                });
                self.state.should_quit = true;
            }
            _ => self.detach(),
        }
    }

    /// Whether a spyc newer than this one is installed where it runs from, and
    /// the file there now: `:upgrade`'s test, and what `spyc attach` asks.
    pub(super) fn newer_install(&self) -> (bool, Option<BinaryStamp>) {
        let now = BinaryStamp::installed();
        let newer = match (&self.runtime.started_from, &now) {
            (Some(then), Some(now)) => then.replaced_by(now),
            _ => false,
        };
        (newer, now)
    }

    /// What runs outside a jar, which a handover would end.
    pub(super) fn unjarred(&self) -> Option<String> {
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
        self.close_event_log();
        let name = &handover.name;
        if handover.in_place {
            return self.upgrade_in_place(name);
        }
        self.save_released(name);
        let exe = crate::paths::installed_spyc()?;
        let cwd = std::env::current_dir().context("spyc's directory")?;
        let size = crossterm::terminal::size().map_or((24, 80), |(cols, rows)| (rows, cols));
        let started = crate::pane::jar::session::start(
            name,
            vec![exe.display().to_string()],
            &cwd,
            size,
            crate::pane::jar::session::Begin::OnAttach,
        );
        self.release_jars();
        started.with_context(|| {
            format!("{name}'s tabs run on in their jars: `spyc -r` takes them back")
        })?;
        let err = std::process::Command::new(&exe)
            // `--force`: this terminal shows this spyc already, so nothing nests,
            // though a spyc started in another's pane has that pane's env.
            .args(["attach", "--force", name])
            .exec();
        Err(err).with_context(|| format!("become `spyc attach {name}`, which you can run"))
    }

    /// An upgrade in spyc's own jar: save, leave the jars running, and `exec`
    /// the new binary, which keeps this pid and so the jar, and restores the
    /// session. Returns only when the `exec` fails.
    fn upgrade_in_place(&mut self, name: &str) -> anyhow::Result<()> {
        use std::os::unix::process::CommandExt;
        let exe = crate::paths::installed_spyc()?;
        self.save_released(name);
        self.release_jars();
        let err = std::process::Command::new(&exe)
            .args(std::env::args_os().skip(1))
            .exec();
        Err(err).with_context(|| {
            format!(
                "run {}; {name}'s tabs run on in their jars: `spyc -r` takes them back",
                exe.display()
            )
        })
    }

    /// Save the session as no running spyc's: this process goes on as `spyc
    /// attach` or as the next spyc, and a session whose spyc runs is restored
    /// as a copy.
    fn save_released(&mut self, name: &str) {
        let mut manifest = self.build_manifest();
        manifest.pid = None;
        manifest.process_started = None;
        if let Err(e) = crate::state::sessions::save_session(&manifest) {
            eprintln!("spyc: {name} wasn't saved ({e:#}); its tabs run on in their jars");
        }
    }

    /// Leave every jar running for the next spyc, its connection ended as a
    /// crash would, and clear what this spyc kept for itself.
    pub(super) fn release_jars(&mut self) {
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

    /// `:upgrade`, and in spyc's own jar a `^d`: it writes the terminal's
    /// reset, then has the jar end the client, which the jar does once the
    /// reset has reached it. Whether there is anything to draw.
    pub(super) fn settle_detach(&mut self, terminal: &mut Tui) -> bool {
        let upgrade = std::mem::take(&mut self.runtime.upgrade_pending);
        if upgrade {
            self.upgrade();
        }
        if !std::mem::take(&mut self.runtime.detach_pending) {
            return upgrade;
        }
        let Some(jar) = &self.runtime.own_jar else {
            return upgrade;
        };
        let reset = crate::terminal::detach_reset(terminal);
        if !jar.detach_client() {
            self.state.flash_error("detach: spyc's jar didn't answer");
        } else if let Err(e) = reset {
            self.state
                .flash_error(format!("detach: the terminal's reset: {e:#}"));
        }
        true
    }

    /// The link tab `pane_id`'s program is given as its `SSH_AUTH_SOCK`,
    /// pointed at this spyc's terminal's agent (`pane::ssh_agent`); `None`
    /// with no agent, and the program keeps the one spyc has.
    pub(super) fn tab_ssh_agent(&self, pane_id: &str) -> Option<std::path::PathBuf> {
        use crate::pane::ssh_agent;
        let agent = self.runtime.ssh_agent.as_ref()?;
        let link = ssh_agent::tab_link(&ssh_agent::links_dir()?, pane_id);
        match ssh_agent::point(&link, agent) {
            Ok(()) => Some(link),
            Err(e) => {
                crate::spyc_debug!("ssh agent: no link for tab {pane_id}: {e:#}");
                None
            }
        }
    }

    /// A terminal attached to spyc's own jar: when `spyc attach` named its
    /// agent, every tab's link points there now (`pane::ssh_agent`).
    pub(super) fn follow_attached_agent(&mut self) {
        use crate::pane::ssh_agent;
        let (Some(jar), Some(dir)) = (&self.runtime.own_jar, ssh_agent::links_dir()) else {
            return;
        };
        let Some(agent) = ssh_agent::pointed_at(&ssh_agent::session_link(&dir, jar.address()))
        else {
            return;
        };
        if self.runtime.ssh_agent.as_ref() == Some(&agent) {
            return;
        }
        let ids: Vec<String> = self
            .runtime
            .every_tab_set()
            .flat_map(|(_, tabs)| tabs.tabs().iter().map(|e| e.info.id.clone()))
            .collect();
        let moved = !ids.is_empty();
        for id in ids {
            let link = ssh_agent::tab_link(&dir, &id);
            if std::fs::symlink_metadata(&link).is_ok()
                && let Err(e) = ssh_agent::point(&link, &agent)
            {
                crate::spyc_debug!("ssh agent: tab {id} keeps its agent: {e:#}");
            }
        }
        if moved {
            self.state.note_event(&format!(
                "SSH agent: the tabs reach {} now",
                agent.display()
            ));
        }
        self.runtime.ssh_agent = Some(agent);
    }

    /// Have the session go by `name`, already normalized.
    pub(super) fn name_session(&mut self, name: String) {
        let own = self
            .runtime
            .own_jar
            .as_ref()
            .map(|j| j.address().to_string());
        if let Some(sessions) = &self.runtime.sessions_dir
            && crate::pane::jar::attach::named_elsewhere(sessions, &name, own.as_deref())
        {
            self.state.flash_error(format!(
                "name: another session goes by {name}; `spyc attach {name}` reaches it"
            ));
            return;
        }
        if let Some(jar) = self.runtime.own_jar.as_mut()
            && let Err(e) = jar.rename(&name)
        {
            self.state.flash_error(format!("name: {e:#}"));
            return;
        }
        self.state.flash_info(format!("session name: {name}"));
        self.state.session_name = Some(name);
    }

    /// spyc in its own jar, at the loop's start: hear the jar, and restore the
    /// session of its name.
    pub(super) fn start_own_jar(&mut self, terminal: &mut Tui, foreground_exec: &ForegroundExec) {
        self.watch_own_jar();
        self.restore_own_session(terminal, foreground_exec);
    }

    /// spyc in its own jar, at its start: the session the jar holds comes
    /// back, saved by the spyc that handed it over under the name it goes by
    /// or, renamed since that save, its address; its tabs taken back from
    /// their jars. A session whose spyc still runs isn't taken.
    fn restore_own_session(&mut self, terminal: &mut Tui, foreground_exec: &ForegroundExec) {
        let Some((name, address)) = self
            .runtime
            .own_jar
            .as_ref()
            .map(|j| (j.name().to_string(), j.address().to_string()))
        else {
            return;
        };
        let Some(manifest) = crate::state::sessions::load_manifests()
            .into_iter()
            .filter(|m| (m.name == name || m.name == address) && !m.running())
            .max_by_key(|m| m.epoch_secs)
        else {
            return;
        };
        let effects = self.restore_session(&manifest);
        // Saved before a rename, the session came back under its address.
        self.state.session_name = Some(name);
        self.run_effects(effects, terminal, foreground_exec);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_file_at_the_path_is_an_install_and_the_same_one_isnt() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("spyc");
        std::fs::write(&path, "one").unwrap();
        let then = BinaryStamp::of(&path).unwrap();
        assert!(!then.replaced_by(&BinaryStamp::of(&path).unwrap()));
        let next = dir.path().join("spyc.new");
        std::fs::write(&next, "two").unwrap();
        std::fs::rename(&next, &path).unwrap();
        assert!(then.replaced_by(&BinaryStamp::of(&path).unwrap()));
    }

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
                    name: "ANISE".into(),
                    in_place: false,
                })
            );
        });
    }
}
