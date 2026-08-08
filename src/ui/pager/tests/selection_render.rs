//! Selection *painting* tests: which cells carry the selection background.
//!
//! These assert on resolved cell backgrounds, not glyphs. A glyph-level
//! snapshot is blind to exactly the bug this cluster covers (#120) — the
//! characters are identical whether or not the highlight reaches the end of
//! the row — so the whole cluster reads `Cell::bg`.

use super::*;
use ratatui::style::Color;
use ratatui::{Terminal, backend::TestBackend};

/// Per-row cell backgrounds of a rendered pager, `[row][col]`.
fn render_row_bgs(view: &PagerView, w: u16, h: u16) -> Vec<Vec<Color>> {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    let theme = Theme::default();
    terminal
        .draw(|f| super::super::render(f, Rect::new(0, 0, w, h), view, &theme))
        .unwrap();
    let buf = terminal.backend().buffer().clone();
    (0..h)
        .map(|y| (0..w).map(|x| buf.cell((x, y)).unwrap().bg).collect())
        .collect()
}

/// Borderless, gutterless view so row/col indices map straight onto the
/// content area — the highlight geometry is what's under test, not the chrome.
fn bare_view(lines: &[&str]) -> PagerView {
    let mut view = PagerView::new_plain(
        "sel.txt",
        lines.iter().map(|s| (*s).to_string()).collect::<Vec<_>>(),
    );
    view.full_width = true;
    view.show_line_numbers = false;
    view
}

const W: u16 = 10;

#[test]
fn visual_line_paints_a_blank_row_across_the_full_width() {
    // The reported symptom: a blank line inside a `V` range showed no
    // highlight at all, so the selection read as a column with holes in it.
    let mut view = bare_view(&["aaaa", "", "bbbb"]);
    view.enter_visual();
    view.visual_move(2, 8);
    let bgs = render_row_bgs(&view, W, 4);
    let theme = Theme::default();

    assert!(
        bgs[1].iter().all(|c| *c == theme.cursor_bg_dim),
        "blank row inside the selection should carry the selection bg across \
         the full width, got {:?}",
        bgs[1]
    );
    // The cursor row is the brighter bg, also full width.
    assert!(
        bgs[2].iter().all(|c| *c == theme.cursor_bg),
        "cursor row should be fully painted, got {:?}",
        bgs[2]
    );
}

#[test]
fn visual_line_paints_a_short_rows_tail() {
    // Same defect, less obvious instance: a line shorter than the viewport
    // was highlighted only as far as its text.
    let mut view = bare_view(&["ab", "cd"]);
    view.enter_visual();
    view.visual_move(1, 8);
    let bgs = render_row_bgs(&view, W, 3);
    let theme = Theme::default();

    assert!(
        bgs[0].iter().all(|c| *c == theme.cursor_bg_dim),
        "short row's tail should carry the selection bg, got {:?}",
        bgs[0]
    );
}

#[test]
fn visual_line_leaves_rows_outside_the_range_unpainted() {
    let mut view = bare_view(&["aaaa", "", "bbbb"]);
    view.enter_visual();
    // Selection covers row 0 only; rows 1-2 stay clean.
    let bgs = render_row_bgs(&view, W, 4);
    let theme = Theme::default();

    assert!(bgs[0].iter().all(|c| *c == theme.cursor_bg));
    for (offset, row_bgs) in bgs[1..=2].iter().enumerate() {
        assert!(
            row_bgs.iter().all(|c| *c == Color::Reset),
            "row {} is outside the selection and must not be painted, got {row_bgs:?}",
            offset + 1
        );
    }
}

#[test]
fn visual_line_row_that_already_fills_the_width_is_unchanged() {
    // A fully-populated row needs no padding — every cell was already painted
    // before the fix, and still is, with nothing spilling past the width.
    let mut view = bare_view(&["0123456789", "0123456789"]);
    view.enter_visual();
    view.visual_move(1, 8);
    let bgs = render_row_bgs(&view, W, 3);
    let theme = Theme::default();

    assert_eq!(bgs[0].len(), W as usize);
    assert!(bgs[0].iter().all(|c| *c == theme.cursor_bg_dim));
    assert!(bgs[1].iter().all(|c| *c == theme.cursor_bg));
}

#[test]
fn visual_char_does_not_get_a_full_width_highlight() {
    // Regression guard for the fix above: charwise selects a character range,
    // so a partial row stays partial. Anchor mid-row on the last line of the
    // selection => cells past `to` must stay unpainted.
    let mut view = bare_view(&["aaaaaaaa", "bbbbbbbb"]);
    view.begin_char_selection(0, 0);
    view.extend_char_selection(1, 2);
    let bgs = render_row_bgs(&view, W, 3);

    // Row 1 is selected only through column 2.
    assert!(
        bgs[1][3..].iter().all(|c| *c == Color::Reset),
        "charwise must not fill the row tail, got {:?}",
        bgs[1]
    );
    assert!(
        bgs[1][..=2].iter().all(|c| *c != Color::Reset),
        "charwise selected cells should still be painted, got {:?}",
        bgs[1]
    );
    // Even the interior/first row stops at end-of-text, not end-of-width.
    assert!(
        bgs[0][8..].iter().all(|c| *c == Color::Reset),
        "charwise first row must stop at end-of-text, got {:?}",
        bgs[0]
    );
}

#[test]
fn visual_block_row_is_not_widened_past_its_rectangle() {
    // The other column-bounded kind: `^v` paints a rectangle, so the fill
    // must not leak past `hi_col`.
    let mut view = bare_view(&["aaaaaaaa", "bbbbbbbb"]);
    view.enter_visual_block();
    view.visual_move(1, 8);
    view.visual_col_move(2);
    let bgs = render_row_bgs(&view, W, 3);

    assert!(
        bgs[0][3..].iter().all(|c| *c == Color::Reset),
        "block selection must stay a rectangle, got {:?}",
        bgs[0]
    );
}

#[test]
fn line_placement_preview_fills_the_candidate_row() {
    // The first `V` of the double-tap arm previews the whole candidate row;
    // a blank candidate row should still be visible as the target.
    let mut view = bare_view(&["aaaa", "", "bbbb"]);
    view.enter_placement_line();
    view.placement_move(1, 0, 8);
    let bgs = render_row_bgs(&view, W, 4);
    let theme = Theme::default();

    assert!(
        bgs[1].iter().all(|c| *c == theme.cursor_bg),
        "blank placement row should preview across the full width, got {:?}",
        bgs[1]
    );
}
