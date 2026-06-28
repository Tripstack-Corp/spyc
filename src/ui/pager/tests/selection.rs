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
