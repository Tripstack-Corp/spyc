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

pub(super) fn agent_app(dir: &std::path::Path, name: &str) -> App {
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

pub(super) fn dump(app: &mut App) -> String {
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

#[test]
fn codex_legacy_only_hook_migration_requires_restart_and_clears_duplicate_diagnostics() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = agent_app(tmp.path(), "codex");
        app.install_status_hooks(tmp.path(), crate::state::sessions::AgentKind::Codex);
        let canonical = tmp.path().join(".codex/config.toml");
        let before = std::fs::read(&canonical).unwrap();
        app.runtime
            .pane_tabs
            .as_mut()
            .unwrap()
            .active_info_mut()
            .status_hooks_restart_needed = false;
        std::fs::write(
            tmp.path().join(".codex/hooks.json"),
            r#"{"hooks":{"Stop":[{"hooks":[{"command":"spyc --report-status done"}]}]}}"#,
        )
        .unwrap();
        assert!(dump(&mut app).contains("additional spyc reporters"));
        app.install_status_hooks(tmp.path(), crate::state::sessions::AgentKind::Codex);
        let after = dump(&mut app);
        assert!(
            after.contains("restart needed"),
            "a JSON-only change requires restart: {after}"
        );
        assert!(!after.contains("additional spyc reporters"), "{after}");
        assert_eq!(
            std::fs::read(canonical).unwrap(),
            before,
            "canonical hooks were already current"
        );
        assert!(
            after.contains("execution/trust unverified"),
            "migration cannot establish trust: {after}"
        );
    });
}

#[test]
fn codex_refused_migration_retains_shared_ownership_of_existing_reporters() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        assert!(crate::mcp::ensure_codex_status_hooks(tmp.path()));
        std::fs::write(tmp.path().join(".codex/hooks.json"), "{broken").unwrap();
        let mut app = agent_app(tmp.path(), "codex");
        app.install_status_hooks(tmp.path(), crate::state::sessions::AgentKind::Codex);
        assert!(dump(&mut app).contains("legacy .codex/hooks.json is malformed"));
        assert!(
            !crate::state::dir_owners::release(
                crate::state::dir_owners::Shared::StatusHooks,
                tmp.path(),
                1
            ),
            "a sibling must not be allowed to remove this pane's existing reporters"
        );
        assert!(tmp.path().join(".codex/config.toml").exists());
    });
}

#[test]
fn codex_refused_migration_retains_shared_ownership_of_legacy_only_reporters() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        std::fs::create_dir_all(tmp.path().join(".codex")).unwrap();
        std::fs::write(tmp.path().join(".codex/config.toml"), "{broken").unwrap();
        let legacy = tmp.path().join(".codex/hooks.json");
        std::fs::write(
            &legacy,
            r#"{"hooks":{"Stop":[{"hooks":[{"command":"spyc --report-status done"}]}]}}"#,
        )
        .unwrap();
        let mut app = agent_app(tmp.path(), "codex");
        app.install_status_hooks(tmp.path(), crate::state::sessions::AgentKind::Codex);
        assert!(
            !crate::state::dir_owners::release(
                crate::state::dir_owners::Shared::StatusHooks,
                tmp.path(),
                1
            ),
            "a refused migration must retain ownership of legacy-only reporters"
        );
        assert!(legacy.exists());
    });
}

fn question_report(app: &mut App, signal: &str, event: &str, call: &str) {
    let result = app.execute_mcp_command(McpCommand::ReportStatus {
        pane_id: None, pane: None, status: signal.into(), ttl_ms: Some(120_000),
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
            .pane_tabs
            .as_ref()
            .unwrap()
            .active_info()
            .id
            .clone();
        let recovery = app.state.codex_recovery.get_mut(&id);
        crate::app::effect::clear_blocked_for_input(
            app.runtime.pane_tabs.as_mut().unwrap().active_info_mut(),
            &input,
            recovery,
        );
        assert!(dump(&mut app).contains("source: SELF-REPORT status=blocked"));
        question_report(&mut app, "codex-question-end", "PostToolUse", "call-1");
        assert!(dump(&mut app).contains("source: SELF-REPORT status=working"));
    });
}

#[test]
fn codex_question_hook_definitions_are_narrow_and_require_a_metadata_capable_host() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(crate::mcp::ensure_codex_status_hooks(tmp.path()));
    let path = tmp.path().join(".codex/config.toml");
    let value: toml::Value = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    for (event, signal) in [
        ("PreToolUse", "codex-question-start"),
        ("PostToolUse", "codex-question-end"),
    ] {
        assert_eq!(
            value["hooks"][event][0]["matcher"].as_str(),
            Some("^request_user_input$")
        );
        assert!(
            value["hooks"][event][0]["hooks"][0]["command"]
                .as_str()
                .unwrap()
                .contains(signal)
        );
    }
    let before = std::fs::read(&path).unwrap();
    assert!(crate::mcp::ensure_codex_status_hooks(tmp.path()));
    assert_eq!(std::fs::read(&path).unwrap(), before);
    crate::mcp::cleanup_codex_status_hooks(tmp.path());
    assert!(!path.exists());
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
                session_id: Some("session-1".into()),
                hook_event: None,
            }),
            McpResponse::Ok { .. }
        ));
        assert!(
            app.runtime
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
            .pane_tabs
            .as_mut()
            .unwrap()
            .active_info_mut()
            .command = "codex".into();
        question_report(&mut app, "codex-question-end", "PostToolUse", "call-1");
        let tabs = app.runtime.pane_tabs.as_ref().unwrap();
        assert_eq!(
            tabs.tabs()[0].info.reported.unwrap().status,
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
            .pane_tabs
            .as_ref()
            .unwrap()
            .active_info()
            .id
            .clone();
        let recovery = app.state.codex_recovery.get_mut(&id);
        crate::app::effect::clear_blocked_for_input(
            app.runtime.pane_tabs.as_mut().unwrap().active_info_mut(),
            &crate::app::effect::PaneInput::Key(KeyEvent::new(
                KeyCode::Enter,
                KeyModifiers::empty(),
            )),
            recovery,
        );
        assert!(
            app.runtime
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
            let info = app.runtime.pane_tabs.as_mut().unwrap().active_info_mut();
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
                    .pane_tabs
                    .as_mut()
                    .unwrap()
                    .active_info_mut()
                    .command = name.into();
                report(&mut app, "blocked");
                app.settle_agent_activity(std::time::Instant::now(), &mut RunCtx::for_test());
                app.runtime.pane_tabs.as_mut().unwrap().switch_to(0);
                app.execute_pane_input(PaneTarget::Active, PaneInput::Key(key), None, None);
                let tabs = app.runtime.pane_tabs.as_ref().unwrap();
                assert!(
                    tabs.tabs()[0].info.reported.is_none(),
                    "{name} interrupt left a working report"
                );
                assert_eq!(
                    tabs.tabs()[1].info.reported.unwrap().status,
                    AgentActivity::Blocked
                );
                let (_, effects) = app.settle_agent_activity(
                    std::time::Instant::now() + Duration::from_secs(121),
                    &mut RunCtx::for_test(),
                );
                assert_eq!(
                    app.runtime.pane_tabs.as_ref().unwrap().tabs()[0]
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
                    .pane_tabs
                    .as_ref()
                    .unwrap()
                    .active_info()
                    .reported
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
                session_id: None,
                hook_event: None,
            });
            assert!(matches!(result, McpResponse::Ok { .. }), "{result:?}");
            let reported = app
                .runtime
                .pane_tabs
                .as_ref()
                .unwrap()
                .active_info()
                .reported
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
        while !app.runtime.pane_tabs.as_ref().unwrap().active().is_closed() {
            app.drain_pane_output();
            assert!(std::time::Instant::now() < until, "child failed to exit");
            std::thread::sleep(Duration::from_millis(10));
        }
        app.settle_agent_activity(std::time::Instant::now(), &mut RunCtx::for_test());
        let info = app.runtime.pane_tabs.as_ref().unwrap().active_info();
        assert!(info.reported.is_none());
        assert_eq!(info.activity, AgentActivity::Idle);
        assert!(dump(&mut app).contains("source: process-exit"));
        app.dispatch_command("why-status");
        assert!(app.flash_text().unwrap().contains("(process-exit)"));
        let result = app.execute_mcp_command(McpCommand::ReportStatus {
            pane_id: None,
            pane: None,
            status: "working".into(),
            ttl_ms: None,
            session_id: None,
            hook_event: None,
        });
        assert!(
            matches!(result, McpResponse::Error { .. }),
            "a late child report resurrected an exited pane: {result:?}"
        );
    });
}
