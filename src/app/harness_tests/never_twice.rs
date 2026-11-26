//! A program never runs twice (review 3.0: C1, C2, P1, C12). Every start
//! whose record names a jar asks that jar first; a tab this spyc owned is
//! taken back for as long as its jar runs, however long the jar is silent and
//! whatever connection of this spyc's still holds it; `^a R` on a tab whose
//! jar runs ends the program there before anything starts, or refuses when
//! another spyc owns it; and a detached tab says why on its screen.

use super::jar_start::{built_jar, eventually, jar_app, jar_says, land, short_dir, tabs};
use super::*;
use std::io::{BufRead, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// A test's program is ended before the test is: a pane's jar outlives it.
const END: Duration = Duration::from_millis(500);

/// The bound on a retake's wait for a silent jar to have outlasted one
/// welcome (`ADOPT_WAIT`, 20 s, plus a welcome's own wait): a bound only.
const PAST_A_WELCOME: Duration = Duration::from_secs(60);

/// A program that adds its pid to `dir/pid` each time it starts, then
/// echoes what's typed.
fn marking(dir: &Path) -> String {
    format!("sh -c 'echo $$ >> {}; exec cat'", dir.join("pid").display())
}

/// The pid of each start `dir/pid` has seen, in order.
fn starts(dir: &Path) -> Vec<i32> {
    std::fs::read_to_string(dir.join("pid"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.trim().parse().ok())
        .collect()
}

fn alive(pid: i32) -> bool {
    rustix::process::Pid::from_raw(pid)
        .is_some_and(|p| rustix::process::test_kill_process(p).is_ok())
}

/// Open a jarred tab running [`marking`], landed and started: its program's
/// pid and its jar's socket.
fn marked_tab(app: &mut App, dir: &Path) -> (i32, PathBuf) {
    assert!(app.open_pane_tab_in(&marking(dir), dir));
    land(app);
    eventually("its program started", || !starts(dir).is_empty());
    let socket = tabs(app).active().jar_socket().unwrap().to_path_buf();
    (starts(dir)[0], socket)
}

/// What the active tab shows.
fn screen(app: &mut App) -> String {
    app.drain_pane_output();
    tabs(app)
        .active()
        .with_screen(crate::pane::engine::TerminalScreen::contents)
}

fn end(app: &mut App) {
    if let Some(tabs) = app.runtime.project_mut().pane_tabs.as_mut() {
        tabs.active_mut().shutdown_reporting(END, || {});
    }
}

/// Where the test's place keeps the token of the jar at `socket`.
fn token_of(app: &App, socket: &Path) -> PathBuf {
    app.runtime
        .jar_place
        .as_ref()
        .unwrap()
        .tokens
        .join(socket.file_stem().unwrap())
}

fn owner_none(socket: &Path) -> bool {
    jar_says(socket, "owner").is_some_and(|o| o.starts_with("none"))
}

/// The active tab as a saved session records it, its jar at `socket`.
fn saved_tab(app: &mut App, dir: &Path, socket: &Path) -> crate::state::sessions::SavedTab {
    let info = tabs(app).active_info();
    crate::state::sessions::SavedTab {
        command: info.command.clone(),
        label: info.command.clone(),
        cwd: dir.to_path_buf(),
        agent_kind: crate::state::sessions::AgentKind::Other,
        agent_session_id: None,
        agent_session_name: None,
        codex_home: None,
        codex_home_unresolved: false,
        claude_home: None,
        claim_owner: String::new(),
        pane_id: info.id.clone(),
        agent_hint: None,
        unopened: false,
        jar_socket: Some(socket.to_path_buf()),
        live: None,
    }
}

/// Leave the tab's jar and program running with no owner, as a crash of
/// spyc does, the tab gone from this spyc.
fn crash_away(app: &mut App, socket: &Path) {
    let gone = app.runtime.project_mut().pane_tabs.take().unwrap();
    gone.active().host.sever_jar();
    drop(gone);
    eventually("the jar has no owner", || owner_none(socket));
}

/// An owner's connection to the jar at `socket`, pinging so the jar keeps
/// it: its receiving half and its outbox, whose `close` ends it.
fn own(socket: &Path, token: &Path) -> (spyc_jar::client::Receiver, spyc_jar::client::Outbox) {
    let token = spyc_jar::client::read_token_file(token).unwrap();
    let (client, _) = spyc_jar::client::Client::connect(
        socket,
        spyc_jar::proto::Role::Owner,
        Duration::from_secs(20),
        &token,
    )
    .unwrap();
    client
        .split_queued(Duration::from_secs(1), Duration::from_secs(20))
        .unwrap()
}

/// Another spyc, as far as a jar can tell: a process of its own that owns
/// the jar at a socket until it is dropped.
struct OtherSpyc(std::process::Child);

impl OtherSpyc {
    fn owning(socket: &Path, token: &Path) -> Self {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "app::harness_tests::never_twice::own_a_jar_as_another_spyc",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("JAR_TEST_OWN", socket)
            .env("JAR_TEST_TOKEN", token)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let out = std::io::BufReader::new(child.stdout.take().unwrap());
        let owning = out
            .lines()
            .map_while(Result::ok)
            .any(|l| l.contains("owning the jar"));
        assert!(owning, "the other spyc never owned the jar");
        Self(child)
    }

    fn pid(&self) -> u32 {
        self.0.id()
    }

    /// Let go of the jar, the process still running.
    fn let_go(&mut self) {
        use std::io::Write as _;
        let stdin = self.0.stdin.as_mut().unwrap();
        stdin.write_all(b"x").unwrap();
        stdin.flush().unwrap();
    }
}

impl Drop for OtherSpyc {
    fn drop(&mut self) {
        drop(self.0.stdin.take());
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Run by the tests here as another spyc, in a process of its own: own the
/// jar at `$JAR_TEST_OWN` with the token in `$JAR_TEST_TOKEN`, say so, and
/// hold it until standard input closes.
#[test]
#[ignore = "run by the tests here as another spyc, in a process of its own"]
fn own_a_jar_as_another_spyc() {
    use std::io::Write as _;
    let (Some(socket), Some(token)) = (
        std::env::var_os("JAR_TEST_OWN"),
        std::env::var_os("JAR_TEST_TOKEN"),
    ) else {
        return;
    };
    let owner = own(Path::new(&socket), Path::new(&token));
    println!("owning the jar");
    std::io::stdout().flush().unwrap();
    // A byte lets go of the jar; the end of standard input ends the process.
    let mut byte = [0u8];
    if std::io::stdin().read(&mut byte).is_ok_and(|n| n == 1) {
        owner.1.close();
        drop(owner);
        let _ = std::io::stdin().read(&mut byte);
    }
}

/// A restored tab whose jar another spyc owns, landed detached: the program's
/// pid, the jar's socket, and that spyc.
fn detached_for_another(app: &mut App, dir: &Path) -> (i32, PathBuf, OtherSpyc) {
    let (first, socket) = marked_tab(app, dir);
    let saved = saved_tab(app, dir, &socket);
    let token = token_of(app, &socket);
    crash_away(app, &socket);
    let other = OtherSpyc::owning(&socket, &token);
    assert!(app.spawn_pane_tab(
        &saved.command,
        dir,
        super::super::pane_tabs::TabSlot::Append,
        None,
        Some(&saved),
        None,
    ));
    land(app);
    app.drain_pane_output();
    assert!(
        tabs(app).active().detached(),
        "premise: the tab is detached, its jar another spyc's"
    );
    (first, socket, other)
}

fn restart_asked(app: &App) -> bool {
    matches!(&app.state.mode, Mode::Prompting(p) if matches!(p.kind, PromptKind::RestartPane))
}

/// P1 at the tab: a tab whose link ended while a connection of this spyc's
/// still held its jar, as a stop and continue leaves one on Linux (the
/// reader gone, its connection not yet closed), is taken back once that
/// connection has gone, and never left detached for having met it. The test
/// holds that connection open, so the retake meets it whatever the order.
#[test]
fn a_retake_that_meets_this_spycs_old_connection_waits_for_it_to_go() {
    let dir = short_dir();
    crate::state::with_state_root(dir.path(), || {
        let mut app = jar_app(dir.path(), built_jar(), crate::pane::jar::place::HOLD);
        let (program, socket) = marked_tab(&mut app, dir.path());
        let token = token_of(&app, &socket);
        tabs(&mut app).active().host.sever_jar();
        eventually("the tab's lease lapsed", || {
            app.drain_pane_output();
            tabs(&mut app).active().lease_lapsed()
        });
        eventually("the jar let that connection go", || owner_none(&socket));
        let (_rx, old) = own(&socket, &token);

        assert!(app.retake_lapsed_tabs());
        eventually("the retake met the old connection", || {
            app.runtime.pane_starts.wait(Duration::from_millis(20))
                || screen(&mut app).contains("last connection")
        });
        old.close();
        land(&mut app);
        assert!(
            tabs(&mut app).active().adopted(),
            "the tab was taken back once its old connection had gone: {}",
            tabs(&mut app).active_info().label
        );
        assert_eq!(
            tabs(&mut app).active().process_id(),
            u32::try_from(program).ok(),
            "the same program"
        );
        assert_eq!(starts(dir.path()), vec![program], "started once");
        end(&mut app);
    });
}

/// C2, the lost retake: a tab whose jar is silent for longer than one
/// welcome is waited for (`ADOPT_WAIT`, 20 s) stays this spyc's to take
/// back, and is taken back once the jar answers, the same program.
#[test]
fn a_tab_whose_jar_is_silent_past_one_welcome_is_taken_back_once_it_answers() {
    use rustix::process::{Pid, Signal, kill_process};
    let dir = short_dir();
    crate::state::with_state_root(dir.path(), || {
        let mut app = jar_app(dir.path(), built_jar(), crate::pane::jar::place::HOLD);
        let (program, socket) = marked_tab(&mut app, dir.path());
        let jar: i32 = jar_says(&socket, "pid").unwrap().parse().unwrap();
        let jar = Pid::from_raw(jar).unwrap();
        kill_process(jar, Signal::STOP).unwrap();
        tabs(&mut app).active().host.sever_jar();
        eventually("the tab's lease lapsed", || {
            app.drain_pane_output();
            tabs(&mut app).active().lease_lapsed()
        });
        assert!(app.retake_lapsed_tabs());
        // The premise: silent past the one welcome a retake used to wait for.
        // The retake shows it (its stand-in says the jar hasn't answered), or
        // ends on it (its pane lands).
        let until = Instant::now() + PAST_A_WELCOME;
        loop {
            if app.runtime.pane_starts.wait(Duration::from_millis(50))
                || screen(&mut app).contains("hasn't answered")
            {
                break;
            }
            assert!(
                Instant::now() < until,
                "never: the retake waited out a welcome"
            );
        }
        kill_process(jar, Signal::CONT).unwrap();
        eventually("the tab was taken back once its jar answered", || {
            app.apply_pane_starts();
            app.retake_lapsed_tabs();
            app.drain_pane_output();
            tabs(&mut app).active().adopted()
        });
        assert_eq!(
            tabs(&mut app).active().process_id(),
            u32::try_from(program).ok(),
            "the same program"
        );
        assert_eq!(starts(dir.path()), vec![program], "started once");
        end(&mut app);
    });
}

/// C2, the double run: `^a R` on a tab whose jar runs on (here one left
/// detached, its other spyc gone since) asks, and ends the program in the
/// jar before it starts it again: two never run at once.
#[test]
fn restarting_a_tab_whose_jar_runs_ends_its_program_there_first() {
    let dir = short_dir();
    crate::state::with_state_root(dir.path(), || {
        let mut app = jar_app(dir.path(), built_jar(), crate::pane::jar::place::HOLD);
        let (first, socket, other) = detached_for_another(&mut app, dir.path());
        drop(other);
        eventually("the jar has no owner", || owner_none(&socket));

        app.restart_active_tab();
        let asked = restart_asked(&app);
        if asked {
            app.handle_key(key('y')).unwrap();
        }
        eventually("its program started again", || {
            app.apply_pane_starts();
            starts(dir.path()).len() == 2
        });
        assert!(
            !alive(first),
            "its program ran twice: {first} still ran when {:?} started",
            starts(dir.path())
        );
        assert!(asked, "^a R on a tab whose jar still runs asks first");
        end(&mut app);
    });
}

/// C2: `^a R` on a tab whose jar another spyc owns is refused, saying who,
/// and nothing starts.
#[test]
fn restarting_a_tab_another_spyc_owns_is_refused_and_says_who() {
    let dir = short_dir();
    crate::state::with_state_root(dir.path(), || {
        let mut app = jar_app(dir.path(), built_jar(), crate::pane::jar::place::HOLD);
        let (first, _socket, other) = detached_for_another(&mut app, dir.path());

        app.restart_active_tab();
        assert!(!restart_asked(&app), "refused, not asked");
        assert!(
            !tabs(&mut app).active().starting(),
            "nothing was started in the tab's place"
        );
        let flash = app.flash_text().unwrap_or_default();
        assert!(
            flash.contains(&other.pid().to_string()),
            "the refusal names the spyc that owns it: {flash:?}"
        );
        assert_eq!(starts(dir.path()), vec![first], "started once");
    });
}

/// C12: a tab detached because another spyc owns its jar says so on its
/// screen, naming that spyc.
#[test]
fn a_tab_detached_for_another_spyc_says_so_on_its_screen() {
    let dir = short_dir();
    crate::state::with_state_root(dir.path(), || {
        let mut app = jar_app(dir.path(), built_jar(), crate::pane::jar::place::HOLD);
        let (_first, _socket, other) = detached_for_another(&mut app, dir.path());
        let pid = other.pid().to_string();
        eventually("the tab says why it's detached", || {
            screen(&mut app).contains(&pid)
        });
    });
}

/// C1's pattern, by another road: a restored tab whose saved id a live tab
/// already holds keeps its record of the jar, and isn't started again beside
/// the program that jar still runs.
#[test]
fn a_restored_tab_whose_id_a_live_tab_holds_is_not_started_again() {
    let dir = short_dir();
    crate::state::with_state_root(dir.path(), || {
        let mut app = jar_app(dir.path(), built_jar(), crate::pane::jar::place::HOLD);
        let (first, socket) = marked_tab(&mut app, dir.path());
        let saved = saved_tab(&mut app, dir.path(), &socket);
        assert!(app.open_restored_pane_tab(&saved.command, dir.path(), None, &saved));
        land(&mut app);
        eventually("the restored tab started its program or didn't", || {
            !tabs(&mut app).active().jarred() || starts(dir.path()).len() == 2
        });
        assert_eq!(starts(dir.path()), vec![first], "its program ran twice");
        end(&mut app);
        tabs(&mut app).tabs_mut()[0]
            .pane
            .shutdown_reporting(END, || {});
    });
}

/// P1: a retake closes the tab's old connection itself. The test keeps the
/// old link alive, as another of the pane's handles can, and ends the tab's
/// reader with the connection still open, as an interrupted read leaves it:
/// only the retake's own close lets the jar take the tab back.
#[test]
fn a_retake_closes_the_tabs_old_connection_itself() {
    let dir = short_dir();
    crate::state::with_state_root(dir.path(), || {
        let mut app = jar_app(dir.path(), built_jar(), crate::pane::jar::place::HOLD);
        let (program, _socket) = marked_tab(&mut app, dir.path());
        let kept = tabs(&mut app).active().host.jar_signals();
        assert!(kept.is_some(), "premise: the tab's link");
        let host = &mut tabs(&mut app).active_mut().host;
        host.closed = true;
        host.exit_status = Some(portable_pty::ExitStatus::with_signal(
            crate::pane::jar::Lost::Held.label(),
        ));
        assert!(
            tabs(&mut app).active().lease_lapsed(),
            "premise: the lease lapsed, the connection open"
        );
        assert!(app.retake_lapsed_tabs());
        land(&mut app);
        assert!(
            tabs(&mut app).active().adopted(),
            "the tab was taken back: {}",
            tabs(&mut app).active_info().label
        );
        assert_eq!(
            tabs(&mut app).active().process_id(),
            u32::try_from(program).ok(),
            "the same program"
        );
        drop(kept);
        end(&mut app);
    });
}

/// The owner's decision 1: a restore with jars off and a live jar brings the
/// tab back `[detached]`, never started again, its screen saying why and
/// what to do, and its record of the jar kept for the next save.
#[test]
fn a_restore_with_jars_off_leaves_the_tab_detached_saying_why() {
    let dir = short_dir();
    crate::state::with_state_root(dir.path(), || {
        let mut app = jar_app(dir.path(), built_jar(), crate::pane::jar::place::HOLD);
        let (first, socket) = left_in_its_jar_with_jars_off(&mut app, dir.path());
        assert_eq!(starts(dir.path()), vec![first], "its program ran twice");
        assert!(tabs(&mut app).active().detached(), "the tab is [detached]");
        assert_eq!(
            tabs(&mut app).active().jar_socket(),
            Some(socket.as_path()),
            "the tab keeps its record of the jar"
        );
        eventually("the screen says why and what to do", || {
            let shown = screen(&mut app);
            shown.contains("jars = false") && shown.contains("^a R")
        });
    });
}

/// The owner's decision 2 with jars off: `^a R` on such a tab asks, ends the
/// program in its jar and sees it gone, then starts it on a PTY of spyc's.
#[test]
fn restarting_with_jars_off_ends_the_program_in_its_jar_first() {
    let dir = short_dir();
    crate::state::with_state_root(dir.path(), || {
        let mut app = jar_app(dir.path(), built_jar(), crate::pane::jar::place::HOLD);
        let (first, _socket) = left_in_its_jar_with_jars_off(&mut app, dir.path());
        assert!(
            tabs(&mut app).active().detached(),
            "premise: the tab is [detached]"
        );
        app.restart_active_tab();
        assert!(restart_asked(&app), "^a R asks first");
        app.handle_key(key('y')).unwrap();
        eventually("its program started again", || {
            app.apply_pane_starts();
            starts(dir.path()).len() == 2
        });
        assert!(
            !alive(first),
            "its program ran twice: {first} still ran when {:?} started",
            starts(dir.path())
        );
        assert!(!tabs(&mut app).active().jarred(), "started here, jars off");
        end(&mut app);
    });
}

/// A restore that finds its jar's socket gone while the jar runs (a cleaner
/// removed it; the jar binds it again a moment later) asks the jar's lock,
/// and never starts the program beside it. The jar is held stopped so it
/// can't bind its socket again before the restore asks.
#[test]
fn a_restored_tab_whose_jar_lost_its_socket_file_is_not_started_again() {
    use rustix::process::{Pid, Signal, kill_process};
    let dir = short_dir();
    crate::state::with_state_root(dir.path(), || {
        let mut app = jar_app(dir.path(), built_jar(), crate::pane::jar::place::HOLD);
        let (first, socket) = marked_tab(&mut app, dir.path());
        let saved = saved_tab(&mut app, dir.path(), &socket);
        let jar: i32 = jar_says(&socket, "pid").unwrap().parse().unwrap();
        let jar = Pid::from_raw(jar).unwrap();
        crash_away(&mut app, &socket);
        kill_process(jar, Signal::STOP).unwrap();
        std::fs::remove_file(&socket).unwrap();
        assert!(app.spawn_pane_tab(
            &saved.command,
            dir.path(),
            super::super::pane_tabs::TabSlot::Append,
            None,
            Some(&saved),
            None,
        ));
        kill_process(jar, Signal::CONT).unwrap();
        land(&mut app);
        eventually("the tab was taken back or started again", || {
            app.drain_pane_output();
            tabs(&mut app).active().adopted() || starts(dir.path()).len() > 1
        });
        assert_eq!(starts(dir.path()), vec![first], "its program ran twice");
        end(&mut app);
    });
}

/// A jarred tab left running in its jar, as a crash leaves it, then restored
/// by a spyc that runs tabs without jars: its program's pid and the jar's
/// socket, once the restore has landed one way or the other.
fn left_in_its_jar_with_jars_off(app: &mut App, dir: &Path) -> (i32, PathBuf) {
    let (first, socket) = marked_tab(app, dir);
    let saved = saved_tab(app, dir, &socket);
    crash_away(app, &socket);
    app.state.active_project_mut().config.pane.jars = false;
    assert!(app.spawn_pane_tab(
        &saved.command,
        dir,
        super::super::pane_tabs::TabSlot::Append,
        None,
        Some(&saved),
        None,
    ));
    eventually(
        "the restored tab started its program or was left to its jar",
        || {
            app.drain_pane_output();
            tabs(app).active().detached() || starts(dir).len() > 1
        },
    );
    (first, socket)
}

/// A tab whose jar this spyc has no token for says so, and `^a R` refuses
/// rather than offer to end a program it can't reach: nothing starts.
#[test]
fn a_tab_whose_jar_has_no_token_here_says_so_and_restart_refuses() {
    let dir = short_dir();
    crate::state::with_state_root(dir.path(), || {
        let mut app = jar_app(dir.path(), built_jar(), crate::pane::jar::place::HOLD);
        let (first, socket) = marked_tab(&mut app, dir.path());
        let saved = saved_tab(&mut app, dir.path(), &socket);
        let token = token_of(&app, &socket);
        crash_away(&mut app, &socket);
        std::fs::remove_file(&token).unwrap();
        assert!(app.spawn_pane_tab(
            &saved.command,
            dir.path(),
            super::super::pane_tabs::TabSlot::Append,
            None,
            Some(&saved),
            None,
        ));
        land(&mut app);
        eventually("the tab says it has no token for its jar", || {
            screen(&mut app).contains("no token")
        });
        assert!(
            tabs(&mut app).active().detached(),
            "[detached], not a tab whose restart ends the program in its jar"
        );
        assert!(
            !screen(&mut app).contains("^a R ends it"),
            "its screen promises what ^a R won't do: {}",
            screen(&mut app)
        );
        app.restart_active_tab();
        assert!(!restart_asked(&app), "refused, not asked");
        assert!(!tabs(&mut app).active().starting(), "nothing was started");
        let flash = app.flash_text().unwrap_or_default();
        assert!(flash.contains("token"), "the refusal says why: {flash:?}");
        assert_eq!(starts(dir.path()), vec![first], "started once");
        // Nothing here owns the jar: end it and its program by hand.
        for key in ["program_pid", "pid"] {
            if let Some(pid) = jar_says(&socket, key).and_then(|p| p.parse().ok()) {
                let _ = rustix::process::kill_process_group(
                    rustix::process::Pid::from_raw(pid).unwrap(),
                    rustix::process::Signal::KILL,
                );
            }
        }
    });
}

/// A live tab's restart confirmed after its jar's token went still ends the
/// program first, over the tab's own connection, and then starts it: the
/// jar ending by itself needs no token.
#[test]
fn a_live_tabs_restart_whose_token_went_after_the_question_ends_then_starts() {
    let dir = short_dir();
    crate::state::with_state_root(dir.path(), || {
        let mut app = jar_app(dir.path(), built_jar(), crate::pane::jar::place::HOLD);
        let (first, socket) = marked_tab(&mut app, dir.path());
        app.restart_active_tab();
        assert!(restart_asked(&app), "premise: ^a R asks");
        std::fs::remove_file(token_of(&app, &socket)).unwrap();
        app.handle_key(key('y')).unwrap();
        eventually("its program started again", || {
            app.apply_pane_starts();
            starts(dir.path()).len() == 2
        });
        assert!(!alive(first), "its program ran twice");
        end(&mut app);
    });
}

/// A detached tab's restart confirmed after its jar's token went (a
/// cleaner, another spyc's `--remove`) can't end the program there: the tab
/// stays `[detached]`, its screen says why and promises nothing `^a R`
/// won't do, and nothing starts.
#[test]
fn a_detached_tabs_restart_whose_token_went_after_the_question_starts_nothing_and_says_why() {
    let dir = short_dir();
    crate::state::with_state_root(dir.path(), || {
        let mut app = jar_app(dir.path(), built_jar(), crate::pane::jar::place::HOLD);
        let (first, socket, mut other) = detached_for_another(&mut app, dir.path());
        other.let_go();
        eventually("the jar has no owner", || owner_none(&socket));
        app.restart_active_tab();
        assert!(restart_asked(&app), "premise: ^a R asks");
        std::fs::remove_file(token_of(&app, &socket)).unwrap();
        app.handle_key(key('y')).unwrap();
        // Past the restart's own wait for the jar to end (`END_HELD_WAIT`).
        let until = std::time::Instant::now() + std::time::Duration::from_secs(90);
        while !screen(&mut app).contains("no token") {
            assert!(
                std::time::Instant::now() < until,
                "never: the tab says it has no token for its jar"
            );
            app.apply_pane_starts();
            app.drain_pane_output();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(tabs(&mut app).active().detached(), "the tab is [detached]");
        assert!(
            !screen(&mut app).contains("^a R ends it"),
            "its screen promises what ^a R won't do: {}",
            screen(&mut app)
        );
        assert_eq!(starts(dir.path()), vec![first], "started once");
    });
}

/// A restored tab whose jar nothing can tell about (a socket's path with no
/// lock and nothing answering) isn't started, says so, and `^a R` refuses
/// rather than start it beside a jar that may still run.
#[test]
fn a_tab_whose_jar_cant_be_told_about_says_so_and_restart_refuses() {
    let dir = short_dir();
    crate::state::with_state_root(dir.path(), || {
        let mut app = jar_app(dir.path(), built_jar(), crate::pane::jar::place::HOLD);
        let unknown = dir.path().join("unknown.sock");
        std::fs::write(&unknown, "").unwrap();
        let command = marking(dir.path());
        let saved = crate::state::sessions::SavedTab {
            command: command.clone(),
            label: command.clone(),
            cwd: dir.path().to_path_buf(),
            agent_kind: crate::state::sessions::AgentKind::Other,
            agent_session_id: None,
            agent_session_name: None,
            codex_home: None,
            codex_home_unresolved: false,
            claude_home: None,
            claim_owner: String::new(),
            pane_id: uuid::Uuid::now_v7().to_string(),
            agent_hint: None,
            unopened: false,
            jar_socket: Some(unknown),
            live: None,
        };
        assert!(app.spawn_pane_tab(
            &command,
            dir.path(),
            super::super::pane_tabs::TabSlot::Append,
            None,
            Some(&saved),
            None,
        ));
        land(&mut app);
        eventually("the tab started or said it can't tell", || {
            screen(&mut app).contains("Can't tell") || !starts(dir.path()).is_empty()
        });
        assert!(starts(dir.path()).is_empty(), "its program was started");
        app.restart_active_tab();
        assert!(!restart_asked(&app), "refused, not asked");
        assert!(!tabs(&mut app).active().starting(), "nothing was started");
        let flash = app.flash_text().unwrap_or_default();
        assert!(
            flash.contains("can't tell"),
            "the refusal says why: {flash:?}"
        );
    });
}

/// `^a R`'s refusal asks the jar, not what this spyc saw at the restore: a
/// spyc that owned the tab's jar then and has let go since, though it still
/// runs, no longer keeps `^a R` from asking.
#[test]
fn restarting_a_tab_whose_other_spyc_let_go_asks() {
    let dir = short_dir();
    crate::state::with_state_root(dir.path(), || {
        let mut app = jar_app(dir.path(), built_jar(), crate::pane::jar::place::HOLD);
        let (first, socket, mut other) = detached_for_another(&mut app, dir.path());
        other.let_go();
        eventually("the jar has no owner", || owner_none(&socket));
        app.restart_active_tab();
        assert!(
            restart_asked(&app),
            "^a R asks, the jar's other spyc gone: {:?}",
            app.flash_text()
        );
        app.handle_key(key('n')).unwrap();
        assert_eq!(starts(dir.path()), vec![first], "started once");
    });
}
