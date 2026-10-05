use super::*;
use crate::mcp_cmd::{McpCommand, McpResponse};
use crate::pane::AgentActivity;
use std::time::Duration;

#[test]
fn codex_interrupt_hook_reports_idle_without_changing_user_hooks_or_trust() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join(".codex/config.toml");
    std::fs::create_dir(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "[[hooks.Interrupt]]\n[[hooks.Interrupt.hooks]]\ntype = 'command'\ncommand = 'user-reporter'\n[hooks.state.user]\ntrusted_hash = 'preserve-me'\n").unwrap();
    assert!(crate::mcp::ensure_codex_status_hooks(tmp.path()));
    let value: toml::Value = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let commands: Vec<_> = value["hooks"]["Interrupt"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|group| group["hooks"].as_array().unwrap())
        .map(|handler| handler["command"].as_str().unwrap())
        .collect();
    assert!(
        commands
            .iter()
            .any(|command| command.contains("--report-status idle")),
        "{commands:?}"
    );
    assert!(commands.contains(&"user-reporter"));
    assert_eq!(
        value["hooks"]["state"]["user"]["trusted_hash"].as_str(),
        Some("preserve-me")
    );
    let once = std::fs::read(&path).unwrap();
    assert!(crate::mcp::ensure_codex_status_hooks(tmp.path()));
    assert_eq!(std::fs::read(&path).unwrap(), once);
    crate::mcp::cleanup_codex_status_hooks(tmp.path());
    let cleaned: toml::Value = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(cleaned["hooks"]["Interrupt"].as_array().unwrap().len(), 1);
    assert_eq!(
        cleaned["hooks"]["state"]["user"]["trusted_hash"].as_str(),
        Some("preserve-me")
    );
}

fn agent_app(dir: &std::path::Path, name: &str) -> App {
    let mut app = App::test_app(dir.to_path_buf());
    assert!(app.open_pane_tab_in("cat", dir));
    app.runtime
        .pane_tabs
        .as_mut()
        .unwrap()
        .active_info_mut()
        .command = name.into();
    app
}

fn report(app: &mut App, status: &str) {
    assert!(matches!(
        app.execute_mcp_command(McpCommand::ReportStatus {
            pane_id: None,
            pane: None,
            status: status.into(),
            ttl_ms: Some(120_000),
            session_id: None,
            hook_event: None,
        }),
        McpResponse::Ok { .. }
    ));
}

fn dump(app: &mut App) -> String {
    app.dispatch_command("activity dump");
    app.view
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

#[test]
fn codex_and_claude_reports_survive_output_and_quiet_tool_waits() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        for name in ["codex", "claude"] {
            let mut app = agent_app(tmp.path(), name);
            report(&mut app, "working");
            let at = app
                .runtime
                .pane_tabs
                .as_ref()
                .unwrap()
                .active_info()
                .reported
                .unwrap()
                .at;
            app.runtime
                .pane_tabs
                .as_mut()
                .unwrap()
                .active_info_mut()
                .last_output_at = Some(at + Duration::from_secs(1));
            let mut ctx = RunCtx::for_test();
            app.settle_agent_activity(at + Duration::from_secs(30), &mut ctx);
            let info = app.runtime.pane_tabs.as_ref().unwrap().active_info();
            assert_eq!(
                info.activity,
                AgentActivity::Working,
                "{name}: output must not erase a working report"
            );
            assert!(info.reported.is_some());

            report(&mut app, "done");
            let at = app
                .runtime
                .pane_tabs
                .as_ref()
                .unwrap()
                .active_info()
                .reported
                .unwrap()
                .at;
            app.runtime
                .pane_tabs
                .as_mut()
                .unwrap()
                .active_info_mut()
                .last_output_at = Some(at + Duration::from_secs(1));
            app.settle_agent_activity(at + Duration::from_secs(30), &mut ctx);
            assert_eq!(
                app.runtime
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

#[test]
fn codex_dump_distinguishes_definitions_from_reports_and_retains_last_report() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = agent_app(tmp.path(), "codex");
        app.install_status_hooks(tmp.path(), crate::state::sessions::AgentKind::Codex);
        let before = dump(&mut app);
        assert!(before.contains("restart needed"), "{before}");
        assert!(before.contains("execution/trust unverified"), "{before}");
        assert!(before.contains("last_report: none received"), "{before}");
        assert!(before.contains("/hooks"), "{before}");

        report(&mut app, "working");
        let at = app
            .runtime
            .pane_tabs
            .as_ref()
            .unwrap()
            .active_info()
            .reported
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
        app.dispatch_command("why-status");
        assert!(app.flash_text().unwrap().contains("/hooks"));
    });
}

#[test]
fn codex_hooks_installed_before_spawn_still_require_review_not_assumed_readiness() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let command = fake_agent(&dir, "codex").display().to_string();
        let mut app = App::test_app(dir.clone());
        app.view.mcp_running = true;
        crate::state::hook_consent::set_consent(&dir, true);
        assert!(app.open_pane_tab_in(&command, &dir));
        let lines = dump(&mut app);
        assert!(lines.contains("; present at launch;"), "{lines}");
        assert!(!lines.contains("not present at launch"), "{lines}");
        assert!(!lines.contains("restart needed"), "{lines}");
        assert!(lines.contains("execution/trust unverified"), "{lines}");
        assert!(lines.contains("last_report: none received"), "{lines}");

        app.install_status_hooks(&dir, crate::state::sessions::AgentKind::Codex);
        assert!(
            !dump(&mut app).contains("restart needed"),
            "idempotent installation is not a config change"
        );
        let config = dir.join(".codex/config.toml");
        let content = std::fs::read_to_string(&config).unwrap();
        std::fs::write(
            &config,
            content.replace("--report-status working", "--report-status idle"),
        )
        .unwrap();
        app.install_status_hooks(&dir, crate::state::sessions::AgentKind::Codex);
        let changed = dump(&mut app);
        assert!(
            changed.contains("restart needed"),
            "changed startup-only hooks require reload: {changed}"
        );
    });
}

#[test]
fn first_codex_consent_explains_restart_and_review_without_authorizing_hooks() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let command = fake_agent(&dir, "codex").display().to_string();
        let mut app = App::test_app(dir.clone());
        app.view.mcp_running = true;
        assert!(app.open_pane_tab_in(&command, &dir));
        let Mode::Prompting(prompt) = &app.state.mode else {
            panic!("first launch should ask for consent")
        };
        assert!(prompt.prefix.contains("/hooks"));
        app.handle_key(key('y')).unwrap();
        assert!(app.flash_text().unwrap().contains("review /hooks"));
        let text = dump(&mut app);
        assert!(text.contains("restart needed"), "{text}");
        assert!(text.contains("last_report: none received"), "{text}");
        let value: toml::Value =
            toml::from_str(&std::fs::read_to_string(dir.join(".codex/config.toml")).unwrap())
                .unwrap();
        assert!(
            value["hooks"].get("state").is_none(),
            "spyc must not author hook-trust state"
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
            .pane_tabs
            .as_ref()
            .unwrap()
            .active_info()
            .id
            .clone();
        assert!(app.open_pane_tab_in("cat", &dir));
        app.runtime
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
            session_id: None,
            hook_event: None,
        });
        let at = app.runtime.pane_tabs.as_ref().unwrap().tabs()[0]
            .info
            .reported
            .unwrap()
            .at;
        app.runtime.pane_tabs.as_mut().unwrap().tabs_mut()[0]
            .info
            .last_output_at = Some(at + Duration::from_secs(2));
        let mut ctx = RunCtx::for_test();
        app.settle_agent_activity(at + Duration::from_secs(60), &mut ctx);
        let tabs = app.runtime.pane_tabs.as_ref().unwrap();
        assert_eq!(tabs.tabs()[0].info.activity, AgentActivity::Blocked);
        assert_eq!(
            tabs.tabs()[0].info.last_reported.unwrap().status,
            AgentActivity::Blocked
        );
        assert!(tabs.tabs()[1].info.last_reported.is_none());
        assert!(dump(&mut app).contains("latched until answered/dismissed"));

        let command = fake_agent(&dir, "codex").display().to_string();
        assert!(app.spawn_agent_into_tab(0, &command, &dir, None));
        assert!(
            app.runtime.pane_tabs.as_ref().unwrap().tabs()[0]
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
                    session_id: None,
                    hook_event: Some(event),
                }),
                McpResponse::Ok { .. }
            ));
        }
        let tabs = app.runtime.pane_tabs.as_ref().unwrap();
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
            app.runtime.pane_tabs.as_ref().unwrap().tabs()[0]
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
                session_id: None,
                hook_event: Some(event),
            }),
            McpResponse::Ok { .. }
        ));
        let at = app
            .runtime
            .pane_tabs
            .as_ref()
            .unwrap()
            .active_info()
            .reported
            .unwrap()
            .at;
        let mut ctx = RunCtx::for_test();
        app.settle_agent_activity(at + Duration::from_secs(60), &mut ctx);
        assert_eq!(
            app.runtime
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
            .pane_tabs
            .as_ref()
            .unwrap()
            .active_info()
            .reported
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
