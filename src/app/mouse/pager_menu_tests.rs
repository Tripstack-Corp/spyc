//! The menu of an open pager. A right-click on a live `!` capture, the task
//! viewer, a `^a v` scrollback or a `D` pager opens it, titled with the
//! pager's title, and a click on a row does what the row's key does in that
//! pager. Each case goes through `handle_mouse` or `handle_key` and reads the
//! menu as the frame draws it.

use super::hint_tests::{count_on_disk, run};
use super::menu_tests::{app_over_entries, app_with_tab, cursor_on, flash, offers, right_click};
use super::tests::{FRAME, centre, click, drawn, press};
use crate::app::{App, Effect, SigOk};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use std::path::Path;

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

/// An App over `work/` running `cat` as a live `!` capture.
fn app_with_capture(tmp: &Path) -> App {
    let mut app = app_over_entries(tmp);
    app.start_capture("cat", "cat", "cat");
    assert!(
        app.runtime.pending_capture.is_some(),
        "premise: the capture runs"
    );
    app
}

/// An App whose `cat` captures went to the background, one per `count`
/// (tasks #1 to #`count`), with the task viewer open on the newest by `gB`.
fn app_in_the_task_viewer(tmp: &Path, count: usize) -> App {
    let mut app = app_over_entries(tmp);
    for _ in 0..count {
        app.start_capture("cat", "cat", "cat");
        app.background_capture();
    }
    press(&mut app, 'g');
    press(&mut app, 'B');
    assert_eq!(
        viewed_task(&app),
        u32::try_from(count).ok(),
        "premise: `gB` opened the task viewer on the newest task"
    );
    app
}

pub(super) fn viewed_task(app: &App) -> Option<u32> {
    app.view.project().pager.as_ref().and_then(|v| v.task_id)
}

fn key(app: &mut App, code: KeyCode) -> Vec<Effect> {
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
        .unwrap()
}

/// The drawn text of row `y` from column `x` to `right`, exclusive.
fn text(buf: &ratatui::buffer::Buffer, y: u16, x: u16, right: u16) -> String {
    (x..right).map(|c| buf[(c, y)].symbol()).collect()
}

/// The open menu holding a row that reads `row`, as drawn: its box, its title
/// row and its body lines. The smallest box that holds it wins, since the
/// pager's own frame holds the menu drawn over it.
fn menu_holding(app: &mut App, row: &str) -> Option<(Rect, String, Vec<String>)> {
    let buf = drawn(app);
    let mut found: Option<(Rect, String, Vec<String>)> = None;
    for y in 0..FRAME.height {
        for x in 0..FRAME.width {
            if buf[(x, y)].symbol() != "┌" {
                continue;
            }
            let Some(right) = (x + 1..FRAME.width).find(|&c| buf[(c, y)].symbol() == "┐") else {
                continue;
            };
            let Some(bottom) = (y + 1..FRAME.height).find(|&r| buf[(x, r)].symbol() == "└")
            else {
                continue;
            };
            let rect = Rect::new(x, y, right - x + 1, bottom - y + 1);
            let lines: Vec<String> = (y + 1..bottom)
                .map(|r| text(&buf, r, x + 1, right))
                .collect();
            let smaller = found.as_ref().is_none_or(|(r, ..)| r.area() > rect.area());
            if smaller && offers(&lines, row) {
                found = Some((rect, text(&buf, y, x, right + 1), lines));
            }
        }
    }
    found
}

/// The body lines of the open menu holding `row`.
pub(super) fn menu_lines(app: &mut App, row: &str) -> Vec<String> {
    menu_holding(app, row)
        .unwrap_or_else(|| panic!("no menu with a row {row:?} is drawn"))
        .2
}

/// Where `row` is drawn in the open menu holding it: where a click aims.
pub(super) fn row_at(app: &mut App, row: &str) -> (u16, u16) {
    let (menu, ..) =
        menu_holding(app, row).unwrap_or_else(|| panic!("no menu with a row {row:?} is drawn"));
    let buf = drawn(app);
    for y in menu.y + 1..menu.bottom() - 1 {
        let cells: Vec<&str> = (menu.x..menu.right())
            .map(|x| buf[(x, y)].symbol())
            .collect();
        let Some(byte) = cells.concat().find(row) else {
            continue;
        };
        let mut at = 0;
        for (i, cell) in cells.iter().enumerate() {
            if at == byte {
                return (menu.x + u16::try_from(i).unwrap(), y);
            }
            at += cell.len();
        }
    }
    panic!("{row:?} is not drawn in its menu");
}

/// A right-click on the middle of the screen, over the pager drawn there.
pub(super) fn right_click_the_pager(app: &mut App) {
    drawn(app);
    right_click(app, centre(FRAME));
}

// ── a live `!` capture ──

/// A right-click on a live capture opens its menu, titled with the capture's
/// title: background, interrupt and hard kill, each with its key. `q` and `s`
/// would be typed into the command while it runs, so neither close nor save is
/// offered.
#[test]
fn a_right_click_on_a_live_capture_offers_background_interrupt_and_kill() {
    with_mouse(|tmp| {
        let mut app = app_with_capture(tmp);

        right_click_the_pager(&mut app);

        let (_, title, menu) = menu_holding(&mut app, "background the capture")
            .expect("a right-click on the capture opened its menu");
        assert!(
            title.contains("! cat"),
            "titled with the pager's: {title:?}"
        );
        assert!(offers(&menu, "^Z  background the capture"), "{menu:#?}");
        assert!(offers(&menu, "^C  interrupt"), "{menu:#?}");
        assert!(offers(&menu, "^\\  kill"), "{menu:#?}");
        assert!(!offers(&menu, "close"), "`q` is the command's: {menu:#?}");
        assert!(!offers(&menu, "save"), "and so is `s`: {menu:#?}");
    });
}

/// A click on "background" does what `^Z` does: the capture becomes a
/// background task and its pager closes.
#[test]
fn a_click_on_background_sends_the_capture_to_the_background() {
    with_mouse(|tmp| {
        let mut app = app_with_capture(tmp);
        right_click_the_pager(&mut app);
        let row = row_at(&mut app, "background the capture");

        click(&mut app, row);

        assert!(app.runtime.pending_capture.is_none(), "the capture let go");
        assert_eq!(
            app.runtime.background_tasks.tasks.len(),
            1,
            "the background tasks hold it"
        );
        assert!(app.view.project().pager.is_none(), "and its pager closed");
        assert!(app.view.chord_hint.is_none(), "the menu closed");
    });
}

/// A click on "kill" does what `^\` does: the capture is torn down and its
/// pager stays, marked interrupted.
#[test]
fn a_click_on_kill_tears_the_capture_down() {
    with_mouse(|tmp| {
        let mut app = app_with_capture(tmp);
        right_click_the_pager(&mut app);
        let row = row_at(&mut app, "kill");

        click(&mut app, row);

        assert!(app.runtime.pending_capture.is_none(), "the capture is gone");
        let title = app.view.project().pager.as_ref().map(|v| v.title.clone());
        assert_eq!(title.as_deref(), Some("\u{2717} ! cat — interrupted"));
    });
}

/// A click on "interrupt" does what `^C` does: a SIGINT to the command's
/// process group, and the capture runs on until its command exits.
#[cfg(unix)]
#[test]
fn a_click_on_interrupt_signals_the_capture() {
    with_mouse(|tmp| {
        let mut app = app_with_capture(tmp);
        right_click_the_pager(&mut app);
        let row = row_at(&mut app, "interrupt");

        let fx = click(&mut app, row);

        assert!(
            fx.iter().any(|e| matches!(
                e,
                Effect::SignalCapture { sig, .. } if *sig == rustix::process::Signal::INT
            )),
            "a SIGINT, as `^C` sends: {fx:?}"
        );
        assert!(app.runtime.pending_capture.is_some(), "the capture runs on");
    });
}

/// The capture's menu offers what only a running capture has, so it closes
/// when the command exits.
#[test]
fn the_capture_menu_closes_when_the_command_exits() {
    with_mouse(|tmp| {
        let mut app = app_with_capture(tmp);
        right_click_the_pager(&mut app);
        assert!(app.view.chord_hint.is_some(), "premise: the menu is open");

        // `^D` ends `cat`'s input, and it exits.
        let capture = app.runtime.pending_capture.as_mut().expect("the capture");
        capture.host.write_all(b"\x04").unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while app.runtime.pending_capture.is_some() {
            assert!(std::time::Instant::now() < deadline, "`cat` never exited");
            app.drain_pending_capture();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        assert!(app.view.chord_hint.is_none(), "the menu closed");
        assert!(app.view.project().pager.is_some(), "the pager stays");
    });
}

/// The menu takes its own keys over the capture: `↓` highlights its first
/// row and `Enter` fires it, and neither reaches the command.
#[test]
fn arrows_and_enter_drive_the_capture_menu_not_the_command() {
    with_mouse(|tmp| {
        let mut app = app_with_capture(tmp);
        right_click_the_pager(&mut app);
        assert!(app.view.chord_hint.is_some(), "premise: the menu is open");

        let mut fx = key(&mut app, KeyCode::Down);
        fx.extend(key(&mut app, KeyCode::Enter));

        assert!(
            !fx.iter().any(|e| matches!(e, Effect::SendToCapture { .. })),
            "nothing was typed into the command: {fx:?}"
        );
        assert_eq!(
            app.runtime.background_tasks.tasks.len(),
            1,
            "the first row, background, fired"
        );
    });
}

/// `?` in the capture's menu is the command's like any other key: it closes the
/// menu and reaches the command, and the capture keeps its pager.
#[test]
fn a_question_mark_in_the_capture_menu_reaches_the_command() {
    with_mouse(|tmp| {
        let mut app = app_with_capture(tmp);
        right_click_the_pager(&mut app);
        assert!(app.view.chord_hint.is_some(), "premise: the menu is open");

        let fx = app
            .handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE))
            .unwrap();

        assert!(app.view.chord_hint.is_none(), "the menu closed");
        assert!(
            matches!(fx.as_slice(), [Effect::SendToCapture { bytes }] if bytes == b"?"),
            "`?` reached the command: {fx:?}"
        );
        let title = app.view.project().pager.as_ref().map(|v| v.title.clone());
        assert!(
            title.is_some_and(|t| t.contains("! cat")),
            "the capture's pager is still the one open"
        );
    });
}

// ── the task viewer ──

/// The task viewer's menu, for a task running in the background, offers to
/// bring it to the foreground, as `:fg N` would from the list, and to pause it,
/// and has no capture to background.
#[test]
fn the_task_viewers_menu_offers_foreground_and_pause_and_no_background() {
    with_mouse(|tmp| {
        let mut app = app_in_the_task_viewer(tmp, 1);

        right_click_the_pager(&mut app);

        let menu = menu_lines(&mut app, "pause the task");
        assert!(offers(&menu, ":fg 1 (list)"), "{menu:#?}");
        assert!(offers(&menu, "S  pause the task"), "{menu:#?}");
        assert!(!offers(&menu, "background the capture"), "{menu:#?}");
        assert!(!offers(&menu, "continue the task"), "it runs: {menu:#?}");
    });
}

/// A click on "pause" does what `S` does: a SIGSTOP to the task's process
/// group, which marks it paused once delivered. Its menu then offers to
/// continue it, and a click there sends the SIGCONT `C` would.
#[test]
fn a_click_on_pause_pauses_the_task_and_its_menu_then_offers_continue() {
    with_mouse(|tmp| {
        let mut app = app_in_the_task_viewer(tmp, 1);
        right_click_the_pager(&mut app);
        let row = row_at(&mut app, "pause the task");

        let fx = click(&mut app, row);

        assert!(
            fx.iter().any(|e| matches!(
                e,
                Effect::SignalGroup { sig, on_ok: SigOk::Pause(1), .. }
                    if *sig == rustix::process::Signal::STOP
            )),
            "a SIGSTOP to task #1, as `S` sends: {fx:?}"
        );
        // What the executor does once the signal is delivered.
        app.runtime.background_tasks.tasks[0].paused = true;

        right_click_the_pager(&mut app);
        let menu = menu_lines(&mut app, "continue the task");
        assert!(offers(&menu, "C  continue the task"), "{menu:#?}");
        assert!(!offers(&menu, "pause the task"), "{menu:#?}");
        let row = row_at(&mut app, "continue the task");
        let fx = click(&mut app, row);
        assert!(
            fx.iter().any(|e| matches!(
                e,
                Effect::SignalGroup { sig, on_ok: SigOk::Resume(1), .. }
                    if *sig == rustix::process::Signal::CONT
            )),
            "a SIGCONT to task #1, as `C` sends: {fx:?}"
        );
    });
}

/// A click on "foreground" does what `:fg 1` does: the task streams into the
/// live pager again.
#[test]
fn a_click_on_foreground_resumes_the_task_as_fg_does() {
    with_mouse(|tmp| {
        let mut app = app_in_the_task_viewer(tmp, 1);
        right_click_the_pager(&mut app);
        let row = row_at(&mut app, "foreground");

        click(&mut app, row);

        let resumed = app
            .runtime
            .pending_capture
            .as_ref()
            .and_then(|c| c.original_id);
        assert_eq!(resumed, Some(1), "task #1 is the live capture again");
        assert!(
            app.runtime.background_tasks.tasks.is_empty(),
            "and left the background tasks"
        );
    });
}

/// A click on "to pane tab" does what `:task-to-pane 1` does: the task becomes
/// a pane tab.
#[test]
fn a_click_on_to_pane_moves_the_task_to_a_tab() {
    with_mouse(|tmp| {
        let mut app = app_in_the_task_viewer(tmp, 1);
        right_click_the_pager(&mut app);
        let menu = menu_lines(&mut app, ":task-to-pane 1 (list)");
        assert!(offers(&menu, "pane tab"), "{menu:#?}");
        let row = row_at(&mut app, ":task-to-pane 1");

        click(&mut app, row);

        assert!(
            app.runtime.background_tasks.tasks.is_empty(),
            "the task left the background tasks"
        );
        assert_eq!(
            app.runtime
                .project()
                .pane_tabs
                .as_ref()
                .map(crate::pane::tabs::PaneTabs::len),
            Some(1),
            "for a pane tab"
        );
    });
}

/// With two tasks, the viewer's menu steps between them: "next task" does what
/// `]t` does, wrapping from the newest to the oldest.
#[test]
fn a_click_on_next_task_steps_the_viewer_as_bracket_t_does() {
    with_mouse(|tmp| {
        let mut app = app_in_the_task_viewer(tmp, 2);
        right_click_the_pager(&mut app);
        let menu = menu_lines(&mut app, "next task");
        assert!(offers(&menu, "]t  next task"), "{menu:#?}");
        assert!(offers(&menu, "[t  previous task"), "{menu:#?}");
        let row = row_at(&mut app, "next task");

        click(&mut app, row);

        assert_eq!(viewed_task(&app), Some(1), "the viewer moved to task #1");
    });
}

/// With one task there is no other to step to.
#[test]
fn a_lone_tasks_menu_has_no_task_to_step_to() {
    with_mouse(|tmp| {
        let mut app = app_in_the_task_viewer(tmp, 1);
        right_click_the_pager(&mut app);

        let menu = menu_lines(&mut app, "pause the task");

        assert!(!offers(&menu, "next task"), "{menu:#?}");
        assert!(!offers(&menu, "previous task"), "{menu:#?}");
    });
}

// ── the scrollback and file pagers ──

/// A `^a v` scrollback's menu has save and close, and none of a task's rows.
/// A click on "save" does what `s` does in it: writes what it holds to a file.
#[test]
fn the_scrollbacks_menu_offers_save_and_close_and_no_task_rows() {
    with_mouse(|tmp| {
        let mut app = app_with_tab(tmp, "cat");
        // What `^a v` mounts once the pane's history has settled.
        let history = (0..50).map(|i| format!("line {i}").into()).collect();
        app.install_lower_pane_scroll_view(" cat (history)".to_string(), history, None);
        assert!(
            app.view.project().scroll_pager.is_some(),
            "premise: the scrollback is open"
        );
        let pane = app.frame_layout(FRAME).pane.expect("a pane is open");
        drawn(&mut app);

        right_click(&mut app, centre(pane));

        let menu = menu_lines(&mut app, "close the pager");
        assert!(offers(&menu, "s  save to a file"), "{menu:#?}");
        assert!(offers(&menu, "q  close the pager"), "{menu:#?}");
        for task_row in [
            "background",
            "interrupt",
            "kill",
            "foreground",
            "pause",
            "continue",
            "pane tab",
            "next task",
            "previous task",
        ] {
            assert!(!offers(&menu, task_row), "{task_row:?}: {menu:#?}");
        }
        let row = row_at(&mut app, "save to a file");
        let fx = click(&mut app, row);
        assert!(
            fx.iter()
                .any(|e| matches!(e, Effect::SavePagerOutput { .. })),
            "the scrollback's text goes to a file: {fx:?}"
        );
    });
}

/// A `D` pager's menu closes it. The file is on disk already, so there is
/// nothing to save.
#[test]
fn a_file_pagers_menu_closes_it() {
    with_mouse(|tmp| {
        let mut app = app_over_entries(tmp);
        cursor_on(&mut app, "notes.txt");
        press(&mut app, 'D');
        assert!(
            app.view.project().pager.is_some(),
            "premise: `D` opened the file in a pager"
        );
        let list = app.frame_layout(FRAME).list;
        drawn(&mut app);

        right_click(&mut app, centre(list));

        let menu = menu_lines(&mut app, "close the pager");
        assert!(!offers(&menu, "save"), "{menu:#?}");
        let row = row_at(&mut app, "close the pager");
        click(&mut app, row);
        assert!(app.view.project().pager.is_none(), "the pager closed");
    });
}

// ── naming the key a row stands for ──

/// A pager row fired by a click names its key, as every menu's row does: in
/// front of the action's own message when it has one.
#[test]
fn a_click_on_background_names_its_key_before_the_message() {
    with_mouse(|tmp| {
        let mut app = app_with_capture(tmp);
        right_click_the_pager(&mut app);
        let row = row_at(&mut app, "background the capture");

        click(&mut app, row);

        assert_eq!(
            flash(&app).as_deref(),
            Some("^Z · task #1 backgrounded — :fg to resume")
        );
    });
}

/// A row whose action says nothing is named with its label. A pager that stays
/// open covers the status bar, so the hint shows in the pager, where its key's
/// own message would.
#[test]
fn a_click_on_pause_names_its_key_in_the_pager() {
    with_mouse(|tmp| {
        let mut app = app_in_the_task_viewer(tmp, 1);
        right_click_the_pager(&mut app);
        let row = row_at(&mut app, "pause the task");

        click(&mut app, row);

        let shown = app
            .view
            .project()
            .pager
            .as_ref()
            .and_then(|v| v.flash.clone());
        assert_eq!(shown.as_deref(), Some("S  pause the task"));
        assert!(flash(&app).is_none(), "not on the covered status bar");
    });
}

/// A closed pager's hint stays on the status bar: nothing covers it.
#[test]
fn a_click_on_close_names_its_key() {
    with_mouse(|tmp| {
        let mut app = app_over_entries(tmp);
        cursor_on(&mut app, "notes.txt");
        press(&mut app, 'D');
        let list = app.frame_layout(FRAME).list;
        drawn(&mut app);
        right_click(&mut app, centre(list));
        let row = row_at(&mut app, "close the pager");

        click(&mut app, row);

        assert_eq!(flash(&app).as_deref(), Some("q  close the pager"));
    });
}

/// A picker's keys are its own (`q` may be typed into the history editor's
/// line), so a picker has no pager menu: a right-click on the jump history
/// opens nothing.
#[test]
fn a_picker_has_no_pager_menu() {
    with_mouse(|tmp| {
        let mut app = app_over_entries(tmp);
        app.state.jump_history.push(&tmp.display().to_string());
        press(&mut app, 'J');
        press(&mut app, '?');
        assert!(
            app.view
                .project()
                .pager
                .as_ref()
                .is_some_and(|v| v.picker.is_some()),
            "premise: `J ?` opened the jump history"
        );

        right_click_the_pager(&mut app);

        assert!(app.view.chord_hint.is_none(), "no menu opened");
        assert!(!app.state.resolver.is_pending(), "and no leader is armed");
    });
}

// ── the hint fades as a pager's keys are learned ──

/// What the open pager shows in its footer, where a fired row's hint goes.
fn pager_flash(app: &App) -> Option<String> {
    app.view
        .project()
        .pager
        .as_ref()
        .and_then(|v| v.flash.clone())
}

/// Open the task viewer's menu and click "pause the task".
fn click_pause(app: &mut App) -> Vec<Effect> {
    right_click_the_pager(app);
    let row = row_at(app, "pause the task");
    click(app, row)
}

/// Whether `fx` pauses task #1, as `S` does.
fn pauses_task_one(fx: &[Effect]) -> bool {
    fx.iter().any(|e| {
        matches!(
            e,
            Effect::SignalGroup { sig, on_ok: SigOk::Pause(1), .. }
                if *sig == rustix::process::Signal::STOP
        )
    })
}

/// Three `S`s in the task viewer and the pause row's key is learned: a click
/// on the row still pauses the task, and names no key. Two, and it still does.
#[test]
#[ignore = "red: a pager's keys never call `learn_key`, so the pause row names `S` forever"]
fn the_pause_rows_hint_fades_after_three_s_presses() {
    with_mouse(|tmp| {
        let mut app = app_in_the_task_viewer(tmp, 1);
        press(&mut app, 'S');
        press(&mut app, 'S');
        click_pause(&mut app);
        assert_eq!(
            pager_flash(&app).as_deref(),
            Some("S  pause the task"),
            "two presses"
        );

        press(&mut app, 'S');
        let fx = click_pause(&mut app);

        assert!(pauses_task_one(&fx), "the row still fires: {fx:?}");
        assert_eq!(pager_flash(&app), None, "three presses");
        assert!(flash(&app).is_none(), "{:?}", flash(&app));
    });
}

/// A click is not a key: clicking the capture menu's interrupt row again and
/// again never silences the `^C` it is there to teach, and writes no count.
#[test]
fn capture_menu_clicks_never_count() {
    with_mouse(|tmp| {
        let mut app = app_with_capture(tmp);
        for click_number in 1..=5 {
            // Clear the last click's hint, so the hint read is this click's.
            if let Some(view) = app.view.project_mut().pager.as_mut() {
                view.flash = None;
            }
            right_click_the_pager(&mut app);
            let row = row_at(&mut app, "interrupt the command");

            let fx = click(&mut app, row);

            assert!(
                !fx.iter().any(|e| matches!(e, Effect::SaveLearned(_))),
                "click {click_number} wrote a count: {fx:?}"
            );
            assert_eq!(
                pager_flash(&app).as_deref(),
                Some("^C  interrupt the command"),
                "click {click_number}"
            );
        }
    });
}

/// `q` closing a pager is its close row's key, and counts: `learned.json`
/// holds it under `pager_close`.
#[test]
#[ignore = "red: a pager's keys never call `learn_key`, so `q` writes no count"]
fn q_closing_a_pager_counts_as_pager_close() {
    with_mouse(|tmp| {
        let mut app = app_over_entries(tmp);
        cursor_on(&mut app, "notes.txt");
        press(&mut app, 'D');
        assert!(
            app.view.project().pager.is_some(),
            "premise: `D` opened the file in a pager"
        );

        let fx = key(&mut app, KeyCode::Char('q'));
        run(&mut app, fx);

        assert!(app.view.project().pager.is_none(), "`q` closed it");
        assert_eq!(count_on_disk(tmp, "pager_close"), Some(1));
    });
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn plain(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

fn one_task(tmp: &Path) -> App {
    app_in_the_task_viewer(tmp, 1)
}

fn two_tasks(tmp: &Path) -> App {
    app_in_the_task_viewer(tmp, 2)
}

fn a_paused_task(tmp: &Path) -> App {
    let mut app = app_in_the_task_viewer(tmp, 1);
    // What the executor does once `S`'s SIGSTOP is delivered.
    app.runtime.background_tasks.tasks[0].paused = true;
    app
}

/// The count for `name` in the write of `learned.json` among `fx`, if any.
fn written_count(fx: &[Effect], name: &str) -> Option<u32> {
    fx.iter().find_map(|e| match e {
        Effect::SaveLearned(learned) => Some(learned.count(name)),
        _ => None,
    })
}

/// Each key a pager's row names counts under the row's name, wherever the
/// pager reads it: a live capture's three, the task viewer's `^C`, `S`, `C`,
/// `]t` and `[t`, and `s` and `q`.
#[test]
#[ignore = "red: a pager's keys never call `learn_key`, so none writes a count"]
fn each_pager_rows_key_counts_under_its_name() {
    type Open = fn(&Path) -> App;
    with_mouse(|tmp| {
        let cases: [(&str, Open, Vec<KeyEvent>, &str); 10] = [
            (
                "a capture's ^Z",
                app_with_capture,
                vec![ctrl('z')],
                "pager_background",
            ),
            (
                "a capture's ^C",
                app_with_capture,
                vec![ctrl('c')],
                "pager_interrupt",
            ),
            (
                "a capture's ^\\",
                app_with_capture,
                vec![ctrl('\\')],
                "pager_kill",
            ),
            (
                "the task viewer's ^C",
                one_task,
                vec![ctrl('c')],
                "pager_interrupt",
            ),
            ("S", one_task, vec![plain('S')], "pager_pause"),
            ("C", a_paused_task, vec![plain('C')], "pager_continue"),
            (
                "]t",
                two_tasks,
                vec![plain(']'), plain('t')],
                "pager_next_task",
            ),
            (
                "[t",
                two_tasks,
                vec![plain('['), plain('t')],
                "pager_prev_task",
            ),
            ("s", one_task, vec![plain('s')], "pager_save"),
            ("q", one_task, vec![plain('q')], "pager_close"),
        ];
        for (i, (typed, open, keys, name)) in cases.into_iter().enumerate() {
            let root = tmp.join(format!("case{i}"));
            std::fs::create_dir(&root).unwrap();
            let mut app = open(&root);

            let fx: Vec<Effect> = keys
                .into_iter()
                .flat_map(|k| app.handle_key(k).unwrap())
                .collect();

            assert_eq!(written_count(&fx, name), Some(1), "{typed}: {fx:?}");
        }
    });
}

// ── other modals ──

/// The finder is a modal with a pager of its own, and keeps its keys: a
/// right-click on it opens no menu and arms no leader.
#[test]
fn a_right_click_on_the_finder_opens_nothing() {
    with_mouse(|tmp| {
        let mut app = app_over_entries(tmp);
        press(&mut app, 'F');
        assert!(
            app.runtime.find_picker.is_some(),
            "premise: `F` opened the finder"
        );

        right_click_the_pager(&mut app);

        assert!(app.view.chord_hint.is_none(), "no menu opened");
        assert!(!app.state.resolver.is_pending(), "and no leader is armed");
        assert!(
            app.runtime.find_picker.is_some(),
            "the finder is still open"
        );
    });
}
