//! `yd` yanks the directory the focused column is in, and `yf` the entry under
//! the cursor (or the picks), each pressed as keys.

use super::archive::{build_zip, mount_inline};
use super::*;
use crate::app::effect::ClipMsg;

use std::path::{Path, PathBuf};

/// Press each key in turn, and return what the last one produced.
fn press_keys(app: &mut App, keys: &str) -> Vec<Effect> {
    let mut effects = Vec::new();
    for c in keys.chars() {
        effects = app.handle_key(key(c)).unwrap();
    }
    effects
}

/// The text `keys` put on the clipboard, and the flash that says so. Fails
/// when the keys produced anything but one clipboard copy.
fn yanked(app: &mut App, keys: &str) -> (String, String) {
    let effects = press_keys(app, keys);
    let (text, flash) = effects
        .clipboard()
        .unwrap_or_else(|| panic!("`{keys}` copies one text to the clipboard: {effects:?}"));
    (text.to_string(), flash)
}

/// The flash `yf` lands for one yanked path. A long path is cut short in it,
/// and a temp dir's path can be long.
fn one_path_flash(path: &str) -> String {
    ClipMsg::SinglePath.success(path)
}

/// `root/work`, canonical, holding a subdirectory `sub/` (listed first, so the
/// cursor starts on it) and two files.
fn work_dir(root: &Path) -> PathBuf {
    let work = std::fs::canonicalize(root).unwrap().join("work");
    std::fs::create_dir_all(work.join("sub")).unwrap();
    std::fs::write(work.join("notes.txt"), "notes\n").unwrap();
    std::fs::write(work.join("todo.txt"), "todo\n").unwrap();
    work
}

fn listed_app(dir: &Path) -> App {
    let mut app = App::test_app(dir.to_path_buf());
    app.state.refresh_listing();
    app
}

/// The headline: `yd` yanks the directory the column is in, not the entry
/// under the cursor, even when that entry is a directory itself.
#[test]
fn yd_yanks_the_directory_the_column_is_in() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let work = work_dir(tmp.path());
        let mut app = listed_app(&work);
        assert_eq!(
            app.state.selection_paths(),
            [work.join("sub").as_path()],
            "premise: the cursor is on the subdirectory"
        );

        let (text, flash) = yanked(&mut app, "yd");

        let want = work.display().to_string();
        assert_eq!(text, want, "the column's directory, as one line");
        assert_eq!(flash, one_path_flash(&want), "`yf`'s one-path flash");
    });
}

/// Picks choose what `yf` yanks, never what `yd` does.
#[test]
fn yd_with_picks_still_yanks_the_directory() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let work = work_dir(tmp.path());
        let mut app = listed_app(&work);
        press_keys(&mut app, "jtjt");
        assert_eq!(app.state.cur().picks.len(), 2, "premise: two files picked");

        let (text, flash) = yanked(&mut app, "yd");

        let want = work.display().to_string();
        assert_eq!(text, want);
        assert_eq!(flash, one_path_flash(&want));
    });
}

/// With two columns open, `yd` yanks the focused one's directory: `b`'s while
/// `b` has focus, then `a`'s once focus moves back.
#[test]
fn yd_yanks_the_focused_columns_directory() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let work = work_dir(tmp.path());
        let other = work.join("sub");
        let mut app = listed_app(&work);
        app.open_second_commander_at(&other);
        assert_eq!(
            app.state.cur().listing.dir,
            other,
            "premise: `b` is open on `sub` and focused"
        );

        let (in_b, _) = yanked(&mut app, "yd");
        assert_eq!(in_b, other.display().to_string(), "`b`'s directory");

        app.apply(&Action::VsplitFocusLeft).unwrap();
        let (in_a, _) = yanked(&mut app, "yd");
        assert_eq!(in_a, work.display().to_string(), "then `a`'s");
    });
}

/// Inside a mounted archive the column lists a path inside the archive, and
/// that is what `yd` yanks, not the directory holding the archive.
#[test]
fn yd_inside_an_archive_yanks_the_path_inside_it() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let work = work_dir(tmp.path());
        let archive = work.join("pkg.zip");
        build_zip(&archive);
        let mut app = listed_app(&work);
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()))
            .unwrap();
        let inside = archive.join("src");
        assert_eq!(
            app.state.cur().listing.dir,
            inside,
            "premise: the column lists `src/` inside the archive"
        );

        let (text, flash) = yanked(&mut app, "yd");

        let want = inside.display().to_string();
        assert_eq!(text, want, "the path inside the archive");
        assert_eq!(flash, one_path_flash(&want));
    });
}

/// `yf` keeps yanking the entry under the cursor, a directory as readily as a
/// file, and every pick when there are picks.
#[test]
fn yf_yanks_the_cursor_entry_or_the_picks() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let work = work_dir(tmp.path());
        let mut app = listed_app(&work);

        let (dir, flash) = yanked(&mut app, "yf");
        let sub = work.join("sub").display().to_string();
        assert_eq!(dir, sub, "the directory under the cursor");
        assert_eq!(flash, one_path_flash(&sub));

        let (file, _) = yanked(&mut app, "jyf");
        assert_eq!(file, work.join("notes.txt").display().to_string());

        press_keys(&mut app, "tjt");
        let (picks, flash) = yanked(&mut app, "yf");
        let mut lines: Vec<&str> = picks.lines().collect();
        lines.sort_unstable();
        assert_eq!(
            lines,
            [
                work.join("notes.txt").display().to_string(),
                work.join("todo.txt").display().to_string()
            ],
            "every pick, one per line"
        );
        assert_eq!(flash, "yanked 2 paths");
    });
}
