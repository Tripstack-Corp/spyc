//! #99 step 5e, pair 2: opening a project, and moving between projects, from
//! the leader (`Space n`, `Space Space`, `Space 1-9`, `Space ]` / `Space [`).
//! Closing one (`Space x`) is `projects_close.rs`'s, which shares these
//! fixtures.
//!
//! `x` is where the user starts, `y` is open beside it, and `zproj/` is a
//! directory no project has yet. `$HOME` is a temp dir of its own, so no test
//! reads the developer's `~/.spycrc.toml`.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::*;
use crossterm::event::KeyCode;

pub(super) struct Three {
    pub(super) app: App,
    pub(super) x: state::ProjectId,
    pub(super) y: state::ProjectId,
    pub(super) home: PathBuf,
    pub(super) x_home: PathBuf,
    pub(super) y_home: PathBuf,
    pub(super) z_home: PathBuf,
}

pub(super) fn with_three(test: impl FnOnce(&mut Three)) {
    let tmp = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(tmp.path()).unwrap();
    let [state_dir, home, x_home, y_home, z_home] =
        ["state", "home", "xproj", "yproj", "zproj"].map(|d| base.join(d));
    for d in [&state_dir, &home, &x_home, &y_home, &z_home] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(z_home.join("in_z.txt"), "z\n").unwrap();
    crate::state::with_state_root(&state_dir, || {
        crate::config::with_home_root(&home, || {
            let mut app = App::test_app(x_home.clone());
            let x = app.state.active;
            app.ready_test_project(x, &x_home);
            let y = app.add_test_project(&y_home);
            test(&mut Three {
                app,
                x,
                y,
                home: home.clone(),
                x_home,
                y_home,
                z_home,
            });
        });
    });
}

pub(super) fn leader(app: &mut App, c: char) {
    app.handle_key(key(' ')).unwrap();
    app.handle_key(key(c)).unwrap();
}

/// `Space n`, then `dir` in the prompt it opens, then Enter.
pub(super) fn open(app: &mut App, dir: &Path) {
    leader(app, 'n');
    let Mode::Prompting(prompt) = &mut app.state.mode else {
        panic!("Space n opened no prompt: {:?}", app.flash_text());
    };
    prompt.buffer = dir.display().to_string();
    let typed = prompt.buffer.clone();
    if let Some(editor) = prompt.editor.as_mut() {
        editor.set_content(&typed);
    }
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();
}

fn project_at(app: &App, home: &Path) -> Option<state::ProjectId> {
    app.state
        .open_projects()
        .find(|p| p.home.as_deref() == Some(home))
        .map(|p| p.id)
}

/// Land the file ops in flight (the arriving project's columns are re-read on
/// the worker), for at most five seconds.
fn land_relist(app: &mut App, done: impl Fn(&App) -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done(app) {
        if Instant::now() >= deadline {
            return false;
        }
        app.apply_file_outcomes();
        std::thread::sleep(Duration::from_millis(10));
    }
    true
}

// ── Opening ──────────────────────────────────────────────────────────────────

/// `Space n` opens a directory as a project: a third one, homed there, the one
/// in view, its column listing the directory once the worker has read it, and
/// with its context file written for its agents.
#[test]
fn space_n_opens_a_directory_as_a_project() {
    with_three(|t| {
        let app = &mut t.app;
        open(app, &t.z_home);
        assert_eq!(app.state.open_projects().count(), 3);
        let z = project_at(app, &t.z_home).expect("a project homed at zproj");
        assert_eq!(app.state.active, z, "the new project isn't in view");
        assert!(
            land_relist(app, |app| app
                .state
                .cur()
                .rows
                .iter()
                .any(|r| r.display == "in_z.txt")),
            "its column never listed zproj: {:?}",
            app.state.cur().listing.dir
        );
        assert!(
            app.view.project().context_path.exists(),
            "its agents have no context file"
        );
    });
}

/// **`Space n` onto a directory whose `.spycrc.toml` can't load** (#827) opens
/// it on the user's own settings, not the built-in defaults, and the flash
/// says the repo rc was skipped.
#[test]
fn space_n_onto_a_broken_repo_rc_opens_on_the_users_own_settings() {
    with_three(|t| {
        std::fs::write(t.home.join(".spycrc.toml"), "[pager]\ntab_width = 2\n").unwrap();
        std::fs::write(t.z_home.join(".spycrc.toml"), "[layout\n").unwrap();
        let app = &mut t.app;
        open(app, &t.z_home);
        assert_eq!(
            app.state.active,
            project_at(app, &t.z_home).expect("a project homed at zproj")
        );
        assert_eq!(
            app.state.active_project().config.pager.tab_width,
            2,
            "the user's own setting"
        );
        let flash = app.flash_text().unwrap_or_default().to_owned();
        assert!(flash.contains("skipped"), "{flash}");
    });
}

/// `Space n` onto a directory that is already an open project's home goes to
/// that project rather than opening a second one there.
#[test]
fn space_n_onto_an_open_projects_home_goes_to_it() {
    with_three(|t| {
        let app = &mut t.app;
        open(app, &t.y_home);
        assert_eq!(app.state.open_projects().count(), 2, "a second yproj");
        assert_eq!(app.state.active, t.y);
    });
}

/// `Space n` onto a directory another open project started in goes to that
/// project, though its home has moved: one directory is one project's.
#[test]
fn space_n_onto_an_open_projects_start_dir_goes_to_it() {
    with_three(|t| {
        let elsewhere = t.home.join("elsewhere");
        std::fs::create_dir(&elsewhere).unwrap();
        let y = t.y;
        t.app.as_project(y, |app| {
            app.state.active_project_mut().home = Some(elsewhere);
        });
        let app = &mut t.app;
        open(app, &t.y_home);
        assert_eq!(app.state.open_projects().count(), 2, "a second yproj");
        assert_eq!(app.state.active, t.y);
    });
}

/// A project homed through a symlink is the project at the directory the link
/// names, so `Space n` there goes to it rather than opening a second.
#[test]
fn space_n_onto_a_linked_home_goes_to_its_project() {
    with_three(|t| {
        let link = t.home.join("zlink");
        std::os::unix::fs::symlink(&t.z_home, &link).unwrap();
        let z = t.app.add_test_project(&link);
        let app = &mut t.app;
        open(app, &t.z_home);
        assert_eq!(
            app.state.open_projects().count(),
            3,
            "a second project at zproj"
        );
        assert_eq!(app.state.active, z);
    });
}

/// A relative path is read against the column the user is in, as `J` and
/// Tab completion read one, not against spyc's process cwd.
#[test]
fn a_relative_path_opens_against_the_column() {
    with_three(|t| {
        let app = &mut t.app;
        let parent = t.z_home.parent().expect("zproj has a parent");
        app.state.cur_mut().listing =
            crate::fs::listing::Listing::read(parent).expect("the parent lists");
        open(app, Path::new("zproj"));
        assert_eq!(
            project_at(app, &t.z_home),
            Some(app.state.active),
            "{:?}",
            app.flash_text()
        );
    });
}

/// Enter on the prompt's pre-fill, the project in view's own directory, says
/// so rather than doing nothing.
#[test]
fn space_n_onto_the_project_in_view_says_so() {
    with_three(|t| {
        let app = &mut t.app;
        leader(app, 'n');
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.state.active, t.x);
        assert_eq!(app.state.open_projects().count(), 2);
        assert!(
            app.flash_text()
                .is_some_and(|f| f.contains("already in project xproj")),
            "{:?}",
            app.flash_text()
        );
    });
}

/// The sidecar discovery reads lists the new project's root beside the
/// others', so its agents' `spyc --mcp` proxies find this spyc.
#[test]
fn space_n_records_the_new_projects_root_for_discovery() {
    with_three(|t| {
        let app = &mut t.app;
        app.view.mcp_running = true;
        open(app, &t.z_home);
        let state_dir = crate::state::state_root().expect("the test's state root");
        let sidecar = crate::mcp::root_marker_path_in(&state_dir, std::process::id());
        let recorded = std::fs::read_to_string(sidecar).unwrap_or_default();
        let mut roots: Vec<&str> = recorded.lines().collect();
        roots.sort_unstable();
        let mut want = [&t.x_home, &t.y_home, &t.z_home].map(|p| p.to_str().unwrap());
        want.sort_unstable();
        assert_eq!(roots, want);
    });
}

/// A project `Space n` opens starts the `[pane] tabs` of `~/.spycrc.toml`, as a
/// launch there would.
#[test]
fn space_n_starts_the_new_projects_startup_tabs() {
    with_three(|t| {
        std::fs::write(t.home.join(".spycrc.toml"), "[pane]\ntabs = [\"cat\"]\n").unwrap();
        let app = &mut t.app;
        open(app, &t.z_home);
        let tabs: Vec<String> = app
            .runtime
            .project()
            .pane_tabs
            .as_ref()
            .map(|tabs| tabs.tabs().iter().map(|t| t.info.command.clone()).collect())
            .unwrap_or_default();
        assert_eq!(tabs, ["cat"]);
    });
}

/// A repo's own `[pane] tabs` ask before they run, as at a launch.
#[test]
fn a_repos_startup_tabs_ask_first() {
    with_three(|t| {
        std::fs::write(t.z_home.join(".spycrc.toml"), "[pane]\ntabs = [\"cat\"]\n").unwrap();
        let app = &mut t.app;
        open(app, &t.z_home);
        assert!(
            matches!(
                &app.state.mode,
                Mode::Prompting(p) if matches!(p.kind, PromptKind::ProjectTabsConsent { .. })
            ),
            "the repo's tabs ran without asking"
        );
        assert!(app.runtime.project().pane_tabs.is_none(), "a tab opened");
    });
}

/// An `init.lua` `spyc.map` key binds in a project opened after the load.
#[test]
fn a_lua_map_binds_in_a_project_opened_later() {
    with_three(|t| {
        let app = &mut t.app;
        let config = t.home.join("config");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(
            config.join("init.lua"),
            "spyc.map(\"<F5>\", function() end)\n",
        )
        .unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        app.runtime.pane_wake_tx = Some(tx.clone());
        app.runtime.lua = Some(crate::lua::LuaWorker::spawn(move || {
            let _ = tx.send(Message::Wake(Wake::Lua));
        }));
        crate::state::with_config_root(&config, || app.reload_init_lua(false));
        let deadline = Instant::now() + Duration::from_secs(10);
        while !app.handle_lua_done().0 {
            assert!(Instant::now() < deadline, "init.lua never finished");
            let _ = rx.recv_timeout(Duration::from_millis(50));
        }
        open(app, &t.z_home);
        let maps = app
            .state
            .active_project()
            .user_keymap
            .iter()
            .filter(|b| matches!(&b.action, crate::keymap::BoundAction::Lua(n) if n.starts_with("@map:")))
            .count();
        assert_eq!(maps, 1, "the new project's keymap");
    });
}

// ── Moving between projects ──────────────────────────────────────────────────

/// `Space 2` goes to the second project, and `Space Space` back to the one
/// the user was in last.
#[test]
fn space_digit_and_space_space_move_between_projects() {
    with_three(|t| {
        let app = &mut t.app;
        leader(app, '2');
        assert_eq!(app.state.active, t.y, "Space 2");
        leader(app, ' ');
        assert_eq!(app.state.active, t.x, "Space Space");
        leader(app, ' ');
        assert_eq!(app.state.active, t.y, "Space Space again");
    });
}

/// A digit past the open projects says so, and goes nowhere.
#[test]
fn space_digit_past_the_projects_says_so() {
    with_three(|t| {
        let app = &mut t.app;
        leader(app, '7');
        assert_eq!(app.state.active, t.x);
        assert!(
            app.flash_text().is_some_and(|f| f.contains("no project 7")),
            "{:?}",
            app.flash_text()
        );
    });
}

/// `Space ]` and `Space [` step through the projects in order, wrapping.
#[test]
fn space_brackets_step_through_projects_and_wrap() {
    with_three(|t| {
        let app = &mut t.app;
        leader(app, ']');
        assert_eq!(app.state.active, t.y, "Space ]");
        leader(app, ']');
        assert_eq!(app.state.active, t.x, "Space ] wraps");
        leader(app, '[');
        assert_eq!(app.state.active, t.y, "Space [ wraps back");
    });
}

/// With one project open, moving between projects does nothing, and says
/// nothing: a user who never opens a second project sees no change.
#[test]
fn with_one_project_the_moves_do_nothing_and_say_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let home = std::fs::canonicalize(tmp.path()).unwrap();
    crate::state::with_state_root(&home.join("state"), || {
        let mut app = App::test_app(home.clone());
        let only = app.state.active;
        for c in [' ', ']', '[', '1', '2'] {
            leader(&mut app, c);
            assert_eq!(app.state.active, only, "Space {c:?}");
            assert_eq!(app.flash_text(), None, "Space {c:?}");
        }
    });
}

/// An agent's call, served inside its project, is not the user moving: where
/// `Space Space` goes stays as it was.
#[test]
fn serving_another_project_leaves_the_last_project_alone() {
    with_three(|t| {
        let y = t.y;
        t.app.as_project(y, |_| {});
        assert_eq!(t.app.state.last_project, None);
    });
}

/// The leader's other keys still resolve as before.
#[test]
fn the_leaders_other_keys_still_resolve() {
    let mut resolver = crate::keymap::Resolver::new();
    let keymap = crate::keymap::UserKeymap::default();
    let name = |outcome: crate::keymap::ResolverOutcome| match outcome {
        crate::keymap::ResolverOutcome::Action(a) => Some(a.canonical_name()),
        _ => None,
    };
    let _ = resolver.feed(key(' '), &keymap);
    assert_eq!(
        name(resolver.feed(key('p'), &keymap)),
        Some("jump_project_home")
    );
    let _ = resolver.feed(key(' '), &keymap);
    assert_eq!(
        name(resolver.feed(key('P'), &keymap)),
        Some("set_project_home_here")
    );
    let _ = resolver.feed(key(' '), &keymap);
    let _ = resolver.feed(key('w'), &keymap);
    assert_eq!(
        name(resolver.feed(key('l'), &keymap)),
        Some("worktree_list")
    );
}

/// A repo opened as a project shows its branch, with its repo root resolved
/// for the git walk that the switch to it runs.
#[test]
fn a_repo_opened_as_a_project_shows_its_branch() {
    with_three(|t| {
        let run_git = crate::git::test_support::run_git;
        run_git(&t.z_home, &["init", "-q", "--initial-branch=trunk"]);
        run_git(&t.z_home, &["add", "in_z.txt"]);
        run_git(&t.z_home, &["commit", "-q", "-m", "z"]);
        let app = &mut t.app;
        open(app, &t.z_home);
        assert!(
            app.state
                .cur()
                .git
                .info
                .as_deref()
                .is_some_and(|i| i.contains("trunk")),
            "{:?}",
            app.state.cur().git.info
        );
        assert_eq!(
            app.state.cur().git_cache.current_repo_root.as_deref(),
            Some(t.z_home.as_path())
        );
    });
}
