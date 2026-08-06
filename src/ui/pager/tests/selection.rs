//! Pager visual / block / placement selection + state-clamp tests,
//! split out of `pager/tests.rs` (800-LoC campaign). Behavior-identical;
//! relocated verbatim.

use super::*;

#[test]
fn enter_visual_anchors_at_top_visible_line() {
    let mut view = sample_view();
    view.scroll = 5;
    view.enter_visual();
    let sel = view.visual.expect("should be in visual mode");
    assert_eq!(sel.anchor, 5);
    assert_eq!(sel.cursor, 5);
    assert!(view.is_visual());
}

#[test]
fn enter_visual_on_empty_buffer_is_noop() {
    let mut view = PagerView::new_plain("v", Vec::<String>::new());
    view.enter_visual();
    assert!(view.visual.is_none());
}

#[test]
fn visual_move_extends_cursor_and_clamps() {
    let mut view = sample_view();
    view.enter_visual();
    view.visual_move(3, 10);
    assert_eq!(view.visual.unwrap().cursor, 3);
    // Clamp at the bottom — buffer has 20 lines (idx 0..=19).
    view.visual_move(100, 10);
    assert_eq!(view.visual.unwrap().cursor, 19);
    // And at the top.
    view.visual_move(-100, 10);
    assert_eq!(view.visual.unwrap().cursor, 0);
    // Anchor is unchanged through movement.
    assert_eq!(view.visual.unwrap().anchor, 0);
}

#[test]
fn visual_range_is_inclusive_and_order_independent() {
    let sel = VisualSelection {
        anchor: 5,
        cursor: 10,
        anchor_col: 0,
        cursor_col: 0,
        kind: VisualKind::Line,
    };
    assert_eq!(sel.range(), (5, 10));
    let sel = VisualSelection {
        anchor: 10,
        cursor: 5,
        anchor_col: 0,
        cursor_col: 0,
        kind: VisualKind::Line,
    };
    // Cursor moved up past the anchor — range still goes low → high.
    assert_eq!(sel.range(), (5, 10));
}

#[test]
fn visual_move_auto_scrolls_when_cursor_leaves_viewport() {
    let mut view = sample_view();
    view.scroll = 0;
    view.enter_visual();
    // Viewport = 5 rows. Move cursor past the bottom edge — scroll
    // should advance so the cursor stays visible.
    view.visual_move(7, 5);
    assert_eq!(view.visual.unwrap().cursor, 7);
    // cursor=7, vh=5 → scroll = 7 + 1 - 5 = 3
    assert_eq!(view.scroll, 3);
    // Move back up past the top — scroll should retreat.
    view.visual_move(-7, 5);
    assert_eq!(view.visual.unwrap().cursor, 0);
    assert_eq!(view.scroll, 0);
}

#[test]
fn visual_jump_to_clamps_and_scrolls() {
    let mut view = sample_view();
    view.enter_visual();
    view.visual_jump_to(15, 5);
    assert_eq!(view.visual.unwrap().cursor, 15);
    assert_eq!(view.scroll, 11);
    // Beyond the end is clamped.
    view.visual_jump_to(999, 5);
    assert_eq!(view.visual.unwrap().cursor, 19);
}

#[test]
fn clamp_state_to_lines_clamps_visual_past_end() {
    // A selection made when the buffer was long, then the buffer shrank
    // under it (streaming task viewer front-trim).
    let mut view = PagerView::new_plain("v", vec!["a".to_string(), "b".to_string()]);
    view.visual = Some(VisualSelection {
        anchor: 10,
        cursor: 15,
        anchor_col: 0,
        cursor_col: 0,
        kind: VisualKind::Line,
    });
    view.clamp_state_to_lines();
    let sel = view.visual.unwrap();
    assert_eq!(
        (sel.anchor, sel.cursor),
        (1, 1),
        "clamped to last valid row"
    );
}

#[test]
fn clamp_state_to_lines_drops_state_on_empty_buffer() {
    let mut view = PagerView::new_plain("v", Vec::<String>::new());
    view.visual = Some(VisualSelection {
        anchor: 3,
        cursor: 5,
        anchor_col: 0,
        cursor_col: 0,
        kind: VisualKind::Line,
    });
    view.clamp_state_to_lines();
    assert!(view.visual.is_none(), "selection dropped on empty buffer");
}

#[test]
fn clamp_scroll_auto_pulls_scroll_back_from_past_end() {
    // A `:N` jump past EOF or a `|` layout toggle that shrinks the line
    // count can leave `scroll` past the document end, which renders as a
    // blank viewport. clamp_scroll_auto (using the last render's viewport
    // height) must pull it back to scroll_max.
    let mut view = PagerView::new_plain("v", (0..5).map(|i| format!("line {i}")).collect());
    // 5 lines in a 3-row viewport: content-pin max 2, +1 for the [EOF] row → 3.
    view.last_viewport_h.set(3);
    view.scroll = 999; // jumped well past the end
    view.clamp_scroll_auto();
    assert_eq!(
        view.scroll, 3,
        "scroll must be clamped to scroll_max, not left past the end"
    );
}

#[test]
fn clamp_scroll_auto_leaves_in_range_scroll_untouched() {
    let mut view = PagerView::new_plain("v", (0..10).map(|i| format!("line {i}")).collect());
    view.last_viewport_h.set(4);
    view.scroll = 3; // within [0, scroll_max]
    view.clamp_scroll_auto();
    assert_eq!(view.scroll, 3, "an already-valid scroll is left alone");
}

#[test]
fn yank_visual_past_end_clamps_instead_of_panicking() {
    // Selection sits entirely past the (shrunk) buffer: range() returns
    // lo=10,hi=15 but len=3. Pre-fix this slice panicked.
    let mut view = PagerView::new_plain(
        "v",
        vec![
            "line 0".to_string(),
            "line 1".to_string(),
            "line 2".to_string(),
        ],
    );
    view.visual = Some(VisualSelection {
        anchor: 10,
        cursor: 15,
        anchor_col: 0,
        cursor_col: 0,
        kind: VisualKind::Line,
    });

    let result = view.visual_yank_text(false);
    let (text, n, _in_block) = result.expect("clamped selection should extract text");
    assert_eq!(n, 1, "clamped to the single last line");
    assert!(text.contains("line 2"), "yanked the clamped tail: {text:?}");
    assert!(!text.contains("line 0"));
}

#[test]
fn cancel_visual_clears_state() {
    let mut view = sample_view();
    view.enter_visual();
    assert!(view.is_visual());
    view.cancel_visual();
    assert!(!view.is_visual());
}

#[test]
fn visual_move_outside_visual_mode_is_noop() {
    let mut view = sample_view();
    view.scroll = 4;
    view.visual_move(5, 10);
    // No selection started, no scroll change.
    assert!(view.visual.is_none());
    assert_eq!(view.scroll, 4);
}

#[test]
fn visual_status_text_reports_range_and_count() {
    let mut view = sample_view();
    view.enter_visual();
    view.visual_move(4, 10);
    let s = view.status_text().expect("status while visual");
    assert!(s.contains("VISUAL"), "expected VISUAL marker, got: {s}");
    assert!(s.contains("L1-L5"), "expected L1-L5, got: {s}");
    assert!(s.contains("5 lines"), "expected count, got: {s}");
}

#[test]
fn visual_status_pluralizes_correctly_for_single_line() {
    let mut view = sample_view();
    view.enter_visual();
    // anchor == cursor → single-line range.
    let s = view.status_text().expect("status while visual");
    assert!(s.contains("(1 line)"), "expected singular, got: {s}");
}

// ── v1.5 Phase 4: visual block (columnar) mode ─────────────────

fn block_view_with(content: &[&str]) -> PagerView {
    PagerView::new_plain("v", content.iter().map(|&s| s.to_string()).collect())
}

#[test]
fn placement_move_then_commit_anchors_at_cursor() {
    let mut view = block_view_with(&["abcdef", "ghi jkl", "mnopqr"]);
    view.enter_placement();
    let p = view.placement.expect("placement active");
    assert_eq!((p.row, p.col), (0, 0));
    // hjkl-style motion: down 1, right 2.
    view.placement_move(1, 2, 5);
    // Word forward from "ghi jkl" col 2 ('i') → 'j' at col 4.
    view.placement_word_forward();
    let p = view.placement.expect("still placement");
    assert_eq!((p.row, p.col), (1, 4));
    // Second ^v commits to block visual at the cursor.
    view.commit_placement_to_visual_block();
    assert!(view.placement.is_none(), "placement consumed on commit");
    let sel = view.visual.expect("block visual");
    assert_eq!(sel.kind, VisualKind::Block);
    assert_eq!(sel.anchor, 1);
    assert_eq!(sel.cursor, 1);
    assert_eq!(sel.anchor_col, 4);
    assert_eq!(sel.cursor_col, 4);
}

#[test]
fn placement_uppercase_v_commits_to_line_at_cursor_row() {
    let mut view = block_view_with(&["aaa", "bbb", "ccc"]);
    view.enter_placement();
    view.placement_move(2, 0, 5);
    view.commit_placement_to_visual_line();
    let sel = view.visual.expect("line visual");
    assert_eq!(sel.kind, VisualKind::Line);
    assert_eq!(sel.anchor, 2);
    assert_eq!(sel.cursor, 2);
}

#[test]
fn v_arm_places_a_line_cursor_then_anchors_at_the_chosen_line() {
    // The double-tap `V`: first `V` arms a Line placement (no selection
    // yet), motions move the cursor to the exact start line, second `V`
    // anchors the line visual there.
    let mut view = block_view_with(&["aaa", "bbb", "ccc", "ddd"]);
    view.enter_placement_line();
    let p = view.placement.expect("line placement armed");
    assert_eq!(p.kind, VisualKind::Line, "armed as a Line cursor");
    assert_eq!((p.row, p.col), (0, 0));
    assert!(view.visual.is_none(), "no selection until the second V");
    // Move down to the desired start line, then anchor.
    view.placement_move(2, 0, 5);
    view.commit_placement_to_visual_line();
    assert!(view.placement.is_none(), "placement consumed on commit");
    let sel = view.visual.expect("line visual armed");
    assert_eq!(sel.kind, VisualKind::Line);
    assert_eq!(
        (sel.anchor, sel.cursor),
        (2, 2),
        "anchored at the chosen line"
    );
}

#[test]
fn picker_move_autoscrolls_via_shared_keep_visible() {
    let mut view = block_view_with(&["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"]);
    view.picker_cursor = Some(0);
    view.scroll = 0;
    // Jump to line 7 with a 5-row viewport: bottom-align so 7 is visible
    // (scroll = 7 + 1 - 5 = 3).
    view.picker_move(7, 5);
    assert_eq!(view.picker_cursor, Some(7));
    assert_eq!(view.scroll, 3);
    // Back up to line 1: top-align (scroll = 1).
    view.picker_move(-6, 5);
    assert_eq!(view.picker_cursor, Some(1));
    assert_eq!(view.scroll, 1);
    // Degenerate zero-height viewport: cursor still moves, scroll is left
    // alone (the shared keep-visible guard the inline version lacked).
    view.picker_move(1, 0);
    assert_eq!(view.picker_cursor, Some(2));
    assert_eq!(view.scroll, 1);
}

#[test]
fn placement_esc_clears_without_starting_visual() {
    let mut view = block_view_with(&["a", "b"]);
    view.enter_placement();
    view.placement_move(1, 0, 5);
    view.cancel_placement();
    assert!(view.placement.is_none());
    assert!(view.visual.is_none());
}

#[test]
fn enter_visual_block_starts_in_block_mode() {
    let mut view = block_view_with(&["abc", "def", "ghi"]);
    view.enter_visual_block();
    let sel = view.visual.expect("visual active");
    assert_eq!(sel.kind, VisualKind::Block);
    assert_eq!(sel.anchor_col, 0);
    assert_eq!(sel.cursor_col, 0);
}

#[test]
fn enter_visual_block_upgrades_existing_line_visual() {
    let mut view = block_view_with(&["abcdef", "ghijkl", "mnopqr"]);
    view.enter_visual();
    view.visual_move(2, 5);
    let pre = view.visual.expect("line visual");
    assert_eq!(pre.kind, VisualKind::Line);
    view.enter_visual_block();
    let post = view.visual.expect("block visual");
    assert_eq!(post.kind, VisualKind::Block);
    // Anchor / cursor lines preserved through the upgrade.
    assert_eq!(post.anchor, pre.anchor);
    assert_eq!(post.cursor, pre.cursor);
}

#[test]
fn col_range_is_inclusive_and_order_independent() {
    let sel = VisualSelection {
        anchor: 0,
        cursor: 0,
        anchor_col: 2,
        cursor_col: 7,
        kind: VisualKind::Block,
    };
    assert_eq!(sel.col_range(), (2, 7));
    let sel = VisualSelection {
        anchor: 0,
        cursor: 0,
        anchor_col: 7,
        cursor_col: 2,
        kind: VisualKind::Block,
    };
    // Cursor moved left past anchor — range still goes low→high.
    assert_eq!(sel.col_range(), (2, 7));
}

#[test]
fn visual_col_move_extends_and_clamps_at_zero() {
    let mut view = block_view_with(&["abcdef"]);
    view.enter_visual_block();
    view.visual_col_move(3);
    assert_eq!(view.visual.unwrap().cursor_col, 3);
    // Clamp at 0 on the left.
    view.visual_col_move(-100);
    assert_eq!(view.visual.unwrap().cursor_col, 0);
    // Anchor unchanged.
    assert_eq!(view.visual.unwrap().anchor_col, 0);
}

#[test]
fn visual_col_move_is_noop_outside_block_mode() {
    // Line mode: visual_col_move must not touch the cursor_col
    // (it's stored but ignored, by design).
    let mut view = block_view_with(&["abcdef"]);
    view.enter_visual();
    view.visual_col_move(3);
    assert_eq!(view.visual.unwrap().cursor_col, 0);
}

#[test]
fn block_yank_extracts_rectangular_slice() {
    // 4-line CSV-ish grid, yank a 3×3 rectangle (rows 0..=2,
    // cols 1..=3) → "bcd / fgh / jkl".
    let mut view = block_view_with(&["abcde", "efghi", "ijklm", "mnopq"]);
    view.enter_visual_block();
    view.visual_move(2, 5); // rows 0..=2
    view.visual_col_move(3); // cols 0..=3 inclusive...
    // Wait: anchor_col=0, cursor_col=3 → col_range = (0,3) → 4 chars
    // So yank picks chars 0..=3 of each row.
    let sel = view.visual.unwrap();
    let (lo_col, hi_col) = sel.col_range();
    assert_eq!((lo_col, hi_col), (0, 3));
    // We can't exercise the system-clipboard side from a unit test, but
    // the slice math is what we want to verify. Reproduce the
    // same logic the yank uses:
    let plain: Vec<String> = view
        .lines
        .iter()
        .take(3)
        .map(|l| {
            line_plain_text(l)
                .chars()
                .skip(lo_col)
                .take(hi_col + 1 - lo_col)
                .collect()
        })
        .collect();
    assert_eq!(plain, vec!["abcd", "efgh", "ijkl"]);
}

#[test]
fn block_yank_handles_short_rows_gracefully() {
    // The middle row is shorter than the column range — yank
    // takes whatever chars are available and stops, doesn't
    // pad or panic.
    let mut view = block_view_with(&["abcdefgh", "xy", "1234567"]);
    view.enter_visual_block();
    view.visual_move(2, 5);
    view.visual_col_move(5); // col_range = (0, 5) → 6 chars wanted

    let sel = view.visual.unwrap();
    let (lo_col, hi_col) = sel.col_range();
    let plain: Vec<String> = view
        .lines
        .iter()
        .take(3)
        .map(|l| {
            line_plain_text(l)
                .chars()
                .skip(lo_col)
                .take(hi_col + 1 - lo_col)
                .collect()
        })
        .collect();
    assert_eq!(plain, vec!["abcdef", "xy", "123456"]);
}

#[test]
fn block_status_text_reports_rect_dimensions() {
    let mut view = block_view_with(&["abcdef", "ghijkl", "mnopqr"]);
    view.enter_visual_block();
    view.visual_move(2, 5);
    view.visual_col_move(3);
    let s = view.status_text().expect("status while visual block");
    assert!(s.contains("VISUAL BLOCK"), "got: {s}");
    assert!(s.contains("L1-L3"), "got: {s}");
    assert!(s.contains("C1-C4"), "got: {s}");
    assert!(s.contains("(3×4)"), "got: {s}");
}

#[test]
fn block_range_stays_inclusive_when_anchor_higher_than_cursor() {
    // Direct construction so we can pin both axes — the
    // public API only ever sets `anchor_col = 0` at entry.
    // Anchor at (line 5, col 7), cursor dragged up-and-left
    // to (line 2, col 3). Both range helpers must still
    // return low → high so the renderer and yank get a
    // sensible rectangle.
    let sel = VisualSelection {
        anchor: 5,
        cursor: 2,
        anchor_col: 7,
        cursor_col: 3,
        kind: VisualKind::Block,
    };
    assert_eq!(sel.range(), (2, 5));
    assert_eq!(sel.col_range(), (3, 7));
}

// ── charwise (`VisualKind::Char`) ─────────────────────────────────────

/// Build a view with known, non-uniform content for charwise slicing.
fn charwise_view() -> PagerView {
    PagerView::new_plain(
        "c",
        vec![
            "first line".to_string(),
            "second".to_string(),
            "third line here".to_string(),
        ],
    )
}

fn char_sel(view: &mut PagerView, anchor: (usize, usize), cursor: (usize, usize)) {
    view.visual = Some(VisualSelection {
        anchor: anchor.0,
        anchor_col: anchor.1,
        cursor: cursor.0,
        cursor_col: cursor.1,
        kind: VisualKind::Char,
    });
}

/// The headline: a charwise selection takes the first line from its start
/// column, whole lines between, and the last line up to its end column.
#[test]
fn charwise_yank_spans_partial_first_and_last_lines() {
    let mut view = charwise_view();
    // "first line" from col 6 ("line") through "third" on line 2 (col 4).
    char_sel(&mut view, (0, 6), (2, 4));
    let (text, count, block) = view.visual_yank_text(false).expect("charwise yanks");
    assert_eq!(text, "line\nsecond\nthird");
    assert_eq!(count, 3);
    assert!(!block, "charwise is not block mode");
}

/// A single-line charwise selection is the intersection of both column bounds,
/// not "col..end of line".
#[test]
fn charwise_yank_within_one_line_is_bounded_both_ends() {
    let mut view = charwise_view();
    char_sel(&mut view, (2, 6), (2, 9));
    let (text, ..) = view.visual_yank_text(false).expect("charwise yanks");
    assert_eq!(text, "line");
}

/// A backwards drag must keep each column attached to its own line.
///
/// This is what `range()` + `col_range()` get wrong: ordering the axes
/// independently would start at col 4 on line 0, selecting "st line…" — text the
/// pointer never crossed. Dragging up-and-right is the case that exposes it,
/// because the low line carries the HIGH column.
#[test]
fn charwise_backwards_drag_orders_by_line_col_pair_not_per_axis() {
    let mut view = charwise_view();
    // Anchor late on line 2, drag back to col 4 of line 0 → same selection as
    // anchoring at (0,4) and dragging to (2,4).
    char_sel(&mut view, (2, 4), (0, 4));
    let (backwards, ..) = view.visual_yank_text(false).expect("charwise yanks");

    let mut fwd = charwise_view();
    char_sel(&mut fwd, (0, 4), (2, 4));
    let (forwards, ..) = fwd.visual_yank_text(false).expect("charwise yanks");

    assert_eq!(
        backwards, forwards,
        "drag direction must not change the text"
    );
    assert_eq!(backwards, "t line\nsecond\nthird");
}

/// Line numbers and whitespace markers are added in `render.rs`, never stored in
/// `lines`, so toggling them cannot change what a selection copies. The spec's
/// "the content and nothing else" depends on this staying true.
#[test]
fn charwise_yank_ignores_gutter_and_whitespace_decoration() {
    let mut view = charwise_view();
    char_sel(&mut view, (0, 0), (0, 4));
    let plain = view.visual_yank_text(false).expect("charwise yanks").0;

    let mut decorated = charwise_view();
    decorated.show_line_numbers = true;
    decorated.show_whitespace = true;
    char_sel(&mut decorated, (0, 0), (0, 4));
    let with_chrome = decorated.visual_yank_text(false).expect("charwise yanks").0;

    assert_eq!(plain, "first");
    assert_eq!(
        with_chrome, plain,
        "decoration must not reach the clipboard"
    );
}

/// Grid-padded content would otherwise paste a ragged block of spaces.
#[test]
fn charwise_yank_trims_trailing_whitespace_per_line() {
    let mut view = PagerView::new_plain("c", vec!["padded   ".to_string(), "b".to_string()]);
    char_sel(&mut view, (0, 0), (1, 0));
    let (text, ..) = view.visual_yank_text(false).expect("charwise yanks");
    assert_eq!(text, "padded\nb");
}

/// Out-of-range rows are clamped rather than panicking — same contract the
/// Line/Block arms have, and reachable when a streaming view front-trims.
#[test]
fn charwise_yank_clamps_rows_past_the_end() {
    let mut view = charwise_view();
    char_sel(&mut view, (0, 0), (99, 3));
    let (text, ..) = view.visual_yank_text(false).expect("charwise yanks");
    assert!(text.starts_with("first line"), "got {text:?}");
}

// ── pointer hit-test ──────────────────────────────────────────────────

/// Put the view in the state a render would have left it: content at `area`,
/// wrap width recorded, and the gutter explicitly OFF.
///
/// `show_line_numbers` defaults to `true`, so without turning it off a press at
/// the content rect's left edge lands on the gutter and correctly selects
/// nothing. Stating it here keeps each test's column arithmetic obvious;
/// `hit_test_excludes_the_gutter_and_offsets_columns_past_it` turns it back on.
fn drawn(view: &mut PagerView, x: u16, y: u16, w: u16, h: u16) {
    view.show_line_numbers = false;
    view.last_content_area
        .set(ratatui::layout::Rect::new(x, y, w, h));
    view.last_body_w.set(w);
}

#[test]
fn hit_test_maps_absolute_screen_position_to_line_and_col() {
    let mut view = charwise_view();
    view.wrap = false;
    drawn(&mut view, 10, 5, 40, 3);
    // Top-left of the content rect is line 0, col 0.
    assert_eq!(view.hit_test(10, 5), Some((0, 0)));
    // Third row down, four cells in.
    assert_eq!(view.hit_test(14, 7), Some((2, 4)));
}

/// A never-rendered view has no geometry, so no position names a character.
#[test]
fn hit_test_returns_none_before_the_first_render() {
    let view = charwise_view();
    assert_eq!(view.hit_test(0, 0), None);
}

#[test]
fn hit_test_rejects_positions_outside_the_content_rect() {
    let mut view = charwise_view();
    drawn(&mut view, 10, 5, 40, 3);
    for (col, row) in [(9, 5), (10, 4), (50, 5), (10, 8)] {
        assert_eq!(view.hit_test(col, row), None, "({col},{row}) is outside");
    }
}

/// The gutter is chrome, not content — a press on the digits selects nothing.
/// Also pins that the gutter shifts the column origin, so col 0 of the *text*
/// is `gutter_w` cells in.
#[test]
fn hit_test_excludes_the_gutter_and_offsets_columns_past_it() {
    let mut view = charwise_view();
    view.wrap = false;
    drawn(&mut view, 0, 0, 40, 3);
    view.show_line_numbers = true;
    // 3 lines → ilog10(3) == 0 → gutter is 2 cells.
    assert_eq!(view.hit_test(0, 0), None, "col 0 is gutter");
    assert_eq!(view.hit_test(1, 0), None, "col 1 is gutter");
    assert_eq!(
        view.hit_test(2, 0),
        Some((0, 0)),
        "text starts after gutter"
    );
}

/// Scroll offsets which line the top row is.
#[test]
fn hit_test_follows_the_scroll_position() {
    let mut view = charwise_view();
    view.wrap = false;
    view.scroll = 2;
    drawn(&mut view, 0, 0, 40, 3);
    assert_eq!(view.hit_test(0, 0), Some((2, 0)));
    assert_eq!(view.hit_test(0, 1), None, "nothing below the last line");
}

/// Under wrap one logical line owns several screen rows, so a row is NOT a line
/// index — and the column continues across the wrap rather than restarting.
/// Getting this wrong is how the highlight and the copied text come to disagree.
#[test]
fn hit_test_accounts_for_wrapped_rows() {
    let mut view = PagerView::new_plain("w", vec!["0123456789".to_string(), "second".to_string()]);
    view.wrap = true;
    // Body width 5 → line 0 occupies screen rows 0 and 1.
    drawn(&mut view, 0, 0, 5, 4);
    assert_eq!(view.hit_test(0, 0), Some((0, 0)));
    assert_eq!(
        view.hit_test(1, 1),
        Some((0, 6)),
        "second wrapped row of line 0"
    );
    assert_eq!(view.hit_test(0, 2), Some((1, 0)), "line 1 starts on row 2");
}

/// Clicking past end-of-line selects to the end, as every editor does, instead of
/// returning nothing.
#[test]
fn hit_test_clamps_past_end_of_line() {
    let mut view = charwise_view();
    view.wrap = false;
    drawn(&mut view, 0, 0, 40, 3);
    // "second" is 6 chars; col 30 is well past it.
    assert_eq!(view.hit_test(30, 1), Some((1, 5)));
}

/// A press that never moves must not leave a selection behind, and a moved one
/// must report as selecting.
#[test]
fn char_selection_is_nonempty_only_after_the_pointer_moves() {
    let mut view = charwise_view();
    view.begin_char_selection(1, 2);
    assert!(
        !view.char_selection_is_nonempty(),
        "a click selects nothing"
    );
    view.extend_char_selection(1, 3);
    assert!(view.char_selection_is_nonempty());
}

/// Extending must not hijack a keyboard `V` / `^v` selection into charwise.
#[test]
fn extend_char_selection_ignores_a_line_or_block_selection() {
    let mut view = charwise_view();
    view.enter_visual(); // Line mode
    view.extend_char_selection(2, 4);
    let sel = view.visual.expect("still visual");
    assert_eq!(sel.kind, VisualKind::Line);
    assert_eq!(sel.cursor, 0, "cursor untouched by a charwise extend");
}
