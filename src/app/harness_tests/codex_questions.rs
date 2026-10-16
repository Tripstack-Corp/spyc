use super::agent_readiness::{dump, why_status};
use super::*;
use crate::mcp_cmd::{McpCommand, McpResponse};
use crate::pane::AgentActivity;
use std::time::{Duration, Instant};

const OPEN: &str = include_str!("../../../tests/fixtures/codex-questions/expanded-options.txt");
const COLLAPSED: &str = include_str!("../../../tests/fixtures/codex-questions/collapsed.txt");
const CLEAR: &str = "ANSWERED\n\n› Ask Codex to do anything\n\n100% context left | Vim: Insert";

fn add_pane(app: &mut App, dir: &std::path::Path) {
    assert!(app.open_pane_tab_in("sh -c 'stty raw -echo; printf READY; exec cat'", dir));
    wait_screen(app, "READY");
    let entry = app
        .runtime
        .project_mut()
        .pane_tabs
        .as_mut()
        .unwrap()
        .tabs_mut()
        .last_mut()
        .unwrap();
    entry.info.command = "codex".into();
    entry.pane.resize(30, 100).unwrap();
}

fn wait_screen(app: &mut App, expected: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let pane = app
            .runtime
            .project_mut()
            .pane_tabs
            .as_mut()
            .unwrap()
            .active_mut();
        pane.drain_output();
        let visible = pane.visible_lines().join("\n");
        if visible.contains(expected) {
            return;
        }
        assert!(Instant::now() < deadline, "missing {expected}: {visible}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn paint(app: &mut App, text: &str, expected: &str) {
    app.runtime
        .project_mut()
        .pane_tabs
        .as_mut()
        .unwrap()
        .active_mut()
        .send_bytes(format!("\x1b[2J\x1b[H{}", text.replace('\n', "\r\n")).as_bytes())
        .unwrap();
    wait_screen(app, expected);
}

fn report(app: &mut App, status: &str) {
    let result = app.execute_mcp_command(McpCommand::ReportStatus {
        pane_id: None,
        pane: None,
        status: status.into(),
        ttl_ms: Some(120_000),
        note: None,
        session_id: None,
        hook_event: None,
    });
    assert!(matches!(result, McpResponse::Ok { .. }), "{result:?}");
}

fn settle(app: &mut App, base: Instant) -> Vec<Effect> {
    let mut ctx = RunCtx::for_test();
    for elapsed in [0, 100, 200, 300] {
        let now = base + Duration::from_millis(elapsed);
        let info = app
            .runtime
            .project_mut()
            .pane_tabs
            .as_mut()
            .unwrap()
            .active_info_mut();
        info.last_output_at = Some(now);
        info.scrape_dirty = true;
        app.settle_scrape_quiet(now, &mut ctx);
    }
    app.settle_agent_activity(base + Duration::from_millis(300), &mut ctx)
        .1
}

#[test]
fn codex_questions_native_view_requires_attention_during_work_and_retains_the_report() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(tmp.path().to_path_buf());
        add_pane(&mut app, tmp.path());
        report(&mut app, "working");
        let original = app
            .runtime
            .project()
            .pane_tabs
            .as_ref()
            .unwrap()
            .active_info()
            .reported
            .clone()
            .unwrap();
        app.state.active_project_mut().focus = state::Focus::FileList;
        paint(&mut app, OPEN, "enter submit");
        let effects = settle(&mut app, Instant::now());
        let info = app
            .runtime
            .project()
            .pane_tabs
            .as_ref()
            .unwrap()
            .active_info();
        assert_eq!(
            info.activity,
            AgentActivity::Blocked,
            "continued redraws must not hide a question"
        );
        assert_eq!(info.reported.as_ref().unwrap().at, original.at);
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::Notify { .. }))
        );
        assert!(
            dump(&mut app).contains(
                "source: SCRAPE-FALLBACK status=blocked (awaiting async question answer)"
            )
        );
        assert!(why_status(&mut app).contains("awaiting async question answer"));
        paint(&mut app, COLLAPSED, "to answer");
        settle(&mut app, Instant::now());
        assert_eq!(
            app.runtime
                .project()
                .pane_tabs
                .as_ref()
                .unwrap()
                .active_info()
                .activity,
            AgentActivity::Blocked
        );
        paint(&mut app, CLEAR, "ANSWERED");
        settle(&mut app, Instant::now());
        let info = app
            .runtime
            .project()
            .pane_tabs
            .as_ref()
            .unwrap()
            .active_info();
        assert_eq!(info.activity, AgentActivity::Working);
        assert_eq!(info.reported.as_ref().unwrap().at, original.at);
    });
}

#[test]
fn codex_questions_latest_done_report_resumes_after_the_native_view_clears() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(tmp.path().to_path_buf());
        add_pane(&mut app, tmp.path());
        paint(&mut app, OPEN, "enter submit");
        report(&mut app, "working");
        settle(&mut app, Instant::now());
        report(&mut app, "done");
        settle(&mut app, Instant::now());
        assert_eq!(
            app.runtime
                .project()
                .pane_tabs
                .as_ref()
                .unwrap()
                .active_info()
                .activity,
            AgentActivity::Blocked
        );
        paint(&mut app, CLEAR, "ANSWERED");
        settle(&mut app, Instant::now());
        assert_eq!(
            app.runtime
                .project()
                .pane_tabs
                .as_ref()
                .unwrap()
                .active_info()
                .activity,
            AgentActivity::Done
        );
    });
}

#[test]
fn codex_questions_each_pane_keeps_its_own_unanswered_view() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(tmp.path().to_path_buf());
        add_pane(&mut app, tmp.path());
        report(&mut app, "working");
        paint(&mut app, OPEN, "enter submit");
        settle(&mut app, Instant::now());
        add_pane(&mut app, tmp.path());
        report(&mut app, "working");
        paint(&mut app, OPEN, "enter submit");
        settle(&mut app, Instant::now());
        assert!(
            app.runtime
                .project()
                .pane_tabs
                .as_ref()
                .unwrap()
                .tabs()
                .iter()
                .all(|tab| tab.info.activity == AgentActivity::Blocked)
        );
        paint(&mut app, CLEAR, "ANSWERED");
        settle(&mut app, Instant::now());
        let tabs = app.runtime.project().pane_tabs.as_ref().unwrap().tabs();
        assert_eq!(tabs[0].info.activity, AgentActivity::Blocked);
        assert_eq!(tabs[1].info.activity, AgentActivity::Working);
        assert_ne!(tabs[0].info.id, tabs[1].info.id);
    });
}

#[test]
fn codex_questions_a_semantic_block_cannot_erase_the_unanswered_view() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(tmp.path().to_path_buf());
        add_pane(&mut app, tmp.path());
        report(&mut app, "working");
        paint(&mut app, OPEN, "enter submit");
        settle(&mut app, Instant::now());
        report(&mut app, "blocked");
        app.settle_agent_activity(Instant::now(), &mut RunCtx::for_test());
        assert!(dump(&mut app).contains("source: SELF-REPORT status=blocked"));
        report(&mut app, "working");
        app.settle_agent_activity(Instant::now(), &mut RunCtx::for_test());
        assert_eq!(
            app.runtime
                .project()
                .pane_tabs
                .as_ref()
                .unwrap()
                .active_info()
                .activity,
            AgentActivity::Blocked,
            "the same unanswered question must remain visible without another child repaint"
        );
        assert!(dump(&mut app).contains("awaiting async question answer"));
        report(&mut app, "blocked");
        paint(&mut app, CLEAR, "ANSWERED");
        settle(&mut app, Instant::now());
        assert_eq!(
            app.runtime
                .project()
                .pane_tabs
                .as_ref()
                .unwrap()
                .active_info()
                .activity,
            AgentActivity::Blocked,
            "clearing the view must not retire an independent semantic block"
        );
    });
}

#[test]
fn codex_questions_expired_reports_do_not_become_working_after_the_view_clears() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(tmp.path().to_path_buf());
        add_pane(&mut app, tmp.path());
        let now = Instant::now();
        app.runtime
            .project_mut()
            .pane_tabs
            .as_mut()
            .unwrap()
            .active_info_mut()
            .reported = Some(crate::pane::ReportedStatus::new(
            AgentActivity::Working,
            now,
            now + Duration::from_secs(30),
        ));
        paint(&mut app, OPEN, "enter submit");
        settle(&mut app, now + Duration::from_secs(90));
        assert_eq!(
            app.runtime
                .project()
                .pane_tabs
                .as_ref()
                .unwrap()
                .active_info()
                .activity,
            AgentActivity::Blocked
        );
        assert!(
            app.runtime
                .project()
                .pane_tabs
                .as_ref()
                .unwrap()
                .active_info()
                .reported
                .is_none()
        );
        paint(&mut app, CLEAR, "ANSWERED");
        settle(&mut app, now + Duration::from_secs(90));
        app.settle_agent_activity(now + Duration::from_secs(100), &mut RunCtx::for_test());
        assert_eq!(
            app.runtime
                .project()
                .pane_tabs
                .as_ref()
                .unwrap()
                .active_info()
                .activity,
            AgentActivity::Idle
        );
    });
}
