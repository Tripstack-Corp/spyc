//! Scrollback adapter: `vt100::Screen` → `Vec<Line<'static>>`.
//!
//! Bridges the pane's vt100 emulator (cell grid + bounded
//! scrollback) into the pager's data model (styled lines), so the
//! v1.5 `^a-v` rewrite can use the in-app pager — search, jump,
//! visual-mode range yank, line numbers — over pane history
//! instead of vt100's flat scroll-mode buffer.
//!
//! Phase 2 just lays the adapter; Phase 3 wires it into
//! `App::handle_action(Action::PaneScrollEnter)`.
//!
//! ## Algorithm
//!
//! vt100 0.16's public `Screen` API only exposes the *visible*
//! window via `cell(row, col)`; the scrollback buffer itself is
//! not iterable. To capture the whole history we walk the visible
//! window backwards through scrollback by mutating
//! `scrollback_offset` (clamped by `set_scrollback`), reading one
//! page at a time. The original offset is restored before the
//! function returns, so callers can keep using the screen state
//! they had before invoking the adapter.
//!
//! Pages are sized to `rows_len` and chosen so each is a clean
//! contiguous slice with no overlap — the partial last scrollback
//! page (when `scrollback_len % rows_len != 0`) reads only its
//! valid prefix; the final live-screen page (offset = 0) emits
//! all `rows_len` rows. Net output is in chronological order:
//! oldest scrollback first, current live screen last.

use ratatui::{
    style::Style,
    text::{Line, Span},
};

use crate::pane::cell_style;

/// Snapshot the whole vt100 buffer (scrollback + live screen) as
/// styled lines, in chronological order (oldest first). The live-
/// screen portion is preserved verbatim — including blank rows
/// below the cursor — so the snapshot mirrors the on-screen
/// geometry the user just had in front of them. Trailing-
/// whitespace runs within a single row are dropped because vt100
/// pads short rows with spaces out to the full column width and
/// that's a fixed-grid artifact, not user content.
///
/// `&mut Screen` is required because the implementation walks the
/// scrollback by adjusting `scrollback_offset`; the original
/// offset is restored before returning. Callers can keep their
/// own scroll state across the call.
pub fn lines_from_scrollback(screen: &mut vt100::Screen) -> Vec<Line<'static>> {
    let saved_offset = screen.scrollback();
    let (rows_u16, cols_u16) = screen.size();
    let rows_len = rows_u16 as usize;
    let cols_len = cols_u16;

    // `set_scrollback` clamps to actual scrollback length; ask for
    // the max and read the resulting offset to discover the cap.
    screen.set_scrollback(usize::MAX);
    let scrollback_len = screen.scrollback();

    let mut out = Vec::with_capacity(scrollback_len + rows_len);

    // Walk scrollback in `rows_len`-sized pages from oldest to
    // newest. Each iteration reads exactly `chunk` rows of pure
    // scrollback content, where `chunk` is `rows_len` for full
    // pages and the remainder on the partial last page.
    let mut remaining = scrollback_len;
    while remaining > 0 {
        let chunk = remaining.min(rows_len);
        screen.set_scrollback(remaining);
        for row in 0..chunk {
            out.push(line_from_visible_row(screen, row as u16, cols_len));
        }
        remaining -= chunk;
    }

    // Live screen (offset = 0): the current rows_len rows.
    screen.set_scrollback(0);
    for row in 0..rows_len {
        out.push(line_from_visible_row(screen, row as u16, cols_len));
    }

    screen.set_scrollback(saved_offset);

    // Deliberately do NOT trim trailing blank lines. The live
    // screen often has the cursor mid-grid with empty rows below
    // (a shell prompt sitting at row 5 of 24, blank rows 6..23).
    // Trimming those would chop the snapshot's bottom up to the
    // cursor row and the pager's "scroll to bottom on entry"
    // would then anchor at the cursor row — visually shifting
    // content up vs. what was just on screen. Reported as "the
    // positioning of text jumps when entering ^a-v". Mirroring
    // the screen geometry verbatim makes ^a-v feel like a frozen
    // copy of the live pty.
    out
}

/// Number of rows in the vt100 scrollback buffer (history above the
/// live screen), independent of the current view offset. Zero means
/// the pane has no captured history — either a fresh process that
/// hasn't scrolled, or (the case this exists for) an app like codex
/// that confines its history to a DECSTBM scroll region so lines
/// never scroll off the top into the main buffer.
///
/// `&mut Screen` because the probe walks `scrollback_offset`; the
/// original offset is restored before returning.
pub fn scrollback_len(screen: &mut vt100::Screen) -> usize {
    let saved_offset = screen.scrollback();
    // `set_scrollback` clamps to the real length; ask for the max
    // and read back the clamped offset to discover it.
    screen.set_scrollback(usize::MAX);
    let len = screen.scrollback();
    screen.set_scrollback(saved_offset);
    len
}

/// Build a single styled `Line` from row `row` of the screen's
/// current visible window. Adjacent cells with identical styles
/// are merged into one span; trailing whitespace at the row's
/// right edge is dropped (preserving leading / interior spaces).
fn line_from_visible_row(screen: &vt100::Screen, row: u16, cols: u16) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut current_text = String::new();
    let mut current_style: Option<Style> = None;

    for col in 0..cols {
        let Some(cell) = screen.cell(row, col) else {
            break;
        };
        // vt100 represents wide-char continuations as separate
        // cells with empty contents. Skip them so the wide char's
        // first-half cell carries the full glyph and we don't
        // emit a stray empty span (which would split runs of
        // identical style).
        if cell.is_wide_continuation() {
            continue;
        }
        let style = cell_style(cell);
        let contents = cell.contents();
        let ch: &str = if contents.is_empty() { " " } else { contents };

        if Some(style) != current_style {
            if !current_text.is_empty() {
                spans.push(Span::styled(
                    std::mem::take(&mut current_text),
                    current_style.unwrap_or_default(),
                ));
            }
            current_style = Some(style);
        }
        current_text.push_str(ch);
    }
    if !current_text.is_empty() {
        spans.push(Span::styled(
            current_text,
            current_style.unwrap_or_default(),
        ));
    }
    trim_trailing_whitespace_run(&mut spans);
    Line::from(spans)
}

/// Drop trailing space-only spans from the right edge of a line,
/// and trim trailing spaces from the last non-blank span. Leaves
/// interior whitespace alone — only the right-edge run goes.
///
/// This is what makes the result read like a vi buffer: vt100
/// pads short rows with spaces out to the right edge, and we
/// don't want every line to be exactly `cols` characters wide
/// in the pager.
fn trim_trailing_whitespace_run(spans: &mut Vec<Span<'static>>) {
    while spans
        .last()
        .is_some_and(|s| s.content.chars().all(|c| c == ' '))
    {
        spans.pop();
    }
    if let Some(last) = spans.last_mut() {
        let trimmed = last.content.trim_end_matches(' ').to_string();
        if trimmed.len() != last.content.len() {
            last.content = trimmed.into();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parser_with(rows: u16, cols: u16, scrollback: usize, bytes: &[u8]) -> vt100::Parser {
        let mut p = vt100::Parser::new(rows, cols, scrollback);
        p.process(bytes);
        p
    }

    fn plain_lines(lines: &[Line<'static>]) -> Vec<String> {
        lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    #[test]
    fn empty_buffer_yields_blank_live_rows() {
        // 4-row screen, no input → 4 blank live rows. We
        // deliberately preserve them so the pager mirrors the
        // on-screen geometry: an empty live screen renders as
        // empty rows in the pager, not as nothing.
        let mut p = parser_with(4, 20, 100, b"");
        let lines = lines_from_scrollback(p.screen_mut());
        let plain = plain_lines(&lines);
        assert_eq!(plain, vec!["", "", "", ""]);
    }

    #[test]
    fn live_screen_lines_appear_in_order_with_blank_tail() {
        // 4-row screen with 3 lines of content; cursor lands on
        // row 3 (blank). The blank row is preserved so the
        // snapshot reflects what the user just saw.
        let mut p = parser_with(4, 20, 100, b"alpha\r\nbeta\r\ngamma\r\n");
        let lines = lines_from_scrollback(p.screen_mut());
        let plain = plain_lines(&lines);
        assert_eq!(plain, vec!["alpha", "beta", "gamma", ""]);
    }

    #[test]
    fn scrollback_len_zero_when_no_history() {
        // 4-row screen, 3 lines — nothing spilled to scrollback.
        let mut p = parser_with(4, 20, 100, b"alpha\r\nbeta\r\ngamma\r\n");
        assert_eq!(scrollback_len(p.screen_mut()), 0);
    }

    #[test]
    fn scrollback_len_counts_spilled_rows() {
        // 2-row screen, 6 lines pushed → 4 spill into scrollback,
        // 2 stay live. (The trailing CRLF's blank live row doesn't
        // add to scrollback.)
        let mut p = parser_with(
            2,
            20,
            100,
            b"one\r\ntwo\r\nthree\r\nfour\r\nfive\r\nsix\r\n",
        );
        assert_eq!(scrollback_len(p.screen_mut()), 5);
    }

    #[test]
    fn scrollback_len_restores_offset() {
        let mut p = parser_with(2, 20, 100, b"a\r\nb\r\nc\r\nd\r\ne\r\nf\r\n");
        p.screen_mut().set_scrollback(2);
        let _ = scrollback_len(p.screen_mut());
        assert_eq!(p.screen().scrollback(), 2);
    }

    #[test]
    fn scrollback_lines_appear_before_live_lines() {
        // 2-row screen with 100-line scrollback. Push 6 lines so
        // 4 spill into scrollback and 2 remain live. The trailing
        // CRLF leaves the cursor on a blank live row, which we
        // preserve verbatim — that's the on-screen geometry.
        let mut p = parser_with(
            2,
            20,
            100,
            b"one\r\ntwo\r\nthree\r\nfour\r\nfive\r\nsix\r\n",
        );
        let lines = lines_from_scrollback(p.screen_mut());
        let plain = plain_lines(&lines);
        assert_eq!(
            plain,
            vec!["one", "two", "three", "four", "five", "six", ""]
        );
    }

    #[test]
    fn trailing_padding_trimmed_within_line() {
        // Cell-grid emulator pads to cols=20. We emit "hi" (2
        // chars), not "hi" + 18 spaces. The trailing CRLF leaves
        // the second row blank, which we preserve.
        let mut p = parser_with(2, 20, 0, b"hi\r\n");
        let lines = lines_from_scrollback(p.screen_mut());
        let plain = plain_lines(&lines);
        assert_eq!(plain, vec!["hi", ""]);
    }

    #[test]
    fn scrollback_offset_is_restored_after_call() {
        // Caller has scroll_offset = 3; adapter must put it back.
        let mut p = parser_with(2, 20, 100, b"a\r\nb\r\nc\r\nd\r\ne\r\nf\r\n");
        p.screen_mut().set_scrollback(3);
        assert_eq!(p.screen().scrollback(), 3);
        let _ = lines_from_scrollback(p.screen_mut());
        assert_eq!(
            p.screen().scrollback(),
            3,
            "adapter must restore caller's scrollback offset",
        );
    }

    #[test]
    fn paged_walk_is_chunked_correctly() {
        // 3-row screen, lots of content. Tests that the page-walk
        // doesn't double-read or skip rows when scrollback_len is
        // not a multiple of rows_len. Trailing CRLF leaves a blank
        // live row at the end, preserved verbatim.
        let payload: String = (1..=20).fold(String::new(), |mut acc, i| {
            use std::fmt::Write as _;
            let _ = write!(acc, "line{i:02}\r\n");
            acc
        });
        let mut p = parser_with(3, 20, 100, payload.as_bytes());
        let lines = lines_from_scrollback(p.screen_mut());
        let plain = plain_lines(&lines);
        let mut expected: Vec<String> = (1..=20).map(|i| format!("line{i:02}")).collect();
        expected.push(String::new());
        assert_eq!(plain, expected);
    }

    #[test]
    fn styled_text_preserves_colors() {
        // Red "hi" — verify the result has a red span. No trailing
        // CRLF here so the cursor stays on the content row and the
        // first emitted line is the styled one (the second row is
        // a blank live row, still preserved).
        use ratatui::style::Color;
        let mut p = parser_with(2, 20, 100, b"\x1b[31mhi\x1b[0m");
        let lines = lines_from_scrollback(p.screen_mut());
        assert!(!lines.is_empty(), "expected at least one line");
        let red_span = lines[0]
            .spans
            .iter()
            .find(|s| s.style.fg == Some(Color::Indexed(1)))
            .expect("expected a red span in styled line");
        assert_eq!(red_span.content.as_ref(), "hi");
    }

    #[test]
    fn adjacent_same_style_cells_merge_into_one_span() {
        // No styling means every cell shares Style::default(); they
        // should land in a single span, not 5 single-char spans.
        // Drop the trailing CRLF so the cursor stays on the content
        // row — keeps the test focused on the merge behavior.
        let mut p = parser_with(2, 20, 100, b"hello");
        let lines = lines_from_scrollback(p.screen_mut());
        assert!(!lines.is_empty());
        assert_eq!(
            lines[0].spans.len(),
            1,
            "expected one merged span; got {} ({:?})",
            lines[0].spans.len(),
            lines[0].spans,
        );
    }

    #[test]
    fn scrollback_smaller_than_one_page_works() {
        // 5-row screen; only 2 lines of input → no scrollback
        // overflow. The page-walk's `while remaining > 0` should
        // skip cleanly and we still get the live rows. Trailing
        // CRLF leaves the next 3 live rows blank, preserved
        // verbatim.
        let mut p = parser_with(5, 20, 100, b"a\r\nb\r\n");
        let lines = lines_from_scrollback(p.screen_mut());
        let plain = plain_lines(&lines);
        assert_eq!(plain, vec!["a", "b", "", "", ""]);
    }

    #[test]
    fn scrollback_capacity_zero_emits_only_live() {
        // 0-row scrollback: lines that scroll off are lost; only
        // the live rows survive. No trailing CRLF after `d` so
        // the cursor ends on row 1 with `d`, not on a fresh blank
        // row that would have scrolled `c` off.
        let mut p = parser_with(2, 20, 0, b"a\r\nb\r\nc\r\nd");
        let lines = lines_from_scrollback(p.screen_mut());
        let plain = plain_lines(&lines);
        assert_eq!(plain, vec!["c", "d"]);
    }
}
