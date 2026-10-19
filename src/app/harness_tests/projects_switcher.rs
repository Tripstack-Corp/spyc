//! #99 step 5e, pair 3a: the project switcher (`Space j`), which lists the open
//! projects and goes to the one picked.
//!
//! The fixtures are `projects_open.rs`'s: `x` is where the user starts and `y`
//! is open beside it.

use super::projects_open::{leader, with_three};
use super::*;
use crossterm::event::KeyCode;

/// The switcher's rows, while it is open in the project in view.
fn rows(app: &App) -> Option<Vec<String>> {
    app.view
        .project()
        .pager
        .as_ref()
        .map(|v| v.lines.iter().map(ToString::to_string).collect())
}

/// `Space j` lists the open projects in switcher order, numbered, each with
/// its home and how many tabs it has, the one in view marked and highlighted.
#[test]
#[ignore = "red: Space j opens nothing"]
fn space_j_lists_the_open_projects_numbered() {
    with_three(|t| {
        let (y, y_home) = (t.y, t.y_home.clone());
        let app = &mut t.app;
        app.open_tab_in_project(y, &y_home);
        leader(app, 'j');
        let rows = rows(app).expect("Space j opened no switcher");
        let x_row = rows
            .iter()
            .find(|r| r.contains("xproj"))
            .expect("x is listed");
        let y_row = rows
            .iter()
            .find(|r| r.contains("yproj"))
            .expect("y is listed");
        assert!(
            x_row.contains("[1]") && x_row.contains("← current"),
            "{x_row}"
        );
        assert!(y_row.contains("[2]") && y_row.contains("1 tab"), "{y_row}");
        assert!(!y_row.contains("← current"), "{y_row}");
        let cursor = app
            .view
            .project()
            .pager
            .as_ref()
            .and_then(|v| v.picker_cursor);
        let x_at = rows.iter().position(|r| r.contains("xproj"));
        assert_eq!(cursor, x_at, "the highlight isn't on the project in view");
    });
}

/// `j` then Enter goes to the highlighted project, and the switcher closes:
/// it was the project left's pager, and that project's view is clear of it.
#[test]
#[ignore = "red: Space j opens nothing"]
fn enter_goes_to_the_highlighted_project() {
    with_three(|t| {
        let (x, y) = (t.x, t.y);
        let app = &mut t.app;
        leader(app, 'j');
        app.handle_key(key('j')).unwrap();
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.state.active, y);
        assert!(
            app.as_project(x, |app| app.view.project().pager.is_none()),
            "the switcher is still open in the project left"
        );
    });
}

/// A digit goes to the project numbered.
#[test]
#[ignore = "red: Space j opens nothing"]
fn a_digit_goes_to_that_project() {
    with_three(|t| {
        let app = &mut t.app;
        leader(app, 'j');
        app.handle_key(key('2')).unwrap();
        assert_eq!(app.state.active, t.y);
    });
}

/// A digit past the list goes nowhere, and the switcher stays open.
#[test]
#[ignore = "red: Space j opens nothing"]
fn a_digit_past_the_list_stays_put() {
    with_three(|t| {
        let app = &mut t.app;
        leader(app, 'j');
        app.handle_key(key('7')).unwrap();
        assert_eq!(app.state.active, t.x);
        assert!(rows(app).is_some(), "the switcher closed");
    });
}

/// `q` closes the switcher where the user is.
#[test]
#[ignore = "red: Space j opens nothing"]
fn q_closes_the_switcher_where_it_was() {
    with_three(|t| {
        let app = &mut t.app;
        leader(app, 'j');
        assert!(rows(app).is_some(), "Space j opened no switcher");
        app.handle_key(key('q')).unwrap();
        assert_eq!(app.state.active, t.x);
        assert!(rows(app).is_none(), "the switcher is still open");
    });
}
