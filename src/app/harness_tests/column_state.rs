//! #1308: a save keeps what each column shows in its directory (the file
//! under its cursor, its picks and its `=` filter), and a restore, which a
//! handover is, puts them back.

use super::*;
use crate::app::state::Side;

/// `dir` with files to pick, and `right/` inside it for column `b`.
fn tree(dir: &std::path::Path) -> std::path::PathBuf {
    for name in ["a.txt", "b.txt", "c.md", "d.txt"] {
        std::fs::write(dir.join(name), name).unwrap();
    }
    let right = dir.join("right");
    std::fs::create_dir(&right).unwrap();
    for name in ["x.txt", "y.txt"] {
        std::fs::write(right.join(name), name).unwrap();
    }
    right
}

fn cursor_on(app: &mut App, side: Side, name: &str) {
    let col = app.state.col_mut(side);
    let at = col
        .rows
        .iter()
        .position(|r| r.path.file_name().is_some_and(|n| n == name))
        .unwrap_or_else(|| panic!("{name} isn't listed"));
    col.cursor.index = at;
}

fn under_cursor(app: &App, side: Side) -> String {
    let col = app.state.col(side);
    col.rows[col.cursor.index]
        .path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

fn picked(app: &App, side: Side) -> Vec<String> {
    let mut names: Vec<String> = app
        .state
        .col(side)
        .picks
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Column `a` with b.txt and d.txt picked, the `*.txt` filter and the cursor
/// on d.txt; column `b` in `right/` with y.txt picked and the cursor on it.
fn arranged(dir: &std::path::Path, right: &std::path::Path) -> App {
    let mut app = App::test_app(dir.to_path_buf());
    app.state.chdir_side(Side::Left, dir).unwrap();
    for name in ["b.txt", "d.txt"] {
        app.state.col_mut(Side::Left).picks.insert(&dir.join(name));
    }
    app.state.col_mut(Side::Left).temp_filter = Some("*.txt".into());
    app.state.rebuild_rows_for(Side::Left);
    cursor_on(&mut app, Side::Left, "d.txt");
    app.open_second_commander_at(right);
    app.state
        .col_mut(Side::Right)
        .picks
        .insert(&right.join("y.txt"));
    app.state.rebuild_rows_for(Side::Right);
    cursor_on(&mut app, Side::Right, "y.txt");
    app
}

#[test]
fn a_restore_puts_back_each_columns_cursor_picks_and_filter() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let right = tree(&dir);
        let saved = arranged(&dir, &right).build_manifest();

        let mut app = App::test_app(dir);
        let _effects = app.restore_session(&saved);
        assert_eq!(picked(&app, Side::Left), ["b.txt", "d.txt"]);
        assert_eq!(
            app.state.col(Side::Left).temp_filter.as_deref(),
            Some("*.txt")
        );
        assert_eq!(under_cursor(&app, Side::Left), "d.txt");
        let shown: Vec<_> = app
            .state
            .col(Side::Left)
            .rows
            .iter()
            .map(|r| r.display.clone())
            .collect();
        assert!(
            !shown.iter().any(|d| d.contains("c.md")),
            "the filter wasn't applied: {shown:?}"
        );
        assert_eq!(picked(&app, Side::Right), ["y.txt"]);
        assert_eq!(under_cursor(&app, Side::Right), "y.txt");
    });
}

/// A file picked, or under the cursor, that has gone by the restore is left
/// out; the rest comes back.
#[test]
fn a_file_gone_by_the_restore_is_neither_picked_nor_under_the_cursor() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let right = tree(&dir);
        let saved = arranged(&dir, &right).build_manifest();
        std::fs::remove_file(dir.join("d.txt")).unwrap();

        let mut app = App::test_app(dir);
        let _effects = app.restore_session(&saved);
        assert_eq!(picked(&app, Side::Left), ["b.txt"]);
        assert_eq!(app.state.col(Side::Left).cursor.index, 0);
        assert_eq!(
            app.state.col(Side::Left).temp_filter.as_deref(),
            Some("*.txt")
        );
    });
}

/// A column that lands somewhere other than where it was saved takes none of
/// what it showed there.
#[test]
fn a_column_restored_elsewhere_keeps_none_of_it() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let right = tree(&dir);
        let mut saved = arranged(&dir, &right).build_manifest();
        saved.projects[0].left_cwd = Some(dir.join("gone"));

        let mut app = App::test_app(dir);
        let _effects = app.restore_session(&saved);
        assert!(app.state.col(Side::Left).picks.is_empty());
        assert_eq!(app.state.col(Side::Left).temp_filter, None);
    });
}

/// Picks and the filter dirty the autosave; the cursor, which moves with
/// every key, doesn't.
#[test]
fn picks_and_the_filter_change_the_session_fingerprint_and_the_cursor_doesnt() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let right = tree(&dir);
        let mut app = arranged(&dir, &right);
        let before = app.session_fingerprint();
        cursor_on(&mut app, Side::Left, "b.txt");
        assert_eq!(app.session_fingerprint(), before, "a cursor move");
        app.state
            .col_mut(Side::Left)
            .picks
            .insert(&dir.join("a.txt"));
        let picked = app.session_fingerprint();
        assert_ne!(picked, before, "a pick");
        app.state.col_mut(Side::Right).temp_filter = Some("x*".into());
        assert_ne!(app.session_fingerprint(), picked, "column b's filter");
    });
}

/// A project restored beside the one in view, as `:attach project`, `Space j`
/// and a session's other projects are, gets its column back too.
#[test]
fn a_project_restored_out_of_view_gets_its_column_back() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let base = std::fs::canonicalize(tmp.path()).unwrap();
        let (dir, elsewhere) = (base.join("proj"), base.join("elsewhere"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        let right = tree(&dir);
        let saved = arranged(&dir, &right).build_manifest();

        let mut app = App::test_app(elsewhere);
        let held = crate::state::sessions::Held::by_running(&[]);
        let opened = app
            .restore_out_of_view(&saved.projects[0], false, &held)
            .expect("the project opens");
        let (picks, filter) = app.as_project(opened.id, |app| {
            (
                picked(app, Side::Left),
                app.state.col(Side::Left).temp_filter.clone(),
            )
        });
        assert_eq!(picks, ["b.txt", "d.txt"]);
        assert_eq!(filter.as_deref(), Some("*.txt"));
    });
}
