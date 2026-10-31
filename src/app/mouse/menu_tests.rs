//! The menu of the object under the cursor or the pointer: `.` for the entry
//! under the cursor, `^a .` for the active pane tab, and a right-click for
//! whatever is under the pointer. Each case goes through the real entry point,
//! `handle_key` or `handle_mouse`, and reads the menu as the frame draws it.

use super::tests::{
    FRAME, at, centre, click, drawn, drawn_menu, drawn_row, inside, menu_in, press,
};
use crate::app::{App, Effect, PaneInput, PaneTarget};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

fn special(app: &mut App, code: KeyCode) -> Vec<Effect> {
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
        .unwrap()
}

fn ctrl_a(app: &mut App) {
    app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL))
        .unwrap();
}

/// An App over `work/`, holding a plain file, an executable and a directory.
fn app_over_entries(tmp: &Path) -> App {
    let dir = tmp.join("work");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("notes.txt"), "x").unwrap();
    let run = dir.join("run.sh");
    std::fs::write(&run, "#!/bin/sh\necho hi\n").unwrap();
    std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::create_dir(dir.join("sub")).unwrap();
    let mut app = App::test_app(dir);
    app.view.term_size = (FRAME.width, FRAME.height);
    app.state.refresh_listing();
    app
}

/// Put the cursor on the entry called `name`, and return its path.
fn cursor_on(app: &mut App, name: &str) -> PathBuf {
    let at = app
        .state
        .cur()
        .rows
        .iter()
        .position(|r| r.path.file_name().is_some_and(|n| n == name))
        .unwrap_or_else(|| panic!("{name} is listed"));
    app.state.cur_mut().cursor.index = at;
    app.state.cur().rows[at].path.clone()
}

/// `.` on the entry called `name`: the menu it opens, each body line as drawn.
fn dot_menu(app: &mut App, name: &str) -> Vec<String> {
    cursor_on(app, name);
    press(app, '.');
    assert!(app.view.chord_hint.is_some(), "premise: `.` opened a menu");
    menu_lines(app, name)
}

/// The body lines of the menu titled `title`, as drawn.
fn menu_lines(app: &mut App, title: &str) -> Vec<String> {
    let buf = drawn(app);
    let menu = menu_in(&buf, title);
    (menu.y + 1..menu.bottom() - 1)
        .map(|y| {
            (menu.x + 1..menu.right() - 1)
                .map(|x| buf[(x, y)].symbol())
                .collect()
        })
        .collect()
}

fn offers(lines: &[String], text: &str) -> bool {
    lines.iter().any(|l| l.contains(text))
}

/// The active tab's label, which titles its menu.
fn tab_label(app: &App) -> String {
    app.runtime
        .project()
        .pane_tabs
        .as_ref()
        .expect("a pane tab")
        .active_info()
        .label
        .clone()
}

/// An App over `work/` with one pane tab running `cmd`, the keyboard in it.
fn app_with_tab(tmp: &Path, cmd: &str) -> App {
    let dir = tmp.join("work");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("notes.txt"), "x").unwrap();
    let mut app = App::test_app(dir);
    app.view.term_size = (FRAME.width, FRAME.height);
    app.state.refresh_listing();
    app.open_pane_tab(cmd);
    app.set_pane_focus(true);
    assert!(
        app.state.pane_focused(),
        "premise: the pane has the keyboard"
    );
    app
}

fn right_click(app: &mut App, (col, row): (u16, u16)) {
    app.handle_mouse(at(MouseEventKind::Down(MouseButton::Right), col, row));
    app.handle_mouse(at(MouseEventKind::Up(MouseButton::Right), col, row));
}

/// Where `text` is drawn on the screen.
fn drawn_at(app: &mut App, text: &str) -> (u16, u16) {
    let buf = drawn(app);
    for y in 0..FRAME.height {
        let cells: Vec<String> = (0..FRAME.width)
            .map(|x| buf[(x, y)].symbol().to_string())
            .collect();
        let line = cells.concat();
        let Some(byte) = line.find(text) else {
            continue;
        };
        let mut at = 0;
        for (i, cell) in cells.iter().enumerate() {
            if at == byte {
                return (u16::try_from(i).unwrap(), y);
            }
            at += cell.len();
        }
    }
    panic!("{text:?} is not drawn");
}

fn sent_to_pane(fx: &[Effect], c: char) -> bool {
    fx.iter().any(|e| {
        matches!(e, Effect::SendToPane {
            target: PaneTarget::Active,
            input: PaneInput::Key(k),
            ..
        } if k.code == KeyCode::Char(c))
    })
}

// ── `.` on an entry ──

/// `.` opens a menu for the file under the cursor, titled with its name, and
/// each row shows the key that does the same: `e` edits it, and a plain file
/// can be made executable but not run.
#[test]
fn a_dot_opens_the_menu_of_the_file_under_the_cursor() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = app_over_entries(tmp.path());

        let menu = dot_menu(&mut app, "notes.txt");

        assert!(
            offers(&menu, "e  enter dir / editor"),
            "`e` edits the file: {menu:#?}"
        );
        assert!(
            offers(&menu, ":chmod"),
            "a plain file can be made executable"
        );
        assert!(!offers(&menu, ":exec"), "but not run: {menu:#?}");
    });
}

/// A directory has nothing to edit or view in a pane.
#[test]
fn a_directorys_menu_has_no_file_rows() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = app_over_entries(tmp.path());

        let menu = dot_menu(&mut app, "sub");

        for label in [
            "enter dir / editor",
            "open editor in top pane",
            "open $PAGER in top pane",
        ] {
            assert!(!offers(&menu, label), "{label:?} on a directory: {menu:#?}");
        }
        assert!(offers(&menu, "toggle pick"), "premise: the menu is drawn");
    });
}

/// An executable can be run, and is executable already.
#[test]
fn an_executables_menu_runs_it_and_offers_no_chmod() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = app_over_entries(tmp.path());

        let menu = dot_menu(&mut app, "run.sh");

        assert!(offers(&menu, ":exec"), "the executable runs: {menu:#?}");
        assert!(!offers(&menu, ":chmod"), "and is executable already");
    });
}

/// A row's key, typed with the menu open, does what the row says, and the
/// menu closes: the menu teaches the key rather than replacing it.
#[test]
fn a_rows_key_typed_in_the_menu_does_what_the_row_says() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = app_over_entries(tmp.path());
        let path = cursor_on(&mut app, "notes.txt");
        press(&mut app, '.');
        assert!(app.view.chord_hint.is_some(), "premise: the menu is open");

        press(&mut app, 't');

        assert!(app.state.cur().picks.contains(&path), "`t` picked the file");
        assert!(app.view.chord_hint.is_none(), "and the menu closed");
    });
}

/// `↓` moves a highlight down the rows from the first, the highlighted row is
/// drawn reversed, and `Enter` fires it.
#[test]
fn arrows_highlight_a_row_and_enter_fires_it() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = app_over_entries(tmp.path());
        let path = cursor_on(&mut app, "sub");
        press(&mut app, '.');
        assert!(app.view.chord_hint.is_some(), "premise: the menu is open");

        for _ in 0..3 {
            special(&mut app, KeyCode::Down);
        }
        // A directory's rows open `Enter`, `t`, `yf`: the third yanks its path.
        let buf = drawn(&mut app);
        let (col, row) = drawn_row(&mut app, "sub", "yank cursor file path");
        assert!(
            buf[(col, row)]
                .modifier
                .contains(ratatui::style::Modifier::REVERSED),
            "the highlighted row is drawn reversed"
        );
        let fx = special(&mut app, KeyCode::Enter);

        let yanked = path.display().to_string();
        assert!(
            fx.iter()
                .any(|e| matches!(e, Effect::CopyToClipboard { text, .. } if *text == yanked)),
            "the third row fired, yanking the path: {fx:?}"
        );
        assert!(app.view.chord_hint.is_none(), "and the menu closed");
    });
}

/// `Esc` closes the menu and does nothing else.
#[test]
fn esc_closes_the_menu_without_acting() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = app_over_entries(tmp.path());
        let path = cursor_on(&mut app, "sub");
        let cursor = app.state.cur().cursor.index;
        let dir = app.state.cur().listing.dir.clone();
        press(&mut app, '.');
        assert!(app.view.chord_hint.is_some(), "premise: the menu is open");

        special(&mut app, KeyCode::Down);
        special(&mut app, KeyCode::Esc);

        assert!(app.view.chord_hint.is_none(), "the menu closed");
        assert_eq!(app.state.cur().listing.dir, dir, "nothing was entered");
        assert_eq!(app.state.cur().cursor.index, cursor, "the cursor stayed");
        assert!(!app.state.cur().picks.contains(&path), "nothing was picked");
    });
}

/// From the pane, `.` is the child's: it is typed into the agent, and no menu
/// opens.
#[test]
fn a_dot_typed_in_the_pane_reaches_the_child() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = app_with_tab(tmp.path(), "cat");

        let fx = app
            .handle_key(KeyEvent::new(KeyCode::Char('.'), KeyModifiers::NONE))
            .unwrap();

        assert!(sent_to_pane(&fx, '.'), "`.` went to the child: {fx:?}");
        assert!(app.view.chord_hint.is_none(), "and no menu opened");
    });
}

// ── `^a .` on the active tab ──

/// `^a .` opens the active tab's menu, titled with its label. A shell keeps
/// `^z` for its own job control, so its menu offers no suspend; any other
/// tab's does.
#[test]
fn ctrl_a_dot_opens_the_tab_menu_with_suspend_only_off_a_shell() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        for (cmd, suspends) in [("sh", false), ("sleep 30", true)] {
            let dir = tmp.path().join(cmd.replace(' ', "-"));
            std::fs::create_dir(&dir).unwrap();
            let mut app = app_with_tab(&dir, cmd);
            let label = tab_label(&app);

            ctrl_a(&mut app);
            press(&mut app, '.');

            assert!(app.view.chord_hint.is_some(), "{cmd}: `^a .` opened a menu");
            let menu = menu_lines(&mut app, &label);
            assert!(offers(&menu, "zoom pane"), "{cmd}: premise: {menu:#?}");
            assert_eq!(
                offers(&menu, "suspend / resume pane tab"),
                suspends,
                "{cmd}: {menu:#?}"
            );
        }
    });
}

/// A key that isn't the menu's closes it and goes where it would have gone
/// with no menu open: from the pane, `j` is typed into the child.
#[test]
fn a_key_the_menu_does_not_take_reaches_its_usual_target() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = app_with_tab(tmp.path(), "cat");
        ctrl_a(&mut app);
        press(&mut app, '.');
        assert!(app.view.chord_hint.is_some(), "premise: the menu is open");

        let fx = app
            .handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE))
            .unwrap();

        assert!(app.view.chord_hint.is_none(), "the menu closed");
        assert!(sent_to_pane(&fx, 'j'), "and `j` reached the child: {fx:?}");
    });
}

// ── right-click ──

/// A right-click on a file row puts the cursor there and opens that file's
/// menu, as `.` would on it.
#[test]
fn a_right_click_on_a_row_opens_that_entrys_menu() {
    let _lock = crate::mouse_test_lock();
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = super::tests::app_over_files(tmp.path(), 12);
        crate::set_mouse_capture_for_test(true);
        let row = drawn_at(&mut app, "file05.txt");

        right_click(&mut app, row);

        assert_eq!(app.state.cur().cursor.index, 5, "the cursor moved there");
        assert!(
            !app.state.resolver.is_pending(),
            "no chord is armed: this is the entry's menu, not the leader"
        );
        drawn_menu(&mut app, "file05.txt");
        crate::set_mouse_capture_for_test(false);
    });
}

/// Blank list space below the last entry is no object, so a right-click there
/// still opens the leader.
#[test]
fn a_right_click_below_the_last_entry_opens_the_leader() {
    let _lock = crate::mouse_test_lock();
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = super::tests::app_over_files(tmp.path(), 3);
        crate::set_mouse_capture_for_test(true);
        drawn(&mut app);
        let layout = app.frame_layout(FRAME);
        let blank = (layout.list.x + 1, layout.list.bottom() - 1);

        right_click(&mut app, blank);

        assert_eq!(
            app.state.resolver.pending_display().as_deref(),
            Some("leader-"),
            "the leader is armed"
        );
        crate::set_mouse_capture_for_test(false);
    });
}

/// A right-click on the pane body gives it the keyboard and opens the active
/// tab's menu, and a click on a row fires it: "zoom pane" zooms the pane.
#[test]
fn a_right_click_on_the_pane_opens_its_tab_menu_and_a_row_fires() {
    let _lock = crate::mouse_test_lock();
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = app_with_tab(tmp.path(), "cat");
        app.set_pane_focus(false);
        crate::set_mouse_capture_for_test(true);
        let label = tab_label(&app);
        let pane = app.frame_layout(FRAME).pane.expect("a pane is open");

        right_click(&mut app, centre(pane));

        assert!(app.state.pane_focused(), "the pane has the keyboard");
        let row = drawn_row(&mut app, &label, "zoom pane");
        click(&mut app, row);
        assert_eq!(
            app.state.active_project().pane.zoom,
            crate::app::state::ZoomTarget::BottomPane,
            "the row zoomed the pane"
        );
        assert!(app.view.chord_hint.is_none(), "and the menu closed");
        crate::set_mouse_capture_for_test(false);
    });
}

/// A right-click on a tab's label switches to that tab, as a left click does,
/// and opens its menu.
#[test]
fn a_right_click_on_a_tab_label_switches_to_it_and_opens_its_menu() {
    let _lock = crate::mouse_test_lock();
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = app_with_tab(tmp.path(), "cat");
        let first = tab_label(&app);
        app.open_pane_tab("sleep 30");
        app.set_pane_focus(false);
        crate::set_mouse_capture_for_test(true);
        let divider = app.frame_layout(FRAME).divider.expect("a divider");
        let widths = app.divider_tab_widths(divider.width);
        let on_first = (divider.x..divider.right())
            .find(|&col| {
                super::tab_hit::tab_at_point(Some(divider), &widths, col, divider.y) == Some(0)
            })
            .expect("the first tab's label is drawn");

        right_click(&mut app, (on_first, divider.y));

        let tabs = app.runtime.project().pane_tabs.as_ref().expect("tabs");
        assert_eq!(tabs.active_index(), 0, "the first tab is active");
        assert!(app.state.pane_focused(), "and has the keyboard");
        drawn_menu(&mut app, &first);
        crate::set_mouse_capture_for_test(false);
    });
}

/// A left click beside an open entry menu closes it and does nothing else,
/// as beside every menu.
#[test]
fn a_left_click_outside_an_entry_menu_closes_it_and_does_nothing_else() {
    let _lock = crate::mouse_test_lock();
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = super::tests::app_over_files(tmp.path(), 12);
        crate::set_mouse_capture_for_test(true);
        press(&mut app, '.');
        assert!(app.view.chord_hint.is_some(), "premise: the menu is open");
        let menu = drawn_menu(&mut app, "file00.txt");
        let layout = app.frame_layout(FRAME);
        let elsewhere = (layout.list.x + 1, layout.list.y + 3);
        assert!(!inside(menu, elsewhere), "premise: the click is beside it");

        click(&mut app, elsewhere);

        assert!(app.view.chord_hint.is_none(), "the menu is closed");
        assert_eq!(app.state.cur().cursor.index, 0, "the cursor stayed");
        assert!(app.view.mouse_selection.is_none(), "no row selection began");
        assert!(app.state.cur().picks.is_empty(), "and nothing was picked");
        crate::set_mouse_capture_for_test(false);
    });
}

/// The wheel beside an open entry menu would move the cursor off the entry the
/// menu names, and a row fired after that would act on another file. So the
/// wheel closes the menu, and moves nothing.
#[test]
fn the_wheel_beside_an_entry_menu_closes_it_rather_than_moving_the_cursor() {
    let _lock = crate::mouse_test_lock();
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = super::tests::app_over_files(tmp.path(), 12);
        crate::set_mouse_capture_for_test(true);
        app.state.active_project_mut().config.mouse.scroll_lines = 1;
        press(&mut app, '.');
        assert!(app.view.chord_hint.is_some(), "premise: the menu is open");
        let menu = drawn_menu(&mut app, "file00.txt");
        let layout = app.frame_layout(FRAME);
        let (col, row) = (layout.list.x + 1, layout.list.y + 3);
        assert!(!inside(menu, (col, row)), "premise: the wheel is beside it");

        app.handle_mouse(at(MouseEventKind::ScrollDown, col, row));

        assert!(app.view.chord_hint.is_none(), "the menu is closed");
        assert_eq!(app.state.cur().cursor.index, 0, "and the cursor stayed");
        crate::set_mouse_capture_for_test(false);
    });
}
