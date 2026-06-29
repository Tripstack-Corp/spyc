//! Unit tests for the pager. The visual / block / placement selection cluster
//! lives in the `selection` submodule (split out under the 800-LoC rule); this
//! module holds scroll-math, wrapping, rendering, and markdown-toggle tests
//! plus the helpers shared with `selection` (`sample_view`, `plain_text`).

use super::layout::{line_plain_text, pager_inner_area, visual_rows, wrap_line, wrap_line_capped};
use super::*;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Span;

mod selection;

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

/// The render pass computes the multi-column partition once and feeds it to
/// the position indicator via `position_indicator_multi`. That shared-partition
/// path must produce the same label as the original `position_indicator`
/// (which re-partitions internally) for the same view + viewport — the
/// invariant the dedup relies on.
#[test]
fn position_indicator_multi_matches_recomputing_path() {
    // Two columns with a blank line, so the partition isn't a trivial even cut.
    let mut lines: Vec<String> = (0..40).map(|i| format!("line {i}")).collect();
    lines[18] = String::new(); // section break shifts the chunk boundary
    let mut view = PagerView::new_plain("t", lines);
    view.columns = 2;
    let chunks = super::layout::partition_lines_static(&view.lines, 2);
    for (scroll, vh) in [(0usize, 6u16), (3, 6), (8, 6), (100, 6)] {
        view.scroll = scroll;
        assert_eq!(
            view.position_indicator_multi(&chunks, vh),
            view.position_indicator(vh),
            "shared-partition indicator diverged at scroll={scroll} vh={vh}"
        );
    }
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
    // wrap off → logical-line behavior. 10 - 4 = 6 pins the last line to the
    // bottom; +1 reserves a row for the `[EOF]` marker (long-file end signal).
    view.wrap = false;
    assert_eq!(view.scroll_max(4), 7);
    // wrap on but body_w = 0 (e.g. before first render) →
    // fall back to logical-line behavior so we don't return a
    // bogus value when the wrap-aware path can't compute.
    view.wrap = true;
    view.last_body_w.set(0);
    assert_eq!(view.scroll_max(4), 7);
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

// Shared fixture for the visual-mode tests (in `selection`) and the
// scroll-to-bottom / mount / inner-area tests below.
fn sample_view() -> PagerView {
    PagerView::new_plain("v", (0..20).map(|i| format!("line {i}")).collect())
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
    // 20 lines, viewport=5: content-pin max 15, +1 for the [EOF] row → 16
    // (last line visible one row up, end marker on the bottom row).
    let mut view = sample_view();
    view.scroll_to_bottom(5);
    assert_eq!(view.scroll, 16);
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

// ── backward search (`?`) and direction-aware n/N ─────────────

#[test]
fn select_match_forward_picks_first_at_or_after_anchor() {
    let m = [5usize, 15, 25];
    // Anchor below the first match → the first match.
    assert_eq!(PagerView::select_match(&m, 0, false), 0);
    // Anchor exactly on a match → that match (stays put).
    assert_eq!(PagerView::select_match(&m, 15, false), 1);
    // Anchor between matches → the next one down.
    assert_eq!(PagerView::select_match(&m, 16, false), 2);
    // Anchor past the last match → wraps to the first.
    assert_eq!(PagerView::select_match(&m, 99, false), 0);
}

#[test]
fn select_match_backward_picks_last_at_or_before_anchor() {
    let m = [5usize, 15, 25];
    // Anchor past the last match → the last match.
    assert_eq!(PagerView::select_match(&m, 99, true), 2);
    // Anchor exactly on a match → that match.
    assert_eq!(PagerView::select_match(&m, 15, true), 1);
    // Anchor between matches → the previous one up.
    assert_eq!(PagerView::select_match(&m, 14, true), 0);
    // Anchor above the first match → wraps to the last.
    assert_eq!(PagerView::select_match(&m, 0, true), 2);
}

#[test]
fn select_match_single_match_always_lands_on_it() {
    let m = [7usize];
    assert_eq!(PagerView::select_match(&m, 0, false), 0);
    assert_eq!(PagerView::select_match(&m, 100, false), 0);
    assert_eq!(PagerView::select_match(&m, 0, true), 0);
    assert_eq!(PagerView::select_match(&m, 100, true), 0);
}

/// Matches at lines 5/15/25; a view scrolled into the middle.
fn needle_view() -> PagerView {
    let lines: Vec<String> = (0..30)
        .map(|i| {
            if i % 10 == 5 {
                format!("line {i} needle")
            } else {
                format!("line {i}")
            }
        })
        .collect();
    PagerView::new_plain("t", lines)
}

fn run_search(view: &mut PagerView, backward: bool, viewport: u16) {
    if backward {
        view.begin_search_backward();
    } else {
        view.begin_search();
    }
    for c in "needle".chars() {
        view.search_push_char(c);
    }
    assert!(view.commit_search(viewport));
}

#[test]
fn forward_search_lands_below_current_scroll_not_top_of_file() {
    let mut view = needle_view();
    view.scroll = 20; // below the line-15 match, above line-25
    run_search(&mut view, false, 10);
    // Forward from line 20 finds line 25, NOT line 5 (the old top-of-file jump).
    assert_eq!(view.current_match_line(), Some(25));
}

#[test]
fn backward_search_lands_above_current_scroll() {
    let mut view = needle_view();
    view.scroll = 20;
    run_search(&mut view, true, 10);
    // Backward from line 20 finds line 15.
    assert_eq!(view.current_match_line(), Some(15));
}

#[test]
fn n_repeats_in_direction_shift_n_against_it() {
    let viewport = 10u16;

    // Forward search: n walks down (wraps), N walks up.
    let mut fwd = needle_view();
    fwd.scroll = 0;
    run_search(&mut fwd, false, viewport); // lands on 5
    assert_eq!(fwd.current_match_line(), Some(5));
    fwd.search_repeat(viewport); // n → 15
    assert_eq!(fwd.current_match_line(), Some(15));
    fwd.search_repeat_opposite(viewport); // N → 5
    assert_eq!(fwd.current_match_line(), Some(5));
    fwd.search_repeat_opposite(viewport); // N → wraps up to 25
    assert_eq!(fwd.current_match_line(), Some(25));

    // Backward search: n walks up (wraps), N walks down.
    let mut bwd = needle_view();
    bwd.scroll = 29;
    run_search(&mut bwd, true, viewport); // lands on 25
    assert_eq!(bwd.current_match_line(), Some(25));
    bwd.search_repeat(viewport); // n → 15
    assert_eq!(bwd.current_match_line(), Some(15));
    bwd.search_repeat(viewport); // n → 5
    assert_eq!(bwd.current_match_line(), Some(5));
    bwd.search_repeat(viewport); // n → wraps down to 25
    assert_eq!(bwd.current_match_line(), Some(25));
    bwd.search_repeat_opposite(viewport); // N → 5
    assert_eq!(bwd.current_match_line(), Some(5));
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

/// A file taller than the viewport can now scroll one row past the last line
/// so the `[EOF]` end marker renders at the true bottom (it used to only show
/// for files that fit the viewport). The last content line stays visible.
#[test]
fn long_file_shows_eof_marker_at_bottom() {
    use ratatui::{Terminal, backend::TestBackend};
    let lines: Vec<String> = (0..100).map(|i| format!("line {i}")).collect();
    let mut view = PagerView::new_plain("long.txt", lines);
    let theme = Theme::default();
    let mut term = Terminal::new(TestBackend::new(40, 20)).unwrap();
    term.draw(|f| render(f, f.area(), &view, &theme)).unwrap(); // sets last_viewport_h
    view.scroll_to_bottom_auto();
    term.draw(|f| render(f, f.area(), &view, &theme)).unwrap();
    let buf = term.backend().buffer().clone();
    let mut text = String::new();
    for y in 0..20 {
        for x in 0..40 {
            text.push_str(buf.cell((x, y)).unwrap().symbol());
        }
        text.push('\n');
    }
    assert!(
        text.contains("[EOF]"),
        "the [EOF] marker renders at the bottom:\n{text}"
    );
    assert!(
        text.contains("line 99"),
        "the last content line stays visible:\n{text}"
    );
}

// ---- PR4: scroll math (u16 saturation, wrap-row reachability) -----------

/// Finding `ui/pager/mod.rs:141` / `pager_handler/mod.rs:311`: `scroll` was a
/// `u16`, so any pager over 65 535 lines (a big log, a generated file) could
/// not scroll past line 65 536. With `scroll: usize`, the bottom of a 70k-line
/// document is reachable.
#[test]
fn scroll_reaches_beyond_u16_max_lines() {
    let mut view = PagerView::new_plain("big", (0..70_000).map(|i| i.to_string()).collect());
    view.wrap = false;
    view.scroll_to_bottom(10);
    assert!(
        view.scroll > u16::MAX as usize,
        "scroll {} should exceed the old u16 cap of 65535",
        view.scroll
    );
    // logical scroll_max = 70_000 - 10, +1 for the [EOF] end-marker row.
    assert_eq!(view.scroll, 69_991);
}

/// Finding `ui/pager/layout.rs:131`: `visual_rows` underestimated wrapped rows
/// because `total_width.div_ceil(width)` assumes perfect packing. A wide
/// (2-cell) glyph that doesn't fit the last cell of a row is pushed whole to
/// the next row, so the true count is higher. The greedy walk must match
/// `wrap_line`.
#[test]
fn visual_rows_counts_wide_char_greedy_waste() {
    // FULLWIDTH 'A' is 2 cells. 5 of them = 10 cells. div_ceil(10,5)=2, but
    // greedy fits 2 per 5-cell row (1 cell wasted) → 3 rows, matching wrap_line.
    let wide = Line::from("\u{ff21}".repeat(5));
    assert_eq!(visual_rows(&wide, 5, 4), 3);
    assert_eq!(visual_rows(&wide, 5, 4), wrap_line(&wide, 5).len());
    // ASCII packs perfectly: 10 chars at width 5 = 2 rows.
    let ascii = Line::from("x".repeat(10));
    assert_eq!(visual_rows(&ascii, 5, 4), 2);
    // A glyph wider than the whole width is forced onto one row (never zero).
    assert_eq!(visual_rows(&Line::from("\u{ff21}".to_string()), 1, 4), 1);
    // Empty line is one visual row.
    assert_eq!(visual_rows(&Line::from(""), 5, 4), 1);
    // A tab counts as `tab_width` breakable cells: "\t\tx" at width 4, tab=4 →
    // 8 cells of indent + 'x' = 9 cells → 3 rows (matches the expanded render).
    let tabbed = Line::from("\t\tx");
    assert_eq!(visual_rows(&tabbed, 4, 4), 3);
}

/// Finding `ui/pager/selection.rs:281`: the visual-cursor auto-scroll assumed
/// one logical line == one screen row, so under wrap the cursor slid off the
/// bottom without the viewport following. It must count *visual* rows.
#[test]
fn scroll_to_keep_visible_is_wrap_aware() {
    // 5 lines, each 60 wide; body_w=20 → 3 visual rows each. Viewport = 6 rows.
    let mut view = PagerView::new_plain("t", vec!["x".repeat(60); 5]);
    view.wrap = true;
    view.last_body_w.set(20);
    view.enter_visual();
    view.visual_jump_to(4, 6); // jump the cursor to the last line
    // Logical math would leave scroll=0 (line 4 < 0+6) and lines 0,1 (6 rows)
    // would fill the viewport, hiding line 4. Wrap-aware: walk back from 4
    // (3+3=6 fits, +3=9 overflows) → top = line 3, so line 4 sits at the bottom.
    assert_eq!(view.scroll, 3, "viewport must follow the cursor under wrap");
    // With wrap off, the logical math applies: line 4, vh 6 → still visible at 0.
    let mut plain = PagerView::new_plain("t", vec!["x".repeat(60); 5]);
    plain.wrap = false;
    plain.enter_visual();
    plain.visual_jump_to(4, 6);
    assert_eq!(plain.scroll, 0, "no wrap: 5 lines fit in 6 rows, no scroll");
}

/// Finding `ui/pager/render.rs:180`: the render path materialized a logical
/// line's *entire* wrapped expansion every frame even though only the visible
/// rows are painted. `wrap_line_capped` bounds the work to `max_rows`, and the
/// capped prefix must equal the full expansion's prefix (so output is identical).
#[test]
fn wrap_line_capped_bounds_to_visible_rows() {
    let line = Line::from("x".repeat(100)); // 10 rows at width 10
    let full = wrap_line(&line, 10);
    assert_eq!(full.len(), 10);
    let capped = wrap_line_capped(&line, 10, 3);
    assert_eq!(capped.len(), 3, "bounded to the 3 requested rows");
    for i in 0..3 {
        assert_eq!(
            plain_text(&capped[i]),
            plain_text(&full[i]),
            "row {i} differs"
        );
    }
    // max_rows past the real row count is a no-op (full expansion).
    assert_eq!(wrap_line_capped(&line, 10, 999).len(), 10);
}
