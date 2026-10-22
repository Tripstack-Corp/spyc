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
/// the picker back with its rows and its digits live.
#[test]
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
        app.handle_key(key('1')).unwrap();
        assert_eq!(
            app.state.session_id,
            Some(301),
            "the picker's digits are dead"
        );
    });
}

/// In help over the jump-history popup, `x` deletes no entry, and back in
/// the popup it deletes the one highlighted, the newest.
#[test]
fn help_over_the_jump_history_deletes_nothing() {
    let mut app = App::test_app(std::env::temp_dir());
    app.state.jump_history.push("/one");
    app.state.jump_history.push("/two");
    app.show_jump_history_popup();
    open_help(&mut app);
    app.handle_key(key('x')).unwrap();
    assert_eq!(app.state.jump_history.entries(), ["/one", "/two"]);
    press(&mut app, KeyCode::Char('q'));
    app.handle_key(key('x')).unwrap();
    assert_eq!(app.state.jump_history.entries(), ["/one"], "x is dead");
}

/// A pager a worker opens over the switcher, a git diff landing late, keeps
/// its own keys: a digit or Enter there goes to no project.
#[test]
fn a_pager_opened_over_the_switcher_keeps_its_own_keys() {
    with_three(|t| {
        let x = t.x;
        let app = &mut t.app;
        leader(app, 'j');
        let diff = crate::ui::pager::PagerView::new_plain("git diff", vec!["+a".to_owned()]);
        app.set_pager(diff);
        app.handle_key(key('2')).unwrap();
        press(app, KeyCode::Enter);
        assert_eq!(
            app.state.active, x,
            "a key in the diff went to the switcher"
        );
    });
}

/// A pager opened over the `!` history popup, a worker's landing late, keeps
/// its own keys: `^D` there deletes no history entry.
#[test]
fn a_pager_opened_over_the_history_popup_deletes_nothing() {
    let mut app = App::test_app(std::env::temp_dir());
    app.state.history.push("echo one");
    app.state.history.push("echo two");
    app.show_history_popup();
    let diff = crate::ui::pager::PagerView::new_plain("git diff", vec!["+a".to_owned()]);
    app.set_pager(diff);
    app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL))
        .unwrap();
    assert_eq!(app.state.history.entries(), ["echo one", "echo two"]);
}

/// A `/` search in help over the switcher moves the help, and puts no picker
/// cursor on it.
#[test]
fn a_search_in_help_over_the_switcher_moves_no_picker_cursor() {
    with_three(|t| {
        let app = &mut t.app;
        leader(app, 'j');
        open_help(app);
        for c in "/pager".chars() {
            app.handle_key(key(c)).unwrap();
        }
        press(app, KeyCode::Enter);
        let help = app.view.project().pager.as_ref().expect("the help");
        assert!(
            help.current_match_line().is_some(),
            "the search matched nothing"
        );
        assert_eq!(help.picker_cursor, None);
    });
}

/// `W l` opens the worktree picker on its own pager: `j` moves its highlight
/// to the next worktree.
#[test]
fn the_worktree_picker_takes_its_own_keys() {
    use crate::git::test_support::run_git;
    let tmp = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(tmp.path()).unwrap();
    let repo = base.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    run_git(&repo, &["init"]);
    std::fs::write(repo.join("f.txt"), "v1\n").unwrap();
    run_git(&repo, &["add", "f.txt"]);
    run_git(&repo, &["commit", "-m", "c1"]);
    let other = base.join("other");
    run_git(
        &repo,
        &[
            "worktree",
            "add",
            &other.display().to_string(),
            "-b",
            "other",
        ],
    );
    crate::state::with_state_root(&base.join("state"), || {
        let mut app = App::test_app(repo.clone());
        app.worktree_list();
        let cursor = |app: &App| {
            app.view
                .project()
                .pager
                .as_ref()
                .and_then(|v| v.picker_cursor)
        };
        let start = cursor(&app).expect("W l opened no picker");
        app.handle_key(key('j')).unwrap();
        assert_ne!(cursor(&app), Some(start), "j didn't move the highlight");
    });
}

/// A pager opened over the switcher and closed with `q` goes into the buffer
/// history, as any pager does: it isn't the switcher.
#[test]
fn a_pager_closed_over_the_switcher_is_kept_for_bracket_b() {
    with_three(|t| {
        let app = &mut t.app;
        leader(app, 'j');
        let diff = crate::ui::pager::PagerView::new_plain("git diff", vec!["+a".to_owned()]);
        app.set_pager(diff);
        press(app, KeyCode::Char('q'));
        assert_eq!(app.view.project().pager_history.back_len(), 1);
    });
}

/// Clearing the pickers' rows drops every picker's pager from the buffer
/// history, so `]b` back to an old picker can't hand it a newer picker's
/// rows.
#[test]
fn clearing_the_pickers_drops_their_pagers_from_the_history() {
    let mut app = App::test_app(std::env::temp_dir());
    let mut old = crate::ui::pager::PagerView::new_plain("projects", vec!["x".to_owned()]);
    old.picker = Some(crate::ui::pager::PickerKind::Projects);
    app.view.project_mut().pager_history.push(old);
    let notes = crate::ui::pager::PagerView::new_plain("notes", vec!["n".to_owned()]);
    app.view.project_mut().pager_history.push(notes);
    app.clear_pending_pickers();
    assert_eq!(
        app.view.project().pager_history.back_len(),
        1,
        "a picker's pager is kept"
    );
}
