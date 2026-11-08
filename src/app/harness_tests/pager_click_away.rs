//! A click beside the centred pager's box closes it, as `q` does, so a file
//! opened with a double-click is put away with the mouse too. The click
//! closes the pager and does nothing else.

use super::clicks::{click, double_click, drawn_at, listing, paged};
use super::*;

/// Open `notes.txt` with a double-click, and draw the pager over the list.
fn viewing_notes(app: &mut App, work: &std::path::Path) {
    let notes = drawn_at(app, "notes.txt");
    double_click(app, notes);
    assert_eq!(paged(app), Some(work.join("notes.txt")), "the pager opened");
    drawn_at(app, "second line");
}

/// The cell at the left edge, halfway down: beside the centred box.
fn beside_the_box(app: &App) -> (u16, u16) {
    (0, app.view.term_size.1 / 2)
}

#[test]
#[ignore = "red: a click beside the pager is swallowed"]
fn a_click_beside_the_pager_closes_it() {
    listing(|app, work| {
        viewing_notes(app, work);
        let cursor = app.state.cur().cursor.index;

        click(app, beside_the_box(app));

        assert_eq!(paged(app), None, "the pager closed");
        assert_eq!(app.state.cur().listing.dir, work);
        assert_eq!(
            app.state.cur().cursor.index,
            cursor,
            "the click closed the pager and did not reach the list"
        );
    });
}

/// A click on the pager's own text keeps it open.
#[test]
fn a_click_on_the_pagers_text_keeps_it_open() {
    listing(|app, work| {
        viewing_notes(app, work);
        let text = drawn_at(app, "second line");

        click(app, text);

        assert_eq!(paged(app), Some(work.join("notes.txt")));
    });
}

/// The wheel beside the box scrolls nothing and closes nothing.
#[test]
fn the_wheel_beside_the_pager_keeps_it_open() {
    listing(|app, work| {
        viewing_notes(app, work);

        super::clicks::mouse(
            app,
            crossterm::event::MouseEventKind::ScrollDown,
            beside_the_box(app),
        );

        assert_eq!(paged(app), Some(work.join("notes.txt")));
    });
}
