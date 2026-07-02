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

/// A `J` jump to a typo'd / nonexistent path flashes an error rather than
/// silently no-op'ing (was `let _ = jump_to(..)`).
#[test]
fn jump_prompt_flashes_on_bad_path() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(tmp.path().to_path_buf());
        let mut p = Prompt::simple(PromptKind::Jump, "jump: ");
        p.buffer = "/no/such/path/xyz123".to_string();
        app.dispatch_prompt(p);
        assert!(
            app.flash_text().unwrap_or_default().contains("jump"),
            "a typo'd jump target must flash, not silently no-op"
        );
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

/// Worktree picker: `j`/`k` move the highlighted `picker_cursor` (not the pager
/// scroll), clamping at the ends, and don't close the picker. (Selection on
/// Enter/1-9 chdirs the focused column, which `set_current_dir`s — left out
/// here to keep the unit test chdir-free per the parallel-runner rule.)
#[test]
fn worktree_picker_jk_moves_highlight() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(std::path::PathBuf::from("/tmp/harness"));
        // Simulate an open worktree picker: 3 entries, cursor on the first.
        app.state.pending_worktrees = Some(vec![
            std::path::PathBuf::from("/tmp/wt0"),
            std::path::PathBuf::from("/tmp/wt1"),
            std::path::PathBuf::from("/tmp/wt2"),
        ]);
        let mut view =
            PagerView::new_plain("worktrees", vec!["wt0".into(), "wt1".into(), "wt2".into()]);
        view.picker_cursor = Some(0);
        app.view.pager = Some(view);

        let cursor = |app: &App| app.view.pager.as_ref().unwrap().picker_cursor;
        app.handle_key(key('j')).unwrap();
        assert_eq!(cursor(&app), Some(1), "j moves down");
        app.handle_key(key('j')).unwrap();
        assert_eq!(cursor(&app), Some(2));
        app.handle_key(key('j')).unwrap();
        assert_eq!(cursor(&app), Some(2), "clamps at the last row");
        app.handle_key(key('k')).unwrap();
        assert_eq!(cursor(&app), Some(1), "k moves up");
        assert!(app.view.pager.is_some(), "j/k keep the picker open");
        assert!(app.state.pending_worktrees.is_some());
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

/// `:notify test` fires every channel (bell + both desktop mechanisms) and
/// starts the visual border pulse regardless of `[notify]` config — the
/// on-demand diagnostic. A bare `:notify` is a usage no-op that fires nothing.
#[test]
fn notify_test_command_fires_all_channels() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(std::path::PathBuf::from("/tmp/harness"));
        // Bare `:notify` → usage flash, no effect, no pulse.
        assert!(super::commands::cmd_notify(&mut app, "").is_empty());
        assert!(app.view.visual_bell.is_none());
        // `:notify test` → one Notify carrying every channel, and the pulse starts.
        let fx = super::commands::cmd_notify(&mut app, "test");
        assert!(app.view.visual_bell.is_some(), "the visual pulse starts");
        match fx.as_slice() {
            [Effect::Notify { system, osc9, bell }] => {
                assert!(system.is_some(), "OS notifier fires");
                assert!(osc9.is_some(), "OSC-9 fires");
                assert!(*bell, "bell rings");
            }
            other => panic!("expected one Effect::Notify, got {other:?}"),
        }
    });
}

/// Regression: a hook-driven `report_status` pre-sets `activity` for an instant
/// dot (`mcp.rs`), which used to erase the Idle→Blocked edge before
/// `settle_agent_activity` observed it — so the dot went red but the ping never
/// fired. The edge now keys off the settle-owned `notified` field, so the
/// transition still emits `Effect::Notify` even with `activity` pre-set.
#[test]
fn agent_transition_notifies_even_when_activity_preset_by_report() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().join("work");
        std::fs::create_dir(&dir).unwrap();
        let mut app = App::test_app(dir.clone());
        // `cat` stands in for the agent process; the "claude" command makes the
        // tab detect as an agent. Same spawn idiom as the pane harness tests.
        let wake = app.make_pane_wake();
        let pane = crate::pane::Pane::spawn("cat", 24, 80, &dir, &app.view.context_path, wake)
            .expect("spawn cat");
        let entry =
            crate::pane::tabs::TabEntry::new(pane, crate::pane::tabs::TabInfo::new("claude", &dir));
        app.runtime.pane_tabs = Some(crate::pane::tabs::PaneTabs::new(entry));

        // The state right after the MCP report handler applied a `blocked`
        // report: `reported` + `activity` are Blocked (the instant-dot pre-set),
        // but the settle-owned `notified` still lags at Idle. The tab isn't
        // pane-focused (default focus is the file list) → not suppressed.
        let now = std::time::Instant::now();
        {
            let info = app.runtime.pane_tabs.as_mut().unwrap().active_info_mut();
            info.reported = Some(crate::pane::ReportedStatus {
                status: crate::pane::AgentActivity::Blocked,
                at: now,
                expiry: now + std::time::Duration::from_secs(300),
            });
            info.activity = crate::pane::AgentActivity::Blocked;
            info.notified = crate::pane::AgentActivity::Idle;
        }

        let mut ctx = RunCtx::for_test();
        let (_drew, effects) = app.settle_agent_activity(now, &mut ctx);
        assert!(
            effects.iter().any(|e| matches!(e, Effect::Notify { .. })),
            "a Blocked transition must ping even when `activity` was pre-set by the report",
        );
    });
}

mod mcp;
mod pane;
mod per_column;
mod second_commander;
mod vsplit;
