//! A pane tab or an editor overlay opened inside a mounted archive. A path in
//! there names no directory, and a pty handed one starts its child in `$HOME`;
//! each starts in the directory holding the archive instead, and outside the
//! archive each still starts where it did.

use super::*;

use crate::app::harness_tests::process_cwd::wait_until_in;
use crate::config::NewTabCwd;

/// A tab's command: it outlives `wait_until_in`'s bound, so a failure reads
/// the directory the child is in rather than its exit.
const TAB: &str = "sleep 60";

fn press(app: &mut App, code: KeyCode) -> Vec<Effect> {
    app.handle_key(KeyEvent::new(code, KeyModifiers::empty()))
        .unwrap()
}

/// What an open prompt offers, replaced by `text` (`^U` first), then `Enter`.
fn submit(app: &mut App, text: &str) {
    app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL))
        .unwrap();
    for c in text.chars() {
        press(app, KeyCode::Char(c));
    }
    press(app, KeyCode::Enter);
}

/// `^a c` with `cmd` at its first prompt. The cwd prompt is left up.
fn ask_for_a_tab(app: &mut App, cmd: &str) {
    app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL))
        .unwrap();
    press(app, KeyCode::Char('c'));
    submit(app, cmd);
}

/// `^a c` with `cmd`, then `Enter` on the cwd it offers, which it hands back.
fn new_tab_where_offered(app: &mut App, cmd: &str) -> String {
    ask_for_a_tab(app, cmd);
    let offered = match &app.state.mode {
        Mode::Prompting(p) if matches!(p.kind, PromptKind::PaneNewTabCwd) => p.buffer.clone(),
        _ => panic!("`^a c` asked for no cwd: {:?}", app.flash_text()),
    };
    press(app, KeyCode::Enter);
    offered
}

/// The newest tab's pid, and the cwd it records.
fn newest_tab(app: &App) -> (u32, PathBuf) {
    let tab = app
        .runtime
        .project()
        .pane_tabs
        .as_ref()
        .and_then(|tabs| tabs.tabs().last())
        .unwrap_or_else(|| panic!("no tab opened: {:?}", app.flash_text()));
    (
        tab.pane.process_id().expect("the tab runs"),
        tab.info.cwd.clone(),
    )
}

/// The editor overlay's pid.
fn overlay_pid(app: &App) -> u32 {
    app.runtime
        .project()
        .top_overlay
        .as_ref()
        .unwrap_or_else(|| panic!("no editor opened: {:?}", app.flash_text()))
        .process_id()
        .expect("the editor runs")
}

/// A canonical temp root holding `pkg.zip` and `work/notes.txt`, with the
/// state dir isolated inside it, and an app whose column is in `pkg.zip/src/`.
fn inside_a_mount(test: impl FnOnce(&Path, &mut App)) {
    let tmp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(tmp.path()).unwrap();
    std::fs::create_dir(root.join("work")).unwrap();
    std::fs::write(root.join("work/notes.txt"), "notes\n").unwrap();
    crate::state::with_state_root(&root.join("state"), || {
        let archive = root.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(root.clone());
        app.state.active_project_mut().home = None;
        mount_inline(&mut app, &archive, &root.join("staging"));
        app.state.focus_on_path(&archive.join("src"));
        app.apply(&Action::EnterOrDisplay).unwrap();
        assert_eq!(app.state.cur().listing.dir, archive.join("src"));
        test(&root, &mut app);
    });
}

/// From inside the archive to `work/`, beside it, with the archive still
/// mounted.
fn beside_a_mount(test: impl FnOnce(&Path, &mut App)) {
    inside_a_mount(|root, app| {
        for _ in 0..2 {
            let fx = app.apply(&Action::Climb).unwrap();
            apply_effects(app, fx);
        }
        app.state.focus_on_path(&root.join("work"));
        app.apply(&Action::EnterOrDisplay).unwrap();
        assert_eq!(app.state.cur().listing.dir, root.join("work"));
        assert!(
            app.state.mounts.contains(&root.join("pkg.zip")),
            "the archive stays mounted"
        );
        test(root, app);
    });
}

/// `^a c` inside an archive offers, starts in and records the directory
/// holding it, under each `[pane] new_tab_cwd`: a project with no home and no
/// repository in an archive leaves each with the column's directory.
#[test]
#[ignore = "red: the cwd offered is the archive's own path, and the pty starts the child in $HOME"]
fn a_new_tab_inside_an_archive_starts_in_the_directory_holding_it() {
    for mode in [
        NewTabCwd::WorktreeRoot,
        NewTabCwd::ProjectHome,
        NewTabCwd::BrowseDir,
    ] {
        inside_a_mount(|root, app| {
            app.state.active_project_mut().config.pane.new_tab_cwd = mode;

            let offered = new_tab_where_offered(app, TAB);

            let (pid, recorded) = newest_tab(app);
            wait_until_in(pid, root);
            assert_eq!(offered, root.display().to_string(), "{mode:?} offered");
            assert_eq!(recorded, root, "{mode:?}: the tab records");
        });
    }
}

/// A cwd cleared from the prompt asks for the default one, which inside an
/// archive is the directory holding it too.
#[test]
#[ignore = "red: the default cwd is the archive's own path, and the pty starts the child in $HOME"]
fn a_new_tab_given_no_cwd_inside_an_archive_starts_beside_it() {
    inside_a_mount(|root, app| {
        ask_for_a_tab(app, TAB);
        submit(app, "");

        let (pid, recorded) = newest_tab(app);
        wait_until_in(pid, root);
        assert_eq!(recorded, root, "the tab records");
    });
}

/// Inside an archive inside another, the directory holding the outer one: the
/// inner one's path is inside the outer.
#[test]
#[ignore = "red: the cwd offered is the inner archive's own path, and the pty starts the child in $HOME"]
fn a_new_tab_inside_a_nested_archive_starts_beside_the_outer_one() {
    let tmp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(tmp.path()).unwrap();
    crate::state::with_state_root(&root.join("state"), || {
        let outer = root.join("outer.zip");
        super::navigation::build_nested_zip(&outer);
        let mut app = App::test_app(root.clone());
        mount_inline(&mut app, &outer, &root.join("staging"));
        let inner = outer.join("bundle.zip");
        app.state.focus_on_path(&inner);
        let fx = app.apply(&Action::EnterOrDisplay).unwrap();
        settle(&mut app, fx);
        assert_eq!(app.state.cur().listing.dir, inner, "{:?}", app.flash_text());

        let offered = new_tab_where_offered(&mut app, TAB);

        let (pid, _) = newest_tab(&app);
        wait_until_in(pid, &root);
        assert_eq!(offered, root.display().to_string());
    });
}

/// With a home, `project_home` starts the tab there, inside an archive as
/// anywhere else.
#[test]
fn a_new_tab_inside_an_archive_still_starts_at_the_project_home() {
    inside_a_mount(|root, app| {
        let home = root.join("work");
        app.state.active_project_mut().home = Some(home.clone());
        app.state.active_project_mut().config.pane.new_tab_cwd = NewTabCwd::ProjectHome;

        let offered = new_tab_where_offered(app, TAB);

        let (pid, _) = newest_tab(app);
        wait_until_in(pid, &home);
        assert_eq!(offered, home.display().to_string());
    });
}

/// Beside a mounted archive, a new tab starts in the column, as it does
/// anywhere.
#[test]
fn a_new_tab_beside_a_mounted_archive_starts_in_the_column() {
    for mode in [NewTabCwd::WorktreeRoot, NewTabCwd::BrowseDir] {
        beside_a_mount(|root, app| {
            app.state.active_project_mut().config.pane.new_tab_cwd = mode;
            let work = root.join("work");

            let offered = new_tab_where_offered(app, TAB);

            let (pid, recorded) = newest_tab(app);
            wait_until_in(pid, &work);
            assert_eq!(offered, work.display().to_string(), "{mode:?} offered");
            assert_eq!(recorded, work, "{mode:?}: the tab records");
        });
    }
}

/// `V` on a member opens the editor on its extracted copy, started in the
/// directory holding the archive, as a tab there would be.
#[test]
#[ignore = "red: the overlay is spawned in the archive's own path, and the pty starts it in $HOME"]
fn an_editor_opened_on_a_member_starts_beside_the_archive() {
    inside_a_mount(|root, app| {
        app.state.focus_on_path(&root.join("pkg.zip/src/main.rs"));

        crate::shell::with_editor("tail -f", || {
            let fx = press(app, KeyCode::Char('V'));
            settle(app, fx);
        });

        wait_until_in(overlay_pid(app), root);
    });
}

/// Beside a mounted archive, `V` starts the editor in the column, as it does
/// anywhere.
#[test]
fn an_editor_beside_a_mounted_archive_starts_in_the_column() {
    beside_a_mount(|root, app| {
        let work = root.join("work");
        app.state.focus_on_path(&work.join("notes.txt"));

        crate::shell::with_editor("tail -f", || press(app, KeyCode::Char('V')));

        wait_until_in(overlay_pid(app), &work);
    });
}
