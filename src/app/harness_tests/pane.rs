use super::*;

// ── pane/pty workflow smoke tests (testing campaign, cluster 4) ──
// These spawn a real `cat` pane (the established pattern — cat blocks on
// stdin, keeping the pty alive for the test) and drive the App-level pane
// handlers. A handful of smoke tests per the charter; the pure decision
// logic (decide_scroll_source, PaneTabs index math) is unit-tested in
// src/pane.

/// Opening a bare pane tab spawns it in the *current listing dir*
/// (deliberately not PROJECT_HOME — see `open_pane_tab` docs) and moves
/// focus into the pane.
#[test]
fn open_pane_tab_spawns_in_listing_dir_and_focuses_pane() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().join("work");
        std::fs::create_dir(&dir).unwrap();
        let mut app = App::test_app(dir.clone());
        assert!(app.runtime.pane_tabs.is_none());
        app.open_pane_tab("cat");
        let tabs = app.runtime.pane_tabs.as_ref().expect("a tab was opened");
        assert_eq!(tabs.active_info().cwd, app.state.left.listing.dir);
        assert_eq!(tabs.active_info().cwd, dir);
        assert_eq!(
            app.state.focus,
            state::Focus::Pane,
            "opening a pane focuses it"
        );
    });
}

/// `^a z` zooms the *active* region: with the list focused it zooms the
/// list (`TopList`), with the pane focused it zooms the pane (`BottomPane`).
/// Focus is left unchanged across the toggle (it already names the zoomed
/// region — this is the fix for the old "zoom always grabbed the bottom
/// pane" bug), and `pane_height_pct` is preserved across the round-trip.
#[test]
fn zoom_targets_the_active_region() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().join("work");
        std::fs::create_dir(&dir).unwrap();
        let mut app = App::test_app(dir);
        app.open_pane_tab("cat");
        let pct_before = app.state.pane.pane_height_pct;

        // List focused → zoom the list; focus stays on the list.
        app.state.focus = state::Focus::FileList;
        app.toggle_pane_zoom();
        assert_eq!(
            app.state.pane.zoom,
            state::ZoomTarget::TopList,
            "list focused → zoom the list"
        );
        assert_eq!(
            app.state.focus,
            state::Focus::FileList,
            "list-zoom leaves focus on the list"
        );
        app.toggle_pane_zoom();
        assert_eq!(
            app.state.pane.zoom,
            state::ZoomTarget::None,
            "second toggle clears the zoom"
        );

        // Pane focused → zoom the pane; focus stays on the pane.
        app.state.focus = state::Focus::Pane;
        app.toggle_pane_zoom();
        assert_eq!(
            app.state.pane.zoom,
            state::ZoomTarget::BottomPane,
            "pane focused → zoom the pane"
        );
        assert_eq!(
            app.state.focus,
            state::Focus::Pane,
            "pane-zoom leaves focus on the pane"
        );
        app.toggle_pane_zoom();
        assert_eq!(app.state.pane.zoom, state::ZoomTarget::None);

        assert_eq!(
            app.state.pane.pane_height_pct, pct_before,
            "zoom must not disturb pane_height_pct"
        );
    });
}

/// While zoomed, `^a j` / `^a k` (`set_pane_focus`) are inert — you can't
/// focus the collapsed/off-screen region; only `^a z` exits the zoom.
#[test]
fn focus_switch_is_inert_while_zoomed() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().join("work");
        std::fs::create_dir(&dir).unwrap();
        let mut app = App::test_app(dir);
        app.open_pane_tab("cat");

        // Zoom the pane (pane focused → BottomPane).
        app.state.focus = state::Focus::Pane;
        app.toggle_pane_zoom();
        assert_eq!(app.state.pane.zoom, state::ZoomTarget::BottomPane);

        // `^a k` would normally move focus to the list — inert while zoomed.
        app.set_pane_focus(false);
        assert_eq!(
            app.state.focus,
            state::Focus::Pane,
            "focus must not move while zoomed"
        );

        // Un-zoom, then the same call moves focus as usual.
        app.toggle_pane_zoom();
        app.set_pane_focus(false);
        assert_eq!(
            app.state.focus,
            state::Focus::FileList,
            "focus moves again once un-zoomed"
        );
    });
}

/// Creating a pane while the list is zoomed reveals the split — otherwise the
/// new pane would be created off-screen behind a fullscreen list.
#[test]
fn new_pane_reveals_a_list_zoomed_session() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().join("work");
        std::fs::create_dir(&dir).unwrap();
        let mut app = App::test_app(dir);
        app.open_pane_tab("cat");
        // List focused → zoom the list (pane collapses to its tab bar).
        app.state.focus = state::Focus::FileList;
        app.toggle_pane_zoom();
        assert_eq!(app.state.pane.zoom, state::ZoomTarget::TopList);

        // Creating another pane clears the list-zoom so the pane is visible.
        app.open_pane_tab("cat");
        assert_eq!(
            app.state.pane.zoom,
            state::ZoomTarget::None,
            "a new pane reveals the split"
        );
    });
}

/// `^a <n>` from a fullscreen list (`TopList` zoom) fullscreens the chosen
/// pane (switch + flip to `BottomPane` zoom), navigating between fullscreen
/// views via the bottom tab bar.
#[test]
fn pane_index_fullscreens_from_list_zoom() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().join("work");
        std::fs::create_dir(&dir).unwrap();
        let mut app = App::test_app(dir);
        app.open_pane_tab("cat");
        app.open_pane_tab("cat"); // two tabs
        app.state.focus = state::Focus::FileList;
        app.toggle_pane_zoom();
        assert_eq!(app.state.pane.zoom, state::ZoomTarget::TopList);

        app.apply(&Action::PaneTabByIndex(1)).unwrap();
        assert_eq!(
            app.state.pane.zoom,
            state::ZoomTarget::BottomPane,
            "^a <n> fullscreens the chosen pane from a zoomed list"
        );
        assert_eq!(app.state.focus, state::Focus::Pane);
    });
}

/// Regression (fix:): opening `^a v` on a *plain* pane with empty
/// scrollback must (a) flash the accurate hint — not the old
/// "this app keeps its own history", false for a fresh shell, and not
/// the "scroll: on" flash that used to clobber it — and (b) stay live
/// rather than trap the user in an empty, un-scrollable pager.
#[test]
fn empty_scrollback_flashes_hint_and_stays_live() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().join("work");
        std::fs::create_dir(&dir).unwrap();
        let mut app = App::test_app(dir);
        app.open_pane_tab("cat"); // fresh cat: no output → empty scrollback
        app.open_pane_scroll_pager();
        assert_eq!(
            app.flash_text(),
            Some("no terminal scrollback captured"),
            "empty scrollback should flash the accurate hint"
        );
        assert!(
            app.view.scroll_pager.is_none(),
            "empty scrollback must not enter scroll mode (no dead-end pager)"
        );
    });
}

/// An inline agent pane (claude/agy) whose transcript scrollback is toggled
/// off reaches the same empty-scrollback path — but its history *is*
/// recoverable via the transcript hook, so the hint must point there, not
/// claim the history is lost. The tab's command detects as `claude` (→ a
/// transcript spec, default-off) while actually running `cat` so it spawns
/// in a test.
#[test]
fn empty_scrollback_agent_pane_points_to_transcript() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().join("work");
        std::fs::create_dir(&dir).unwrap();
        let mut app = App::test_app(dir.clone());
        let wake = app.make_pane_wake();
        let pane = crate::pane::Pane::spawn("cat", 24, 80, &dir, &app.view.context_path, wake)
            .expect("spawn cat");
        let entry =
            crate::pane::tabs::TabEntry::new(pane, crate::pane::tabs::TabInfo::new("claude", dir));
        app.runtime.pane_tabs = Some(crate::pane::tabs::PaneTabs::new(entry));
        app.open_pane_scroll_pager();
        assert_eq!(
            app.flash_text(),
            Some(
                "no terminal scrollback — claude keeps its history in a transcript (toggle it on)"
            ),
            "an inline agent with transcript off should point at its transcript"
        );
        assert!(
            app.view.scroll_pager.is_none(),
            "empty scrollback must not enter scroll mode, agent or not"
        );
    });
}

/// `^a n` / `^a p` cycle tabs on *every* chord, including back-to-back with
/// no loop iteration between — a routing-level guard for the "rapid `^a-n`
/// eats the command" class. Exercises the full `handle_key` path with the
/// pane focused; a regression that routed the chord's second key to the pane
/// child instead of completing the chord would fail here. (The reported
/// real-world flake is in live event delivery, which a unit test can't
/// reproduce; this at least locks the synchronous routing.)
#[test]
fn rapid_pane_next_prev_chords_each_switch_tabs() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().join("w");
        std::fs::create_dir(&dir).unwrap();
        let mut app = App::test_app(dir);
        app.open_pane_tab("cat");
        app.open_pane_tab("cat");
        app.open_pane_tab("cat"); // 3 tabs; pane focused
        // Ctrl held through the second key — the real fast-typing case
        // (`^a ^n` / `^a ^p`), which used to be eaten by the generic Ctrl
        // block. End-to-end guard through the full handle_key path.
        let ctrl_a = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL);
        let n = KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL);
        let p = KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL);
        let idx = |app: &App| app.runtime.pane_tabs.as_ref().unwrap().active_index();

        let mut prev = idx(&app);
        for i in 1..=4 {
            app.handle_key(ctrl_a).unwrap();
            app.handle_key(n).unwrap();
            assert_eq!(idx(&app), (prev + 1) % 3, "^a n #{i} must advance one tab");
            prev = idx(&app);
        }
        for i in 1..=4 {
            app.handle_key(ctrl_a).unwrap();
            app.handle_key(p).unwrap();
            assert_eq!(
                idx(&app),
                (prev + 2) % 3,
                "^a p #{i} must step back one tab"
            );
            prev = idx(&app);
        }
    });
}

// ── background-task workflow smoke tests (testing campaign, cluster 5) ──
// `BackgroundTask` owns a live PtyHost (can't be built without forking), so
// these drive the real `! `-capture path with `cat` (blocks on stdin, stays
// Running). The pure helpers (id alloc, glyph, counts) are unit-tested in
// src/app/tasks.rs.

/// `^Z` from a streaming capture backgrounds it (keeps a task entry, closes
/// the pager); `:fg` re-attaches it as the live capture and removes it from
/// the list.
#[test]
fn capture_backgrounds_with_ctrl_z_then_foregrounds_with_fg() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().join("w");
        std::fs::create_dir(&dir).unwrap();
        let mut app = App::test_app(dir);
        app.start_capture("cat", "cat", "cat"); // real running capture
        assert!(app.runtime.pending_capture.is_some(), "capture is live");
        assert!(app.view.pager.is_some(), "streaming pager is open");

        app.background_capture(); // ^Z
        assert!(
            app.runtime.pending_capture.is_none(),
            "no live capture after ^Z"
        );
        assert!(app.view.pager.is_none(), "pager closed on background");
        assert_eq!(
            app.runtime.background_tasks.tasks.len(),
            1,
            "the task entry is kept"
        );
        assert!(matches!(
            app.runtime.background_tasks.tasks[0].status,
            TaskStatus::Running
        ));
        assert_eq!(
            app.flash_text(),
            Some("task #1 backgrounded — :fg to resume")
        );

        app.foreground_task(None); // :fg
        assert!(
            app.runtime.pending_capture.is_some(),
            ":fg re-attaches the capture"
        );
        assert!(
            app.runtime.background_tasks.tasks.is_empty(),
            ":fg removes it from the task list"
        );
        assert!(app.view.pager.is_some(), ":fg reopens the streaming pager");
    });
}

/// `gB` / `:task` views a backgrounded task WITHOUT taking ownership: the
/// task stays in the list, gets marked viewed (clearing its unread divider),
/// and the pager tracks its id.
#[test]
fn open_task_viewer_views_without_taking_ownership() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().join("w");
        std::fs::create_dir(&dir).unwrap();
        let mut app = App::test_app(dir);
        app.start_capture("cat", "cat", "cat");
        app.background_capture();
        // Mark unread so we can prove the view clears it.
        app.runtime.background_tasks.tasks[0].has_unread_output = true;

        app.open_task_viewer(None); // gB / :task
        assert_eq!(
            app.runtime.background_tasks.tasks.len(),
            1,
            "viewing must NOT take ownership"
        );
        let t = &app.runtime.background_tasks.tasks[0];
        assert!(t.viewed_in_task_viewer, "task is marked viewed");
        assert!(!t.has_unread_output, "viewing clears the unread divider");
        let pager = app.view.pager.as_ref().expect("task viewer pager opened");
        assert_eq!(pager.task_id, Some(1), "pager tracks the viewed task id");
    });
}

/// `[t` / `]t` cycle the task viewer across tasks with wraparound, viewing
/// each without taking ownership.
#[test]
fn cycle_task_viewer_wraps_across_tasks() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().join("w");
        std::fs::create_dir(&dir).unwrap();
        let mut app = App::test_app(dir);
        app.start_capture("cat", "cat", "cat");
        app.background_capture(); // task #1
        app.start_capture("cat", "cat", "cat");
        app.background_capture(); // task #2
        assert_eq!(app.runtime.background_tasks.tasks.len(), 2);

        let viewed = |app: &App| app.view.pager.as_ref().and_then(|v| v.task_id);
        app.open_task_viewer(None); // most-recent → #2
        assert_eq!(viewed(&app), Some(2));
        app.cycle_task_viewer(1); // forward wraps → #1
        assert_eq!(viewed(&app), Some(1));
        app.cycle_task_viewer(1); // → #2
        assert_eq!(viewed(&app), Some(2));
        app.cycle_task_viewer(-1); // back → #1
        assert_eq!(viewed(&app), Some(1));
        // Cycling is view-only — both tasks remain.
        assert_eq!(app.runtime.background_tasks.tasks.len(), 2);
    });
}

// ── quick-select dispatch + path-jump (testing campaign, cluster 6) ──
// The scanner is unit-tested in src/pane/quick_select.rs and the kind ×
// intent action matrix in src/app/quick_select.rs; these cover the overlay
// state machine + the path leaf's *not-found* branch. The successful jump
// executes `chdir` → `std::env::set_current_dir` (a process-global mutation
// that races the parallel test runner — the codebase keeps unit tests
// chdir-free), and the yank / URL-open / git-show leaves are impure
// (clipboard / OS opener / git), so those are covered at the pure-matrix
// level rather than executed here.

/// A path jump to a nonexistent path flashes and does NOT chdir.
#[test]
fn jump_to_pane_path_missing_flashes_not_found() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let start = tmp.path().join("start");
        std::fs::create_dir(&start).unwrap();
        let mut app = App::test_app(start);
        let before = app.state.left.listing.dir.clone();
        app.jump_to_pane_path("/no/such/path/xyz123");
        assert_eq!(
            app.state.left.listing.dir, before,
            "missing path must not chdir"
        );
        assert!(
            app.flash_text()
                .unwrap_or_default()
                .contains("path not found"),
            "missing path flashes not-found"
        );
    });
}

fn qs_path_overlay(target: &str) -> crate::pane::quick_select::QuickSelect {
    crate::pane::quick_select::QuickSelect {
        matches: vec![crate::pane::quick_select::Match {
            text: target.to_string(),
            kind: crate::pane::quick_select::MatchKind::Path,
            label: "a".to_string(),
            row: 0,
            col: 0,
        }],
        pending_first: None,
        all_two_letter: false,
        open_intent: false,
    }
}

/// Esc closes the Quick Select overlay with no dispatch (no chdir).
#[test]
fn quick_select_esc_closes_without_dispatch() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let start = tmp.path().join("start");
        let target = tmp.path().join("target");
        std::fs::create_dir(&start).unwrap();
        std::fs::create_dir(&target).unwrap();
        let mut app = App::test_app(start);
        let before = app.state.left.listing.dir.clone();
        app.view.quick_select = Some(qs_path_overlay(target.to_str().unwrap()));
        app.handle_key(esc()).unwrap();
        assert!(app.view.quick_select.is_none(), "Esc closes the overlay");
        assert_eq!(app.state.left.listing.dir, before, "Esc dispatches nothing");
    });
}

/// An uppercase (open-intent) label on a Path match dispatches the *open*
/// (jump), not a yank — proving the lowercase-yank / uppercase-open split
/// end to end through the full key path. Targets a *missing* path so the
/// jump flashes and returns before `chdir` (a successful jump would
/// `set_current_dir` and race the parallel runner); a yank would not flash
/// "path not found".
#[test]
fn quick_select_uppercase_path_label_dispatches_open() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(tmp.path().to_path_buf());
        app.view.quick_select = Some(qs_path_overlay("/no/such/qs/path/zzz"));
        app.handle_key(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::empty()))
            .unwrap();
        assert!(
            app.view.quick_select.is_none(),
            "label commit closes the overlay"
        );
        assert!(
            app.flash_text()
                .unwrap_or_default()
                .contains("path not found"),
            "uppercase Path label dispatched the open (jump) → not-found flash"
        );
    });
}

/// In the 2-letter-label case, an uppercase first keystroke arms the sticky
/// open-intent bit and narrows (the overlay stays open for the 2nd key).
#[test]
fn quick_select_two_letter_uppercase_first_arms_open_intent() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(tmp.path().to_path_buf());
        let mk = |label: &str| crate::pane::quick_select::Match {
            text: "x".to_string(),
            kind: crate::pane::quick_select::MatchKind::Path,
            label: label.to_string(),
            row: 0,
            col: 0,
        };
        app.view.quick_select = Some(crate::pane::quick_select::QuickSelect {
            matches: vec![mk("aa"), mk("ab")],
            pending_first: None,
            all_two_letter: true,
            open_intent: false,
        });
        app.handle_key(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::empty()))
            .unwrap();
        let qs = app
            .view
            .quick_select
            .as_ref()
            .expect("overlay stays open after the first of two keys");
        assert_eq!(qs.pending_first, Some('a'));
        assert!(qs.open_intent, "uppercase first keystroke arms open intent");
    });
}

/// `^a x` on a tab whose child is still running confirms before closing —
/// killing a live agent would lose the session. `n` keeps it, `y` closes it.
/// (An exited tab closes silently, so it never reaches this prompt.)
#[test]
fn closing_a_running_tab_confirms_first() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().join("work");
        std::fs::create_dir(&dir).unwrap();
        let mut app = App::test_app(dir);
        app.open_pane_tab("cat"); // cat blocks on stdin → the tab stays live
        app.open_pane_tab("cat"); // two live tabs
        let count = |app: &App| app.runtime.pane_tabs.as_ref().map_or(0, |t| t.tabs().len());
        assert_eq!(count(&app), 2);

        // Closing the active (running) tab opens a confirm — nothing closes yet.
        app.close_active_tab();
        assert!(
            matches!(&app.state.mode, Mode::Prompting(p) if matches!(p.kind, PromptKind::ClosePane)),
            "closing a running tab opens the ClosePane confirm"
        );
        assert_eq!(count(&app), 2, "nothing closes before confirming");

        // `n` keeps the tab.
        app.handle_key(key('n')).unwrap();
        assert!(matches!(app.state.mode, Mode::Normal));
        assert_eq!(count(&app), 2, "n cancels the close");

        // `y` closes it.
        app.close_active_tab();
        app.handle_key(key('y')).unwrap();
        assert!(matches!(app.state.mode, Mode::Normal));
        assert_eq!(count(&app), 1, "y closes the running tab");
    });
}
