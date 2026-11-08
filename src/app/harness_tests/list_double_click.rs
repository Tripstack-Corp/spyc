//! A double-click on a file row does what `Enter` does there: a directory is
//! gone into and a file opens in the pager. Each case draws the listing and
//! presses where a row is drawn, through `handle_mouse`, as the user would.

use super::*;
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use std::path::{Path, PathBuf};

/// `root/work`, canonical, holding `docs/` (with `docs/inner/` inside) and
/// `notes.txt`.
fn work_dir(root: &Path) -> PathBuf {
    let work = std::fs::canonicalize(root).unwrap().join("work");
    std::fs::create_dir_all(work.join("docs").join("inner")).unwrap();
    std::fs::write(work.join("notes.txt"), "first line\nsecond line\n").unwrap();
    work
}

/// Run `test` against an app whose one column lists `work_dir`, with the
/// mouse on.
fn listing(test: impl FnOnce(&mut App, &Path)) {
    let _lock = crate::mouse_test_lock();
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let work = work_dir(tmp.path());
        let mut app = App::test_app(work.clone());
        app.view.term_size = (100, 24);
        app.state.chdir(&work).unwrap();
        crate::set_mouse_capture_for_test(true);
        test(&mut app, &work);
        crate::set_mouse_capture_for_test(false);
    });
}

fn drawn(app: &mut App) -> ratatui::buffer::Buffer {
    let (width, height) = app.view.term_size;
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| app.render(f)).unwrap();
    terminal.backend().buffer().clone()
}

/// Where `text` is first drawn, top to bottom and left to right.
fn drawn_at(app: &mut App, text: &str) -> (u16, u16) {
    let buf = drawn(app);
    for y in 0..buf.area.height {
        let cells: Vec<&str> = (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect();
        let Some(byte) = cells.concat().find(text) else {
            continue;
        };
        let mut at = 0;
        for (x, cell) in cells.iter().enumerate() {
            if at == byte {
                return (u16::try_from(x).unwrap(), y);
            }
            at += cell.len();
        }
    }
    panic!("{text:?} is not drawn");
}

fn mouse(app: &mut App, kind: MouseEventKind, (column, row): (u16, u16)) -> Vec<Effect> {
    app.handle_mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    })
}

/// A left press and release at `at`, and what they started.
fn click(app: &mut App, at: (u16, u16)) -> Vec<Effect> {
    let mut effects = mouse(app, MouseEventKind::Down(MouseButton::Left), at);
    effects.extend(mouse(app, MouseEventKind::Up(MouseButton::Left), at));
    effects
}

/// Two clicks at `at`, as quick as a double-click is.
fn double_click(app: &mut App, at: (u16, u16)) -> Vec<Effect> {
    let mut effects = click(app, at);
    effects.extend(click(app, at));
    effects
}

/// The file the pager over the list shows, if one is open.
fn paged(app: &App) -> Option<PathBuf> {
    app.view.project().pager.as_ref()?.source_path.clone()
}

/// A double-click on a directory goes into it, as `Enter` does.
#[test]
fn double_clicking_a_directory_goes_into_it() {
    listing(|app, work| {
        let docs = drawn_at(app, "docs");

        double_click(app, docs);

        assert_eq!(app.state.cur().listing.dir, work.join("docs"));
    });
}

/// A double-click on a file opens it in the pager, as `Enter` does, and the
/// list stays where it was.
#[test]
fn double_clicking_a_file_opens_it_in_the_pager() {
    listing(|app, work| {
        let notes = drawn_at(app, "notes.txt");

        double_click(app, notes);

        assert_eq!(paged(app), Some(work.join("notes.txt")));
        assert_eq!(app.state.cur().listing.dir, work);
    });
}

/// A double-click in the second column goes into the directory there, though
/// the first column had the keyboard.
#[test]
fn double_clicking_a_directory_in_the_other_column_goes_into_it_there() {
    listing(|app, work| {
        let other = work.join("docs");
        app.open_second_commander_at(&other);
        app.state
            .active_project_mut()
            .vsplit
            .as_mut()
            .expect("the second column is open")
            .focus = state::Side::Left;
        let inner = drawn_at(app, "inner");

        double_click(app, inner);

        assert_eq!(app.state.cur().listing.dir, other.join("inner"));
        assert_eq!(
            app.state.col(state::Side::Left).listing.dir,
            work,
            "the first column stays put"
        );
    });
}

/// Quick clicks on two different rows are two single clicks: neither row is
/// opened, and the cursor ends on the second.
#[test]
fn quick_clicks_on_two_rows_open_neither() {
    listing(|app, work| {
        let docs = drawn_at(app, "docs");
        let notes = drawn_at(app, "notes.txt");

        click(app, docs);
        click(app, notes);

        assert_eq!(app.state.cur().listing.dir, work);
        assert_eq!(paged(app), None);
        let cur = app.state.cur();
        assert_eq!(cur.rows[cur.cursor.index].path, work.join("notes.txt"));
    });
}

/// Once a double-click has gone into a directory, a third quick click lands
/// on whatever row the new listing drew there: it selects that row and does
/// not open it.
#[test]
fn a_third_quick_click_after_going_into_a_directory_does_not_open_the_new_row() {
    listing(|app, work| {
        let docs = drawn_at(app, "docs");

        double_click(app, docs);
        let inner = drawn_at(app, "inner");
        click(app, inner);

        assert_eq!(
            app.state.cur().listing.dir,
            work.join("docs"),
            "still in docs, not docs/inner"
        );
    });
}
