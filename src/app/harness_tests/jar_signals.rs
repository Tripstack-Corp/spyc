//! SPYC-TRAP `jar-signals-through-the-jar` for tasks and captures (the
//! 3.0 review, E3): every signal spyc sends a task's or a `!` capture's
//! program in a jar goes as a frame to its jar, never to the pid the jar
//! reported, which after the program's exit may be anyone's. Each case gives
//! a fake jar a bystander's pid to report, does what the user does, and
//! checks that the jar was asked and the bystander left alone.

use super::*;
use crate::app::ForegroundExec;
use crate::pane::jar::fake::{FakeJar, WAIT, host_in_fake_jar};
use crate::pane::pty_host::PtyHost;
use spyc_jar::proto::{JarSignal, Msg};
use std::io::{BufRead, Write};
use std::sync::mpsc;

/// What the test types after the action it checks: every frame before it is
/// that action's.
const MARK: &[u8] = b"spyc-test-mark";

/// A process of the test's own, in a process group of its own, standing for
/// whatever now holds the pid the jar reported. It echoes each line it is
/// given, so a stopped or ended one is seen by its not answering.
struct Bystander {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    lines: mpsc::Receiver<String>,
}

impl Bystander {
    fn new() -> Self {
        use std::os::unix::process::CommandExt;
        let mut child = std::process::Command::new("cat")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .process_group(0)
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            stdin,
            lines,
        }
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Whether it still answers: `Err` says how it didn't.
    fn answers(&mut self) -> Result<(), String> {
        writeln!(self.stdin, "ping").map_err(|e| format!("it can't be written to: {e}"))?;
        match self.lines.recv_timeout(WAIT) {
            Ok(_) => Ok(()),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err("it ended".into()),
            Err(mpsc::RecvTimeoutError::Timeout) => Err("it stopped answering".into()),
        }
    }

    /// The bystander, still running, or the test fails saying what spyc did
    /// to it.
    fn untouched(&mut self) {
        if let Err(how) = self.answers() {
            panic!("the bystander holding the pid the jar reported was signalled: {how}");
        }
    }
}

impl Drop for Bystander {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn app_in(tmp: &std::path::Path) -> App {
    let dir = tmp.join("w");
    std::fs::create_dir_all(&dir).unwrap();
    App::test_app(dir)
}

/// A live `!` capture whose program runs in a fake jar that reports `pid`.
fn jarred_capture(app: &mut App, pid: u32) -> FakeJar {
    app.start_capture("cat", "cat", "cat");
    let capture = app
        .runtime
        .pending_capture
        .as_mut()
        .expect("premise: the capture runs");
    let (jar, host) = host_in_fake_jar(pid);
    capture.host = host;
    jar
}

/// A background task whose program runs in a fake jar that reports `pid`,
/// and its id.
fn jarred_task(app: &mut App, pid: u32) -> (FakeJar, u32) {
    let jar = jarred_capture(app, pid);
    app.background_capture();
    let id = app
        .runtime
        .background_tasks
        .tasks
        .last()
        .expect("premise: the capture became a task")
        .id;
    (jar, id)
}

fn task(app: &mut App, id: u32) -> &mut crate::app::tasks::BackgroundTask {
    app.runtime
        .background_tasks
        .tasks
        .iter_mut()
        .find(|t| t.id == id)
        .expect("the task")
}

fn capture_host(app: &mut App) -> &mut PtyHost {
    &mut app
        .runtime
        .pending_capture
        .as_mut()
        .expect("the capture")
        .host
}

/// Carry `fx` out as the loop does.
fn run(app: &mut App, fx: Vec<Effect>) {
    use ratatui::backend::CrosstermBackend;
    use ratatui::layout::Rect;
    use ratatui::{Terminal, TerminalOptions, Viewport};
    let mut terminal = Terminal::with_options(
        CrosstermBackend::new(std::io::stdout()),
        TerminalOptions {
            viewport: Viewport::Fixed(Rect::new(0, 0, 100, 32)),
        },
    )
    .unwrap();
    let fg = ForegroundExec {
        park: std::sync::Arc::default(),
        acked: std::sync::Arc::default(),
        reader_done: std::sync::Arc::default(),
        reader: std::thread::current(),
    };
    app.run_effects(fx, &mut terminal, &fg);
}

/// Every frame the jar got before [`MARK`], pings aside.
fn before_mark(jar: &FakeJar) -> Vec<Msg> {
    let mut got = Vec::new();
    loop {
        match jar.next() {
            Msg::Input { bytes } if bytes == MARK => return got,
            m => got.push(m),
        }
    }
}

/// The signals among `frames`.
fn signals(frames: &[Msg]) -> Vec<JarSignal> {
    frames
        .iter()
        .filter_map(|m| match m {
            Msg::Signal { signal } => JarSignal::from_wire(*signal),
            _ => None,
        })
        .collect()
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

/// `:pause` on a task in a jar asks the jar to stop it.
#[test]
#[ignore = "red until E3: fix/task-signals"]
fn pausing_a_jarred_task_asks_its_jar() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut other = Bystander::new();
        let mut app = app_in(tmp.path());
        let (jar, id) = jarred_task(&mut app, other.pid());

        let fx = app.pause_task(Some(id));
        run(&mut app, fx);

        other.untouched();
        task(&mut app, id).host.write_all(MARK).unwrap();
        assert_eq!(signals(&before_mark(&jar)), [JarSignal::Stop]);
        assert!(task(&mut app, id).paused, "the task is paused");
    });
}

/// `:resume` on a paused task in a jar asks the jar to continue it.
#[test]
#[ignore = "red until E3: fix/task-signals"]
fn resuming_a_jarred_task_asks_its_jar() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut other = Bystander::new();
        let mut app = app_in(tmp.path());
        let (jar, id) = jarred_task(&mut app, other.pid());
        task(&mut app, id).paused = true;

        let fx = app.resume_task(Some(id));
        run(&mut app, fx);

        task(&mut app, id).host.write_all(MARK).unwrap();
        assert_eq!(signals(&before_mark(&jar)), [JarSignal::Continue]);
        assert!(!task(&mut app, id).paused, "the task runs again");
        other.untouched();
    });
}

/// `^C` in the task viewer interrupts a task in a jar through its jar.
#[test]
#[ignore = "red until E3: fix/task-signals"]
fn interrupting_a_jarred_task_asks_its_jar() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut other = Bystander::new();
        let mut app = app_in(tmp.path());
        let (jar, id) = jarred_task(&mut app, other.pid());

        let said = app.interrupt_task(Some(id));

        other.untouched();
        assert!(said.is_ok(), "{said:?}");
        task(&mut app, id).host.write_all(MARK).unwrap();
        assert_eq!(signals(&before_mark(&jar)), [JarSignal::Interrupt]);
    });
}

/// A paused task in a jar moved to a tab is continued by its jar.
#[test]
#[ignore = "red until E3: fix/task-signals"]
fn a_paused_jarred_task_moved_to_a_tab_is_continued_by_its_jar() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut other = Bystander::new();
        let mut app = app_in(tmp.path());
        let (jar, id) = jarred_task(&mut app, other.pid());
        task(&mut app, id).paused = true;

        app.promote_task_to_pane(Some(id));

        let tabs = app
            .runtime
            .project_mut()
            .pane_tabs
            .as_mut()
            .expect("the task became a tab");
        tabs.active_mut().send_bytes(MARK).unwrap();
        assert_eq!(signals(&before_mark(&jar)), [JarSignal::Continue]);
        other.untouched();
    });
}

/// `:fg` on a paused task in a jar continues it through its jar.
#[test]
#[ignore = "red until E3: fix/task-signals"]
fn a_paused_jarred_task_brought_to_the_foreground_is_continued_by_its_jar() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut other = Bystander::new();
        let mut app = app_in(tmp.path());
        let (jar, id) = jarred_task(&mut app, other.pid());
        task(&mut app, id).paused = true;

        app.foreground_task(Some(id));

        capture_host(&mut app).write_all(MARK).unwrap();
        assert_eq!(signals(&before_mark(&jar)), [JarSignal::Continue]);
        other.untouched();
    });
}

/// `^C` in a live capture in a jar interrupts it through its jar.
#[test]
#[ignore = "red until E3: fix/task-signals"]
fn ctrl_c_in_a_jarred_capture_asks_its_jar() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut other = Bystander::new();
        let mut app = app_in(tmp.path());
        let jar = jarred_capture(&mut app, other.pid());

        let fx = app.handle_key(ctrl('c')).unwrap();
        run(&mut app, fx);

        other.untouched();
        capture_host(&mut app).write_all(MARK).unwrap();
        assert_eq!(signals(&before_mark(&jar)), [JarSignal::Interrupt]);
        assert!(app.runtime.pending_capture.is_some(), "the capture runs on");
    });
}

/// Where the jar reports another job in its PTY's foreground (a capture's
/// shell runs its command as a job of its own), `^C` reaches that job as
/// the terminal's own key does, by the byte the PTY turns into the signal,
/// and the jar interrupts its program: no number is signalled.
#[test]
#[ignore = "red until E3: fix/task-signals"]
fn ctrl_c_with_another_job_in_the_foreground_types_the_key_and_asks_the_jar() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut program = Bystander::new();
        let mut job = Bystander::new();
        let mut app = app_in(tmp.path());
        let mut jar = jarred_capture(&mut app, program.pid());
        jar.send(&Msg::Foreground { pgid: job.pid() });
        let deadline = std::time::Instant::now() + WAIT;
        while capture_host(&mut app).foreground_pgrp() != Some(job.pid()) {
            assert!(
                std::time::Instant::now() < deadline,
                "the jar's foreground never reached the host"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        let fx = app.handle_key(ctrl('c')).unwrap();
        run(&mut app, fx);

        job.untouched();
        program.untouched();
        capture_host(&mut app).write_all(MARK).unwrap();
        let frames = before_mark(&jar);
        assert_eq!(signals(&frames), [JarSignal::Interrupt], "{frames:?}");
        assert!(
            frames.contains(&Msg::Input { bytes: vec![0x03] }),
            "the terminal's own ^C: {frames:?}"
        );
    });
}

/// `^\` in a live capture in a jar kills it through its jar, the job
/// in its foreground included: neither number the jar reported is
/// signalled.
#[test]
#[ignore = "red until E3: fix/task-signals"]
fn ctrl_backslash_in_a_jarred_capture_kills_through_its_jar() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut program = Bystander::new();
        let mut job = Bystander::new();
        let mut app = app_in(tmp.path());
        let mut jar = jarred_capture(&mut app, program.pid());
        jar.send(&Msg::Foreground { pgid: job.pid() });
        let deadline = std::time::Instant::now() + WAIT;
        while capture_host(&mut app).foreground_pgrp() != Some(job.pid()) {
            assert!(
                std::time::Instant::now() < deadline,
                "the jar's foreground never reached the host"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        let fx = app.handle_key(ctrl('\\')).unwrap();
        run(&mut app, fx);

        job.untouched();
        program.untouched();
        assert!(app.runtime.pending_capture.is_none(), "the capture is gone");
        loop {
            if let Msg::Signal { signal } = jar.next() {
                assert_eq!(JarSignal::from_wire(signal), Some(JarSignal::Kill));
                break;
            }
        }
    });
}

/// Who in spyc may signal a process by its number. A jarred program's pid is
/// its jar's report, so only the jar may signal it; spyc signals by number
/// only what it started and reaps itself, and only where it is listed here
/// with why. Fails closed: each listed file must still hold a site, and the
/// scan must have read spyc's sources.
#[test]
#[ignore = "red until E3: fix/task-signals"]
fn no_number_is_signalled_outside_the_listed_sites() {
    /// Files that may signal by number, and why.
    const ALLOWED: &[(&str, &str)] = &[
        (
            "src/pane/pty_host.rs",
            "a program on spyc's own PTY, its child; a jarred host returns before it \
             (`pane::jar::tests::every_signal_goes_through_the_jar`)",
        ),
        (
            "src/pane/jar/attach.rs",
            "`spyc attach` ends the attach client the session's jar names, never a jar's program",
        ),
    ];
    const CALLS: &[&str] = &[
        "kill_process_group(",
        "kill_process(",
        "kill_pg(",
        "kill_group(",
        "killpg(",
        "libc::kill(",
    ];
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let files: Vec<String> = crate::guard_support::repo_files(root, &["rs"])
        .into_iter()
        .filter(|p| p.starts_with("src/"))
        .filter(|p| {
            let name = p.rsplit('/').next().unwrap_or("");
            name != "tests.rs"
                && name != "test_support.rs"
                && name != "test_dir.rs"
                && !name.ends_with("_tests.rs")
                && !p.contains("_tests/")
                && !p.contains("/tests/")
        })
        .collect();
    assert!(
        files.len() > 200,
        "only {} source files read: the guard reads the wrong place",
        files.len()
    );
    let mut offenders = Vec::new();
    let mut seen = vec![false; ALLOWED.len()];
    for path in &files {
        let src = std::fs::read_to_string(root.join(path)).unwrap();
        let production = crate::guard_support::production_half(&src);
        for (n, line) in production.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            let calls = CALLS.iter().any(|call| {
                code.match_indices(call).any(|(at, _)| {
                    !code[..at]
                        .chars()
                        .next_back()
                        .is_some_and(|c| c.is_alphanumeric() || c == '_')
                })
            });
            if !calls {
                continue;
            }
            match ALLOWED.iter().position(|(file, _)| file == path) {
                Some(i) => seen[i] = true,
                None => offenders.push(format!("{path}:{}: {}", n + 1, line.trim())),
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "a process signalled by its number: a jarred program's goes to its jar \
         (`PtyHost`), and anything else is listed here with why:\n{}",
        offenders.join("\n")
    );
    let stale: Vec<_> = ALLOWED
        .iter()
        .zip(&seen)
        .filter(|(_, seen)| !**seen)
        .map(|((file, _), _)| *file)
        .collect();
    assert!(
        stale.is_empty(),
        "listed files that signal nothing: {stale:?}"
    );
}
