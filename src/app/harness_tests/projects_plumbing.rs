//! #99 step 5e, pair 1: what a second project needs before a key opens one.
//!
//! Two projects in one `App`: `x`, where the user starts, and `y`.

use std::path::PathBuf;

use super::*;

struct Two {
    app: App,
    y: state::ProjectId,
    x_home: PathBuf,
    y_home: PathBuf,
}

/// `x` is active and lists `xproj/`; `y` lists `yproj/`.
fn with_two(test: impl FnOnce(&mut Two)) {
    let tmp = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(tmp.path()).unwrap();
    let (state_dir, x_home, y_home) = (base.join("state"), base.join("xproj"), base.join("yproj"));
    for d in [&state_dir, &x_home.join("sub"), &y_home] {
        std::fs::create_dir_all(d).unwrap();
    }
    crate::state::with_state_root(&state_dir, || {
        let mut app = App::test_app(x_home.clone());
        let x = app.state.active;
        app.ready_test_project(x, &x_home);
        let y = app.add_test_project(&y_home);
        test(&mut Two {
            app,
            y,
            x_home,
            y_home,
        });
    });
}

fn home(app: &App) -> Option<PathBuf> {
    app.state.active_project().home.clone()
}

// ── Homes are unique ─────────────────────────────────────────────────────────

/// `Space P` in Y, with Y's column standing in X's home, refuses: one
/// directory is one project's home, so which project it belongs to is never
/// in doubt.
#[test]
fn space_p_onto_another_projects_home_refuses() {
    with_two(|t| {
        let app = &mut t.app;
        app.make_active(t.y);
        // Put Y's column there without `chdir`, which moves the process cwd.
        app.state.cur_mut().listing = crate::fs::listing::Listing::read(&t.x_home).unwrap();
        app.handle_key(key(' ')).unwrap();
        app.handle_key(key('P')).unwrap();
        assert_eq!(home(app), Some(t.y_home.clone()), "Y took X's home");
        assert!(
            app.flash_text()
                .is_some_and(|f| f.contains("project xproj")),
            "the refusal names X: {:?}",
            app.flash_text()
        );
    });
}

/// `:project <dir>` refuses another open project's home the same way.
#[test]
fn project_command_onto_another_projects_home_refuses() {
    with_two(|t| {
        let app = &mut t.app;
        app.make_active(t.y);
        let _ = app
            .state
            .dispatch_command(&format!("project {}", t.x_home.display()));
        assert_eq!(home(app), Some(t.y_home.clone()), "Y took X's home");
        assert!(
            app.flash_text()
                .is_some_and(|f| f.contains("project xproj")),
            "the refusal names X: {:?}",
            app.flash_text()
        );
    });
}

/// `:project clear` leaves a project anchored at its start directory, so it
/// is refused when another project has that directory as its home.
#[test]
fn clearing_a_home_onto_another_projects_refuses() {
    with_two(|t| {
        let app = &mut t.app;
        let x = app.state.active;
        let sub = t.x_home.join("sub");
        let _ = app
            .state
            .dispatch_command(&format!("project {}", sub.display()));
        app.make_active(t.y);
        let _ = app
            .state
            .dispatch_command(&format!("project {}", t.x_home.display()));
        assert_eq!(
            home(app),
            Some(t.x_home.clone()),
            "X's home is sub, so Y may"
        );
        app.make_active(x);
        let _ = app.state.dispatch_command("project clear");
        assert_eq!(home(app), Some(sub), "X fell back onto Y's home");
    });
}

/// Moving a homeless project's start directory (`gS`, `:startdir`) moves its
/// anchor, so onto another project's home it is refused. Y's home is not its
/// start dir, so the rule that start dirs are unique plays no part.
#[test]
fn moving_a_homeless_projects_start_onto_another_home_refuses() {
    with_two(|t| {
        let app = &mut t.app;
        let x = app.state.active;
        let y_sub = t.y_home.join("sub");
        std::fs::create_dir(&y_sub).unwrap();
        app.make_active(t.y);
        let _ = app
            .state
            .dispatch_command(&format!("project {}", y_sub.display()));
        app.make_active(x);
        let _ = app.state.dispatch_command("project clear");
        let start = || t.x_home.clone();
        app.state.cur_mut().listing = crate::fs::listing::Listing::read(&y_sub).unwrap();
        app.handle_key(key('g')).unwrap();
        app.handle_key(key('S')).unwrap();
        assert_eq!(
            app.state.active_project().start_dir,
            start(),
            "gS took Y's home"
        );
        let _ = app
            .state
            .dispatch_command(&format!("startdir {}", y_sub.display()));
        assert_eq!(
            app.state.active_project().start_dir,
            start(),
            ":startdir took Y's home"
        );
    });
}

/// A project whose home was cleared (`:project clear`) is anchored at its
/// start directory, which no other project may take as its home.
#[test]
fn a_project_with_no_home_still_holds_its_start_directory() {
    with_two(|t| {
        let app = &mut t.app;
        let x_start = app.state.active_project().start_dir.clone();
        let _ = app.state.dispatch_command("project clear");
        app.make_active(t.y);
        let _ = app
            .state
            .dispatch_command(&format!("project {}", x_start.display()));
        assert_eq!(home(app), Some(t.y_home.clone()), "Y took X's anchor");
    });
}

/// The control: re-homing onto a directory no project has works as before.
#[test]
fn rehoming_onto_a_free_directory_works() {
    with_two(|t| {
        let app = &mut t.app;
        let sub = t.x_home.join("sub");
        let _ = app
            .state
            .dispatch_command(&format!("project {}", sub.display()));
        assert_eq!(home(app), Some(sub));
    });
}

/// The project an MCP connection that names only `cwd` binds to.
fn bound_by_cwd(app: &mut App, cwd: &std::path::Path) -> state::ProjectId {
    let reply = app.execute_mcp_command(crate::mcp_cmd::McpCommand::Bind {
        pane_id: None,
        cwd: Some(cwd.to_path_buf()),
    });
    let crate::mcp_cmd::McpResponse::Ok { message } = reply else {
        panic!("{reply:?}");
    };
    let bound: serde_json::Value = serde_json::from_str(&message).unwrap();
    bound["project"].as_str().unwrap().parse().unwrap()
}

/// A start dir is the root an agent's cwd binds to its project by, so one
/// project's is refused as another's, even for a project with a home: two
/// projects there would bind an agent working in X's start dir to Y.
#[test]
fn moving_a_start_dir_onto_another_projects_start_dir_refuses() {
    with_two(|t| {
        let app = &mut t.app;
        let x = app.state.active;
        app.make_active(t.y);
        let _ = app
            .state
            .dispatch_command(&format!("startdir {}", t.x_home.display()));
        assert_eq!(
            app.state.active_project().start_dir,
            t.y_home,
            ":startdir took X's start dir"
        );
        assert!(
            app.flash_text()
                .is_some_and(|f| f.contains("project xproj's start dir")),
            "the refusal names X's start dir: {:?}",
            app.flash_text()
        );
        assert_eq!(
            bound_by_cwd(app, &t.x_home),
            x,
            "an agent in X's start dir bound to Y"
        );
    });
}

/// A project with no home is anchored at its start dir, and a refusal calls
/// that directory what it is rather than the project's home.
#[test]
fn a_refusal_names_a_homeless_projects_start_dir_as_such() {
    with_two(|t| {
        let app = &mut t.app;
        let _ = app.state.dispatch_command("project clear");
        app.make_active(t.y);
        let _ = app
            .state
            .dispatch_command(&format!("project {}", t.x_home.display()));
        let flash = app.flash_text().unwrap_or_default().to_string();
        assert!(flash.contains("project xproj's start dir"), "{flash}");
        assert!(!flash.contains("'s home"), "{flash}");
    });
}

/// `:project clear` falls back to the project's start dir, so a refusal
/// because that is taken says so: the directory it names is not the home
/// being cleared.
#[test]
fn a_refused_clear_says_where_it_would_fall_back() {
    with_two(|t| {
        let app = &mut t.app;
        let x = app.state.active;
        let _ = app
            .state
            .dispatch_command(&format!("project {}", t.x_home.join("sub").display()));
        app.make_active(t.y);
        let _ = app
            .state
            .dispatch_command(&format!("project {}", t.x_home.display()));
        app.make_active(x);
        let _ = app.state.dispatch_command("project clear");
        let flash = app.flash_text().unwrap_or_default().to_string();
        assert!(flash.contains("fall back"), "{flash}");
        assert!(flash.contains(&t.x_home.display().to_string()), "{flash}");
    });
}
