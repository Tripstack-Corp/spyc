//! `--cwd-file` (#63): a quit writes the directory the file list shows, for a
//! shell wrapper to `cd` into once spyc has exited.
//!
//! Every case quits through the real entry points, `Q` and `:q`, each tapped
//! twice as the confirm asks. Nothing here navigates by `chdir`: the columns
//! are opened where the test wants them, so the process cwd stays put.

use std::path::{Path, PathBuf};

use super::archive::{build_zip, mount_inline};
use super::*;

/// A canonical temp root holding `a/` and `b/`, with the state dir isolated
/// inside it.
fn with_root(test: impl FnOnce(&Path)) {
    let tmp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(tmp.path()).unwrap();
    for d in ["a", "b", "state"] {
        std::fs::create_dir_all(root.join(d)).unwrap();
    }
    crate::state::with_state_root(&root.join("state"), || test(&root));
}

fn quit_with_q(app: &mut App) {
    app.handle_key(key('Q')).unwrap();
    app.handle_key(key('Q')).unwrap();
    assert!(app.state.should_quit, "two taps of `Q` quit");
}

fn quit_with_colon_q(app: &mut App) {
    app.dispatch_command("q");
    app.dispatch_command("q");
    assert!(app.state.should_quit, "two `:q`s quit");
}

/// The cwd file's bytes: the path alone, with no newline for a wrapper's
/// `cd "$(cat …)"` to trip on.
fn written(file: &Path) -> Vec<u8> {
    std::fs::read(file).unwrap_or_else(|e| panic!("{} was not written: {e}", file.display()))
}

fn bytes(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt as _;
    path.as_os_str().as_bytes().to_vec()
}

/// `Q` with `b` focused writes `b`'s directory, and only once the second tap
/// confirms the quit.
#[test]
fn quitting_with_q_writes_the_focused_column_directory() {
    with_root(|root| {
        let cwd_file = root.join("cwd");
        let mut app = App::test_app(root.join("a"));
        app.set_cwd_file(Some(cwd_file.clone()));
        app.open_second_commander_at(&root.join("b"));

        app.handle_key(key('Q')).unwrap();
        assert!(
            !app.state.should_quit,
            "the first tap only arms the confirm"
        );
        assert!(!cwd_file.exists(), "nothing is written before the quit");

        app.handle_key(key('Q')).unwrap();
        assert!(app.state.should_quit, "the second tap quits");
        assert_eq!(written(&cwd_file), bytes(&root.join("b")));
    });
}

/// `:q` takes the same path, and with `a` focused it is `a`'s directory that
/// is written, though `b` is still open beside it.
#[test]
fn quitting_with_colon_q_writes_column_a_when_it_has_focus() {
    with_root(|root| {
        let cwd_file = root.join("cwd");
        let mut app = App::test_app(root.join("a"));
        app.set_cwd_file(Some(cwd_file.clone()));
        app.open_second_commander_at(&root.join("b"));
        app.apply(&Action::VsplitFocusLeft).unwrap();

        quit_with_colon_q(&mut app);

        assert_eq!(written(&cwd_file), bytes(&root.join("a")));
    });
}

/// A wrapper usually hands spyc a file `mktemp` already made. The quit
/// replaces it whole, by a rename, so a reader sees the old file or the new
/// one and never a half-written path, and no byte of the old contents is left.
#[test]
fn the_cwd_file_is_replaced_whole_never_rewritten_in_place() {
    use std::os::unix::fs::MetadataExt as _;
    with_root(|root| {
        let cwd_file = root.join("cwd");
        std::fs::write(&cwd_file, "/a/stale/path/longer/than/the/one/written\n").unwrap();
        let before = std::fs::metadata(&cwd_file).unwrap().ino();
        let mut app = App::test_app(root.join("a"));
        app.set_cwd_file(Some(cwd_file.clone()));

        quit_with_q(&mut app);

        assert_eq!(written(&cwd_file), bytes(&root.join("a")));
        assert_ne!(
            std::fs::metadata(&cwd_file).unwrap().ino(),
            before,
            "the file was rewritten in place, so a reader could catch it half-written"
        );
    });
}

/// With two projects open, the one in view is the one whose directory is
/// written.
#[test]
fn quitting_writes_the_directory_of_the_project_in_view() {
    with_root(|root| {
        let cwd_file = root.join("cwd");
        let mut app = App::test_app(root.join("a"));
        let first = app.state.active;
        app.ready_test_project(first, &root.join("a"));
        let second = app.add_test_project(&root.join("b"));
        app.set_cwd_file(Some(cwd_file.clone()));
        app.switch_project(second);

        quit_with_q(&mut app);

        assert_eq!(written(&cwd_file), bytes(&root.join("b")));
    });
}

/// Inside a mounted archive the column shows a path no shell can `cd` into, so
/// the quit writes the directory that holds the archive.
#[test]
fn quitting_inside_an_archive_writes_the_directory_holding_it() {
    with_root(|root| {
        let cwd_file = root.join("cwd");
        let archive = root.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(root.to_path_buf());
        app.set_cwd_file(Some(cwd_file.clone()));
        mount_inline(&mut app, &archive, &root.join("staging"));
        // The cursor sits on `src/`, the first row; `Enter` descends into it.
        app.apply(&Action::EnterOrDisplay).unwrap();
        assert_eq!(app.state.cur().listing.dir, archive.join("src"));

        quit_with_q(&mut app);

        assert_eq!(written(&cwd_file), bytes(root));
    });
}

/// A cwd file that can't be written doesn't keep spyc from quitting: the
/// failure joins the exit summary, beside what the session save reported.
#[test]
fn an_unwritable_cwd_file_is_reported_at_exit_and_spyc_still_quits() {
    with_root(|root| {
        let cwd_file: PathBuf = root.join("missing").join("cwd");
        let mut app = App::test_app(root.join("a"));
        app.set_cwd_file(Some(cwd_file.clone()));

        quit_with_q(&mut app);

        assert!(!cwd_file.exists());
        let summary = app.exit_summary.as_deref().unwrap_or_default();
        assert!(
            summary.contains("missing/cwd"),
            "the exit summary names the file it couldn't write: {summary:?}"
        );
        assert!(
            summary.contains("session"),
            "and keeps the session save's report: {summary:?}"
        );
    });
}
