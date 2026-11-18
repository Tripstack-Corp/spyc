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

pub(super) const WAIT: Duration = Duration::from_secs(20);
/// A test's program is ended before the test is: a pane's jar outlives it.
const END: Duration = Duration::from_millis(500);

/// A directory under `/tmp` whose jars end with it.
pub(super) fn short_dir() -> crate::pane::jar::test_dir::TestDir {
    crate::pane::jar::test_dir::TestDir::new("js")
}

/// The `spyc-jar` this build made.
pub(super) fn built_jar() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    crate::pane::jar::warmed(
        exe.ancestors()
            .skip(1)
            .map(|d| d.join("spyc-jar"))
            .find(|j| j.is_file())
            .expect("a built spyc-jar: make test builds it"),
    )
}

/// An executable shell script at `dir/name`.
pub(super) fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// A `spyc-jar` that marks `began` and waits for `go` before it runs: its
/// start is as slow as the test makes it.
pub(super) fn gated_jar(dir: &Path) -> PathBuf {
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

pub(super) fn jar_app(dir: &Path, binary: PathBuf, hold: Duration) -> App {
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

pub(super) fn tabs(app: &mut App) -> &mut crate::pane::PaneTabs {
    app.runtime.project_mut().pane_tabs.as_mut().unwrap()
}

fn screen(app: &mut App) -> String {
    tabs(app)
        .active()
        .with_screen(crate::pane::engine::TerminalScreen::contents)
}

pub(super) fn eventually(what: &str, mut done: impl FnMut() -> bool) {
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
pub(super) fn land(app: &mut App) {
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

pub(super) fn end(app: &mut App) {
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
        // Silent until the test makes `answers`. A mark the first start wrote
        // itself would race the hold, which kills a start slow to write it,
        // and leave the restart silent too.
        let answers = dir.path().join("answers");
        let silent_until = script(
            dir.path(),
            "silent-until",
            &format!(
                "if [ -e '{answers}' ]; then exec '{jar}' \"$@\"; fi\nexec sleep 600\n",
                answers = answers.display(),
                jar = built_jar().display(),
            ),
        );
        let mut app = jar_app(dir.path(), silent_until, Duration::from_secs(2));
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

        // The jar that answers gets a production hold: under load, starting
        // it takes longer than the two seconds that fail the first one fast.
        app.runtime.jar_place.as_mut().unwrap().hold = crate::pane::jar::place::HOLD;
        std::fs::write(&answers, "").unwrap();
        app.restart_active_tab_now();
        land(&mut app);
        // Never a word the failure's own text holds ("^a R starts it again").
        type_into(&mut app, b"restarted\r");
        shows(&mut app, "restarted");
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
pub(super) fn live_tab(app: &mut App, dir: &Path) -> i32 {
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

/// Whether spyc is offering to run tabs without jars.
pub(super) fn offering(app: &App) -> bool {
    matches!(&app.state.mode, Mode::Prompting(p) if matches!(p.kind, PromptKind::JarlessFallback))
}

/// No usable jar, with jars on: the tab doesn't start, says to check spyc's
/// installation, and spyc offers to run tabs without jars. Taken, the tab's
/// program runs once, outside any jar, and so does a tab opened after.
#[test]
fn with_no_usable_jar_a_tab_runs_without_one_once_the_user_says_so() {
    let dir = short_dir();
    crate::state::with_state_root(dir.path(), || {
        let missing = dir.path().join("missing").join("spyc-jar");
        let mut app = jar_app(dir.path(), missing, crate::pane::jar::place::HOLD);
        let ran = dir.path().join("ran");
        assert!(app.open_pane_tab_in(&marking(&ran), dir.path()));
        land(&mut app);
        shows(&mut app, "installation");
        assert!(offering(&app), "spyc offers to run tabs without jars");
        assert!(!ran.exists(), "nothing ran before the user said so");

        app.handle_key(key('y')).unwrap();
        assert!(!offering(&app));
        assert!(!tabs(&mut app).active().starting(), "started without a jar");
        assert!(tabs(&mut app).active().jar_socket().is_none(), "no jar");
        type_into(&mut app, b"typed\r");
        shows(&mut app, "typed");
        assert_eq!(ran_once(&ran), "once\n");
        end(&mut app);

        assert!(app.open_pane_tab_in("cat", dir.path()));
        assert!(!tabs(&mut app).active().starting(), "a later tab, unasked");
        assert!(!offering(&app));
        end(&mut app);
    });
}

/// Declined, a tab with no usable jar stays `[not started]`, its program
/// never run, and spyc doesn't ask again this session.
#[test]
fn with_no_usable_jar_declined_tabs_stay_not_started_and_it_asks_once() {
    let dir = short_dir();
    crate::state::with_state_root(dir.path(), || {
        let missing = dir.path().join("missing").join("spyc-jar");
        let mut app = jar_app(dir.path(), missing, crate::pane::jar::place::HOLD);
        let ran = dir.path().join("ran");
        assert!(app.open_pane_tab_in(&marking(&ran), dir.path()));
        land(&mut app);
        assert!(offering(&app), "spyc offers to run tabs without jars");
        app.handle_key(key('x')).unwrap();
        assert!(offering(&app), "a stray key doesn't answer");
        app.handle_key(key('n')).unwrap();
        assert!(!offering(&app));
        assert!(
            app.flash_text().is_some_and(|f| f.contains("jars = false")),
            "{:?}",
            app.flash_text()
        );
        eventually("the tab ended", || {
            app.drain_pane_output();
            tabs(&mut app).mark_exited();
            tabs(&mut app)
                .active_info()
                .label
                .ends_with("[not started]")
        });

        assert!(app.open_pane_tab_in(&marking(&ran), dir.path()));
        land(&mut app);
        assert!(!offering(&app), "asked once a session");
        assert!(!ran.exists(), "no program ran");
    });
}

/// A tab with no usable jar while another prompt is open waits for it to
/// close: the offer comes once nothing else is asked, never over it.
#[test]
fn with_no_usable_jar_the_offer_waits_for_an_open_prompt() {
    let dir = short_dir();
    crate::state::with_state_root(dir.path(), || {
        let missing = dir.path().join("missing").join("spyc-jar");
        let mut app = jar_app(dir.path(), missing, crate::pane::jar::place::HOLD);
        assert!(app.open_pane_tab_in("cat", dir.path()));
        app.state.mode = Mode::Prompting(Prompt::shell(PromptKind::Command, ":"));
        land(&mut app);
        assert!(!offering(&app), "the open prompt keeps its keys");
        app.state.mode = Mode::Normal;
        assert!(app.offer_jarless(), "the loop's next pass offers");
        assert!(offering(&app));
    });
}

/// What the jar at `socket` says `key` is.
pub(super) fn jar_says(socket: &Path, key: &str) -> Option<String> {
    spyc_jar::client::describe(socket)
        .ok()?
        .into_iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v)
}

/// R18: a tab's jar knows what it is, whose tab, in which project, so a
/// spyc that finds it with no session describing it can say. A rename
/// reaches the jar too.
#[test]
fn a_tabs_jar_knows_what_it_holds_and_hears_a_rename() {
    let dir = short_dir();
    crate::state::with_state_root(dir.path(), || {
        let mut app = jar_app(dir.path(), built_jar(), crate::pane::jar::place::HOLD);
        live_tab(&mut app, dir.path());
        let socket = tabs(&mut app).active().jar_socket().unwrap().to_path_buf();
        let id = tabs(&mut app).active_info().id.clone();
        let project = app.state.active_project().label();
        let home = app.state.active_project().anchor().display().to_string();
        app.settle_jar_info();
        // The jar takes the keys one at a time, and the last one told is
        // `project_home`: once it's there, the rest are.
        eventually("the jar knows its tab", || {
            jar_says(&socket, "project_home").as_deref() == Some(home.as_str())
        });
        assert_eq!(jar_says(&socket, "pane_id").as_deref(), Some(id.as_str()));
        assert_eq!(jar_says(&socket, "is").as_deref(), Some("pane"));
        assert_eq!(jar_says(&socket, "agent").as_deref(), Some("other"));
        assert_eq!(
            jar_says(&socket, "project").as_deref(),
            Some(project.as_str())
        );

        tabs(&mut app).active_info_mut().label = "renamed".into();
        app.settle_jar_info();
        eventually("the jar hears the rename", || {
            jar_says(&socket, "label").as_deref() == Some("renamed")
        });
        end(&mut app);
    });
}

/// A session saved while a tab's jar starts records no jar for it. The jar
/// landing changes what a save records, so the session is saved again with
/// it: otherwise, under load, a session never named the tab's jar.
#[test]
fn a_tabs_jar_landing_makes_the_session_save_again() {
    let dir = short_dir();
    crate::state::with_state_root(dir.path(), || {
        let mut app = jar_app(
            dir.path(),
            gated_jar(dir.path()),
            crate::pane::jar::place::HOLD,
        );
        assert!(app.open_pane_tab_in("cat", dir.path()));
        eventually("the jar began", || dir.path().join("began").exists());
        let starting = app.session_fingerprint();
        std::fs::write(dir.path().join("go"), "").unwrap();
        land(&mut app);
        assert!(tabs(&mut app).active().jar_socket().is_some());
        assert_ne!(
            app.session_fingerprint(),
            starting,
            "a save made while the jar started is never made again"
        );
        end(&mut app);
    });
}

/// S2c: a tab demoted to a task keeps its jar and its program, which goes on
/// printing into the task, and the jar hears it holds a task; brought back
/// as a tab, a pane again.
#[test]
fn a_demoted_tab_keeps_its_jar_and_the_jar_hears_it_holds_a_task() {
    let dir = short_dir();
    crate::state::with_state_root(dir.path(), || {
        let mut app = jar_app(dir.path(), built_jar(), crate::pane::jar::place::HOLD);
        let program = live_tab(&mut app, dir.path());
        let socket = tabs(&mut app).active().jar_socket().unwrap().to_path_buf();
        app.settle_jar_info();
        eventually("the jar holds a pane", || {
            jar_says(&socket, "is").as_deref() == Some("pane")
        });

        app.demote_pane_to_task();
        app.settle_jar_info();
        eventually("the jar holds a task", || {
            jar_says(&socket, "is").as_deref() == Some("task")
        });
        let task = &mut app.runtime.background_tasks.tasks[0];
        assert_eq!(task.host.jar_socket(), Some(socket.as_path()));
        assert_eq!(task.host.process_id(), u32::try_from(program).ok());
        task.host.write_all(b"in the task\r").unwrap();
        eventually("the task's program printed", || {
            app.drain_background_tasks();
            String::from_utf8_lossy(&app.runtime.background_tasks.tasks[0].buffer)
                .contains("in the task")
        });

        app.promote_task_to_pane(None);
        app.settle_jar_info();
        eventually("the jar holds a pane again", || {
            jar_says(&socket, "is").as_deref() == Some("pane")
        });
        end(&mut app);
    });
}
