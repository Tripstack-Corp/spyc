//! `^a s` (`send_selection_to_pane`) against a real pane child: what the
//! agent actually receives, not what the effect claims to carry.

use super::*;
use crate::app::effect::Effect;

/// Deliver `effects`' pane input to the active pane the way the executor does,
/// then press Enter so a line-reading child hands the line over.
fn deliver(app: &mut App, effects: Vec<Effect>) {
    let tabs = app.runtime.pane_tabs.as_mut().expect("a pane tab");
    for e in effects {
        if let Effect::SendToPane { input, .. } = e {
            input.send_to(tabs.active_mut()).expect("send");
        }
    }
    tabs.active_mut().send_bytes(b"\r").expect("enter");
}

/// What a `cat > file` pane child wrote, once it has a whole line.
fn received(app: &mut App, got: &std::path::Path) -> String {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        if let Ok(s) = std::fs::read_to_string(got)
            && s.ends_with('\n')
        {
            return s;
        }
        if let Some(tabs) = app.runtime.pane_tabs.as_mut() {
            tabs.active_mut().drain_output();
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!("the pane child never received a line");
}

/// #9: a path under the pane's own cwd goes out relative to THAT cwd, and
/// anything else absolute. The pane here is an agent in its own worktree
/// while PROJECT_HOME is the repo above it, the workflow AGENTS.md
/// prescribes. A PROJECT_HOME-relative `wt/sub/f.txt` resolves against the
/// wrong directory from inside the worktree.
#[test]
fn send_selection_anchors_on_the_panes_cwd_not_project_home() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        let home = root.join("proj");
        let wt = home.join("wt");
        std::fs::create_dir_all(wt.join("sub")).unwrap();
        let inside = wt.join("sub/f.txt");
        let outside = home.join("other.txt");
        std::fs::write(&inside, "x").unwrap();
        std::fs::write(&outside, "y").unwrap();
        let got = root.join("got.txt");

        let mut app = App::test_app(home.clone());
        app.state.project_home = Some(home);
        let cmd = format!(
            "cat > {}",
            crate::shell::shell_quote(&got.to_string_lossy())
        );
        assert!(
            app.open_pane_tab_in(&cmd, &wt),
            "pane spawned in the worktree"
        );
        app.state.left.picks.insert(&inside);
        app.state.left.picks.insert(&outside);

        let effects = app.send_selection_to_pane();
        deliver(&mut app, effects);
        let line = received(&mut app, &got);
        let mut sent: Vec<&str> = line.split_whitespace().collect();
        sent.sort_unstable();
        let outside_abs = outside.to_string_lossy();
        let mut want = vec!["sub/f.txt", outside_abs.as_ref()];
        want.sort_unstable();
        assert_eq!(sent, want, "received: {line:?}");
        // What the requirement is about: from the pane's own cwd, every path
        // it was handed names a file that was picked.
        for token in sent {
            let resolved = wt.join(token);
            assert!(
                resolved == inside || resolved == outside,
                "{token} resolves to {} from the pane",
                resolved.display()
            );
        }
    });
}

/// The anchor is read when the paths are delivered, not taken from the tab's
/// cached cwd. A pane that moved since it spawned (a shell after `cd`) gets
/// paths relative to where it is now; the cache would still say the worktree,
/// and `sub/f.txt` would resolve to `sub/sub/f.txt`.
#[test]
fn a_pane_that_changed_directory_gets_paths_from_where_it_is_now() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        let wt = root.join("wt");
        std::fs::create_dir_all(wt.join("sub")).unwrap();
        let inside = wt.join("sub/f.txt");
        std::fs::write(&inside, "x").unwrap();
        let got = root.join("got.txt");

        let mut app = App::test_app(wt.clone());
        app.state.project_home = Some(wt.clone());
        let child = format!(
            "cd sub && exec cat > {}",
            crate::shell::shell_quote(&got.to_string_lossy())
        );
        let cmd = format!("sh -c {}", crate::shell::shell_quote(&child));
        assert!(app.open_pane_tab_in(&cmd, &wt));
        app.state.left.picks.insert(&inside);

        // Wait until the child has actually moved before sending.
        let pid = app
            .runtime
            .pane_tabs
            .as_ref()
            .and_then(|t| t.active().process_id())
            .expect("pane pid");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while crate::proc_cwd::cwd_for_pid(pid).as_deref() != Some(wt.join("sub").as_path()) {
            assert!(
                std::time::Instant::now() < deadline,
                "child never reached sub/"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        let effects = app.send_selection_to_pane();
        deliver(&mut app, effects);
        assert_eq!(received(&mut app, &got).trim_end(), "f.txt");
    });
}
