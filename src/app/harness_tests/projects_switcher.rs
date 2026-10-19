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
/// how many tabs it has and its home, the one in view marked and highlighted.
#[test]
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
        assert!(x_row.contains("0 tabs"), "{x_row}");
        let x_home = crate::paths::display_tilde(&t.x_home);
        assert!(x_row.contains(&x_home), "no home: {x_row}");
        assert!(!y_row.contains("← current"), "{y_row}");
        // The home, of no fixed width, comes last, so a long one can't push the
        // marker past the pager's edge.
        assert!(
            x_row.find("← current") < x_row.rfind("xproj"),
            "the marker follows the home: {x_row}"
        );
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
        assert!(app.state.pending_projects.is_none());
    });
}

/// A digit goes to the project numbered.
#[test]
fn a_digit_goes_to_that_project() {
    with_three(|t| {
        let app = &mut t.app;
        leader(app, 'j');
        app.handle_key(key('2')).unwrap();
        assert_eq!(app.state.active, t.y);
        assert!(app.state.pending_projects.is_none());
    });
}

/// A digit past the list goes nowhere, and the switcher stays open.
#[test]
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
fn q_closes_the_switcher_where_it_was() {
    with_three(|t| {
        let app = &mut t.app;
        leader(app, 'j');
        assert!(rows(app).is_some(), "Space j opened no switcher");
        app.handle_key(key('q')).unwrap();
        assert_eq!(app.state.active, t.x);
        assert!(rows(app).is_none(), "the switcher is still open");
        assert!(app.state.pending_projects.is_none());
    });
}

/// The highlight and the `← current` mark follow the project in view, not the
/// first row.
#[test]
fn the_highlight_follows_the_project_in_view() {
    with_three(|t| {
        let app = &mut t.app;
        leader(app, '2');
        leader(app, 'j');
        let rows = rows(app).expect("Space j opened no switcher");
        let y_at = rows.iter().position(|r| r.contains("yproj"));
        let cursor = app
            .view
            .project()
            .pager
            .as_ref()
            .and_then(|v| v.picker_cursor);
        assert_eq!(cursor, y_at);
        let y_row = &rows[y_at.expect("y is listed")];
        assert!(y_row.contains("← current"), "{y_row}");
    });
}

/// A `/` search lands the highlight on its match, so Enter then goes there.
#[test]
fn a_search_lands_the_highlight_on_its_match() {
    with_three(|t| {
        let app = &mut t.app;
        leader(app, 'j');
        for c in "/yproj".chars() {
            app.handle_key(key(c)).unwrap();
        }
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        app.handle_key(enter).unwrap();
        app.handle_key(enter).unwrap();
        assert_eq!(app.state.active, t.y);
    });
}

/// The arrow keys move the highlight, as `j`/`k` do.
#[test]
fn the_arrow_keys_move_the_highlight() {
    with_three(|t| {
        let app = &mut t.app;
        leader(app, 'j');
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
            .unwrap();
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.state.active, t.y);
    });
}

/// With column `b` focused and its own `D` pager open, a pick closes the
/// switcher, the modal pager over every column, not `b`'s pager.
#[test]
fn a_pick_closes_the_switcher_not_column_bs_pager() {
    with_three(|t| {
        let (x, x_home) = (t.x, t.x_home.clone());
        let app = &mut t.app;
        app.open_second_commander_at(&x_home);
        let mut d = crate::ui::pager::PagerView::new_plain("b", vec!["right".to_string()]);
        d.mount = crate::ui::pager::Mount::TopPane;
        app.view.project_mut().pager_right = Some(d);
        app.state.active_project_mut().focus =
            state::Focus::Pager(crate::ui::pager::Mount::TopPane);
        app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL))
            .unwrap();
        leader(app, 'j');
        assert!(rows(app).is_some(), "^a Space j opened no switcher");
        app.handle_key(key('2')).unwrap();
        assert_eq!(app.state.active, t.y);
        app.as_project(x, |app| {
            assert!(
                app.view.project().pager.is_none(),
                "the switcher is still open"
            );
            assert!(app.view.project().pager_right.is_some(), "b's pager closed");
        });
    });
}

/// From inside the editor the switcher opens over it and has the keyboard: a
/// digit goes to that project, and the editor carries on in the project left.
#[test]
#[ignore = "red: Space j refuses from the editor"]
fn space_j_from_the_editor_takes_the_keys() {
    with_three(|t| {
        let (x, x_home) = (t.x, t.x_home.clone());
        let app = &mut t.app;
        let wake = app.make_pane_wake();
        let context = app.view.project().context_path.clone();
        let editor = crate::pane::Pane::spawn("cat", 24, 80, &x_home, &context, wake)
            .expect("the editor spawns");
        app.runtime.project_mut().top_overlay = Some(editor);
        app.state.active_project_mut().focus = state::Focus::Overlay;
        app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL))
            .unwrap();
        leader(app, 'j');
        assert!(
            rows(app).is_some(),
            "Space j opened no switcher: {:?}",
            app.flash_text()
        );
        // The loop top derives the focus again before it reads each key.
        app.recompute_focus();
        app.handle_key(key('2')).unwrap();
        assert_eq!(app.state.active, t.y);
        assert!(
            app.as_project(x, |app| app.runtime.project().top_overlay.is_some()),
            "the editor quit"
        );
    });
}

/// From inside the editor the switcher doesn't open: the editor keeps the
/// keyboard over a pager opened on top of it, so the keys would go to the
/// file.
#[test]
fn space_j_from_the_editor_says_quit_it_first() {
    with_three(|t| {
        let x_home = t.x_home.clone();
        let app = &mut t.app;
        let wake = app.make_pane_wake();
        let context = app.view.project().context_path.clone();
        let editor = crate::pane::Pane::spawn("cat", 24, 80, &x_home, &context, wake)
            .expect("the editor spawns");
        app.runtime.project_mut().top_overlay = Some(editor);
        app.state.active_project_mut().focus = state::Focus::Overlay;
        app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL))
            .unwrap();
        leader(app, 'j');
        assert!(rows(app).is_none(), "the switcher opened over the editor");
        assert!(
            app.flash_text()
                .is_some_and(|f| f.contains("quit the editor first")),
            "{:?}",
            app.flash_text()
        );
    });
}
