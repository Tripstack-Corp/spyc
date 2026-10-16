//! The list's page settle (`stabilize_grid`). After the cursor moves, the page
//! drawn must hold it, and `l` off a page's last column must open the next page
//! at its first column. Reported in a large downloads folder at 238×60;
//! reproduced here with synthetic names.
//!
//! Both cases need neighbouring pages that fit different numbers of columns:
//! `ListView::grid` sizes a page from its own names, so a long name narrows the
//! page it lands on.
#![allow(clippy::unwrap_used)] // test fixtures; the module deny is for production

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, layout::Rect};

use crate::app::App;
use crate::ui::list_view::ListView;

/// The owner's terminal with no pane: a 58-row list.
const BARE: (u16, u16) = (238, 60);
/// The 16-row list the owner had beside their pane.
const BESIDE_PANE: (u16, u16) = (238, 18);

/// Mostly short names with an occasional long one. Each starts with a unique
/// `nNNNN-` to search for.
fn mixed_length_names() -> Vec<String> {
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
    (0..600)
        .map(|i| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let len = if seed.is_multiple_of(12) {
                40 + seed / 12 % 20
            } else {
                8 + seed % 23
            };
            format!("n{i:04}-{}", "x".repeat(len as usize))
        })
        .collect()
}

/// A paneless `App` listing [`mixed_length_names`].
fn listing() -> (App, Vec<String>) {
    let names = mixed_length_names();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let mut app = App::test_app(std::env::temp_dir());
    app.state.active_project_mut().left.listing.dir = PathBuf::from("/projects/demo");
    app.seed_rows(&refs);
    // `seed_rows` doesn't bump `list_generation`, which the row cache is keyed on.
    app.view.project_mut().cached_rows_gen = app
        .state
        .active_project()
        .left
        .list_generation
        .wrapping_sub(1);
    (app, names)
}

fn draw(app: &mut App, (w, h): (u16, u16)) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| app.render(f)).unwrap();
    terminal.backend().buffer().clone()
}

/// One key, then a frame, as the terminal draws between keystrokes.
fn press(app: &mut App, code: KeyCode, size: (u16, u16)) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::empty()))
        .unwrap();
    draw(app, size);
}

/// `/<query><Enter>`, a key at a time.
fn search(app: &mut App, query: &str, size: (u16, u16)) {
    press(app, KeyCode::Char('/'), size);
    for c in query.chars() {
        press(app, KeyCode::Char(c), size);
    }
    press(app, KeyCode::Enter, size);
}

/// Where the list draws its cursor bar, and the name in it.
fn cursor_bar(app: &App, buf: &Buffer) -> Option<(u16, u16, String)> {
    let list = app.frame_layout(buf.area).list;
    let bar = Some(app.view.theme.cursor_bg);
    for y in list.y..list.bottom() {
        let lit: Vec<u16> = (list.x..list.right())
            .filter(|&x| buf.cell((x, y)).map(|c| c.bg) == bar)
            .collect();
        if let (Some(&first), Some(&last)) = (lit.first(), lit.last()) {
            let text: String = (first..=last)
                .map(|x| buf.cell((x, y)).map_or(" ", |c| c.symbol()))
                .collect();
            return Some((first, y, text.trim().to_string()));
        }
    }
    None
}

/// How many columns the page starting at `view_top` fits.
fn page_cols(app: &App, view_top: usize, (w, h): (u16, u16)) -> u16 {
    let list = app.frame_layout(Rect::new(0, 0, w, h)).list;
    ListView {
        rows: &app.view.project().cached_rows,
        cursor: 0,
        view_top,
        empty_marker: false,
        focused: true,
        theme: &app.view.theme,
        selection: None,
    }
    .grid(list)
    .cols
}

/// `/` jumps the cursor to the match, and the list must draw the cursor bar on
/// it. The case: two pages could hold row 240, one from 192 (three columns,
/// ending at 239) and one from 240 (four).
#[test]
fn a_search_jump_draws_the_cursor_bar_on_the_match() {
    let (mut app, names) = listing();
    draw(&mut app, BESIDE_PANE);
    search(&mut app, "n0240-", BESIDE_PANE);
    assert_eq!(
        app.state.active_project().left.cursor.index,
        240,
        "the search found its match"
    );
    assert_eq!(
        (
            page_cols(&app, 192, BESIDE_PANE),
            page_cols(&app, 240, BESIDE_PANE)
        ),
        (3, 4),
        "the case: 16-row columns, three of them from 192 and four from 240",
    );

    let buf = draw(&mut app, BESIDE_PANE);
    let bar = cursor_bar(&app, &buf).map(|(_, _, name)| name);
    assert_eq!(bar.as_deref(), Some(names[240].as_str()));
}

/// `l` from a page's last column moves to the next page, whose first column is
/// the cursor's new one. The case: the page after 348-521 starts at 522 and
/// fits two columns, and a page from 464 would fit three.
#[test]
fn l_off_the_last_column_opens_the_next_page_at_its_first_column() {
    let (mut app, names) = listing();
    draw(&mut app, BARE);
    search(&mut app, "n0464-", BARE);
    let buf = draw(&mut app, BARE);
    let left = &app.state.active_project().left;
    assert_eq!(
        (
            left.cursor.view_top,
            left.grid_dims.cols,
            left.grid_dims.rows_per_col
        ),
        (348, 3, 58),
        "the cursor starts in the last of three 58-row columns, on the page 348-521",
    );
    assert_eq!(
        (page_cols(&app, 522, BARE), page_cols(&app, 464, BARE)),
        (2, 3),
        "the case: the next page, from 522, fits two columns; one from 464 fits three",
    );
    let (_, row, _) = cursor_bar(&app, &buf).unwrap();

    press(&mut app, KeyCode::Char('l'), BARE);
    let buf = draw(&mut app, BARE);
    let first_column = app.frame_layout(buf.area).list.x;
    assert_eq!(
        cursor_bar(&app, &buf),
        Some((first_column, row, names[522].clone()))
    );
}

/// PageDown moves to the same column and row on the next page, or to its last
/// row when it has no such column. The case: from the last of three columns on
/// the page 348-521, the next page, 522-599, has two.
#[test]
fn page_down_to_a_narrower_page_lands_on_its_last_row() {
    let (mut app, names) = listing();
    draw(&mut app, BARE);
    search(&mut app, "n0464-", BARE);
    assert_eq!(
        app.state.active_project().left.cursor.view_top,
        348,
        "the cursor starts on the page 348-521"
    );

    press(&mut app, KeyCode::PageDown, BARE);
    let buf = draw(&mut app, BARE);
    let bar = cursor_bar(&app, &buf).map(|(_, _, name)| name);
    assert_eq!(bar.as_deref(), Some(names[599].as_str()));
}

/// PageUp moves to the same column and row on the page before. The case: from
/// the top of the two-column page at 522 to the top of the three-column page
/// at 348.
#[test]
fn page_up_from_a_pages_first_row_lands_on_the_previous_pages_first_row() {
    let (mut app, names) = listing();
    draw(&mut app, BARE);
    search(&mut app, "n0522-", BARE);
    let buf = draw(&mut app, BARE);
    let first_column = app.frame_layout(buf.area).list.x;
    assert_eq!(
        cursor_bar(&app, &buf).map(|(x, _, _)| x),
        Some(first_column),
        "the cursor starts at the top of its page",
    );

    press(&mut app, KeyCode::PageUp, BARE);
    let buf = draw(&mut app, BARE);
    let (x, _, name) = cursor_bar(&app, &buf).unwrap();
    assert_eq!((x, name.as_str()), (first_column, names[348].as_str()));
}
