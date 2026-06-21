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

/// The context snapshot announces the running spyc's identity — its pid and
/// build (version + git SHA) — so an MCP client can detect a stale server and
/// name the process to restart.
#[test]
fn snapshot_context_announces_pid_and_version() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let app = App::test_app(tmp.path().to_path_buf());
        let ctx = app.snapshot_context();
        assert_eq!(ctx.pid, std::process::id(), "reports our own pid");
        assert_eq!(
            ctx.version,
            crate::VERSION,
            "reports the baked build string"
        );
        assert!(
            ctx.version.contains('(') && ctx.version.contains(')'),
            "version carries the git SHA in parens: {}",
            ctx.version
        );
    });
}

/// MCP telemetry: each `ToolCalled` bumps the cumulative per-tool tally the
/// `A` overlay renders, plus the aggregate `mcp:N/s` rate. A read tool the
/// socket thread serves still flows through here, so reads are counted too.
#[test]
fn mcp_tool_called_tallies_per_tool_counts() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(tmp.path().to_path_buf());
        for name in ["search_content", "search_content", "navigate_to"] {
            app.execute_mcp_command(crate::mcp_cmd::McpCommand::ToolCalled {
                name: name.to_string(),
            });
        }
        let calls = &app.view.activity.mcp_tool_calls;
        assert_eq!(calls.get("search_content"), Some(&2), "per-tool count");
        assert_eq!(calls.get("navigate_to"), Some(&1));
        assert_eq!(
            app.view.activity.live.mcp_reqs, 3,
            "aggregate rate counts every tools/call"
        );
    });
}

/// MCP follows focus: with a second commander focused, the context snapshot
/// the agent reads reports `b`'s cwd, and a mutating MCP command (set_filter)
/// targets `b` — not the left/primary column. (PR F: MCP focus-aware.)
#[test]
fn mcp_context_and_mutations_follow_the_focused_column() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let left_dir = tmp.path().join("left");
        let right_dir = tmp.path().join("right");
        std::fs::create_dir(&left_dir).unwrap();
        std::fs::create_dir(&right_dir).unwrap();
        std::fs::write(right_dir.join("a.rs"), "").unwrap();
        let mut app = App::test_app(left_dir);
        app.open_second_commander_at(&right_dir); // b open + focused

        // Read side: the snapshot the agent reads reports b's cwd.
        let want = std::fs::canonicalize(&right_dir).unwrap();
        assert_eq!(
            app.snapshot_context().cwd,
            want,
            "get_spyc_context reports the focused (b) column's cwd"
        );

        // Mutate side: set_filter targets b, leaving a untouched.
        app.execute_mcp_command(crate::mcp_cmd::McpCommand::SetFilter {
            pattern: Some("*.rs".to_string()),
        });
        assert_eq!(
            app.state.right.as_ref().unwrap().temp_filter.as_deref(),
            Some("*.rs"),
            "set_filter applies to the focused (b) column"
        );
        assert!(
            app.state.left.temp_filter.is_none(),
            "the left column's filter is untouched"
        );
    });
}

/// MCP `create_worktree` makes a git worktree off the FOCUSED column's repo
/// and replies `{branch, path}` — the entry point a skill uses to spin up a
/// worktree to work in `b`.
#[test]
fn mcp_create_worktree_makes_a_worktree_off_the_focused_repo() {
    let tmp = tempfile::tempdir().unwrap();
    let run_git = |dir: &std::path::Path, args: &[&str]| {
        let ok = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@x")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@x")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .status()
            .expect("spawn git")
            .success();
        assert!(ok, "git {args:?} failed");
    };
    crate::state::with_state_root(tmp.path(), || {
        let repo = std::fs::canonicalize(tmp.path()).unwrap().join("repo");
        std::fs::create_dir(&repo).unwrap();
        run_git(&repo, &["init", "-q", "--initial-branch=main"]);
        std::fs::write(repo.join("f.txt"), "v1\n").unwrap();
        run_git(&repo, &["add", "f.txt"]);
        run_git(&repo, &["commit", "-q", "-m", "v1"]);

        let mut app = App::test_app(repo.clone());
        app.state.update_repo_root(state::Side::Left, &repo);

        let resp = app.execute_mcp_command(crate::mcp_cmd::McpCommand::CreateWorktree {
            branch: "mcp-wt".to_string(),
        });
        match resp {
            crate::mcp_cmd::McpResponse::Ok { message } => {
                let v: serde_json::Value = serde_json::from_str(&message).unwrap();
                assert_eq!(v["branch"], "mcp-wt");
                let path = std::path::PathBuf::from(v["path"].as_str().unwrap());
                assert!(path.is_dir(), "worktree dir created: {path:?}");
                assert!(path.join("f.txt").exists(), "branch tree checked out");
            }
            crate::mcp_cmd::McpResponse::Error { message } => {
                panic!("create_worktree errored: {message}")
            }
        }
    });
}

/// Empty branch is rejected before touching git.
#[test]
fn mcp_create_worktree_rejects_empty_branch() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(tmp.path().to_path_buf());
        let resp = app.execute_mcp_command(crate::mcp_cmd::McpCommand::CreateWorktree {
            branch: "   ".to_string(),
        });
        assert!(
            matches!(resp, crate::mcp_cmd::McpResponse::Error { .. }),
            "blank branch is an error"
        );
    });
}

/// MCP `remove_worktree` tears down a clean worktree — but refuses one a column
/// is currently open in (removing it would strand the column on a deleted dir).
#[test]
fn mcp_remove_worktree_tears_down_and_guards_occupied() {
    use crate::mcp_cmd::{McpCommand, McpResponse};
    let tmp = tempfile::tempdir().unwrap();
    let run_git = |dir: &std::path::Path, args: &[&str]| {
        let ok = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@x")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@x")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .status()
            .expect("spawn git")
            .success();
        assert!(ok, "git {args:?} failed");
    };
    crate::state::with_state_root(tmp.path(), || {
        let repo = std::fs::canonicalize(tmp.path()).unwrap().join("repo");
        std::fs::create_dir(&repo).unwrap();
        run_git(&repo, &["init", "-q", "--initial-branch=main"]);
        std::fs::write(repo.join("f.txt"), "v1\n").unwrap();
        run_git(&repo, &["add", "f.txt"]);
        run_git(&repo, &["commit", "-q", "-m", "v1"]);

        let mut app = App::test_app(repo.clone());
        app.state.update_repo_root(state::Side::Left, &repo);

        // Create, then capture its path.
        let created = app.execute_mcp_command(McpCommand::CreateWorktree {
            branch: "teardown-wt".to_string(),
        });
        let path = match created {
            McpResponse::Ok { message } => {
                let v: serde_json::Value = serde_json::from_str(&message).unwrap();
                std::path::PathBuf::from(v["path"].as_str().unwrap().to_string())
            }
            McpResponse::Error { message } => panic!("create failed: {message}"),
        };
        assert!(path.is_dir(), "worktree created");

        // Open `b` inside it → remove must refuse (would strand b).
        app.open_second_commander_at(&path);
        let occ = app.execute_mcp_command(McpCommand::RemoveWorktree {
            path: path.display().to_string(),
        });
        assert!(
            matches!(occ, McpResponse::Error { .. }),
            "refuses to remove a worktree a column is open in"
        );
        assert!(path.is_dir(), "still there while occupied");

        // Close `b`, then removal succeeds and the dir is gone.
        app.close_second_commander();
        let ok = app.execute_mcp_command(McpCommand::RemoveWorktree {
            path: path.display().to_string(),
        });
        assert!(
            matches!(ok, McpResponse::Ok { .. }),
            "removes a clean, unoccupied worktree: {ok:?}"
        );
        assert!(!path.exists(), "worktree dir removed");
    });
}

/// MCP `open_worktree` opens column `b` at the given dir (the "work in it in b"
/// step), so `cur()` then resolves to `b`.
#[test]
fn mcp_open_worktree_opens_column_b() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let a = std::fs::canonicalize(tmp.path()).unwrap().join("a");
        let wt = std::fs::canonicalize(tmp.path()).unwrap().join("wt");
        std::fs::create_dir(&a).unwrap();
        std::fs::create_dir(&wt).unwrap();
        let mut app = App::test_app(a);
        assert!(app.state.right.is_none(), "single column to start");

        let resp = app.execute_mcp_command(crate::mcp_cmd::McpCommand::OpenWorktree {
            path: wt.display().to_string(),
        });
        assert!(
            matches!(resp, crate::mcp_cmd::McpResponse::Ok { .. }),
            "open ok: {resp:?}"
        );
        let b = app.state.right.as_ref().expect("column b opened");
        assert_eq!(b.listing.dir, wt, "b opened at the worktree");
        assert_eq!(app.focused_side(), state::Side::Right, "focus moved to b");
    });
}

/// A non-directory path is an error and leaves the layout unchanged.
#[test]
fn mcp_open_worktree_rejects_non_dir() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(tmp.path().to_path_buf());
        let resp = app.execute_mcp_command(crate::mcp_cmd::McpCommand::OpenWorktree {
            path: tmp.path().join("does-not-exist").display().to_string(),
        });
        assert!(matches!(resp, crate::mcp_cmd::McpResponse::Error { .. }));
        assert!(app.state.right.is_none(), "b not opened on error");
    });
}

/// A `!` capture runs in the FOCUSED column's dir — `!touch foo` with `b`
/// focused lands in `b`, not `a`/PROJECT_HOME. Regression: `start_capture`
/// hardcoded `state.left.listing.dir`.
#[test]
fn capture_runs_in_the_focused_columns_dir() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let a = std::fs::canonicalize(tmp.path()).unwrap().join("a");
        let b = std::fs::canonicalize(tmp.path()).unwrap().join("b");
        std::fs::create_dir(&a).unwrap();
        std::fs::create_dir(&b).unwrap();
        let mut app = App::test_app(a.clone());
        app.open_second_commander_at(&b); // b focused
        assert_eq!(app.focused_side(), state::Side::Right);

        app.start_capture("touch marker.txt", "touch", "!touch marker.txt");

        // `touch` is near-instant; poll briefly for the spawned subprocess.
        let in_b = b.join("marker.txt");
        for _ in 0..200 {
            if in_b.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(in_b.exists(), "! ran in b (focused) → b/marker.txt");
        assert!(
            !a.join("marker.txt").exists(),
            "did NOT run in a / PROJECT_HOME"
        );
    });
}

/// Dual fs-watch: with a second commander open, the fs-event path predicates
/// recognize column `b`'s tree + gitdir too — so `b`'s working-tree edits and
/// index/HEAD changes drive a refresh, not just the ≤1 s poll PR E left.
#[test]
fn fs_predicates_recognize_the_second_column() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let a = std::fs::canonicalize(tmp.path()).unwrap().join("a");
        let b = std::fs::canonicalize(tmp.path()).unwrap().join("b");
        std::fs::create_dir(&a).unwrap();
        std::fs::create_dir(&b).unwrap();
        let mut app = App::test_app(a.clone());
        app.state.left.git_cache.current_gitdir = Some(a.join(".git"));
        app.open_second_commander_at(&b);
        app.state.right.as_mut().unwrap().git_cache.current_gitdir = Some(b.join(".git"));

        // b's tree + index/HEAD are recognized…
        assert!(app.is_listing_path(&b.join("src/main.rs")), "b tree file");
        assert!(app.is_gitdir_status_path(&b.join(".git/index")), "b index");
        assert!(app.is_gitdir_status_path(&b.join(".git/HEAD")), "b HEAD");
        // …and a's still are (both columns active)…
        assert!(app.is_listing_path(&a.join("lib.rs")), "a tree file");
        assert!(app.is_gitdir_status_path(&a.join(".git/index")), "a index");
        // …while b's .git housekeeping is still rejected.
        assert!(
            !app.is_gitdir_status_path(&b.join(".git/objects/ab/cd")),
            "b objects churn rejected"
        );
    });
}

/// `/` incremental search moves the FOCUSED column's cursor — searching in `b`
/// must not scroll `a`. Regression: the match application hardcoded `left`.
#[test]
fn slash_search_targets_the_focused_column() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let left_dir = tmp.path().join("left");
        let right_dir = tmp.path().join("right");
        std::fs::create_dir(&left_dir).unwrap();
        std::fs::create_dir(&right_dir).unwrap();
        let mut app = App::test_app(left_dir);
        app.open_second_commander_at(&right_dir); // b focused

        // Deterministic rows for b; a starts at cursor 0.
        let r = app.state.right.as_mut().unwrap();
        r.rows = vec![
            RowData {
                path: right_dir.join("alpha"),
                display: "alpha".to_string(),
                kind: EntryKind::File,
            },
            RowData {
                path: right_dir.join("zebra"),
                display: "zebra".to_string(),
                kind: EntryKind::File,
            },
        ];
        r.cursor.index = 0;

        app.apply(&Action::SearchPrompt).unwrap(); // saves b's cursor (0)
        for c in "zeb".chars() {
            app.handle_key(key(c)).unwrap();
        }
        assert_eq!(
            app.state.right.as_ref().unwrap().cursor.index,
            1,
            "/ moved b's cursor to the match"
        );
        assert_eq!(
            app.state.left.cursor.index, 0,
            "a's cursor is untouched by a search in b"
        );
    });
}

/// Dual git (PR E): each column resolves + shows its OWN repo's markers and
/// branch — `b` in a different repo doesn't inherit `a`'s state, and the two
/// don't collide on a single generation. (Sync walk: test harness has no
/// worker, so `git_file_statuses_cached` runs inline.)
#[test]
fn dual_git_each_column_shows_its_own_repo() {
    use std::path::Path;
    let tmp = tempfile::tempdir().unwrap();
    let run_git = |dir: &Path, args: &[&str]| {
        let ok = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@x")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@x")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .status()
            .expect("spawn git")
            .success();
        assert!(ok, "git {args:?} failed in {dir:?}");
    };
    let mk_repo = |name: &str| {
        let d = std::fs::canonicalize(tmp.path()).unwrap().join(name);
        std::fs::create_dir(&d).unwrap();
        run_git(&d, &["init", "-q", "--initial-branch=main"]);
        std::fs::write(d.join("file.txt"), "v1\n").unwrap();
        run_git(&d, &["add", "file.txt"]);
        run_git(&d, &["commit", "-q", "-m", "v1"]);
        d
    };
    crate::state::with_state_root(tmp.path(), || {
        let repo_a = mk_repo("a"); // stays clean
        let repo_b = mk_repo("b");
        std::fs::write(repo_b.join("file.txt"), "v2\n").unwrap(); // b is dirty

        let mut app = App::test_app(repo_a.clone());
        app.state.update_repo_root(state::Side::Left, &repo_a);
        app.state.refresh_git_state(); // a: clean
        app.open_second_commander_at(&repo_b); // resolves + refreshes b (dirty)

        assert!(
            app.state.left.git.files.is_empty(),
            "a (clean) has no markers: {:?}",
            app.state.left.git.files
        );
        assert_eq!(
            app.state.left.git.info.as_deref(),
            Some("main"),
            "a clean branch"
        );
        let b = app.state.right.as_ref().expect("b open");
        assert!(
            b.git.files.contains_key("file.txt"),
            "b shows ITS own M marker, not a's: {:?}",
            b.git.files
        );
        assert!(
            b.git.info.as_deref().is_some_and(|s| s.ends_with('*')),
            "b's branch shows dirty: {:?}",
            b.git.info
        );
    });
}

/// `O` (new file) creates the file in the FOCUSED column's dir — so a worktree
/// in `b` gets the file, not `a`'s dir. Regression: the create path joined
/// `state.left.listing.dir`.
#[test]
fn new_file_creates_in_the_focused_column() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let a = tmp.path().join("a");
        let b = tmp.path().join("b");
        std::fs::create_dir(&a).unwrap();
        std::fs::create_dir(&b).unwrap();
        let mut app = App::test_app(a.clone());
        app.open_second_commander_at(&b); // b focused
        assert_eq!(app.focused_side(), state::Side::Right);

        // O → NewFile prompt, type a name, submit.
        let mut p = Prompt::simple(PromptKind::NewFile, "new file: ");
        p.buffer = "made_in_b.txt".to_string();
        app.state.mode = Mode::Prompting(p);
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()))
            .unwrap();

        assert!(
            std::fs::canonicalize(&b)
                .unwrap()
                .join("made_in_b.txt")
                .exists(),
            "O creates the file in b (the focused column)"
        );
        assert!(
            !a.join("made_in_b.txt").exists(),
            "the file is NOT created in a"
        );
    });
}

/// `g w` (JumpWorktreeRoot) targets the FOCUSED column's own repo/worktree
/// root — `b` jumps to b's root, not a's. (PROJECT_HOME, jumped by `g h`,
/// stays the overall anchor.)
#[test]
fn gw_targets_the_focused_columns_worktree_root() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir_a = tmp.path().join("a");
        let dir_b = tmp.path().join("b");
        std::fs::create_dir(&dir_a).unwrap();
        std::fs::create_dir(&dir_b).unwrap();
        let mut app = App::test_app(dir_a);
        app.open_second_commander_at(&dir_b); // b focused

        let root_a = tmp.path().join("repoA");
        let root_b = tmp.path().join("repoB");
        app.state.left.git_cache.current_repo_root = Some(root_a);
        app.state
            .right
            .as_mut()
            .unwrap()
            .git_cache
            .current_repo_root = Some(root_b.clone());

        let fx = app.apply(&Action::JumpWorktreeRoot).unwrap();
        assert!(
            fx.iter()
                .any(|e| matches!(e, Effect::ChangeDir { path, .. } if *path == root_b)),
            "g w targets b's (focused) worktree root, not a's: {fx:?}"
        );
    });
}

/// grep `F` / find walk the FOCUSED column's worktree root via `tool_root`:
/// with `b` focused in a separate repo, the F-finder scopes to b's root — not
/// a's, not PROJECT_HOME. (MCP search shares the same root, exercised in
/// `mcp::tests::search_root_overrides_project_home`.)
#[test]
fn find_picker_walks_the_focused_columns_worktree_root() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir_a = tmp.path().join("a");
        let dir_b = tmp.path().join("b");
        std::fs::create_dir(&dir_a).unwrap();
        std::fs::create_dir(&dir_b).unwrap();
        let mut app = App::test_app(dir_a);
        app.open_second_commander_at(&dir_b); // b focused

        let root_a = tmp.path().join("repoA");
        let root_b = tmp.path().join("repoB");
        std::fs::create_dir(&root_a).unwrap();
        std::fs::create_dir(&root_b).unwrap();
        app.state.left.git_cache.current_repo_root = Some(root_a);
        app.state
            .right
            .as_mut()
            .unwrap()
            .git_cache
            .current_repo_root = Some(root_b.clone());

        app.open_find_picker();
        let root = app
            .runtime
            .find_picker
            .as_ref()
            .expect("finder open")
            .root
            .clone();
        assert_eq!(root, root_b, "F walks b's (focused) worktree root");
    });
}

/// Harpoon is per-column, pinned to each column's worktree: an entry appended
/// in `a` does NOT bleed into `b`'s list when `b` is a different worktree, and
/// vice versa. Harpoon stores ABSOLUTE paths, so a shared list would jump `b`
/// into `a`'s copy of a file — the wrong worktree/branch. Each column keeps
/// its own bookmarks keyed by `harpoon_root`.
#[test]
fn harpoon_is_per_column_pinned_to_each_worktree() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir_a = std::fs::canonicalize(tmp.path()).unwrap().join("a");
        let dir_b = std::fs::canonicalize(tmp.path()).unwrap().join("b");
        std::fs::create_dir(&dir_a).unwrap();
        std::fs::create_dir(&dir_b).unwrap();
        let fa = dir_a.join("fa.txt");
        let fb = dir_b.join("fb.txt");
        std::fs::write(&fa, "").unwrap();
        std::fs::write(&fb, "").unwrap();

        let mut app = App::test_app(dir_a.clone());
        // `a`'s worktree root enables + keys its harpoon.
        app.state.left.git_cache.current_repo_root = Some(dir_a);
        app.reconcile_harpoon();
        // Cursor on fa.txt → Ha appends it to a's list.
        app.state.left.rows = vec![RowData {
            path: fa.clone(),
            display: "fa.txt".into(),
            kind: EntryKind::File,
        }];
        app.state.left.cursor.index = 0;
        app.harpoon_append();

        // Open `b` in its own worktree, harpoon fb.txt there.
        app.open_second_commander_at(&dir_b);
        app.state
            .right
            .as_mut()
            .unwrap()
            .git_cache
            .current_repo_root = Some(dir_b.clone());
        app.reconcile_harpoon();
        app.state.right.as_mut().unwrap().rows = vec![RowData {
            path: fb.clone(),
            display: "fb.txt".into(),
            kind: EntryKind::File,
        }];
        app.state.right.as_mut().unwrap().cursor.index = 0;
        app.harpoon_append();

        let a = app.state.left.harpoon.as_ref().expect("a harpoon loaded");
        let b = app
            .state
            .right
            .as_ref()
            .unwrap()
            .harpoon
            .as_ref()
            .expect("b harpoon loaded");
        assert!(
            a.contains(&fa) && !a.contains(&fb),
            "a's list = a's file only"
        );
        assert!(
            b.contains(&fb) && !b.contains(&fa),
            "b's list = b's file only — no cross-worktree bleed"
        );
        assert_ne!(
            a.project, b.project,
            "each column's harpoon keys to its OWN worktree root"
        );
    });
}
