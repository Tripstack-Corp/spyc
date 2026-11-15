//! The kitty keyboard protocol, end to end through a real pane (#1176): a
//! program that pushes the flags gets Shift+Enter as `CSI 13;2u`, where the
//! legacy encoding spyc sends everyone else would give it a newline; one that
//! asks for key releases (flag 2) gets them, and no other program does.

use super::*;
use crate::app::effect::{PaneInput, PaneTarget};
use crossterm::event::KeyEventKind;
use std::time::{Duration, Instant};

fn contents(app: &App) -> String {
    app.runtime.project().pane_tabs.as_ref().unwrap().tabs()[0]
        .pane
        .with_screen(crate::pane::engine::TerminalScreen::contents)
}

fn wait_for(app: &mut App, needle: &str) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        app.drain_pane_output();
        if contents(app).contains(needle) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "never saw {needle:?}: {:?}",
            contents(app)
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn press(app: &mut App, code: KeyCode, mods: KeyModifiers) {
    app.execute_pane_input(
        PaneTarget::Active,
        PaneInput::Key(KeyEvent::new(code, mods)),
        None,
        None,
    );
}

fn release(app: &mut App, code: KeyCode, mods: KeyModifiers) {
    app.execute_pane_input(
        PaneTarget::Active,
        PaneInput::KeyRelease(KeyEvent::new_with_kind(code, mods, KeyEventKind::Release)),
        None,
        None,
    );
}

/// A pane running `script`, with its first line of output seen.
fn pane_running(dir: &std::path::Path, script: &str) -> App {
    let path = dir.join("kitty.sh");
    std::fs::write(&path, script).unwrap();
    let mut app = App::test_app(dir.to_path_buf());
    assert!(app.open_pane_tab_in(&format!("sh {}", path.display()), dir));
    wait_for(&mut app, "ready");
    app
}

#[test]
fn a_program_that_pushes_the_flags_gets_kitty_keys() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let script = dir.join("kitty.sh");
        // `exec` keeps the pushing process the pane's foreground group.
        std::fs::write(&script, "printf '\\033[>1uready\\n'\nexec cat -v\n").unwrap();
        let mut app = App::test_app(dir.clone());
        assert!(app.open_pane_tab_in(&format!("sh {}", script.display()), &dir));
        wait_for(&mut app, "ready");
        press(&mut app, KeyCode::Enter, KeyModifiers::SHIFT);
        // The terminal's echo shows the escape as `^[[13;2u`.
        wait_for(&mut app, "[13;2u");
    });
}

/// Flag 2: a held key comes as repeats, and letting it go as a release.
#[test]
fn a_program_that_asks_for_key_releases_gets_them() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let mut app = pane_running(&dir, "printf '\\033[>3uready\\n'\nexec cat -v\n");
        assert!(app.key_releases_wanted(), "the program asked");
        release(&mut app, KeyCode::Char('a'), KeyModifiers::NONE);
        wait_for(&mut app, "[97;1:3u");
        app.execute_pane_input(
            PaneTarget::Active,
            PaneInput::Key(KeyEvent::new_with_kind(
                KeyCode::Char('i'),
                KeyModifiers::CONTROL,
                KeyEventKind::Repeat,
            )),
            None,
            None,
        );
        wait_for(&mut app, "[105;5:2u");
    });
}

/// A program that never asked for releases (none of the protocol, or only
/// flag 1) gets none: a legacy release would type the key a second time.
#[test]
fn a_release_reaches_no_program_that_did_not_ask() {
    for push in ["", "\\033[>1u"] {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(&tmp.path().join("state"), || {
            let dir = std::fs::canonicalize(tmp.path()).unwrap();
            let mut app = pane_running(&dir, &format!("printf '{push}ready\\n'\nexec cat -v\n"));
            assert!(!app.key_releases_wanted(), "{push:?}");
            release(&mut app, KeyCode::Char('x'), KeyModifiers::NONE);
            press(&mut app, KeyCode::Char('y'), KeyModifiers::NONE);
            wait_for(&mut app, "y");
            let screen = contents(&app);
            assert!(
                !screen.contains('x') && !screen.contains("1:3u"),
                "{push:?}: {screen:?}"
            );
        });
    }
}

/// A program that pops its flags stops wanting releases.
#[test]
fn releases_stop_when_the_program_pops_its_flags() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let mut app = pane_running(
            &dir,
            "printf '\\033[>3uready\\n'\nread x\nprintf '\\033[<upopped\\n'\nexec cat\n",
        );
        assert!(app.key_releases_wanted());
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        wait_for(&mut app, "popped");
        assert!(!app.key_releases_wanted());
    });
}

/// A release is never typing: Enter let go doesn't send the prompt.
#[test]
fn a_release_never_counts_as_typing() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let mut app = pane_running(&dir, "printf '\\033[>3uready\\n'\nexec cat\n");
        press(&mut app, KeyCode::Char('h'), KeyModifiers::NONE);
        press(&mut app, KeyCode::Char('i'), KeyModifiers::NONE);
        release(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        let pane = &app.state.active_project().pane;
        assert_eq!(pane.pane_prompt_buf, "hi");
        assert_eq!(pane.last_pane_prompt, None);
    });
}

/// A release goes where its press would have: to the pane in front, and
/// nowhere when spyc would have taken the press itself.
#[test]
fn a_release_goes_where_its_press_would() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        // Not a shell, so `^z` is spyc's.
        let mut app = App::test_app(dir.clone());
        assert!(app.open_pane_tab_in("cat", &dir));
        let up = |code, mods| KeyEvent::new_with_kind(code, mods, KeyEventKind::Release);
        let a = up(KeyCode::Char('a'), KeyModifiers::NONE);

        app.state.active_project_mut().focus = state::Focus::Pane;
        let fx = app.handle_key_release(a);
        assert!(
            matches!(
                fx.as_slice(),
                [Effect::SendToPane {
                    target: PaneTarget::Active,
                    input: PaneInput::KeyRelease(k),
                    ..
                }] if *k == a
            ),
            "{fx:?}"
        );
        let ctrl_a = up(KeyCode::Char('a'), KeyModifiers::CONTROL);
        assert!(
            app.handle_key_release(ctrl_a).is_empty(),
            "spyc's own prefix"
        );
        let ctrl_z = up(KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert!(app.handle_key_release(ctrl_z).is_empty(), "spyc's ^z");
        // `^a a` just focused the pane: the `a` let go ends spyc's chord.
        app.view.focus_chord_completed = Some((Instant::now(), KeyCode::Char('a')));
        assert!(app.handle_key_release(a).is_empty(), "the chord's last key");
        app.view.focus_chord_completed = None;

        app.state.active_project_mut().focus = state::Focus::FileList;
        assert!(app.handle_key_release(a).is_empty(), "the file list's key");

        app.state.active_project_mut().focus = state::Focus::Pane;
        app.runtime
            .project_mut()
            .pane_tabs
            .as_mut()
            .unwrap()
            .tabs_mut()[0]
            .info
            .suspended = true;
        assert!(app.handle_key_release(a).is_empty(), "a stopped program");
    });
}
