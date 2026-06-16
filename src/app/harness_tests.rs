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
        assert_eq!(app.state.cursor.index, 0);
        assert!(app.runtime.pane_tabs.is_none());
        assert!(app.view.pager.is_none());
        assert!(app.flash_text().is_none());
        assert_eq!(
            app.state.listing.dir,
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
        assert_eq!(app.state.cursor.index, 0);
        let post = app.apply(&Action::Down(1)).unwrap();
        assert_eq!(app.state.cursor.index, 1);
        assert!(post.is_empty());
        app.apply(&Action::Up(1)).unwrap();
        assert_eq!(app.state.cursor.index, 0);
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
        assert_eq!(app.state.cursor.index, 1, "j should move the cursor down");
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
            app.state.cursor.index, 0,
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
            app.state.cursor.index, 0,
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
        assert_eq!(tabs.active_info().cwd, app.state.listing.dir);
        assert_eq!(tabs.active_info().cwd, dir);
        assert_eq!(
            app.state.focus,
            state::Focus::Pane,
            "opening a pane focuses it"
        );
    });
}

/// `^a z` forces focus into the pane and sets the zoom flag; toggling
/// again restores the *prior* focus (FileList here) and clears the flag.
/// `pane_height_pct` is preserved across the round-trip.
#[test]
fn zoom_toggles_and_restores_prior_focus() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().join("work");
        std::fs::create_dir(&dir).unwrap();
        let mut app = App::test_app(dir);
        app.open_pane_tab("cat");
        // The user moves focus back to the list before zooming.
        app.state.focus = state::Focus::FileList;
        let pct_before = app.state.pane.pane_height_pct;

        app.toggle_pane_zoom();
        assert!(app.state.pane.pane_zoomed, "first toggle zooms on");
        assert_eq!(
            app.state.focus,
            state::Focus::Pane,
            "zoom-on forces pane focus"
        );

        app.toggle_pane_zoom();
        assert!(!app.state.pane.pane_zoomed, "second toggle zooms off");
        assert_eq!(
            app.state.focus,
            state::Focus::FileList,
            "unzoom restores the prior focus"
        );
        assert_eq!(
            app.state.pane.pane_height_pct, pct_before,
            "zoom must not disturb pane_height_pct"
        );
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
