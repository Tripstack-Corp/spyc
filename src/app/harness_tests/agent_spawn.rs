//! A codex pane runs outside codex's shared background server, so its hooks
//! and MCP servers inherit this pane's env.

use super::*;

/// Codex's shared server spawns every session's hooks and MCP servers with the
/// environment of whichever codex started it. So a pane's `SPYC_MCP_SOCK` and
/// `SPYC_PANE_ID` only reach them when the pane's codex runs on its own; the
/// tab still shows the command the user typed.
#[test]
fn a_codex_pane_runs_without_the_shared_daemon() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let cmd = format!("{} resume abc", fake_agent(&dir, "codex").display());
        let mut app = App::test_app(dir.clone());

        assert!(app.open_pane_tab_in(&cmd, &dir));

        assert_eq!(agent_argv(&dir, "codex"), ["--no-daemon", "resume", "abc"]);
        let tabs = app.runtime.pane_tabs.as_ref().unwrap();
        assert_eq!(
            tabs.tabs()[0].info.command,
            cmd,
            "the tab keeps what was typed"
        );
    });
}

/// A crashed agent tab respawns the same way.
#[test]
fn a_respawned_codex_tab_runs_without_the_shared_daemon() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let cmd = fake_agent(&dir, "codex").display().to_string();
        let mut app = App::test_app(dir.clone());
        assert!(app.open_pane_tab_in("cat", &dir));

        assert!(app.spawn_agent_into_tab(0, &cmd, &dir, None));

        assert_eq!(agent_argv(&dir, "codex"), ["--no-daemon"]);
    });
}

/// `[pane] codex_daemon = true` leaves the command alone, and so does one that
/// already says `--no-daemon`.
#[test]
fn the_daemon_flag_is_added_only_when_wanted_and_missing() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let exe = fake_agent(&dir, "codex").display().to_string();

        let mut app = App::test_app(dir.clone());
        app.state.config.pane.codex_daemon = true;
        assert!(app.open_pane_tab_in(&exe, &dir));
        assert!(
            agent_argv(&dir, "codex").is_empty(),
            "shared daemon allowed: no flag"
        );

        std::fs::remove_file(dir.join("codex.argv")).unwrap();
        let mut app = App::test_app(dir.clone());
        assert!(app.open_pane_tab_in(&format!("{exe} --no-daemon"), &dir));
        assert_eq!(
            agent_argv(&dir, "codex"),
            ["--no-daemon"],
            "not added twice"
        );
    });
}
