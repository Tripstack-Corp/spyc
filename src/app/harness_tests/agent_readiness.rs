use super::*;
use crate::mcp_cmd::{McpCommand, McpResponse};
use crate::pane::AgentActivity;
use std::time::Duration;

pub(super) fn agent_app(dir: &std::path::Path, name: &str) -> App {
    let mut app = App::test_app(dir.to_path_buf());
    assert!(app.open_pane_tab_in("cat", dir));
    app.runtime
        .project_mut()
        .pane_tabs
        .as_mut()
        .unwrap()
        .active_info_mut()
        .command = name.into();
    if name == "codex" {
        app.runtime
            .project_mut()
            .pane_tabs
            .as_mut()
            .unwrap()
            .active_info_mut()
            .codex_home = Some(dir.join("codex-home"));
    }
    app
}

fn report(app: &mut App, status: &str) {
    assert!(matches!(
        app.execute_mcp_command(McpCommand::ReportStatus {
            pane_id: None,
            pane: None,
            status: status.into(),
            ttl_ms: Some(120_000),
            note: None,
            session_id: None,
            hook_event: None,
        }),
        McpResponse::Ok { .. }
    ));
}

pub(super) fn dump(app: &mut App) -> String {
    pager_text(app, "activity dump")
}

/// `:why-status`'s pager text.
pub(super) fn why_status(app: &mut App) -> String {
    pager_text(app, "why-status")
}

/// The pager `command` opens. Any pager already open is closed first, so a
/// command that opens none can't be read as the previous one.
fn pager_text(app: &mut App, command: &str) -> String {
    app.view.project_mut().pager = None;
    app.dispatch_command(command);
    app.view
        .project()
        .pager
        .as_ref()
        .unwrap()
        .lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// An `idle` report from codex or claude (codex's `Interrupt` hook sends one)
/// holds through the redraw that follows it, as every other report of theirs
/// does: output after it is not a new turn, so the dot stays idle rather than
/// flipping to the timing guess.
#[test]
fn a_codex_or_claude_idle_report_survives_the_redraw_after_it() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        for name in ["codex", "claude"] {
            let mut app = agent_app(tmp.path(), name);
            report(&mut app, "idle");
            let at = app
                .runtime
                .project()
                .pane_tabs
                .as_ref()
                .unwrap()
                .active_info()
                .reported
                .clone()
                .unwrap()
                .at;
            app.runtime
                .project_mut()
                .pane_tabs
                .as_mut()
                .unwrap()
                .active_info_mut()
                .last_output_at = Some(at + Duration::from_secs(1));
            let mut ctx = RunCtx::for_test();
            app.settle_agent_activity(at + Duration::from_millis(1500), &mut ctx);
            let info = app
                .runtime
                .project()
                .pane_tabs
                .as_ref()
                .unwrap()
                .active_info();
            assert_eq!(
                info.activity,
                AgentActivity::Idle,
                "{name}: a redraw must not turn an idle report into working"
            );
            assert!(info.reported.is_some(), "{name}: the report was dropped");
        }
    });
}

#[test]
fn codex_and_claude_reports_survive_output_and_quiet_tool_waits() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        for name in ["codex", "claude"] {
            let mut app = agent_app(tmp.path(), name);
            report(&mut app, "working");
            let at = app
                .runtime
                .project()
                .pane_tabs
                .as_ref()
                .unwrap()
                .active_info()
                .reported
                .clone()
                .unwrap()
                .at;
            app.runtime
                .project_mut()
                .pane_tabs
                .as_mut()
                .unwrap()
                .active_info_mut()
                .last_output_at = Some(at + Duration::from_secs(1));
            let mut ctx = RunCtx::for_test();
            app.settle_agent_activity(at + Duration::from_secs(30), &mut ctx);
            let info = app
                .runtime
                .project()
                .pane_tabs
                .as_ref()
                .unwrap()
                .active_info();
            assert_eq!(
                info.activity,
                AgentActivity::Working,
                "{name}: output must not erase a working report"
            );
            assert!(info.reported.is_some());

            report(&mut app, "waiting");
            let at = app
                .runtime
                .project()
                .pane_tabs
                .as_ref()
                .unwrap()
                .active_info()
                .reported
                .clone()
                .unwrap()
                .at;
            app.runtime
                .project_mut()
                .pane_tabs
                .as_mut()
                .unwrap()
                .active_info_mut()
                .last_output_at = Some(at + Duration::from_secs(1));
            app.settle_agent_activity(at + Duration::from_secs(30), &mut ctx);
            assert_eq!(
                app.runtime
                    .project()
                    .pane_tabs
                    .as_ref()
                    .unwrap()
                    .active_info()
                    .activity,
                AgentActivity::Waiting,
                "{name}: background task redraw must not erase a waiting report"
            );

            report(&mut app, "done");
            let at = app
                .runtime
                .project()
                .pane_tabs
                .as_ref()
                .unwrap()
                .active_info()
                .reported
                .clone()
                .unwrap()
                .at;
            app.runtime
                .project_mut()
                .pane_tabs
                .as_mut()
                .unwrap()
                .active_info_mut()
                .last_output_at = Some(at + Duration::from_secs(1));
            app.settle_agent_activity(at + Duration::from_secs(30), &mut ctx);
            assert_eq!(
                app.runtime
                    .project()
                    .pane_tabs
                    .as_ref()
                    .unwrap()
                    .active_info()
                    .activity,
                AgentActivity::Done,
                "{name}: footer redraw is not a new turn"
            );
            app.settle_agent_activity(at + Duration::from_secs(120), &mut ctx);
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
            assert!(
                ctx.scheduler.next().is_none(),
                "expired reports must not keep idle animation running"
            );
        }
    });
}

fn tab(app: &App) -> &crate::pane::TabInfo {
    app.runtime
        .project()
        .pane_tabs
        .as_ref()
        .unwrap()
        .active_info()
}

fn tab_mut(app: &mut App) -> &mut crate::pane::TabInfo {
    app.runtime
        .project_mut()
        .pane_tabs
        .as_mut()
        .unwrap()
        .active_info_mut()
}

/// Reports `status` and returns the instant the report landed.
fn report_at(app: &mut App, status: &str) -> std::time::Instant {
    report(app, status);
    tab(app).reported.as_ref().unwrap().at
}

/// agy's spinner redraws during a turn, and a pause shorter than the quiet
/// window, leave its `working` report standing.
#[test]
fn agy_working_survives_redraws_and_a_brief_pause() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = agent_app(tmp.path(), "agy");
        let at = report_at(&mut app, "working");
        tab_mut(&mut app).last_output_at = Some(at + Duration::from_secs(1));
        app.settle_agent_activity(at + Duration::from_secs(5), &mut RunCtx::for_test());
        assert_eq!(tab(&app).activity, AgentActivity::Working);
    });
}

/// Output during the turn restarts the quiet window.
#[test]
fn agy_working_quiet_window_restarts_on_later_output() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = agent_app(tmp.path(), "agy");
        let at = report_at(&mut app, "working");
        tab_mut(&mut app).last_output_at = Some(at + Duration::from_secs(20));
        app.settle_agent_activity(at + Duration::from_secs(25), &mut RunCtx::for_test());
        assert_eq!(tab(&app).activity, AgentActivity::Working);
    });
}

/// With no `Stop`, 15 s of pane silence ends agy's `working`: the report is
/// dropped, output timing says idle, and nothing stays armed to pulse the dot.
#[test]
#[ignore = "red: agy's working report stands for its whole TTL when Stop never arrives"]
fn agy_working_yields_to_timing_after_the_quiet_window() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = agent_app(tmp.path(), "agy");
        let at = report_at(&mut app, "working");
        tab_mut(&mut app).last_output_at = Some(at + Duration::from_secs(20));
        let mut ctx = RunCtx::for_test();
        app.settle_agent_activity(at + Duration::from_secs(35), &mut ctx);
        assert_eq!(tab(&app).activity, AgentActivity::Idle);
        assert!(tab(&app).reported.is_none(), "the quiet report is dropped");
        assert!(
            ctx.scheduler.next().is_none(),
            "nothing stays armed to pulse the dot"
        );
    });
}

/// The quiet window applies to `working` only: agy's `waiting` and `done`
/// survive the redraws after `Stop`, and `done` ends at its TTL.
#[test]
fn agy_waiting_and_done_survive_post_stop_redraws() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = agent_app(tmp.path(), "agy");
        for (status, expected) in [
            ("waiting", AgentActivity::Waiting),
            ("done", AgentActivity::Done),
        ] {
            let at = report_at(&mut app, status);
            tab_mut(&mut app).last_output_at = Some(at + Duration::from_secs(1));
            let mut ctx = RunCtx::for_test();
            app.settle_agent_activity(at + Duration::from_secs(30), &mut ctx);
            assert_eq!(tab(&app).activity, expected, "agy {status}");
        }
        let at = tab(&app).reported.as_ref().unwrap().at;
        let mut ctx = RunCtx::for_test();
        app.settle_agent_activity(at + Duration::from_secs(300), &mut ctx);
        assert_eq!(
            tab(&app).activity,
            AgentActivity::Idle,
            "done ends at its TTL"
        );
        assert!(ctx.scheduler.next().is_none());
    });
}

/// The dialog draws at 10 s, which is the pane's last output, and is answered
/// at 60 s. The post-answer grace counts from the answer, not from the draw.
#[test]
#[ignore = "red: agy's working report has no quiet window to measure from the answer"]
fn agy_working_grace_counts_from_the_answer_not_the_dialog_draw() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = agent_app(tmp.path(), "agy");
        let at = report_at(&mut app, "working");
        let mut ctx = RunCtx::for_test();
        {
            let info = tab_mut(&mut app);
            info.last_output_at = Some(at + Duration::from_secs(10));
            info.scrape_status = Some((
                AgentActivity::Blocked,
                Some("awaiting tool-execution approval"),
            ));
        }
        app.settle_agent_activity(at + Duration::from_secs(10), &mut ctx);
        app.settle_agent_activity(at + Duration::from_secs(30), &mut ctx);
        assert_eq!(tab(&app).activity, AgentActivity::Blocked);
        app.settle_agent_activity(at + Duration::from_secs(60), &mut ctx);
        crate::app::effect::clear_blocked_for_input(
            tab_mut(&mut app),
            &crate::app::effect::PaneInput::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            None,
        );
        app.settle_agent_activity(at + Duration::from_secs(61), &mut ctx);
        app.settle_agent_activity(at + Duration::from_secs(70), &mut ctx);
        assert_eq!(
            tab(&app).activity,
            AgentActivity::Working,
            "nine seconds after the answer is inside the grace"
        );
        app.settle_agent_activity(at + Duration::from_secs(80), &mut ctx);
        assert_eq!(tab(&app).activity, AgentActivity::Idle);
    });
}

#[test]
fn agy_working_why_status_reports_quiet_expiry_alongside_ttl_backstop() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = agent_app(tmp.path(), "agy");
        report(&mut app, "working");
        let at = app
            .runtime
            .project()
            .pane_tabs
            .as_ref()
            .unwrap()
            .active_info()
            .reported
            .clone()
            .unwrap()
            .at;
        let why = why_status(&mut app);
        assert!(
            why.contains("of quiet (") && why.contains("TTL backstop)"),
            "why-status must report agy's working-quiet window alongside the TTL backstop: {why}"
        );
        let mut ctx = RunCtx::for_test();
        app.settle_agent_activity(at + Duration::from_secs(15), &mut ctx);
        let why = why_status(&mut app);
        assert!(
            why.contains("no longer authoritative: pane quiet for 15s"),
            "why-status must explain that agy working yielded after 15s of pane silence: {why}"
        );
    });
}

#[test]
fn agy_working_approval_override_preserves_post_answer_quiet_grace() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = agent_app(tmp.path(), "agy");
        report(&mut app, "working");
        let at = app
            .runtime
            .project()
            .pane_tabs
            .as_ref()
            .unwrap()
            .active_info()
            .reported
            .clone()
            .unwrap()
            .at;
        let mut ctx = RunCtx::for_test();
        app.runtime
            .project_mut()
            .pane_tabs
            .as_mut()
            .unwrap()
            .active_info_mut()
            .scrape_status = Some((AgentActivity::Blocked, Some("agy waiting on tool approval")));
        app.settle_agent_activity(at + Duration::from_secs(10), &mut ctx);
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
        {
            let info = app
                .runtime
                .project_mut()
                .pane_tabs
                .as_mut()
                .unwrap()
                .active_info_mut();
            crate::app::effect::clear_blocked_for_input(
                info,
                &crate::app::effect::PaneInput::Key(KeyEvent::new(
                    KeyCode::Enter,
                    KeyModifiers::NONE,
                )),
                None,
            );
        }
        app.settle_agent_activity(at + Duration::from_secs(30), &mut ctx);
        app.settle_agent_activity(at + Duration::from_secs(40), &mut ctx);
        assert_eq!(
            app.runtime
                .project()
                .pane_tabs
                .as_ref()
                .unwrap()
                .active_info()
                .activity,
            AgentActivity::Working,
            "answering an approval after >15s must not drop working before the 15s post-answer window"
        );
        let mut ctx = RunCtx::for_test();
        app.settle_agent_activity(at + Duration::from_secs(45), &mut ctx);
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

#[test]
fn codex_report_history_is_pane_local_and_not_carried_into_a_restart() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let mut app = agent_app(&dir, "codex");
        let first = app
            .runtime
            .project()
            .pane_tabs
            .as_ref()
            .unwrap()
            .active_info()
            .id
            .clone();
        assert!(app.open_pane_tab_in("cat", &dir));
        app.runtime
            .project_mut()
            .pane_tabs
            .as_mut()
            .unwrap()
            .active_info_mut()
            .command = "codex".into();
        app.execute_mcp_command(McpCommand::ReportStatus {
            pane_id: Some(first),
            pane: None,
            status: "blocked".into(),
            ttl_ms: Some(1),
            note: None,
            session_id: None,
            hook_event: None,
        });
        let at = app.runtime.project().pane_tabs.as_ref().unwrap().tabs()[0]
            .info
            .reported
            .clone()
            .unwrap()
            .at;
        app.runtime
            .project_mut()
            .pane_tabs
            .as_mut()
            .unwrap()
            .tabs_mut()[0]
            .info
            .last_output_at = Some(at + Duration::from_secs(2));
        let mut ctx = RunCtx::for_test();
        app.settle_agent_activity(at + Duration::from_secs(60), &mut ctx);
        let tabs = app.runtime.project().pane_tabs.as_ref().unwrap();
        assert_eq!(tabs.tabs()[0].info.activity, AgentActivity::Blocked);
        assert_eq!(
            tabs.tabs()[0].info.last_reported.clone().unwrap().status,
            AgentActivity::Blocked
        );
        assert!(tabs.tabs()[1].info.last_reported.is_none());
        assert!(dump(&mut app).contains("latched until answered/dismissed"));

        let command = fake_agent(&dir, "codex").display().to_string();
        assert!(app.spawn_agent_into_tab(0, &command, &dir, None));
        assert!(
            app.runtime.project().pane_tabs.as_ref().unwrap().tabs()[0]
                .info
                .last_reported
                .is_none()
        );
    });
}

#[test]
fn status_hook_event_history_is_bounded_pane_local_and_cleared_on_restart() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let mut app = agent_app(&dir, "codex");
        let first = app
            .runtime
            .project()
            .pane_tabs
            .as_ref()
            .unwrap()
            .active_info()
            .id
            .clone();
        assert!(app.open_pane_tab_in("cat", &dir));
        for index in 0..10 {
            let event = crate::agent::status_hook::StatusHookEvent::from_value(&serde_json::json!({
                "hook_event_name":"PermissionRequest", "tool_name":"Bash", "turn_id":format!("turn-{index}"),
                "tool_input": {"command":"private-command"}
            })).unwrap();
            assert!(matches!(
                app.execute_mcp_command(McpCommand::ReportStatus {
                    pane_id: Some(first.clone()),
                    pane: None,
                    status: "blocked".into(),
                    ttl_ms: Some(1),
                    note: None,
                    session_id: None,
                    hook_event: Some(event),
                }),
                McpResponse::Ok { .. }
            ));
        }
        let tabs = app.runtime.project().pane_tabs.as_ref().unwrap();
        let history = &tabs.tabs()[0].info.recent_hook_events;
        assert_eq!(history.len(), 8);
        assert_eq!(
            history.front().unwrap().0.turn_id.as_deref(),
            Some("turn-2")
        );
        assert_eq!(history.back().unwrap().0.turn_id.as_deref(), Some("turn-9"));
        assert!(tabs.active_info().recent_hook_events.is_empty());
        let lines = dump(&mut app);
        assert!(
            lines.contains("reported metadata (unverified; oldest first)"),
            "{lines}"
        );
        assert!(
            lines.contains(
                "event=PermissionRequest status=blocked tool=Bash turn=turn-9 call=not-reported"
            ),
            "{lines}"
        );
        assert!(!lines.contains("turn=turn-0 "), "{lines}");
        assert!(!lines.contains("private-command"));

        let command = fake_agent(&dir, "codex").display().to_string();
        assert!(app.spawn_agent_into_tab(0, &command, &dir, None));
        assert!(
            app.runtime.project().pane_tabs.as_ref().unwrap().tabs()[0]
                .info
                .recent_hook_events
                .is_empty()
        );
    });
}

#[test]
fn status_hook_event_metadata_does_not_infer_permission_answers_or_hook_trust() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        crate::state::wiring_consent::set(crate::state::sessions::AgentKind::Codex, Some(true));
        let mut app = agent_app(tmp.path(), "codex");
        let event = crate::agent::status_hook::StatusHookEvent::from_value(&serde_json::json!({
            "hook_event_name":"PostToolUse", "tool_name":"Bash", "turn_id":"turn-1", "tool_use_id":"unrelated-call"
        })).unwrap();
        assert!(matches!(
            app.execute_mcp_command(McpCommand::ReportStatus {
                pane_id: None,
                pane: None,
                status: "blocked".into(),
                ttl_ms: Some(1),
                note: None,
                session_id: None,
                hook_event: Some(event),
            }),
            McpResponse::Ok { .. }
        ));
        let at = app
            .runtime
            .project()
            .pane_tabs
            .as_ref()
            .unwrap()
            .active_info()
            .reported
            .clone()
            .unwrap()
            .at;
        let mut ctx = RunCtx::for_test();
        app.settle_agent_activity(at + Duration::from_secs(60), &mut ctx);
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
        report(&mut app, "done");
        let at = app
            .runtime
            .project()
            .pane_tabs
            .as_ref()
            .unwrap()
            .active_info()
            .reported
            .clone()
            .unwrap()
            .at;
        app.settle_agent_activity(at + Duration::from_secs(121), &mut ctx);
        let lines = dump(&mut app);
        assert!(lines.contains("execution/trust unverified"), "{lines}");
        assert!(
            lines.contains("event=PostToolUse status=blocked"),
            "{lines}"
        );
        assert!(lines.contains("last_report: status=done"), "{lines}");
        assert!(lines.contains("no longer authoritative"), "{lines}");
    });
}

fn question_report(app: &mut App, signal: &str, event: &str, call: &str) {
    let result = app.execute_mcp_command(McpCommand::ReportStatus {
        pane_id: None, pane: None, status: signal.into(), ttl_ms: Some(120_000), note: None,
        session_id: Some("session-1".into()),
        hook_event: crate::agent::status_hook::StatusHookEvent::from_value(&serde_json::json!({
            "hook_event_name":event, "tool_name":"request_user_input", "turn_id":"turn-1", "tool_use_id":call
        }))
    });
    assert!(matches!(result, McpResponse::Ok { .. }), "{result:?}");
}

#[test]
fn codex_question_recovery_matches_the_call_and_survives_quiet_after_answer() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = agent_app(tmp.path(), "codex");
        question_report(&mut app, "codex-question-start", "PreToolUse", "call-1");
        question_report(
            &mut app,
            "codex-question-end",
            "PostToolUse",
            "unrelated-call",
        );
        assert!(dump(&mut app).contains("source: SELF-REPORT status=blocked"));
        assert!(dump(&mut app).contains("not applied:"));
        question_report(&mut app, "codex-question-end", "PostToolUse", "call-1");
        let at = app
            .runtime
            .project()
            .pane_tabs
            .as_ref()
            .unwrap()
            .active_info()
            .reported
            .clone()
            .unwrap()
            .at;
        let mut ctx = RunCtx::for_test();
        app.settle_agent_activity(at + Duration::from_secs(60), &mut ctx);
        assert_eq!(
            app.runtime
                .project()
                .pane_tabs
                .as_ref()
                .unwrap()
                .active_info()
                .activity,
            AgentActivity::Working
        );
        report(&mut app, "done");
        question_report(&mut app, "codex-question-end", "PostToolUse", "call-1");
        assert!(dump(&mut app).contains("source: SELF-REPORT status=done"));
    });
}

#[test]
fn codex_question_recovery_waits_for_completion_instead_of_enter() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = agent_app(tmp.path(), "codex");
        question_report(&mut app, "codex-question-start", "PreToolUse", "call-1");
        let input = crate::app::effect::PaneInput::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::empty(),
        ));
        let id = app
            .runtime
            .project()
            .pane_tabs
            .as_ref()
            .unwrap()
            .active_info()
            .id
            .clone();
        let recovery = app.state.codex_recovery.get_mut(&id);
        crate::app::effect::clear_blocked_for_input(
            app.runtime
                .project_mut()
                .pane_tabs
                .as_mut()
                .unwrap()
                .active_info_mut(),
            &input,
            recovery,
        );
        assert!(dump(&mut app).contains("source: SELF-REPORT status=blocked"));
        question_report(&mut app, "codex-question-end", "PostToolUse", "call-1");
        assert!(dump(&mut app).contains("source: SELF-REPORT status=working"));
    });
}

#[test]
fn codex_question_recovery_ignores_old_reporters_and_other_panes() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = agent_app(tmp.path(), "codex");
        assert!(matches!(
            app.execute_mcp_command(McpCommand::ReportStatus {
                pane_id: None,
                pane: None,
                status: "codex-question-start".into(),
                ttl_ms: None,
                note: None,
                session_id: Some("session-1".into()),
                hook_event: None,
            }),
            McpResponse::Ok { .. }
        ));
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
        assert!(dump(&mut app).contains("not applied: question hook requires"));
        question_report(&mut app, "codex-question-start", "PreToolUse", "call-1");
        assert!(app.open_pane_tab_in("cat", tmp.path()));
        app.runtime
            .project_mut()
            .pane_tabs
            .as_mut()
            .unwrap()
            .active_info_mut()
            .command = "codex".into();
        question_report(&mut app, "codex-question-end", "PostToolUse", "call-1");
        let tabs = app.runtime.project().pane_tabs.as_ref().unwrap();
        assert_eq!(
            tabs.tabs()[0].info.reported.clone().unwrap().status,
            AgentActivity::Blocked
        );
        assert!(
            tabs.tabs()[1].info.reported.is_none(),
            "another pane cannot answer this question"
        );
    });
}

#[test]
fn codex_question_recovery_prunes_a_replaced_pane_without_carrying_its_wait() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = agent_app(tmp.path(), "codex");
        question_report(&mut app, "codex-question-start", "PreToolUse", "call-1");
        let old = app
            .runtime
            .project()
            .pane_tabs
            .as_ref()
            .unwrap()
            .active_info()
            .id
            .clone();
        let command = fake_agent(tmp.path(), "codex").display().to_string();
        assert!(app.spawn_agent_into_tab(0, &command, tmp.path(), None));
        let new = app
            .runtime
            .project()
            .pane_tabs
            .as_ref()
            .unwrap()
            .active_info()
            .id
            .clone();
        assert_ne!(old, new);
        app.settle_agent_activity(std::time::Instant::now(), &mut RunCtx::for_test());
        assert!(!app.state.codex_recovery.contains_key(&old));
        question_report(&mut app, "codex-question-end", "PostToolUse", "call-1");
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
    });
}

#[test]
fn answered_codex_permission_does_not_poison_the_next_question_in_the_same_turn() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = agent_app(tmp.path(), "codex");
        report(&mut app, "blocked");
        let id = app
            .runtime
            .project()
            .pane_tabs
            .as_ref()
            .unwrap()
            .active_info()
            .id
            .clone();
        let recovery = app.state.codex_recovery.get_mut(&id);
        crate::app::effect::clear_blocked_for_input(
            app.runtime
                .project_mut()
                .pane_tabs
                .as_mut()
                .unwrap()
                .active_info_mut(),
            &crate::app::effect::PaneInput::Key(KeyEvent::new(
                KeyCode::Enter,
                KeyModifiers::empty(),
            )),
            recovery,
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
        question_report(&mut app, "codex-question-start", "PreToolUse", "call-1");
        question_report(&mut app, "codex-question-end", "PostToolUse", "call-1");
        assert!(dump(&mut app).contains("source: SELF-REPORT status=working"));
        assert!(!dump(&mut app).contains("not applied:"));
    });
}

#[test]
fn typing_or_pasting_does_not_retire_an_unanswered_codex_permission() {
    use crate::app::effect::{PaneInput, clear_blocked_for_input};
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = agent_app(tmp.path(), "codex");
        for input in [
            PaneInput::Key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::empty())),
            PaneInput::Bytes(b"answer\r".to_vec()),
        ] {
            report(&mut app, "blocked");
            let info = app
                .runtime
                .project_mut()
                .pane_tabs
                .as_mut()
                .unwrap()
                .active_info_mut();
            clear_blocked_for_input(info, &input, app.state.codex_recovery.get_mut(&info.id));
            assert!(dump(&mut app).contains("source: SELF-REPORT status=blocked"));
            question_report(&mut app, "codex-question-start", "PreToolUse", "call-1");
            question_report(&mut app, "codex-question-end", "PostToolUse", "call-1");
            assert!(dump(&mut app).contains("source: SELF-REPORT status=working"));
            assert!(!dump(&mut app).contains("not applied:"));
        }
    });
}

#[test]
fn invalid_question_start_cannot_supersede_a_generic_block() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = agent_app(tmp.path(), "codex");
        for event in [
            None,
            crate::agent::status_hook::StatusHookEvent::from_value(
                &serde_json::json!({"hook_event_name":"PreToolUse", "tool_name":"request_user_input", "turn_id":"turn-1"}),
            ),
        ] {
            question_report(&mut app, "codex-question-start", "PreToolUse", "call-1");
            report(&mut app, "blocked");
            assert!(matches!(
                app.execute_mcp_command(McpCommand::ReportStatus {
                    pane_id: None,
                    pane: None,
                    status: "codex-question-start".into(),
                    ttl_ms: None,
                    note: None,
                    session_id: Some("session-1".into()),
                    hook_event: event,
                }),
                McpResponse::Ok { .. }
            ));
            question_report(&mut app, "codex-question-end", "PostToolUse", "call-1");
            assert!(dump(&mut app).contains("source: SELF-REPORT status=blocked"));
            assert!(dump(&mut app).contains("not applied:"));
        }
    });
}

#[test]
fn accepted_interrupt_retires_working_reports_without_touching_the_other_tab() {
    use crate::app::effect::{PaneInput, PaneTarget};
    for name in ["claude", "codex"] {
        for key in [
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        ] {
            let tmp = tempfile::tempdir().unwrap();
            crate::state::with_state_root(tmp.path(), || {
                let mut app = agent_app(tmp.path(), name);
                report(&mut app, "working");
                assert!(app.open_pane_tab_in("cat", tmp.path()));
                app.runtime
                    .project_mut()
                    .pane_tabs
                    .as_mut()
                    .unwrap()
                    .active_info_mut()
                    .command = name.into();
                report(&mut app, "blocked");
                app.settle_agent_activity(std::time::Instant::now(), &mut RunCtx::for_test());
                app.runtime
                    .project_mut()
                    .pane_tabs
                    .as_mut()
                    .unwrap()
                    .switch_to(0);
                app.execute_pane_input(PaneTarget::Active, PaneInput::Key(key), None, None);
                let tabs = app.runtime.project().pane_tabs.as_ref().unwrap();
                assert!(
                    tabs.tabs()[0].info.reported.is_none(),
                    "{name} interrupt left a working report"
                );
                assert_eq!(
                    tabs.tabs()[1].info.reported.clone().unwrap().status,
                    AgentActivity::Blocked
                );
                let (_, effects) = app.settle_agent_activity(
                    std::time::Instant::now() + Duration::from_secs(121),
                    &mut RunCtx::for_test(),
                );
                assert_eq!(
                    app.runtime.project().pane_tabs.as_ref().unwrap().tabs()[0]
                        .info
                        .activity,
                    AgentActivity::Idle
                );
                assert!(
                    !effects.iter().any(|fx| matches!(fx, Effect::Notify { .. })),
                    "cancelled work is not a done notification"
                );
            });
        }
    }
}

#[test]
fn ordinary_input_keeps_a_working_report() {
    use crate::app::effect::{PaneInput, PaneTarget};
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = agent_app(tmp.path(), "claude");
        for input in [
            PaneInput::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            PaneInput::Bytes(b"\x1b[A".to_vec()),
            PaneInput::Paste {
                bytes: b"\x1b\x03".to_vec(),
                text: "paste".into(),
            },
        ] {
            report(&mut app, "working");
            app.execute_pane_input(PaneTarget::Active, input, None, None);
            assert_eq!(
                app.runtime
                    .project()
                    .pane_tabs
                    .as_ref()
                    .unwrap()
                    .active_info()
                    .reported
                    .clone()
                    .unwrap()
                    .status,
                AgentActivity::Working
            );
        }
    });
}

#[test]
fn report_ttl_is_bounded_at_dispatch_even_for_the_largest_u64() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = agent_app(tmp.path(), "claude");
        for ttl in [300_001, u64::MAX] {
            let result = app.execute_mcp_command(McpCommand::ReportStatus {
                pane_id: None,
                pane: None,
                status: "working".into(),
                ttl_ms: Some(ttl),
                note: None,
                session_id: None,
                hook_event: None,
            });
            assert!(matches!(result, McpResponse::Ok { .. }), "{result:?}");
            let reported = app
                .runtime
                .project()
                .pane_tabs
                .as_ref()
                .unwrap()
                .active_info()
                .reported
                .clone()
                .unwrap();
            assert!(reported.expiry.duration_since(reported.at) <= Duration::from_secs(300));
        }
    });
}

#[test]
fn a_closed_agent_pane_retires_reports_and_rejects_late_reports() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = agent_app(tmp.path(), "claude");
        report(&mut app, "blocked");
        let pid = app
            .runtime
            .project()
            .pane_tabs
            .as_ref()
            .unwrap()
            .active()
            .process_id()
            .unwrap();
        rustix::process::kill_process(
            rustix::process::Pid::from_raw(pid as i32).unwrap(),
            rustix::process::Signal::KILL,
        )
        .unwrap();
        let until = std::time::Instant::now() + Duration::from_secs(10);
        while !app
            .runtime
            .project()
            .pane_tabs
            .as_ref()
            .unwrap()
            .active()
            .is_closed()
        {
            app.drain_pane_output();
            assert!(std::time::Instant::now() < until, "child failed to exit");
            std::thread::sleep(Duration::from_millis(10));
        }
        app.settle_agent_activity(std::time::Instant::now(), &mut RunCtx::for_test());
        let info = app
            .runtime
            .project()
            .pane_tabs
            .as_ref()
            .unwrap()
            .active_info();
        assert!(info.reported.is_none());
        assert_eq!(info.activity, AgentActivity::Idle);
        assert!(dump(&mut app).contains("source: process-exit"));
        assert!(why_status(&mut app).contains("(process-exit)"));
        let result = app.execute_mcp_command(McpCommand::ReportStatus {
            pane_id: None,
            pane: None,
            status: "working".into(),
            ttl_ms: None,
            note: None,
            session_id: None,
            hook_event: None,
        });
        assert!(
            matches!(result, McpResponse::Error { .. }),
            "a late child report resurrected an exited pane: {result:?}"
        );
    });
}

/// The wiring env a test pins: a throwaway home, so nothing reaches the
/// developer's own agent configuration.
fn wiring_env(root: &std::path::Path) -> crate::wiring::Env {
    crate::wiring::Env {
        home: root.join("home"),
        claude_config_dir: None,
        codex_home: root.join("home/.codex"),
        codex_mcp: true,
        managed_mcp: Vec::new(),
        managed_settings: Vec::new(),
        claude_cli: None,
        spyc: "spyc".to_string(),
    }
}

/// `:hooks on codex`, and the outcome its worker landed.
fn wire_codex(app: &mut App) {
    app.dispatch_command("hooks on codex");
    assert!(app.apply_wiring_outcomes(), "the install landed");
}

#[test]
fn codex_dump_distinguishes_definitions_from_reports_and_retains_last_report() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        crate::wiring::with_env(wiring_env(tmp.path()), || {
            let mut app = agent_app(tmp.path(), "codex");
            wire_codex(&mut app);
            let before = dump(&mut app);
            assert!(before.contains("restart needed"), "{before}");
            assert!(before.contains("execution/trust unverified"), "{before}");
            assert!(before.contains("last_report: none received"), "{before}");
            assert!(before.contains("/hooks"), "{before}");

            report(&mut app, "working");
            let at = app
                .runtime
                .project()
                .pane_tabs
                .as_ref()
                .unwrap()
                .active_info()
                .reported
                .clone()
                .unwrap()
                .at;
            let mut ctx = RunCtx::for_test();
            app.settle_agent_activity(at + Duration::from_secs(121), &mut ctx);
            let after = dump(&mut app);
            assert!(after.contains("last_report: status=working"), "{after}");
            assert!(after.contains("no longer authoritative"), "{after}");
            assert!(
                after.contains("execution/trust unverified"),
                "a self-report does not establish hook trust: {after}"
            );
            assert!(why_status(&mut app).contains("/hooks"));
        });
    });
}

#[test]
fn codex_hooks_installed_before_spawn_still_require_review_not_assumed_readiness() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = std::fs::canonicalize(tmp.path()).unwrap();
    crate::state::with_state_root(&dir.join("state"), || {
        crate::wiring::with_env(wiring_env(&dir), || {
            let command = fake_agent(&dir, "codex").display().to_string();
            let mut app = App::test_app(dir.clone());
            app.view.mcp_running = true;
            wire_codex(&mut app);
            assert!(app.open_pane_tab_in(&command, &dir));
            let lines = dump(&mut app);
            assert!(lines.contains("\n      present at launch\n"), "{lines}");
            assert!(!lines.contains("not present at launch"), "{lines}");
            assert!(!lines.contains("restart needed"), "{lines}");
            assert!(lines.contains("execution/trust unverified"), "{lines}");
            assert!(lines.contains("last_report: none received"), "{lines}");

            wire_codex(&mut app);
            assert!(
                !dump(&mut app).contains("restart needed"),
                "an install that changes nothing is not a config change"
            );
            let hooks = dir.join("home/.codex/hooks.json");
            let content = std::fs::read_to_string(&hooks).unwrap();
            std::fs::write(
                &hooks,
                content.replace("--report-status working", "--report-status idle"),
            )
            .unwrap();
            wire_codex(&mut app);
            let changed = dump(&mut app);
            assert!(
                changed.contains("restart needed"),
                "changed startup-only hooks require reload: {changed}"
            );
        });
    });
}

#[test]
fn first_codex_consent_explains_restart_and_review_without_authorizing_hooks() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = std::fs::canonicalize(tmp.path()).unwrap();
    crate::state::with_state_root(&dir.join("state"), || {
        crate::wiring::with_env(wiring_env(&dir), || {
            let command = fake_agent(&dir, "codex").display().to_string();
            let mut app = App::test_app(dir.clone());
            app.view.mcp_running = true;
            assert!(app.open_pane_tab_in(&command, &dir));
            let Mode::Prompting(prompt) = &app.state.mode else {
                panic!("first launch should ask for consent")
            };
            assert!(prompt.prefix.contains("/hooks"));
            app.handle_key(key('y')).unwrap();
            assert!(app.apply_wiring_outcomes());
            let flash = app.flash_text().unwrap();
            assert!(
                flash.contains("restart codex") && flash.contains("/hooks"),
                "{flash}"
            );
            let text = dump(&mut app);
            assert!(text.contains("restart needed"), "{text}");
            assert!(text.contains("last_report: none received"), "{text}");
            let value: toml::Value = toml::from_str(
                &std::fs::read_to_string(dir.join("home/.codex/config.toml")).unwrap(),
            )
            .unwrap();
            assert!(
                value.get("hooks").is_none(),
                "spyc must not author hook-trust state"
            );
            assert!(!dir.join(".codex").exists(), "nothing in the project");
        });
    });
}
