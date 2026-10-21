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
#[ignore = "red: #99 7a, per-project config files"]
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
#[ignore = "red: #99 7a, per-project config files"]
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
#[ignore = "red: #99 7a, per-project config files"]
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
#[ignore = "red: #99 7a, per-project config files"]
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
#[ignore = "red: #99 7a, per-project config files"]
fn a_file_in_the_projects_directory_is_watched() {
    with_three(|t| {
        with_config(t, |t, config| {
            let file = project_file(config, "work", &t.z_home, "");
            assert!(t.app.is_config_path(&file));
        });
    });
}
