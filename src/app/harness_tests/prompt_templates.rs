//! Prompt templates against a real pane child: what the agent receives when a
//! `prompt` binding fires.

use super::*;
use crate::keymap::user::BoundAction;

/// A pane child in `cwd` that writes the first line it's given to `got`.
fn cat_pane(app: &mut App, cwd: &std::path::Path, got: &std::path::Path) {
    let cmd = format!(
        "cat > {}",
        crate::shell::shell_quote(&got.to_string_lossy())
    );
    assert!(app.open_pane_tab_in(&cmd, cwd));
    app.set_pane_focus(false);
}

/// The template reaches the pane with the picks filled in, relative to the
/// pane's own cwd the way `^a s` sends them, and the pane has the keyboard so
/// Enter sends it.
#[test]
fn a_prompt_binding_types_the_template_into_the_pane() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        let wt = root.join("wt");
        std::fs::create_dir_all(wt.join("sub")).unwrap();
        let picked = wt.join("sub/f.txt");
        std::fs::write(&picked, "x").unwrap();
        let got = root.join("got.txt");

        let mut app = App::test_app(root.clone());
        app.state
            .config
            .prompts
            .insert("review".into(), "review % from %d, 100%% please".into());
        cat_pane(&mut app, &wt, &got);
        app.state.left.picks.insert(&picked);

        let effects = app
            .apply_user(&BoundAction::Prompt("review".into()))
            .unwrap();
        assert!(app.state.pane_focused(), "Enter goes to the pane");
        deliver(&mut app, effects);

        let root_shown = root.to_string_lossy();
        assert_eq!(
            received(&mut app, &got),
            format!("review sub/f.txt from {root_shown}, 100% please\n")
        );
    });
}

#[test]
fn an_unknown_prompt_types_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        let mut app = App::test_app(root.clone());
        cat_pane(&mut app, &root, &root.join("got.txt"));

        let effects = app.apply_user(&BoundAction::Prompt("nope".into())).unwrap();

        assert!(effects.is_empty());
        assert!(
            app.flash_text().unwrap().contains("no prompt named 'nope'"),
            "{:?}",
            app.flash_text()
        );
    });
}

#[test]
fn a_prompt_with_no_pane_open_types_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(std::fs::canonicalize(tmp.path()).unwrap());
        app.state
            .config
            .prompts
            .insert("review".into(), "review %".into());

        let effects = app
            .apply_user(&BoundAction::Prompt("review".into()))
            .unwrap();

        assert!(effects.is_empty());
        assert!(app.flash_text().unwrap().contains("no pane open"));
    });
}

#[test]
fn bare_prompt_command_lists_the_templates() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(std::fs::canonicalize(tmp.path()).unwrap());
        app.state.config.prompts.insert("review".into(), "r".into());
        app.state
            .config
            .prompts
            .insert("explain".into(), "e".into());

        let effects = app.dispatch_command("prompt");

        assert!(effects.is_empty());
        assert_eq!(app.flash_text(), Some("prompts: explain, review"));
    });
}

/// An agent turns bracketed paste on, and a multi-line template must reach it
/// as ONE paste: typed raw, its first newline would submit half a prompt.
#[test]
fn a_child_that_asked_for_bracketed_paste_gets_one_paste() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        let got = root.join("got.txt");
        let mut app = App::test_app(root.clone());
        app.state
            .config
            .prompts
            .insert("two".into(), "first line\nsecond line".into());
        let script = format!(
            "printf '\\033[?2004h'; cat > {}",
            crate::shell::shell_quote(&got.to_string_lossy())
        );
        let cmd = format!("sh -c {}", crate::shell::shell_quote(&script));
        assert!(app.open_pane_tab_in(&cmd, &root));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let tabs = app.runtime.pane_tabs.as_mut().unwrap();
            tabs.active_mut().drain_output();
            if tabs.active().bracketed_paste_enabled() {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "child never enabled bracketed paste"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        let effects = app.apply_user(&BoundAction::Prompt("two".into())).unwrap();
        deliver(&mut app, effects);

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let text = loop {
            let text = std::fs::read_to_string(&got).unwrap_or_default();
            if text.contains("\u{1b}[201~") || std::time::Instant::now() > deadline {
                break text;
            }
            app.runtime
                .pane_tabs
                .as_mut()
                .unwrap()
                .active_mut()
                .drain_output();
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        assert!(
            text.starts_with("\u{1b}[200~first line\nsecond line\u{1b}[201~"),
            "{text:?}"
        );
    });
}
