//! Dragging a project's label along the status bar moves that project, as
//! `:order` does. The press goes to it, as a click does, and each label the
//! pointer moves onto gives up its place, the bar drawn again as it goes.
//! Every case goes through `handle_mouse`, with a frame drawn before each
//! motion as the loop draws one.

use super::status_menu_tests::{on_bar, projects};
use super::tests::at;
use crate::app::App;
use crate::app::state::Project;
use crossterm::event::{MouseButton, MouseEventKind};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::path::Path;

/// Run `body` over a fresh state root, in a canonical temp dir, with mouse
/// reporting on.
fn with_mouse(body: impl FnOnce(&Path)) {
    let _lock = crate::mouse_test_lock();
    let tmp = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(tmp.path()).unwrap();
    let state = base.join("state");
    std::fs::create_dir_all(&state).unwrap();
    crate::state::with_state_root(&state, || {
        crate::set_mouse_capture_for_test(true);
        body(&base);
        crate::set_mouse_capture_for_test(false);
    });
}

/// One frame at the App's own size, as the loop draws after each event.
fn draw(app: &mut App) {
    let (width, height) = app.view.term_size;
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| app.render(f)).unwrap();
}

/// The open projects' labels, in order.
fn order(app: &App) -> Vec<String> {
    app.state.open_projects().map(Project::label).collect()
}

fn press(app: &mut App, (col, row): (u16, u16)) {
    draw(app);
    app.handle_mouse(at(MouseEventKind::Down(MouseButton::Left), col, row));
}

fn drag(app: &mut App, (col, row): (u16, u16)) {
    draw(app);
    app.handle_mouse(at(MouseEventKind::Drag(MouseButton::Left), col, row));
}

fn release(app: &mut App, (col, row): (u16, u16)) {
    draw(app);
    app.handle_mouse(at(MouseEventKind::Up(MouseButton::Left), col, row));
}

/// Press on `from`'s label and drag along the bar a cell at a time to column
/// `to`, the order after each step.
fn drag_along(app: &mut App, from: (u16, u16), to: u16) -> Vec<Vec<String>> {
    press(app, from);
    let steps: Vec<u16> = if to < from.0 {
        (to..from.0).rev().collect()
    } else {
        (from.0 + 1..=to).collect()
    };
    steps
        .into_iter()
        .map(|x| {
            drag(app, (x, from.1));
            order(app)
        })
        .collect()
}

/// A label dragged left onto the first one's place moves to the front, the
/// others shifting along, and stays the project in view.
#[test]
fn a_label_dragged_onto_the_first_moves_to_the_front() {
    with_mouse(|base| {
        let (mut app, ids) = projects(base, &["alpha", "bravo", "charlie"], 120);
        let from = on_bar(&mut app, "charlie");
        let to = on_bar(&mut app, "alpha").0;
        drag_along(&mut app, from, to);
        release(&mut app, (to, from.1));
        assert_eq!(
            order(&app),
            ["charlie", "alpha", "bravo"],
            "{:?}",
            app.flash_text()
        );
        assert_eq!(
            app.state.active, ids[2],
            "the dragged project isn't in view"
        );
    });
}

/// A label dragged right onto the last one's place moves to the end.
#[test]
fn a_label_dragged_onto_the_last_moves_to_the_end() {
    with_mouse(|base| {
        let (mut app, ids) = projects(base, &["alpha", "bravo", "charlie"], 120);
        let from = on_bar(&mut app, "alpha");
        let to = on_bar(&mut app, "charlie").0;
        drag_along(&mut app, from, to);
        release(&mut app, (to, from.1));
        assert_eq!(
            order(&app),
            ["bravo", "charlie", "alpha"],
            "{:?}",
            app.flash_text()
        );
        assert_eq!(app.state.active, ids[0]);
    });
}

/// The labels move while the button is held, so the bar shows where the
/// project will land, and dragging back puts it back.
#[test]
fn the_order_follows_the_pointer_and_back() {
    with_mouse(|base| {
        let (mut app, _) = projects(base, &["alpha", "bravo", "charlie"], 120);
        let from = on_bar(&mut app, "alpha");
        let to = on_bar(&mut app, "charlie").0;
        let out = drag_along(&mut app, from, to);
        assert_eq!(
            out.last().map(Vec::as_slice),
            Some(&["bravo", "charlie", "alpha"].map(String::from)[..]),
            "nothing moved while held"
        );
        for x in (from.0..to).rev() {
            drag(&mut app, (x, from.1));
        }
        release(&mut app, from);
        assert_eq!(order(&app), ["alpha", "bravo", "charlie"]);
    });
}

/// A short label dragged across a long one moves past it once and stays
/// there: after the move the pointer is over the long label again, which
/// must not send the short one back, step after step.
#[test]
fn a_short_label_crossing_a_long_one_does_not_flip_back() {
    with_mouse(|base| {
        let (mut app, _) = projects(base, &["a-much-longer-name", "b"], 120);
        let from = on_bar(&mut app, " b ");
        let from = (from.0 + 1, from.1);
        let to = on_bar(&mut app, "a-much-longer-name").0;
        let out = drag_along(&mut app, from, to);
        let first = out
            .iter()
            .position(|o| o[0] == "b")
            .unwrap_or_else(|| panic!("b never moved to the front: {out:?}"));
        assert!(
            out[first..].iter().all(|o| o[0] == "b"),
            "b flipped back: {out:?}"
        );
        release(&mut app, (to, from.1));
        assert_eq!(order(&app), ["b", "a-much-longer-name"]);
    });
}

/// A press and a release on one label is a click: it goes there and moves
/// nothing.
#[test]
fn a_click_on_a_label_still_only_goes_there() {
    with_mouse(|base| {
        let (mut app, ids) = projects(base, &["alpha", "bravo", "charlie"], 120);
        let at = on_bar(&mut app, "bravo");
        press(&mut app, at);
        drag(&mut app, (at.0 + 1, at.1));
        release(&mut app, (at.0 + 1, at.1));
        assert_eq!(app.state.active, ids[1]);
        assert_eq!(order(&app), ["alpha", "bravo", "charlie"]);
    });
}

/// A drag that leaves the labels for the path moves nothing: no label gave
/// up its place.
#[test]
fn a_drag_off_the_labels_moves_nothing() {
    with_mouse(|base| {
        let (mut app, ids) = projects(base, &["alpha", "bravo", "charlie"], 120);
        let from = on_bar(&mut app, "charlie");
        let to = on_bar(&mut app, "/").0;
        drag_along(&mut app, from, to);
        release(&mut app, (to, from.1));
        assert_eq!(order(&app), ["alpha", "bravo", "charlie"]);
        assert_eq!(app.state.active, ids[2]);
    });
}
