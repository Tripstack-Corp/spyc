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

// ── Agent status ─────────────────────────────────────────────────────────────
//
// An agent tab here is `cat` under an agent's name: it reads as that agent
// without starting one.

use crate::pane::{AgentActivity, ReportedStatus, TabInfo};

/// Give project `id` a tab that reads as `command`, in `dir`. Returns its pane
/// id.
pub(super) fn agent_tab_in(
    app: &mut App,
    id: state::ProjectId,
    dir: &std::path::Path,
    command: &str,
) -> String {
    app.as_project(id, |app| {
        let wake = app.make_pane_wake();
        let pane =
            crate::pane::Pane::spawn("cat", 24, 80, dir, &app.view.project().context_path, wake)
                .expect("spawn cat");
        let entry = crate::pane::TabEntry::new(pane, TabInfo::new(command, dir));
        let pane_id = entry.info.id.clone();
        let slot = &mut app.runtime.project_mut().pane_tabs;
        match slot.as_mut() {
            Some(tabs) => tabs.push(entry),
            None => *slot = Some(crate::pane::PaneTabs::new(entry)),
        }
        pane_id
    })
}

/// Change project `id`'s first tab.
fn edit_first_tab(app: &mut App, id: state::ProjectId, f: impl FnOnce(&mut TabInfo)) {
    app.as_project(id, |app| {
        let tabs = app.runtime.project_mut().pane_tabs.as_mut().expect("a tab");
        f(&mut tabs.tabs_mut()[0].info);
    });
}

fn report(status: AgentActivity, now: Instant) -> ReportedStatus {
    ReportedStatus::new(status, now, now + Duration::from_secs(300))
}

/// An agent in a project the user isn't in keeps its activity dot: its
/// report lights it.
#[test]
fn a_report_in_another_project_lights_its_dot() {
    with_two(|t| {
        let (app, x, x_home) = (&mut t.app, t.x, t.x_home.clone());
        agent_tab_in(app, x, &x_home, "claude");
        let now = Instant::now();
        edit_first_tab(app, x, |info| {
            info.reported = Some(report(AgentActivity::Working, now));
        });
        app.make_active(t.y);
        app.settle_agent_activity(now, &mut RunCtx::for_test());
        assert_eq!(
            first_tab(app, x, |tab| tab.info.activity),
            AgentActivity::Working
        );
    });
}

/// Set project `id`'s `[notify]`.
fn notify_in(
    app: &mut App,
    id: state::ProjectId,
    f: impl FnOnce(&mut crate::config::NotifyConfig),
) {
    app.as_project(id, |app| {
        f(&mut app.state.active_project_mut().config.notify);
    });
}

/// An agent blocked in a project the user isn't in notifies, though the pane
/// in view is focused, `suppress_focused_tab` is on, and the blocked tab is
/// its own project's active one: the user can't see it.
#[test]
fn a_blocked_agent_in_another_project_notifies_though_the_pane_is_focused() {
    with_two(|t| {
        let (app, x, y, x_home) = (&mut t.app, t.x, t.y, t.x_home.clone());
        agent_tab_in(app, x, &x_home, "claude");
        let now = Instant::now();
        edit_first_tab(app, x, |info| {
            info.reported = Some(report(AgentActivity::Blocked, now));
        });
        for id in [x, y] {
            notify_in(app, id, |notify| notify.suppress_focused_tab = true);
        }
        app.make_active(y);
        app.state.active_project_mut().focus = state::Focus::Pane;
        let (_, effects) = app.settle_agent_activity(now, &mut RunCtx::for_test());
        assert!(
            effects.iter().any(|e| matches!(e, Effect::Notify { .. })),
            "X's agent is blocked, and nothing told the user: {effects:?}"
        );
    });
}

/// An agent in a project the user isn't in notifies under its own project's
/// `[notify]`, not the one in view's.
#[test]
fn a_blocked_agent_in_another_project_notifies_under_its_own_settings() {
    with_two(|t| {
        let (app, x, y, x_home) = (&mut t.app, t.x, t.y, t.x_home.clone());
        agent_tab_in(app, x, &x_home, "claude");
        let now = Instant::now();
        edit_first_tab(app, x, |info| {
            info.reported = Some(report(AgentActivity::Blocked, now));
        });
        notify_in(app, y, |notify| {
            notify.desktop = false;
            notify.bell = false;
            notify.visual = false;
        });
        app.make_active(y);
        let (_, effects) = app.settle_agent_activity(now, &mut RunCtx::for_test());
        assert!(
            effects.iter().any(|e| matches!(e, Effect::Notify { .. })),
            "X's agent notified under Y's settings: {effects:?}"
        );
    });
}

/// A codex question in flight in a project the user isn't in survives a
/// settle: its tab is still open.
#[test]
fn a_codex_question_in_another_project_survives_a_settle() {
    with_two(|t| {
        let (app, x, x_home) = (&mut t.app, t.x, t.x_home.clone());
        let tab = agent_tab_in(app, x, &x_home, "codex");
        app.state.codex_recovery.insert(
            tab.clone(),
            crate::agent::codex_recovery::CodexRecovery::default(),
        );
        app.make_active(t.y);
        app.settle_agent_activity(Instant::now(), &mut RunCtx::for_test());
        assert!(
            app.state.codex_recovery.contains_key(&tab),
            "X's codex question was forgotten"
        );
    });
}

/// A quiet agent in a project the user isn't in is still scanned for a
/// prompt its hooks can't report.
#[test]
fn a_quiet_agent_in_another_project_is_scanned() {
    with_two(|t| {
        let (app, x, x_home) = (&mut t.app, t.x, t.x_home.clone());
        agent_tab_in(app, x, &x_home, "claude");
        edit_first_tab(app, x, |info| info.scrape_dirty = true);
        app.make_active(t.y);
        app.settle_scrape_quiet(Instant::now(), &mut RunCtx::for_test());
        assert!(
            !first_tab(app, x, |tab| tab.info.scrape_dirty),
            "X's tab was never scanned"
        );
    });
}

/// An agent started in a shell tab in a project the user isn't in is seen,
/// and in that tab, not in the tab at the same place in the project in view.
#[test]
fn an_agent_started_in_a_shell_tab_in_another_project_is_seen() {
    use crate::state::sessions::AgentKind;
    with_two(|t| {
        let (app, x, y) = (&mut t.app, t.x, t.y);
        let (x_home, y_home) = (t.x_home.clone(), t.y_home.clone());
        let bin = x_home.with_file_name("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::os::unix::fs::symlink("/bin/sleep", bin.join("claude")).unwrap();
        app.as_project(x, |app| assert!(app.open_pane_tab_in("sh", &x_home)));
        app.as_project(y, |app| assert!(app.open_pane_tab_in("sh", &y_home)));
        app.make_active(y);
        type_into(
            app,
            x,
            format!("{} 60\r", bin.join("claude").display()).as_bytes(),
        );
        let deadline = Instant::now() + Duration::from_secs(15);
        while first_tab(app, x, |tab| tab.info.agent().kind()) != AgentKind::Claude {
            assert!(
                Instant::now() < deadline,
                "the claude in X's shell tab was never seen"
            );
            app.drain_pane_output();
            app.settle_shell_agents(Instant::now(), &mut RunCtx::for_test());
            std::thread::sleep(Duration::from_millis(30));
        }
        assert_eq!(
            first_tab(app, y, |tab| tab.info.agent().kind()),
            AgentKind::Other,
            "Y's shell tab took X's agent"
        );
        type_into(app, x, b"\x03");
    });
}

/// An unpinned codex in a project the user isn't in still gets a session
/// scan.
#[test]
fn an_unpinned_codex_in_another_project_is_scanned_for() {
    with_two(|t| {
        let (app, x, x_home) = (&mut t.app, t.x, t.x_home.clone());
        agent_tab_in(app, x, &x_home, "codex");
        app.make_active(t.y);
        app.kick_codex_session_scan();
        assert!(
            app.runtime
                .codex_scan_in_flight
                .load(std::sync::atomic::Ordering::Acquire),
            "no scan started for X's codex"
        );
    });
}

/// A codex session one project's tab has claimed is never handed to a tab in
/// another project: X's codex gets the other session in its directory.
#[test]
fn a_codex_session_claimed_in_one_project_is_not_given_to_another() {
    use crate::app::codex_pin::HomeSnapshot;
    use crate::state::codex_transcript::RolloutMeta;
    const TAKEN: &str = "0193a0b1-c2d3-7e4f-8a9b-0c1d2e3f4a5b";
    const FREE: &str = "0193a0b1-c2d3-7e4f-8a9b-0c1d2e3f4a5c";
    with_two(|t| {
        let (app, x, y, x_home) = (&mut t.app, t.x, t.y, t.x_home.clone());
        let home = x_home.with_file_name("codex-home");
        let x_tab = agent_tab_in(app, x, &x_home, "codex");
        let y_tab = agent_tab_in(app, y, &x_home, "codex");
        edit_first_tab(app, x, |info| info.codex_home = Some(home.clone()));
        edit_first_tab(app, y, |info| {
            info.codex_home = Some(home.clone());
            info.codex_session_id = Some(TAKEN.into());
        });
        app.make_active(y);
        let spawn = first_tab(app, x, |tab| tab.info.agent_epoch_secs());
        let cwd = x_home.to_string_lossy().into_owned();
        let rollout = |uuid: &str, at: u64| RolloutMeta {
            uuid: uuid.into(),
            cwd: cwd.clone(),
            started_secs: at,
            mtime_secs: at,
        };
        *app.runtime.codex_pin_pending.lock().unwrap() = Some(vec![HomeSnapshot {
            home,
            pane_ids: [x_tab, y_tab].into_iter().collect(),
            rollouts: vec![rollout(TAKEN, spawn + 1), rollout(FREE, spawn + 2)],
        }]);
        app.apply_codex_session_pins();
        assert_eq!(
            first_tab(app, x, |tab| tab.info.codex_session_id.clone()).as_deref(),
            Some(FREE)
        );
    });
}

// ── #99 step 6a: attention across projects ─────────────────────────────────

/// Change project `id`'s tab `index`.
fn edit_tab(app: &mut App, id: state::ProjectId, index: usize, f: impl FnOnce(&mut TabInfo)) {
    app.as_project(id, |app| {
        let tabs = app.runtime.project_mut().pane_tabs.as_mut().expect("a tab");
        f(&mut tabs.tabs_mut()[index].info);
    });
}

/// Make project `id`'s tab `index` its active one.
fn activate(app: &mut App, id: state::ProjectId, index: usize) {
    app.as_project(id, |app| {
        app.runtime
            .project_mut()
            .pane_tabs
            .as_mut()
            .expect("a tab")
            .switch_to(index);
    });
}

/// The cell after `[n]` in the tab bar of the project in view: its dot, `+`,
/// or blank.
fn tab_glyph(app: &mut App, n: usize) -> Option<char> {
    let (w, h) = (100, 30);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| app.render(f)).unwrap();
    let buf = terminal.backend().buffer();
    let marker = format!("[{n}]");
    (0..h).find_map(|y| {
        let row: String = (0..w)
            .map(|x| buf.cell((x, y)).map_or(" ", |c| c.symbol()))
            .collect();
        row.find(&marker)
            .and_then(|at| row[at + marker.len()..].chars().next())
    })
}

/// A done on a tab the user isn't looking at stays marked after its report
/// decays: its dot stays a teal `■` until the tab is in view, then reads idle.
#[test]
fn a_done_on_a_tab_out_of_view_stays_marked_until_seen() {
    with_two(|t| {
        let (app, x, x_home) = (&mut t.app, t.x, t.x_home.clone());
        agent_tab_in(app, x, &x_home, "claude");
        agent_tab_in(app, x, &x_home, "claude");
        activate(app, x, 0);
        let now = Instant::now();
        edit_tab(app, x, 1, |info| {
            info.reported = Some(report(AgentActivity::Done, now));
        });
        app.settle_agent_activity(now, &mut RunCtx::for_test());
        let later = now + Duration::from_secs(301);
        app.settle_agent_activity(later, &mut RunCtx::for_test());
        assert_eq!(tab_glyph(app, 2), Some('■'), "the done decayed unseen");

        activate(app, x, 1);
        app.settle_agent_activity(later, &mut RunCtx::for_test());
        activate(app, x, 0);
        assert_eq!(tab_glyph(app, 2), Some('·'), "seen, it reads idle");
    });
}

/// The control: a done on the tab in view is seen as it lands, so once its
/// report decays it reads idle wherever the user goes next.
#[test]
fn a_done_on_the_tab_in_view_is_seen() {
    with_two(|t| {
        let (app, x, x_home) = (&mut t.app, t.x, t.x_home.clone());
        agent_tab_in(app, x, &x_home, "claude");
        agent_tab_in(app, x, &x_home, "claude");
        activate(app, x, 0);
        let now = Instant::now();
        edit_tab(app, x, 0, |info| {
            info.reported = Some(report(AgentActivity::Done, now));
        });
        app.settle_agent_activity(now, &mut RunCtx::for_test());
        let later = now + Duration::from_secs(301);
        app.settle_agent_activity(later, &mut RunCtx::for_test());
        activate(app, x, 1);
        assert_eq!(tab_glyph(app, 1), Some('·'));
    });
}

/// With the list zoomed over the pane, the tab is out of view: a done there
/// is unseen, and still marked once the user unzooms and moves on.
#[test]
fn a_done_behind_a_zoomed_list_is_unseen() {
    with_two(|t| {
        let (app, x, x_home) = (&mut t.app, t.x, t.x_home.clone());
        agent_tab_in(app, x, &x_home, "claude");
        agent_tab_in(app, x, &x_home, "claude");
        activate(app, x, 0);
        app.state.active_project_mut().pane.zoom = state::ZoomTarget::TopList;
        let now = Instant::now();
        edit_tab(app, x, 0, |info| {
            info.reported = Some(report(AgentActivity::Done, now));
        });
        app.settle_agent_activity(now, &mut RunCtx::for_test());
        let later = now + Duration::from_secs(301);
        app.settle_agent_activity(later, &mut RunCtx::for_test());
        app.state.active_project_mut().pane.zoom = state::ZoomTarget::None;
        activate(app, x, 1);
        assert_eq!(tab_glyph(app, 1), Some('■'), "the done decayed unseen");
    });
}

/// The notification text from an agent's blocked edge, as one line.
fn notification_line(effects: &[Effect]) -> Option<String> {
    effects.iter().find_map(|effect| match effect {
        Effect::Notify { system, osc9, .. } => system
            .as_ref()
            .map(|(summary, body)| format!("{summary} — {body}"))
            .or_else(|| osc9.clone()),
        _ => None,
    })
}

/// With several projects open, a notification names its project: a tab
/// number alone doesn't say which project's tab.
#[test]
fn a_notification_names_its_project_when_several_are_open() {
    with_two(|t| {
        let (app, x, x_home) = (&mut t.app, t.x, t.x_home.clone());
        agent_tab_in(app, x, &x_home, "claude");
        let now = Instant::now();
        edit_first_tab(app, x, |info| {
            info.reported = Some(report(AgentActivity::Blocked, now));
        });
        app.make_active(t.y);
        let (_, effects) = app.settle_agent_activity(now, &mut RunCtx::for_test());
        let line = notification_line(&effects).expect("a notification");
        assert_eq!(line, "claude in xproj needs you — xproj: tab 1 is blocked");
    });
}

/// The control: with one project open, the notification reads as before.
#[test]
fn with_one_project_the_notification_reads_as_before() {
    with_two(|t| {
        let (app, x, x_home) = (&mut t.app, t.x, t.x_home.clone());
        app.remove_test_project(t.y);
        agent_tab_in(app, x, &x_home, "claude");
        let now = Instant::now();
        edit_first_tab(app, x, |info| {
            info.reported = Some(report(AgentActivity::Blocked, now));
        });
        let (_, effects) = app.settle_agent_activity(now, &mut RunCtx::for_test());
        let line = notification_line(&effects).expect("a notification");
        assert_eq!(line, "claude needs you — tab 1 is blocked");
    });
}

/// Output in another project's active tab marks it: no one is looking at
/// any of that project's tabs.
#[test]
fn output_in_another_projects_active_tab_marks_it() {
    with_two(|t| {
        let (app, x) = (&mut t.app, t.x);
        app.open_tab_in_project(x, &t.x_home);
        app.make_active(t.y);
        type_into(app, x, b"hello\n");
        assert!(
            drain_until(app, |app| first_tab(app, x, |tab| tab.info.has_activity)),
            "X's tab printed unseen, and it isn't marked"
        );
        app.make_active(x);
        assert_eq!(tab_glyph(app, 1), Some(' '), "in view, its + is seen");
    });
}

/// A blocked agent's wait is timed from when it began, kept while it lasts
/// and dropped once it ends; an unseen done keeps the time of the first done.
#[test]
fn a_blocked_wait_and_an_unseen_done_keep_their_first_time() {
    with_two(|t| {
        let (app, x, x_home) = (&mut t.app, t.x, t.x_home.clone());
        agent_tab_in(app, x, &x_home, "claude");
        app.make_active(t.y);
        let now = Instant::now();
        let at = |secs| now + Duration::from_secs(secs);
        let since = |app: &mut App| first_tab(app, x, |tab| tab.info.blocked_since);
        let unseen = |app: &mut App| first_tab(app, x, |tab| tab.info.unseen_done);
        for (secs, status) in [(0, AgentActivity::Blocked), (5, AgentActivity::Blocked)] {
            edit_first_tab(app, x, |info| {
                info.reported = Some(report(status, at(secs)));
            });
            app.settle_agent_activity(at(secs), &mut RunCtx::for_test());
        }
        assert_eq!(since(app), Some(now), "the wait keeps its start");
        for (secs, status) in [
            (10, AgentActivity::Done),
            (20, AgentActivity::Working),
            (30, AgentActivity::Done),
        ] {
            edit_first_tab(app, x, |info| {
                info.reported = Some(report(status, at(secs)));
            });
            app.settle_agent_activity(at(secs), &mut RunCtx::for_test());
        }
        assert_eq!(since(app), None, "the wait ended");
        assert_eq!(unseen(app), Some(at(10)), "the first done is kept");
    });
}

/// An agent that finished and exited out of view keeps its unseen done until
/// the user looks: the done is what they haven't seen. Its wait, if it had
/// one, ends with it.
#[test]
fn an_exited_agent_keeps_its_unseen_done_until_seen() {
    with_two(|t| {
        let (app, x) = (&mut t.app, t.x);
        app.open_tab_in_project(x, &t.x_home);
        app.make_active(t.y);
        let now = Instant::now();
        edit_first_tab(app, x, |info| {
            info.unseen_done = Some(now);
            info.blocked_since = Some(now);
        });
        type_into(app, x, b"\x04");
        assert!(drain_until(app, |app| first_tab(app, x, |tab| tab
            .pane
            .is_closed())));
        app.settle_agent_activity(now, &mut RunCtx::for_test());
        assert_eq!(
            first_tab(app, x, |tab| (tab.info.unseen_done, tab.info.blocked_since)),
            (Some(now), None),
            "the exit took its done with it"
        );
        app.make_active(x);
        app.settle_agent_activity(now, &mut RunCtx::for_test());
        assert_eq!(first_tab(app, x, |tab| tab.info.unseen_done), None, "seen");
    });
}

/// A bell rung in the project in view flashes nothing new: the user is there.
#[test]
fn the_bell_in_view_names_nothing() {
    with_two(|t| {
        let (app, x, x_home) = (&mut t.app, t.x, t.x_home.clone());
        agent_tab_in(app, x, &x_home, "claude");
        notify_in(app, x, |notify| notify.visual = true);
        let now = Instant::now();
        edit_first_tab(app, x, |info| {
            info.reported = Some(report(AgentActivity::Blocked, now));
        });
        app.settle_agent_activity(now, &mut RunCtx::for_test());
        assert!(app.view.visual_bell.is_some(), "the bell rang");
        assert_eq!(app.flash_text(), None);
    });
}

/// An error on the status line outranks the bell's "where": a bell from
/// another project leaves it showing.
#[test]
fn the_bell_leaves_an_error_showing() {
    with_two(|t| {
        let (app, x, x_home) = (&mut t.app, t.x, t.x_home.clone());
        agent_tab_in(app, x, &x_home, "claude");
        notify_in(app, x, |notify| notify.visual = true);
        let now = Instant::now();
        edit_first_tab(app, x, |info| {
            info.reported = Some(report(AgentActivity::Blocked, now));
        });
        app.make_active(t.y);
        app.state.flash_error("disk full");
        app.settle_agent_activity(now, &mut RunCtx::for_test());
        assert!(app.view.visual_bell.is_some(), "the bell rang");
        assert_eq!(app.flash_text(), Some("disk full"));
    });
}

/// The tab in view is the active tab of the project in view while its pane is
/// drawn: hiding the pane, zooming the list or the right column, or a modal
/// pager or full-screen image over the frame leaves none; a `D` pager doesn't.
#[test]
fn the_tab_in_view_needs_its_pane_drawn() {
    with_two(|t| {
        let (app, x, x_home) = (&mut t.app, t.x, t.x_home.clone());
        agent_tab_in(app, x, &x_home, "claude");
        assert_eq!(app.in_view_tab(), Some(0));
        app.state.active_project_mut().pane.pane_hidden = true;
        assert_eq!(app.in_view_tab(), None, "the pane is hidden");
        app.state.active_project_mut().pane.pane_hidden = false;
        for zoom in [state::ZoomTarget::TopList, state::ZoomTarget::RightColumn] {
            app.state.active_project_mut().pane.zoom = zoom;
            assert_eq!(app.in_view_tab(), None, "zoomed onto {zoom:?}");
        }
        app.state.active_project_mut().pane.zoom = state::ZoomTarget::BottomPane;
        assert_eq!(app.in_view_tab(), Some(0), "zoomed onto the pane");
        app.state.active_project_mut().pane.zoom = state::ZoomTarget::None;
        app.set_pager(crate::ui::pager::PagerView::new_plain(
            "help",
            vec!["help".to_string()],
        ));
        assert_eq!(app.in_view_tab(), None, "a modal pager covers it");
        app.clear_pager();
        let mut d = crate::ui::pager::PagerView::new_plain("d", vec!["d".to_string()]);
        d.mount = crate::ui::pager::Mount::TopPane;
        app.set_pager(d);
        assert_eq!(app.in_view_tab(), Some(0), "a D pager keeps the pane");
        app.clear_pager();
        let protocol = crate::app::image_ops::raster_to_protocol(
            image::DynamicImage::new_rgba8(1, 1),
            &ratatui_image::picker::Picker::halfblocks(),
            1,
            1,
        )
        .unwrap();
        app.view.image_view = Some(crate::app::ImageView {
            protocol,
            encoded: Vec::new(),
            ext: "png",
            dims: (1, 1),
            origin: crate::app::image_ops::ImageOrigin::Pending { seq: 0 },
            dark: false,
            flash: None,
        });
        assert_eq!(app.in_view_tab(), None, "a full-screen image covers it");
        app.view.image_view = None;
        app.make_active(t.y);
        assert_eq!(app.in_view_tab(), None, "Y has no tabs");
    });
}

/// A blocked agent in another project rings the bell, and the flash beside it
/// says where: the border pulse alone says only "look".
#[test]
fn the_bell_for_another_project_names_it() {
    with_two(|t| {
        let (app, x, x_home) = (&mut t.app, t.x, t.x_home.clone());
        agent_tab_in(app, x, &x_home, "claude");
        notify_in(app, x, |notify| notify.visual = true);
        let now = Instant::now();
        edit_first_tab(app, x, |info| {
            info.reported = Some(report(AgentActivity::Blocked, now));
        });
        app.make_active(t.y);
        app.settle_agent_activity(now, &mut RunCtx::for_test());
        assert!(app.view.visual_bell.is_some(), "the bell rang");
        assert_eq!(app.flash_text(), Some("xproj: tab 1 is blocked"));
    });
}

// ── #99 step 6b: `Space !` and `^a !`, to the agent that needs you ─────────

/// `Space !` from the file list.
fn space_bang(app: &mut App) {
    app.handle_key(key(' ')).unwrap();
    app.handle_key(key('!')).unwrap();
}

/// Project `id`'s tab `index` reports `status` at `at`, and the walk sees it.
pub(super) fn report_at(
    app: &mut App,
    id: state::ProjectId,
    index: usize,
    status: AgentActivity,
    at: Instant,
) {
    edit_tab(app, id, index, |info| {
        info.reported = Some(report(status, at));
    });
    app.settle_agent_activity(at, &mut RunCtx::for_test());
}

/// The active tab of the project in view, and whether the pane has the keys.
fn where_now(app: &App) -> (state::ProjectId, usize, bool) {
    let tab = app
        .runtime
        .project()
        .pane_tabs
        .as_ref()
        .map_or(0, crate::pane::PaneTabs::active_index);
    (app.state.active, tab, app.state.pane_focused())
}

/// `Space !` goes to the agent that needs you, in any project: its project,
/// its tab, and the pane focused on it.
#[test]
fn space_bang_goes_to_a_blocked_agent_in_another_project() {
    with_two(|t| {
        let (app, x, x_home) = (&mut t.app, t.x, t.x_home.clone());
        agent_tab_in(app, x, &x_home, "claude");
        agent_tab_in(app, x, &x_home, "claude");
        activate(app, x, 0);
        app.make_active(t.y);
        report_at(app, x, 1, AgentActivity::Blocked, Instant::now());
        space_bang(app);
        assert_eq!(where_now(app), (x, 1, true));
    });
}

/// The longest-blocked agent comes first, and `Space !` again goes on to the
/// next, so pressing it walks through every agent waiting on you.
#[test]
fn space_bang_again_goes_to_the_next_agent() {
    with_two(|t| {
        let (app, x, y, x_home, y_home) =
            (&mut t.app, t.x, t.y, t.x_home.clone(), t.y_home.clone());
        agent_tab_in(app, x, &x_home, "claude");
        agent_tab_in(app, x, &x_home, "cat");
        agent_tab_in(app, y, &y_home, "claude");
        // The user is on x's other tab, so the first press starts from the top.
        activate(app, x, 1);
        let now = Instant::now();
        report_at(app, y, 0, AgentActivity::Blocked, now);
        report_at(
            app,
            x,
            0,
            AgentActivity::Blocked,
            now + Duration::from_secs(5),
        );
        space_bang(app);
        assert_eq!(where_now(app), (y, 0, true), "y has waited longer");
        app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL))
            .unwrap();
        app.handle_key(key('!')).unwrap();
        assert_eq!(where_now(app), (x, 0, true), "then x");
    });
}

/// With nothing blocked, a done the user hasn't seen is next.
#[test]
fn space_bang_goes_to_an_unseen_done() {
    with_two(|t| {
        let (app, x, x_home) = (&mut t.app, t.x, t.x_home.clone());
        agent_tab_in(app, x, &x_home, "claude");
        app.make_active(t.y);
        report_at(app, x, 0, AgentActivity::Done, Instant::now());
        space_bang(app);
        assert_eq!(where_now(app), (x, 0, true));
    });
}

/// With nothing waiting, it says so and goes nowhere.
#[test]
fn space_bang_with_nothing_waiting_says_so() {
    with_two(|t| {
        let (app, x, x_home) = (&mut t.app, t.x, t.x_home.clone());
        agent_tab_in(app, x, &x_home, "claude");
        app.make_active(t.y);
        report_at(app, x, 0, AgentActivity::Working, Instant::now());
        space_bang(app);
        assert_eq!(app.state.active, t.y);
        assert_eq!(app.flash_text(), Some("nothing needs you"));
    });
}

/// `^a !` reaches it from inside an agent's pane, in two keys.
#[test]
fn ctrl_a_bang_works_from_the_pane() {
    with_two(|t| {
        let (app, x, y, x_home, y_home) =
            (&mut t.app, t.x, t.y, t.x_home.clone(), t.y_home.clone());
        agent_tab_in(app, x, &x_home, "claude");
        agent_tab_in(app, y, &y_home, "claude");
        app.make_active(y);
        app.state.active_project_mut().focus = state::Focus::Pane;
        report_at(app, x, 0, AgentActivity::Blocked, Instant::now());
        app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL))
            .unwrap();
        app.handle_key(key('!')).unwrap();
        assert_eq!(where_now(app), (x, 0, true));
    });
}

/// With one project open it still goes to the tab, and brings a hidden or
/// zoomed-away pane back into view.
#[test]
fn space_bang_in_one_project_shows_the_pane() {
    with_two(|t| {
        let (app, x, x_home) = (&mut t.app, t.x, t.x_home.clone());
        app.remove_test_project(t.y);
        agent_tab_in(app, x, &x_home, "claude");
        agent_tab_in(app, x, &x_home, "claude");
        activate(app, x, 0);
        report_at(app, x, 1, AgentActivity::Blocked, Instant::now());
        for hide in [
            |app: &mut App| app.state.active_project_mut().pane.pane_hidden = true,
            |app: &mut App| app.state.active_project_mut().pane.zoom = state::ZoomTarget::TopList,
            |app: &mut App| {
                app.state.active_project_mut().pane.zoom = state::ZoomTarget::RightColumn;
            },
        ] {
            activate(app, x, 0);
            app.state.active_project_mut().focus = state::Focus::FileList;
            hide(app);
            space_bang(app);
            assert_eq!(where_now(app), (x, 1, true));
            let pane = &app.state.active_project().pane;
            assert!(
                !pane.pane_hidden
                    && matches!(
                        pane.zoom,
                        state::ZoomTarget::None | state::ZoomTarget::BottomPane
                    ),
                "the pane is still out of view"
            );
            app.state.active_project_mut().pane.pane_hidden = false;
            app.state.active_project_mut().pane.zoom = state::ZoomTarget::None;
        }
    });
}

/// A modal pager left open in the project it goes to would cover the agent:
/// it closes, as `q` would close it, so a picker it was doesn't stay armed
/// for the next pager to open.
#[test]
fn space_bang_closes_a_pager_over_the_agent() {
    with_two(|t| {
        let (app, x, x_home) = (&mut t.app, t.x, t.x_home.clone());
        agent_tab_in(app, x, &x_home, "claude");
        app.as_project(x, App::open_project_switcher);
        assert!(app.state.pending_projects.is_some(), "the switcher is open");
        app.make_active(t.y);
        report_at(app, x, 0, AgentActivity::Blocked, Instant::now());
        space_bang(app);
        assert_eq!(app.state.active, x);
        assert_eq!(app.in_view_tab(), Some(0), "the pager still covers it");
        assert!(
            app.state.pending_projects.is_none(),
            "the picker is still armed"
        );
    });
}

/// Pressing again walks through every agent waiting: a blocked one before an
/// older unseen done, and on from a done just seen, which no longer waits,
/// to the next rather than back to the top.
#[test]
fn space_bang_walks_on_past_a_done_just_seen() {
    with_two(|t| {
        let (app, x, y, x_home, y_home) =
            (&mut t.app, t.x, t.y, t.x_home.clone(), t.y_home.clone());
        agent_tab_in(app, x, &x_home, "claude");
        agent_tab_in(app, x, &x_home, "cat");
        agent_tab_in(app, y, &y_home, "claude");
        agent_tab_in(app, y, &y_home, "claude");
        activate(app, x, 1);
        let t0 = Instant::now();
        let at = |secs| t0 + Duration::from_secs(secs);
        report_at(app, y, 0, AgentActivity::Done, at(0));
        report_at(app, y, 1, AgentActivity::Done, at(1));
        report_at(app, x, 0, AgentActivity::Blocked, at(2));
        let bang = |app: &mut App| {
            app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL))
                .unwrap();
            app.handle_key(key('!')).unwrap();
            app.settle_agent_activity(at(3), &mut RunCtx::for_test());
            where_now(app)
        };
        assert_eq!(bang(app), (x, 0, true), "blocked before an older done");
        assert_eq!(bang(app), (y, 0, true), "the older done next");
        assert_eq!(
            first_tab(app, y, |tab| tab.info.unseen_done),
            None,
            "the done is seen"
        );
        assert_eq!(
            bang(app),
            (y, 1, true),
            "on to the next done, not back to x"
        );
        assert_eq!(bang(app), (x, 0, true), "and round to the top");
    });
}

/// A done the user hasn't seen, on an agent that has gone back to work, isn't
/// waiting on them: its dot says working.
#[test]
fn an_agent_back_at_work_is_not_waiting() {
    with_two(|t| {
        let (app, x, x_home) = (&mut t.app, t.x, t.x_home.clone());
        agent_tab_in(app, x, &x_home, "claude");
        app.make_active(t.y);
        let now = Instant::now();
        report_at(app, x, 0, AgentActivity::Done, now);
        report_at(
            app,
            x,
            0,
            AgentActivity::Working,
            now + Duration::from_secs(1),
        );
        space_bang(app);
        assert_eq!(app.state.active, t.y);
        assert_eq!(app.flash_text(), Some("nothing needs you"));
    });
}
