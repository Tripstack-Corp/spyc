//! A tab suspended with `^z` and moved to the background tasks stays as the
//! user left it: stopped, and a paused task, whose menu offers to continue it.
//! The case goes through `handle_key` for the keys, `handle_mouse` for the
//! menu, and `run_effects` for the signals, so the stop and the continue are
//! delivered to the real process.

use super::make_menu_tests::terminal_and_foreground;
use super::menu_tests::{app_with_tab, offers};
use super::pager_menu_tests::{menu_lines, right_click_the_pager, row_at, viewed_task};
use super::tests::{click, press};
use crate::app::{App, Effect, SigOk};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rustix::process::Signal;

fn key(app: &mut App, code: KeyCode, modifiers: KeyModifiers) -> Vec<Effect> {
    app.handle_key(KeyEvent::new(code, modifiers)).unwrap()
}

/// Carry out `fx` as the loop does.
fn run(app: &mut App, fx: Vec<Effect>) {
    let (mut terminal, fg) = terminal_and_foreground();
    app.run_effects(fx, &mut terminal, &fg);
}

/// `:command`, typed from the file list; what it asked the loop to do.
fn colon(app: &mut App, command: &str) -> Vec<Effect> {
    let mut fx = key(app, KeyCode::Char(':'), KeyModifiers::NONE);
    for c in command.chars() {
        fx.extend(key(app, KeyCode::Char(c), KeyModifiers::NONE));
    }
    fx.extend(key(app, KeyCode::Enter, KeyModifiers::NONE));
    fx
}

/// Whether `fx` would wake a stopped process.
fn continues(fx: &[Effect]) -> bool {
    fx.iter().any(|e| match e {
        Effect::SignalGroup { sig, .. } => *sig == Signal::CONT,
        Effect::SignalPane { resume, .. } => *resume,
        _ => false,
    })
}

/// A tab suspended with `^z`, then moved to the background tasks, stays
/// stopped and becomes a paused task: the move sends no continue, and the task
/// viewer's menu offers continue, not pause. Continuing it sends the `SIGCONT`
/// and clears the pause.
#[test]
#[ignore = "red: `:pane-to-task` makes a running, unpaused task of a suspended tab"]
fn a_suspended_tab_moved_to_a_task_is_a_paused_task() {
    let _lock = crate::mouse_test_lock();
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        crate::set_mouse_capture_for_test(true);
        let mut app = app_with_tab(tmp.path(), "sleep 30");
        let fx = key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert!(
            fx.iter()
                .any(|e| matches!(e, Effect::SignalPane { resume: false, .. })),
            "premise: `^z` stops the tab: {fx:?}"
        );
        run(&mut app, fx);
        app.set_pane_focus(false);

        let fx = colon(&mut app, "pane-to-task");

        assert!(!continues(&fx), "the move wakes nothing: {fx:?}");
        let task = app
            .runtime
            .background_tasks
            .tasks
            .first()
            .expect("the tab became a task");
        let id = task.id;
        assert!(task.paused, "the task is paused, as the tab was");

        press(&mut app, 'g');
        press(&mut app, 'B');
        assert_eq!(viewed_task(&app), Some(id), "premise: `gB` shows it");
        right_click_the_pager(&mut app);
        let menu = menu_lines(&mut app, "continue the task");
        assert!(offers(&menu, "C  continue the task"), "{menu:#?}");
        assert!(!offers(&menu, "pause the task"), "{menu:#?}");

        let row = row_at(&mut app, "continue the task");
        let fx = click(&mut app, row);

        assert!(
            fx.iter().any(|e| matches!(
                e,
                Effect::SignalGroup { sig, on_ok: SigOk::Resume(task), .. }
                    if *sig == Signal::CONT && *task == id
            )),
            "continue sends the task a SIGCONT: {fx:?}"
        );
        run(&mut app, fx);
        let task = app.runtime.background_tasks.tasks.first().expect("a task");
        assert!(!task.paused, "and the task runs again");
        crate::set_mouse_capture_for_test(false);
    });
}

/// A tab that runs, moved to the tasks, is a running task: pause, not
/// continue.
#[test]
fn a_running_tab_moved_to_a_task_is_a_running_task() {
    let _lock = crate::mouse_test_lock();
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        crate::set_mouse_capture_for_test(true);
        let mut app = app_with_tab(tmp.path(), "sleep 30");
        app.set_pane_focus(false);

        colon(&mut app, "pane-to-task");

        let task = app.runtime.background_tasks.tasks.first().expect("a task");
        assert!(!task.paused, "the task runs");
        press(&mut app, 'g');
        press(&mut app, 'B');
        right_click_the_pager(&mut app);
        let menu = menu_lines(&mut app, "pause the task");
        assert!(offers(&menu, "S  pause the task"), "{menu:#?}");
        assert!(!offers(&menu, "continue the task"), "{menu:#?}");
        crate::set_mouse_capture_for_test(false);
    });
}
