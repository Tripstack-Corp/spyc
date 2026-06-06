//! Pure word-wrap + span-slicing helpers for the markdown renderer.
//!
//! These operate on plain text and `Span` sequences with no `Renderer`
//! state, so they live apart from the event state machine. `super::renderer`
//! uses them to wrap prose paragraphs and table cells at a target column
//! width. Split out of `markdown.rs` verbatim during the 800-LoC
//! decomposition — behavior-identical.

use ratatui::text::Span;

/// Compute byte-range break points for word-wrapping `text` at
/// `width` visual columns. Prefers breaks at whitespace; falls back
/// to a hard break when no whitespace exists in the budget. The
/// whitespace at break points is *consumed* — the next range starts
/// after it — so wrapped lines don't begin with a stray space.
pub(super) fn word_wrap_ranges(text: &str, width: usize) -> Vec<(usize, usize)> {
    if text.is_empty() {
        return vec![(0, 0)];
    }
    let width = width.max(1);
    let mut ranges = Vec::new();
    let mut line_start = 0usize;
    let mut last_space_end: Option<usize> = None;
    let mut col = 0usize;
    for (idx, ch) in text.char_indices() {
        let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        // Track byte position immediately after the last whitespace,
        // so we can break right after a word ends without leading
        // space on the next row.
        if ch == ' ' {
            last_space_end = Some(idx + ch.len_utf8());
            col += cw;
            continue;
        }
        if col + cw > width && idx > line_start {
            // Need a break. Prefer the last whitespace if we saw one
            // since the line started; else hard-break before this
            // char.
            let break_pos = last_space_end
                .filter(|&p| p > line_start && p <= idx)
                .unwrap_or(idx);
            // End of the previous range trims trailing whitespace.
            let trimmed_end = trim_trailing_space_end(text, break_pos);
            ranges.push((line_start, trimmed_end));
            line_start = break_pos;
            last_space_end = None;
            // Recompute col for content already past break_pos up to idx.
            col = text[break_pos..idx]
                .chars()
                .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(0))
                .sum::<usize>()
                + cw;
        } else {
            col += cw;
        }
    }
    let final_end = trim_trailing_space_end(text, text.len());
    if line_start < final_end {
        ranges.push((line_start, final_end));
    } else if ranges.is_empty() {
        // Whitespace-only or empty after trimming — preserve a single
        // empty range so callers can still emit a (possibly prefix-
        // only) row if they want.
        ranges.push((line_start, text.len()));
    }
    ranges
}

/// Walk back from `end` past trailing ASCII spaces. Used so wrap
/// boundaries don't carry visible trailing whitespace into yanked
/// text or the rendered display.
fn trim_trailing_space_end(text: &str, end: usize) -> usize {
    let bytes = text.as_bytes();
    let mut e = end;
    while e > 0 && bytes[e - 1] == b' ' {
        e -= 1;
    }
    e
}

/// Slice a sequence of styled spans by a byte range over the
/// concatenated plain text. Spans that fall outside the range are
/// dropped; spans that straddle the boundary are split at the byte
/// offset, preserving their style on the kept portion. Used to
/// reconstruct each wrapped row's spans from the original
/// paragraph's spans.
pub(super) fn slice_spans(spans: &[Span<'static>], start: usize, end: usize) -> Vec<Span<'static>> {
    let mut out = Vec::new();
    let mut cursor = 0usize;
    for span in spans {
        let span_start = cursor;
        let span_end = cursor + span.content.len();
        cursor = span_end;
        if span_end <= start {
            continue;
        }
        if span_start >= end {
            break;
        }
        let lo = start.saturating_sub(span_start);
        let hi = (end - span_start).min(span.content.len());
        // Only keep slices that lie on UTF-8 char boundaries; if the
        // wrap point happens to land mid-char (rare given we walk
        // char_indices in word_wrap_ranges), back up to the nearest
        // boundary by extending the chunk one byte at a time.
        let lo = floor_char_boundary(&span.content, lo);
        let hi = floor_char_boundary(&span.content, hi);
        if hi > lo {
            let chunk = span.content[lo..hi].to_string();
            out.push(Span::styled(chunk, span.style));
        }
    }
    out
}

const fn floor_char_boundary(s: &str, mut idx: usize) -> usize {
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

/// Visual width (terminal columns) of a styled span sequence,
/// computed via `unicode-width`. Used by the table renderer to
/// size columns from natural cell content.
pub(super) fn spans_visual_width(spans: &[Span<'static>]) -> usize {
    use unicode_width::UnicodeWidthStr;
    spans.iter().map(|s| s.content.as_ref().width()).sum()
}

/// Wrap a styled span sequence into one or more visual rows, each
/// at most `max_w` visual columns wide. Uses the same
/// `word_wrap_ranges` routine as paragraph wrap (par-style word
/// boundaries with hard-break fallback for unbreakable tokens).
/// Per-span styling is preserved across wrap boundaries via
/// `slice_spans`. Used by the table renderer so cells can flow to
/// multiple visual rows instead of truncating with `…`.
pub(super) fn wrap_spans_to_width(
    spans: &[Span<'static>],
    max_w: usize,
) -> Vec<Vec<Span<'static>>> {
    if spans.is_empty() || max_w == 0 {
        return vec![spans.to_vec()];
    }
    let plain: String = spans.iter().map(|s| s.content.as_ref()).collect();
    if plain.is_empty() {
        return vec![Vec::new()];
    }
    word_wrap_ranges(&plain, max_w)
        .into_iter()
        .map(|(s, e)| slice_spans(spans, s, e))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_wrap_ranges_breaks_at_spaces() {
        let s = "hello world foo bar baz";
        let ranges = word_wrap_ranges(s, 11);
        let pieces: Vec<&str> = ranges.iter().map(|&(a, b)| &s[a..b]).collect();
        assert_eq!(pieces, vec!["hello world", "foo bar baz"]);
    }

    #[test]
    fn word_wrap_ranges_hard_breaks_when_no_space() {
        // No spaces ⇒ hard break at width.
        let s = "abcdefghijklmnopqrstuvwxyz";
        let ranges = word_wrap_ranges(s, 10);
        let pieces: Vec<&str> = ranges.iter().map(|&(a, b)| &s[a..b]).collect();
        assert_eq!(pieces, vec!["abcdefghij", "klmnopqrst", "uvwxyz"]);
    }
}
