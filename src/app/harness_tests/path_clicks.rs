//! A click on a folder in the status bar's path goes up to it, as `u` does
//! that many times, with the cursor on the folder it came from. The folder
//! you're in, and a name the bar cut short, go nowhere.

use super::clicks::{change_dirs, click, drawn, drawn_at, listing};
use super::*;
use std::path::Path;

/// In `work/docs/inner`, at `width` columns.
fn deep(app: &mut App, work: &Path, width: u16) {
    app.view.term_size = (width, 24);
    app.state.chdir(&work.join("docs").join("inner")).unwrap();
}

/// The row under the cursor.
fn on(app: &App) -> std::path::PathBuf {
    let cur = app.state.cur();
    cur.rows[cur.cursor.index].path.clone()
}

/// Where `name` is drawn in the status bar's path: the first place it is
/// drawn, which the tests make sure is on the bar.
fn in_the_bar(app: &mut App, name: &str) -> (u16, u16) {
    let at = drawn_at(app, name);
    let status = app.view.status_row.borrow().as_ref().map(|r| r.y);
    assert_eq!(Some(at.1), status, "{name:?} is drawn on the status bar");
    at
}

fn click_in_the_bar(app: &mut App, name: &str) {
    let at = in_the_bar(app, name);
    let effects = click(app, at);
    change_dirs(app, effects);
}

#[test]
fn a_click_on_a_folder_in_the_path_goes_up_to_it() {
    listing(|app, work| {
        deep(app, work, 200);

        click_in_the_bar(app, "work");

        assert_eq!(app.state.cur().listing.dir, work);
        assert_eq!(
            on(app),
            work.join("docs"),
            "the cursor is on the folder it came from"
        );
    });
}

#[test]
fn a_click_on_the_parent_in_the_path_goes_up_one() {
    listing(|app, work| {
        deep(app, work, 200);

        click_in_the_bar(app, "docs");

        assert_eq!(app.state.cur().listing.dir, work.join("docs"));
        assert_eq!(on(app), work.join("docs").join("inner"));
    });
}

/// The folder you're in is where you already are.
#[test]
fn a_click_on_the_folder_you_are_in_stays_there() {
    listing(|app, work| {
        deep(app, work, 200);

        click_in_the_bar(app, "inner");

        assert_eq!(app.state.cur().listing.dir, work.join("docs").join("inner"));
    });
}

/// In a narrow bar the path is cut in its middle; the folders still drawn
/// after the cut go where they say.
#[test]
fn a_click_after_the_cut_in_a_shortened_path_goes_up_to_that_folder() {
    listing(|app, work| {
        deep(app, work, 50);
        let buf = drawn(app);
        let row: String = (0..buf.area.width).map(|x| buf[(x, 0)].symbol()).collect();
        assert!(row.contains("..."), "the path is cut: {row:?}");

        click_in_the_bar(app, "docs");

        assert_eq!(app.state.cur().listing.dir, work.join("docs"));
    });
}

/// Without colour the bar is drawn plainly, and its path is as clickable.
#[test]
fn a_click_on_a_folder_in_the_plain_bar_goes_up_to_it() {
    listing(|app, work| {
        app.view.theme.mono = true;
        deep(app, work, 200);

        click_in_the_bar(app, "docs");

        assert_eq!(app.state.cur().listing.dir, work.join("docs"));
    });
}

/// A double-click on a folder goes up to it once: the folder clicked is the
/// path's last now, drawn where it was, and the second click goes nowhere.
#[test]
fn a_double_click_on_a_folder_goes_up_to_it_once() {
    listing(|app, work| {
        deep(app, work, 200);
        let docs = in_the_bar(app, "docs");

        let effects = super::clicks::double_click(app, docs);
        change_dirs(app, effects);

        assert_eq!(app.state.cur().listing.dir, work.join("docs"));
    });
}
