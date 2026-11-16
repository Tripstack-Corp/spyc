//! A jarred tab's start, off the loop (finding 5): the tab opens at once on a
//! stand-in saying it's starting while its jar is slow, goes live when the
//! jar answers with its program run once, and drops what's typed meanwhile. A
//! jar that never answers fails the tab, having run nothing, and `^a R`
//! starts it again. A restored tab whose jar is stopped is taken back once
//! it answers. Slowness comes from wrapper binaries held on a file and from
//! `SIGSTOP`, never from timing.

use super::*;
use crate::pane::jar::Place;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(20);
/// A test's program is ended before the test is: a pane's jar outlives it.
const END: Duration = Duration::from_millis(500);

/// A directory under `/tmp`, short enough for a socket's path on macOS.
fn short_dir() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("js")
        .tempdir_in("/tmp")
        .unwrap()
}

/// The `spyc-jar` this build made.
fn built_jar() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    exe.ancestors()
        .skip(1)
        .map(|d| d.join("spyc-jar"))
        .find(|j| j.is_file())
        .expect("a built spyc-jar: make test builds it")
}

/// An executable shell script at `dir/name`.
fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// A `spyc-jar` that marks `began` and waits for `go` before it runs: its
/// start is as slow as the test makes it.
fn gated_jar(dir: &Path) -> PathBuf {
    script(
        dir,
        "gated-jar",
        &format!(
            ": > '{began}'\nwhile [ ! -e '{go}' ]; do sleep 0.05; done\nexec '{jar}' \"$@\"\n",
            began = dir.join("began").display(),
            go = dir.join("go").display(),
            jar = built_jar().display(),
        ),
    )
}

fn jar_app(dir: &Path, binary: PathBuf, hold: Duration) -> App {
    let mut app = App::test_app(dir.to_path_buf());
    app.state.active_project_mut().config.pane.jars = true;
    app.runtime.jar_place = Some(Place {
        binary,
        dirs: vec![dir.join("spyc"), dir.join("spyc").join("jars")],
        tokens: dir.join("tokens"),
        hold,
        repaint_hold: Duration::from_secs(2),
    });
    app
}

/// A program that notes each start in `ran`, then echoes what's typed.
fn marking(ran: &Path) -> String {
    format!("sh -c 'echo once >> {}; exec cat'", ran.display())
}

/// What `ran` holds once the program noted its start: a terminal echoes
/// what's typed before the program behind it has read anything.
fn ran_once(ran: &Path) -> String {
    eventually("the program noted its start", || ran.exists());
    std::fs::read_to_string(ran).unwrap()
}

fn tabs(app: &mut App) -> &mut crate::pane::PaneTabs {
    app.runtime.project_mut().pane_tabs.as_mut().unwrap()
}

fn screen(app: &mut App) -> String {
    tabs(app)
        .active()
        .with_screen(crate::pane::engine::TerminalScreen::contents)
}

fn eventually(what: &str, mut done: impl FnMut() -> bool) {
    let until = Instant::now() + WAIT;
    while !done() {
        assert!(Instant::now() < until, "never: {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn shows(app: &mut App, needle: &str) {
    let until = Instant::now() + WAIT;
    loop {
        app.drain_pane_output();
        if screen(app).contains(needle) {
            return;
        }
        assert!(
            Instant::now() < until,
            "never saw {needle:?}: {:?}",
            screen(app)
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Land the tab's pane as the loop's drain does.
fn land(app: &mut App) {
    assert!(
        app.runtime.pane_starts.wait(WAIT + WAIT),
        "the tab's pane never landed"
    );
    assert!(app.apply_pane_starts());
}

fn type_into(app: &mut App, bytes: &[u8]) {
    tabs(app).active_mut().send_bytes(bytes).unwrap();
}

/// Arm the tab's `/resume`, due at once, as a restore does.
fn arm_resume(app: &mut App) {
    tabs(app).tabs_mut()[0].info.pending_resume_send =
        Some(crate::pane::tabs::PendingResumeSend::Text {
            sid: "sid".into(),
            after: Instant::now(),
        });
}

/// Where the tab's `/resume` is.
#[derive(Debug, PartialEq, Eq)]
enum Resume {
    Unarmed,
    DueAt(Instant),
    Typed,
}

fn resume_due(app: &mut App) -> Resume {
    use crate::pane::tabs::PendingResumeSend;
    match &tabs(app).tabs()[0].info.pending_resume_send {
        None => Resume::Unarmed,
        Some(PendingResumeSend::Text { after, .. }) => Resume::DueAt(*after),
        Some(_) => Resume::Typed,
    }
}

fn end(app: &mut App) {
    tabs(app).active_mut().shutdown_reporting(END, || {});
}

/// Test 5 of the timer brief: a jar slow to start gives the tab a stand-in
/// at once, the loop free meanwhile; the tab goes live when the jar answers,
/// its program run once.
#[test]
fn a_tab_whose_jar_is_slow_to_start_shows_it_starting_then_goes_live() {
    let dir = short_dir();
    crate::state::with_state_root(dir.path(), || {
        let mut app = jar_app(
            dir.path(),
            gated_jar(dir.path()),
            crate::pane::jar::place::HOLD,
        );
        let ran = dir.path().join("ran");
        // The jar waits for `go`, which only this thread makes, after this.
        assert!(app.open_pane_tab_in(&marking(&ran), dir.path()));
        eventually("the jar began", || dir.path().join("began").exists());
        assert!(tabs(&mut app).active().starting(), "the tab is starting");
        shows(&mut app, "Starting this tab's jar");
        assert!(!ran.exists(), "nothing runs before the jar answers");
        arm_resume(&mut app);
        app.send_pending_resumes(Instant::now());
        assert!(
            matches!(resume_due(&mut app), Resume::DueAt(_)),
            "a starting tab's `/resume` waits for its pane"
        );

        std::fs::write(dir.path().join("go"), "").unwrap();
        let landed = Instant::now();
        land(&mut app);
        assert!(!tabs(&mut app).active().starting(), "the tab is live");
        assert!(
            matches!(resume_due(&mut app), Resume::DueAt(at) if at > landed),
            "a program started now gets its `/resume` once its banner settles"
        );
        type_into(&mut app, b"typed\r");
        shows(&mut app, "typed");
        assert_eq!(ran_once(&ran), "once\n");
        end(&mut app);
    });
}

/// What's typed into a starting tab is dropped: it never reaches the
/// program once the tab goes live, and what's typed after does.
#[test]
fn keys_typed_into_a_starting_tab_are_dropped() {
    let dir = short_dir();
    crate::state::with_state_root(dir.path(), || {
        let mut app = jar_app(
            dir.path(),
            gated_jar(dir.path()),
            crate::pane::jar::place::HOLD,
        );
        assert!(app.open_pane_tab_in("cat", dir.path()));
        eventually("the jar began", || dir.path().join("began").exists());
        type_into(&mut app, b"early\r");
        std::fs::write(dir.path().join("go"), "").unwrap();
        land(&mut app);
        type_into(&mut app, b"late\r");
        shows(&mut app, "late");
        assert!(!screen(&mut app).contains("early"), "{}", screen(&mut app));
        end(&mut app);
    });
}

/// Test 6 of the timer brief: a jar that never answers fails its tab once
/// it stops holding the program, says why, and ran nothing; `^a R` starts
/// the tab again, and with a jar that answers it goes live.
#[test]
fn a_tab_whose_jar_never_answers_fails_having_run_nothing_and_starts_again() {
    let dir = short_dir();
    crate::state::with_state_root(dir.path(), || {
        let tried = dir.path().join("tried");
        let once_silent = script(
            dir.path(),
            "silent-once",
            &format!(
                "if [ -e '{tried}' ]; then exec '{jar}' \"$@\"; fi\n: > '{tried}'\nexec sleep 600\n",
                tried = tried.display(),
                jar = built_jar().display(),
            ),
        );
        let mut app = jar_app(dir.path(), once_silent, Duration::from_secs(2));
        let ran = dir.path().join("ran");
        assert!(app.open_pane_tab_in(&marking(&ran), dir.path()));
        land(&mut app);
        shows(&mut app, "didn't answer");
        eventually("the tab ended", || {
            app.drain_pane_output();
            tabs(&mut app).mark_exited();
            tabs(&mut app)
                .active_info()
                .label
                .ends_with("[not started]")
        });
        assert!(!ran.exists(), "the program never ran");

        app.restart_active_tab_now();
        land(&mut app);
        type_into(&mut app, b"again\r");
        shows(&mut app, "again");
        assert_eq!(ran_once(&ran), "once\n");
        end(&mut app);
    });
}

/// A tab closed while its jar starts leaves nothing running: the pane that
/// lands for it is ended, its program with it.
#[test]
fn a_tab_closed_while_it_starts_leaves_nothing_running() {
    let dir = short_dir();
    crate::state::with_state_root(dir.path(), || {
        let mut app = jar_app(
            dir.path(),
            gated_jar(dir.path()),
            crate::pane::jar::place::HOLD,
        );
        let pid = dir.path().join("pid");
        let program = format!("sh -c 'echo $$ > {}; exec cat'", pid.display());
        assert!(app.open_pane_tab_in(&program, dir.path()));
        eventually("the jar began", || dir.path().join("began").exists());
        assert!(!tabs(&mut app).remove_at(0), "the only tab closed");
        app.runtime.project_mut().pane_tabs = None;
        std::fs::write(dir.path().join("go"), "").unwrap();
        land(&mut app);
        eventually("its program noted its pid", || {
            std::fs::read_to_string(&pid).is_ok_and(|p| p.ends_with('\n'))
        });
        let pid: i32 = std::fs::read_to_string(&pid)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        eventually("its program ended", || {
            rustix::process::test_kill_process(rustix::process::Pid::from_raw(pid).unwrap())
                .is_err()
        });
    });
}

/// Test 7 of the timer brief, at the tab: a restored tab whose jar is stopped
/// while it's taken back opens on its stand-in and is taken back once the jar
/// answers, the same program, typing reaching it.
#[test]
fn a_restored_tab_whose_jar_is_stopped_is_taken_back_once_it_answers() {
    use rustix::process::{Pid, Signal, kill_process};
    let dir = short_dir();
    crate::state::with_state_root(dir.path(), || {
        let mut app = jar_app(dir.path(), built_jar(), crate::pane::jar::place::HOLD);
        let pid = dir.path().join("pid");
        let program = format!("sh -c 'echo $$ > {}; exec cat'", pid.display());
        assert!(app.open_pane_tab_in(&program, dir.path()));
        land(&mut app);
        eventually("its program noted its pid", || {
            std::fs::read_to_string(&pid).is_ok_and(|p| p.ends_with('\n'))
        });
        let socket = tabs(&mut app).active().jar_socket().unwrap().to_path_buf();
        let id = tabs(&mut app).active_info().id.clone();
        // As a crash of spyc leaves it: the jar and its program run on.
        let first = app.runtime.project_mut().pane_tabs.take().unwrap();
        first.active().host.sever_jar();
        drop(first);
        let jar = spyc_jar::client::describe(&socket)
            .unwrap()
            .into_iter()
            .find(|(k, _)| k == "pid")
            .map(|(_, v)| v.parse::<i32>().unwrap())
            .unwrap();
        let jar = Pid::from_raw(jar).unwrap();
        kill_process(jar, Signal::STOP).unwrap();

        let saved = crate::state::sessions::SavedTab {
            command: program.clone(),
            label: program.clone(),
            cwd: dir.path().to_path_buf(),
            agent_kind: crate::state::sessions::AgentKind::Other,
            agent_session_id: None,
            agent_session_name: None,
            codex_home: None,
            codex_home_unresolved: false,
            claude_home: None,
            claim_owner: String::new(),
            pane_id: id,
            agent_hint: None,
            unopened: false,
            jar_socket: Some(socket),
        };
        assert!(app.spawn_pane_tab(
            &program,
            dir.path(),
            super::super::pane_tabs::TabSlot::Append,
            None,
            Some(&saved),
            None,
        ));
        assert!(tabs(&mut app).active().starting(), "the tab is starting");
        shows(&mut app, "Taking this tab's program back");
        arm_resume(&mut app);
        kill_process(jar, Signal::CONT).unwrap();
        land(&mut app);
        assert!(
            tabs(&mut app).active().adopted(),
            "the program was taken back"
        );
        assert_eq!(
            resume_due(&mut app),
            Resume::Unarmed,
            "a program taken back is in its conversation: no `/resume`"
        );
        type_into(&mut app, b"typed\r");
        shows(&mut app, "typed");
        let started = std::fs::read_to_string(&pid).unwrap();
        assert_eq!(started.lines().count(), 1, "started once: {started}");
        end(&mut app);
    });
}

/// Open a jarred tab running `cat` that notes its pid in `dir/pid`, landed;
/// its program's pid.
fn live_tab(app: &mut App, dir: &Path) -> i32 {
    let pid = dir.join("pid");
    let program = format!("sh -c 'echo $$ > {}; exec cat'", pid.display());
    assert!(app.open_pane_tab_in(&program, dir));
    land(app);
    eventually("its program noted its pid", || {
        std::fs::read_to_string(&pid).is_ok_and(|p| p.ends_with('\n'))
    });
    std::fs::read_to_string(&pid)
        .unwrap()
        .trim()
        .parse()
        .unwrap()
}

/// End the tab's connection as its jar does a lapsed lease, and wait until
/// spyc sees it.
fn lapse(app: &mut App) {
    tabs(app).active().host.sever_jar();
    eventually("the tab's lease lapsed", || {
        app.drain_pane_output();
        tabs(app).active().lease_lapsed()
    });
}

/// S2b at the tab: a tab whose jar ended its lease (as a spyc stopped past
/// the deadline finds) is taken back, the same program, what it printed
/// before still shown, and typing reaches it.
#[test]
fn a_tab_whose_jar_ended_its_lease_is_taken_back() {
    let dir = short_dir();
    crate::state::with_state_root(dir.path(), || {
        let mut app = jar_app(dir.path(), built_jar(), crate::pane::jar::place::HOLD);
        let program = live_tab(&mut app, dir.path());
        type_into(&mut app, b"before\r");
        shows(&mut app, "before");
        lapse(&mut app);
        assert!(app.retake_lapsed_tabs(), "the tab is taken back");
        assert!(
            tabs(&mut app).active().starting(),
            "on a stand-in meanwhile"
        );
        land(&mut app);
        assert!(tabs(&mut app).active().adopted(), "its jar was taken back");
        assert_eq!(
            tabs(&mut app).active().process_id(),
            u32::try_from(program).ok(),
            "the same program"
        );
        shows(&mut app, "before");
        type_into(&mut app, b"after\r");
        shows(&mut app, "after");
        assert!(!app.retake_lapsed_tabs(), "taken back once");
        end(&mut app);
    });
}

/// A tab left detached because its jar answers to another spyc has no lease
/// of this spyc's to lapse: it's never taken back, so it can't loop.
#[test]
fn a_detached_tab_is_not_taken_back() {
    let dir = short_dir();
    crate::state::with_state_root(dir.path(), || {
        let mut app = App::test_app(dir.path().to_path_buf());
        let wake = app.make_pane_wake();
        let mut pane = crate::pane::Pane::standing_in(
            crate::pane::jar::ended("", crate::pane::jar::Lost::Held, (24, 80)),
            false,
            (24, 80),
            wake,
        )
        .unwrap();
        eventually("the detached tab ended", || {
            pane.drain_output();
            pane.is_closed()
        });
        pane.try_harvest_exit_status();
        assert!(!pane.lease_lapsed());
    });
}

/// A tab whose jar is gone by the time it's taken back ends `[jar lost]`,
/// its program never started afresh.
#[test]
fn a_tab_whose_jar_is_gone_when_taken_back_ends_jar_lost() {
    use rustix::process::{Pid, Signal, kill_process};
    let dir = short_dir();
    crate::state::with_state_root(dir.path(), || {
        let mut app = jar_app(dir.path(), built_jar(), crate::pane::jar::place::HOLD);
        let program = live_tab(&mut app, dir.path());
        let socket = tabs(&mut app).active().jar_socket().unwrap().to_path_buf();
        let jar: i32 = spyc_jar::client::describe(&socket)
            .unwrap()
            .into_iter()
            .find(|(k, _)| k == "pid")
            .map(|(_, v)| v.parse().unwrap())
            .unwrap();
        lapse(&mut app);
        kill_process(Pid::from_raw(program).unwrap(), Signal::KILL).unwrap();
        kill_process(Pid::from_raw(jar).unwrap(), Signal::KILL).unwrap();
        eventually("the jar's lock is free", || {
            spyc_jar::lock::state(&socket) != spyc_jar::lock::LockState::Held
        });
        assert!(app.retake_lapsed_tabs());
        land(&mut app);
        shows(&mut app, "gone");
        eventually("the tab ended", || {
            app.drain_pane_output();
            tabs(&mut app).mark_exited();
            tabs(&mut app).active_info().label.ends_with("[jar lost]")
        });
        assert_eq!(
            std::fs::read_to_string(dir.path().join("pid"))
                .unwrap()
                .lines()
                .count(),
            1,
            "the program wasn't started afresh"
        );
    });
}
