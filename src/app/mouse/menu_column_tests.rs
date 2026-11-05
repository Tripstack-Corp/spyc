//! `←`/`→` across a menu's columns. A menu too tall for the frame flows into
//! columns, and the side arrows move its highlight to the entry drawn on the
//! same line of the next or previous column. Each case opens a menu through
//! `handle_key`, moves with the arrows, and reads the highlight as the frame
//! draws it.

use super::make_menu_tests::{app_over, land_make_menu, pager_title, space_m, special, with_mouse};
use super::menu_tests::{app_over_entries, cursor_on};
use super::tests::{drawn, menu_in, press};
use crate::app::App;
use crossterm::event::KeyCode;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use std::fmt::Write as _;

/// A Makefile of `n` targets, `target-01` on, each labelled with its number;
/// the first labelled `first` instead, when given.
fn makefile(n: usize, first: Option<&str>) -> String {
    let mut text = String::new();
    for i in 1..=n {
        let label = match first {
            Some(first) if i == 1 => first.to_string(),
            _ => format!("Label number {i:02}"),
        };
        writeln!(text, "target-{i:02}: ## {label}\n\t@true").unwrap();
    }
    text
}

/// An App over `work/` with the make menu of `n` targets open.
fn make_menu_of(tmp: &std::path::Path, n: usize, first: Option<&str>) -> App {
    let mut app = app_over(&tmp.join("work"), &[("Makefile", &makefile(n, first))]);
    let fx = space_m(&mut app);
    land_make_menu(&mut app, fx);
    app
}

/// The highlighted entry in `menu` as `buf` draws it: where its first reversed
/// cell sits, and the reversed text on that line.
fn lit_in(buf: &Buffer, menu: Rect) -> Option<((u16, u16), String)> {
    for y in menu.y + 1..menu.bottom() - 1 {
        let cells: Vec<u16> = (menu.x + 1..menu.right() - 1)
            .filter(|&x| buf[(x, y)].modifier.contains(Modifier::REVERSED))
            .collect();
        if let Some(&x) = cells.first() {
            let text: String = cells.iter().map(|&x| buf[(x, y)].symbol()).collect();
            return Some(((x, y), text.trim().to_string()));
        }
    }
    None
}

/// The highlight of the menu titled `title`, as drawn, which must be open.
fn lit(app: &mut App, title: &str) -> Option<((u16, u16), String)> {
    assert!(app.view.chord_hint.is_some(), "the menu closed");
    let buf = drawn(app);
    lit_in(&buf, menu_in(&buf, title))
}

/// Where `text` starts in the menu titled `title`, as drawn.
fn drawn_in_menu(app: &mut App, title: &str, text: &str) -> (u16, u16) {
    let buf = drawn(app);
    let menu = menu_in(&buf, title);
    for y in menu.y + 1..menu.bottom() - 1 {
        let line: String = (menu.x..menu.right())
            .map(|x| buf[(x, y)].symbol())
            .collect();
        if let Some(byte) = line.find(text) {
            let col = line[..byte].chars().count();
            return (menu.x + u16::try_from(col).unwrap(), y);
        }
    }
    panic!("{text:?} is not drawn in the menu titled {title:?}");
}

fn press_n(app: &mut App, code: KeyCode, n: usize) {
    for _ in 0..n {
        special(app, code);
    }
}

/// Thirty targets flow into two columns of fifteen. `→` from the second row
/// of the first highlights the second row of the second, drawn on the same
/// line; `←` brings the highlight back, and `Enter` fires the row `→` reached.
#[test]
fn right_and_left_move_along_a_line_across_columns() {
    with_mouse(|tmp| {
        let mut app = make_menu_of(tmp, 30, None);
        press_n(&mut app, KeyCode::Down, 2);
        let before = lit(&mut app, "make").expect("premise: a highlight");
        assert!(
            before.1.starts_with(":make target-02"),
            "premise: {before:?}"
        );

        special(&mut app, KeyCode::Right);

        let ((x, y), text) = lit(&mut app, "make").expect("the menu is open, highlighted");
        assert!(
            text.starts_with(":make target-17"),
            "row 2, column 2: {text}"
        );
        assert_eq!(y, before.0.1, "drawn on the same line");
        assert!(x > before.0.0, "in the column to the right");

        special(&mut app, KeyCode::Left);
        assert_eq!(lit(&mut app, "make"), Some(before), "`←` came back");

        special(&mut app, KeyCode::Right);
        special(&mut app, KeyCode::Enter);
        let title = pager_title(&app).unwrap_or_default();
        assert!(
            title.contains("! make target-17"),
            "`Enter` fired it: {title}"
        );
    });
}

/// Twenty-nine targets flow into fifteen and fourteen. `→` from the first
/// column's last row, below the second column's end, lands on the second
/// column's last entry.
#[test]
fn right_past_a_shorter_columns_end_lands_on_its_last_entry() {
    with_mouse(|tmp| {
        let mut app = make_menu_of(tmp, 29, None);
        press_n(&mut app, KeyCode::Down, 15);
        let ((_, from), text) = lit(&mut app, "make").expect("premise: a highlight");
        assert!(text.starts_with(":make target-15"), "premise: {text}");

        special(&mut app, KeyCode::Right);

        let ((_, y), text) = lit(&mut app, "make").expect("the menu is open, highlighted");
        assert!(
            text.starts_with(":make target-29"),
            "the last entry: {text}"
        );
        assert_eq!(y + 1, from, "the line above, where the column ends");
    });
}

/// The arrows stop at the outer columns: `←` in the first and `→` in the last
/// leave the highlight where it is, and the menu open.
#[test]
fn the_side_arrows_do_not_wrap_at_the_edges() {
    with_mouse(|tmp| {
        let mut app = make_menu_of(tmp, 30, None);
        special(&mut app, KeyCode::Down);
        let first = lit(&mut app, "make").expect("premise: a highlight");
        assert!(first.1.starts_with(":make target-01"), "premise: {first:?}");

        special(&mut app, KeyCode::Left);
        assert_eq!(
            lit(&mut app, "make"),
            Some(first),
            "`←` in the first column"
        );

        special(&mut app, KeyCode::Up);
        let last = lit(&mut app, "make").expect("premise: `↑` wraps to the last");
        assert!(last.1.starts_with(":make target-30"), "premise: {last:?}");

        special(&mut app, KeyCode::Right);
        assert_eq!(lit(&mut app, "make"), Some(last), "`→` in the last column");
    });
}

/// With nothing highlighted, `→` and `←` highlight the first entry, as `↓`
/// does.
#[test]
fn a_side_arrow_with_no_highlight_highlights_the_first_entry() {
    with_mouse(|tmp| {
        for code in [KeyCode::Right, KeyCode::Left] {
            let mut app = make_menu_of(tmp, 30, None);
            assert_eq!(lit(&mut app, "make"), None, "premise: nothing highlighted");

            special(&mut app, code);

            let (_, text) = lit(&mut app, "make").expect("the menu is open, highlighted");
            assert!(text.starts_with(":make target-01"), "{code:?}: {text}");
        }
    });
}

/// A wrapped label takes several lines, so an entry's line in its column is
/// not its place in the list. `→` lands on the entry drawn on the highlight's
/// own line: with the first label on two lines, `target-03` sits on the
/// fourth line, beside `target-19`, not `target-18`.
#[test]
fn right_lands_on_the_line_drawn_beside_a_wrapped_label() {
    with_mouse(|tmp| {
        let long = "A label long enough to wrap onto a second line";
        let mut app = make_menu_of(tmp, 30, Some(long));
        let (_, top) = drawn_in_menu(&mut app, "make", ":make target-01");
        let (_, third) = drawn_in_menu(&mut app, "make", ":make target-03");
        assert_eq!(third, top + 3, "premise: the first label takes two lines");
        press_n(&mut app, KeyCode::Down, 3);

        special(&mut app, KeyCode::Right);

        let ((_, y), text) = lit(&mut app, "make").expect("the menu is open, highlighted");
        assert!(text.starts_with(":make target-19"), "{text}");
        assert_eq!(y, third, "drawn on the same line");
    });
}

/// A menu of one column has nowhere to go sideways: `→` and `←` move nothing
/// and the menu stays open, with or without a highlight, and nothing beneath
/// it moves either.
#[test]
fn a_one_column_menu_ignores_the_side_arrows() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = app_over_entries(tmp.path());
        cursor_on(&mut app, "notes.txt");
        let cursor = app.state.cur().cursor.index;
        press(&mut app, '.');
        let entries = app.view.chord_hint.as_ref().map_or(0, |h| h.entries.len());
        let buf = drawn(&mut app);
        let menu = menu_in(&buf, "notes.txt");
        assert!(
            usize::from(menu.height - 2) >= entries,
            "premise: one column holds all {entries} entries"
        );

        for code in [KeyCode::Right, KeyCode::Left] {
            special(&mut app, code);
            assert!(app.view.chord_hint.is_some(), "{code:?} kept the menu open");
            assert_eq!(
                lit(&mut app, "notes.txt"),
                None,
                "{code:?} highlighted nothing"
            );
        }
        special(&mut app, KeyCode::Down);
        let first = lit(&mut app, "notes.txt").expect("premise: `↓` highlights");
        for code in [KeyCode::Right, KeyCode::Left] {
            special(&mut app, code);
            assert_eq!(lit(&mut app, "notes.txt"), Some(first.clone()), "{code:?}");
        }
        assert_eq!(app.state.cur().cursor.index, cursor, "the cursor stayed");
        assert!(app.view.project().pager.is_none(), "nothing opened");
    });
}
