#![allow(clippy::wildcard_imports)]
use super::*;
use crate::app::effect::matchers::EffectSliceExt;

#[test]
fn apply_down_moves_cursor() {
    let mut s = state_with_rows(&["a", "b", "c"]);
    assert!(matches!(s.apply(&Action::Down(1)), ApplyResult::Handled));
    assert_eq!(s.left.cursor.index, 1);
}

#[test]
fn apply_up_wraps() {
    let mut s = state_with_rows(&["a", "b", "c"]);
    assert!(matches!(s.apply(&Action::Up(1)), ApplyResult::Handled));
    assert_eq!(s.left.cursor.index, 2);
}

#[test]
fn apply_down_with_count() {
    let mut s = state_with_rows(&["a", "b", "c", "d", "e"]);
    s.apply(&Action::Down(3));
    assert_eq!(s.left.cursor.index, 3);
}

#[test]
fn apply_page_down() {
    let mut s = state_with_rows(&["a", "b", "c", "d", "e", "f"]);
    s.left.grid_dims = GridDims {
        cols: 1,
        rows_per_col: 3,
    };
    s.apply(&Action::PageDown);
    assert_eq!(s.left.cursor.index, 3);
}

#[test]
fn apply_goto_first() {
    let mut s = state_with_rows(&["a", "b", "c"]);
    s.left.cursor.index = 2;
    s.apply(&Action::GotoFirst);
    assert_eq!(s.left.cursor.index, 0);
}

#[test]
fn apply_goto_last() {
    let mut s = state_with_rows(&["a", "b", "c"]);
    s.apply(&Action::GotoLast);
    assert_eq!(s.left.cursor.index, 2);
}

#[test]
fn apply_left_right_columns() {
    let mut s = state_with_rows(&["a", "b", "c", "d", "e", "f"]);
    s.left.grid_dims = GridDims {
        cols: 2,
        rows_per_col: 3,
    };
    s.apply(&Action::Right(1));
    assert_eq!(s.left.cursor.index, 3);
    s.apply(&Action::Left(1));
    assert_eq!(s.left.cursor.index, 0);
}

#[test]
fn apply_toggle_pick() {
    let mut s = state_with_rows(&["a.txt", "b.txt"]);
    s.apply(&Action::TogglePick);
    assert!(s.left.picks.contains(Path::new("/tmp/test/a.txt")));
}

#[test]
fn apply_pick_toggle_all() {
    let mut s = state_with_rows(&["a", "b", "c"]);
    s.apply(&Action::PickToggleAll);
    assert_eq!(s.left.picks.len(), 3);
    s.apply(&Action::PickToggleAll);
    assert!(s.left.picks.is_empty());
}

#[test]
fn apply_take_adds_to_inventory() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut s = state_with_real_files(tmp.path(), &["a.txt"]);
        s.apply(&Action::Take);
        assert_eq!(s.inventory.len(), 1);
        assert!(s.inventory.contains(&tmp.path().join("a.txt")));
    });
}

#[test]
fn apply_drop_removes_from_inventory() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut s = state_with_real_files(tmp.path(), &["a.txt"]);
        s.take(); // yank first
        s.toggle_inventory_view();
        s.apply(&Action::Drop);
        assert!(s.inventory.is_empty());
    });
}

#[test]
fn apply_toggle_inventory_view() {
    let mut s = test_state();
    s.apply(&Action::ToggleInventoryView);
    assert_eq!(s.left.view, View::Inventory);
    s.apply(&Action::ToggleInventoryView);
    assert_eq!(s.left.view, View::Dir);
}

#[test]
fn apply_empty_inventory() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut s = state_with_real_files(tmp.path(), &["a.txt"]);
        s.take(); // yank first
        assert_eq!(s.inventory.len(), 1);
        s.apply(&Action::EmptyInventory);
        assert!(s.inventory.is_empty());
    });
}

#[test]
fn apply_toggle_mask() {
    let mut s = test_state();
    let was_enabled = s.left.masks.mask1.enabled;
    s.apply(&Action::ToggleMask(1));
    assert_ne!(s.left.masks.mask1.enabled, was_enabled);
}

#[test]
fn apply_search_next_finds_match() {
    let mut s = state_with_rows(&["alpha", "beta", "gamma"]);
    s.last_search = Some("g".to_string());
    s.apply(&Action::SearchNext);
    assert_eq!(s.left.cursor.index, 2);
}

#[test]
fn apply_search_prev_finds_match() {
    // Only `alpha` contains `lph`, so the backward sweep from
    // gamma → beta → alpha lands unambiguously on idx 0 under
    // substring matching too.
    let mut s = state_with_rows(&["alpha", "beta", "gamma"]);
    s.left.cursor.index = 2;
    s.last_search = Some("lph".to_string());
    s.apply(&Action::SearchPrev);
    assert_eq!(s.left.cursor.index, 0);
}

#[test]
fn apply_start_shell_returns_spawn() {
    let mut s = test_state();
    let result = s.apply(&Action::StartShell);
    assert!(matches!(
        result,
        ApplyResult::Post(ref e) if matches!(e.as_slice(), [Effect::ForegroundExec { .. }])
    ));
}

#[test]
fn apply_prompt_actions_set_mode() {
    let mut s = test_state();
    s.apply(&Action::SearchPrompt);
    assert!(matches!(s.mode, Mode::Prompting(_)));

    s.mode = Mode::Normal;
    s.apply(&Action::ShellCapturedPrompt);
    assert!(matches!(s.mode, Mode::Prompting(_)));

    s.mode = Mode::Normal;
    s.apply(&Action::CommandPrompt);
    assert!(matches!(s.mode, Mode::Prompting(_)));

    s.mode = Mode::Normal;
    s.apply(&Action::JumpPrompt);
    assert!(matches!(s.mode, Mode::Prompting(_)));

    s.mode = Mode::Normal;
    s.apply(&Action::LimitPrompt);
    assert!(matches!(s.mode, Mode::Prompting(_)));

    s.mode = Mode::Normal;
    s.apply(&Action::SetEnvPrompt);
    assert!(matches!(s.mode, Mode::Prompting(_)));
}

#[test]
fn apply_set_mark() {
    let mut s = state_with_rows(&["file.txt"]);
    s.apply(&Action::SetMark('a'));
    assert!(s.marks.get('a').is_some());
}

#[test]
fn apply_date_flashes() {
    let mut s = test_state();
    s.apply(&Action::Date);
    assert!(s.flash.is_some());
    assert!(s.flash.as_ref().unwrap().text.contains("UTC"));
}

#[test]
fn apply_version_flashes() {
    let mut s = test_state();
    s.apply(&Action::Version);
    let flash = s.flash.as_ref().unwrap();
    assert!(flash.text.contains("spyc"));
}

#[test]
fn apply_noop_does_nothing() {
    let mut s = test_state();
    let result = s.apply(&Action::Noop);
    assert!(matches!(result, ApplyResult::Handled));
}

#[test]
fn apply_macro_record_reserved_flashes_hint() {
    let mut s = test_state();
    let result = s.apply(&Action::MacroRecordReserved);
    assert!(matches!(result, ApplyResult::Handled));
    let flash = s.flash.as_ref().unwrap();
    assert!(flash.text.contains("reserved"), "got: {}", flash.text);
    assert!(flash.text.contains('Q'), "should hint at Q: {}", flash.text);
}

#[test]
fn apply_long_list_returns_pager() {
    let mut s = state_with_rows(&["a.txt"]);
    let result = s.apply(&Action::LongList);
    assert!(matches!(result, ApplyResult::OpenPager(_)));
}

#[test]
fn apply_file_type_single_flashes() {
    let mut s = state_with_rows(&["a.txt"]);
    let result = s.apply(&Action::FileType);
    // Single file: flashes info, returns Handled
    assert!(matches!(result, ApplyResult::Handled));
    assert!(s.flash.is_some());
}

#[test]
fn apply_pane_actions_not_handled() {
    let mut s = test_state();
    assert!(matches!(
        s.apply(&Action::TogglePane),
        ApplyResult::NotHandled
    ));
    assert!(matches!(
        s.apply(&Action::PaneFocusDown),
        ApplyResult::NotHandled
    ));
    assert!(matches!(s.apply(&Action::Help), ApplyResult::NotHandled));
    assert!(matches!(s.apply(&Action::Redraw), ApplyResult::NotHandled));
    assert!(matches!(
        s.apply(&Action::ColorToggle),
        ApplyResult::NotHandled
    ));
}

#[test]
fn apply_worktree_new_sets_prompt_or_errors() {
    let mut s = test_state();
    // No git info → error
    s.apply(&Action::WorktreeNew);
    assert!(matches!(s.flash.as_ref().unwrap().kind, FlashKind::Error));

    // With git info → prompt
    s.flash = None;
    s.left.git.info = Some("main".to_string());
    s.apply(&Action::WorktreeNew);
    assert!(matches!(s.mode, Mode::Prompting(_)));
}

#[test]
fn apply_jump_prev_dir() {
    let mut s = test_state();
    // No prev dir → error
    s.apply(&Action::JumpPrevDir);
    assert!(matches!(s.flash.as_ref().unwrap().kind, FlashKind::Error));
}

// ── ChangeDir effect emission (MVU Phase 5 / PR7) ──────────────
// The pure-Model `apply()` Action arms emit a deferred
// `Effect::ChangeDir` instead of calling `chdir` inline; `run_effects`
// runs the IO. These tests pin the emitted effect's fields byte-for-byte
// (no `chdir` is called, so the parallel-runner CWD race is avoided —
// the suite is deliberately `chdir`-free).

#[test]
fn apply_jump_start_dir_emits_change_dir() {
    let mut s = test_state(); // start_dir = /tmp/test
    let ApplyResult::Post(fx) = s.apply(&Action::JumpStartDir) else {
        panic!("expected Post");
    };
    let cd = fx.change_dir().expect("one ChangeDir effect");
    assert_eq!(cd.path(), Path::new("/tmp/test"));
    assert!(cd.focus().is_none());
    assert!(cd.on_ok().is_none());
    assert_eq!(cd.err_prefix(), "jump to start failed");
}

#[test]
fn apply_jump_prev_dir_emits_change_dir_when_set() {
    let mut s = test_state();
    s.prev_dir = Some(PathBuf::from("/tmp/prev"));
    let ApplyResult::Post(fx) = s.apply(&Action::JumpPrevDir) else {
        panic!("expected Post");
    };
    let cd = fx.change_dir().expect("one ChangeDir effect");
    assert_eq!(cd.path(), Path::new("/tmp/prev"));
    assert_eq!(cd.err_prefix(), "jump back failed");
}

#[test]
fn apply_jump_project_home_emits_change_dir_when_set() {
    let mut s = test_state();
    s.project_home = Some(PathBuf::from("/tmp/proj"));
    let ApplyResult::Post(fx) = s.apply(&Action::JumpProjectHome) else {
        panic!("expected Post");
    };
    let cd = fx.change_dir().expect("one ChangeDir effect");
    assert_eq!(cd.path(), Path::new("/tmp/proj"));
    assert_eq!(cd.err_prefix(), "jump to project home failed");
}

#[test]
fn apply_jump_project_home_unset_flashes_no_effect() {
    let mut s = test_state(); // project_home = None
    assert!(matches!(
        s.apply(&Action::JumpProjectHome),
        ApplyResult::Handled
    ));
    assert!(matches!(s.flash.as_ref().unwrap().kind, FlashKind::Error));
}

#[test]
fn apply_jump_mark_emits_change_dir_with_focus_and_flash() {
    let mut s = test_state();
    s.marks.set(
        'a',
        Mark {
            dir: PathBuf::from("/tmp/marked"),
            focus: Some(PathBuf::from("/tmp/marked/file.rs")),
        },
    );
    let ApplyResult::Post(fx) = s.apply(&Action::JumpMark('a')) else {
        panic!("expected Post");
    };
    let cd = fx.change_dir().expect("one ChangeDir effect");
    assert_eq!(cd.path(), Path::new("/tmp/marked"));
    assert_eq!(cd.focus(), Some(Path::new("/tmp/marked/file.rs")));
    assert_eq!(cd.on_ok(), Some("jumped to mark 'a'"));
    assert_eq!(cd.err_prefix(), "jump failed");
}

#[test]
fn apply_jump_mark_unset_flashes_no_effect() {
    let mut s = test_state(); // no marks
    match s.apply(&Action::JumpMark('z')) {
        ApplyResult::Post(fx) => assert!(fx.is_empty(), "unset mark emits no effect"),
        other => panic!("expected empty Post, got {other:?}"),
    }
    assert!(matches!(s.flash.as_ref().unwrap().kind, FlashKind::Error));
}

#[test]
fn apply_climb_emits_change_dir_to_parent_focusing_old_dir() {
    let mut s = test_state(); // listing.dir = /tmp/test
    let ApplyResult::Post(fx) = s.apply(&Action::Climb) else {
        panic!("expected Post");
    };
    let cd = fx.change_dir().expect("one ChangeDir effect");
    assert_eq!(cd.path(), Path::new("/tmp"));
    // Focus by the just-left dir's path — the parent's row for that child
    // has `r.path == /tmp/test`, so this lands on the same row the former
    // by-display-name match did.
    assert_eq!(cd.focus(), Some(Path::new("/tmp/test")));
    assert!(cd.on_ok().is_none());
    assert_eq!(cd.err_prefix(), "chdir");
}

#[test]
fn apply_home_emits_change_dir() {
    let mut s = test_state();
    let result = s.apply(&Action::Home);
    // `Home` reads $HOME at dispatch; guard so the test is deterministic
    // regardless of the runner's environment (CI always has it set).
    if std::env::var_os("HOME").is_some() {
        let ApplyResult::Post(fx) = result else {
            panic!("expected Post");
        };
        let cd = fx.change_dir().expect("one ChangeDir effect");
        assert!(cd.focus().is_none());
        assert!(cd.on_ok().is_none());
        assert_eq!(cd.err_prefix(), "chdir");
    } else {
        assert!(matches!(result, ApplyResult::Handled));
    }
}

#[test]
fn apply_clamps_cursor_after_action() {
    let mut s = state_with_rows(&["a", "b"]);
    s.left.cursor.index = 10; // out of bounds
    s.apply(&Action::Noop); // any handled action should clamp
    assert_eq!(s.left.cursor.index, 1); // clamped to last valid
}

// ── COMMAND_TABLE registry sanity (MVU Phase 6) ───────────────
// (`command_table_is_sorted_and_unique` lives with the table in
// `command_table.rs`; this test stays here because it exercises
// `AppState::dispatch_command`'s routing.)
