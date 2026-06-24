use super::*;

/// `^a |` cycles the vertical split off → top-only → full-height → off, and
/// closing clears the right-region preview.
#[test]
fn vsplit_cycle_opens_flips_and_closes() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().join("work");
        std::fs::create_dir(&dir).unwrap();
        // A real file under the cursor so the preview loads (its source_path
        // drives the same-file mode-cycle vs different-file swap decision).
        std::fs::write(dir.join("a.md"), "# A\n\nbody").unwrap();
        let mut app = App::test_app(dir);
        app.open_pane_tab("cat"); // a pane is open → open defaults to top-only
        app.seed_rows(&["a.md"]);
        assert!(app.state.vsplit.is_none());

        app.apply(&Action::VsplitCycle).unwrap();
        assert_eq!(
            app.state.vsplit.map(|v| v.mode),
            Some(state::VsplitMode::TopOnly),
            "first ^a | opens top-only (a pane is open)"
        );
        app.apply(&Action::VsplitCycle).unwrap();
        assert_eq!(
            app.state.vsplit.map(|v| v.mode),
            Some(state::VsplitMode::FullHeight),
            "second ^a | flips to full-height"
        );
        app.apply(&Action::VsplitCycle).unwrap();
        assert!(app.state.vsplit.is_none(), "third ^a | closes the split");
        assert!(
            app.view.right_pager.is_none(),
            "closing clears the right-region preview"
        );
    });
}

/// `^a b` focuses the right column (so its keys route to the preview pager),
/// `^a a` focuses the left; with no split, `^a b` is a no-op.
#[test]
fn vsplit_focus_toggles_the_active_column() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().join("work");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("a.md"), "# A\n\nbody").unwrap();
        let mut app = App::test_app(dir);
        app.seed_rows(&["a.md"]);

        // No split: focusing right is a no-op (no right column to own input).
        app.apply(&Action::VsplitFocusRight).unwrap();
        assert!(!app.right_column_focused());

        app.apply(&Action::VsplitCycle).unwrap(); // open (focus defaults to left)
        assert!(
            !app.right_column_focused(),
            "opens with the left column active"
        );
        app.apply(&Action::VsplitFocusRight).unwrap();
        assert!(
            app.right_column_focused(),
            "^a b activates the right column"
        );
        app.apply(&Action::VsplitFocusLeft).unwrap();
        assert!(
            !app.right_column_focused(),
            "^a a returns to the left column"
        );
    });
}

/// `^a |` with the cursor on a *different* file swaps the preview to that file
/// and keeps the split's shape (mode/width) — "send this file to the split".
#[test]
fn vsplit_cycle_swaps_preview_keeping_shape() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().join("work");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("a.md"), "# A").unwrap();
        std::fs::write(dir.join("b.md"), "# B").unwrap();
        let mut app = App::test_app(dir);
        app.open_pane_tab("cat"); // a pane is open → open defaults to top-only
        app.seed_rows(&["a.md", "b.md"]);

        app.apply(&Action::VsplitCycle).unwrap(); // open on a.md (top-only)
        assert_eq!(
            app.state.vsplit.map(|v| v.mode),
            Some(state::VsplitMode::TopOnly)
        );
        assert!(
            app.view
                .right_pager
                .as_ref()
                .and_then(|v| v.source_path.as_ref())
                .is_some_and(|p| p.ends_with("a.md")),
            "preview starts on a.md"
        );

        app.state.left.cursor.index = 1; // move to b.md
        app.apply(&Action::VsplitCycle).unwrap();
        assert_eq!(
            app.state.vsplit.map(|v| v.mode),
            Some(state::VsplitMode::TopOnly),
            "swap keeps the shape (no mode cycle)"
        );
        assert!(
            app.view
                .right_pager
                .as_ref()
                .and_then(|v| v.source_path.as_ref())
                .is_some_and(|p| p.ends_with("b.md")),
            "preview swapped to the cursor file b.md"
        );
    });
}

/// From the bottom pane, `^a l` reaches the right column only in full-height
/// (where it sits beside the pane); in top-only h/l from the pane are inert.
#[test]
fn vsplit_focus_right_from_pane_only_in_full_height() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(std::path::PathBuf::from("/tmp/harness"));
        app.state.vsplit = Some(state::VSplit {
            width_pct: 50,
            mode: state::VsplitMode::TopOnly,
            focus: state::Side::Left,
        });
        app.state.focus = state::Focus::Pane;

        // Top-only: `^a l` from the pane is inert (pane keeps focus).
        app.apply(&Action::VsplitFocusRight).unwrap();
        assert!(matches!(app.state.focus, state::Focus::Pane));

        // Full-height: the right column is beside the pane — `^a l` jumps to it.
        app.state.vsplit.as_mut().unwrap().mode = state::VsplitMode::FullHeight;
        app.apply(&Action::VsplitFocusRight).unwrap();
        assert!(
            app.right_column_focused(),
            "full-height: ^a l from the pane reaches the right column"
        );

        // `^a h` from the pane stays inert even in full-height (left isn't beside it).
        app.state.focus = state::Focus::Pane;
        app.apply(&Action::VsplitFocusLeft).unwrap();
        assert!(
            matches!(app.state.focus, state::Focus::Pane),
            "^a h from the pane is inert"
        );
    });
}

/// `^a h` (return to the left) restores the left side's *vertical* position —
/// the bottom pane if that's where focus left from, not always the top list.
#[test]
fn vsplit_h_restores_left_vertical_position() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(std::path::PathBuf::from("/tmp/harness"));
        app.open_pane_tab("cat"); // a pane must exist for Focus::Pane to restore
        app.state.vsplit = Some(state::VSplit {
            width_pct: 50,
            mode: state::VsplitMode::FullHeight,
            focus: state::Side::Left,
        });

        // From the bottom pane → right → back: returns to the pane.
        app.state.focus = state::Focus::Pane;
        app.apply(&Action::VsplitFocusRight).unwrap();
        assert!(app.right_column_focused());
        app.apply(&Action::VsplitFocusLeft).unwrap();
        assert!(
            matches!(app.state.focus, state::Focus::Pane),
            "^a h returns to the pane it left from"
        );

        // From the file list → right → back: returns to the list.
        app.state.focus = state::Focus::FileList;
        app.apply(&Action::VsplitFocusRight).unwrap();
        app.apply(&Action::VsplitFocusLeft).unwrap();
        assert!(
            matches!(app.state.focus, state::Focus::FileList),
            "^a h returns to the list when that's where it left from"
        );
    });
}

/// `^a j` from the right column descends to the bottom pane but *remembers*
/// the column — `^a k` then climbs back to the right column, not the left list.
#[test]
fn vsplit_pane_descend_remembers_the_right_column() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(std::path::PathBuf::from("/tmp/harness"));
        app.open_pane_tab("cat"); // a pane to descend into
        app.state.vsplit = Some(state::VSplit {
            width_pct: 50,
            mode: state::VsplitMode::FullHeight,
            focus: state::Side::Right,
        });
        app.state.focus = state::Focus::FileList; // right column owns input
        assert!(app.right_column_focused());

        // `^a j` → the pane, leaving `vsplit.focus` on the right.
        app.apply(&Action::PaneFocusDown).unwrap();
        assert!(matches!(app.state.focus, state::Focus::Pane));
        assert_eq!(
            app.state.vsplit.map(|v| v.focus),
            Some(state::Side::Right),
            "descending keeps the column it came from"
        );

        // `^a k` → back up to the right column it descended from.
        app.apply(&Action::PaneFocusUp).unwrap();
        assert!(
            app.right_column_focused(),
            "^a k climbs back to the right column, not the left list"
        );
    });
}

/// With no lower pane open, `^a |` opens the split **full-height** (top-only
/// would reserve a strip for a pane that isn't there).
#[test]
fn vsplit_opens_full_height_without_a_pane() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().join("work");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("a.md"), "# A").unwrap();
        let mut app = App::test_app(dir);
        app.seed_rows(&["a.md"]); // no pane open
        app.apply(&Action::VsplitCycle).unwrap();
        assert_eq!(
            app.state.vsplit.map(|v| v.mode),
            Some(state::VsplitMode::FullHeight),
            "no pane → opens full-height"
        );
    });
}

/// A tab switch (`^a n`/`^a #`) from a fullscreen list (`TopList` zoom)
/// fullscreens the newly-active tab instead of flipping a hidden pane.
#[test]
fn tab_switch_from_list_zoom_fullscreens_the_tab() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().join("work");
        std::fs::create_dir(&dir).unwrap();
        let mut app = App::test_app(dir);
        app.open_pane_tab("cat");
        app.open_pane_tab("cat"); // two tabs
        app.state.focus = state::Focus::FileList;
        app.toggle_pane_zoom(); // list focused → TopList
        assert_eq!(app.state.pane.zoom, state::ZoomTarget::TopList);

        app.apply(&Action::PaneNextTab).unwrap();
        assert_eq!(
            app.state.pane.zoom,
            state::ZoomTarget::BottomPane,
            "^a n from a fullscreen list fullscreens the chosen tab"
        );
    });
}

/// `^a z` zooms the *focused* split column: the right column (preview) →
/// `RightColumn`, the left column (list) → `TopList`. Works even with no pane.
#[test]
fn z_zooms_the_focused_split_column() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().join("work");
        std::fs::create_dir(&dir).unwrap();
        let mut app = App::test_app(dir); // no pane
        app.state.vsplit = Some(state::VSplit {
            width_pct: 50,
            mode: state::VsplitMode::TopOnly,
            focus: state::Side::Right,
        });
        app.state.focus = state::Focus::FileList;
        assert!(app.right_column_focused());

        app.toggle_pane_zoom();
        assert_eq!(
            app.state.pane.zoom,
            state::ZoomTarget::RightColumn,
            "^a z on the right column zooms the preview (no pane needed)"
        );
        app.toggle_pane_zoom();
        assert_eq!(app.state.pane.zoom, state::ZoomTarget::None);

        // Left column focused → zooms the list (TopList).
        app.state.vsplit.as_mut().unwrap().focus = state::Side::Left;
        app.toggle_pane_zoom();
        assert_eq!(app.state.pane.zoom, state::ZoomTarget::TopList);
    });
}

/// `q` on a focused right-split preview closes the whole split (not just the
/// pager): the shape clears and the preview is dropped.
#[test]
fn q_on_focused_preview_closes_the_split() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().join("work");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("a.md"), "# A\n\nbody").unwrap();
        let mut app = App::test_app(dir);
        app.seed_rows(&["a.md"]);

        app.apply(&Action::VsplitCycle).unwrap(); // open
        app.apply(&Action::VsplitFocusRight).unwrap(); // focus the preview
        assert!(app.right_column_focused());

        app.handle_pager_key(key('q'));
        assert!(
            app.state.vsplit.is_none(),
            "q on the focused preview closes the split"
        );
        assert!(app.view.right_pager.is_none());
    });
}

/// `^a |` on a directory warns and stays closed — a dir isn't previewable.
#[test]
fn vsplit_cycle_on_directory_warns_and_stays_closed() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().join("work");
        std::fs::create_dir(&dir).unwrap();
        let mut app = App::test_app(dir.clone());
        app.state.left.rows = vec![RowData {
            path: dir.join("sub"),
            display: "sub".to_string(),
            kind: EntryKind::Dir,
            deleted: false,
        }];
        app.state.left.cursor.index = 0;

        app.apply(&Action::VsplitCycle).unwrap();
        assert!(
            app.state.vsplit.is_none(),
            "^a | on a directory stays closed"
        );
        assert!(app.view.right_pager.is_none());
    });
}
