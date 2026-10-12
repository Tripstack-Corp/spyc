//! Shell tabs and the agents launched inside them (#544): the env a shell's
//! agents inherit, the config a consented project gets up front, and what a
//! report to a shell tab does before the tab knows which agent runs.

use super::*;
use crate::mcp_cmd::{McpCommand, McpResponse};

/// The id of the only tab.
fn tab_id(app: &App) -> String {
    app.runtime.pane_tabs.as_ref().unwrap().tabs()[0]
        .info
        .id
        .clone()
}

/// An agent typed into a shell inherits the shell's env, so the shell tab has
/// to carry the socket and its own id for that agent to reach spyc and name
/// the tab.
#[test]
fn a_shell_tab_carries_the_agent_env() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let got = dir.join("env.txt");
        let cmd = format!(
            "sh -c 'printf \"%s|%s\" \"$SPYC_PANE_ID\" \"$SPYC_MCP_SOCK\" > {}; exec sleep 30'",
            got.display()
        );
        let mut app = App::test_app(dir.clone());
        assert!(app.open_pane_tab_in(&cmd, &dir));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while !got.exists() {
            assert!(std::time::Instant::now() < deadline, "the shell never ran");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let sock = crate::mcp::socket_path().unwrap();
        assert_eq!(
            std::fs::read_to_string(&got).unwrap(),
            format!("{}|{}", tab_id(&app), sock.display())
        );
    });
}

/// Every supported agent reads its MCP entry, and codex and agy their hooks,
/// at startup, before spyc can see which one a shell started. In a project
/// that consented to status hooks, a shell tab writes them all up front, and
/// exit cleanup removes them as it does an agent tab's.
#[test]
fn a_shell_tab_in_a_consented_project_provisions_every_agent() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap().join("project");
        std::fs::create_dir_all(&dir).unwrap();
        crate::state::hook_consent::set_consent(&dir, true);
        let mut app = App::test_app(dir.clone());
        app.view.mcp_running = true;
        assert!(app.open_pane_tab_in("sh", &dir));

        let mut written = vec![
            ".agents/mcp_config.json",
            ".codex/config.toml",
            ".claude/settings.json",
            ".agents/hooks.json",
        ];
        // Claude Code's enterprise policy can own or forbid the spyc entry, and
        // then no `.mcp.json` is written for any tab. Ask this machine.
        let probe = tmp.path().join("probe");
        std::fs::create_dir_all(&probe).unwrap();
        if matches!(
            crate::mcp::ensure_mcp_json(&probe),
            Ok(crate::mcp::McpConfigStatus::Configured)
        ) {
            written.push(".mcp.json");
        }
        for file in written {
            assert!(dir.join(file).exists(), "{file} was not provisioned");
        }
        let codex = std::fs::read_to_string(dir.join(".codex/config.toml")).unwrap();
        assert!(codex.contains("--report-status"), "codex hooks: {codex}");
        assert!(
            app.flash_text().is_some_and(|f| f.starts_with("pane: sh")),
            "provisioning is quiet: {:?}",
            app.flash_text()
        );

        app.cleanup_written_mcp_configs();
        for file in [".mcp.json", ".agents/mcp_config.json", ".agents/hooks.json"] {
            assert!(!dir.join(file).exists(), "{file} survived cleanup");
        }
        let codex = std::fs::read_to_string(dir.join(".codex/config.toml")).unwrap_or_default();
        assert!(
            !codex.contains("--report-status"),
            "codex hooks survived cleanup"
        );
    });
}

/// Without consent, or with it declined, a shell tab writes nothing.
#[test]
fn a_shell_tab_without_consent_writes_nothing() {
    for consent in [None, Some(false)] {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(&tmp.path().join("state"), || {
            let dir = std::fs::canonicalize(tmp.path()).unwrap().join("project");
            std::fs::create_dir_all(&dir).unwrap();
            if let Some(answer) = consent {
                crate::state::hook_consent::set_consent(&dir, answer);
            }
            let mut app = App::test_app(dir.clone());
            app.view.mcp_running = true;
            assert!(app.open_pane_tab_in("sh", &dir));
            let entries: Vec<_> = std::fs::read_dir(&dir)
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect();
            assert!(entries.is_empty(), "consent {consent:?} wrote {entries:?}");
        });
    }
}

/// A report reaching a shell tab is kept for the diagnostics but moves no dot:
/// until the tab knows which agent runs, it is evidence, not a status.
#[test]
fn a_report_to_a_shell_tab_is_recorded_not_applied() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let mut app = App::test_app(dir.clone());
        assert!(app.open_pane_tab_in("sh", &dir));
        let response = app.execute_mcp_command(McpCommand::ReportStatus {
            pane_id: Some(tab_id(&app)),
            pane: None,
            status: "working".into(),
            ttl_ms: None,
            session_id: Some("11111111-1111-4111-8111-111111111111".into()),
            hook_event: None,
        });
        match response {
            McpResponse::Ok { message } => assert!(
                message.contains("not applied: no agent is running in this tab"),
                "{message}"
            ),
            McpResponse::Error { message } => panic!("report refused: {message}"),
        }
        let info = app.runtime.pane_tabs.as_ref().unwrap().active_info();
        assert!(info.reported.is_none());
        assert_eq!(info.activity, crate::pane::AgentActivity::Unknown);
        assert!(info.last_reported.is_some(), "kept for :why-status");
        assert!(info.live_session_id.is_none());
    });
}
