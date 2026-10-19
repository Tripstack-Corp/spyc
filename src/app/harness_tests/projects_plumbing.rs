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
    let home = base.join("home");
    std::fs::create_dir_all(&home).unwrap();
    crate::state::with_state_root(&state_dir, || {
        crate::config::with_home_root(&home, || {
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

// ── Config, for every project ────────────────────────────────────────────────

fn write_rc(home: &std::path::Path, body: &str) {
    std::fs::write(home.join(".spycrc.toml"), body).unwrap();
}

/// `^R`, or an edit to a project's rc, reloads every open project from its
/// own rc, not only the one in view: Y's edit is in force once the user goes
/// back to Y.
#[test]
fn a_reload_reaches_every_project_from_its_own_rc() {
    with_two(|t| {
        let app = &mut t.app;
        write_rc(&t.y_home, "[layout]\nstatus_position = \"bottom\"\n");
        app.reload_config();
        assert_eq!(
            app.as_project(t.y, |app| app
                .state
                .active_project()
                .config
                .layout
                .status_position),
            crate::config::StatusPosition::Bottom,
            "Y's edit isn't in force in Y"
        );
    });
}

/// An error in another project's rc is the flash, named for that project,
/// over the "reloaded" note of the project in view.
#[test]
fn an_error_in_another_projects_rc_is_the_flash() {
    with_two(|t| {
        let app = &mut t.app;
        write_rc(&t.y_home, "[layout\nstatus_position = \"top\"\n");
        app.reload_config();
        let flash = app.state.flash.clone();
        assert!(
            flash
                .as_ref()
                .is_some_and(|f| matches!(f.kind, crate::app::FlashKind::Error)
                    && f.text.starts_with("yproj:")),
            "{flash:?}"
        );
    });
}

/// An error in the project in view's own rc wins over one in another's: it
/// is the one the user is looking at.
#[test]
fn an_error_in_the_project_in_views_rc_wins() {
    with_two(|t| {
        let app = &mut t.app;
        write_rc(&t.x_home, "[layout\n");
        write_rc(&t.y_home, "[layout\n");
        app.reload_config();
        let flash = app.state.flash.clone();
        assert!(
            flash
                .as_ref()
                .is_some_and(|f| matches!(f.kind, crate::app::FlashKind::Error)
                    && f.text.starts_with("config error")),
            "{flash:?}"
        );
    });
}

/// Reloading another project's rc leaves the theme the user is looking at:
/// that project's colours apply when the user switches to it. Even when the
/// project in view's own rc fails, so its reload puts no theme back.
#[test]
fn reloading_another_projects_rc_leaves_the_users_theme() {
    with_two(|t| {
        let app = &mut t.app;
        let before = format!("{:?}", app.view.theme);
        write_rc(&t.x_home, "[layout\n");
        write_rc(&t.y_home, "[colors]\ndir = \"#ff0000\"\n");
        app.reload_config();
        assert_eq!(format!("{:?}", app.view.theme), before, "Y's colours");
    });
}

/// Every open project's rc is a config file: the watcher's candidates name
/// it, and an event on it reloads.
#[test]
fn every_projects_rc_is_a_config_path() {
    with_two(|t| {
        let app = &mut t.app;
        let y_rc = t.y_home.join(".spycrc.toml");
        assert!(
            app.candidate_config_paths().contains(&y_rc),
            "{:?}",
            app.candidate_config_paths()
        );
        assert!(app.is_config_path(&y_rc));
        assert!(app.config_parents().contains(&t.y_home));
    });
}

/// Moving a project's start dir moves the rc it loads, so the watcher is told:
/// the rc in the new start dir is watched, not the old one.
#[test]
fn moving_the_start_dir_tells_the_watcher() {
    with_two(|t| {
        let app = &mut t.app;
        let (tx, rx) = std::sync::mpsc::channel();
        let mut ctx = RunCtx::for_test();
        ctx.watch_tx = Some(tx);
        app.sync_watch(&mut ctx);
        let _primed = rx.try_iter().count();
        let _ = app
            .state
            .dispatch_command(&format!("startdir {}", t.x_home.join("sub").display()));
        app.sync_watch(&mut ctx);
        let told: Vec<Vec<std::path::PathBuf>> = rx
            .try_iter()
            .filter_map(|c| match c {
                crate::app::watch::WatchCommand::SyncConfigParents(parents) => Some(parents),
                crate::app::watch::WatchCommand::SyncListing { .. } => None,
            })
            .collect();
        let sub = t.x_home.join("sub");
        assert!(
            told.last().is_some_and(|p| p.contains(&sub)
                && p.contains(&t.y_home)
                && !p.contains(&t.x_home)),
            "the watcher wasn't told to watch the new start dir and only it: {told:?}"
        );
    });
}

// ── Lua maps, in every project's keymap ──────────────────────────────────────

/// Attach a real Lua worker whose wakes land on the returned channel.
fn attach_lua(app: &mut App) -> std::sync::mpsc::Receiver<Message> {
    let (tx, rx) = std::sync::mpsc::channel();
    app.runtime.pane_wake_tx = Some(tx.clone());
    app.runtime.lua = Some(crate::lua::LuaWorker::spawn(move || {
        let _ = tx.send(Message::Wake(Wake::Lua));
    }));
    rx
}

/// Load `body` as `<config>/init.lua` and land its registrations, through
/// `run` (`reload_init_lua`, `:lua reload`).
fn land_init_lua(
    app: &mut App,
    rx: &std::sync::mpsc::Receiver<Message>,
    config: &std::path::Path,
    run: impl FnOnce(&mut App),
) {
    std::fs::create_dir_all(config).unwrap();
    crate::state::with_config_root(config, || run(app));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let (landed, _) = app.handle_lua_done();
        if landed {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "init.lua never finished"
        );
        let _ = rx.recv_timeout(std::time::Duration::from_millis(50));
    }
}

/// How many `spyc.map` bindings project `id`'s keymap holds.
fn lua_maps_in(app: &mut App, id: state::ProjectId) -> usize {
    app.as_project(id, |app| {
        app.state
            .active_project()
            .user_keymap
            .iter()
            .filter(|b| matches!(&b.action, crate::keymap::BoundAction::Lua(n) if n.starts_with("@map:")))
            .count()
    })
}

const INIT_LUA_MAP: &str = "spyc.map(\"<F5>\", function() end)\n";

/// A `spyc.map` key in `init.lua` binds in every open project's keymap, not
/// only the one in view.
#[test]
fn an_init_lua_map_binds_in_every_open_project() {
    with_two(|t| {
        let app = &mut t.app;
        let x = app.state.active;
        let config = t.x_home.parent().unwrap().join("config");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(config.join("init.lua"), INIT_LUA_MAP).unwrap();
        let rx = attach_lua(app);
        land_init_lua(app, &rx, &config, |app| app.reload_init_lua(false));
        assert_eq!(lua_maps_in(app, x), 1, "X's keymap");
        assert_eq!(lua_maps_in(app, t.y), 1, "Y's keymap");
    });
}

/// `:lua reload` replaces the `spyc.map` bindings rather than adding to them:
/// each project's keymap holds one per map afterwards.
#[test]
fn lua_reload_leaves_one_binding_per_map_in_each_project() {
    with_two(|t| {
        let app = &mut t.app;
        let x = app.state.active;
        let config = t.x_home.parent().unwrap().join("config");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(config.join("init.lua"), INIT_LUA_MAP).unwrap();
        let rx = attach_lua(app);
        land_init_lua(app, &rx, &config, |app| app.reload_init_lua(false));
        land_init_lua(app, &rx, &config, |app| {
            let _ = app.dispatch_command("lua reload");
        });
        assert_eq!(lua_maps_in(app, x), 1, "X's keymap");
        assert_eq!(lua_maps_in(app, t.y), 1, "Y's keymap");
    });
}

/// A reload keeps the colours-off toggle, as a switch does: the rebuilt theme
/// is the rc's colours, in the user's choice of whether to show them.
#[test]
fn a_reload_keeps_colours_off() {
    with_two(|t| {
        let app = &mut t.app;
        app.view.theme.mono = true;
        app.reload_config();
        assert!(app.view.theme.mono, "the reload turned colours back on");
    });
}

/// Serving a project inside another's serving leaves the outer one in the
/// background still.
#[test]
fn a_nested_in_project_leaves_the_outer_one_in_the_background() {
    with_two(|t| {
        let app = &mut t.app;
        let x = app.state.active;
        let still = app.in_project(t.y, |app| {
            app.in_project(x, |_| ());
            app.state.in_background
        });
        assert!(still);
        assert!(!app.state.in_background);
    });
}
