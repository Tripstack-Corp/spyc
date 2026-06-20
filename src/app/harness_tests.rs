//! Harness-driven tests for the App event loop / lifecycle, relocated from
//! mod.rs (800-LoC campaign).

use super::*;
use crate::app::effect::matchers::EffectSliceExt;
use crate::keymap::Action;
use crossterm::event::KeyModifiers;

fn key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::empty())
}

/// Acceptance: a fresh harness starts with a deterministic cwd,
/// listing, cursor, focus, and no pane/pager.
#[test]
fn fresh_harness_is_deterministic() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let app = App::test_app(std::path::PathBuf::from("/tmp/harness"));
        assert_eq!(app.state.focus, state::Focus::FileList);
        assert!(!app.state.pane_focused());
        assert!(matches!(app.state.mode, Mode::Normal));
        assert_eq!(app.state.left.cursor.index, 0);
        assert!(app.runtime.pane_tabs.is_none());
        assert!(app.view.pager.is_none());
        assert!(app.flash_text().is_none());
        assert_eq!(
            app.state.left.listing.dir,
            std::path::PathBuf::from("/tmp/harness")
        );
    });
}

/// Acceptance: the harness can apply an `Action` and observe the
/// resulting state (cursor movement here) plus a `PostAction`.
#[test]
fn apply_action_moves_cursor() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(std::path::PathBuf::from("/tmp/harness"));
        app.seed_rows(&["a", "b", "c"]);
        assert_eq!(app.state.left.cursor.index, 0);
        let post = app.apply(&Action::Down(1)).unwrap();
        assert_eq!(app.state.left.cursor.index, 1);
        assert!(post.is_empty());
        app.apply(&Action::Up(1)).unwrap();
        assert_eq!(app.state.left.cursor.index, 0);
    });
}

/// PR 5b: `gf`/`gF` emit a `ReadPaneText`/`GotoFile` effect (the pickable
/// read + navigation run in `run_effects`); `gF` sets `open_at_line`.
#[test]
fn goto_file_actions_emit_read_pane_text_pickable() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(std::path::PathBuf::from("/tmp/harness"));
        let fx = app.apply(&Action::GotoFile).unwrap();
        let (kind, then) = fx.read_pane_text().expect("gf emits one ReadPaneText");
        assert!(
            matches!(kind, PaneTextKind::Pickable(200)),
            "gf reads pickable(200)"
        );
        assert!(
            matches!(
                then,
                PaneTextSink::GotoFile {
                    open_at_line: false
                }
            ),
            "gf navigates without opening at a line"
        );
        let fx = app.apply(&Action::GotoFileLine).unwrap();
        let (kind, then) = fx.read_pane_text().expect("gF emits one ReadPaneText");
        assert!(
            matches!(kind, PaneTextKind::Pickable(200)),
            "gF reads pickable(200)"
        );
        assert!(
            matches!(then, PaneTextSink::GotoFile { open_at_line: true }),
            "gF opens the target at its line"
        );
    });
}

/// Acceptance: a `KeyEvent` routes through the full `handle_key`
/// path (resolver → route → dispatch) with no pane/overlay open.
#[test]
fn handle_key_routes_j_to_cursor_down() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(std::path::PathBuf::from("/tmp/harness"));
        app.seed_rows(&["a", "b", "c"]);
        app.handle_key(key('j')).unwrap();
        assert_eq!(
            app.state.left.cursor.index, 1,
            "j should move the cursor down"
        );
    });
}

/// PR4: the term-title compose + dedup stay loop-side. First call
/// emits a `SetTerminalTitle` effect; an unchanged title dedups to
/// `None` (so `term_title::set` only runs when the title changed).
#[test]
fn term_title_effect_emits_then_dedups() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(std::path::PathBuf::from("/tmp/harness"));
        assert!(
            matches!(
                app.term_title_effect(),
                Some(Effect::SetTerminalTitle { .. })
            ),
            "first call emits the title effect"
        );
        assert!(
            app.term_title_effect().is_none(),
            "unchanged title is deduped to None"
        );
    });
}

/// PR4: the send/pipe pre-pane guards still short-circuit with no
/// effect (and flash inline) when no pane is open.
#[test]
fn send_and_pipe_no_pane_emit_no_effect() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(std::path::PathBuf::from("/tmp/harness"));
        assert!(
            app.send_selection_to_pane().is_empty(),
            "no-pane send emits nothing"
        );
        assert!(
            app.pipe_content_to_pane(false).is_empty(),
            "no-pane pipe emits nothing"
        );
    });
}

fn esc() -> KeyEvent {
    KeyEvent::new(KeyCode::Esc, KeyModifiers::empty())
}

/// Routing: while a prompt is open, a printable key edits the prompt
/// buffer and does NOT move the list cursor (prompt wins).
#[test]
fn prompt_input_wins_over_list() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(std::path::PathBuf::from("/tmp/harness"));
        app.seed_rows(&["a", "b", "c"]);
        app.state.mode = Mode::Prompting(Prompt::simple(PromptKind::Jump, "jump: "));
        app.handle_key(key('x')).unwrap();
        assert_eq!(
            app.state.left.cursor.index, 0,
            "cursor must not move while prompting"
        );
        match &app.state.mode {
            Mode::Prompting(p) => assert_eq!(p.buffer, "x"),
            Mode::Normal => panic!("prompt should still be open"),
        }
    });
}

/// Routing: an Overlay-mounted in-app pager consumes normal keys —
/// `j` is handled by the pager, the list cursor stays put.
#[test]
fn overlay_pager_consumes_keys_not_list() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(std::path::PathBuf::from("/tmp/harness"));
        app.seed_rows(&["a", "b", "c"]);
        let lines: Vec<String> = (0..50).map(|i| format!("line {i}")).collect();
        app.view.pager = Some(PagerView::new_plain("t", lines));
        app.handle_key(key('j')).unwrap();
        assert_eq!(
            app.state.left.cursor.index, 0,
            "list cursor must not move with a pager open"
        );
        assert!(app.view.pager.is_some(), "pager stays open on j");
    });
}

/// Routing: a paste while the F-finder is open feeds the text into the
/// picker query (type-to-filter), not the bottom pane. Regression for the
/// key/paste dispatch asymmetry — keys hit the modal finder first
/// (`handle_find_picker_key`) but paste used to fall through to the pane arm
/// and land in claude/shell. Newlines are stripped (single-line fuzzy query).
#[test]
fn paste_into_open_finder_filters_not_pane() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(std::path::PathBuf::from("/tmp/harness"));
        app.runtime.find_picker = Some(super::find_picker::FindPicker {
            candidates: vec![std::path::PathBuf::from("src/app/foo.rs")],
            root: std::path::PathBuf::from("/tmp/harness"),
            query: String::new(),
            filtered: Vec::new(),
            selected: 0,
            limit: 200,
            walk_rx: None,
            walk_complete: true,
        });
        app.handle_paste("foo\n".to_string());
        let picker = app
            .runtime
            .find_picker
            .as_ref()
            .expect("finder stays open after paste");
        assert_eq!(
            picker.query, "foo",
            "paste feeds the query, newline stripped"
        );
        assert_eq!(picker.filtered.len(), 1, "query refilters the candidates");
    });
}

/// Routing: Esc on an open overlay pager closes it.
#[test]
fn esc_closes_overlay_pager() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(std::path::PathBuf::from("/tmp/harness"));
        app.view.pager = Some(PagerView::new_plain("t", vec!["a".to_string()]));
        app.handle_key(esc()).unwrap();
        assert!(app.view.pager.is_none(), "Esc should close the pager");
    });
}

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

/// A session saved while a second commander is open must NOT persist the split
/// shape — the commander can't be reconstructed on restore yet, so a saved
/// shape would restore an empty divider with nothing in `b`.
#[test]
fn session_does_not_persist_a_commander_split() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().join("work");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "x").unwrap();
        let mut app = App::test_app(dir);
        app.state.session_name = Some("vsplit-commander-test".to_string());

        let d = app.state.left.listing.dir.clone();
        app.open_second_commander_at(&d);
        assert!(app.state.vsplit.is_some() && app.state.right.is_some());

        app.save_session();
        let saved = crate::state::sessions::load_sessions()
            .into_iter()
            .find(|s| s.name == "vsplit-commander-test")
            .expect("session was saved");
        assert!(
            saved.vsplit.is_none(),
            "a commander split must not be persisted (would orphan on restore)"
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

/// `^d` (Action::QuitOrCloseCommander) is contextual: it closes the second
/// commander when one is open, otherwise quits — and the no-split quit keeps
/// its existing two-tap "press again to quit" confirm.
#[test]
fn ctrl_d_closes_second_commander_then_quits() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().join("work");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "x").unwrap();
        let mut app = App::test_app(dir);
        let d = app.state.left.listing.dir.clone();
        app.open_second_commander_at(&d);
        assert!(app.state.right.is_some());

        // With a commander open, `^d` closes it (does NOT quit).
        app.apply(&Action::QuitOrCloseCommander).unwrap();
        assert!(app.state.right.is_none(), "^d closes the second commander");
        assert!(
            !app.state.should_quit,
            "^d must not quit while a commander is open"
        );

        // No commander now → `^d` is the first quit tap (arms the confirm only).
        app.apply(&Action::QuitOrCloseCommander).unwrap();
        assert!(
            !app.state.should_quit,
            "first quit tap only arms the confirm"
        );

        // Second tap quits.
        app.apply(&Action::QuitOrCloseCommander).unwrap();
        assert!(app.state.should_quit, "second quit tap quits");
    });
}
