//! `J` offers the newest path the pane printed as the jump prompt's default.

use super::*;

fn special(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::empty())
}

/// A tree with a file the pane "printed", and `CLAUDE.md` for the footer.
fn tree() -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(tmp.path()).unwrap();
    std::fs::create_dir_all(root.join("tasks")).unwrap();
    std::fs::write(root.join("tasks/run.output"), "").unwrap();
    std::fs::write(root.join("CLAUDE.md"), "").unwrap();
    (tmp, root)
}

fn open_jump_prompt(app: &mut App, suggestion: &str) {
    let mut p = Prompt::shell(PromptKind::Jump, "jump to: ");
    p.suggestion = Some(suggestion.to_string());
    app.state.mode = Mode::Prompting(p);
}

#[test]
fn j_asks_the_pane_for_its_default() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(std::path::PathBuf::from("/tmp/harness"));
        let fx = app.apply(&Action::JumpPrompt).unwrap();
        assert!(matches!(
            &app.state.mode,
            Mode::Prompting(p) if matches!(p.kind, PromptKind::Jump)
        ));
        let (kind, then) = fx.read_pane_text().expect("J emits one ReadPaneText");
        assert!(matches!(kind, PaneTextKind::Pickable(200)));
        assert!(matches!(then, PaneTextSink::JumpDefault));
    });
}

// The submits below name paths that don't exist: a jump that resolves chdirs,
// which `set_current_dir`s, and the unit tests stay chdir-free for the
// parallel runner. `jump_to` itself (a file lands in its directory with the
// cursor on it) is the existing path both cases reach.

/// History records where Enter went, and the failed jump names it.
#[test]
fn enter_on_the_empty_prompt_submits_the_suggestion() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(tmp.path().to_path_buf());
        open_jump_prompt(&mut app, "/no/such/dir/run.output");

        app.handle_key(special(KeyCode::Enter)).unwrap();

        assert!(matches!(app.state.mode, Mode::Normal));
        assert_eq!(
            app.state.jump_history.prev(""),
            Some("/no/such/dir/run.output")
        );
        let flash = app.flash_text().unwrap_or_default();
        assert!(flash.contains("/no/such/dir/run.output"), "{flash:?}");
    });
}

#[test]
fn typed_text_wins_over_the_suggestion() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(tmp.path().to_path_buf());
        open_jump_prompt(&mut app, "/no/such/dir/run.output");

        for c in "/no/typed".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(special(KeyCode::Enter)).unwrap();

        assert_eq!(app.state.jump_history.prev(""), Some("/no/typed"));
        let flash = app.flash_text().unwrap_or_default();
        assert!(
            flash.contains("/no/typed") && !flash.contains("run.output"),
            "{flash:?}"
        );
    });
}

/// `→` takes the suggestion into the buffer so it can be edited, not jumped to.
#[test]
fn right_arrow_loads_the_suggestion_for_editing() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(tmp.path().to_path_buf());
        open_jump_prompt(&mut app, "~/src/spyc");

        app.handle_key(special(KeyCode::Right)).unwrap();
        app.handle_key(key('/')).unwrap();

        let Mode::Prompting(p) = &app.state.mode else {
            panic!("the prompt stays open");
        };
        assert_eq!(p.buffer, "~/src/spyc/");
        assert_eq!(
            p.editor
                .as_ref()
                .map(crate::ui::line_edit::LineEditor::text)
                .as_deref(),
            Some("~/src/spyc/")
        );
    });
}

/// With a buffer, `→` is the editor's own key again.
#[test]
fn right_arrow_after_typing_does_not_load_the_suggestion() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(tmp.path().to_path_buf());
        open_jump_prompt(&mut app, "~/src/spyc");

        app.handle_key(key('x')).unwrap();
        app.handle_key(special(KeyCode::Right)).unwrap();

        let Mode::Prompting(p) = &app.state.mode else {
            panic!("the prompt stays open");
        };
        assert_eq!(p.buffer, "x");
    });
}

/// The worker resolves what the pane printed against the pane's cwd and hands
/// the answer to the prompt that asked.
#[test]
fn the_worker_offers_the_newest_printed_path() {
    let (tmp, root) = tree();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let mut app = App::test_app(root.clone());
        app.state.mode = Mode::Prompting(Prompt::shell(PromptKind::Jump, "jump to: "));
        let lines = vec![
            "⏺ Read CLAUDE.md".to_string(),
            "  Output is being written to: tasks/run.output. To check it, use Read.".to_string(),
        ];

        app.spawn_jump_default(lines, root.clone());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !app.apply_jump_defaults() {
            assert!(
                std::time::Instant::now() < deadline,
                "the worker never answered"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        let Mode::Prompting(p) = &app.state.mode else {
            panic!("the prompt stays open");
        };
        assert_eq!(
            p.suggestion.as_deref(),
            Some(crate::paths::display_tilde(&root.join("tasks/run.output")).as_str())
        );
    });
}

/// Claude's status line names `CLAUDE.md`, below everything it printed. Read
/// bottom-up without the cut, that wins over the path the reply named.
#[test]
fn claudes_footer_does_not_outrank_its_output() {
    let (_tmp, root) = tree();
    let rule = "─".repeat(80);
    let lines: Vec<String> = [
        "⏺ Bash(make update)",
        "  ⎿  Output is being written to: tasks/run.output. To check it, use Read.",
        "",
        &rule,
        "❯\u{a0}",
        &rule,
        "  [Opus 5.5] │ spyc git:(main)",
        "  1 CLAUDE.md | 1 MCPs | 11 hooks",
        "  -- INSERT -- ⏵⏵ auto mode on",
    ]
    .into_iter()
    .map(String::from)
    .collect();

    let uncut =
        crate::app::navigate::find_path_ref(&lines, std::slice::from_ref(&root)).map(|r| r.path);
    assert_eq!(
        uncut,
        Some(root.join("CLAUDE.md")),
        "the footer is in reach"
    );

    let n = crate::agent::detect("claude").output_len(&lines);
    let cut = crate::app::navigate::find_path_ref(&lines[..n], std::slice::from_ref(&root))
        .map(|r| r.path);
    assert_eq!(cut, Some(root.join("tasks/run.output")));
}
