use super::*;

/// `^s n` opens a second file-commander in the right column: `state.right` is
/// populated (rows read from disk), the split focuses `b`, and `cur()` then
/// resolves to the right commander — so `^a a` flips the focused column back to
/// the left. `^s x` tears it down to a single column again.
#[test]
fn second_commander_open_focus_and_close() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().join("work");
        std::fs::create_dir(&dir).unwrap();
        for n in ["a.txt", "b.txt", "c.txt"] {
            std::fs::write(dir.join(n), "x").unwrap();
        }
        let mut app = App::test_app(dir);

        let d = app.state.left.listing.dir.clone();
        app.open_second_commander_at(&d);
        assert!(app.state.right.is_some(), "^s n opens a second commander");
        assert_eq!(
            app.state.vsplit.map(|v| v.focus),
            Some(state::Side::Right),
            "opening focuses the new (right) column"
        );
        assert_eq!(
            app.state.vsplit.map(|v| v.mode),
            Some(state::VsplitMode::TopOnly),
            "a commander always opens top-only — never full-height (no pane under a only)"
        );
        assert_eq!(
            app.state.right.as_ref().unwrap().rows.len(),
            3,
            "the right commander read + built its own rows"
        );
        assert!(
            app.view.right_pager.is_none(),
            "a commander and the preview are mutually exclusive"
        );

        // cur() follows focus: with `b` focused it is the right commander.
        app.state.right.as_mut().unwrap().cursor.index = 2;
        assert_eq!(
            app.state.cur().cursor.index,
            2,
            "cur() == right when b focused"
        );
        app.apply(&Action::VsplitFocusLeft).unwrap();
        assert_eq!(
            app.state.cur().cursor.index,
            0,
            "^a a flips cur() back to the left column"
        );

        app.apply(&Action::CloseSecondCommander).unwrap();
        assert!(
            app.state.right.is_none(),
            "^s x closes the second commander"
        );
        assert!(
            app.state.vsplit.is_none(),
            "closing returns to a single column"
        );
    });
}

/// User-initiated (`^s n`) vs agent-initiated (MCP worktree open) focus: from
/// the pane, `^s n` pulls the keyboard into `b` (the user asked for it), while a
/// background open leaves the keyboard in the pane (the user is still typing to
/// the agent). Both make `b` the active/`cur()` column so a follow-up MCP call
/// lands there — the difference is purely the keyboard axis.
#[test]
fn background_open_keeps_pane_focus_while_user_open_grabs_it() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().join("work");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "x").unwrap();
        let b = tmp.path().join("wt");
        std::fs::create_dir(&b).unwrap();

        // Agent-initiated: user is mid-conversation in the pane.
        let mut app = App::test_app(dir.clone());
        app.state.focus = state::Focus::Pane;
        app.open_second_commander_at_background(&b);
        assert!(
            app.state.pane_focused(),
            "background open leaves the keyboard on the pane (still typing to the agent)"
        );
        assert_eq!(
            app.focused_side(),
            state::Side::Right,
            "b is still made the active column so the agent's follow-ups target it"
        );
        assert_eq!(
            std::fs::canonicalize(&app.state.cur().listing.dir).ok(),
            std::fs::canonicalize(&b).ok(),
            "cur() resolves to b even though the pane keeps the keyboard"
        );

        // User-initiated: `^s n` from the same pane-focused start grabs it.
        let mut app = App::test_app(dir);
        app.state.focus = state::Focus::Pane;
        app.open_second_commander_at(&b);
        assert!(
            !app.state.pane_focused(),
            "^s n moves the keyboard out of the pane into the new column"
        );
        assert_eq!(app.focused_side(), state::Side::Right, "b is focused");
    });
}

/// `^a |` is disabled while a second commander occupies the right column —
/// the two are mutually exclusive, so the cycle no-ops (no preview opens) and
/// the commander stays put.
#[test]
fn vsplit_cycle_disabled_with_second_commander() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().join("work");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "x").unwrap();
        let mut app = App::test_app(dir);

        let d = app.state.left.listing.dir.clone();
        app.open_second_commander_at(&d);
        let shape = app.state.vsplit;

        app.apply(&Action::VsplitCycle).unwrap();
        assert!(
            app.state.right.is_some(),
            "^a | must not tear down the commander"
        );
        assert!(
            app.view.right_pager.is_none(),
            "^a | must not open a preview over the commander"
        );
        assert_eq!(app.state.vsplit, shape, "the split shape is unchanged");
    });
}

/// PR G: a session saved with a second commander open persists its cwd
/// (`right_cwd`) + split shape, and restore reopens column `b` there — the
/// left column returns to the anchor, `b` to its own saved dir.
#[test]
fn session_persists_and_restores_a_commander_split() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let left = std::fs::canonicalize(tmp.path()).unwrap().join("left");
        let right = std::fs::canonicalize(tmp.path()).unwrap().join("right");
        std::fs::create_dir(&left).unwrap();
        std::fs::create_dir(&right).unwrap();
        let mut app = App::test_app(left.clone());
        app.state.session_name = Some("vsplit-commander-test".to_string());
        app.open_second_commander_at(&right);
        if let Some(v) = app.state.vsplit.as_mut() {
            v.width_pct = 42; // non-default → proves the shape round-trips
        }

        app.save_session();
        let saved = crate::state::sessions::load_sessions()
            .into_iter()
            .find(|s| s.name == "vsplit-commander-test")
            .expect("session saved");
        let sv = saved.vsplit.as_ref().expect("commander split persisted");
        assert_eq!(
            sv.right_cwd.as_deref(),
            Some(right.as_path()),
            "b's cwd is saved"
        );
        assert_eq!(sv.width_pct, 42, "split shape saved");

        // Restore the split into a fresh app via the chdir-free helper (calling
        // restore_session would set_current_dir and race the parallel runner) →
        // b reopens at its saved cwd with the saved shape.
        let mut fresh = App::test_app(left);
        fresh.restore_vsplit(sv, false);
        let b = fresh.state.right.as_ref().expect("b reopened on restore");
        assert_eq!(b.listing.dir, right, "b restored at its saved cwd");
        assert_eq!(
            fresh.state.vsplit.map(|v| v.width_pct),
            Some(42),
            "split width restored"
        );
    });
}

/// restore_vsplit must not restore a *blank* preview split: a saved preview
/// whose file vanished between sessions would otherwise open a carved, empty
/// right column. And a present preview restores the split, re-loads the file,
/// and clamps an out-of-range (hand-edited / older) width.
#[test]
fn restore_vsplit_drops_blank_split_and_clamps_width() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let mut app = App::test_app(dir.clone());

        // (a) preview file gone → no split, no phantom right pager.
        let gone = crate::state::sessions::SavedVsplit {
            width_pct: 50,
            full_height: false,
            focus_right: false,
            preview_path: Some(dir.join("vanished.md")),
            right_cwd: None,
        };
        app.restore_vsplit(&gone, false);
        assert!(app.state.vsplit.is_none(), "gone preview → no split");
        assert!(app.view.right_pager.is_none(), "no phantom right pager");

        // (b) preview present + out-of-range width → split restored, file
        // loaded, width clamped to [20, 80].
        let f = dir.join("preview.md");
        std::fs::write(&f, "# hi\n").unwrap();
        let ok = crate::state::sessions::SavedVsplit {
            width_pct: 95,
            full_height: false,
            focus_right: false,
            preview_path: Some(f.clone()),
            right_cwd: None,
        };
        app.restore_vsplit(&ok, false);
        assert_eq!(
            app.state.vsplit.map(|v| v.width_pct),
            Some(80),
            "split restored with width clamped to 80"
        );
        assert_eq!(
            app.view
                .right_pager
                .as_ref()
                .and_then(|p| p.source_path.clone()),
            Some(f),
            "preview file re-loaded"
        );
    });
}

/// load_right_preview reports whether the slot landed: `true` for a readable
/// file, `false` for a bad path — and on failure it leaves any existing
/// preview untouched. cycle_vsplit's swap branch relies on this so it doesn't
/// flash "preview: X" for a file it never actually showed.
#[test]
fn load_right_preview_reports_landing() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let mut app = App::test_app(dir.clone());

        let good = dir.join("a.md");
        std::fs::write(&good, "# a\n").unwrap();
        assert!(app.load_right_preview(&good), "readable file loads");
        assert!(app.view.right_pager.is_some());

        // A missing file fails and leaves the prior preview in place (the swap
        // branch keeps showing the old file).
        assert!(
            !app.load_right_preview(&dir.join("nope.md")),
            "missing file → false"
        );
        assert_eq!(
            app.view
                .right_pager
                .as_ref()
                .and_then(|p| p.source_path.clone()),
            Some(good),
            "old preview retained on failure"
        );
    });
}

/// `^s n` opens the second commander directly at PROJECT_HOME (no prompt) —
/// `b` is a second view into the same project, so its start dir is the shared
/// project home, not a chosen path.
#[test]
fn ctrl_s_n_opens_at_project_home() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let home = tmp.path().join("proj");
        let sub = home.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(home.join("root.txt"), "x").unwrap();
        // Focused column sits in a subdir; PROJECT_HOME is the project root.
        let mut app = App::test_app(sub);
        app.state.project_home = Some(std::fs::canonicalize(&home).unwrap());

        app.apply(&Action::OpenSecondCommander).unwrap();
        assert!(
            matches!(app.state.mode, Mode::Normal),
            "no prompt — opens directly"
        );
        let right = app.state.right.as_ref().expect("second commander opened");
        assert_eq!(
            right.listing.dir,
            std::fs::canonicalize(&home).unwrap(),
            "b opens at PROJECT_HOME, not the focused subdir"
        );
    });
}

/// App-side actions operate on the FOCUSED column: `SortCycle` driven while the
/// second commander (`b`) is focused changes `b`'s sort order, not the left's.
/// (The App-side `actions.rs` sort arm reads/writes `cur()`, not `self.left`.)
#[test]
fn app_side_sort_targets_the_focused_column() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().join("work");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "x").unwrap();
        let mut app = App::test_app(dir);
        let d = app.state.left.listing.dir.clone();
        app.open_second_commander_at(&d);
        assert!(app.right_column_focused(), "b is focused after open");

        let left_before = app.state.left.sort_order;
        let right_before = app.state.right.as_ref().unwrap().sort_order;
        app.apply(&Action::SortCycle).unwrap();

        assert_eq!(
            app.state.left.sort_order, left_before,
            "the left column's sort is untouched"
        );
        assert_ne!(
            app.state.right.as_ref().unwrap().sort_order,
            right_before,
            "SortCycle cycled the focused (right) column"
        );
    });
}

/// `^a l`/`^a h` switch columns freely with a `D` TopPane pager open in `a`:
/// the pager stays pinned to its column (`overlay_column`) and keeps rendering
/// there, while focus moves to `b`'s commander (so you can drive `b` beside an
/// open pager). `^a h` returns focus to the pager. (Regression: this used to be
/// blocked / dragged the pager onto `b`.)
#[test]
fn vsplit_focus_switches_with_pager_pinned_to_its_column() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().join("work");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "x").unwrap();
        let mut app = App::test_app(dir);
        let d = app.state.left.listing.dir.clone();
        app.open_second_commander_at(&d); // b open + focused
        // Focus the left column, then open a `D` pager there (pinned to `a`).
        app.apply(&Action::VsplitFocusLeft).unwrap();
        app.view.overlay_column = Some(state::Side::Left);
        let mut pv = PagerView::new_plain("t", vec!["a".to_string()]);
        pv.mount = crate::ui::pager::Mount::TopPane;
        app.view.pager = Some(pv);

        // `^a l` moves focus to `b`; the pager stays pinned to `a`.
        app.apply(&Action::VsplitFocusRight).unwrap();
        assert_eq!(
            app.state.vsplit.map(|v| v.focus),
            Some(state::Side::Right),
            "^a l switches focus to b even with a pager open"
        );
        assert_eq!(
            app.view.overlay_column,
            Some(state::Side::Left),
            "the pager stays pinned to its own column"
        );
        app.recompute_focus();
        assert!(
            app.right_column_focused(),
            "focus is on b's commander, not the pager in a"
        );
    });
}

/// Dual overlay slots: a `D` pager open in BOTH columns coexists (neither
/// evicts the other), and input routes to the FOCUSED column's pager.
/// Regression: a single slot meant opening `b`'s pager closed `a`'s.
#[test]
fn d_pagers_in_both_columns_coexist_and_route_to_the_focused_one() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().join("work");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "x").unwrap();
        let mut app = App::test_app(dir);
        let d = app.state.left.listing.dir.clone();
        app.open_second_commander_at(&d); // b open + focused

        // A `D` TopPane pager in each column, open at the same time.
        let mut la = PagerView::new_plain("a", vec!["left".to_string()]);
        la.mount = crate::ui::pager::Mount::TopPane;
        app.view.pager = Some(la);
        let mut rb = PagerView::new_plain("b", vec!["right".to_string()]);
        rb.mount = crate::ui::pager::Mount::TopPane;
        app.view.pager_right = Some(rb);
        app.state.focus = state::Focus::Pager(crate::ui::pager::Mount::TopPane);

        // b focused → its own pager owns input.
        assert_eq!(app.focused_side(), state::Side::Right);
        assert_eq!(app.active_pager_ref().map(|v| v.title.as_str()), Some("b"));

        // Switch focus to a → a's pager owns input; b's survives untouched.
        app.apply(&Action::VsplitFocusLeft).unwrap();
        assert_eq!(app.active_pager_ref().map(|v| v.title.as_str()), Some("a"));
        assert!(
            app.view.pager_right.is_some(),
            "b's pager isn't evicted by a's — they have separate slots"
        );
        assert!(app.view.pager.is_some());
    });
}

/// A full-frame modal pager (grep / git-view / help / `;cmd`) owns input
/// regardless of which column is focused — the column-scoped slots yield to it.
#[test]
fn modal_pager_owns_input_even_with_right_column_focused() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().join("work");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "x").unwrap();
        let mut app = App::test_app(dir);
        let d = app.state.left.listing.dir.clone();
        app.open_second_commander_at(&d); // b open + focused

        // b has a `D` pager, but a full-frame grep (Overlay mount) is also up.
        let mut rb = PagerView::new_plain("b", vec!["x".to_string()]);
        rb.mount = crate::ui::pager::Mount::TopPane;
        app.view.pager_right = Some(rb);
        let mut grep = PagerView::new_plain("grep", vec!["hit".to_string()]);
        grep.mount = crate::ui::pager::Mount::Overlay;
        app.view.pager = Some(grep);
        app.state.focus = state::Focus::Pager(crate::ui::pager::Mount::Overlay);

        assert_eq!(
            app.active_pager_ref().map(|v| v.title.as_str()),
            Some("grep"),
            "the modal wins over b's column-scoped pager"
        );
    });
}

/// Closing the second commander (`^s x`) drops any overlay/pager open in `b`
/// along with the column — no orphaned editor PTY / pager left behind.
#[test]
fn closing_b_drops_its_pager() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().join("work");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "x").unwrap();
        let mut app = App::test_app(dir);
        let d = app.state.left.listing.dir.clone();
        app.open_second_commander_at(&d);
        let mut rb = PagerView::new_plain("b", vec!["x".to_string()]);
        rb.mount = crate::ui::pager::Mount::TopPane;
        app.view.pager_right = Some(rb);

        app.close_second_commander();
        assert!(app.state.right.is_none());
        assert!(
            app.view.pager_right.is_none(),
            "b's pager is dropped when its column closes"
        );
    });
}

/// Closing `b`'s pager with `q` closes ONLY `b`'s — `a`'s pager in the other
/// column survives. Regression: the `q`-close hardcoded `view.pager.take()`, so
/// it evicted `a`'s pager while `clear_pager` closed `b`'s — both went dark.
#[test]
fn q_on_b_pager_leaves_a_pager_open() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().join("work");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "x").unwrap();
        let mut app = App::test_app(dir);
        let d = app.state.left.listing.dir.clone();
        app.open_second_commander_at(&d); // b focused

        // A `D` pager in each column.
        app.apply(&Action::VsplitFocusLeft).unwrap();
        let mut pa = PagerView::new_plain("a-pager", vec!["x".to_string()]);
        pa.mount = crate::ui::pager::Mount::TopPane;
        app.install_top_pager(pa);
        app.apply(&Action::VsplitFocusRight).unwrap();
        let mut pb = PagerView::new_plain("b-pager", vec!["y".to_string()]);
        pb.mount = crate::ui::pager::Mount::TopPane;
        app.install_top_pager(pb);
        app.recompute_focus();
        assert!(app.view.pager.is_some() && app.view.pager_right.is_some());

        // q closes b's pager (the focused one) and leaves a's intact.
        app.handle_key(key('q')).unwrap();
        assert!(
            app.view.pager_right.is_none(),
            "q closes the focused column's (b's) pager"
        );
        assert!(
            app.view.pager.is_some(),
            "a's pager in the other column survives"
        );
        // …and a's pager stays PINNED to the left column — closing b's pager
        // must not clear `overlay_column`, or the carve would shove a's pager
        // into the (now focused) right column and blank the left (image: A
        // blank, B's list + A's pager overlapping).
        assert_eq!(
            app.view.overlay_column,
            Some(state::Side::Left),
            "a's pager stays pinned to the left column"
        );
    });
}

/// `q` from a focused `b` *commander* (no pager in `b`) does NOT close `a`'s
/// pager — it falls through to the resolver (b's file-list owns the key).
#[test]
fn q_from_b_commander_does_not_close_a_pager() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().join("work");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "x").unwrap();
        let mut app = App::test_app(dir);
        let d = app.state.left.listing.dir.clone();
        app.open_second_commander_at(&d);
        app.apply(&Action::VsplitFocusLeft).unwrap();
        let mut pv = PagerView::new_plain("a-pager", vec!["x".to_string()]);
        pv.mount = crate::ui::pager::Mount::TopPane;
        app.install_top_pager(pv);
        app.apply(&Action::VsplitFocusRight).unwrap(); // focus b's commander
        app.recompute_focus();

        app.handle_key(key('q')).unwrap();
        assert!(
            app.view.pager.is_some(),
            "q from b's commander leaves a's pager open"
        );
    });
}

/// `column_focused` tracks the keyboard-owning column regardless of surface
/// type — so opening a pager in `b` (focus becomes `Pager`, not `FileList`)
/// still reports `b` focused / `a` not, which is what dims `a`. Regression:
/// `render_left_list` used `right_column_focused` (FileList-only), so `a`
/// un-dimmed the instant `b` opened a pager/editor.
#[test]
fn column_focused_tracks_any_surface_in_b() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().join("work");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "x").unwrap();
        let mut app = App::test_app(dir);
        let d = app.state.left.listing.dir.clone();
        app.open_second_commander_at(&d); // b focused
        let mut pb = PagerView::new_plain("b-pager", vec!["y".to_string()]);
        pb.mount = crate::ui::pager::Mount::TopPane;
        app.install_top_pager(pb); // focus is now Pager(TopPane) in b
        app.recompute_focus();

        assert!(
            !app.right_column_focused(),
            "right_column_focused is FileList-only — false with b's pager up"
        );
        assert!(
            app.column_focused(state::Side::Right),
            "but b's column still owns the keyboard"
        );
        assert!(
            !app.column_focused(state::Side::Left),
            "a is not focused → it stays dimmed"
        );
    });
}

/// `^d` (Action::Quit) quits with the two-tap "press again to quit" confirm and
/// NEVER closes the second commander — so `^d^d` with `b` open leaves `b` open
/// when the session saves, and `-r` restores the split. Closing `b` is `^s x`.
#[test]
fn ctrl_d_quits_and_keeps_the_second_commander_open() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().join("work");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "x").unwrap();
        let mut app = App::test_app(dir);
        let d = app.state.left.listing.dir.clone();
        app.open_second_commander_at(&d);
        // Focus the LEFT column — the case the user hit: `^d` from `a` used to
        // close `b`.
        app.apply(&Action::VsplitFocusLeft).unwrap();
        assert!(app.state.right.is_some());

        // First `^d` arms the quit confirm; `b` stays open, no quit yet.
        app.apply(&Action::Quit).unwrap();
        assert!(
            app.state.right.is_some(),
            "^d must not close the second commander"
        );
        assert!(
            !app.state.should_quit,
            "first quit tap only arms the confirm"
        );

        // Second `^d` quits — with `b` still open, so the session save captures it.
        app.apply(&Action::Quit).unwrap();
        assert!(app.state.should_quit, "second quit tap quits");
        assert!(
            app.state.right.is_some(),
            "b is still open at quit → session save persists right_cwd for -r"
        );
    });
}
