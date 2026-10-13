use super::*;

use crossterm::event::KeyCode;

// ── The `!?` history editor (#176) ──
// The popup lists past `!` commands newest first, with the selected one loaded
// into a vi line editor. These drive it through `handle_key` and check what the
// user sees: which row is selected, what the editor holds, what runs.

/// An `App` showing the history popup over `commands`, oldest first.
fn editing_history(commands: &[&str], test: impl FnOnce(&mut App)) {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().join("w");
        std::fs::create_dir(&dir).unwrap();
        let mut app = App::test_app(dir);
        for command in commands {
            app.state.history.push(command);
        }
        app.show_history_popup();
        assert!(popup_open(&app), "the popup opened");
        test(&mut app);
    });
}

fn press(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::empty()))
        .unwrap();
}

fn type_str(app: &mut App, text: &str) {
    for c in text.chars() {
        press(app, KeyCode::Char(c));
    }
}

fn ctrl(app: &mut App, c: char) {
    app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))
        .unwrap();
}

fn popup_open(app: &App) -> bool {
    app.view.pending_history_pick.is_some() && app.view.pager.is_some()
}

fn selected(app: &App) -> usize {
    app.view
        .pager
        .as_ref()
        .and_then(|v| v.picker_cursor)
        .expect("a selected row")
}

/// What the editor holds — the command Enter would run.
fn editing(app: &App) -> String {
    app.view
        .pending_history_pick
        .as_ref()
        .expect("the editor")
        .text()
}

fn rows(app: &App) -> Vec<String> {
    app.view
        .pager
        .as_ref()
        .expect("the popup")
        .lines
        .iter()
        .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
        .collect()
}

fn history(app: &App) -> Vec<String> {
    app.state.history.entries().to_vec()
}

const THREE: [&str; 3] = ["echo one", "echo two", "echo three"];

#[test]
fn the_popup_lists_newest_first_with_the_newest_loaded() {
    editing_history(&THREE, |app| {
        assert_eq!(
            rows(app),
            ["    1  echo three", "    2  echo two", "    3  echo one"]
        );
        assert_eq!((selected(app), editing(app)), (0, "echo three".into()));
    });
}

#[test]
fn moving_the_selection_loads_that_command() {
    editing_history(&THREE, |app| {
        press(app, KeyCode::Char('j'));
        assert_eq!((selected(app), editing(app)), (1, "echo two".into()));
        press(app, KeyCode::Down);
        assert_eq!((selected(app), editing(app)), (2, "echo one".into()));
        press(app, KeyCode::Char('k'));
        assert_eq!((selected(app), editing(app)), (1, "echo two".into()));
        press(app, KeyCode::Up);
        press(app, KeyCode::Up);
        assert_eq!(
            (selected(app), editing(app)),
            (0, "echo three".into()),
            "the top row stays the top row"
        );
    });
}

#[test]
fn capital_g_and_g_g_jump_to_the_ends() {
    editing_history(&THREE, |app| {
        press(app, KeyCode::Char('G'));
        assert_eq!((selected(app), editing(app)), (2, "echo one".into()));
        type_str(app, "gg");
        assert_eq!((selected(app), editing(app)), (0, "echo three".into()));
        // A lone `g` waits for its second: anything else cancels it.
        type_str(app, "gjg");
        assert_eq!(selected(app), 1, "`g j g` is a move down, not `gg`");
    });
}

#[test]
fn ctrl_d_deletes_the_selected_command_from_history() {
    editing_history(&THREE, |app| {
        press(app, KeyCode::Char('j'));
        ctrl(app, 'd');
        assert_eq!(history(app), ["echo one", "echo three"]);
        assert_eq!(rows(app), ["    1  echo three", "    2  echo one"]);
        assert_eq!(
            (selected(app), editing(app)),
            (1, "echo one".into()),
            "the next command takes the deleted one's place"
        );

        press(app, KeyCode::Char('i'));
        ctrl(app, 'd');
        assert_eq!(history(app), ["echo three"], "while editing too");
    });
}

#[test]
fn deleting_the_last_command_closes_the_popup() {
    editing_history(&["only"], |app| {
        ctrl(app, 'd');
        assert!(history(app).is_empty());
        assert!(!popup_open(app));
        assert_eq!(app.flash_text(), Some("history is empty"));
    });
}

#[test]
fn an_edited_command_runs_and_is_remembered() {
    editing_history(&THREE, |app| {
        press(app, KeyCode::Char('j'));
        press(app, KeyCode::Char('A'));
        type_str(app, " again");
        assert_eq!(
            rows(app)[1],
            "    2  echo two again",
            "the row shows the edit"
        );
        press(app, KeyCode::Enter);
        assert!(!popup_open(app), "running closes the popup");
        assert!(app.runtime.pending_capture.is_some(), "the command runs");
        assert_eq!(
            app.state.last_captured_cmd.as_deref(),
            Some("echo two again")
        );
        assert_eq!(
            history(app).last().map(String::as_str),
            Some("echo two again")
        );
    });
}

#[test]
fn enter_on_an_emptied_line_closes_without_running() {
    editing_history(&["ls"], |app| {
        press(app, KeyCode::Char('A'));
        press(app, KeyCode::Backspace);
        press(app, KeyCode::Backspace);
        press(app, KeyCode::Enter);
        assert!(!popup_open(app));
        assert!(app.runtime.pending_capture.is_none(), "nothing ran");
        assert_eq!(history(app), ["ls"]);
    });
}

#[test]
fn esc_leaves_insert_mode_first_then_closes() {
    editing_history(&THREE, |app| {
        press(app, KeyCode::Char('i'));
        press(app, KeyCode::Esc);
        assert!(popup_open(app), "the first Esc only leaves Insert mode");
        press(app, KeyCode::Esc);
        assert!(!popup_open(app));
        assert!(app.runtime.pending_capture.is_none(), "nothing ran");
    });
}

#[test]
fn up_and_down_while_typing_move_between_commands() {
    editing_history(&THREE, |app| {
        press(app, KeyCode::Char('i'));
        press(app, KeyCode::Down);
        assert_eq!((selected(app), editing(app)), (1, "echo two".into()));
        press(app, KeyCode::Up);
        assert_eq!((selected(app), editing(app)), (0, "echo three".into()));
    });
}

#[test]
fn a_search_selects_each_match_in_turn() {
    editing_history(
        &["echo one", "echo two", "echo one more", "echo three"],
        |app| {
            // Rows: 1 `echo three`, 2 `echo one more`, 3 `echo two`, 4 `echo one`.
            press(app, KeyCode::Char('/'));
            type_str(app, "one");
            press(app, KeyCode::Enter);
            assert_eq!((selected(app), editing(app)), (1, "echo one more".into()));
            press(app, KeyCode::Char('n'));
            assert_eq!((selected(app), editing(app)), (3, "echo one".into()));
            press(app, KeyCode::Char('N'));
            assert_eq!((selected(app), editing(app)), (1, "echo one more".into()));
        },
    );
}

#[test]
fn colon_n_selects_command_n() {
    editing_history(&THREE, |app| {
        press(app, KeyCode::Char(':'));
        type_str(app, "3");
        press(app, KeyCode::Enter);
        assert_eq!((selected(app), editing(app)), (2, "echo one".into()));
    });
}

// Found while writing the tests above; the fix un-ignores it.

#[test]
#[ignore = "bug: `l` is swallowed before the line editor sees it, so it can't move right"]
fn h_and_l_move_the_editors_cursor() {
    editing_history(&THREE, |app| {
        let cursor = |app: &App| app.view.pending_history_pick.as_ref().unwrap().cursor;
        press(app, KeyCode::Char('0'));
        assert_eq!(cursor(app), 0);
        press(app, KeyCode::Char('l'));
        assert_eq!(cursor(app), 1, "`l` moves right");
        press(app, KeyCode::Char('h'));
        assert_eq!(cursor(app), 0, "`h` moves left");
    });
}
