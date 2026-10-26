use super::*;

use crate::fs::listing::SortMode;
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
    app.view.pending_history_pick.is_some() && app.view.project().pager.is_some()
}

fn selected(app: &App) -> usize {
    app.view
        .project()
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
        .project()
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

#[test]
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

// ── Which history `Esc Space` opens, and the `:` history popup (#82) ──
// From a prompt's line editor in Normal mode, `Space` (or `?`) opens that
// prompt's history: the shell's from `!` and `;`, the jump history from `J`,
// and `:`'s own from `:`, in the editor above, where Enter runs the chosen
// line as a `:` command.

/// What `Esc Space` and `Esc ?` open from each prompt with a line editor but
/// `:`: the shell's history from `!` and `;`, the jump history from `J`, and
/// nothing from the rest, which stay open.
#[test]
fn esc_space_opens_each_prompts_own_history_or_none() {
    use PromptKind as K;
    let cases = || -> Vec<(&str, PromptKind, Option<&str>)> {
        vec![
            ("!", K::ShellCmdCaptured, Some("history")),
            (";", K::ShellCmd, Some("history")),
            ("J", K::Jump, Some("jump history")),
            ("pane command", K::PaneNewTabCmd, None),
            ("pane cwd", K::PaneNewTabCwd, None),
            ("tab name", K::PaneRenameTab, None),
            ("open project", K::ProjectOpen, None),
            ("worktree branch", K::WorktreeNewBranch, None),
            ("copy to", K::CopyTo, None),
            ("move to", K::MoveTo, None),
            ("mkdir", K::MakeDir, None),
        ]
    };
    for key in [' ', '?'] {
        for (label, kind, opens) in cases() {
            let mut app = App::test_app(std::env::temp_dir());
            for history in [
                &mut app.state.history,
                &mut app.state.jump_history,
                &mut app.state.command_history,
                &mut app.state.pane_history,
                &mut app.state.pane_cwd_history,
                &mut app.state.project_history,
            ] {
                history.push("/an/entry");
            }
            app.state.mode = Mode::Prompting(Prompt::shell(kind, "> "));
            press(&mut app, KeyCode::Esc);
            press(&mut app, KeyCode::Char(key));
            let title = app.view.project().pager.as_ref().map(|v| v.title.clone());
            assert_eq!(
                title.as_deref().and_then(|t| t.split(" — ").next()),
                opens,
                "`Esc {key}` in the {label} prompt"
            );
            if opens.is_none() {
                assert!(
                    matches!(app.state.mode, Mode::Prompting(_)),
                    "`Esc {key}` closed the {label} prompt"
                );
            }
        }
    }
}

/// An `App` that has run `:sort size` and then `:sort mtime` through the `:`
/// prompt, beside a shell history of its own.
fn after_two_commands(test: impl FnOnce(&mut App)) {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().join("w");
        std::fs::create_dir(&dir).unwrap();
        let mut app = App::test_app(dir);
        app.state.history.push("echo shell");
        for command in ["sort size", "sort mtime"] {
            press(&mut app, KeyCode::Char(':'));
            type_str(&mut app, command);
            press(&mut app, KeyCode::Enter);
        }
        assert_eq!(app.state.cur().sort_order, SortMode::Mtime, "both ran");
        test(&mut app);
    });
}

/// `:`, then `Esc` for the line editor's Normal mode, then `key`.
fn from_the_command_prompt(app: &mut App, key: char) {
    press(app, KeyCode::Char(':'));
    press(app, KeyCode::Esc);
    press(app, KeyCode::Char(key));
}

fn command_history(app: &App) -> Vec<String> {
    app.state.command_history.entries().to_vec()
}

#[test]
#[ignore = "red: the `:` prompt opens no history popup (#82)"]
fn esc_space_in_the_command_prompt_lists_the_commands_run() {
    after_two_commands(|app| {
        for key in [' ', '?'] {
            from_the_command_prompt(app, key);
            assert!(popup_open(app), "`Esc {key}` opened no popup");
            let title = app.view.project().pager.as_ref().map(|v| v.title.clone());
            assert!(
                title.as_deref().is_some_and(|t| t.starts_with(": history")),
                "`Esc {key}`: {title:?}"
            );
            assert_eq!(
                rows(app),
                ["    1  sort mtime", "    2  sort size"],
                "`Esc {key}` lists the `:` history, not the shell's"
            );
            assert_eq!((selected(app), editing(app)), (0, "sort mtime".into()));
            press(app, KeyCode::Esc);
            assert!(!popup_open(app), "Esc closes it");
        }
    });
}

#[test]
#[ignore = "red: the `:` prompt opens no history popup (#82)"]
fn enter_runs_the_chosen_entry_as_a_command() {
    after_two_commands(|app| {
        from_the_command_prompt(app, ' ');
        press(app, KeyCode::Char('j'));
        assert_eq!(editing(app), "sort size");
        press(app, KeyCode::Enter);
        assert!(!popup_open(app), "running closes the popup");
        assert_eq!(
            app.state.cur().sort_order,
            SortMode::Size,
            "`:sort size` ran"
        );
        assert!(
            app.runtime.pending_capture.is_none(),
            "no shell command ran"
        );
        assert_eq!(app.state.last_captured_cmd, None);
        assert_eq!(
            history(app),
            ["echo shell"],
            "the shell history is untouched"
        );
        assert_eq!(
            command_history(app),
            ["sort mtime", "sort size"],
            "the `:` history remembers it"
        );
    });
}

#[test]
#[ignore = "red: the `:` prompt opens no history popup (#82)"]
fn an_edited_entry_runs_as_edited() {
    after_two_commands(|app| {
        from_the_command_prompt(app, ' ');
        press(app, KeyCode::Char('A'));
        for _ in "mtime".chars() {
            press(app, KeyCode::Backspace);
        }
        type_str(app, "ext");
        press(app, KeyCode::Enter);
        assert_eq!(app.state.cur().sort_order, SortMode::Ext, "`:sort ext` ran");
        assert_eq!(
            command_history(app),
            ["sort size", "sort mtime", "sort ext"]
        );
    });
}

#[test]
#[ignore = "red: the `:` prompt opens no history popup (#82)"]
fn ctrl_d_deletes_from_the_command_history_only() {
    after_two_commands(|app| {
        from_the_command_prompt(app, ' ');
        ctrl(app, 'd');
        assert_eq!(command_history(app), ["sort size"]);
        assert_eq!(
            history(app),
            ["echo shell"],
            "the shell history is untouched"
        );
        assert_eq!(rows(app), ["    1  sort size"]);
        assert_eq!(editing(app), "sort size");
        ctrl(app, 'd');
        assert!(command_history(app).is_empty());
        assert!(!popup_open(app), "deleting the last closes the popup");
        assert_eq!(history(app), ["echo shell"]);
    });
}

/// A chosen command's effects reach the loop as a typed one's do: Enter on
/// `cd /an/elsewhere` hands back that change of directory.
#[test]
#[ignore = "red: the `:` prompt opens no history popup (#82)"]
fn a_chosen_commands_effects_reach_the_loop() {
    after_two_commands(|app| {
        press(app, KeyCode::Char(':'));
        type_str(app, "cd /an/elsewhere");
        press(app, KeyCode::Enter);
        from_the_command_prompt(app, ' ');
        assert_eq!(editing(app), "cd /an/elsewhere");
        let effects = app
            .handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()))
            .unwrap();
        let cd = effects.change_dir().expect("one change of directory");
        assert_eq!(cd.path(), std::path::Path::new("/an/elsewhere"));
    });
}

/// Every way of moving the selection loads the `:` command it lands on.
#[test]
#[ignore = "red: the `:` prompt opens no history popup (#82)"]
fn moving_through_the_command_popup_loads_its_commands() {
    after_two_commands(|app| {
        from_the_command_prompt(app, ' ');
        press(app, KeyCode::Char('/'));
        type_str(app, "size");
        press(app, KeyCode::Enter);
        assert_eq!(
            (selected(app), editing(app)),
            (1, "sort size".into()),
            "a search"
        );
        press(app, KeyCode::Char(':'));
        type_str(app, "1");
        press(app, KeyCode::Enter);
        assert_eq!(
            (selected(app), editing(app)),
            (0, "sort mtime".into()),
            "`:1`"
        );
        press(app, KeyCode::Char('n'));
        assert_eq!(
            (selected(app), editing(app)),
            (1, "sort size".into()),
            "`n`"
        );
        press(app, KeyCode::Char('i'));
        press(app, KeyCode::Up);
        assert_eq!(
            (selected(app), editing(app)),
            (0, "sort mtime".into()),
            "Up while typing"
        );
    });
}
