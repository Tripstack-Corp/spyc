//! A startup tab's `agent` hint (#610): a command that wraps an agent is that
//! agent's tab, run exactly as written.

use super::*;
use crate::state::sessions::AgentKind;

/// A wrapper that records how many arguments it was started with, and which,
/// and the pane id it inherited, then waits: anything spyc appended would
/// arrive as arguments. It starts with `env`, as `npx` would, not a shell: a
/// shell tab gets the agent env for being a shell, which would hide whether
/// the hint gave it.
fn recording_wrapper(dir: &std::path::Path) -> (String, std::path::PathBuf, std::path::PathBuf) {
    let args = dir.join("args.txt");
    let pane = dir.join("pane.txt");
    let cmd = format!(
        "env sh -c 'printf %s \"$# $*\" > {}; printf %s \"$SPYC_PANE_ID\" > {}; exec sleep 60' wrap",
        args.display(),
        pane.display()
    );
    (cmd, args, pane)
}

fn info(app: &App) -> &crate::pane::TabInfo {
    &app.runtime.project().pane_tabs.as_ref().unwrap().tabs()[0].info
}

/// The hint makes a wrapper its agent's tab (env, identity) while spyc adds
/// nothing to its command, and save keeps it as written.
#[test]
fn a_wrapper_tab_is_its_agents_and_runs_as_written() {
    let tmp = tempfile::tempdir().unwrap();
    let state_root = tmp.path().join("state");
    crate::state::with_state_root(&state_root, || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let (cmd, args, pane) = recording_wrapper(&dir);
        let mut app = App::test_app(dir.clone());
        assert!(app.open_pane_tab_as(&cmd, &dir, Some(AgentKind::Codex)));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while !pane.exists() {
            assert!(
                std::time::Instant::now() < deadline,
                "the wrapper never ran"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(
            std::fs::read_to_string(&args).unwrap(),
            "0 ",
            "spyc added arguments"
        );
        assert_eq!(std::fs::read_to_string(&pane).unwrap(), info(&app).id);
        let tab = info(&app);
        assert_eq!(tab.agent().kind(), AgentKind::Codex);
        assert!(tab.wraps_agent());
        assert_eq!(tab.command, cmd);

        // A claude wrapper too: claude's own flag-stripping would take the `-c`.
        let claude = "env sh -c 'exec sleep 60' claude-wrap";
        assert!(app.open_pane_tab_as(claude, &dir, Some(AgentKind::Claude)));

        app.save_session();
        let file = std::fs::read_dir(state_root.join("sessions"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| p.extension().is_some_and(|x| x == "json"))
            .expect("the session was saved");
        let saved: crate::state::sessions::Session =
            serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
        assert_eq!(saved.tabs[0].command, cmd);
        assert_eq!(saved.tabs[0].agent_kind, AgentKind::Codex);
        assert_eq!(saved.tabs[0].agent_hint, Some(AgentKind::Codex));
        assert_eq!(saved.tabs[1].command, claude);
        assert_eq!(saved.tabs[1].agent_hint, Some(AgentKind::Claude));
    });
}

/// Forking would mean adding flags to the wrapper, so it is refused.
#[test]
fn a_wrapper_tab_is_not_forked() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let mut app = App::test_app(dir.clone());
        assert!(app.open_pane_tab_as(
            "env sh -c 'exec sleep 60' claude-wrap",
            &dir,
            Some(AgentKind::Claude)
        ));
        app.runtime
            .project_mut()
            .pane_tabs
            .as_mut()
            .unwrap()
            .tabs_mut()[0]
            .info
            .live_session_id = Some("11111111-1111-4111-8111-111111111111".into());
        app.fork_active_tab();
        assert_eq!(
            app.runtime
                .project()
                .pane_tabs
                .as_ref()
                .unwrap()
                .tabs()
                .len(),
            1
        );
        assert!(
            app.flash_text()
                .is_some_and(|f| f.contains("adds no flags to a wrapper")),
            "{:?}",
            app.flash_text()
        );
    });
}

/// `^a R` and `:hooks on!` respawn the tab: it stays its agent's, and its
/// command stays as written. `:hooks on!` would otherwise strip claude's flags
/// from it, here the `-c` that belongs to `sh`.
#[test]
fn a_wrapper_tab_keeps_its_hint_and_command_through_a_respawn() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let cmd = "env sh -c 'exec sleep 60' claude-wrap";
        let mut app = App::test_app(dir.clone());
        assert!(app.open_pane_tab_as(cmd, &dir, Some(AgentKind::Claude)));

        app.restart_active_tab_now();
        assert_eq!(info(&app).agent_hint, Some(AgentKind::Claude), "^a R");
        assert_eq!(info(&app).command, cmd);

        app.dispatch_command("hooks on!");
        assert_eq!(info(&app).agent_hint, Some(AgentKind::Claude), ":hooks on!");
        assert_eq!(info(&app).command, cmd, ":hooks on!");
    });
}
