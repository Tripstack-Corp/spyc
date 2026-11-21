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
