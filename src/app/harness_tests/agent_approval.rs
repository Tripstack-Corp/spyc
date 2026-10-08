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
    let run = crate::guard_support::production_half(include_str!("../pre_recv.rs"));
    assert!(run.contains("self.settle_scrape_quiet(now_pre, ctx)"));
}

#[test]
fn codex_file_edit_approval_requires_a_complete_current_dialogue() {
    let rules = crate::agent::profile_for(AgentKind::Codex).detection_rules();
    let text = include_str!("../../../tests/fixtures/codex-approval-edit.txt");
    let scan = |text: &str| {
        crate::agent::detect_rules::scan(&text.lines().map(String::from).collect::<Vec<_>>(), rules)
    };
    let expected = Some((AgentActivity::Blocked, Some("awaiting file-edit approval")));
    assert_eq!(scan(text), expected);
    for phrase in [
        "Would you like to make the following edits?",
        "Yes, proceed",
        "Yes, and don't ask again for these files",
        "No, and tell Codex what to do differently",
        "Press enter to confirm or esc to cancel",
    ] {
        assert_eq!(scan(&text.replace(phrase, "")), None, "missing {phrase}");
    }
    assert_eq!(
        scan(&format!(
            "{text}\n› Ask Codex to do anything\nGPT | Vim: Insert"
        )),
        None,
        "an old dialogue above the composer is not a current wait"
    );
    assert_eq!(
        scan(&text.replace("› 1.", "  1.").replace("  3.", "› 3.")),
        expected,
        "choosing decline still needs the user until submitted"
    );
    let details = (0..20)
        .map(|index| format!("  edit detail line {index}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        scan(&text.replace("  Description: Apply proposed file edits", &details)),
        expected,
        "a complete long dialogue still needs the user"
    );
}

#[test]
fn codex_mcp_and_network_waits_use_recorded_native_dialogues() {
    let rules = crate::agent::profile_for(AgentKind::Codex).detection_rules();
    for (text, reason, phrases, footer) in [
        (
            include_str!("../../../tests/fixtures/codex-approval-mcp.txt"),
            "awaiting MCP tool approval",
            &[
                "Field 1/1",
                "Run the tool and continue",
                "Cancel this tool call",
            ][..],
            "enter to submit | esc to cancel",
        ),
        (
            include_str!("../../../tests/fixtures/codex-approval-mcp-narrow.txt"),
            "awaiting MCP tool approval",
            &[
                "Field 1/1",
                "Run the tool and continue",
                "Cancel this tool call",
            ][..],
            "enter to submit | esc to cancel",
        ),
        (
            include_str!("../../../tests/fixtures/codex-approval-mcp-session.txt"),
            "awaiting MCP tool approval",
            &[
                "Field 1/1",
                "Run the tool and continue",
                "Cancel this tool call",
            ][..],
            "enter to submit | esc to cancel",
        ),
        (
            include_str!("../../../tests/fixtures/codex-approval-network.txt"),
            "awaiting network approval",
            &[
                "Do you want to approve network access to",
                "Yes, just this once",
                "No, and tell Codex what to do differently",
            ][..],
            "Press enter to confirm or esc to cancel",
        ),
    ] {
        let scan = |text: &str| {
            crate::agent::detect_rules::scan(
                &text.lines().map(String::from).collect::<Vec<_>>(),
                rules,
            )
        };
        let expected = Some((AgentActivity::Blocked, Some(reason)));
        assert_eq!(scan(text), expected, "{text}");
        for phrase in phrases.iter().copied().chain(std::iter::once(footer)) {
            assert_eq!(scan(&text.replace(phrase, "")), None, "missing {phrase}");
        }
        assert_eq!(
            scan(&format!(
                "{text}\n› Ask Codex to do anything\nGPT | Vim: Insert"
            )),
            None,
            "quoted modal above the composer"
        );
    }
}

#[test]
fn codex_approval_scan_has_a_deadline_despite_continuous_redraws() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(tmp.path().to_path_buf());
        assert!(app.open_pane_tab_in("sh -c 'stty raw -echo; printf READY; exec cat'", tmp.path()));
        let wait_screen = |app: &mut App, expected: &str| {
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                let pane = &mut app.runtime.pane_tabs.as_mut().unwrap().tabs_mut()[0].pane;
                pane.drain_output();
                let visible = pane.visible_lines().join("\n");
                if visible.contains(expected) {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "screen did not contain {expected}: {visible}"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
        };
        wait_screen(&mut app, "READY");
        let fixture = include_str!("../../../tests/fixtures/codex-approval-exec.txt");
        {
            let entry = &mut app.runtime.pane_tabs.as_mut().unwrap().tabs_mut()[0];
            entry.info.command = "codex".into();
            entry.pane.resize(30, 100).unwrap();
            entry
                .pane
                .send_bytes(format!("\x1b[2J\x1b[H{}", fixture.replace('\n', "\r\n")).as_bytes())
                .unwrap();
        }
        wait_screen(&mut app, "Press enter to confirm or esc to cancel");
        report(&mut app, "working", None);
        let base = Instant::now();
        let mut ctx = RunCtx::for_test();
        for elapsed in [0, 100, 200, 300] {
            let now = base + Duration::from_millis(elapsed);
            {
                let info = app.runtime.pane_tabs.as_mut().unwrap().active_info_mut();
                info.last_output_at = Some(now);
                info.scrape_dirty = true;
            }
            app.settle_scrape_quiet(now, &mut ctx);
            app.settle_agent_activity(now, &mut ctx);
        }
        let info = app.runtime.pane_tabs.as_ref().unwrap().active_info();
        assert_eq!(
            info.activity,
            AgentActivity::Blocked,
            "a repaint cannot postpone every approval scan"
        );
        assert_eq!(
            info.reported.unwrap().status,
            AgentActivity::Working,
            "retain working for answer recovery"
        );
        assert!(!info.scrape_dirty, "the deadline consumes the pending scan");
        assert!(
            dump(&mut app)
                .contains("source: SCRAPE-FALLBACK status=blocked (awaiting command approval)")
        );
    });
}

#[test]
fn codex_command_approval_handles_recorded_native_wrapping() {
    let rules = crate::agent::profile_for(AgentKind::Codex).detection_rules();
    let text = include_str!("../../../tests/fixtures/codex-approval-exec-narrow.txt");
    let scan = |text: &str| {
        crate::agent::detect_rules::scan(&text.lines().map(String::from).collect::<Vec<_>>(), rules)
    };
    let expected = Some((AgentActivity::Blocked, Some("awaiting command approval")));
    assert_eq!(scan(text), expected);
    for required in [
        "Would you like",
        "Yes, proceed",
        "No, and tell",
        "Press enter",
        "  cancel",
    ] {
        assert_eq!(
            scan(&text.replace(required, "")),
            None,
            "missing {required}"
        );
    }
    assert_eq!(scan(&format!("{text}\n› Ask Codex to do anything")), None);
    assert_eq!(
        scan(&text.replace("Press enter", "Quoted footer: Press enter")),
        None
    );
}
