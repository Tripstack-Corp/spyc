//! Pure text word-wrap shared across the UI.
//!
//! `word_wrap_ranges` is the single greedy word-wrap routine used by both the
//! markdown renderer (prose paragraphs, table cells — see
//! [`crate::ui::markdown`]) and the `?` help screen's description column
//! (`crate::ui::help`). It operates on plain `&str` with no widget/`Renderer`
//! state, so it lives at the `ui` root rather than inside either consumer.

/// Compute byte-range break points for word-wrapping `text` at `width` visual
/// columns. Prefers breaks at whitespace; falls back to a hard break when no
/// whitespace exists in the budget. The whitespace at break points is
/// *consumed* — the next range starts after it — so wrapped lines don't begin
/// with a stray space, and trailing whitespace is trimmed off each range.
///
/// Advances one **grapheme cluster** at a time and measures each with
/// `UnicodeWidthStr`, because a cluster's width is not the sum of its chars'
/// widths: `"❤️"` (U+2764 U+FE0F) is 2 columns where its chars sum to 1, and a
/// ZWJ emoji sums to 6 where the terminal draws 2. A per-char walk therefore
/// both mismeasured the budget and could break *inside* a cluster.
pub fn word_wrap_ranges(text: &str, width: usize) -> Vec<(usize, usize)> {
    use unicode_segmentation::UnicodeSegmentation;
    use unicode_width::UnicodeWidthStr;

    if text.is_empty() {
        return vec![(0, 0)];
    }
    let width = width.max(1);
    let mut ranges = Vec::new();
    let mut line_start = 0usize;
    let mut last_space_end: Option<usize> = None;
    let mut col = 0usize;
    for (idx, g) in text.grapheme_indices(true) {
        let cw = g.width();
        // Track byte position immediately after the last whitespace,
        // so we can break right after a word ends without leading
        // space on the next row.
        if g == " " {
            last_space_end = Some(idx + g.len());
            col += cw;
            continue;
        }
        if col + cw > width && idx > line_start {
            // Need a break. Prefer the last whitespace if we saw one
            // since the line started; else hard-break before this
            // cluster.
            let break_pos = last_space_end
                .filter(|&p| p > line_start && p <= idx)
                .unwrap_or(idx);
            // End of the previous range trims trailing whitespace.
            let trimmed_end = trim_trailing_space_end(text, break_pos);
            ranges.push((line_start, trimmed_end));
            line_start = break_pos;
            last_space_end = None;
            // Recompute col for content already past break_pos up to idx.
            col = text[break_pos..idx].width() + cw;
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

    // ── grapheme-cluster boundaries ───────────────────────────────
    //
    // Clusters whose width is NOT the sum of their chars' widths.
    const HEART: &str = "\u{2764}\u{FE0F}"; // ❤️ 2 cols, chars sum to 1
    const FAMILY: &str = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}"; // 2 cols, chars sum to 6
    const ACUTE: &str = "e\u{0301}"; // é 1 col

    fn dw(s: &str) -> usize {
        unicode_width::UnicodeWidthStr::width(s)
    }

    #[test]
    fn a_zwj_sequence_is_never_broken_mid_cluster() {
        // Walking chars, the width-2 👩 in the middle overflowed a width-3 row
        // and forced a break at its byte offset — emitting a "👨\u{200d}"
        // fragment, which renders as a lone man plus a stray joiner.
        let s = FAMILY.repeat(2);
        let ranges = word_wrap_ranges(&s, 3);
        let pieces: Vec<&str> = ranges.iter().map(|&(a, b)| &s[a..b]).collect();
        assert_eq!(
            pieces,
            vec![FAMILY, FAMILY],
            "each row holds one whole family"
        );
    }

    #[test]
    fn a_variation_selector_pair_is_budgeted_at_its_rendered_width() {
        // Per-char, ❤️ measured 1 column, so three of them "fit" a width-3 row
        // that actually renders 6 columns wide.
        let s = HEART.repeat(3);
        for &(a, b) in &word_wrap_ranges(&s, 3) {
            assert!(dw(&s[a..b]) <= 3, "row {:?} renders wider than 3", &s[a..b]);
        }
    }

    #[test]
    fn wrap_breaks_only_on_cluster_boundaries_and_holds_the_budget() {
        use unicode_segmentation::UnicodeSegmentation;
        for unit in [HEART, FAMILY, ACUTE] {
            let s = unit.repeat(6);
            let bounds: Vec<usize> = s
                .grapheme_indices(true)
                .map(|(i, _)| i)
                .chain(std::iter::once(s.len()))
                .collect();
            for width in 1..=8usize {
                for &(a, b) in &word_wrap_ranges(&s, width) {
                    assert!(
                        bounds.contains(&a),
                        "{unit:?}@{width}: start {a} mid-cluster"
                    );
                    assert!(bounds.contains(&b), "{unit:?}@{width}: end {b} mid-cluster");
                    let piece = &s[a..b];
                    // A cluster wider than the whole budget is forced onto a row
                    // of its own, so that is the one legal way to exceed width.
                    assert!(
                        dw(piece) <= width || piece.graphemes(true).count() == 1,
                        "{unit:?}@{width}: row {piece:?} is {} cols",
                        dw(piece)
                    );
                }
            }
        }
    }
}
