//! The menus of the two dividers. A right-click on the pane divider, off the
//! tab labels, opens the `pane tabs` menu, and one on the split's vertical
//! divider opens the `split` menu. Each case goes through `handle_mouse` and
//! reads the menu as the frame draws it.

use super::menu_tests::{app_with_tab, ctrl_a, cursor_on, flash, menu_lines, offers, right_click};
use super::route::{Region, region_at};
use super::tab_hit::tab_at_point;
use super::tests::{FRAME, click, drawn, drawn_row, press};
use crate::app::App;
use crate::app::state::{Side, VsplitMode, ZoomTarget};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::Path;

const TABS_MENU: &str = "pane tabs";
const SPLIT_MENU: &str = "split";

/// Run `body` over a fresh state root, with mouse reporting on, as a click
/// needs it.
fn with_mouse(body: impl FnOnce(&Path)) {
    let _lock = crate::mouse_test_lock();
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        crate::set_mouse_capture_for_test(true);
        body(tmp.path());
        crate::set_mouse_capture_for_test(false);
    });
}

/// An App over `work/` with a pane tab running `cat` for each of `tabs`, the
/// keyboard in the file list.
fn app_with_tabs(tmp: &Path, tabs: usize) -> App {
    let mut app = app_with_tab(tmp, "cat");
    for _ in 1..tabs {
        app.open_pane_tab("cat");
    }
    app.set_pane_focus(false);
    assert!(
        !app.state.pane_focused(),
        "premise: the file list has the keyboard"
    );
    app
}

/// A point on the divider's rule past the last tab, as drawn.
fn past_the_tabs(app: &mut App) -> (u16, u16) {
    let buf = drawn(app);
    let divider = app.frame_layout(FRAME).divider.expect("a divider");
    let point = (divider.right() - 2, divider.y);
    let widths = app.divider_tab_widths(divider.width);
    assert_eq!(
        tab_at_point(Some(divider), &widths, point.0, point.1),
        None,
        "premise: no tab label there"
    );
    assert_eq!(buf[point].symbol(), "─", "premise: the rule is drawn there");
    point
}

/// Where `text` is drawn on the divider row.
fn on_the_divider(app: &mut App, text: &str) -> Option<(u16, u16)> {
    let buf = drawn(app);
    let divider = app.frame_layout(FRAME).divider.expect("a divider");
    let cells: Vec<String> = (divider.x..divider.right())
        .map(|x| buf[(x, divider.y)].symbol().to_string())
        .collect();
    let byte = cells.concat().find(text)?;
    let mut at = 0;
    for (i, cell) in cells.iter().enumerate() {
        if at == byte {
            return Some((divider.x + u16::try_from(i).unwrap(), divider.y));
        }
        at += cell.len();
    }
    None
}

fn active_tab(app: &App) -> usize {
    app.runtime
        .project()
        .pane_tabs
        .as_ref()
        .expect("tabs")
        .active_index()
}

fn ctrl_s(app: &mut App) {
    app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL))
        .unwrap();
}

/// The split's shape, which `^a f` flips.
fn split_mode(app: &App) -> Option<VsplitMode> {
    app.state.active_project().vsplit.map(|v| v.mode)
}

/// A point on the split's vertical divider, as the layout places it.
fn on_the_split(app: &mut App) -> (u16, u16) {
    drawn(app);
    let layout = app.frame_layout(FRAME);
    let line = layout.vdivider.expect("premise: the split is open");
    let point = (line.x, line.y + 1);
    assert_eq!(
        region_at(&layout, point.0, point.1),
        Some(Region::VDivider),
        "premise: the point is on the split's divider"
    );
    point
}

/// An App with the split open on `notes.txt` by `^a |`, the keyboard in the
/// file list.
fn app_with_split(tmp: &Path) -> App {
    let mut app = app_with_tabs(tmp, 1);
    cursor_on(&mut app, "notes.txt");
    ctrl_a(&mut app);
    press(&mut app, '|');
    assert!(
        split_mode(&app).is_some(),
        "premise: `^a |` opened the split"
    );
    app
}

// ── the pane divider ──

/// A right-click on the divider past the last tab opens the tabs' menu and
/// gives the pane the keyboard, so the `^a` keys it shows act on the pane.
/// With one tab there is no other tab to step to.
#[test]
fn a_right_click_past_the_last_tab_opens_the_pane_tabs_menu() {
    with_mouse(|tmp| {
        let mut app = app_with_tabs(tmp, 1);
        let point = past_the_tabs(&mut app);

        right_click(&mut app, point);

        let menu = menu_lines(&mut app, TABS_MENU);
        for row in [
            "^a c  new pane tab",
            "^a z  zoom pane (toggle fullscreen)",
            "^a +  grow pane",
            "^a -  shrink pane",
        ] {
            assert!(offers(&menu, row), "{row:?}: {menu:#?}");
        }
        for row in ["next pane tab", "prev pane tab"] {
            assert!(!offers(&menu, row), "{row:?} with one tab: {menu:#?}");
        }
        assert!(app.state.pane_focused(), "the pane has the keyboard");
        assert!(
            !app.state.resolver.is_pending(),
            "no chord is armed: this is the tabs' menu, not the leader"
        );
    });
}

/// With two tabs the menu steps between them.
#[test]
fn with_two_tabs_the_divider_menu_steps_between_them() {
    with_mouse(|tmp| {
        let mut app = app_with_tabs(tmp, 2);
        let point = past_the_tabs(&mut app);

        right_click(&mut app, point);

        let menu = menu_lines(&mut app, TABS_MENU);
        assert!(offers(&menu, "^a n  next pane tab"), "{menu:#?}");
        assert!(offers(&menu, "^a p  prev pane tab"), "{menu:#?}");
    });
}

/// A click on the menu's zoom row zooms the pane, not the list the keyboard
/// was in before the right-click.
#[test]
fn a_click_on_the_divider_menus_zoom_row_zooms_the_pane() {
    with_mouse(|tmp| {
        let mut app = app_with_tabs(tmp, 1);
        let point = past_the_tabs(&mut app);
        right_click(&mut app, point);
        let row = drawn_row(&mut app, TABS_MENU, "zoom pane");

        click(&mut app, row);

        assert_eq!(
            app.state.active_project().pane.zoom,
            ZoomTarget::BottomPane,
            "the row zoomed the pane"
        );
        assert!(app.view.chord_hint.is_none(), "and the menu closed");
    });
}

/// The active tab's cwd, drawn on the divider after the tabs, is no tab label:
/// a right-click there opens the tabs' menu too.
#[test]
fn a_right_click_on_the_dividers_cwd_opens_the_pane_tabs_menu() {
    with_mouse(|tmp| {
        let mut app = app_with_tabs(tmp, 1);
        let point = on_the_divider(&mut app, "work").expect("premise: the cwd is drawn");
        let divider = app.frame_layout(FRAME).divider.expect("a divider");
        let widths = app.divider_tab_widths(divider.width);
        assert_eq!(
            tab_at_point(Some(divider), &widths, point.0, point.1),
            None,
            "premise: no tab label there"
        );

        right_click(&mut app, point);

        let menu = menu_lines(&mut app, TABS_MENU);
        assert!(offers(&menu, "^a c  new pane tab"), "{menu:#?}");
    });
}

/// A row clicked in the tabs' menu names its key, as a which-key row does:
/// `^a n` steps to the next tab and says so.
#[test]
fn a_divider_menu_row_names_its_key() {
    with_mouse(|tmp| {
        let mut app = app_with_tabs(tmp, 2);
        let before = active_tab(&app);
        let point = past_the_tabs(&mut app);
        right_click(&mut app, point);
        let row = drawn_row(&mut app, TABS_MENU, "next pane tab");

        click(&mut app, row);

        assert_ne!(active_tab(&app), before, "the row stepped to the next tab");
        assert_eq!(flash(&app).as_deref(), Some("^a n  next pane tab"));
    });
}

// ── the split's vertical divider ──

/// A right-click on the split's divider opens the split's menu and leaves the
/// keyboard where it was; a click on its height row flips the split. A preview
/// has no second commander to close, so the menu offers no `^s x`.
#[test]
fn a_right_click_on_the_split_opens_its_menu_and_f_flips_its_height() {
    with_mouse(|tmp| {
        let mut app = app_with_split(tmp);
        let shape = split_mode(&app);
        let point = on_the_split(&mut app);

        right_click(&mut app, point);

        let menu = menu_lines(&mut app, SPLIT_MENU);
        for row in [
            "^a |  vertical split: open / close",
            "^a f  vertical split: full-height / top-only",
            "^a h  focus left pane (a)",
            "^a l  focus right pane (b)",
            "^a d  toggle dimming of the inactive pane",
        ] {
            assert!(offers(&menu, row), "{row:?}: {menu:#?}");
        }
        assert!(
            !offers(&menu, "close the second file-commander"),
            "no commander to close beside a preview: {menu:#?}"
        );
        assert!(!app.state.pane_focused(), "the list keeps the keyboard");

        let row = drawn_row(&mut app, SPLIT_MENU, "full-height / top-only");
        click(&mut app, row);

        let flipped = split_mode(&app);
        assert!(flipped.is_some(), "the split stays open");
        assert_ne!(flipped, shape, "the row flipped the split's height");
        assert!(app.view.chord_hint.is_none(), "and the menu closed");
    });
}

/// With a second commander in the right column, the menu's `^s x` row closes
/// it, and the menu offers no `^a |`, which refuses while a commander is open.
#[test]
fn the_split_menu_closes_a_second_commander() {
    with_mouse(|tmp| {
        let mut app = app_with_tabs(tmp, 1);
        ctrl_s(&mut app);
        press(&mut app, 'n');
        assert!(
            app.state.has_col(Side::Right),
            "premise: `^s n` opened a second commander"
        );
        let point = on_the_split(&mut app);
        right_click(&mut app, point);
        let menu = menu_lines(&mut app, SPLIT_MENU);
        assert!(
            offers(&menu, "^s x (list)  close the second file-commander"),
            "{menu:#?}"
        );
        assert!(
            !offers(&menu, "vertical split: open / close"),
            "`^a |` refuses beside a commander: {menu:#?}"
        );
        assert!(
            offers(&menu, "^a f  vertical split: full-height / top-only"),
            "the other split rows stay: {menu:#?}"
        );
        let row = drawn_row(&mut app, SPLIT_MENU, "close the second file-commander");

        click(&mut app, row);

        assert!(!app.state.has_col(Side::Right), "the commander closed");
    });
}

/// A row clicked in the split's menu names its key too. From the pane the
/// split menu leaves the keyboard there, where `^a h` moves nothing and so
/// says nothing of its own.
#[test]
fn a_split_menu_row_names_its_key() {
    with_mouse(|tmp| {
        let mut app = app_with_split(tmp);
        app.set_pane_focus(true);
        assert!(
            app.state.pane_focused(),
            "premise: the pane has the keyboard"
        );
        app.state.flash = None;
        let point = on_the_split(&mut app);
        right_click(&mut app, point);
        assert!(app.state.pane_focused(), "the pane keeps the keyboard");
        let row = drawn_row(&mut app, SPLIT_MENU, "focus left pane (a)");

        click(&mut app, row);

        assert_eq!(flash(&app).as_deref(), Some("^a h  focus left pane (a)"));
    });
}
