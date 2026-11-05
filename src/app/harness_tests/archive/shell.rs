//! A shell command typed inside a mounted archive. The `!` and `;` keys are
//! refused there, since a member's directory exists nowhere on disk for a
//! child to start in; the same command typed at the `:` prompt is refused the
//! same way, and outside the archive both still run in the column.

use super::*;

use crate::app::harness_tests::process_cwd::wait_until_in;

fn press(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::empty()))
        .unwrap();
}

/// `text` typed into a prompt that is already open, then `Enter`.
fn submit(app: &mut App, text: &str) {
    for c in text.chars() {
        press(app, KeyCode::Char(c));
    }
    press(app, KeyCode::Enter);
}

/// A canonical temp root holding `pkg.zip`, with the state dir isolated
/// inside it, and an app whose column is in `pkg.zip/src/`.
fn inside_a_mount(test: impl FnOnce(&Path, &mut App)) {
    let tmp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(tmp.path()).unwrap();
    crate::state::with_state_root(&root.join("state"), || {
        let archive = root.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(root.clone());
        mount_inline(&mut app, &archive, &root.join("staging"));
        // The cursor sits on `src/`, the first row; `Enter` descends into it.
        app.apply(&Action::EnterOrDisplay).unwrap();
        assert_eq!(app.state.cur().listing.dir, archive.join("src"));
        test(&root, &mut app);
    });
}

/// What `key` flashes here: the refusal its `:` form must match.
fn key_refusal(app: &mut App, key: char) -> String {
    press(app, KeyCode::Char(key));
    assert!(
        !matches!(app.state.mode, Mode::Prompting(_)),
        "the `{key}` key opened its prompt inside an archive"
    );
    let why = app.flash_text().unwrap_or_default().to_string();
    assert!(why.starts_with("archive: "), "`{key}` flashed {why:?}");
    app.state.flash = None;
    why
}

fn capture_started(app: &App) -> bool {
    app.runtime.pending_capture.is_some()
}

fn foreground_started(app: &App) -> bool {
    app.runtime.project().top_overlay.is_some()
}

/// The keys' refusal, which the `:` forms match.
#[test]
fn the_shell_keys_are_refused_inside_an_archive() {
    inside_a_mount(|_, app| {
        for key in ['!', ';'] {
            key_refusal(app, key);
        }
        assert!(!capture_started(app) && !foreground_started(app));
    });
}

/// `:!cmd` inside an archive is refused as `!` is, and starts nothing.
#[test]
#[ignore = "red: `:!cmd` bypasses the refusal and spawns in the member's path, which falls back to $HOME"]
fn a_captured_command_at_the_colon_prompt_is_refused_inside_an_archive() {
    inside_a_mount(|_, app| {
        let why = key_refusal(app, '!');

        press(app, KeyCode::Char(':'));
        submit(app, "!sleep 30");

        assert!(!capture_started(app), "`:!` started a capture");
        assert_eq!(app.flash_text(), Some(why.as_str()));
    });
}

/// `:;cmd` inside an archive is refused as `;` is, and starts nothing.
#[test]
#[ignore = "red: `:;cmd` bypasses the refusal and spawns in the member's path, which falls back to $HOME"]
fn a_foreground_command_at_the_colon_prompt_is_refused_inside_an_archive() {
    inside_a_mount(|_, app| {
        let why = key_refusal(app, ';');

        press(app, KeyCode::Char(':'));
        submit(app, ";sleep 30");

        assert!(!foreground_started(app), "`:;` started a command");
        assert_eq!(app.flash_text(), Some(why.as_str()));
    });
}

/// `:!!` repeats the last `!` command, typed outside the archive; inside it
/// that is refused too.
#[test]
#[ignore = "red: `:!!` bypasses the refusal and spawns in the member's path, which falls back to $HOME"]
fn repeating_the_last_captured_command_is_refused_inside_an_archive() {
    inside_a_mount(|_, app| {
        let why = key_refusal(app, '!');
        app.state.last_captured_cmd = Some("sleep 30".to_string());

        press(app, KeyCode::Char(':'));
        submit(app, "!!");

        assert!(!capture_started(app), "`:!!` started a capture");
        assert_eq!(app.flash_text(), Some(why.as_str()));
    });
}

/// Climbed back out to the directory holding `pkg.zip`, still mounted.
fn beside_a_mount(test: impl FnOnce(&Path, &mut App)) {
    inside_a_mount(|root, app| {
        for _ in 0..2 {
            let fx = app.apply(&Action::Climb).unwrap();
            apply_effects(app, fx);
        }
        assert_eq!(app.state.cur().listing.dir, root);
        assert!(
            app.state.mounts.contains(&root.join("pkg.zip")),
            "the archive stays mounted"
        );
        test(root, app);
    });
}

/// Beside a mounted archive, `:!` runs in the column as it does anywhere.
#[test]
fn a_captured_command_at_the_colon_prompt_runs_beside_a_mounted_archive() {
    beside_a_mount(|root, app| {
        press(app, KeyCode::Char(':'));
        submit(app, "!sleep 30");

        let capture = app
            .runtime
            .pending_capture
            .as_ref()
            .unwrap_or_else(|| panic!("`:!` started nothing: {:?}", app.flash_text()));
        wait_until_in(capture.host.process_id().expect("the capture runs"), root);
    });
}

/// Beside a mounted archive, `:;` runs in the column as it does anywhere.
#[test]
fn a_foreground_command_at_the_colon_prompt_runs_beside_a_mounted_archive() {
    beside_a_mount(|root, app| {
        press(app, KeyCode::Char(':'));
        submit(app, ";sleep 30");

        let overlay = app
            .runtime
            .project()
            .top_overlay
            .as_ref()
            .unwrap_or_else(|| panic!("`:;` started nothing: {:?}", app.flash_text()));
        wait_until_in(overlay.process_id().expect("the command runs"), root);
    });
}
