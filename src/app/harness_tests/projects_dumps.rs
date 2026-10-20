//! #99 step 6d: the coordination dumps cover every open project, each
//! project's tabs under its label: `:activity dump`, `:agent list` and
//! `:agent registry`.
//!
//! Two projects in one `App`, `x` (opened first) and `y`, the user in `y`, each
//! with a `claude` tab.

use super::project_tabs::agent_tab_in;
use super::*;
use crate::mcp_cmd::McpCommand;
use crate::state::scope_registry::{ScopeClaim, ScopeIntent};

struct Two {
    app: App,
    x: state::ProjectId,
    y: state::ProjectId,
    base: std::path::PathBuf,
    /// x's tab's pane id.
    x_pane: String,
    /// y's tab's pane id.
    y_pane: String,
}

fn with_two(test: impl FnOnce(&mut Two)) {
    let tmp = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(tmp.path()).unwrap();
    let [state_dir, x_home, y_home] = ["state", "xproj", "yproj"].map(|d| base.join(d));
    for d in [&state_dir, &x_home, &y_home] {
        std::fs::create_dir_all(d).unwrap();
    }
    crate::state::with_state_root(&state_dir, || {
        let mut app = App::test_app(x_home.clone());
        let x = app.state.active;
        app.ready_test_project(x, &x_home);
        let y = app.add_test_project(&y_home);
        let x_pane = agent_tab_in(&mut app, x, &x_home, "claude");
        let y_pane = agent_tab_in(&mut app, y, &y_home, "claude");
        app.make_active(y);
        test(&mut Two {
            app,
            x,
            y,
            base,
            x_pane,
            y_pane,
        });
    });
}

/// The pager's lines after `:<command> <args>`.
fn dump(app: &mut App, command: &str, args: &str) -> Vec<String> {
    match command {
        "activity" => super::super::commands::cmd_activity(app, args),
        _ => super::super::commands::cmd_agent(app, args),
    };
    let view = app.view.project().pager.as_ref().expect("a pager opened");
    view.lines.iter().map(ToString::to_string).collect()
}

fn at(lines: &[String], pred: impl Fn(&str) -> bool) -> Option<usize> {
    lines.iter().position(|l| pred(l))
}

/// A claim owned by the tab `pane`.
fn claim_for(app: &mut App, pane: &str) {
    let (_, entry) = app.runtime.tab_of_pane(pane).expect("a live tab");
    let owner = entry.info.claim_owner.clone();
    app.state.scope_registry.push(ScopeClaim {
        id: 1,
        owner,
        owner_label: "claude".into(),
        paths: vec!["src/lib.rs".into()],
        intent: ScopeIntent::Editing,
        pr: None,
        note: None,
        claimed_at_secs: crate::sysinfo::epoch_secs(),
    });
}

/// The control: with one project open, each dump reads as it always has, no
/// project named anywhere.
#[test]
fn with_one_project_the_dumps_read_as_before() {
    let tmp = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(tmp.path()).unwrap();
    let (state_dir, home) = (base.join("state"), base.join("xproj"));
    for d in [&state_dir, &home] {
        std::fs::create_dir_all(d).unwrap();
    }
    crate::state::with_state_root(&state_dir, || {
        let mut app = App::test_app(home.clone());
        let x = app.state.active;
        app.ready_test_project(x, &home);
        let pane = agent_tab_in(&mut app, x, &home, "claude");
        app.execute_mcp_command(McpCommand::ConnectionInitialized {
            conn: 7,
            pane_id: Some(pane.clone()),
        });
        claim_for(&mut app, &pane);

        let activity = dump(&mut app, "activity", "dump");
        assert!(
            activity
                .iter()
                .any(|l| l.starts_with("  #7  tab [1] \"claude\"  since ")),
            "{activity:#?}"
        );
        assert!(activity.iter().any(|l| l.starts_with("*[1] \"claude\"")));
        assert!(
            !activity.iter().any(|l| l.starts_with("project ")),
            "no project heading: {activity:#?}"
        );
        let list = dump(&mut app, "agent", "list");
        assert!(
            list.iter().any(|l| l.starts_with("*[1] claude  dot=")),
            "{list:#?}"
        );
        assert!(
            !list.iter().any(|l| l.starts_with("project ")),
            "no project heading: {list:#?}"
        );
        let registry = dump(&mut app, "agent", "registry");
        assert!(
            registry
                .iter()
                .any(|l| l.starts_with("[1] claude — editing")),
            "{registry:#?}"
        );

        // Line for line: one blank between the connections and the tab, whose
        // report a blank closes, and the list's lines after its header.
        let conn = at(&activity, |l| l.starts_with("  #7  ")).expect("the connection");
        assert_eq!(activity[conn + 1], "", "{activity:#?}");
        assert!(activity[conn + 2].starts_with("*[1] "), "{activity:#?}");
        assert_eq!(activity.last().map(String::as_str), Some(""));
        assert_eq!(
            list[1..],
            ["", "*[1] claude  dot=unknown", "    scope: src/lib.rs"],
            "{list:#?}"
        );
    });
}

/// The control with no tabs: each dump says so, as it always has.
#[test]
fn with_one_project_and_no_tabs_the_dumps_read_as_before() {
    let tmp = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(tmp.path()).unwrap();
    let (state_dir, home) = (base.join("state"), base.join("xproj"));
    for d in [&state_dir, &home] {
        std::fs::create_dir_all(d).unwrap();
    }
    crate::state::with_state_root(&state_dir, || {
        let mut app = App::test_app(home.clone());
        let x = app.state.active;
        app.ready_test_project(x, &home);
        let activity = dump(&mut app, "activity", "dump");
        let conns = at(&activity, |l| l.starts_with("mcp connections: ")).expect("connections");
        assert_eq!(
            activity[conns + 1..],
            ["", "(no panes open)"],
            "{activity:#?}"
        );
        let list = dump(&mut app, "agent", "list");
        assert_eq!(list[1..], ["(no panes open)"], "{list:#?}");
    });
}

/// `:activity dump` reports every project's tabs, each project under its
/// label in switcher order, the one in view marked.
#[test]
fn activity_dump_reports_every_projects_tabs_under_its_label() {
    with_two(|t| {
        let lines = dump(&mut t.app, "activity", "dump");
        let x_head = at(&lines, |l| l == "project xproj")
            .unwrap_or_else(|| panic!("no heading for x: {lines:#?}"));
        let y_head = at(&lines, |l| l == "project yproj (in view)").expect("y's heading");
        let x_tab = at(&lines, |l| l == format!("    pane_id: {}", t.x_pane));
        let y_tab = at(&lines, |l| l == format!("    pane_id: {}", t.y_pane));
        assert!(x_head < y_head, "switcher order: {lines:#?}");
        assert!(
            x_tab.is_some_and(|i| x_head < i && i < y_head),
            "x's tab is under x: {lines:#?}"
        );
        assert!(y_tab.is_some_and(|i| y_head < i), "y's tab is under y");
    });
}

/// A connection is listed with its tab and the project it acts on.
#[test]
fn activity_dump_names_each_connections_project() {
    with_two(|t| {
        let pane = t.x_pane.clone();
        t.app
            .execute_mcp_command(McpCommand::ConnectionInitialized {
                conn: 7,
                pane_id: Some(pane),
            });
        let lines = dump(&mut t.app, "activity", "dump");
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("  #7  tab [1] \"claude\" in xproj  since ")),
            "{lines:#?}"
        );
    });
}

/// `:agent list` covers every project's agents, under each project's label.
#[test]
fn agent_list_covers_every_project() {
    with_two(|t| {
        let lines = dump(&mut t.app, "agent", "list");
        let x_head = at(&lines, |l| l == "project xproj")
            .unwrap_or_else(|| panic!("no heading for x: {lines:#?}"));
        let y_head = at(&lines, |l| l == "project yproj (in view)").expect("y's heading");
        let agents: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter(|(_, l)| l.starts_with("*[1] claude  dot="))
            .map(|(i, _)| i)
            .collect();
        assert_eq!(agents.len(), 2, "both agents: {lines:#?}");
        assert!(x_head < agents[0] && agents[0] < y_head && y_head < agents[1]);
    });
}

/// `:agent registry` names the project of the tab that owns each claim.
#[test]
fn agent_registry_names_each_claims_project() {
    with_two(|t| {
        let pane = t.x_pane.clone();
        claim_for(&mut t.app, &pane);
        let lines = dump(&mut t.app, "agent", "registry");
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("[1] claude in xproj — editing")),
            "{lines:#?}"
        );
    });
}

/// The connection line for `conn` in `:activity dump`.
fn connection_line(app: &mut App, conn: u64) -> String {
    let lines = dump(app, "activity", "dump");
    let start = format!("  #{conn}  ");
    lines
        .into_iter()
        .find(|l| l.starts_with(&start))
        .expect("the connection is listed")
}

/// A connection with no tab, bound by its cwd or to the project the user was
/// in, is named for the project the server bound it to.
#[test]
fn a_connection_without_a_tab_is_named_for_its_project() {
    with_two(|t| {
        let x = t.x;
        t.app
            .execute_mcp_command(McpCommand::ConnectionInitialized {
                conn: 8,
                pane_id: None,
            });
        assert!(
            !connection_line(&mut t.app, 8).contains(" in "),
            "not bound yet"
        );
        t.app.execute_mcp_command(McpCommand::ConnectionBound {
            conn: 8,
            project: x,
        });
        let line = connection_line(&mut t.app, 8);
        assert!(line.starts_with("  #8  unattributed"), "{line}");
        assert!(line.contains(") in xproj  since "), "{line}");
    });
}

/// A connection whose project has closed says so, even with one project
/// left: it acts on nothing until it binds again.
#[test]
fn a_connection_whose_project_closed_says_so() {
    with_two(|t| {
        let (x, y) = (t.x, t.y);
        t.app
            .execute_mcp_command(McpCommand::ConnectionInitialized {
                conn: 9,
                pane_id: None,
            });
        t.app.execute_mcp_command(McpCommand::ConnectionBound {
            conn: 9,
            project: y,
        });
        t.app.make_active(x);
        t.app.remove_test_project(y);
        let line = connection_line(&mut t.app, 9);
        assert!(line.contains(" in yproj (closed)  since "), "{line}");
    });
}

/// A project with no tabs is listed under its heading too, and says so.
#[test]
fn a_project_with_no_tabs_says_so_under_its_heading() {
    with_two(|t| {
        let z_home = t.base.join("zproj");
        std::fs::create_dir_all(&z_home).unwrap();
        t.app.add_test_project(&z_home);
        for (command, args) in [("activity", "dump"), ("agent", "list")] {
            let lines = dump(&mut t.app, command, args);
            let z = at(&lines, |l| l == "project zproj").expect("z's heading");
            let next = lines.get(z + 1).map(String::as_str);
            assert_eq!(next, Some("(no panes open)"), "{command}: {lines:#?}");
        }
    });
}

/// While several projects are open, one whose tabs are all shells says it has
/// no agents under its heading in `:agent list`, rather than nothing.
#[test]
fn a_project_with_only_shells_says_it_has_no_agents() {
    with_two(|t| {
        let z_home = t.base.join("zproj");
        std::fs::create_dir_all(&z_home).unwrap();
        let z = t.app.add_test_project(&z_home);
        agent_tab_in(&mut t.app, z, &z_home, "cat");
        let lines = dump(&mut t.app, "agent", "list");
        let at_z = at(&lines, |l| l == "project zproj").expect("z's heading");
        let next = lines.get(at_z + 1).map(String::as_str);
        assert_eq!(next, Some("(no agents)"), "{lines:#?}");
    });
}
