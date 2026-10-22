//! #806: a picker's keys act only while its own pager is the one in view. The
//! pager's help (`H`) opened over a picker takes none of the picker's keys, and
//! `q` brings the picker back with its keys live again.

use super::manifest_v2::{crate_root, in_temp_home, manifest, project, write_manifest};
use super::projects_open::{leader, with_three};
use super::*;

const P1: &str = "0192f0e4-7c3a-7b2e-9a51-3c1d2e4f5c01";

/// The title of the pager in view.
fn title(app: &App) -> Option<String> {
    app.view.project().pager.as_ref().map(|v| v.title.clone())
}

/// `H`, the pager's help, over whatever pager is open.
fn open_help(app: &mut App) {
    app.handle_key(key('H')).unwrap();
    assert_eq!(
        title(app).as_deref(),
        Some(crate::ui::pager::PAGER_HELP_TITLE),
        "H opened no help"
    );
}

fn press(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
        .unwrap();
}

/// In help over the switcher, a digit and Enter go to no project, and `q`
/// brings the switcher back, its digits live again.
#[test]
#[ignore = "red: #806, help over the switcher switches project"]
fn help_over_the_switcher_takes_none_of_its_keys() {
    with_three(|t| {
        let (x, y) = (t.x, t.y);
        let app = &mut t.app;
        leader(app, 'j');
        let switcher = title(app).expect("Space j opened no switcher");
        open_help(app);
        app.handle_key(key('2')).unwrap();
        press(app, KeyCode::Enter);
        assert_eq!(app.state.active, x, "a key in help went to the switcher");
        press(app, KeyCode::Char('q'));
        assert_eq!(
            title(app),
            Some(switcher),
            "q didn't bring the switcher back"
        );
        app.handle_key(key('2')).unwrap();
        assert_eq!(app.state.active, y, "the switcher's digits are dead");
    });
}

/// `H` again in the help over the switcher leaves the switcher stashed under
/// it, so `q` still brings the switcher back with its digits live.
#[test]
#[ignore = "red: #806, a second H stashes the help over the switcher"]
fn help_twice_over_the_switcher_still_returns_to_it() {
    with_three(|t| {
        let y = t.y;
        let app = &mut t.app;
        leader(app, 'j');
        let switcher = title(app).expect("Space j opened no switcher");
        open_help(app);
        open_help(app);
        press(app, KeyCode::Char('q'));
        assert_eq!(
            title(app),
            Some(switcher),
            "q didn't bring the switcher back"
        );
        app.handle_key(key('2')).unwrap();
        assert_eq!(app.state.active, y, "the switcher's digits are dead");
    });
}

/// In help over the `-r` picker, a digit restores no session, and `q` brings
/// the picker back with its rows.
#[test]
#[ignore = "red: #806, help over the session picker restores a session"]
fn help_over_the_session_picker_restores_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    in_temp_home(tmp.path(), |state_root| {
        let root = crate_root();
        let saved = manifest(
            301,
            "PICKED",
            1_700_000_000,
            P1,
            vec![project(P1, &root, &root, vec![])],
        );
        write_manifest(state_root, 301, &saved);
        let mut app = App::test_app(root);
        app.show_session_picker();
        let picker = title(&app).expect("the session picker opened");
        open_help(&mut app);
        app.handle_key(key('1')).unwrap();
        assert_ne!(
            app.state.session_id,
            Some(301),
            "a digit in help restored it"
        );
        assert!(
            app.state.pending_sessions.is_some(),
            "the picker's rows went"
        );
        press(&mut app, KeyCode::Char('q'));
        assert_eq!(title(&app), Some(picker), "q didn't bring the picker back");
    });
}

/// In help over the jump-history popup, `x` deletes no entry.
#[test]
#[ignore = "red: #806, help over the jump history deletes an entry"]
fn help_over_the_jump_history_deletes_nothing() {
    let mut app = App::test_app(std::env::temp_dir());
    app.state.jump_history.push("/one");
    app.state.jump_history.push("/two");
    app.show_jump_history_popup();
    open_help(&mut app);
    app.handle_key(key('x')).unwrap();
    assert_eq!(app.state.jump_history.entries(), ["/one", "/two"]);
}
