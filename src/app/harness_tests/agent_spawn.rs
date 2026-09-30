//! A codex pane runs outside codex's shared background server, so its hooks
//! and MCP servers inherit this pane's env.

use super::*;

/// A stand-in `codex` in `dir/bin` that writes the arguments it was started
/// with to `dir/argv`, one per line, and waits.
fn fake_codex(dir: &std::path::Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let exe = bin.join("codex");
    std::fs::write(
        &exe,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nexec sleep 30\n",
            dir.join("argv").display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    exe
}

fn argv(dir: &std::path::Path) -> Vec<String> {
    let path = dir.join("argv");
    for _ in 0..300 {
        if let Ok(text) = std::fs::read_to_string(&path)
            && text.ends_with('\n')
        {
            return text
                .lines()
                .filter(|l| !l.is_empty())
                .map(String::from)
                .collect();
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("the fake codex never started");
}

/// Codex's shared server spawns every session's hooks and MCP servers with the
/// environment of whichever codex started it. So a pane's `SPYC_MCP_SOCK` and
/// `SPYC_PANE_ID` only reach them when the pane's codex runs on its own; the
/// tab still shows the command the user typed.
#[test]
fn a_codex_pane_runs_without_the_shared_daemon() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let cmd = format!("{} resume abc", fake_codex(&dir).display());
        let mut app = App::test_app(dir.clone());

        assert!(app.open_pane_tab_in(&cmd, &dir));

        assert_eq!(argv(&dir), ["--no-daemon", "resume", "abc"]);
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
        let cmd = fake_codex(&dir).display().to_string();
        let mut app = App::test_app(dir.clone());
        assert!(app.open_pane_tab_in("cat", &dir));

        assert!(app.spawn_agent_into_tab(0, &cmd, &dir, None));

        assert_eq!(argv(&dir), ["--no-daemon"]);
    });
}

/// `[pane] codex_daemon = true` leaves the command alone, and so does one that
/// already says `--no-daemon`.
#[test]
fn the_daemon_flag_is_added_only_when_wanted_and_missing() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let exe = fake_codex(&dir).display().to_string();

        let mut app = App::test_app(dir.clone());
        app.state.config.pane.codex_daemon = true;
        assert!(app.open_pane_tab_in(&exe, &dir));
        assert!(argv(&dir).is_empty(), "shared daemon allowed: no flag");

        std::fs::remove_file(dir.join("argv")).unwrap();
        let mut app = App::test_app(dir.clone());
        assert!(app.open_pane_tab_in(&format!("{exe} --no-daemon"), &dir));
        assert_eq!(argv(&dir), ["--no-daemon"], "not added twice");
    });
}
