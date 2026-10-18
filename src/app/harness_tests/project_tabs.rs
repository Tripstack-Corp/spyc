//! #99 step 5c: what spyc does for its tabs, it does for every open project's
//! tabs, not only the project in view.
//!
//! Two projects in one `App`, `x` (where the tab runs) and `y` (where the user
//! is). Each case opens a real `cat` tab in X, switches to Y, and then runs the
//! loop step under test.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::*;

struct Two {
    app: App,
    x: state::ProjectId,
    y: state::ProjectId,
    x_home: PathBuf,
    y_home: PathBuf,
}

/// `x` is active and lists `xproj/`; `y` lists `yproj/`.
fn with_two(test: impl FnOnce(&mut Two)) {
    let tmp = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(tmp.path()).unwrap();
    let (state_dir, x_home, y_home) = (base.join("state"), base.join("xproj"), base.join("yproj"));
    for d in [&state_dir, &x_home, &y_home] {
        std::fs::create_dir_all(d).unwrap();
    }
    crate::state::with_state_root(&state_dir, || {
        let mut app = App::test_app(x_home.clone());
        let x = app.state.active;
        app.ready_test_project(x, &x_home);
        let y = app.add_test_project(&y_home);
        test(&mut Two {
            app,
            x,
            y,
            x_home,
            y_home,
        });
    });
}

/// Read project `id`'s first tab.
fn first_tab<R>(
    app: &mut App,
    id: state::ProjectId,
    f: impl FnOnce(&crate::pane::TabEntry) -> R,
) -> R {
    app.as_project(id, |app| {
        let tabs = app.runtime.project().pane_tabs.as_ref().expect("a tab");
        f(&tabs.tabs()[0])
    })
}

/// Write `bytes` to project `id`'s first tab.
fn type_into(app: &mut App, id: state::ProjectId, bytes: &[u8]) {
    app.as_project(id, |app| {
        let tabs = app.runtime.project_mut().pane_tabs.as_mut().expect("a tab");
        tabs.tabs_mut()[0]
            .pane
            .send_bytes(bytes)
            .expect("the tab takes input");
    });
}

/// Run the pane-output step until `done`, for at most five seconds.
fn drain_until(app: &mut App, done: impl Fn(&mut App) -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        app.drain_pane_output();
        if done(app) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The control: in the project in view, a tab's output is stamped and its
/// exit is marked.
#[test]
fn a_tab_in_the_project_in_view_is_drained_as_before() {
    with_two(|t| {
        let (app, x) = (&mut t.app, t.x);
        app.open_tab_in_project(x, &t.x_home);
        type_into(app, x, b"hello\n");
        assert!(drain_until(app, |app| first_tab(app, x, |tab| tab
            .info
            .last_output_at
            .is_some())));
        type_into(app, x, b"\x04");
        assert!(drain_until(app, |app| first_tab(app, x, |tab| tab
            .info
            .label
            .contains("[exited"))));
    });
}

/// Output from a tab in a project the user isn't in is still seen: its
/// activity dot runs on it.
#[test]
fn output_from_a_tab_in_another_project_is_stamped() {
    with_two(|t| {
        let (app, x) = (&mut t.app, t.x);
        app.open_tab_in_project(x, &t.x_home);
        app.make_active(t.y);
        type_into(app, x, b"hello\n");
        assert!(
            drain_until(app, |app| first_tab(app, x, |tab| tab
                .info
                .last_output_at
                .is_some())),
            "X's tab printed, and nothing saw it"
        );
    });
}

/// A child that exits in a project the user isn't in is marked exited.
#[test]
fn a_tab_that_exits_in_another_project_is_marked_exited() {
    with_two(|t| {
        let (app, x) = (&mut t.app, t.x);
        app.open_tab_in_project(x, &t.x_home);
        app.make_active(t.y);
        type_into(app, x, b"\x04");
        assert!(
            drain_until(app, |app| first_tab(app, x, |tab| tab
                .info
                .label
                .contains("[exited"))),
            "X's tab exited, and its label never said so"
        );
    });
}

/// Output and an exit in a project the user isn't in change nothing on
/// screen, so neither asks for a redraw, from X's active tab or its other one.
#[test]
fn a_tab_in_another_project_asks_for_no_redraw() {
    with_two(|t| {
        let (app, x) = (&mut t.app, t.x);
        app.open_tab_in_project(x, &t.x_home);
        app.open_tab_in_project(x, &t.x_home);
        app.make_active(t.y);
        let every_tab = |app: &mut App, bytes: &[u8]| {
            app.as_project(x, |app| {
                let tabs = app.runtime.project_mut().pane_tabs.as_mut().expect("tabs");
                for entry in tabs.tabs_mut() {
                    entry.pane.send_bytes(bytes).expect("the tab takes input");
                }
            });
        };
        let all = |app: &mut App, done: &dyn Fn(&crate::pane::TabEntry) -> bool| {
            app.as_project(x, |app| {
                let tabs = app.runtime.project().pane_tabs.as_ref().expect("tabs");
                tabs.tabs().iter().all(done)
            })
        };
        let mut redraws = Vec::new();
        let mut watch = |app: &mut App, done: &dyn Fn(&crate::pane::TabEntry) -> bool| {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                redraws.push(app.drain_pane_output());
                if all(app, done) {
                    return;
                }
                assert!(Instant::now() < deadline, "X's tabs never got there");
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        every_tab(app, b"hello\n");
        watch(app, &|tab| tab.info.last_output_at.is_some());
        every_tab(app, b"\x04");
        watch(app, &|tab| tab.info.label.contains("[exited"));
        assert!(
            redraws.iter().all(|(draw, _)| !draw),
            "X's tabs asked for a redraw: {redraws:?}"
        );
    });
}

/// Quitting shuts down every project's tabs, so none is left running.
#[test]
fn teardown_shuts_down_a_tab_in_another_project() {
    with_two(|t| {
        let (app, x) = (&mut t.app, t.x);
        app.open_tab_in_project(x, &t.x_home);
        app.make_active(t.y);
        app.run_teardown();
        assert!(
            first_tab(app, x, |tab| tab.pane.is_closed()),
            "X's tab outlived teardown"
        );
    });
}

/// The quit confirm counts a running tab in a project the user isn't in.
#[test]
fn the_quit_confirm_counts_a_tab_in_another_project() {
    with_two(|t| {
        let app = &mut t.app;
        app.open_tab_in_project(t.x, &t.x_home);
        app.make_active(t.y);
        app.request_quit();
        assert_eq!(
            app.flash_text(),
            Some("1 running process — press again to quit")
        );
    });
}

/// The activity monitor's `panes:` cell counts every project's tabs.
#[test]
fn the_pane_count_covers_every_project() {
    with_two(|t| {
        let app = &mut t.app;
        app.open_tab_in_project(t.x, &t.x_home);
        app.open_tab_in_project(t.y, &t.y_home);
        app.make_active(t.y);
        assert_eq!(app.pane_tab_count(), 2);
    });
}

/// A terminal resize reaches a tab in a project the user isn't in, so its
/// child draws at the size it will be shown at.
#[test]
fn a_resize_reaches_a_tab_in_another_project() {
    with_two(|t| {
        let (app, x, y) = (&mut t.app, t.x, t.y);
        app.open_tab_in_project(x, &t.x_home);
        app.open_tab_in_project(y, &t.y_home);
        app.make_active(y);
        let spawned = first_tab(app, x, |tab| tab.pane.host.last_size);
        app.handle_resize(151, 47);
        let ys = first_tab(app, y, |tab| tab.pane.host.last_size);
        assert_ne!(ys, spawned, "the resize changed the pane size");
        assert_eq!(
            first_tab(app, x, |tab| tab.pane.host.last_size),
            ys,
            "X's tab kept its old size"
        );
    });
}

/// A restored tab in a project the user isn't in still gets its `/resume`,
/// and the loop still wakes for the next step.
#[test]
fn a_restored_tab_in_another_project_gets_its_resume() {
    use crate::pane::tabs::PendingResumeSend;
    with_two(|t| {
        let (app, x) = (&mut t.app, t.x);
        app.open_tab_in_project(x, &t.x_home);
        let now = Instant::now();
        app.as_project(x, |app| {
            let tabs = app.runtime.project_mut().pane_tabs.as_mut().expect("a tab");
            tabs.tabs_mut()[0].info.pending_resume_send = Some(PendingResumeSend::Text {
                sid: "0193a0b1-c2d3-7e4f-8a9b-0c1d2e3f4a5b".into(),
                after: now,
            });
        });
        app.make_active(t.y);
        let mut ctx = RunCtx::for_test();
        app.handle_restore_resumes(now, &mut ctx);
        assert!(
            first_tab(app, x, |tab| matches!(
                tab.info.pending_resume_send,
                Some(PendingResumeSend::Enter { .. })
            )),
            "X's `/resume` was never typed"
        );
        assert!(
            ctx.scheduler.next().is_some(),
            "nothing wakes the loop to submit it"
        );
    });
}

/// A project the user moves to after startup walks git on the background
/// worker, as the first one does, never on the UI thread.
#[test]
fn another_project_walks_git_off_the_ui_thread() {
    with_two(|t| {
        let app = &mut t.app;
        let run_git = crate::git::test_support::run_git;
        run_git(&t.y_home, &["init", "-q", "--initial-branch=main"]);
        app.wire_git_worker();
        app.make_active(t.y);
        let y_home = t.y_home.clone();
        app.state.update_repo_root(state::Side::Left, &y_home);
        app.state
            .git_file_statuses_cached(state::Side::Left, &y_home, true);
        assert_eq!(
            app.state
                .col(state::Side::Left)
                .git_cache
                .pending_git_requests
                .len(),
            1,
            "Y's walk ran inline instead of going to the worker"
        );
    });
}
