//! Where a saved tab reopens: a shell tab where its shell was when the session
//! was saved, an agent tab where the agent started, which is where it finds its
//! conversation.

use super::*;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// A program that changes into the directory it is given and waits there, as
/// a shell does after `cd`. One script beside the test binary, named by a hash
/// of its text, so a new executable isn't scanned for every test.
fn wanderer_script() -> PathBuf {
    use std::hash::{Hash, Hasher};
    use std::os::unix::fs::PermissionsExt;
    const TEXT: &str = "#!/bin/sh\ncd \"$1\" && exec sleep 30\n";
    static SCRIPT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    SCRIPT
        .get_or_init(|| {
            let mut hash = std::collections::hash_map::DefaultHasher::new();
            TEXT.hash(&mut hash);
            let exe = std::env::current_exe().expect("the test binary has a path");
            let script = exe
                .parent()
                .expect("the test binary is in a directory")
                .join(format!("spyc-wanderer-{:016x}", hash.finish()));
            if !script.exists() {
                let tmp = script.with_extension(format!("{}.tmp", std::process::id()));
                std::fs::write(&tmp, TEXT).unwrap();
                std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755)).unwrap();
                std::fs::rename(&tmp, &script).unwrap();
            }
            script
        })
        .clone()
}

/// `dir/bin/<name>`, running [`wanderer_script`]: the name decides whether
/// spyc takes the tab for an agent.
fn wanderer(dir: &Path, name: &str) -> PathBuf {
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let exe = bin.join(name);
    std::os::unix::fs::symlink(wanderer_script(), &exe).unwrap();
    exe
}

fn read_saved(state_root: &Path, id: u64) -> crate::state::sessions::Manifest {
    parsed(
        serde_json::from_slice(
            &std::fs::read(state_root.join("sessions").join(format!("{id}.json"))).unwrap(),
        )
        .unwrap(),
    )
}

/// Wait until every tab's process has moved to `to`. A bound for a broken
/// spawn, not a measure of a working one.
fn wait_until_moved(app: &App, to: &Path) {
    let deadline = Instant::now() + Duration::from_secs(30);
    let tabs = app.runtime.project().pane_tabs.as_ref().unwrap();
    for tab in tabs.tabs() {
        let pid = tab.pane.process_id().expect("the tab's process runs");
        while crate::proc_cwd::cwd_for_pid(pid).as_deref() != Some(to) {
            assert!(Instant::now() < deadline, "{} never moved", tab.info.label);
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// A shell tab saves the directory its shell moved to, background or not; an
/// agent tab saves the one it started in.
#[test]
fn a_shell_tab_saves_where_its_shell_is_and_an_agent_tab_where_it_started() {
    let tmp = tempfile::tempdir().unwrap();
    let state_root = tmp.path().join("state");
    crate::state::with_state_root(&state_root, || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let moved = dir.join("moved");
        std::fs::create_dir(&moved).unwrap();
        let shell = wanderer(&dir, "wander");
        let claude = wanderer(&dir, "claude");
        // The session's own `cwd` is the crate root: restoring it `chdir`s the
        // whole process (see `session_restore`).
        let saved = parsed(serde_json::json!({
            "id": 992, "name": "TAB_CWD", "saved_at": "", "epoch_secs": 0,
            "cwd": env!("CARGO_MANIFEST_DIR"),
            "tabs": [
                {"command": format!("{} {}", shell.display(), moved.display()),
                 "label": "shell", "cwd": dir, "agent_kind": "other"},
                {"command": format!("{} {}", claude.display(), moved.display()),
                 "label": "claude", "cwd": dir, "agent_kind": "claude"},
            ],
            "active_tab": 1, "pane_height_pct": 50, "pane_focused": true
        }));
        let mut app = App::test_app(dir.clone());
        app.restore_session(&saved);
        wait_until_moved(&app, &moved);
        app.save_session();
        let tabs = &read_saved(&state_root, saved.id).projects[0].tabs;
        assert_eq!(tabs[0].cwd, moved, "the shell tab, in the background");
        assert_eq!(tabs[1].cwd, dir, "the agent tab");
    });
}
