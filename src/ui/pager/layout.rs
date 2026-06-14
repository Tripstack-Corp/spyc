//! Pure geometry + text helpers: centered/fit rects and body widths, line
//! wrapping + multi-column partitioning, visual-row counting, word-motion
//! boundaries. Split from `pager` verbatim.

use ratatui::{
    layout::Rect,
    text::{Line, Span},
};

use super::{Mount, PagerView};

/// Flatten styled spans back to plain text (for case-insensitive matching).
pub(super) fn line_plain_text(line: &Line) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

/// Centered pager occupies this percent of the terminal width.
/// Exposed so callers (help content generation) can compute the same
/// column width the pager will actually render at.
pub(super) const CENTERED_W_PCT: u16 = 90;

/// Gap (in cells) between columns in multi-column mode.
pub(super) const COL_GAP: u16 = 2;

/// Column width a centered pager will use for `ncols` columns at the
/// given terminal width. Mirrors the render-path math: centered rect
/// → minus 2 for block borders → divided evenly across columns.
#[must_use]
pub const fn centered_col_width(term_w: u16, ncols: u16) -> u16 {
    let body_w = centered_body_width(term_w);
    let ncols = if ncols < 1 { 1 } else { ncols };
    let gaps = COL_GAP * ncols.saturating_sub(1);
    body_w.saturating_sub(gaps) / ncols
}

/// Body width inside the centered pager (useful for deciding how many
/// columns actually fit before calling `centered_col_width`).
#[must_use]
pub const fn centered_body_width(term_w: u16) -> u16 {
    // Compute in u32: `term_w * 90` overflows u16 for terminals wider than
    // 728 columns (panic in debug, silent wrap in release). The result is
    // always ≤ 0.9·u16::MAX, so the cast back to u16 never truncates.
    ((term_w as u32 * CENTERED_W_PCT as u32 / 100) as u16).saturating_sub(2)
}

/// Vi `w`-style word class: alphanumeric + `_`. Whitespace and
/// punctuation each form their own class — a transition between
/// any two of {word, punct, whitespace} counts as a word
/// boundary for forward/backward motion.
pub(super) fn word_class(c: char) -> u8 {
    if c.is_whitespace() {
        0
    } else if c.is_alphanumeric() || c == '_' {
        1
    } else {
        2
    }
}

/// Index of the next word-start char strictly after `col` in
/// `chars`. Returns `None` when no such position exists.
/// Mirrors vim's `w` motion within a single line: skip the rest
/// of the current word, then any whitespace, land on the first
/// non-whitespace character.
pub(super) fn next_word_start(chars: &[char], col: usize) -> Option<usize> {
    if col >= chars.len() {
        return None;
    }
    let start_class = word_class(chars[col]);
    let mut i = col + 1;
    // Skip the rest of the current run (same class as the start).
    while i < chars.len() && word_class(chars[i]) == start_class && start_class != 0 {
        i += 1;
    }
    // Skip whitespace.
    while i < chars.len() && word_class(chars[i]) == 0 {
        i += 1;
    }
    if i < chars.len() { Some(i) } else { None }
}

/// Index of the previous word-start char strictly before `col` in
/// `chars`. Returns `None` when the cursor is already at the
/// first word of the line.
pub(super) fn prev_word_start(chars: &[char], col: usize) -> Option<usize> {
    if col == 0 {
        return None;
    }
    let mut i = col.saturating_sub(1);
    // Skip whitespace backwards.
    while i > 0 && word_class(chars[i]) == 0 {
        i -= 1;
    }
    if word_class(chars[i]) == 0 {
        return None;
    }
    // Walk back to the start of the current run.
    let cur_class = word_class(chars[i]);
    while i > 0 && word_class(chars[i - 1]) == cur_class {
        i -= 1;
    }
    Some(i)
}

/// Count the number of visual rows `line` will occupy when wrapped
/// at `width`. Mirrors `wrap_line`'s greedy hard-break policy
/// (cells are filled left-to-right, breaks happen at the first
/// char that would overflow), but doesn't allocate — used by
/// `scroll_max` on every keystroke.
///
/// Empty lines render as one visual row (a blank line); this
/// matches the renderer's behavior so the math is symmetric.
/// `width == 0` yields one row to match `wrap_line`'s short-circuit.
pub(super) fn visual_rows(line: &Line<'_>, width: usize) -> usize {
    if width == 0 {
        return 1;
    }
    let total: usize = line
        .spans
        .iter()
        .flat_map(|s| s.content.chars())
        .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(0))
        .sum();
    if total == 0 {
        return 1;
    }
    total.div_ceil(width)
}

/// Split a styled line into 1+ visual rows, each at most `width`
/// columns wide. Hard-break at width if no whitespace boundary is
/// nearby (paths, long single tokens). Preserves per-span styling
/// across the break by splitting the span at the chosen byte
/// offset. Width is in unicode display columns, so wide CJK
/// characters and emoji count as 2 — same units ratatui uses for
/// layout.
pub(super) fn wrap_line(line: &Line<'static>, width: usize) -> Vec<Line<'static>> {
    if width == 0 {
        return vec![line.clone()];
    }
    let mut pieces: Vec<Vec<Span<'static>>> = vec![Vec::new()];
    let mut current_w = 0usize;
    for span in &line.spans {
        let mut rest: &str = span.content.as_ref();
        while !rest.is_empty() {
            let remaining = width.saturating_sub(current_w);
            if remaining == 0 {
                pieces.push(Vec::new());
                current_w = 0;
                continue;
            }
            let mut consumed_bytes = 0usize;
            let mut visual = 0usize;
            for (idx, ch) in rest.char_indices() {
                let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
                if visual + w > remaining {
                    break;
                }
                consumed_bytes = idx + ch.len_utf8();
                visual += w;
            }
            // Force at least one char even if it's wider than the
            // remaining budget (tiny pager boxes shouldn't infinite
            // loop on a 2-col emoji in a 1-col viewport).
            if consumed_bytes == 0
                && let Some(first) = rest.chars().next()
            {
                consumed_bytes = first.len_utf8();
                visual = unicode_width::UnicodeWidthChar::width(first).unwrap_or(1);
            }
            let chunk = rest[..consumed_bytes].to_string();
            rest = &rest[consumed_bytes..];
            if !chunk.is_empty() {
                pieces
                    .last_mut()
                    .expect("pieces seeded with one element, never emptied")
                    .push(Span::styled(chunk, span.style));
                current_w += visual;
            }
            if !rest.is_empty() {
                pieces.push(Vec::new());
                current_w = 0;
            }
        }
    }
    // Drop trailing empty piece (from a span that exactly hit width
    // and started a new row that never got content).
    if pieces.last().is_some_and(Vec::is_empty) && pieces.len() > 1 {
        pieces.pop();
    }
    pieces.into_iter().map(Line::from).collect()
}

/// Partition lines into `ncols` chunks at section boundaries (blank lines),
/// targeting roughly equal chunk sizes. The partition is **static** — it
/// does not depend on the current scroll position. Callers apply the
/// user's scroll offset independently within each chunk so the content-
/// to-column mapping stays fixed as the user scrolls.
pub(super) fn partition_lines_static(lines: &[Line<'static>], ncols: usize) -> Vec<(usize, usize)> {
    let total = lines.len();
    if ncols <= 1 || total == 0 {
        return vec![(0, total)];
    }
    let target = total / ncols;
    let mut chunks = Vec::with_capacity(ncols);
    let mut cursor = 0usize;
    for c in 0..ncols {
        if c + 1 == ncols {
            chunks.push((cursor, total));
            break;
        }
        let ideal = cursor + target;
        // Search within a window ±(target/2) of the ideal break for the
        // closest blank line. Fall back to the ideal cut if no blank
        // exists in the window (rare: implies a single section >target).
        let window_lo = cursor + 1;
        let window_hi = (ideal + target / 2).min(total);
        let mut best = ideal.min(total);
        let mut best_dist = usize::MAX;
        for (i, line_or_end) in (window_lo..=window_hi).map(|idx| (idx, lines.get(idx))) {
            let is_break = line_or_end.is_none_or(is_blank_line);
            if !is_break {
                continue;
            }
            let dist = i.abs_diff(ideal);
            if dist < best_dist {
                best_dist = dist;
                best = i;
            }
        }
        chunks.push((cursor, best));
        cursor = best;
        while cursor < total && is_blank_line(&lines[cursor]) {
            cursor += 1;
        }
    }
    chunks
}

pub(super) fn is_blank_line(line: &Line<'static>) -> bool {
    line.spans.iter().all(|s| s.content.trim().is_empty())
}

/// Where the pager's outer block should draw, given the parent
/// `area` (whatever rect the caller hands to `render`) and the
/// view's `mount` / sizing flags.
///
/// - `Mount::Overlay` keeps the pre-v1.5 dispatch: full-width if
///   the user toggled it, fit-to-content for short summaries,
///   else the centered 90×92 % box.
/// - `Mount::TopPane` / `Mount::LowerPane` use `area` as-is — the
///   caller (App::render) passes the slot's rect directly so the
///   pager fills it without extra centering.
///
/// `full_width` and `fit_to_content` are deliberately ignored for
/// the pane mounts because the slot's rect already defines the
/// pager's footprint there. We could honor them later if a use
/// case demands it.
pub(super) fn pager_inner_area(area: Rect, view: &PagerView) -> Rect {
    match view.mount {
        Mount::Overlay => {
            if view.full_width {
                area
            } else if view.fit_to_content {
                fit_height_rect(area, view)
            } else {
                centered_rect(area, CENTERED_W_PCT, 92)
            }
        }
        Mount::TopPane | Mount::LowerPane => area,
    }
}

const fn centered_rect(area: Rect, percent_w: u16, percent_h: u16) -> Rect {
    // Widen to u32 for the percentage multiply: `area.width * percent_w`
    // overflows u16 past ~728 columns (and height past ~712 rows) — panic
    // in debug, silent wrap in release. Same overflow #359 fixed in
    // `centered_body_width`; this is the sibling it missed.
    let w = (area.width as u32 * percent_w as u32 / 100) as u16;
    let h = (area.height as u32 * percent_h as u32 / 100) as u16;
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect {
        x,
        y,
        width: w,
        height: h,
    }
}

/// Same x / y / width as the standard centered pager, but shrinks from
/// the bottom: height = lines + borders + status row, capped at the
/// standard 92% height. Top edge stays where the user expects (matching
/// the regular pager origin); short summaries don't sit inside a
/// near-full-screen frame.
pub(super) fn fit_height_rect(area: Rect, view: &PagerView) -> Rect {
    const MIN_H: u16 = 5;

    let centered = centered_rect(area, CENTERED_W_PCT, 92);
    let need_h = (view.lines.len() as u16).saturating_add(3);
    let height = need_h.clamp(MIN_H.min(centered.height), centered.height);

    Rect {
        x: centered.x,
        y: centered.y,
        width: centered.width,
        height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centered_body_width_does_not_overflow_on_wide_terminals() {
        // `term_w * 90` overflows u16 past ~728 columns; the u32 widening
        // must keep it from panicking (debug) / wrapping (release).
        for term_w in [728u16, 729, 1000, 4096, u16::MAX] {
            let w = centered_body_width(term_w);
            assert!(w <= term_w, "body width {w} exceeds term width {term_w}");
        }
        // 100 * 90 / 100 - 2
        assert_eq!(centered_body_width(100), 88);
    }

    #[test]
    fn centered_col_width_does_not_overflow_on_wide_terminals() {
        // Same multiply, reached via centered_col_width — just must not panic.
        let _ = centered_col_width(u16::MAX, 3);
        assert!(centered_col_width(u16::MAX, 1) > 0);
    }

    #[test]
    fn centered_rect_does_not_overflow_on_large_areas() {
        // `area.width * percent_w` overflows u16 past ~728 cols (and height
        // past ~712 rows). The u32 widening must keep the centered rect
        // within its area on a very large terminal rather than panic/wrap.
        let area = Rect {
            x: 0,
            y: 0,
            width: u16::MAX,
            height: u16::MAX,
        };
        let r = centered_rect(area, CENTERED_W_PCT, 92);
        assert!(r.width <= area.width, "width {} > area", r.width);
        assert!(r.height <= area.height, "height {} > area", r.height);
        // Centered: a non-trivial inset on both axes (not zero, not full).
        assert!(r.x > 0 && r.y > 0, "rect not inset: {r:?}");
        // Sanity on a normal-size area: 100 * 90 / 100 = 90 wide.
        let small = Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 100,
        };
        assert_eq!(centered_rect(small, CENTERED_W_PCT, 92).width, 90);
    }
}
