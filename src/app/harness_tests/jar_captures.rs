//! S2c: a `!` capture's program runs in a jar, started off the loop
//! (`app::capture_starts`): the capture opens at once, its jar's host lands
//! in its place, its output streams in, and its jar ends once spyc reads its
//! exit. A capture with no usable jar says why and offers to run without
//! jars; one sent to the background while its jar starts gets its jar in its
//! task, and one closed meanwhile leaves nothing running.

use super::jar_start::{
    WAIT, built_jar, eventually, gated_jar, jar_app, jar_says, offering, short_dir,
};
use super::*;

/// Land the capture's host as the loop's drain does.
fn land(app: &mut App) {
    assert!(
        app.runtime.capture_starts.wait(WAIT + WAIT),
        "the capture's host never landed"
    );
    assert!(app.apply_capture_starts());
}

/// What the live capture has printed so far, drained as the loop drains it.
fn printed(app: &mut App) -> String {
    app.drain_pending_capture();
    app.runtime
        .pending_capture
        .as_ref()
        .map_or_default(|c| String::from_utf8_lossy(&c.buffer).into_owned())
}

/// What the capture's pager shows, the capture drained as the loop drains
/// it: its live output, or all of it once finished.
fn shown(app: &mut App) -> String {
    app.drain_pending_capture();
    app.view.project().pager.as_ref().map_or_default(|view| {
        view.lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    })
}

fn write_to_capture(app: &mut App, bytes: &[u8]) {
    let capture = app
        .runtime
        .pending_capture
        .as_mut()
        .expect("a live capture");
    capture.host.write_all(bytes).unwrap();
}

/// A capture runs in a jar, which knows it holds a capture, and ends once
/// spyc has read its program's exit.
#[test]
fn a_capture_runs_in_a_jar_that_ends_once_its_exit_is_read() {
    let dir = short_dir();
    crate::state::with_state_root(dir.path(), || {
        let mut app = jar_app(dir.path(), built_jar(), crate::pane::jar::place::HOLD);
        app.start_capture("echo RESULT=42; read go", "x", "x");
        let capture = app.runtime.pending_capture.as_ref().expect("a capture");
        assert!(capture.jar_start.is_some(), "its jar starts on a worker");
        land(&mut app);
        let socket = app
            .runtime
            .pending_capture
            .as_ref()
            .unwrap()
            .host
            .jar_socket();
        let socket = socket.expect("the capture runs in a jar").to_path_buf();
        app.settle_jar_info();
        eventually("the jar holds a capture", || {
            jar_says(&socket, "is").as_deref() == Some("capture")
        });
        assert_eq!(
            jar_says(&socket, "exit_linger").as_deref(),
            Some("until read"),
            "the jar keeps the capture's exit until spyc reads it"
        );
        eventually("the output", || printed(&mut app).contains("RESULT=42"));
        write_to_capture(&mut app, b"go\r");
        eventually("the capture finished", || {
            app.drain_pending_capture();
            app.runtime.pending_capture.is_none()
        });
        eventually("its jar ended once its exit was read", || !socket.exists());
    });
}

/// No usable jar: the capture says why, and spyc offers to run without
/// jars.
#[test]
fn a_capture_with_no_usable_jar_says_why_and_offers_to_run_without() {
    let dir = short_dir();
    crate::state::with_state_root(dir.path(), || {
        let missing = dir.path().join("missing").join("spyc-jar");
        let mut app = jar_app(dir.path(), missing, crate::pane::jar::place::HOLD);
        let ran = dir.path().join("ran");
        app.start_capture(&format!("touch {}", ran.display()), "x", "x");
        land(&mut app);
        assert!(offering(&app), "spyc offers to run without jars");
        eventually("the capture says why", || {
            shown(&mut app).contains("installation")
        });
        assert!(!ran.exists(), "the program ran without a jar");
    });
}

/// A capture sent to the background while its jar starts gets its jar in
/// its task, its output there.
#[test]
fn a_capture_backgrounded_while_its_jar_starts_lands_in_its_task() {
    let dir = short_dir();
    crate::state::with_state_root(dir.path(), || {
        let mut app = jar_app(
            dir.path(),
            gated_jar(dir.path()),
            crate::pane::jar::place::HOLD,
        );
        app.start_capture("echo RESULT=42; read go", "x", "x");
        eventually("the jar began", || dir.path().join("began").exists());
        app.background_capture();
        std::fs::write(dir.path().join("go"), "").unwrap();
        land(&mut app);
        let task = &app.runtime.background_tasks.tasks[0];
        assert!(task.host.jar_socket().is_some(), "the task has its jar");
        assert!(task.jar_start.is_none());
        eventually("the task's output", || {
            app.drain_background_tasks();
            String::from_utf8_lossy(&app.runtime.background_tasks.tasks[0].buffer)
                .contains("RESULT=42")
        });
        app.runtime.background_tasks.tasks[0]
            .host
            .shutdown(std::time::Duration::from_millis(500));
    });
}

/// A capture closed while its jar starts leaves nothing running: the host
/// that lands for it is ended, and its jar with it.
#[test]
fn a_capture_closed_while_its_jar_starts_leaves_nothing_running() {
    use crate::pane::jar::place::{SocketState, socket_state};
    let dir = short_dir();
    crate::state::with_state_root(dir.path(), || {
        let mut app = jar_app(
            dir.path(),
            gated_jar(dir.path()),
            crate::pane::jar::place::HOLD,
        );
        app.start_capture("exec cat", "x", "x");
        eventually("the jar began", || dir.path().join("began").exists());
        app.runtime.pending_capture = None;
        std::fs::write(dir.path().join("go"), "").unwrap();
        land(&mut app);
        let sockets = dir.path().join("spyc").join("jars");
        eventually("no jar runs on", || {
            std::fs::read_dir(&sockets)
                .into_iter()
                .flatten()
                .flatten()
                .all(|e| socket_state(&e.path()) != SocketState::Live)
        });
    });
}

/// S2c: a session saved while a capture runs in a jar records the capture
/// and its jar, so a restore after a crash can take it back.
#[test]
fn a_saved_session_records_a_running_capture_and_its_jar() {
    let dir = short_dir();
    crate::state::with_state_root(dir.path(), || {
        let mut app = jar_app(dir.path(), built_jar(), crate::pane::jar::place::HOLD);
        let before = app.session_fingerprint();
        app.start_capture("exec cat", "cat", "cat");
        land(&mut app);
        let socket = app
            .runtime
            .pending_capture
            .as_ref()
            .unwrap()
            .host
            .jar_socket();
        let socket = socket.unwrap().to_path_buf();
        assert_ne!(app.session_fingerprint(), before, "a save is due");
        let saved = app.build_manifest().tasks;
        assert_eq!(saved.len(), 1, "{saved:?}");
        assert_eq!(saved[0].jar_socket, socket);
        assert_eq!(saved[0].cmd_display, "cat");
        app.background_capture();
        let saved = app.build_manifest().tasks;
        assert_eq!(saved.len(), 1, "a task is recorded too: {saved:?}");
        app.runtime.background_tasks.tasks[0]
            .host
            .shutdown(std::time::Duration::from_millis(500));
    });
}

/// S2c: a restore takes back a saved capture's jar as a background task,
/// the same program, its output reaching the task, and starts nothing
/// afresh.
#[test]
fn a_restore_takes_a_captures_jar_back_as_a_task() {
    let dir = short_dir();
    crate::state::with_state_root(dir.path(), || {
        let mut first = jar_app(dir.path(), built_jar(), crate::pane::jar::place::HOLD);
        let pid = dir.path().join("pid");
        first.start_capture(&format!("echo $$ >> {}; exec cat", pid.display()), "x", "x");
        land(&mut first);
        eventually("its program noted its pid", || {
            std::fs::read_to_string(&pid).is_ok_and(|p| p.ends_with('\n'))
        });
        let saved = first.build_manifest().tasks;
        // As a crash of spyc leaves it: the jar and its program run on.
        let capture = first.runtime.pending_capture.take().unwrap();
        capture.host.sever_jar();
        std::mem::forget(capture);

        let mut second = jar_app(dir.path(), built_jar(), crate::pane::jar::place::HOLD);
        second.restore_tasks(&saved);
        assert_eq!(second.runtime.background_tasks.tasks.len(), 1);
        land(&mut second);
        let task = &mut second.runtime.background_tasks.tasks[0];
        assert_eq!(task.host.jar_socket(), Some(saved[0].jar_socket.as_path()));
        task.host.write_all(b"after the crash\r").unwrap();
        eventually("the task's output", || {
            second.drain_background_tasks();
            String::from_utf8_lossy(&second.runtime.background_tasks.tasks[0].buffer)
                .contains("after the crash")
        });
        let started = std::fs::read_to_string(&pid).unwrap();
        assert_eq!(started.lines().count(), 1, "started afresh: {started}");
        second.runtime.background_tasks.tasks[0]
            .host
            .shutdown(std::time::Duration::from_millis(500));
    });
}

/// An interactive shell, which runs its command as a job of its own as a
/// capture's `$SHELL -i -c` does, and the flags that keep its startup files
/// out: zsh where it's installed, else bash (GitHub's Ubuntu runners have no
/// zsh). The tests that need one say so when there's neither.
fn job_shell() -> (&'static str, &'static str) {
    let found =
        |paths: [&'static str; 2]| paths.into_iter().find(|p| std::path::Path::new(p).exists());
    found(["/bin/zsh", "/usr/bin/zsh"])
        .map(|zsh| (zsh, "-f"))
        .or_else(|| found(["/bin/bash", "/usr/bin/bash"]).map(|bash| (bash, "--norc --noprofile")))
        .expect("these tests need zsh or bash: an interactive shell that runs its command as a job")
}

/// A raw-mode program in its own job, as a capture's `$SHELL -i -c` runs a
/// command: it prints its pid and process group. It ignores a hangup, as
/// `nohup` or a daemon would. Without `hold` it reads its terminal, printing
/// each byte, and says when an interrupt reaches it; with `hold` it ignores
/// an interrupt too and only waits, so its terminal going ends nothing.
fn raw_job(dir: &std::path::Path, hold: bool) -> String {
    let script = dir.join(if hold { "held.py" } else { "raw.py" });
    let ignore_int = if hold {
        "signal.signal(signal.SIGINT, signal.SIG_IGN)\n"
    } else {
        ""
    };
    let body = if hold {
        "import time\nwhile True:\n    time.sleep(1)\n"
    } else {
        "try:\n    while True:\n        b = os.read(0, 1)\n        \
         sys.stdout.write('GOT%d\\r\\n' % b[0]); sys.stdout.flush()\n\
         except KeyboardInterrupt:\n    sys.stdout.write('INTERRUPTED\\r\\n'); sys.stdout.flush()\n"
    };
    std::fs::write(
        &script,
        format!(
            "import os, signal, sys, tty\n\
             signal.signal(signal.SIGHUP, signal.SIG_IGN)\n{ignore_int}\
             sys.stdout.write('JOB %d %d\\r\\n' % (os.getpid(), os.getpgrp()))\n\
             tty.setraw(0)\n\
             sys.stdout.write('RAW_READY\\r\\n'); sys.stdout.flush()\n{body}"
        ),
    )
    .unwrap();
    let (shell, quiet) = job_shell();
    format!(
        "{shell} {quiet} -i -c 'python3 -I {}; echo AFTER'",
        script.display()
    )
}

/// The job's pid and group, as it printed them.
fn job_ids(app: &mut App) -> (u32, u32) {
    let out = printed(app);
    let ids: Vec<u32> = out
        .lines()
        .find_map(|l| l.trim().strip_prefix("JOB "))
        .map_or_default(|s| {
            s.split_whitespace()
                .filter_map(|n| n.parse().ok())
                .collect()
        });
    (ids[0], ids[1])
}

/// Carry `fx` out as the loop does.
fn run_capture_effects(app: &mut App, fx: Vec<Effect>) {
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
    let fg = crate::app::ForegroundExec {
        park: std::sync::Arc::default(),
        acked: std::sync::Arc::default(),
        reader_done: std::sync::Arc::default(),
        reader: std::thread::current(),
    };
    app.run_effects(fx, &mut terminal, &fg);
}

/// A capture of `hold`'s raw job, in a real jar, its job in the PTY's
/// foreground; its pid and group.
fn capture_of_a_raw_job(app: &mut App, dir: &std::path::Path, hold: bool) -> (u32, u32) {
    app.start_capture(&raw_job(dir, hold), "x", "x");
    land(app);
    eventually("the job in raw mode", || printed(app).contains("RAW_READY"));
    let (pid, group) = job_ids(app);
    eventually("the jar reports the job in the foreground", || {
        app.drain_pending_capture();
        app.runtime
            .pending_capture
            .as_ref()
            .and_then(|c| c.host.foreground_pgrp())
            == Some(group)
    });
    (pid, group)
}

fn alive(pid: u32) -> bool {
    rustix::process::Pid::from_raw(i32::try_from(pid).unwrap())
        .is_some_and(|p| rustix::process::test_kill_process(p).is_ok())
}

/// `^C` in a capture in a jar interrupts its command where the capture's
/// shell runs it as a job of its own, in raw mode, as a real `^C` does
/// (the refuter on E3: the byte was read as data, `GOT3`).
#[test]
fn ctrl_c_interrupts_a_raw_reader_its_shell_runs_as_a_job() {
    let dir = short_dir();
    crate::state::with_state_root(dir.path(), || {
        crate::shell::with_test_shell(job_shell().0, || {
            let mut app = jar_app(dir.path(), built_jar(), crate::pane::jar::place::HOLD);
            capture_of_a_raw_job(&mut app, dir.path(), false);
            let fx = app
                .handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
                .unwrap();
            run_capture_effects(&mut app, fx);
            let mut seen = String::new();
            // The pager: the capture may finish between two looks, its output
            // leaving the live buffer.
            eventually("the job answers ^C", || {
                seen = shown(&mut app);
                seen.contains("INTERRUPTED") || seen.contains("GOT3")
            });
            assert!(
                seen.contains("INTERRUPTED"),
                "^C was read as data: {seen:?}"
            );
        });
    });
}

/// `^\\` in a capture in a jar kills its command's job too, even one that
/// ignores a hangup and an interrupt (the refuter on E3: it reached the job
/// only by its shell's hangup, and the job outlived the capture).
#[test]
fn ctrl_backslash_kills_a_job_that_ignores_hangup() {
    let dir = short_dir();
    crate::state::with_state_root(dir.path(), || {
        crate::shell::with_test_shell(job_shell().0, || {
            let mut app = jar_app(dir.path(), built_jar(), crate::pane::jar::place::HOLD);
            let (pid, _) = capture_of_a_raw_job(&mut app, dir.path(), true);
            let fx = app
                .handle_key(KeyEvent::new(KeyCode::Char('\\'), KeyModifiers::CONTROL))
                .unwrap();
            run_capture_effects(&mut app, fx);
            let until = std::time::Instant::now() + WAIT;
            while alive(pid) && std::time::Instant::now() < until {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            let outlived = alive(pid);
            if outlived {
                // The test's own job, read from its own output a moment ago.
                let _ = rustix::process::kill_process(
                    rustix::process::Pid::from_raw(i32::try_from(pid).unwrap()).unwrap(),
                    rustix::process::Signal::KILL,
                );
            }
            assert!(!outlived, "the job outlived ^\\ (pid {pid})");
        });
    });
}
