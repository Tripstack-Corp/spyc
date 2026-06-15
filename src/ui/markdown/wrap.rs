//! Pure word-wrap + span-slicing helpers for the markdown renderer.
//!
//! These operate on plain text and `Span` sequences with no `Renderer`
//! state, so they live apart from the event state machine. `super::renderer`
//! uses them to wrap prose paragraphs and table cells at a target column
//! width. Split out of `markdown.rs` verbatim during the 800-LoC
//! decomposition — behavior-identical.

use ratatui::text::Span;

use crate::ui::wrap::word_wrap_ranges;

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
