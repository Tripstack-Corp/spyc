//! S5: `:detach project` (`app::project_move`): what it refuses, that a
//! project it can't save stays where it is, and the session it saves for the
//! project it moves. The move itself, through real jars, is
//! `tests/jar_adopt.rs`'s.

use std::path::PathBuf;
use std::time::Instant;

use super::project_tabs::{agent_tab_in, report_at};
use super::*;
use crate::pane::AgentActivity;
use crate::state::scope_registry::{ScopeClaim, ScopeIntent};

/// `xproj` in view and `yproj` beside it, the state directory the test's.
fn with_two(test: impl FnOnce(&mut App, [state::ProjectId; 2], &[PathBuf; 2], &std::path::Path)) {
    let tmp = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(tmp.path()).unwrap();
    let state_dir = base.join("state");
    let homes = [base.join("xproj"), base.join("yproj")];
    for d in std::iter::once(&state_dir).chain(&homes) {
        std::fs::create_dir_all(d).unwrap();
    }
    crate::state::with_state_root(&state_dir, || {
        let mut app = App::test_app(homes[0].clone());
        let x = app.state.active;
        app.ready_test_project(x, &homes[0]);
        let y = app.add_test_project(&homes[1]);
        app.make_active(x);
        test(&mut app, [x, y], &homes, &state_dir);
    });
}

fn claim(app: &mut App, id: u64, owner: String, path: &str) {
    app.state.scope_registry.push(ScopeClaim {
        id,
        owner,
        owner_label: "claude".into(),
        paths: vec![path.into()],
        intent: ScopeIntent::Editing,
        pr: None,
        note: None,
        claimed_at_secs: 0,
    });
}

fn claim_owner(app: &mut App, project: state::ProjectId) -> String {
    app.as_project(project, |app| {
        app.runtime.project().pane_tabs.as_ref().unwrap().tabs()[0]
            .info
            .claim_owner
            .clone()
    })
}

fn saved_files(state: &std::path::Path) -> usize {
    std::fs::read_dir(state.join("sessions")).map_or(0, Iterator::count)
}

#[test]
fn the_only_project_open_isnt_detached() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let mut app = App::test_app(std::fs::canonicalize(tmp.path()).unwrap());
        app.detach_project("");
        let flash = app.flash_text().unwrap_or_default().to_string();
        assert!(flash.contains("only project open"), "{flash}");
        assert_eq!(app.state.open_projects().count(), 1);
        assert_eq!(saved_files(&tmp.path().join("state")), 0);
    });
}

#[test]
fn a_project_named_by_no_open_one_isnt_detached() {
    with_two(|app, _, _, state| {
        app.detach_project("nope");
        let flash = app.flash_text().unwrap_or_default().to_string();
        assert!(flash.contains("no open project is named nope"), "{flash}");
        assert_eq!(app.state.open_projects().count(), 2);
        assert_eq!(saved_files(state), 0);
    });
}

#[test]
fn a_detach_is_refused_while_a_tab_of_the_project_runs_outside_a_jar() {
    with_two(|app, [x, _], homes, state| {
        agent_tab_in(app, x, &homes[0], "claude");
        app.detach_project("xproj");
        let flash = app.flash_text().unwrap_or_default().to_string();
        assert!(flash.contains("outside a jar"), "{flash}");
        assert!(app.state.has_project(x));
        assert_eq!(saved_files(state), 0);
    });
}

/// Finding 6: nothing is let go before the moved project is saved, so one
/// that can't be saved stays, in view, with its claims.
#[test]
fn a_project_that_cant_be_saved_stays_where_it_is() {
    with_two(|app, [x, _], _, state| {
        claim(app, 1, "x-owner".into(), "src/x.rs");
        std::fs::write(state.join("sessions"), "in the way").unwrap();
        app.detach_project("");
        let flash = app.flash_text().unwrap_or_default().to_string();
        assert!(flash.contains("wasn't saved, so it stays"), "{flash}");
        assert_eq!(app.state.active, x, "the user was moved off it");
        assert_eq!(app.state.open_projects().count(), 2);
        assert_eq!(app.state.scope_registry.len(), 1);
    });
}

/// The moved project's session: that project alone, no running spyc's, with
/// its tabs' last reports and their claims, and none of another project's.
#[test]
fn the_moved_session_holds_the_project_its_claims_and_its_agents_reports() {
    with_two(|app, [x, y], homes, _| {
        let moving = agent_tab_in(app, x, &homes[0], "claude");
        agent_tab_in(app, y, &homes[1], "claude");
        report_at(app, x, 0, AgentActivity::Blocked, Instant::now());
        let (mine, theirs) = (claim_owner(app, x), claim_owner(app, y));
        claim(app, 1, mine, "src/x.rs");
        claim(app, 2, theirs, "src/y.rs");
        let manifest = app.moved_manifest(x, "MOVED_NAME");
        assert_eq!(manifest.name, "MOVED_NAME");
        assert_eq!(manifest.pid, None, "a running spyc's session isn't taken");
        let [project] = manifest.projects.as_slice() else {
            panic!("{:?}", manifest.projects);
        };
        assert_eq!(project.anchor(), homes[0].as_path());
        assert_eq!(project.tabs[0].pane_id, moving);
        assert_eq!(
            project.tabs[0].live.as_ref().map(|l| l.status.as_str()),
            Some("blocked")
        );
        let claims: Vec<_> = manifest.scope_claims.iter().map(|c| c.id).collect();
        assert_eq!(claims, [1]);
    });
}

#[test]
fn attaching_this_sessions_own_projects_is_refused() {
    with_two(|app, _, _, _| {
        app.state.session_name = Some("OWN_NAME".into());
        super::super::project_move::cmd_attach(app, "project OWN_NAME");
        let flash = app.flash_text().unwrap_or_default().to_string();
        assert!(flash.contains("OWN_NAME is this session"), "{flash}");
        super::super::project_move::cmd_attach(app, "nonsense");
        let flash = app.flash_text().unwrap_or_default().to_string();
        assert!(flash.contains(":attach project NAME"), "{flash}");
    });
}

/// `spyc/release` with a tab outside a jar: refused, nothing saved, spyc on.
#[test]
fn a_session_with_a_tab_outside_a_jar_isnt_released() {
    with_two(|app, [x, _], homes, state| {
        agent_tab_in(app, x, &homes[0], "claude");
        let answer = app.release_session();
        assert!(
            matches!(&answer, crate::mcp_cmd::McpResponse::Error { message } if message.contains("outside a jar")),
            "{answer:?}"
        );
        assert!(!app.state.should_quit);
        assert_eq!(saved_files(state), 0);
    });
}

/// A released session is saved once, as no running spyc's, with its claims;
/// spyc quits, and saves nothing over it on the way.
#[test]
fn a_released_session_is_saved_once_for_the_spyc_that_takes_it() {
    with_two(|app, _, _, state| {
        app.state.session_name = Some("GIVEN_AWAY".into());
        claim(app, 1, String::new(), "src/x.rs");
        let crate::mcp_cmd::McpResponse::Ok { message } = app.release_session() else {
            panic!("refused");
        };
        let id = serde_json::from_str::<serde_json::Value>(&message).unwrap()["session"]
            .as_u64()
            .unwrap();
        assert!(app.state.should_quit);
        let saved = crate::state::sessions::load_manifests();
        let [manifest] = saved.as_slice() else {
            panic!("{saved:?}");
        };
        assert_eq!((manifest.id, manifest.pid), (id, None));
        assert_eq!(manifest.projects.len(), 2);
        assert_eq!(manifest.scope_claims.len(), 1);
        let written = std::fs::read_dir(state.join("sessions"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let before = std::fs::read_to_string(&written).unwrap();
        app.save_session();
        assert_eq!(
            std::fs::read_to_string(&written).unwrap(),
            before,
            "saved over"
        );
        assert_eq!(saved_files(state), 1);
    });
}

/// A spyc that let its session go ends no program on its way out: they're
/// the other spyc's now.
#[test]
fn a_released_spycs_teardown_leaves_its_programs_running() {
    with_two(|app, [x, _], homes, _| {
        agent_tab_in(app, x, &homes[0], "claude");
        let pid = app.as_project(x, |app| {
            app.runtime.project().pane_tabs.as_ref().unwrap().tabs()[0]
                .pane
                .process_id()
                .unwrap()
        });
        app.runtime.released = true;
        app.run_teardown();
        let alive = |pid: u32| {
            rustix::process::Pid::from_raw(i32::try_from(pid).unwrap())
                .is_some_and(|p| rustix::process::test_kill_process(p).is_ok())
        };
        assert!(alive(pid), "teardown ended the tab's program");
        app.runtime.released = false;
        app.run_teardown();
    });
}

/// Taking a released session: its projects open beside this spyc's, its
/// claims come too, one renumbered whose id a claim here has, and its file
/// goes once this session is saved.
#[test]
fn a_taken_session_opens_beside_this_one_and_its_file_goes() {
    with_two(|app, [x, y], homes, _| {
        let elsewhere = homes[0].parent().unwrap().join("zproj");
        std::fs::create_dir_all(&elsewhere).unwrap();
        let z = app.add_test_project(&elsewhere);
        app.make_active(x);
        let given = app.moved_manifest(z, "GIVEN_AWAY");
        app.forget_project(z, "moved");
        let mut given = given;
        given.scope_claims.push(ScopeClaim {
            id: 7,
            owner: String::new(),
            owner_label: "claude".into(),
            paths: vec!["src/z.rs".into()],
            intent: ScopeIntent::Editing,
            pr: None,
            note: None,
            claimed_at_secs: 0,
        });
        crate::state::sessions::save_session_durably(&given).unwrap();
        claim(app, 7, String::new(), "src/here.rs");
        app.take_session("GIVEN_AWAY", &given);
        assert_eq!(app.state.open_projects().count(), 3);
        assert!(
            app.state.active != x && app.state.active != y,
            "the user isn't in it"
        );
        let ids: Vec<u64> = app.state.scope_registry.iter().map(|c| c.id).collect();
        assert_eq!(ids, [7, 8]);
        let names: Vec<String> = crate::state::sessions::load_manifests()
            .into_iter()
            .map(|m| m.name)
            .collect();
        assert!(!names.contains(&"GIVEN_AWAY".to_string()), "{names:?}");
    });
}
