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

/// Fake agents for a shell to launch: `<dir>/bin/<name>`, links to `sleep`,
/// so each runs as a process whose argv[0] is the agent's name without being
/// a new executable for macOS to scan on first run.
fn fake_agents(dir: &std::path::Path) -> std::path::PathBuf {
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    for name in ["claude", "agy"] {
        std::os::unix::fs::symlink("/bin/sleep", bin.join(name)).unwrap();
    }
    bin
}

/// Type `text` into the tab, then drain output and settle until `done` holds.
fn type_until(app: &mut App, text: &str, what: &str, done: impl Fn(&crate::pane::TabInfo) -> bool) {
    app.runtime.pane_tabs.as_mut().unwrap().tabs_mut()[0]
        .pane
        .send_bytes(text.as_bytes())
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        app.drain_pane_output();
        app.settle_shell_agents(std::time::Instant::now(), &mut RunCtx::for_test());
        if done(&app.runtime.pane_tabs.as_ref().unwrap().tabs()[0].info) {
            return;
        }
        assert!(std::time::Instant::now() < deadline, "never saw: {what}");
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
}

fn kind_of(info: &crate::pane::TabInfo) -> crate::state::sessions::AgentKind {
    info.agent().kind()
}

/// The issue's acceptance path: a shell launches an agent and the tab follows
/// it; the agent exits and the tab is a shell again, with nothing of that
/// agent left; a different agent launched next is a new one.
#[test]
fn a_shell_tab_follows_the_agent_it_runs_and_returns_to_a_shell() {
    use crate::state::sessions::AgentKind;
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let bin = fake_agents(&dir);
        let mut app = App::test_app(dir.clone());
        assert!(app.open_pane_tab_in("sh", &dir));
        type_until(&mut app, "", "the shell", |info| {
            kind_of(info) == AgentKind::Other
        });

        let claude = format!("{} 60\r", bin.join("claude").display());
        type_until(&mut app, &claude, "claude", |info| {
            kind_of(info) == AgentKind::Claude
        });
        let info = &app.runtime.pane_tabs.as_ref().unwrap().tabs()[0].info;
        assert_eq!(
            info.launched_agent().kind(),
            AgentKind::Other,
            "it is still a shell tab"
        );
        let first = info.running.expect("running").pid;

        // Reports apply now that the tab knows what it runs, and the session
        // the hook names is pinned.
        let response = app.execute_mcp_command(McpCommand::ReportStatus {
            pane_id: Some(tab_id(&app)),
            pane: None,
            status: "working".into(),
            ttl_ms: None,
            session_id: Some("11111111-1111-4111-8111-111111111111".into()),
            hook_event: None,
        });
        assert!(
            matches!(response, McpResponse::Ok { ref message } if !message.contains("not applied"))
        );
        let info = &app.runtime.pane_tabs.as_ref().unwrap().tabs()[0].info;
        assert!(info.reported.is_some());
        assert!(info.live_session_id.is_some());

        type_until(&mut app, "\x03", "claude exit", |info| {
            info.running.is_none()
        });
        let info = &app.runtime.pane_tabs.as_ref().unwrap().tabs()[0].info;
        assert_eq!(kind_of(info), AgentKind::Other);
        assert!(
            info.reported.is_none() && info.last_reported.is_none(),
            "stale status"
        );
        assert!(info.live_session_id.is_none(), "stale session binding");

        let agy = format!("{} 60\r", bin.join("agy").display());
        type_until(&mut app, &agy, "agy", |info| {
            kind_of(info) == AgentKind::Agy
        });
        let info = &app.runtime.pane_tabs.as_ref().unwrap().tabs()[0].info;
        assert_ne!(info.running.expect("running").pid, first, "a new process");
        type_until(&mut app, "\x03", "agy exit", |info| info.running.is_none());
    });
}

/// Shell job control is the shell's: `^Z` stops the agent and the tab reads
/// as its shell, keeping the agent; `fg` brings it back.
#[test]
fn a_stopped_agent_leaves_the_tab_a_shell_until_it_is_brought_back() {
    use crate::state::sessions::AgentKind;
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let bin = fake_agents(&dir);
        let mut app = App::test_app(dir.clone());
        assert!(app.open_pane_tab_in("sh", &dir));
        let claude = format!("{} 60\r", bin.join("claude").display());
        type_until(&mut app, &claude, "claude", |info| {
            kind_of(info) == AgentKind::Claude
        });

        type_until(&mut app, "\x1a", "^Z", |info| {
            info.running.is_some_and(|r| r.backgrounded)
        });
        let info = &app.runtime.pane_tabs.as_ref().unwrap().tabs()[0].info;
        assert_eq!(
            kind_of(info),
            AgentKind::Other,
            "the tab reads as its shell"
        );
        assert!(!info.suspended, "keys still reach the shell");

        type_until(&mut app, "fg\r", "fg", |info| {
            info.running.is_some_and(|r| !r.backgrounded)
        });
        assert_eq!(
            kind_of(&app.runtime.pane_tabs.as_ref().unwrap().tabs()[0].info),
            AgentKind::Claude
        );
        type_until(&mut app, "\x03", "exit", |info| info.running.is_none());
    });
}

/// Restore relaunches the tab's command, so a shell tab running an agent is
/// saved as its shell, with no conversation for restore to type into it.
#[test]
fn a_shell_tab_running_an_agent_is_saved_as_its_shell() {
    use crate::state::sessions::AgentKind;
    let tmp = tempfile::tempdir().unwrap();
    let state_root = tmp.path().join("state");
    crate::state::with_state_root(&state_root, || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let bin = fake_agents(&dir);
        let mut app = App::test_app(dir.clone());
        assert!(app.open_pane_tab_in("sh", &dir));
        let claude = format!("{} 60\r", bin.join("claude").display());
        type_until(&mut app, &claude, "claude", |info| {
            kind_of(info) == AgentKind::Claude
        });
        app.runtime.pane_tabs.as_mut().unwrap().tabs_mut()[0]
            .info
            .live_session_id = Some("11111111-1111-4111-8111-111111111111".into());

        app.save_session();
        let file = std::fs::read_dir(state_root.join("sessions"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| p.extension().is_some_and(|x| x == "json"))
            .expect("the session was saved");
        let saved: crate::state::sessions::Session =
            serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
        assert_eq!(saved.tabs[0].command, "sh");
        assert_eq!(saved.tabs[0].agent_kind, AgentKind::Other);
        assert_eq!(saved.tabs[0].agent_session_id, None);
        type_until(&mut app, "\x03", "exit", |info| info.running.is_none());
    });
}

/// `:hooks on!` restarts claude so it reads new hooks. In a shell tab that
/// would restart the shell, and type `/resume` into it, so there it installs
/// the hooks, which claude reloads on its own, and leaves the tab alone.
#[test]
fn hooks_on_bang_leaves_a_shell_tab_running_claude_alone() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap().join("project");
        std::fs::create_dir_all(&dir).unwrap();
        let mut app = App::test_app(dir.clone());
        assert!(app.open_pane_tab_in("sh", &dir));
        let shell = app.runtime.pane_tabs.as_ref().unwrap().tabs()[0]
            .pane
            .process_id();
        app.runtime.pane_tabs.as_mut().unwrap().tabs_mut()[0]
            .info
            .running = Some(crate::pane::RunningAgent {
            kind: crate::state::sessions::AgentKind::Claude,
            pid: 1,
            started_epoch_secs: 1,
            backgrounded: false,
            on_shared_daemon: false,
        });

        app.dispatch_command("hooks on!");

        let entry = &app.runtime.pane_tabs.as_ref().unwrap().tabs()[0];
        assert_eq!(entry.info.command, "sh");
        assert_eq!(entry.pane.process_id(), shell, "the shell was restarted");
        assert!(
            dir.join(".claude/settings.json").exists(),
            "hooks installed"
        );
        assert!(
            app.flash_text()
                .is_some_and(|f| f.contains("isn't restarted")),
            "{:?}",
            app.flash_text()
        );
    });
}
