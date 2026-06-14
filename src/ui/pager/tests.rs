//! Unit tests for the pager, split out of `pager` verbatim.

use super::layout::{line_plain_text, pager_inner_area, wrap_line};
use super::*;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Span;

fn plain_text(line: &Line<'_>) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

/// Regression test for the wrap-vs-bottom bug: a file with
/// long lines that wrap to multiple visual rows would lose the
/// trailing logical lines when scrolled to "Bot". Reported on
/// `docs/spyc-logo.svg` (154 logical lines, several wrap to 2
/// rows each, viewport ~40 rows). The user saw "Bot" but lines
/// 151-154 never appeared.
///
/// Cause: `scroll_max` computed the cap from logical line
/// count, ignoring that wrapped lines consume extra visual
/// rows. Fix: when wrap is on and `body_w` is known, walk the
/// lines from the end summing visual rows; the highest scroll
/// value that still includes the last line in the viewport is
/// the true max.
#[test]
fn scroll_max_accounts_for_wrapped_visual_rows() {
    // 5 logical lines; each one is 60 chars wide. With body_w=20
    // each line takes 3 visual rows. Viewport is 6 visual rows
    // (= 2 logical lines fully unwrapped). Without the fix,
    // scroll_max = 5 - 6 = 0 (saturating; "All"); with the fix
    // we should be able to scroll through ~3 logical lines so
    // line 5's content lands in the last visible row.
    let view = PagerView::new_plain("test", vec!["x".repeat(60); 5]);
    view.last_body_w.set(20);
    assert!(
        view.scroll_max(6) >= 3,
        "scroll_max({}) too small — content past visual viewport \
             will be unreachable",
        view.scroll_max(6),
    );
}

/// Regression test for the "stuck at bottom" search bug in the
/// help pager (which is multi-column). With `ncols >= 2`, `scroll`
/// is interpreted per-column (each column applies the same offset
/// within its own chunk). `scroll_to_match` used to feed the
/// global line index straight into `self.scroll`, so a match in
/// column 2+ overshot `scroll_max` (= longest-chunk - vh) and got
/// clamped to the bottom — hiding the match. Symptom users hit:
/// `/show` in the help overlay then `n n n n` left the view stuck
/// at the bottom.
#[test]
fn scroll_to_match_translates_to_chunk_local_offset_in_multi_col() {
    // 200 lines, no blank lines so partition_lines_static cuts at
    // exactly idx 100 (blank-line search finds nothing in the
    // window and falls back to the ideal cut). Matches every 50:
    // {0, 50, 100, 150}. col1 chunk = [0, 100), col2 = [100, 200).
    let lines: Vec<String> = (0..200)
        .map(|i| {
            if i % 50 == 0 {
                format!("line {i} show")
            } else {
                format!("line {i}")
            }
        })
        .collect();
    let mut view = PagerView::new_plain("help", lines);
    view.columns = 2;
    let viewport = 24u16;

    view.begin_search();
    for c in "show".chars() {
        view.search_push_char(c);
    }
    assert!(view.commit_search(viewport));

    // After commit: cursor=0 (line 0), scroll=0.
    assert_eq!(view.scroll, 0);

    // n → line 50 in col1 chunk. Chunk-local idx = 50, scroll = 50 - 8 = 42.
    view.search_next(viewport);
    assert_eq!(
        view.scroll, 42,
        "n into mid-col1 should land near the match"
    );

    // n → line 100 (start of col2 chunk). Chunk-local idx = 0, scroll = 0.
    // Pre-fix: target = 100 - 8 = 92, clamped to scroll_max = 100 - 24 = 76 → bottom.
    view.search_next(viewport);
    assert_eq!(
        view.scroll, 0,
        "n onto first col2 match should reset scroll to the top of col2's chunk, \
             not pin to scroll_max"
    );

    // n → line 150 in col2 chunk. Chunk-local idx = 50, scroll = 42.
    // Pre-fix: target = 142, clamped to 76 → "stuck at bottom".
    view.search_next(viewport);
    assert_eq!(
        view.scroll, 42,
        "n onto mid-col2 match should land mid-chunk, not pin to scroll_max"
    );
}

#[test]
fn scroll_max_logical_when_no_wrap_or_no_body_w() {
    let mut view = PagerView::new_plain("test", vec!["x".repeat(60); 10]);
    // wrap off → logical-line behavior
    view.wrap = false;
    assert_eq!(view.scroll_max(4), 6); // 10 - 4
    // wrap on but body_w = 0 (e.g. before first render) →
    // fall back to logical-line behavior so we don't return a
    // bogus value when the wrap-aware path can't compute.
    view.wrap = true;
    view.last_body_w.set(0);
    assert_eq!(view.scroll_max(4), 6);
}

#[test]
fn wrap_short_line_returns_one_piece() {
    let line = Line::from("hello");
    let pieces = wrap_line(&line, 80);
    assert_eq!(pieces.len(), 1);
    assert_eq!(plain_text(&pieces[0]), "hello");
}

#[test]
fn wrap_long_line_hard_breaks() {
    let line = Line::from("aaaaabbbbbcccccddddd");
    let pieces = wrap_line(&line, 5);
    assert_eq!(pieces.len(), 4);
    assert_eq!(plain_text(&pieces[0]), "aaaaa");
    assert_eq!(plain_text(&pieces[1]), "bbbbb");
    assert_eq!(plain_text(&pieces[2]), "ccccc");
    assert_eq!(plain_text(&pieces[3]), "ddddd");
}

#[test]
fn wrap_preserves_styled_spans_across_break() {
    let red = Style::default().fg(ratatui::style::Color::Red);
    let blue = Style::default().fg(ratatui::style::Color::Blue);
    let line = Line::from(vec![
        Span::styled("aaaaa", red),
        Span::styled("BBBBB", blue),
    ]);
    let pieces = wrap_line(&line, 4);
    // 10 chars at width 4 ⇒ 3 visual rows (4+4+2). Spans split
    // across the break preserve their style on each side.
    assert_eq!(pieces.len(), 3);
    assert_eq!(plain_text(&pieces[0]), "aaaa");
    assert_eq!(pieces[0].spans[0].style, red);
    assert_eq!(plain_text(&pieces[1]), "aBBB");
    assert_eq!(pieces[1].spans[0].style, red);
    assert_eq!(pieces[1].spans[1].style, blue);
    assert_eq!(plain_text(&pieces[2]), "BB");
    assert_eq!(pieces[2].spans[0].style, blue);
}

#[test]
fn wrap_handles_wide_chars() {
    // A single CJK char is 2 cols wide; in a 3-col viewport
    // we fit one per row.
    let line = Line::from("漢字漢");
    let pieces = wrap_line(&line, 3);
    assert_eq!(pieces.len(), 3);
    assert_eq!(plain_text(&pieces[0]), "漢");
    assert_eq!(plain_text(&pieces[1]), "字");
    assert_eq!(plain_text(&pieces[2]), "漢");
}

#[test]
fn wrap_zero_width_returns_clone() {
    let line = Line::from("anything");
    let pieces = wrap_line(&line, 0);
    assert_eq!(pieces.len(), 1);
    assert_eq!(plain_text(&pieces[0]), "anything");
}

// ── Visual line mode ─────────────────────────────────────────────────

fn sample_view() -> PagerView {
    PagerView::new_plain("v", (0..20).map(|i| format!("line {i}")).collect())
}

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
    view.last_viewport_h.set(3); // 5 lines in a 3-row viewport → scroll_max == 2
    view.scroll = 999; // jumped well past the end
    view.clamp_scroll_auto();
    assert_eq!(
        view.scroll, 2,
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

#[cfg(unix)]
#[test]
fn yank_visual_past_end_clamps_instead_of_panicking() {
    use std::os::unix::fs::PermissionsExt;
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

    let tmp = tempfile::tempdir().unwrap();
    let stub = tmp.path().join("clip.sh");
    let sidecar = tmp.path().join("out.txt");
    std::fs::write(&stub, format!("#!/bin/sh\ncat > {}\n", sidecar.display())).unwrap();
    let mut perms = std::fs::metadata(&stub).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&stub, perms).unwrap();

    let n =
        crate::clipboard::with_clipboard_override(&stub, || view.yank_visual_to_clipboard(false))
            .expect("yank should not panic or error");
    assert_eq!(n, 1, "clamped to the single last line");
    let captured = std::fs::read_to_string(&sidecar).unwrap();
    assert!(
        captured.contains("line 2"),
        "yanked the clamped tail: {captured:?}"
    );
    assert!(!captured.contains("line 0"));
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

// ── v1.5 Phase 3 polish ────────────────────────────────────────

#[test]
fn pending_scroll_to_bottom_default_is_false() {
    let view = sample_view();
    assert!(
        !view.pending_scroll_to_bottom.get(),
        "constructors should leave the flag off by default"
    );
}

#[test]
fn scroll_to_bottom_with_viewport_lands_in_bottom_window() {
    // 20 lines, viewport=5 → scroll_max = 15 (last 5 lines visible).
    let mut view = sample_view();
    view.scroll_to_bottom(5);
    assert_eq!(view.scroll, 15);
}

#[test]
fn lower_pane_mount_renders_borderless() {
    // Render-side check (no actual frame): pager_inner_area for
    // LowerPane mount returns the area as-is — and the borderless
    // branch of the render block uses `Block::default()` with no
    // borders. Verify the rect helper still uses the rect as-is.
    let mut view = sample_view();
    view.mount = Mount::LowerPane;
    let slot = Rect::new(0, 21, 100, 19);
    assert_eq!(pager_inner_area(slot, &view), slot);
}

// ── v1.5 Phase 1: Mount enum & rect dispatch ───────────────────

#[test]
fn mount_default_is_overlay() {
    let view = sample_view();
    assert_eq!(view.mount, Mount::Overlay);
}

#[test]
fn pager_inner_area_overlay_centers() {
    // 100x40 frame, default Mount::Overlay → centered 90×92 %.
    let view = sample_view();
    let frame = Rect::new(0, 0, 100, 40);
    let inner = pager_inner_area(frame, &view);
    assert!(inner.width < frame.width, "should be narrower than frame");
    assert!(inner.height < frame.height, "should be shorter than frame");
    assert!(inner.x > frame.x, "should be inset from left");
    assert!(inner.y > frame.y, "should be inset from top");
}

#[test]
fn pager_inner_area_overlay_full_width_uses_whole_area() {
    let mut view = sample_view();
    view.full_width = true;
    let frame = Rect::new(0, 0, 100, 40);
    assert_eq!(pager_inner_area(frame, &view), frame);
}

#[test]
fn pager_inner_area_top_pane_uses_area_as_is() {
    let mut view = sample_view();
    view.mount = Mount::TopPane;
    // Caller would pass the top-pane slot rect; pager must
    // honor it verbatim (no extra centering / fit logic).
    let slot = Rect::new(0, 0, 100, 20);
    assert_eq!(pager_inner_area(slot, &view), slot);
}

#[test]
fn pager_inner_area_top_pane_ignores_full_width_and_fit() {
    // Pane mounts deliberately ignore the overlay sizing
    // flags — the slot's rect already defines the footprint.
    let mut view = sample_view();
    view.mount = Mount::TopPane;
    view.full_width = true;
    view.fit_to_content = true;
    let slot = Rect::new(5, 2, 80, 15);
    assert_eq!(pager_inner_area(slot, &view), slot);
}

#[test]
fn pager_inner_area_lower_pane_uses_area_as_is() {
    let mut view = sample_view();
    view.mount = Mount::LowerPane;
    let slot = Rect::new(0, 21, 100, 19);
    assert_eq!(pager_inner_area(slot, &view), slot);
}

// ── snapshot tests (TestBackend) ──────────────────────────────
//
// Glyph-level snapshots of the pager's four interesting modes:
// ANSI input (color-tagged source), hex dump styling, line-number
// gutter, and search highlight. We capture symbols only (no
// styling) — same trade-off as `ui::status::tests`. A regression
// that breaks layout, gutter width, search-bar formatting, or
// hex-dump structure will diff visibly.

use crate::ui::theme::Theme;
use ratatui::{Terminal, backend::TestBackend};

fn render_pager_to_string(view: &PagerView, w: u16, h: u16) -> String {
    let backend = TestBackend::new(w, h);
    let mut terminal = Terminal::new(backend).unwrap();
    let theme = Theme::default();
    terminal
        .draw(|f| {
            let area = Rect::new(0, 0, w, h);
            super::render(f, area, view, &theme);
        })
        .unwrap();
    let buf = terminal.backend().buffer().clone();
    let mut out = String::new();
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            out.push_str(buf.cell((x, y)).map_or(" ", |c| c.symbol()));
        }
        out.push('\n');
    }
    // Trim trailing whitespace per line and drop trailing blank
    // lines so the snapshot stays tight.
    out.lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_string()
}

#[test]
fn snapshot_pager_hex() {
    use std::io::Write;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bin");
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(b"\x7fELF\x02\x01\x01\x00hello, spyc!").unwrap();
    let lines = crate::ui::hex::hex_dump_lines(&path, &Theme::default()).unwrap();
    let mut view = PagerView::new_styled("bin", lines);
    view.full_width = true;
    let out = render_pager_to_string(&view, 80, 6);
    insta::assert_snapshot!(out);
}

#[test]
fn snapshot_pager_line_numbers() {
    // 12 lines so the gutter is at least 2 digits wide, which is
    // the case the renderer has to right-align.
    let lines: Vec<String> = (1..=12).map(|i| format!("line {i}")).collect();
    let mut view = PagerView::new_plain("notes.txt", lines);
    view.full_width = true;
    // show_line_numbers is on by default in new_plain; assert.
    assert!(view.show_line_numbers);
    let out = render_pager_to_string(&view, 40, 14);
    insta::assert_snapshot!(out);
}

#[test]
fn snapshot_pager_search_highlight() {
    let mut view = PagerView::new_plain(
        "search.txt",
        vec![
            "alpha".to_string(),
            "beta needle".to_string(),
            "gamma".to_string(),
            "delta needle".to_string(),
            "epsilon".to_string(),
        ],
    );
    view.full_width = true;
    view.begin_search();
    for c in "needle".chars() {
        view.search_push_char(c);
    }
    // Viewport height matches what we'll render with.
    let committed = view.commit_search(8);
    assert!(committed, "search query should match");
    let out = render_pager_to_string(&view, 50, 8);
    insta::assert_snapshot!(out);
}

// ── yank title header ─────────────────────────────────────────

#[test]
fn title_header_prepended_when_include_true() {
    let view = PagerView::new_plain("!cargo build", vec!["hello".into(), "world".into()]);
    let out = view.with_title_header(view.source_text(), true);
    assert_eq!(out, "# !cargo build\n\nhello\nworld");
}

#[test]
fn title_header_skipped_when_include_false() {
    let view = PagerView::new_plain("!cargo build", vec!["hello".into()]);
    let out = view.with_title_header(view.source_text(), false);
    assert_eq!(out, "hello");
}

#[test]
fn title_header_skipped_when_title_empty() {
    // Empty title (rare but possible) ⇒ no header even with
    // include_title = true — pasting "# \n\n..." is uglier than
    // pasting just the content.
    let view = PagerView::new_plain("", vec!["hello".into()]);
    let out = view.with_title_header(view.source_text(), true);
    assert_eq!(out, "hello");
}

// ── markdown toggle scroll preservation ───────────────────────

/// Build a markdown-enabled pager: `lines` is the rendered side
/// (10 entries), `alt_lines` is the source side (5 entries).
/// The two sides have intentionally different sizes — that's the
/// whole reason the old "reset to 0" rule existed.
fn md_view() -> PagerView {
    let rendered: Vec<Line<'static>> = (0..10)
        .map(|i| Line::from(format!("rendered{i}")))
        .collect();
    let source: Vec<Line<'static>> = (0..5).map(|i| Line::from(format!("source{i}"))).collect();
    let mut v = PagerView::new_styled("README.md", rendered);
    v.alt_lines = Some(source);
    v
}

#[test]
fn toggle_markdown_first_time_projects_proportionally() {
    let mut v = md_view();
    // 10 rendered lines, currently at line 8 (≈ 89% down).
    v.scroll = 8;
    assert!(v.toggle_markdown());
    // Source side has 5 lines (max scroll = 4). 8/9 * 4 ≈ 3.55 → 3.
    assert_eq!(v.scroll, 3);
}

#[test]
fn toggle_markdown_round_trip_restores_exact_position() {
    let mut v = md_view();
    v.scroll = 7;
    // rendered → source (proportional projection)
    v.toggle_markdown();
    let source_landing = v.scroll;
    // user reads source, scrolls a bit
    v.scroll = 1;
    // source → rendered (must restore the user's *original* 7, not
    // the proportional projection of 1)
    v.toggle_markdown();
    assert_eq!(v.scroll, 7, "rendered side should restore prior position");
    // rendered → source again (must restore the 1 we left at)
    v.toggle_markdown();
    assert_eq!(
        v.scroll, 1,
        "source side should restore the position we left it at"
    );
    // sanity: source_landing wasn't already 1 (otherwise the test
    // would falsely pass even with broken memory).
    assert_ne!(source_landing, 1);
}

#[test]
fn toggle_markdown_clamps_restored_scroll_to_new_bounds() {
    // If the saved value is past the end of the new buffer (can
    // happen if a buffer gets shorter between visits — not common
    // for markdown but the clamp is cheap insurance), we should
    // land at the last valid index, not panic or sit past EOF.
    let mut v = md_view();
    v.saved_alt_scroll = Some(99);
    v.scroll = 0;
    v.toggle_markdown();
    // Source side has 5 lines → max scroll index 4.
    assert_eq!(v.scroll, 4);
}

#[test]
fn toggle_markdown_no_alt_returns_false() {
    let mut v = PagerView::new_plain("plain.txt", vec!["hi".into()]);
    assert!(!v.toggle_markdown());
    assert_eq!(v.scroll, 0);
}
