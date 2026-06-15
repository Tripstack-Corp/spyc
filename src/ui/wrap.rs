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
pub fn word_wrap_ranges(text: &str, width: usize) -> Vec<(usize, usize)> {
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
