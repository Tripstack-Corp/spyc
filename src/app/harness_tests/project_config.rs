//! #99 step 7a: a project's own config file under
//! `$XDG_CONFIG_HOME/spyc/projects/`, matched by its `home` key, trusted as
//! `~/.spycrc.toml` is, layered over the repo's own `.spycrc.toml`, and naming
//! the project.
//!
//! The fixtures are `projects_open.rs`'s, with spyc's config directory pinned
//! to a temp dir: `x` is where the user starts, `y` is open beside it, and
//! `zproj/` is a directory no project has yet.

use std::path::{Path, PathBuf};

use super::projects_open::{Three, leader, open, with_three};
use super::*;

/// Run `test` with spyc's config directory at `<home>/config`, and return it.
fn with_config(t: &mut Three, test: impl FnOnce(&mut Three, &Path)) {
    let config = t.home.join("config");
    std::fs::create_dir_all(config.join("projects")).unwrap();
    crate::state::with_config_root(&config, || test(t, &config));
}

/// Write `projects/<name>.toml` naming `home`, then `body`.
fn project_file(config: &Path, name: &str, home: &Path, body: &str) -> PathBuf {
    let path = config.join("projects").join(format!("{name}.toml"));
    std::fs::write(
        &path,
        format!("home = {:?}\n{body}", home.display().to_string()),
    )
    .unwrap();
    path
}

fn tab_commands(app: &App) -> Vec<String> {
    app.runtime
        .project()
        .pane_tabs
        .as_ref()
        .map(|tabs| tabs.tabs().iter().map(|t| t.info.command.clone()).collect())
        .unwrap_or_default()
}

fn asks_consent(app: &App) -> bool {
    matches!(
        &app.state.mode,
        Mode::Prompting(p) if matches!(p.kind, PromptKind::ProjectTabsConsent { .. })
    )
}

/// The control: with no file for it, a project is named for its directory,
/// and a repo's own startup tabs still ask first, whatever files name other
/// homes.
#[test]
fn a_project_with_no_file_reads_as_before() {
    with_three(|t| {
        with_config(t, |t, config| {
            project_file(config, "other", &t.y_home, "[pane]\ntabs = [\"cat\"]\n");
            std::fs::write(t.z_home.join(".spycrc.toml"), "[pane]\ntabs = [\"cat\"]\n").unwrap();
            open(&mut t.app, &t.z_home);
            assert_eq!(t.app.state.active_project().label(), "zproj");
            assert!(asks_consent(&t.app), "the repo's tabs ran without asking");
        });
    });
}

/// `Space n` on a directory a file names: the file's startup tabs start
/// without asking, over the repo's own, its executing binding is kept, and
/// the project is named for the file.
#[test]
fn a_project_file_configures_the_project_it_names() {
    with_three(|t| {
        with_config(t, |t, config| {
            project_file(
                config,
                "work",
                &t.z_home,
                "keymap = [\"map j unix make %\"]\n[pane]\ntabs = [\"cat\"]\n",
            );
            std::fs::write(t.z_home.join(".spycrc.toml"), "[pane]\ntabs = [\"sh\"]\n").unwrap();
            open(&mut t.app, &t.z_home);
            let app = &t.app;
            assert!(!asks_consent(app), "the user's own file asked for consent");
            assert_eq!(
                tab_commands(app),
                ["cat"],
                "the file's tabs, not the repo's"
            );
            assert_eq!(app.state.active_project().label(), "work");
            let bound = app
                .state
                .active_project()
                .config
                .bindings
                .iter()
                .any(|b| b.action.is_executing());
            assert!(bound, "the file's executing binding was dropped");
        });
    });
}

/// Two files naming one home: the first by name applies, and the flash names
/// both.
#[test]
fn the_first_of_two_files_naming_a_home_applies() {
    with_three(|t| {
        with_config(t, |t, config| {
            project_file(config, "b", &t.z_home, "");
            project_file(config, "a", &t.z_home, "");
            open(&mut t.app, &t.z_home);
            assert_eq!(t.app.state.active_project().label(), "a");
            let flash = t.app.flash_text().unwrap_or_default().to_owned();
            assert!(
                flash.contains("a.toml") && flash.contains("b.toml"),
                "{flash}"
            );
        });
    });
}

/// Re-homed onto a directory a file names, a reload reads that file.
#[test]
fn a_reload_reads_the_file_for_the_projects_new_home() {
    with_three(|t| {
        with_config(t, |t, config| {
            let w_home = t.home.parent().unwrap().join("wproj");
            std::fs::create_dir_all(&w_home).unwrap();
            project_file(config, "work", &w_home, "");
            let app = &mut t.app;
            let listing = crate::fs::listing::Listing::read(&w_home).expect("w lists");
            app.state.active_project_mut().left.listing = listing;
            app.state.rebuild_rows();
            leader(app, 'P');
            assert_eq!(app.state.active_project().home.as_deref(), Some(&*w_home));
            app.reload_config();
            assert_eq!(app.state.active_project().label(), "work");
        });
    });
}

/// A `home` key means something only in a file under `projects/`: in
/// `~/.spycrc.toml` it is a warning, not a load error.
#[test]
fn a_home_key_in_the_users_rc_is_a_warning() {
    with_three(|t| {
        std::fs::write(t.home.join(".spycrc.toml"), "home = \"/elsewhere\"\n").unwrap();
        let cfg = crate::config::Config::load_default(&t.z_home).expect("the rc loads");
        assert!(
            cfg.warnings.iter().any(|w| w.contains("home")),
            "{:?}",
            cfg.warnings
        );
    });
}

/// A file in the projects directory is a config file: editing one reloads.
#[test]
fn a_file_in_the_projects_directory_is_watched() {
    with_three(|t| {
        with_config(t, |t, config| {
            let file = project_file(config, "work", &t.z_home, "");
            assert!(t.app.is_config_path(&file));
        });
    });
}

// ── The fix: reading again when a project moves ──────────────────────────────

/// `wproj/`, a directory no project has, with `projects/work.toml` naming it
/// and holding `body`; then column `a` of the project in view there, and its
/// config's inputs recorded as a launch records them.
fn w_with_file(t: &mut Three, config: &Path, body: &str) -> PathBuf {
    let w_home = t.home.parent().unwrap().join("wproj");
    std::fs::create_dir_all(&w_home).unwrap();
    project_file(config, "work", &w_home, body);
    let app = &mut t.app;
    let inputs = app.state.active_project().config_inputs();
    app.state.active_project_mut().config_key = Some(inputs);
    let listing = crate::fs::listing::Listing::read(&w_home).expect("w lists");
    app.state.active_project_mut().left.listing = listing;
    app.state.rebuild_rows();
    w_home
}

/// Re-homed onto a directory a file names, the next settle reads that file,
/// no `^R` needed, and once.
#[test]
fn a_re_home_reads_its_file_at_the_next_settle() {
    with_three(|t| {
        with_config(t, |t, config| {
            w_with_file(t, config, "");
            let app = &mut t.app;
            assert!(!app.settle_project_configs(), "nothing has moved yet");
            leader(app, 'P');
            assert!(app.settle_project_configs());
            assert_eq!(app.state.active_project().label(), "work");
            assert!(!app.settle_project_configs(), "read again");
        });
    });
}

/// A read that fails flashes, and isn't tried again on every loop: the
/// project keeps the config it had.
#[test]
fn a_failed_read_is_not_repeated() {
    with_three(|t| {
        with_config(t, |t, config| {
            w_with_file(t, config, "[layout]\nno_such_key = 1\n");
            let app = &mut t.app;
            leader(app, 'P');
            assert!(app.settle_project_configs());
            let flash = app.flash_text().unwrap_or_default().to_owned();
            assert!(flash.starts_with("config error: "), "{flash}");
            assert_eq!(app.state.active_project().label(), "wproj");
            assert!(!app.settle_project_configs(), "read again");
        });
    });
}

/// The settle's read keeps `init.lua`'s `spyc.map` keys, which a keymap
/// rebuilt from the files alone would drop.
#[test]
fn a_re_home_keeps_the_lua_maps() {
    with_three(|t| {
        with_config(t, |t, config| {
            w_with_file(t, config, "");
            let app = &mut t.app;
            let chord = crate::config::dsl::parse_key("<F5>").expect("a chord");
            app.runtime.lua_registry.maps.push((1, chord));
            leader(app, 'P');
            assert!(app.settle_project_configs());
            let maps = app
                .state
                .active_project()
                .user_keymap
                .iter()
                .filter(|b| matches!(&b.action, crate::keymap::BoundAction::Lua(n) if n.starts_with("@map:")))
                .count();
            assert_eq!(maps, 1);
        });
    });
}

/// A project whose config was never read from files is never read by the
/// settle.
#[test]
fn a_project_never_read_from_files_is_left_alone() {
    with_three(|t| {
        with_config(t, |t, config| {
            w_with_file(t, config, "");
            let app = &mut t.app;
            app.state.active_project_mut().config_key = None;
            leader(app, 'P');
            assert!(!app.settle_project_configs());
            assert_eq!(app.state.active_project().label(), "wproj");
        });
    });
}

/// With one project open and a file for it, the bar and the title use the
/// file's name.
#[test]
fn one_project_with_a_file_is_named_for_it() {
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
        app.state.session_name = Some("SAGE".into());
        app.state.active_project_mut().config.project =
            Some(crate::config::project_file::ProjectFile {
                path: base.join("work.toml"),
                label: "work".into(),
            });
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 20)).unwrap();
        terminal.draw(|f| app.render(f)).unwrap();
        let buf = terminal.backend().buffer();
        let bar: String = (0..120)
            .map(|x| buf.cell((x, 0)).map_or(" ", |c| c.symbol()))
            .collect();
        assert!(bar.contains(" work "), "{bar}");
        app.view.last_term_title = None;
        let title = match app.term_title_effect() {
            Some(Effect::SetTerminalTitle { title }) => title,
            other => panic!("no title: {other:?}"),
        };
        assert_eq!(title, "\u{1f336}\u{fe0f}: work \u{b7} SAGE");
    });
}

/// The projects directory is watched even before it exists, and its
/// appearing (an event in spyc's config directory naming it) resends the
/// config dirs to the watch worker, which watches it then, and reads what is
/// in it.
#[test]
fn the_projects_directory_appearing_is_watched_and_read() {
    with_three(|t| {
        let config = t.home.join("later");
        std::fs::create_dir_all(&config).unwrap();
        crate::state::with_config_root(&config, || {
            let projects = config.join("projects");
            let app = &mut t.app;
            assert!(app.config_parents().contains(&projects));
            std::fs::create_dir_all(&projects).unwrap();
            project_file(&config, "work", app.state.active_project().anchor(), "");
            let mut ctx = RunCtx::for_test();
            ctx.watched_config = app.config_parents();
            ctx.fs_pending.push(
                notify::Event::new(notify::EventKind::Create(notify::event::CreateKind::Folder))
                    .add_path(projects),
            );
            app.ingest_fs_and_maybe_refresh(std::time::Instant::now(), &mut ctx);
            assert!(
                ctx.watched_config.is_empty(),
                "the config dirs weren't resent"
            );
            assert_eq!(app.state.active_project().label(), "work");
        });
    });
}

/// `gS` moves the start directory, and so the repo rc read: the next settle
/// reads the new one's.
#[test]
fn a_moved_start_dir_reads_its_rc_at_the_next_settle() {
    with_three(|t| {
        with_config(t, |t, config| {
            let w_home = w_with_file(t, config, "");
            std::fs::write(w_home.join(".spycrc.toml"), "[colors]\ndir = \"#123456\"\n").unwrap();
            let app = &mut t.app;
            app.handle_key(key('g')).unwrap();
            app.handle_key(key('S')).unwrap();
            assert_eq!(app.state.active_project().start_dir, w_home);
            assert!(app.settle_project_configs());
            let dir = app.state.active_project().config.colors.dir.clone();
            assert_eq!(dir.as_deref(), Some("#123456"));
        });
    });
}

/// Through a symlinked config directory, a file in the projects directory is
/// still a config file.
#[test]
fn a_linked_projects_directory_is_still_watched() {
    with_three(|t| {
        let real = t.home.join("real");
        std::fs::create_dir_all(real.join("projects")).unwrap();
        let link = t.home.join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let file = real.join("projects").join("work.toml");
        std::fs::write(&file, "").unwrap();
        crate::state::with_config_root(&link, || assert!(t.app.is_config_path(&file)));
    });
}

/// A project out of view that moves onto a file that doesn't load: the
/// settle's flash names it.
#[test]
fn a_moved_project_out_of_view_names_its_problem() {
    with_three(|t| {
        with_config(t, |t, config| {
            let w_home = t.home.parent().unwrap().join("wproj");
            std::fs::create_dir_all(&w_home).unwrap();
            project_file(config, "work", &w_home, "[layout]\nno_such_key = 1\n");
            let (app, y) = (&mut t.app, t.y);
            app.as_project(y, |app| {
                let inputs = app.state.active_project().config_inputs();
                app.state.active_project_mut().config_key = Some(inputs);
                app.state.set_project_home(w_home.clone());
            });
            assert!(app.settle_project_configs());
            let flash = app.flash_text().unwrap_or_default().to_owned();
            assert!(flash.starts_with("wproj: config error: "), "{flash}");
        });
    });
}

/// A project moving keeps what the user chose for the agent strip, though the
/// file it moved onto sets another: only an edit or `^R` is the newer word.
#[test]
fn a_move_keeps_the_agent_hud_choice() {
    with_three(|t| {
        with_config(t, |t, config| {
            w_with_file(t, config, "[pane]\nagent_hud = \"full\"\n");
            let app = &mut t.app;
            let hud = &mut app.state.active_project_mut().pane.agent_hud;
            hud.choice = Some(crate::config::AgentHudMode::Compact);
            leader(app, 'P');
            assert!(app.settle_project_configs());
            let choice = app.state.active_project().pane.agent_hud.choice;
            assert_eq!(choice, Some(crate::config::AgentHudMode::Compact));
        });
    });
}

/// A move leaves the columns' mask toggles alone: with no file for the new
/// home nothing is read, and with one that sets the same masks the toggles
/// stay.
#[test]
fn a_move_keeps_the_mask_toggles() {
    with_three(|t| {
        with_config(t, |t, config| {
            let w_home = w_with_file(t, config, "[colors]\ndir = \"#123456\"\n");
            let app = &mut t.app;
            let shown = |app: &App| app.state.active_project().left.masks.hides(".hidden");
            let before = shown(app);
            app.state.active_project_mut().left.masks.toggle_mask1();
            leader(app, 'P');
            assert!(app.settle_project_configs(), "the file is read");
            assert_eq!(app.state.active_project().home.as_deref(), Some(&*w_home));
            assert_ne!(shown(app), before, "the toggle went back");
        });
    });
}

/// A move onto a home no file names, the start directory where it was, reads
/// nothing: the same two files would be read. Only the key moves.
#[test]
fn a_move_onto_a_home_no_file_names_reads_nothing() {
    with_three(|t| {
        with_config(t, |t, config| {
            let w_home = w_with_file(t, config, "");
            std::fs::remove_file(config.join("projects").join("work.toml")).unwrap();
            let app = &mut t.app;
            leader(app, 'P');
            assert_eq!(app.state.active_project().home.as_deref(), Some(&*w_home));
            assert!(
                !app.settle_project_configs(),
                "it read the same files again"
            );
            let key = app.state.active_project().config_key.clone();
            assert_eq!(key, Some(app.state.active_project().config_inputs()));
        });
    });
}
