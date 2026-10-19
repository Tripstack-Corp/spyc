//! #99 step 5e, pair 2b: closing a project from the leader (`Space x`).
//!
//! The fixtures are `projects_open.rs`'s: `x` is where the user starts, `y` is
//! open beside it, and `zproj/` is a directory no project has yet.

use std::time::{Duration, Instant};

use super::projects_open::{leader, open, with_three};
use super::*;
use crate::mcp_cmd::McpResponse;
use crate::state::scope_registry::{ScopeClaim, ScopeIntent};

/// The pid of the live tab `pane`, in whichever project holds it.
fn pid_of(app: &App, pane: &str) -> u32 {
    let (_, tab) = app.runtime.tab_of_pane(pane).expect("the tab is open");
    tab.pane.process_id().expect("the tab's child has a pid")
}

/// Whether `pid` exits within five seconds.
fn exits(pid: u32) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while crate::sysinfo::pid_alive(pid) {
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    true
}

fn claim(id: u64, owner: &str) -> ScopeClaim {
    ScopeClaim {
        id,
        owner: owner.to_string(),
        owner_label: format!("tab {id}"),
        paths: vec!["src/lib.rs".into()],
        intent: ScopeIntent::Merging,
        pr: None,
        note: None,
        claimed_at_secs: 0,
    }
}

/// `Space x` on a project with a tab whose child is still running asks
/// first, naming the project and how many are running, and `n` keeps it.
#[test]
#[ignore = "red: Space x closes nothing"]
fn space_x_with_a_running_tab_asks_and_n_keeps_the_project() {
    with_three(|t| {
        let (x, x_home) = (t.x, t.x_home.clone());
        let app = &mut t.app;
        app.open_tab_in_project(x, &x_home);
        leader(app, 'x');
        let Mode::Prompting(prompt) = &app.state.mode else {
            panic!("Space x asked nothing: {:?}", app.flash_text());
        };
        assert!(
            prompt.prefix.contains("close project 'xproj'") && prompt.prefix.contains("1 running"),
            "{}",
            prompt.prefix
        );
        app.handle_key(key('n')).unwrap();
        assert!(matches!(app.state.mode, Mode::Normal));
        assert!(app.state.has_project(x), "n closed it");
        assert_eq!(app.state.active, x);
    });
}

/// `y` closes it: the user lands in the project they were in last, not the
/// neighbour; its tab's child exits; its context file is removed; it is out
/// of every map; and the flash names it. `Space Space` never names a closed
/// project.
#[test]
#[ignore = "red: Space x closes nothing"]
fn y_closes_the_project_and_lands_in_the_last_one() {
    with_three(|t| {
        let app = &mut t.app;
        open(app, &t.z_home);
        let z = app.state.active;
        let pane = app.open_tab_in_project(z, &t.z_home);
        let pid = pid_of(app, &pane);
        let context = app.view.project().context_path.clone();
        assert!(context.exists(), "zproj's context file");

        leader(app, 'x');
        app.handle_key(key('y')).unwrap();

        assert_eq!(
            app.state.active, t.x,
            "not the last project the user was in"
        );
        assert!(!app.state.has_project(z));
        assert!(!app.runtime.projects.contains_key(&z), "its runtime");
        assert!(!app.view.projects.contains_key(&z), "its views");
        assert!(!context.exists(), "its context file");
        assert!(
            app.flash_text()
                .is_some_and(|f| f.contains("closed project zproj")),
            "{:?}",
            app.flash_text()
        );
        assert!(
            app.state
                .last_project
                .is_none_or(|id| app.state.has_project(id)),
            "Space Space goes to a closed project"
        );
        assert!(exits(pid), "its tab's child is still running");
    });
}

/// With no last project to go back to, closing lands on a neighbour. A
/// project with nothing running closes without asking.
#[test]
#[ignore = "red: Space x closes nothing"]
fn with_no_last_project_closing_lands_on_a_neighbour() {
    with_three(|t| {
        let app = &mut t.app;
        leader(app, 'x');
        assert!(matches!(app.state.mode, Mode::Normal), "it asked");
        assert!(!app.state.has_project(t.x));
        assert_eq!(app.state.active, t.y);
    });
}

/// The only project open isn't closed from the leader: quitting is `:q`'s.
#[test]
#[ignore = "red: Space x on the only project says nothing"]
fn space_x_on_the_only_project_says_q_closes_it() {
    let tmp = tempfile::tempdir().unwrap();
    let home = std::fs::canonicalize(tmp.path()).unwrap();
    crate::state::with_state_root(&home.join("state"), || {
        let mut app = App::test_app(home.clone());
        let only = app.state.active;
        leader(&mut app, 'x');
        assert!(app.state.has_project(only));
        assert!(matches!(app.state.mode, Mode::Normal));
        assert!(
            app.flash_text()
                .is_some_and(|f| f.contains("the last project closes with :q")),
            "{:?}",
            app.flash_text()
        );
    });
}

/// A closed project's tabs hold no scope claims, and an agent of its parked
/// on `wait_for_scope_clear` is answered rather than left waiting. Another
/// project's claims stay.
#[test]
#[ignore = "red: Space x closes nothing"]
fn closing_drops_its_tabs_claims_and_answers_their_waits() {
    with_three(|t| {
        let (x, y, x_home, y_home) = (t.x, t.y, t.x_home.clone(), t.y_home.clone());
        let app = &mut t.app;
        let x_pane = app.open_tab_in_project(x, &x_home);
        let y_pane = app.open_tab_in_project(y, &y_home);
        let owner = |app: &App, pane: &str| {
            let (_, tab) = app.runtime.tab_of_pane(pane).expect("the tab is open");
            tab.info.claim_owner.clone()
        };
        let (x_owner, y_owner) = (owner(app, &x_pane), owner(app, &y_pane));
        app.state.scope_registry.push(claim(1, &x_owner));
        app.state.scope_registry.push(claim(2, &y_owner));
        let (reply, answered) = std::sync::mpsc::channel();
        app.runtime.scope_waiters.push(PendingScopeWait {
            owner: x_owner,
            paths: vec!["src/lib.rs".into()],
            deadline: Instant::now() + Duration::from_secs(600),
            reply,
        });

        leader(app, 'x');
        app.handle_key(key('y')).unwrap();

        let owners: Vec<&str> = app
            .state
            .scope_registry
            .iter()
            .map(|c| c.owner.as_str())
            .collect();
        assert_eq!(owners, [y_owner.as_str()]);
        assert!(app.runtime.scope_waiters.is_empty(), "the wait is parked");
        let answer = answered.try_recv();
        assert!(
            matches!(answer, Ok(McpResponse::Error { .. })),
            "{answer:?}"
        );
    });
}

/// The sidecar discovery reads no longer lists a closed project's root.
#[test]
#[ignore = "red: Space x closes nothing"]
fn closing_drops_its_root_from_discovery() {
    with_three(|t| {
        let app = &mut t.app;
        app.view.mcp_running = true;
        leader(app, 'x');
        let state_dir = crate::state::state_root().expect("the test's state root");
        let sidecar = crate::mcp::root_marker_path_in(&state_dir, std::process::id());
        let recorded = std::fs::read_to_string(sidecar).unwrap_or_default();
        assert_eq!(
            recorded.lines().collect::<Vec<_>>(),
            [t.y_home.to_str().unwrap()]
        );
    });
}
