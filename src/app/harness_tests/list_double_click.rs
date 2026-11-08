//! A double-click on a file row does what `Enter` does there: a directory is
//! gone into and a file opens in the pager. Each case draws the listing and
//! presses where a row is drawn, through `handle_mouse`, as the user would.

use super::clicks::{click, double_click, drawn_at, listing, paged};
use super::*;

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
