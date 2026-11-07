//! The background tasks' indicator under the pointer: the divider's task tags
//! with a pane open, the status bar's `bg:` without one. A left click opens
//! the task viewer as `gB` does and names that key; a right-click opens the
//! `tasks` menu, `gB` and then a `:fg N` row per task. Each case goes through
//! `handle_mouse` and aims at what the frame draws.

use super::menu_tests::{app_over_entries, app_with_tab, flash, menu_lines, offers, right_click};
use super::pager_menu_tests::viewed_task;
use super::status_menu_tests::on_bar;
use super::tests::{click, drawn, drawn_row, press};
use crate::app::App;
use std::path::Path;

const TASKS_MENU: &str = "tasks";

/// Run `body` over a fresh state root, with mouse reporting on, as a click
/// needs it.
fn with_mouse(body: impl FnOnce(&Path)) {
    let _lock = crate::mouse_test_lock();
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        crate::set_mouse_capture_for_test(true);
        body(tmp.path());
        crate::set_mouse_capture_for_test(false);
    });
}

/// An App over `work/` with `count` `cat` captures sent to the background
/// (tasks #1 to #`count`), and a pane tab running `cat` when `pane`, the
/// keyboard in the file list.
fn app_with_tasks(tmp: &Path, count: usize, pane: bool) -> App {
    let mut app = if pane {
        let mut app = app_with_tab(tmp, "cat");
        app.set_pane_focus(false);
        app
    } else {
        app_over_entries(tmp)
    };
    for _ in 0..count {
        app.start_capture("cat", "cat", "cat");
        app.background_capture();
    }
    assert_eq!(
        app.runtime.background_tasks.tasks.len(),
        count,
        "premise: the captures went to the background"
    );
    assert_eq!(
        app.runtime.project().pane_tabs.is_some(),
        pane,
        "premise: a pane is open only when asked for"
    );
    app
}

/// Where task `id`'s tag is drawn on the divider: on its number.
fn task_tag(app: &mut App, id: u32) -> (u16, u16) {
    let buf = drawn(app);
    let divider = app
        .frame_layout(super::tests::FRAME)
        .divider
        .expect("a divider");
    let cells: Vec<String> = (divider.x..divider.right())
        .map(|x| buf[(x, divider.y)].symbol().to_string())
        .collect();
    let line = cells.concat();
    let byte = ["\u{25cf}", "+"]
        .iter()
        .find_map(|glyph| line.find(&format!("[{id}{glyph}]")))
        .unwrap_or_else(|| panic!("task #{id}'s tag is not on the divider: {line:?}"));
    let mut at = 0;
    for (i, cell) in cells.iter().enumerate() {
        if at == byte {
            return (divider.x + u16::try_from(i).unwrap() + 1, divider.y);
        }
        at += cell.len();
    }
    panic!("task #{id}'s tag starts inside a cell");
}

/// The hint a click left: in the pager it opened, else on the prompt row.
fn hint(app: &App) -> Option<String> {
    app.view
        .project()
        .pager
        .as_ref()
        .and_then(|v| v.flash.clone())
        .or_else(|| flash(app))
}

/// The task the live capture brought back, if one is live.
fn live_task(app: &App) -> Option<u32> {
    app.runtime
        .pending_capture
        .as_ref()
        .and_then(|c| c.original_id)
}

fn background_ids(app: &App) -> Vec<u32> {
    app.runtime
        .background_tasks
        .tasks
        .iter()
        .map(|t| t.id)
        .collect()
}

// ── a left click ──

/// A click on a task's tag on the divider opens the task viewer, as `gB`
/// does.
#[test]
fn a_click_on_the_dividers_task_tags_opens_the_task_viewer() {
    with_mouse(|tmp| {
        let mut app = app_with_tasks(tmp, 2, true);
        let at = task_tag(&mut app, 2);

        click(&mut app, at);

        assert_eq!(viewed_task(&app), Some(2), "the task viewer is open");
    });
}

/// With no pane open, the tasks ride the status bar's `bg:`, and a click on
/// it does the same.
#[test]
fn with_no_pane_a_click_on_the_status_bars_tasks_opens_the_task_viewer() {
    with_mouse(|tmp| {
        let mut app = app_with_tasks(tmp, 1, false);
        let at = on_bar(&mut app, "bg:1");

        click(&mut app, at);

        assert_eq!(viewed_task(&app), Some(1), "the task viewer is open");
    });
}

/// The click names `gB`, the key that does the same, until `gB` has been
/// pressed three times.
#[test]
fn a_click_on_the_task_tags_names_gb_until_it_is_learned() {
    with_mouse(|tmp| {
        let mut app = app_with_tasks(tmp, 1, true);
        let at = task_tag(&mut app, 1);
        click(&mut app, at);
        assert_eq!(viewed_task(&app), Some(1), "premise: the viewer opened");
        let named = hint(&app).unwrap_or_default();
        assert!(named.starts_with("gB"), "the hint names gB: {named:?}");

        for _ in 0..3 {
            press(&mut app, 'q');
            assert_eq!(viewed_task(&app), None, "premise: `q` closed it");
            press(&mut app, 'g');
            press(&mut app, 'B');
            assert_eq!(viewed_task(&app), Some(1), "premise: `gB` opened it");
        }
        press(&mut app, 'q');
        let at = task_tag(&mut app, 1);
        click(&mut app, at);

        assert_eq!(viewed_task(&app), Some(1), "the click still opens it");
        let named = hint(&app).unwrap_or_default();
        assert!(!named.starts_with("gB"), "three presses: {named:?}");
    });
}

// ── a right-click ──

/// A right-click on the task tags opens `tasks`: `gB` first, then each task
/// with the `:fg N` that brings it back. A click on a row does what `:fg N`
/// does: the task is the live capture again, and the other stays in the
/// background.
#[test]
fn a_right_click_lists_each_task_and_a_row_brings_it_back() {
    with_mouse(|tmp| {
        let mut app = app_with_tasks(tmp, 2, true);
        let at = task_tag(&mut app, 2);

        right_click(&mut app, at);

        let menu = menu_lines(&mut app, TASKS_MENU);
        assert!(menu[0].contains("gB"), "gB first: {menu:#?}");
        assert!(offers(&menu, ":fg 1 (list)  cat"), "{menu:#?}");
        assert!(offers(&menu, ":fg 2 (list)  cat"), "{menu:#?}");

        let row = drawn_row(&mut app, TASKS_MENU, ":fg 1");
        click(&mut app, row);

        assert_eq!(live_task(&app), Some(1), "task #1 is the live capture");
        assert_eq!(background_ids(&app), [2], "task #2 stays in the background");
    });
}

/// With no pane open, a right-click on the status bar's `bg:` opens the same
/// menu, and its `gB` row opens the task viewer.
#[test]
fn with_no_pane_a_right_click_on_the_status_bars_tasks_opens_the_menu() {
    with_mouse(|tmp| {
        let mut app = app_with_tasks(tmp, 1, false);
        let at = on_bar(&mut app, "bg:1");

        right_click(&mut app, at);

        let menu = menu_lines(&mut app, TASKS_MENU);
        assert!(offers(&menu, ":fg 1 (list)  cat"), "{menu:#?}");
        let row = drawn_row(&mut app, TASKS_MENU, "gB");
        click(&mut app, row);
        assert_eq!(viewed_task(&app), Some(1), "the task viewer is open");
    });
}

// ── off the indicator ──

/// The divider's rule beside the task tags is still the divider: a click
/// there opens no task viewer, and a right-click opens the pane tabs' menu.
#[test]
fn off_the_task_tags_the_divider_is_as_before() {
    with_mouse(|tmp| {
        let mut app = app_with_tasks(tmp, 1, true);
        let (x, y) = task_tag(&mut app, 1);
        let beside = (x - 3, y);
        assert_eq!(drawn(&mut app)[beside].symbol(), "─", "premise: the rule");

        click(&mut app, beside);
        assert_eq!(viewed_task(&app), None, "no task viewer");

        right_click(&mut app, beside);
        let menu = menu_lines(&mut app, "pane tabs");
        assert!(offers(&menu, "^a c"), "{menu:#?}");
    });
}

// ── a message already on the status bar ──

const WAITING: &str = "opened in external viewer";

/// Fade the menu hint of the action or command named `name`, as using its key
/// that many times does.
fn learn(app: &mut App, name: &str) {
    let after = app.state.active_project().config.ui.menu_hints_fade_after;
    for _ in 0..after {
        app.state.learned.record(name, after);
    }
}

/// What the open pager shows in its footer.
fn pager_flash(app: &App) -> Option<String> {
    app.view
        .project()
        .pager
        .as_ref()
        .and_then(|v| v.flash.clone())
}

/// A click on the task tags is `gB` typed: a message left on the status bar is
/// cleared, as `gB` clears it, and never shown in the viewer it opens as the
/// click's answer (`gB` learned, so the click names no key).
#[test]
fn a_click_on_the_task_tags_clears_a_waiting_message_rather_than_showing_it() {
    with_mouse(|tmp| {
        let mut app = app_with_tasks(tmp, 1, true);
        learn(
            &mut app,
            crate::keymap::Action::OpenTaskViewer.canonical_name(),
        );
        let at = task_tag(&mut app, 1);
        app.state.flash_error(WAITING);

        click(&mut app, at);

        assert_eq!(viewed_task(&app), Some(1), "premise: the viewer opened");
        assert_eq!(pager_flash(&app), None, "the click said nothing");
        assert_eq!(flash(&app), None, "cleared, as the key clears it");
    });
}
