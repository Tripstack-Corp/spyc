//! `:order N` moves the project in view to slot N of the open projects, the
//! one order `Space 1`-`9`, the switcher (`Space j`), the status bar and a
//! saved session all follow. Bare, it shows that order.
//!
//! The fixtures are `projects_open.rs`'s: `x` is where the user starts and
//! `y` is open beside it; `open` adds `zproj/` as a third and goes to it.

use std::path::{Path, PathBuf};

use super::manifest_v2::read_manifest;
use super::projects_open::{Three, leader, open, with_three};
use super::*;
use crossterm::event::KeyCode;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use serde_json::{Value, json};

/// Type `line` at the `:` prompt and run it.
fn command(app: &mut App, line: &str) {
    app.handle_key(key(':')).unwrap();
    for c in line.chars() {
        app.handle_key(key(c)).unwrap();
    }
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();
}

/// The open projects' homes, in switcher order.
fn homes(app: &App) -> Vec<PathBuf> {
    app.state
        .open_projects()
        .filter_map(|p| p.home.clone())
        .collect()
}

/// `z` opened as a third project, so the order is `x`, `y`, `z` with `z` in
/// view.
fn open_z(t: &mut Three) -> state::ProjectId {
    let z_home = t.z_home.clone();
    open(&mut t.app, &z_home);
    let z = t.app.state.active;
    assert_eq!(
        homes(&t.app),
        [t.x_home.clone(), t.y_home.clone(), z_home],
        "the fixture's order: {:?}",
        t.app.flash_text()
    );
    z
}

/// The switcher's rows, while it is open in the project in view.
fn switcher_rows(app: &App) -> Vec<String> {
    app.view
        .project()
        .pager
        .as_ref()
        .map_or_default(|v| v.lines.iter().map(ToString::to_string).collect())
}

/// The status bar's row of one frame, 120 columns wide.
fn bar(app: &mut App) -> String {
    app.view.term_size = (120, 24);
    let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();
    terminal.draw(|f| app.render(f)).unwrap();
    let buf = terminal.backend().buffer().clone();
    let y = app.frame_layout(Rect::new(0, 0, 120, 24)).status.y;
    (0..120).map(|x| buf[(x, y)].symbol().to_string()).collect()
}

/// Where each of `names` is first drawn in `text`, in the order they are
/// drawn.
fn drawn_order<'a>(text: &str, names: &[&'a str]) -> Vec<&'a str> {
    let mut at: Vec<(usize, &str)> = names
        .iter()
        .map(|n| {
            (
                text.find(n)
                    .unwrap_or_else(|| panic!("{n} not in {text:?}")),
                *n,
            )
        })
        .collect();
    at.sort_unstable();
    at.into_iter().map(|(_, n)| n).collect()
}

/// Where `with_three` keeps spyc's state.
fn state_root(t: &Three) -> PathBuf {
    t.home.parent().expect("the fixture's base").join("state")
}

/// The homes a saved session lists as open, in its order.
fn saved_homes(app: &mut App, state_root: &Path) -> Vec<Value> {
    app.save_session();
    let saved = read_manifest(state_root, app.state.session_id.expect("a session id"));
    saved["projects"]
        .as_array()
        .expect("projects")
        .iter()
        .map(|p| p["home"].clone())
        .collect()
}

/// `:order 1` puts the project in view first and shifts the others along;
/// it stays the project in view.
#[test]
#[ignore = "red until :order moves the project in view"]
fn order_1_puts_the_project_in_view_first() {
    with_three(|t| {
        let z = open_z(t);
        let app = &mut t.app;
        command(app, "order 1");
        assert_eq!(
            homes(app),
            [t.z_home.clone(), t.x_home.clone(), t.y_home.clone()],
            "{:?}",
            app.flash_text()
        );
        assert_eq!(app.state.active, z, "the project in view changed");
    });
}

/// A move to the middle slot, from the first.
#[test]
#[ignore = "red until :order moves the project in view"]
fn order_2_moves_the_first_project_to_the_middle() {
    with_three(|t| {
        open_z(t);
        let x = t.x;
        let app = &mut t.app;
        leader(app, '1');
        assert_eq!(app.state.active, x);
        command(app, "order 2");
        assert_eq!(
            homes(app),
            [t.y_home.clone(), t.x_home.clone(), t.z_home.clone()],
            "{:?}",
            app.flash_text()
        );
    });
}

/// The last slot is the number of projects open.
#[test]
#[ignore = "red until :order moves the project in view"]
fn order_n_moves_it_last() {
    with_three(|t| {
        let app = &mut t.app;
        command(app, "order 2");
        assert_eq!(
            homes(app),
            [t.y_home.clone(), t.x_home.clone()],
            "{:?}",
            app.flash_text()
        );
    });
}

/// `Space N` goes by the new order: after `z` moves first, `Space 1` is `z`
/// and `Space 2` is `x`.
#[test]
#[ignore = "red until :order moves the project in view"]
fn space_n_follows_the_new_order() {
    with_three(|t| {
        let z = open_z(t);
        let x = t.x;
        let app = &mut t.app;
        command(app, "order 1");
        leader(app, '2');
        assert_eq!(app.state.active, x, "Space 2 went elsewhere");
        leader(app, '1');
        assert_eq!(app.state.active, z, "Space 1 went elsewhere");
    });
}

/// The switcher numbers its rows in the new order.
#[test]
#[ignore = "red until :order moves the project in view"]
fn the_switcher_lists_the_new_order() {
    with_three(|t| {
        open_z(t);
        let app = &mut t.app;
        command(app, "order 1");
        leader(app, 'j');
        let rows = switcher_rows(app);
        let row = |home: &str| {
            rows.iter()
                .find(|r| r.contains(home))
                .unwrap_or_else(|| panic!("{home} not listed: {rows:#?}"))
                .clone()
        };
        assert!(row("zproj").contains("[1]"), "{rows:#?}");
        assert!(row("xproj").contains("[2]"), "{rows:#?}");
        assert!(row("yproj").contains("[3]"), "{rows:#?}");
    });
}

/// The status bar draws the projects' labels in the new order.
#[test]
#[ignore = "red until :order moves the project in view"]
fn the_status_bar_draws_the_new_order() {
    with_three(|t| {
        open_z(t);
        let app = &mut t.app;
        let names = ["xproj", "yproj", "zproj"];
        assert_eq!(drawn_order(&bar(app), &names), names, "the control");
        command(app, "order 1");
        let bar = bar(app);
        assert_eq!(
            drawn_order(&bar, &names),
            ["zproj", "xproj", "yproj"],
            "{bar:?}"
        );
    });
}

/// A saved session keeps the new order, so `spyc -r` brings it back.
#[test]
#[ignore = "red until :order moves the project in view"]
fn a_saved_session_keeps_the_new_order() {
    with_three(|t| {
        let state_root = state_root(t);
        let (x, x_home) = (t.x, t.x_home.clone());
        open_z(t);
        let app = &mut t.app;
        app.state.session_id = Some(500);
        app.open_tab_in_project(x, &x_home);
        command(app, "order 1");
        assert_eq!(
            saved_homes(app, &state_root),
            [json!(t.z_home), json!(t.x_home), json!(t.y_home)],
            "{:?}",
            app.flash_text()
        );
    });
}

/// Bare, `:order` shows the order, numbered, and moves nothing.
#[test]
#[ignore = "red until :order moves the project in view"]
fn bare_order_shows_the_order() {
    with_three(|t| {
        open_z(t);
        let app = &mut t.app;
        command(app, "order 1");
        command(app, "order");
        let flash = app.flash_text().unwrap_or_default().to_string();
        assert_eq!(
            drawn_order(&flash, &["xproj", "yproj", "zproj"]),
            ["zproj", "xproj", "yproj"],
            "{flash:?}"
        );
        assert!(
            flash.contains('1') && flash.contains('3'),
            "unnumbered: {flash:?}"
        );
        assert_eq!(
            homes(app),
            [t.z_home.clone(), t.x_home.clone(), t.y_home.clone()]
        );
    });
}

/// A slot that isn't one, `0`, one past the end or no number at all, moves
/// nothing and says which slots there are.
#[test]
#[ignore = "red until :order moves the project in view"]
fn a_slot_out_of_range_moves_nothing_and_names_the_range() {
    with_three(|t| {
        let before = homes(&t.app);
        let app = &mut t.app;
        for arg in ["0", "3", "first", "-1"] {
            command(app, &format!("order {arg}"));
            let flash = app.flash_text().unwrap_or_default().to_string();
            assert_eq!(homes(app), before, ":order {arg} moved a project");
            assert!(
                flash.contains("1–2"),
                ":order {arg} didn't name the range: {flash:?}"
            );
            assert_eq!(app.state.active, t.x, ":order {arg} changed project");
        }
    });
}

/// With one project open there's nothing to put in order, and `:order` says
/// so rather than reading as an unknown command.
#[test]
#[ignore = "red until :order moves the project in view"]
fn with_one_project_open_order_says_there_is_nothing_to_order() {
    let tmp = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(tmp.path()).unwrap();
    let (state_dir, home) = (base.join("state"), base.join("only"));
    for d in [&state_dir, &home] {
        std::fs::create_dir_all(d).unwrap();
    }
    crate::state::with_state_root(&state_dir, || {
        let mut app = App::test_app(home.clone());
        let only = app.state.active;
        app.ready_test_project(only, &home);
        command(&mut app, "order 1");
        let flash = app.flash_text().unwrap_or_default().to_string();
        assert!(
            flash.contains("order") && !flash.contains("unknown"),
            "{flash:?}"
        );
        assert_eq!(app.state.active, only);
    });
}
