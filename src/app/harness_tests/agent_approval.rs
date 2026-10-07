use super::agent_readiness::{agent_app, dump};
use super::*;
use crate::mcp_cmd::{McpCommand, McpResponse};
use crate::pane::AgentActivity;
use std::time::{Duration, Instant};

fn report(app: &mut App, status: &str, event: Option<serde_json::Value>) {
    let result = app.execute_mcp_command(McpCommand::ReportStatus {
        pane_id: None,
        pane: None,
        status: status.into(),
        ttl_ms: Some(120_000),
        session_id: Some("session-1".into()),
        hook_event: event
            .as_ref()
            .and_then(crate::agent::status_hook::StatusHookEvent::from_value),
    });
    assert!(matches!(result, McpResponse::Ok { .. }), "{result:?}");
}

fn permission() -> serde_json::Value {
    serde_json::json!({"hook_event_name":"PermissionRequest", "tool_name":"Bash", "turn_id":"turn-1"})
}

#[test]
fn automatic_codex_permission_review_is_observed_without_latching_blocked() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = agent_app(tmp.path(), "codex");
        report(&mut app, "working", None);
        let original = app
            .runtime
            .pane_tabs
            .as_ref()
            .unwrap()
            .active_info()
            .reported
            .unwrap();
        report(&mut app, "blocked", Some(permission()));
        let now = original.at + Duration::from_secs(30);
        app.runtime
            .pane_tabs
            .as_mut()
            .unwrap()
            .active_info_mut()
            .last_output_at = Some(original.at + Duration::from_secs(1));
        app.settle_agent_activity(now, &mut RunCtx::for_test());
        let info = app.runtime.pane_tabs.as_ref().unwrap().active_info();
        assert_eq!(info.activity, AgentActivity::Working);
        assert_eq!(info.reported.unwrap().at, original.at);
        let explanation = dump(&mut app);
        assert!(
            explanation.contains("source: SELF-REPORT status=working"),
            "{explanation}"
        );
        assert!(
            explanation.contains("not applied: PermissionRequest precedes review"),
            "{explanation}"
        );
        assert!(explanation.contains("event=PermissionRequest status=blocked tool=Bash"));
    });
}

#[test]
fn permission_observation_does_not_invent_working_or_change_claude_permissions() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        for (command, expected) in [
            ("codex", AgentActivity::Idle),
            ("claude", AgentActivity::Blocked),
        ] {
            let mut app = agent_app(tmp.path(), command);
            report(&mut app, "blocked", Some(permission()));
            app.settle_agent_activity(Instant::now(), &mut RunCtx::for_test());
            assert_eq!(
                app.runtime
                    .pane_tabs
                    .as_ref()
                    .unwrap()
                    .active_info()
                    .activity,
                expected
            );
        }
    });
}

#[test]
fn codex_permission_observation_preserves_question_correlation() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = agent_app(tmp.path(), "codex");
        let event = |name, call| {
            Some(
                serde_json::json!({"hook_event_name":name, "tool_name":"request_user_input", "turn_id":"turn-1", "tool_use_id":call}),
            )
        };
        report(
            &mut app,
            "codex-question-start",
            event("PreToolUse", "call-1"),
        );
        report(&mut app, "blocked", Some(permission()));
        report(
            &mut app,
            "codex-question-end",
            event("PostToolUse", "other-call"),
        );
        assert!(dump(&mut app).contains("source: SELF-REPORT status=blocked"));
        report(
            &mut app,
            "codex-question-end",
            event("PostToolUse", "call-1"),
        );
        assert!(dump(&mut app).contains("source: SELF-REPORT status=working"));
        // An explicit agent block still needs its own answer or newer report.
        report(&mut app, "blocked", None);
        report(
            &mut app,
            "codex-question-start",
            event("PreToolUse", "call-2"),
        );
        report(
            &mut app,
            "codex-question-end",
            event("PostToolUse", "call-2"),
        );
        assert!(dump(&mut app).contains("source: SELF-REPORT status=blocked"));
    });
}

#[test]
fn visible_codex_approval_temporarily_overrides_working_with_honest_diagnostics() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = agent_app(tmp.path(), "codex");
        report(&mut app, "working", None);
        let at = app
            .runtime
            .pane_tabs
            .as_ref()
            .unwrap()
            .active_info()
            .reported
            .unwrap()
            .at;
        {
            let info = app.runtime.pane_tabs.as_mut().unwrap().active_info_mut();
            info.last_output_at = Some(at + Duration::from_secs(1));
            info.scrape_status = Some((AgentActivity::Blocked, Some("awaiting command approval")));
        }
        app.settle_agent_activity(at + Duration::from_secs(30), &mut RunCtx::for_test());
        assert_eq!(
            app.runtime
                .pane_tabs
                .as_ref()
                .unwrap()
                .active_info()
                .activity,
            AgentActivity::Blocked
        );
        assert!(
            dump(&mut app)
                .contains("source: SCRAPE-FALLBACK status=blocked (awaiting command approval)")
        );
        app.dispatch_command("why-status");
        assert!(
            app.flash_text()
                .unwrap()
                .contains("blocked (scrape-fallback:")
        );
        let info = app.runtime.pane_tabs.as_mut().unwrap().active_info_mut();
        assert_eq!(
            info.reported.unwrap().at,
            at,
            "dialogue must not discard the silent-work report"
        );
        info.scrape_status = None;
        app.settle_agent_activity(at + Duration::from_secs(60), &mut RunCtx::for_test());
        assert_eq!(
            app.runtime
                .pane_tabs
                .as_ref()
                .unwrap()
                .active_info()
                .activity,
            AgentActivity::Working
        );
        assert!(dump(&mut app).contains("source: SELF-REPORT status=working"));
    });
}

#[test]
fn codex_approval_detector_requires_a_complete_dialogue_at_the_viewport_bottom() {
    let rules = crate::agent::profile_for(AgentKind::Codex).detection_rules();
    let text = include_str!("../../../tests/fixtures/codex-approval-exec.txt");
    let lines = |s: &str| s.lines().map(String::from).collect::<Vec<_>>();
    let positive = crate::agent::detect_rules::scan(&lines(text), rules);
    assert_eq!(
        positive,
        Some((AgentActivity::Blocked, Some("awaiting command approval")))
    );
    let detail = (0..20)
        .map(|index| format!("  command detail line {index}"))
        .collect::<Vec<_>>()
        .join("\n");
    let expanded = text.replace("  $ sleep 30", &detail);
    assert_eq!(
        crate::agent::detect_rules::scan(&lines(&expanded), rules),
        positive,
        "a complete visible dialogue with long command details still needs the user"
    );
    for rejected in [
        text.replace("Yes, proceed", ""),
        text.replace("Would you like to run the following command?", ""),
        text.replace("Press enter to confirm or esc to cancel", ""),
        format!("{text}\n› Ask Codex to do anything\nGPT | Vim: Insert"),
        "The approval says: Would you like to run the following command?".into(),
    ] {
        assert_eq!(
            crate::agent::detect_rules::scan(&lines(&rejected), rules),
            None,
            "{rejected}"
        );
    }
    assert_eq!(
        crate::agent::detect_rules::scan(
            &lines(&text.replace("› 1.", "  1.").replace("  3.", "› 3.")),
            rules
        ),
        positive
    );
}

#[test]
fn approval_scan_is_wired_to_the_current_viewport_and_loop() {
    let status = crate::guard_support::production_half(include_str!("../agent_status.rs"));
    let scan = status
        .split("pub(crate) fn settle_scrape_quiet(")
        .nth(1)
        .unwrap()
        .split("pub(crate) fn settle_agent_activity(")
        .next()
        .unwrap();
    assert!(scan.contains("let lines = entry.pane.visible_lines();"));
    assert!(scan.contains("crate::agent::detect_rules::scan(&lines, rules)"));
    assert!(
        !scan.contains("recent_lines("),
        "old approval history cannot establish a current wait"
    );
    let run = crate::guard_support::production_half(include_str!("../run.rs"));
    assert!(run.contains("self.settle_scrape_quiet(now_pre, &mut ctx)"));
}
