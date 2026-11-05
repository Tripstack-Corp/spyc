//! The process cwd is the loop's to move (#980): a navigation moves only its
//! column, and whatever runs after it in the same tick names the column's
//! directory itself rather than reading the process cwd.
//!
//! Each test navigates only into a directory of this crate, which outlives
//! the run, and puts the process cwd back when it ends: anything that moves
//! it moves it for every test running beside this one.

use super::*;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Puts the process cwd back where it found it, even when an assertion fails.
struct CwdGuard(Option<PathBuf>);

impl CwdGuard {
    fn save() -> Self {
        Self(std::env::current_dir().ok())
    }
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        if let Some(dir) = &self.0 {
            let _ = std::env::set_current_dir(dir);
        }
    }
}

/// `rel`, a directory of this crate, canonical as a listing holds it.
fn crate_dir(rel: &str) -> PathBuf {
    std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join(rel))
        .unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// An app whose column lists `dir`.
fn app_in(dir: &Path) -> App {
    let mut app = App::test_app(dir.to_path_buf());
    app.state.refresh_listing();
    app
}

fn press(app: &mut App, code: KeyCode) -> Vec<Effect> {
    app.handle_key(KeyEvent::new(code, KeyModifiers::empty()))
        .unwrap()
}

fn type_str(app: &mut App, text: &str) {
    for c in text.chars() {
        press(app, KeyCode::Char(c));
    }
}

/// `Enter` on `dir`'s row.
fn enter(app: &mut App, dir: &Path) {
    app.state.focus_on_path(dir);
    press(app, KeyCode::Enter);
    assert_eq!(
        app.state.cur().listing.dir,
        dir,
        "Enter opened the directory"
    );
}

/// Run `effects` through the loop's executor.
fn run(app: &mut App, effects: Vec<Effect>) {
    use ratatui::{Terminal, TerminalOptions, Viewport, backend::CrosstermBackend, layout::Rect};
    let mut terminal = Terminal::with_options(
        CrosstermBackend::new(std::io::stdout()),
        TerminalOptions {
            viewport: Viewport::Fixed(Rect::new(0, 0, 100, 32)),
        },
    )
    .unwrap();
    let fg = crate::app::ForegroundExec {
        park: std::sync::Arc::default(),
        acked: std::sync::Arc::default(),
        reader_done: std::sync::Arc::default(),
        reader: std::thread::current(),
    };
    app.run_effects(effects, &mut terminal, &fg);
}

/// Wait until process `pid` runs in `dir`. A bound for a broken spawn, not a
/// measure of a working one: a child is in its directory before it execs.
fn wait_until_in(pid: u32, dir: &Path) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let cwd = crate::proc_cwd::cwd_for_pid(pid);
        if cwd.as_deref() == Some(dir) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "the child runs in {cwd:?}, not {}",
            dir.display()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// #980's acceptance: `Enter` on a directory moves the column and leaves the
/// process cwd to the loop's settle, which then owes exactly one move, there.
#[test]
fn entering_a_directory_leaves_the_process_cwd_to_the_settle() {
    let _cwd = CwdGuard::save();
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let target = crate_dir("docs/features");
        let mut app = app_in(&crate_dir("docs"));

        enter(&mut app, &target);

        assert_ne!(
            std::env::current_dir().ok().as_deref(),
            Some(target.as_path()),
            "the navigation moved the process cwd"
        );
        let owed = app.settle_process_cwd();
        assert!(
            matches!(owed.as_slice(), [Effect::SetProcessCwd { dir }] if *dir == target),
            "the settle owes {owed:?}"
        );
    });
}

/// A `!` capture typed after a navigation runs in the column it was typed in.
#[test]
fn a_capture_after_a_navigation_runs_in_the_column() {
    let _cwd = CwdGuard::save();
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let target = crate_dir("docs/config");
        let mut app = app_in(&crate_dir("docs"));
        enter(&mut app, &target);

        press(&mut app, KeyCode::Char('!'));
        type_str(&mut app, "sleep 30");
        press(&mut app, KeyCode::Enter);

        let capture = app
            .runtime
            .pending_capture
            .as_ref()
            .expect("the capture started");
        let pid = capture.host.process_id().expect("the capture runs");
        wait_until_in(pid, &target);
    });
}

/// A `;` command typed after a navigation runs in the column it was typed in.
#[test]
fn a_foreground_command_after_a_navigation_runs_in_the_column() {
    let _cwd = CwdGuard::save();
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let target = crate_dir("src/skill");
        let mut app = app_in(&crate_dir("src"));
        enter(&mut app, &target);

        press(&mut app, KeyCode::Char(';'));
        type_str(&mut app, "sleep 30");
        press(&mut app, KeyCode::Enter);

        let overlay = app
            .runtime
            .project()
            .top_overlay
            .as_ref()
            .expect("the command started");
        let pid = overlay.process_id().expect("the command runs");
        wait_until_in(pid, &target);
    });
}

/// `:cd` with a relative path goes there from the column, whatever the process
/// cwd is.
#[test]
fn a_relative_cd_goes_there_from_the_column() {
    let _cwd = CwdGuard::save();
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = app_in(&crate_dir("src/app"));

        press(&mut app, KeyCode::Char(':'));
        type_str(&mut app, "cd harness_tests");
        let fx = press(&mut app, KeyCode::Enter);
        run(&mut app, fx);

        assert_eq!(
            app.state.cur().listing.dir,
            crate_dir("src/app/harness_tests"),
            "{:?}",
            app.flash_text()
        );
    });
}

/// `spyc -c 'cd <crate>/src' -c 'cd keymap'`: the second `cd` starts where
/// the first one went, though both run in one tick, before the loop moves the
/// process.
#[test]
fn startup_cds_each_start_where_the_last_one_went() {
    let _cwd = CwdGuard::save();
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = app_in(&crate_dir("."));
        let first = format!("cd {}", crate_dir("src").display());
        app.queue_startup_commands(vec![first, ":cd keymap".to_string()]);
        let before = std::env::current_dir().ok();

        let fx = app.settle_startup_commands().expect("startup has settled");
        run(&mut app, fx);

        assert_eq!(
            std::env::current_dir().ok(),
            before,
            "the `cd`s moved the process cwd"
        );
        assert_eq!(
            app.state.cur().listing.dir,
            crate_dir("src/keymap"),
            "{:?}",
            app.flash_text()
        );
    });
}

/// A relative `:cd` naming an archive is mounted from the column too: the
/// screen that holds the `cd` back for the mount looks the path up there.
#[test]
fn a_relative_cd_into_an_archive_mounts_it_from_the_column() {
    let _cwd = CwdGuard::save();
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = app_in(&crate_dir("fuzz/seeds/archive_container"));
        app.state.active_project_mut().config.archive.enable = true;

        press(&mut app, KeyCode::Char(':'));
        type_str(&mut app, "cd plain.zip");
        let fx = press(&mut app, KeyCode::Enter);
        run(&mut app, fx);

        let flash = app.flash_text().unwrap_or_default();
        assert!(
            flash.contains("plain.zip") && !flash.starts_with("cd:"),
            "the `cd` wasn't held for a mount: {flash:?}"
        );
    });
}

/// `J` after a navigation jumps from the column it was typed in.
#[test]
fn a_relative_jump_after_a_navigation_goes_there_from_the_column() {
    let _cwd = CwdGuard::save();
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let from = crate_dir("src/ui");
        let mut app = app_in(&crate_dir("src"));
        enter(&mut app, &from);

        press(&mut app, KeyCode::Char('J'));
        type_str(&mut app, "agent_hud");
        press(&mut app, KeyCode::Enter);

        assert_eq!(
            app.state.cur().listing.dir,
            crate_dir("src/ui/agent_hud"),
            "{:?}",
            app.flash_text()
        );
    });
}

/// A relative path `J` recorded goes there from the column when picked from
/// the jump history.
#[test]
fn a_relative_jump_history_entry_goes_there_from_the_column() {
    let _cwd = CwdGuard::save();
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = app_in(&crate_dir("src/app"));
        app.state.jump_history.push("mouse");

        press(&mut app, KeyCode::Char('J'));
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Char(' '));
        assert!(
            app.view.pending_jump_history.is_some(),
            "`Space` in the `J` prompt opened no jump history"
        );
        press(&mut app, KeyCode::Enter);

        assert_eq!(
            app.state.cur().listing.dir,
            crate_dir("src/app/mouse"),
            "{:?}",
            app.flash_text()
        );
    });
}
