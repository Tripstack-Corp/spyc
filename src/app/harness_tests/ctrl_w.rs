//! `^w` and the pane's program (#907). `^w` is the pane prefix, `^a`'s alias,
//! so a shell or an agent in the pane never got its delete-word key: `^w ^w`
//! and `^w ↓` send it, as `^a ↓` sends `^a`, and `[pane] ctrl_w_prefix = false`
//! hands a focused pane's `^w` to its program. Each case presses its keys
//! through `handle_key` and reads what the pane's program received.

use super::*;
use crate::app::effect::PaneTarget;
use std::path::Path;
use std::time::{Duration, Instant};

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn special(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn tabs(app: &mut App) -> &mut crate::pane::PaneTabs {
    app.runtime
        .project_mut()
        .pane_tabs
        .as_mut()
        .expect("a pane tab")
}

fn active_tab(app: &mut App) -> usize {
    tabs(app).active_index()
}

/// Load `toml` as the user's config, through the config loader, and put it in
/// force in the active project.
fn with_config(app: &mut App, dir: &Path, toml: &str) {
    let rc = dir.join("spycrc.toml");
    std::fs::write(&rc, toml).unwrap();
    app.state.active_project_mut().config =
        crate::config::Config::load_from(&[Some(&rc)]).expect("the config loads");
}

/// Press `keys` through `handle_key`, delivering each one's pane input to the
/// active tab the way the executor does.
fn press(app: &mut App, keys: &[KeyEvent]) {
    for key in keys {
        let fx = app.handle_key(*key).unwrap();
        for effect in fx {
            if let Effect::SendToPane {
                target: PaneTarget::Active,
                input,
                on_ok,
                err_prefix,
            } = effect
            {
                app.execute_pane_input(PaneTarget::Active, input, on_ok, err_prefix);
            }
        }
    }
}

/// A spyc with a pane tab, the keyboard on it, whose program writes every byte
/// it receives to `dir/got`, the terminal's line editing off.
fn raw_reader(dir: &Path) -> App {
    let script = dir.join("reader.sh");
    std::fs::write(
        &script,
        format!(
            "stty raw -echo\nprintf READER_READY\nexec cat > '{}'\n",
            dir.join("got").display()
        ),
    )
    .unwrap();
    let mut app = App::test_app(dir.to_path_buf());
    assert!(app.open_pane_tab_in(&format!("/bin/sh '{}'", script.display()), dir));
    app.set_pane_focus(true);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let pane = tabs(&mut app).active_mut();
        pane.drain_output();
        if pane
            .visible_lines()
            .iter()
            .any(|l| l.contains("READER_READY"))
        {
            return app;
        }
        assert!(Instant::now() < deadline, "the reader never started");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The bytes the [`raw_reader`] received for `keys`. A `#` written to the tab
/// past spyc marks where they end.
fn sent(app: &mut App, dir: &Path, keys: &[KeyEvent]) -> Vec<u8> {
    let got = dir.join("got");
    let before = std::fs::read(&got).unwrap_or_default();
    press(app, keys);
    tabs(app).active_mut().send_bytes(b"#").expect("the marker");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let bytes = std::fs::read(&got).unwrap_or_default();
        if bytes.len() > before.len() && bytes.ends_with(b"#") {
            return bytes[before.len()..bytes.len() - 1].to_vec();
        }
        assert!(Instant::now() < deadline, "the marker never arrived");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// `↓` after a prefix sends the prefix that armed it, and `^w ^w` sends one
/// `^w`: the program gets `^w` (`0x17`) and `^a` (`0x01`) as typed.
#[test]
#[ignore = "red: `^w ↓` sends `^a`, and `^w ^w` sends nothing"]
fn each_prefix_sends_itself_to_the_program() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path();
        let mut app = raw_reader(dir);
        assert_eq!(
            sent(&mut app, dir, &[ctrl('w'), ctrl('w')]),
            [0x17],
            "^w ^w"
        );
        let down = special(KeyCode::Down);
        assert_eq!(sent(&mut app, dir, &[ctrl('w'), down]), [0x17], "^w ↓");
        assert_eq!(sent(&mut app, dir, &[ctrl('a'), down]), [0x01], "^a ↓");
    });
}

/// `^w ^w` reaches a line editor as its delete-word key. The editor here is
/// the terminal's own, which `cat` reads through, and it erases the word
/// before the cursor as a shell's does.
#[test]
#[ignore = "red: `^w ^w` sends nothing"]
fn ctrl_w_ctrl_w_deletes_the_word_before_the_cursor() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let got = dir.join("got.txt");
        let mut app = App::test_app(dir.clone());
        let cmd = format!(
            "cat > {}",
            crate::shell::shell_quote(&got.to_string_lossy())
        );
        assert!(app.open_pane_tab_in(&cmd, &dir));
        app.set_pane_focus(true);
        let typed: Vec<KeyEvent> = "one two".chars().map(key).collect();
        press(&mut app, &typed);
        press(&mut app, &[ctrl('w'), ctrl('w')]);
        press(&mut app, &[special(KeyCode::Enter)]);
        assert_eq!(received(&mut app, &got), "one \n");
    });
}

/// `[pane] ctrl_w_prefix = false` makes `^w` an ordinary key while the pane has
/// the keyboard, so the program gets it as typed. Set `true`, `^w` is the
/// prefix it always was.
#[test]
#[ignore = "red: `[pane] ctrl_w_prefix` is no setting, and `^w` always arms the prefix"]
fn with_ctrl_w_prefix_off_ctrl_w_reaches_the_program() {
    for (on, want) in [(false, vec![0x17]), (true, vec![])] {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let dir = tmp.path();
            let mut app = raw_reader(dir);
            with_config(&mut app, dir, &format!("[pane]\nctrl_w_prefix = {on}\n"));
            assert_eq!(
                sent(&mut app, dir, &[ctrl('w')]),
                want,
                "ctrl_w_prefix = {on}"
            );
            assert_eq!(app.state.resolver.is_pending(), on, "ctrl_w_prefix = {on}");
        });
    }
}

/// `^a` is the pane prefix as it was: `^a ^a` goes to the last tab, `^a ↓`
/// sends `^a`, and `^a k` leaves the pane. From the file list `^w` arms the
/// prefix, since no program there could receive it. `config` is the user's
/// `.spycrc.toml`.
fn ctrl_a_chords_work_as_before(config: &str) {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path();
        let mut app = raw_reader(dir);
        assert!(app.open_pane_tab_in("cat", dir));
        app.set_pane_focus(true);
        with_config(&mut app, dir, config);
        assert_eq!(active_tab(&mut app), 1, "premise: the second tab is active");

        press(&mut app, &[ctrl('a'), ctrl('a')]);
        assert_eq!(active_tab(&mut app), 0, "^a ^a");
        let down = special(KeyCode::Down);
        assert_eq!(sent(&mut app, dir, &[ctrl('a'), down]), [0x01], "^a ↓");
        press(&mut app, &[ctrl('a'), key('k')]);
        assert!(!app.state.pane_focused(), "^a k");

        press(&mut app, &[ctrl('w')]);
        assert!(app.state.resolver.is_pending(), "^w from the file list");
        press(&mut app, &[key('j')]);
        assert!(app.state.pane_focused(), "^w j");
    });
}

#[test]
fn ctrl_a_chords_work_as_before_by_default() {
    ctrl_a_chords_work_as_before("");
}

/// With `[pane] ctrl_w_prefix = false`, every `^a` chord works as before.
#[test]
#[ignore = "red: `[pane] ctrl_w_prefix` is no setting"]
fn with_ctrl_w_prefix_off_ctrl_a_chords_work_as_before() {
    ctrl_a_chords_work_as_before("[pane]\nctrl_w_prefix = false\n");
}

/// The which-key popup names the prefix that armed it, and the key that sends
/// that prefix: `^w`'s sends a literal `^w` on `↓` or `^w`, `^a`'s a literal
/// `^a` on `↓`.
#[test]
#[ignore = "red: `^w`'s popup is titled `^a`, and its `↓` row sends `^a`"]
fn the_which_key_popup_names_what_each_prefix_sends() {
    let cases = [
        (ctrl('w'), "^w", ("↓ ^w", "send literal ^w to pane")),
        (ctrl('a'), "^a", ("↓", "send literal ^a to pane")),
    ];
    for (prefix, title, (keys, label)) in cases {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            assert!(app.handle_key(prefix).unwrap().is_empty());
            app.settle_chord_hint(
                Instant::now() + Duration::from_secs(60),
                &mut RunCtx::for_test(),
            );
            let hint = app.view.chord_hint.as_ref().expect("the popup is up");
            assert_eq!(hint.title, title);
            assert!(
                hint.rows.contains(&(keys.to_string(), label.to_string())),
                "{title}: {:?}",
                hint.rows
            );
        });
    }
}
