//! S2c: jars no spyc owns, offered back at a spyc's start
//! (`app::orphan_jars`): restored, a pane comes back as a tab with the same
//! program, never started again; discarded, it ends with its program; a jar
//! this spyc holds is never offered, and the look is made once.

use super::jar_start::{
    WAIT, built_jar, end, eventually, jar_app, jar_says, land, live_tab, short_dir, tabs,
};
use super::*;
use std::path::Path;

/// A spyc's tab, left by a crash of that spyc: its jar and program run on,
/// the jar told what it holds. Its program's pid and its jar's socket.
fn left_by_a_crash(dir: &Path) -> (i32, std::path::PathBuf) {
    let mut first = jar_app(dir, built_jar(), crate::pane::jar::place::HOLD);
    let program = live_tab(&mut first, dir);
    let socket = tabs(&mut first)
        .active()
        .jar_socket()
        .unwrap()
        .to_path_buf();
    let home = first.state.active_project().anchor().display().to_string();
    first.settle_jar_info();
    eventually("the jar knows its tab", || {
        jar_says(&socket, "project_home").as_deref() == Some(home.as_str())
    });
    let crashed = first.runtime.project_mut().pane_tabs.take().unwrap();
    crashed.active().host.sever_jar();
    std::mem::forget(crashed);
    std::mem::forget(first);
    (program, socket)
}

/// Look for jars no spyc owns, as the loop does once startup settles, and
/// land what it finds.
fn look(app: &mut App) {
    assert!(app.settle_orphan_jars(), "the look starts");
    assert!(
        app.runtime.orphan_results.wait(WAIT + WAIT),
        "the look never landed"
    );
    app.apply_orphan_jars();
}

fn offering(app: &App) -> bool {
    matches!(&app.state.mode, Mode::Prompting(p) if matches!(p.kind, PromptKind::OrphanJars))
}

fn running(pid: i32) -> bool {
    rustix::process::Pid::from_raw(pid)
        .is_some_and(|p| rustix::process::test_kill_process(p).is_ok())
}

/// Restored, a jar no spyc owns comes back as a tab, the same program,
/// taken back rather than started again.
#[test]
fn a_jar_no_spyc_owns_is_offered_and_restored_as_its_tab() {
    let dir = short_dir();
    crate::state::with_state_root(dir.path(), || {
        let (program, socket) = left_by_a_crash(dir.path());
        let mut app = jar_app(dir.path(), built_jar(), crate::pane::jar::place::HOLD);
        look(&mut app);
        assert!(offering(&app), "spyc offers the jar back");
        app.handle_key(key('x')).unwrap();
        assert!(offering(&app), "a stray key doesn't answer");
        app.handle_key(key('r')).unwrap();
        assert!(!offering(&app));
        land(&mut app);
        assert!(tabs(&mut app).active().adopted(), "its jar was taken back");
        assert_eq!(tabs(&mut app).active().jar_socket(), Some(socket.as_path()));
        assert_eq!(
            tabs(&mut app).active().process_id(),
            u32::try_from(program).ok(),
            "the same program"
        );
        assert!(!app.settle_orphan_jars(), "the look is made once");
        end(&mut app);
    });
}

/// Discarded, a jar no spyc owns ends, and its program with it.
#[test]
fn a_discarded_jar_no_spyc_owns_ends_with_its_program() {
    let dir = short_dir();
    crate::state::with_state_root(dir.path(), || {
        let (program, socket) = left_by_a_crash(dir.path());
        let mut app = jar_app(dir.path(), built_jar(), crate::pane::jar::place::HOLD);
        look(&mut app);
        assert!(offering(&app));
        app.handle_key(key('d')).unwrap();
        eventually("the jar and its program ended", || {
            !running(program) && !socket.exists()
        });
    });
}

/// A key that reached spyc before the offer was ever drawn answers nothing:
/// typed for whatever the screen showed then, a `d` would end every program
/// the offer names.
#[test]
#[ignore = "red until the orphan prompt is fixed (fix/orphan-discard-confirm)"]
fn a_key_before_the_offer_is_drawn_doesnt_answer_it() {
    let dir = short_dir();
    crate::state::with_state_root(dir.path(), || {
        let (program, socket) = left_by_a_crash(dir.path());
        let mut app = jar_app(dir.path(), built_jar(), crate::pane::jar::place::HOLD);
        look(&mut app);
        assert!(offering(&app), "spyc offers the jar back");
        app.handle_key(key('d')).unwrap();
        let answered = !offering(&app);
        let ran = running(program);
        // The jar no spyc owns, ended by the test once it has looked.
        let _ = crate::pane::jar::orphans::discard(app.runtime.jar_place.clone(), &socket);
        assert!(
            !answered,
            "a key typed before the offer appeared answered it"
        );
        assert!(ran, "its program was ended");
    });
}

/// A jar this spyc holds is never offered.
#[test]
fn a_jar_this_spyc_holds_is_not_offered() {
    let dir = short_dir();
    crate::state::with_state_root(dir.path(), || {
        let mut app = jar_app(dir.path(), built_jar(), crate::pane::jar::place::HOLD);
        live_tab(&mut app, dir.path());
        look(&mut app);
        assert!(!offering(&app), "an owned jar was offered");
        end(&mut app);
    });
}
