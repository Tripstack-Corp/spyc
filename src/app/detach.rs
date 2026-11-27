//! `^d` and `:detach` (S4): the terminal goes, and spyc stays. spyc in its
//! own jar (`pane::jar::session`) writes its terminal's reset and has the jar
//! end the client (`settle_detach`). A plain spyc hands itself over first.
//! It saves the session as no running spyc's, synced (`prepare_handover`),
//! and starts the session's own jar, with a spyc running there at once told
//! this spyc's pid, on a worker so the loop runs on (`apply_handover_starts`
//! takes its answer). Meanwhile keys are dropped and nothing saves the
//! session again (`session_out_of_hand`), so the session the new spyc
//! restores is the one saved. When either fails it stays, and says why. Only
//! then does it leave the loop, end each jar's connection as a crash would, and
//! end, naming the session `spyc attach` takes (`hand_over`, from `lib.rs`).
//! The spyc the jar starts restores the session by its name and takes each jar
//! back once this spyc has let it go (`restore_own_session`,
//! `pane::jar::take`): until then it waits on each jar, never taking one for
//! another spyc's.
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

/// A handover, left for `lib.rs` once the loop ends: the session is saved
/// and, for `^d`, its jar runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handover {
    /// The session's name: its jar's.
    pub name: String,
    /// An upgrade in spyc's own jar: `exec` the new binary there.
    pub in_place: bool,
    /// The spyc it becomes: the binary installed.
    pub exe: std::path::PathBuf,
    /// The session saved as released, which letting the jars go needs.
    pub saved: SavedReleased,
}

/// Proof that the session was saved as no running spyc's, synced: only
/// [`App::save_released`] makes one, and [`App::release_jars`] takes one, so
/// no jar is let go before that save (SPYC-TRAP
/// `handover-ready-before-release`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedReleased {
    /// The saved session's id.
    id: u64,
}

impl SavedReleased {
    /// The saved session's id.
    pub const fn id(&self) -> u64 {
        self.id
    }
}

/// `^d`'s session jar, started off the loop: its name, the binary its spyc
/// runs, the save it restores, and how its start went.
pub(super) struct HandoverStart {
    name: String,
    exe: std::path::PathBuf,
    saved: SavedReleased,
    started: Result<(), String>,
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
        if self.state.session_name.is_none() {
            self.state
                .flash_error("can't detach: the session has no name");
            return;
        }
        self.runtime.handover_asked = true;
    }

    /// `^d`'s handover in a plain spyc, in the loop: save the session as no
    /// running spyc's and start its own jar on a worker
    /// ([`Self::apply_handover_starts`] takes it from there). The loop runs on
    /// while a slow `spyc-jar` starts (SPYC-TRAP `jar-off-the-loop`), keys
    /// dropped ([`Self::input_held_for_handover`]). When the save fails spyc
    /// stays, its tabs its own, and says why.
    pub(super) fn prepare_handover(&mut self) {
        let Some(name) = self.state.session_name.clone() else {
            return;
        };
        if self.runtime.handover_starting {
            self.state
                .flash_info(format!("detach: {name}'s jar is starting"));
            return;
        }
        let ready = crate::paths::installed_spyc().and_then(|exe| {
            let cwd = std::env::current_dir().context("spyc's directory")?;
            Ok((exe, cwd))
        });
        let (exe, cwd) = match ready {
            Ok(ready) => ready,
            Err(e) => return self.state.flash_error(refused_detach(&e)),
        };
        // SPYC-TRAP(handover-ready-before-release): saved, synced, and its jar
        // running before any jar is let go; a failure of either stays here.
        let saved = match self.save_released() {
            Ok(saved) => saved,
            Err(e) => {
                let e = anyhow::Error::new(e).context(format!("{name} wasn't saved"));
                return self.state.flash_error(refused_detach(&e));
            }
        };
        // Noted before the jar starts, its spyc writing to the same log: the
        // log tells the handover in the order it happens. A start that fails
        // is noted after it, as spyc stays.
        self.state.note_event(&format!(
            "detached: spyc moves into {name}'s own jar, its tabs running on"
        ));
        self.flush_events();
        let size = crossterm::terminal::size().map_or((24, 80), |(cols, rows)| (rows, cols));
        let results = self
            .runtime
            .handover_starts
            .sender_waking(self.loop_waker(super::Wake::PaneStart));
        let (named, at, kept) = (name.clone(), exe.clone(), saved.clone());
        let place = self.runtime.jar_place.clone();
        super::landing::spawn_worker(
            results,
            move || {
                let started =
                    crate::pane::jar::session::start(place, &named, handover_argv(&at), &cwd, size)
                        .map(drop)
                        .map_err(|e| format!("{e:#}"));
                HandoverStart {
                    name: named,
                    exe: at,
                    saved: kept,
                    started,
                }
            },
            move |why| HandoverStart {
                name,
                exe,
                saved,
                started: Err(why),
            },
        );
        self.runtime.handover_starting = true;
        self.state
            .flash_progress("detach: starting the session's own jar");
    }

    /// `^d`'s session jar has started, its spyc running and waiting for this
    /// one's jars, or failed to: leave the loop for [`Self::hand_over`], which
    /// lets them go; or stay, owning everything, and say why. Whether there is
    /// anything to draw.
    pub(crate) fn apply_handover_starts(&mut self) -> bool {
        let landed = self.runtime.handover_starts.drain();
        let drawn = !landed.is_empty();
        for HandoverStart {
            name,
            exe,
            saved,
            started,
        } in landed
        {
            self.runtime.handover_starting = false;
            self.state.clear_progress_flash();
            if let Err(why) = started {
                self.keep_session();
                self.state
                    .flash_error(refused_detach(&anyhow::anyhow!("{why}")));
                continue;
            }
            self.runtime.released = true;
            self.runtime.handover = Some(Handover {
                name,
                in_place: false,
                exe,
                saved,
            });
            self.state.should_quit = true;
        }
        drawn
    }

    /// Whether input is dropped while `^d`'s session jar starts: the spyc it
    /// starts restores the session as saved, so nothing may change it
    /// meanwhile. The loop still draws and answers agents.
    pub(super) fn input_held_for_handover(&mut self) -> bool {
        if self.runtime.handover_starting {
            self.state
                .flash_progress("detach: starting the session's own jar; keys are dropped");
        }
        self.runtime.handover_starting
    }

    /// An MCP call refused while `^d`'s session jar starts: one that changes
    /// what the session keeps, which the spyc there restores as saved, or
    /// ends it. The agent asks again once spyc is in its jar.
    pub(super) fn mcp_refused_mid_handover(
        &self,
        command: &crate::mcp_cmd::McpCommand,
    ) -> Option<crate::mcp_cmd::McpResponse> {
        (self.runtime.handover_starting && command.changes_the_session()).then(|| {
            crate::mcp_cmd::McpResponse::Error {
                message: MID_HANDOVER.into(),
            }
        })
    }

    /// The session saved again as this spyc's, after a handover that stayed.
    fn keep_session(&mut self) {
        if self.autosave_session() {
            self.runtime.autosave_last_saved_fp = Some(self.session_fingerprint());
        }
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
        match (&self.runtime.own_jar, self.state.session_name.clone(), now) {
            (Some(_), Some(name), Some(at)) => {
                // SPYC-TRAP(handover-ready-before-release): saved, synced,
                // before any jar is let go; a failed save stays here.
                let saved = match self.save_released() {
                    Ok(saved) => saved,
                    Err(e) => {
                        self.state.flash_error(format!(
                            "can't upgrade: {name} wasn't saved ({e:#}); spyc stays, its tabs with it"
                        ));
                        return;
                    }
                };
                self.runtime.released = true;
                self.runtime.handover = Some(Handover {
                    name,
                    in_place: true,
                    exe: at.path,
                    saved,
                });
                self.state.should_quit = true;
            }
            _ => self.detach(),
        }
    }

    /// `spyc/upgrade`, from `spyc attach`: answered before anything starts
    /// (H7), `{"upgrading": true}` with the upgrade under way, or why it
    /// won't be, which `spyc attach` says and attaches at once.
    pub(super) fn accept_upgrade(&mut self) -> crate::mcp_cmd::McpResponse {
        use crate::mcp_cmd::McpResponse;
        if let Some(why) = self.upgrade_refusal() {
            let said = format!("upgrade: {why}");
            self.state
                .note_event(&format!("refused spyc/upgrade: {why}"));
            self.state.flash_error(said.clone());
            return McpResponse::Error { message: said };
        }
        self.runtime.upgrade_pending = true;
        McpResponse::Ok {
            message: serde_json::json!({ "upgrading": true }).to_string(),
        }
    }

    /// Why `:upgrade` can't start now, as [`Self::upgrade`] would find: no
    /// newer spyc installed, or, outside a session jar of its own, a tab or
    /// task outside a jar or a session with no name.
    fn upgrade_refusal(&self) -> Option<String> {
        let (newer, now) = self.newer_install();
        if !newer {
            let at = now.map_or_else(String::new, |n| format!(" at {}", n.path.display()));
            return Some(format!("no newer spyc is installed{at}"));
        }
        if self.state.session_name.is_none() {
            return Some("the session has no name".into());
        }
        if self.runtime.own_jar.is_none()
            && let Some(what) = self.unjarred()
        {
            return Some(format!("{what} runs outside a jar and would end with spyc"));
        }
        None
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
    /// restored: the session is saved and, for `^d`, its jar runs
    /// (`prepare_handover`), its spyc waiting for this one to let each jar go.
    /// Leave every jar running and end, the terminal freed as any detach frees
    /// it; or in spyc's own jar `exec` the new binary, which keeps this pid and
    /// so the jar, and restores the session. Says what runs on when the `exec`
    /// fails.
    pub fn hand_over(&mut self, handover: &Handover) -> anyhow::Result<()> {
        use std::os::unix::process::CommandExt;
        self.close_event_log();
        let Handover {
            name,
            in_place,
            exe,
            saved,
        } = handover;
        self.release_jars(saved.clone());
        if *in_place {
            let err = std::process::Command::new(exe)
                .args(upgrade_args(std::env::args_os().skip(1)))
                .exec();
            return Err(err).with_context(|| {
                format!(
                    "run {}; {name}'s tabs run on in their jars: `spyc -r` takes them back",
                    exe.display()
                )
            });
        }
        println!("[detached from {name}: `spyc attach {name}` brings it back]");
        Ok(())
    }

    /// Save the session as no running spyc's, synced, each tab with the
    /// status its agent last reported: the next spyc restores it, and a session
    /// whose spyc runs is restored as a copy.
    pub(super) fn save_released(&mut self) -> std::io::Result<SavedReleased> {
        let mut manifest = self.build_manifest();
        manifest.pid = None;
        manifest.process_started = None;
        self.carry_live(&mut manifest);
        crate::state::sessions::save_session_durably(&manifest)?;
        Ok(SavedReleased { id: manifest.id })
    }

    /// Leave every jar running for the next spyc, its connection ended as a
    /// crash would, and clear what this spyc kept for itself. Only for a
    /// session `_saved` as released.
    pub(super) fn release_jars(&mut self, _saved: SavedReleased) {
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
        let handover = std::mem::take(&mut self.runtime.handover_asked);
        if handover {
            self.prepare_handover();
        }
        let settled = upgrade || handover;
        if !std::mem::take(&mut self.runtime.detach_pending) {
            return settled;
        }
        let Some(jar) = &self.runtime.own_jar else {
            return settled;
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

    /// Have the session go by `name`, already normalized. A name its jar's
    /// socket couldn't be bound at is refused here, as it's given: a session
    /// renamed in its own jar keeps its address, but its name is the address
    /// of the next jar it starts, after `spyc -r`.
    pub(super) fn name_session(&mut self, name: String) {
        let own = self
            .runtime
            .own_jar
            .as_ref()
            .map(|j| j.address().to_string());
        if let Some(sessions) = &self.runtime.sessions_dir
            && let Err(e) = crate::pane::jar::session::socket_in(sessions, &name)
        {
            self.state.flash_error(format!("name: {e:#}"));
            return;
        }
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

    /// What another spyc asks of this session's on its MCP socket:
    /// `spyc/behind`, and whether a control request may be acted on.
    pub(super) fn answer_attach(
        &mut self,
        cmd: crate::mcp_cmd::McpCommand,
    ) -> crate::mcp_cmd::McpResponse {
        use crate::mcp_cmd::{McpCommand, McpResponse};
        match cmd {
            McpCommand::Authorize { method, token } => {
                self.authorize_control(&method, token.as_deref())
            }
            _ => McpResponse::Ok {
                message: serde_json::json!({ "behind": self.newer_install().0 }).to_string(),
            },
        }
    }

    /// A control request (`method`, carrying `token`) is acted on only in a
    /// spyc that runs in a session jar of its own, and only with that
    /// session's token (the 3.0 review, E1): any process of the user reaches
    /// the socket, and one request would otherwise end spyc and every tab.
    /// No prompt: a detached spyc has no one to ask. A refusal is flashed and
    /// logged, so the session shows who was turned away.
    fn authorize_control(
        &mut self,
        method: &str,
        token: Option<&str>,
    ) -> crate::mcp_cmd::McpResponse {
        use crate::mcp_cmd::McpResponse;
        let why = match &self.runtime.own_jar {
            None => "this spyc runs in no session jar of its own",
            Some(jar) if token.is_some_and(|t| jar.admits(t.as_bytes())) => {
                return McpResponse::Ok {
                    message: "{}".into(),
                };
            }
            Some(_) if token.is_none() => "it carries no token",
            Some(_) => "its token isn't this session's",
        };
        let said = format!("refused {method} on spyc's MCP socket: {why}");
        self.state.note_event(&said);
        self.state.flash_error(said.clone());
        McpResponse::Error { message: said }
    }
}

/// The flag that tells the spyc a `^d` starts in the session's own jar which
/// spyc it takes the session from (`pane::jar::take::took_over_from`).
pub const HANDED_OVER_BY: &str = "--handed-over-by";

/// What a quit, `spyc shutdown` or an MCP call that changes the session is
/// told while `^d`'s session jar starts.
pub(super) const MID_HANDOVER: &str =
    "spyc is moving into its session's own jar: ask again once it has";

/// The command line of the spyc a `^d` starts in the session's own jar: the
/// binary installed, told this spyc's pid, whose jars it waits for.
fn handover_argv(exe: &std::path::Path) -> Vec<String> {
    vec![
        exe.display().to_string(),
        HANDED_OVER_BY.to_string(),
        std::process::id().to_string(),
    ]
}

/// The arguments an `:upgrade` in place runs the new binary with: this
/// spyc's, less the pid of the spyc it took the session from, which is no
/// longer holding anything (and whose number may be another's by now).
fn upgrade_args(mut args: impl Iterator<Item = std::ffi::OsString>) -> Vec<std::ffi::OsString> {
    let mut out = Vec::new();
    while let Some(arg) = args.next() {
        if arg == HANDED_OVER_BY {
            args.next();
        } else if !arg
            .to_string_lossy()
            .starts_with(&format!("{HANDED_OVER_BY}="))
        {
            out.push(arg);
        }
    }
    out
}

/// What a `^d` that stays says.
fn refused_detach(e: &anyhow::Error) -> String {
    format!("can't detach: {e:#}; spyc stays, its tabs with it")
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

    /// `^d` asks the loop for the handover, which saves the session and
    /// starts its jar before it leaves: the key alone neither quits nor hands
    /// anything over.
    #[test]
    fn an_upgrade_in_place_drops_the_pid_it_was_handed_over_by() {
        let args = ["-r", HANDED_OVER_BY, "4242", "--color", "never"].map(std::ffi::OsString::from);
        assert_eq!(
            upgrade_args(args.into_iter()),
            ["-r", "--color", "never"].map(std::ffi::OsString::from)
        );
        let joined = [format!("{HANDED_OVER_BY}=4242"), "-r".into()].map(std::ffi::OsString::from);
        assert_eq!(
            upgrade_args(joined.into_iter()),
            ["-r"].map(std::ffi::OsString::from)
        );
    }

    /// While `^d`'s session jar starts, its spyc restores the session as
    /// saved, as no running spyc's: nothing writes it back as this spyc's,
    /// and `spyc shutdown` and another spyc's `:attach` (`spyc/release`)
    /// are refused rather than ending or giving away the tabs mid-handover.
    #[test]
    fn nothing_saves_or_ends_the_session_while_its_jar_starts() {
        use crate::mcp_cmd::{McpCommand, McpResponse};
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(&tmp.path().join("state"), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            app.state.session_name = Some("ANISE".into());
            let mut released = app.build_manifest();
            released.pid = None;
            released.process_started = None;
            crate::state::sessions::save_session(&released).unwrap();
            app.runtime.handover_starting = true;

            app.save_session();
            assert!(!app.autosave_session(), "an autosave wrote the session");
            let pids: Vec<_> = crate::state::sessions::load_manifests()
                .into_iter()
                .map(|m| m.pid)
                .collect();
            assert_eq!(pids, [None], "the session was saved as this spyc's");

            let shut = app.execute_mcp_command(McpCommand::Shutdown);
            assert!(matches!(shut, McpResponse::Error { .. }), "{shut:?}");
            assert!(!app.state.should_quit, "spyc shutdown ended the session");
            let given = app.execute_mcp_command(McpCommand::Release);
            assert!(matches!(given, McpResponse::Error { .. }), "{given:?}");
            assert!(!app.runtime.released, "the session was given away");
        });
    }

    #[test]
    fn no_quit_ends_spyc_while_its_session_jar_starts() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(&tmp.path().join("state"), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            app.runtime.handover_starting = true;
            // A double quit, as a Lua `spyc.action("quit", 2)` sends it.
            app.request_quit();
            app.request_quit();
            assert!(!app.state.should_quit, "a double quit ended spyc");
            app.quit_command();
            assert!(!app.state.should_quit, "`:q` ended spyc");
        });
    }

    /// Each MCP call that changes what the session keeps, made while its jar
    /// starts: the spyc in the jar restores the session as saved, so the call
    /// is refused for the agent to make again there, and changes nothing.
    #[test]
    fn mcp_calls_that_change_the_session_are_refused_while_its_jar_starts() {
        use crate::mcp_cmd::{McpCommand, McpResponse};
        use crate::state::scope_registry::{ScopeClaim, ScopeIntent};
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("sub")).unwrap();
        crate::state::with_state_root(&tmp.path().join("state"), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            app.state.scope_registry.push(ScopeClaim {
                id: 1,
                owner: "owner".into(),
                owner_label: "claude".into(),
                paths: vec!["src/a.rs".into()],
                intent: ScopeIntent::Merging,
                pr: None,
                note: None,
                claimed_at_secs: 0,
            });
            let dir = app.state.cur().listing.dir.clone();
            app.runtime.handover_starting = true;
            let elsewhere = tmp.path().join("sub").display().to_string();
            let changing = vec![
                McpCommand::NavigateTo {
                    path: elsewhere.clone(),
                },
                McpCommand::SetFilter {
                    pattern: Some("x".into()),
                },
                McpCommand::PickFiles {
                    patterns: vec!["*".into()],
                },
                McpCommand::ClearPicks,
                McpCommand::OpenWorktree {
                    path: elsewhere.clone(),
                },
                McpCommand::CreateWorktree {
                    branch: "b".into(),
                    base: None,
                    open: true,
                    pane_id: None,
                    cwd: None,
                },
                McpCommand::RemoveWorktree {
                    path: elsewhere.clone(),
                },
                McpCommand::CleanWorktree { path: elsewhere },
                McpCommand::RegisterScope {
                    pane_id: None,
                    pane: None,
                    paths: vec!["src/b.rs".into()],
                    intent: "editing".into(),
                    pr: None,
                    note: None,
                },
                McpCommand::ReleaseScope { id: 1 },
            ];
            for command in changing {
                let asked = format!("{command:?}");
                match app.execute_mcp_command(command) {
                    McpResponse::Error { message } if message.contains("own jar") => {}
                    other => panic!("{asked} was answered as ever: {other:?}"),
                }
            }
            assert_eq!(app.state.scope_registry.len(), 1, "a scope claim changed");
            assert_eq!(app.state.cur().listing.dir, dir, "a column moved");
            assert!(app.state.cur().temp_filter.is_none(), "a filter was set");
            assert!(
                app.state.active_project().right.is_none(),
                "a worktree opened"
            );
            let read = app.execute_mcp_command(McpCommand::ListScopes);
            assert!(
                matches!(read, McpResponse::Ok { .. }),
                "a read was refused: {read:?}"
            );
        });
    }

    #[test]
    fn with_everything_in_jars_detach_asks_the_loop_for_a_handover() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(&tmp.path().join("state"), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            app.state.session_name = Some("ANISE".into());
            app.detach();
            assert!(app.runtime.handover_asked);
            assert!(!app.state.should_quit);
            assert_eq!(app.take_handover(), None);
        });
    }

    /// D1: a name the session's socket couldn't be bound at is refused when
    /// it's given, as `:name` and a click on the session's name give it, and
    /// the session keeps the name it had: `^d` would otherwise start its jar
    /// at a path no socket can have.
    #[test]
    fn a_name_whose_session_socket_would_not_fit_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(&tmp.path().join("state"), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            app.runtime.sessions_dir = Some(tmp.path().join("sessions"));
            app.state.session_name = Some("ANISE".into());
            // Longer than any `sun_path` holds, wherever the sessions are.
            app.name_session("A".repeat(120));
            assert_eq!(
                app.state.session_name.as_deref(),
                Some("ANISE"),
                "renamed to a name no socket can be bound at"
            );
            let flash = app.flash_text().unwrap_or_default();
            assert!(flash.contains("too long"), "{flash}");
            app.name_session("GENERAL_WORK".into());
            assert_eq!(app.state.session_name.as_deref(), Some("GENERAL_WORK"));
        });
    }
}
