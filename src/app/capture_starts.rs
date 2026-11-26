//! A `!` capture's program in a jar (S2c), started off the loop as a tab's
//! is (`app::pane_starts`): the capture opens at once on a stand-in host that
//! prints nothing, a worker starts its jar, and the jar's host takes the
//! stand-in's place when it lands ([`App::apply_capture_starts`]), in the
//! capture or in the task it was sent to meanwhile. A capture closed
//! meanwhile takes nothing: the host that lands for it is ended at once.
//!
//! A capture's jar keeps its output and exit until spyc collects them
//! (`spyc_jar::cli::UNTIL_READ`): with spyc there, the jar ends as its
//! program's exit is read; after a crash, it waits for a spyc to read it.
//! One that can't start says why as the capture's output; for want of a
//! usable jar, spyc offers to run without jars (`app::jarless`).

use std::path::Path;

use crate::pane::jar;
use crate::pane::pty_host::PtyHost;

use super::landing::spawn_worker;
use super::{App, Wake};

/// A capture's host, started in a jar on a worker, or why it wasn't.
pub(super) struct CaptureStart {
    /// The start's id, which its capture or task keeps until it lands.
    id: u64,
    host: anyhow::Result<PtyHost>,
    /// The jar a restored task takes back.
    socket: Option<std::path::PathBuf>,
}

/// How long a stand-in, or a host nothing will read, has to end.
const END_WAIT: std::time::Duration = std::time::Duration::from_millis(250);

impl App {
    /// The host a capture of `cmd` in `dir` starts on, and the id of its
    /// jar's start when one is under way: with jars on, a stand-in while a
    /// worker starts its jar; otherwise its program on a PTY of spyc's.
    pub(super) fn capture_host(
        &mut self,
        cmd: &str,
        dir: &Path,
    ) -> anyhow::Result<(PtyHost, Option<u64>)> {
        let jars = self.state.active_project().config.pane.jars
            && jar::honoured()
            && !self.runtime.jarless.chosen;
        let env = self.state.active_project().env.pairs();
        if !jars {
            return Ok((spawn_capture(cmd, dir, None, &env)?, None));
        }
        let (cmd, dir) = (cmd.to_string(), dir.to_path_buf());
        let place = self.runtime.jar_place.clone();
        let (host, id) =
            self.start_on_worker(None, move || start_in_jar(&cmd, &dir, place, &env))?;
        Ok((host, Some(id)))
    }

    /// A restored task's host, and the id of its jar's start: a stand-in
    /// while a worker takes back the jar at `socket`, which starts nothing
    /// afresh.
    pub(super) fn retake_task_host(&mut self, socket: &Path) -> anyhow::Result<(PtyHost, u64)> {
        let socket = socket.to_path_buf();
        let place = self.runtime.jar_place.clone();
        let record = Some(socket.clone());
        self.start_on_worker(record, move || retake_jar(socket, place))
    }

    /// A stand-in host, and the id of the start `start` makes on a worker.
    fn start_on_worker(
        &mut self,
        socket: Option<std::path::PathBuf>,
        start: impl FnOnce() -> anyhow::Result<PtyHost> + Send + 'static,
    ) -> anyhow::Result<(PtyHost, u64)> {
        let size = capture_size();
        let stand_in = PtyHost::around_jar(jar::starting("", socket.as_deref(), size), size)?;
        self.runtime.capture_start_seq += 1;
        let id = self.runtime.capture_start_seq;
        let results = self
            .runtime
            .capture_starts
            .sender_waking(self.loop_waker(Wake::PaneStart));
        let again = socket.clone();
        spawn_worker(
            results,
            move || CaptureStart {
                id,
                host: start(),
                socket,
            },
            move |panic| CaptureStart {
                id,
                host: Err(anyhow::anyhow!("the start stopped: {panic}")),
                socket: again,
            },
        );
        Ok((stand_in, id))
    }

    /// Put each landed host in its capture's or task's place. Whether any
    /// landed.
    pub(crate) fn apply_capture_starts(&mut self) -> bool {
        let landed = self.runtime.capture_starts.drain();
        let any = !landed.is_empty();
        for start in landed {
            self.land_capture_start(start);
        }
        any
    }

    fn land_capture_start(&mut self, CaptureStart { id, host, socket }: CaptureStart) {
        let size = capture_size();
        let host = match host {
            Ok(host) => host,
            Err(e) => {
                if let Some(why) = jar::no_usable_jar(&e) {
                    self.capture_has_no_usable_jar(why);
                }
                let why = format!("spyc: {e:#}\n");
                // A jar that may still hold the program keeps its record.
                let held = jar::held(socket.as_deref());
                let lost = if held.is_some() {
                    jar::Lost::Held
                } else {
                    jar::Lost::Unstarted
                };
                match PtyHost::around_jar(jar::ended_at(&why, lost, held, size), size) {
                    Ok(host) => host,
                    Err(e) => {
                        self.state.flash_error(format!("capture: {e:#}"));
                        return;
                    }
                }
            }
        };
        let wake = self.make_sink_wake();
        let runtime = &mut self.runtime;
        let slot = match runtime.pending_capture.as_mut() {
            Some(c) if c.jar_start == Some(id) => {
                c.jar_start = None;
                Some(&mut c.host)
            }
            _ => runtime
                .background_tasks
                .tasks
                .iter_mut()
                .find(|t| t.jar_start == Some(id))
                .map(|t| {
                    t.jar_start = None;
                    &mut t.host
                }),
        };
        let Some(slot) = slot else {
            end(host);
            return;
        };
        host.set_wake(wake);
        host.fire_wake_now();
        end(std::mem::replace(slot, host));
    }
}

/// End `host` off the loop.
fn end(mut host: PtyHost) {
    std::thread::spawn(move || host.shutdown(END_WAIT));
}

/// A capture's PTY size: the terminal's.
fn capture_size() -> (u16, u16) {
    let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    (rows, cols)
}

/// Start `cmd`'s jar in `place` (this user's when `None`), keeping its
/// output until spyc reads it.
fn start_in_jar(
    cmd: &str,
    dir: &Path,
    place: Option<jar::Place>,
    env: &[(String, String)],
) -> anyhow::Result<PtyHost> {
    // SPYC-TRAP(jar-off-the-loop): a capture's jar is asked for only on this
    // worker.
    let name = uuid::Uuid::now_v7().to_string();
    let mut spec = jar::spec_for(place, &name, None)?
        .ok_or_else(|| anyhow::anyhow!("this build doesn't run programs in jars"))?;
    spec.exit_linger = Some(spyc_jar::cli::UNTIL_READ);
    spawn_capture(cmd, dir, Some(spec), env)
}

/// Take back the jar at `socket` in `place` (this user's when `None`):
/// never a program started afresh.
fn retake_jar(socket: std::path::PathBuf, place: Option<jar::Place>) -> anyhow::Result<PtyHost> {
    // SPYC-TRAP(jar-off-the-loop): only on this worker.
    let name = uuid::Uuid::now_v7().to_string();
    // Taken back whatever this build or `[pane] jars` says of new ones.
    let place = match place {
        Some(place) => place,
        None => jar::Place::for_this_user()?,
    };
    let spec = jar::spec_for(Some(place), &name, None)?
        .ok_or_else(|| anyhow::anyhow!("no place for the task's jar"))?
        .retaking(socket);
    spawn_capture("", Path::new("/"), Some(spec), &[])
}

/// Spawn a capture's PTY, in `jar` when one is given. TERM=dumb and
/// cat-pagers so sub-tools don't launch a nested TUI or pager against it.
fn spawn_capture(
    cmd: &str,
    cwd: &Path,
    jar: Option<jar::Spec>,
    project_env: &[(String, String)],
) -> anyhow::Result<PtyHost> {
    let (rows, cols) = capture_size();
    // TERM=dumb so TUI programs refuse to run as a TUI inside our
    // capture (the pager only renders SGR colours and CRLF, not
    // cursor positioning / alt-screen). FORCE_COLOR / CLICOLOR_FORCE
    // / COLORTERM ride alongside so tools that respect those (cargo,
    // eza, bat, ripgrep) keep their colour output anyway. PAGER=cat /
    // GIT_PAGER=cat / MANPAGER=cat stops tools that auto-invoke a
    // sub-pager (git log, man) from launching `less` against our
    // pty and freezing the capture.
    let env: &[(&str, &str)] = &[
        ("CLICOLOR_FORCE", "1"),
        ("FORCE_COLOR", "1"),
        ("COLORTERM", "truecolor"),
        ("PAGER", "cat"),
        ("GIT_PAGER", "cat"),
        ("MANPAGER", "cat"),
    ];
    PtyHost::spawn(crate::pane::pty_host::PtySpec {
        command: cmd,
        rows,
        cols,
        cwd,
        env,
        project_env,
        term: "dumb",
        // No SIGWINCH nudge for captures — they aren't interactive
        // shells with rc-file prompts to redraw.
        nudge_winch: false,
        // Captures don't enable the per-byte debug dump (Pane does).
        debug_dump: false,
        // No `exec`: a capture may be a compound command (`a && b`); exec would
        // drop the tail. Captures are one-shot, not suspendable panes anyway.
        exec_replace: false,
        capture_launch_env: false,
        // A capture started afresh has no record; a restored task's jar is
        // taken back, never started.
        record: None,
        jar,
    })
}
