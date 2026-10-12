use super::*;

fn execute_text_effects(app: &mut App, effects: Vec<Effect>) {
    for effect in effects {
        if let Effect::ReadPaneText { kind, then } = effect {
            app.execute_pane_text(kind, then);
        }
    }
}

fn replay(app: &mut App, root: &std::path::Path, label: &str) {
    std::fs::create_dir_all(root.join("files")).unwrap();
    let screen = "\x1b[2J\x1b[H• Printed files/printed.rs:2\r\n\r\n› inspect files/draft-only.rs\r\n  and files/another-draft.rs\r\n\r\n  Context 100% left · files/footer.rs\r\n";
    std::fs::write(root.join("screen.txt"), screen).unwrap();
    for file in [
        "files/printed.rs",
        "files/draft-only.rs",
        "files/another-draft.rs",
        "files/footer.rs",
    ] {
        std::fs::write(root.join(file), "first\nsecond\nthird\n").unwrap();
    }
    app.state.chdir(root).unwrap();
    let wake = app.make_pane_wake();
    let pane = crate::pane::Pane::spawn(
        "/bin/sh -c 'cat screen.txt; sleep 30'",
        24,
        100,
        root,
        &app.view.context_path,
        wake,
    )
    .unwrap();
    let info = crate::pane::tabs::TabInfo::new(label, root.to_path_buf());
    app.runtime.pane_tabs = Some(crate::pane::tabs::PaneTabs::new(
        crate::pane::tabs::TabEntry::new(pane, info),
    ));
    let tabs = app.runtime.pane_tabs.as_mut().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        tabs.active_mut().drain_output();
        if tabs
            .active()
            .visible_lines()
            .iter()
            .any(|row| row.contains("Context 100% left"))
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "fixture never reached the pane"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn codex_gf_and_gf_line_navigate_to_printed_files() {
    for second in ['f', 'F'] {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(&tmp.path().join("state"), || {
            let root = tmp.path().canonicalize().unwrap();
            let mut app = App::test_app(root.clone());
            replay(&mut app, &root, "codex");
            app.state.focus = state::Focus::FileList;
            app.handle_key(key('g')).unwrap();
            let effects = app.handle_key(key(second)).unwrap();
            assert!(
                effects
                    .iter()
                    .any(|fx| matches!(fx, Effect::ReadPaneText { .. }))
            );
            execute_text_effects(&mut app, effects);
            let col = app.state.cur();
            assert_eq!(
                col.rows[col.cursor.index].path,
                root.join("files/printed.rs")
            );
            if second == 'F' {
                assert!(app.view.pager.is_some(), "gF opens the printed file");
            }
        });
    }
}

#[test]
fn codex_jump_suggests_the_printed_path() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let root = tmp.path().canonicalize().unwrap();
        let mut app = App::test_app(root.clone());
        replay(&mut app, &root, "codex");
        let effects = app.handle_key(key('J')).unwrap();
        execute_text_effects(&mut app, effects);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !app.apply_jump_defaults() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let Mode::Prompting(prompt) = &app.state.mode else {
            panic!("J opens the prompt");
        };
        assert_eq!(
            prompt.suggestion.as_deref(),
            Some(crate::paths::display_tilde(&root.join("files/printed.rs")).as_str())
        );
    });
}

#[test]
fn codex_quick_select_keeps_output_coordinates_and_skips_drafts() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let root = tmp.path().canonicalize().unwrap();
        let mut app = App::test_app(root.clone());
        replay(&mut app, &root, "codex");
        app.open_quick_select();
        let matches = &app.view.quick_select.as_ref().unwrap().matches;
        assert!(
            matches
                .iter()
                .any(|m| m.text.contains("files/printed.rs") && m.row == 0)
        );
        assert!(
            !matches
                .iter()
                .any(|m| m.text.contains("draft") || m.text.contains("files/footer.rs"))
        );
    });
}

#[test]
fn codex_scrollback_selection_uses_the_visible_history_verbatim() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let root = tmp.path().canonicalize().unwrap();
        let mut app = App::test_app(root.clone());
        replay(&mut app, &root, "codex");
        app.runtime
            .pane_tabs
            .as_mut()
            .unwrap()
            .active_mut()
            .enter_scroll_mode();
        app.open_quick_select();
        assert!(
            app.view
                .quick_select
                .as_ref()
                .unwrap()
                .matches
                .iter()
                .any(|m| m.text.contains("files/draft-only.rs"))
        );
        let effects = app.apply(&Action::GotoFile).unwrap();
        execute_text_effects(&mut app, effects);
        let col = app.state.cur();
        assert_eq!(
            col.rows[col.cursor.index].path,
            root.join("files/footer.rs")
        );
    });
}

#[test]
fn unknown_agents_quick_select_still_uses_all_visible_text() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let root = tmp.path().canonicalize().unwrap();
        let mut app = App::test_app(root.clone());
        replay(&mut app, &root, "sh");
        app.open_quick_select();
        assert!(
            app.view
                .quick_select
                .as_ref()
                .unwrap()
                .matches
                .iter()
                .any(|m| m.text.contains("files/draft-only.rs"))
        );
    });
}

#[test]
fn a_shells_foreground_codex_supplies_the_navigation_boundary() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let root = tmp.path().canonicalize().unwrap();
        let mut app = App::test_app(root.clone());
        replay(&mut app, &root, "zsh");
        app.runtime
            .pane_tabs
            .as_mut()
            .unwrap()
            .active_info_mut()
            .running = Some(crate::pane::RunningAgent {
            kind: crate::state::sessions::AgentKind::Codex,
            pid: 12345,
            started_epoch_secs: 500,
            backgrounded: false,
            on_shared_daemon: false,
        });
        app.open_quick_select();
        assert!(
            !app.view
                .quick_select
                .as_ref()
                .unwrap()
                .matches
                .iter()
                .any(|m| m.text.contains("draft"))
        );
        let effects = app.apply(&Action::GotoFile).unwrap();
        execute_text_effects(&mut app, effects);
        let col = app.state.cur();
        assert_eq!(
            col.rows[col.cursor.index].path,
            root.join("files/printed.rs")
        );
    });
}
