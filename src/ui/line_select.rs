//! Column-range selection over a rendered [`Line`] — the primitive the
//! single-line chrome surfaces (status bar, pane divider) use for drag-select.
//!
//! Both operations walk the line's spans accumulating **display width**, not byte
//! or char counts, because a screen column is a width. The status bar opens with a
//! 2-cell `🌶️` and can carry wide glyphs in a session name or a path, so a
//! byte/char index would drift from the column the pointer was actually over — and
//! the highlight and the copied text would then disagree about what was selected.
//! One shared walk, so they cannot.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

/// The text occupying columns `lo..=hi` of `line`.
///
/// A char straddling `lo` or `hi` (a wide glyph the range only half covers) is
/// included: a partial cell is still a character the user pointed at, and
/// returning half a glyph is not an option.
pub fn text_between_columns(line: &Line<'_>, lo: usize, hi: usize) -> String {
    let mut out = String::new();
    let mut col = 0usize;
    for span in &line.spans {
        for ch in span.content.chars() {
            let w = unicode_width::UnicodeWidthChar::width(ch)
                .unwrap_or(0)
                .max(1);
            let end = col + w - 1; // last column this char occupies
            if col <= hi && end >= lo {
                out.push(ch);
            }
            col += w;
            if col > hi {
                return out;
            }
        }
    }
    out
}

/// `line` with columns `lo..=hi` reverse-video, for the selection highlight.
///
/// Reverse rather than a theme background: the chrome lines are powerline segments
/// that already set their own per-segment backgrounds, so a fixed bg would be
/// invisible against whichever segment happens to use it. Reverse is relative to
/// whatever each cell already is.
///
/// Splits spans at the range boundaries, preserving each piece's original style —
/// the same shape as the pager's `paint_block_selection`, generalized to any line.
pub fn highlight_columns(line: &Line<'static>, lo: usize, hi: usize) -> Line<'static> {
    let mut out: Vec<Span<'static>> = Vec::with_capacity(line.spans.len() + 2);
    let mut col = 0usize;
    for span in &line.spans {
        // Accumulate runs of same-selectedness so the output isn't one span per
        // character, which would bloat every frame's diff.
        let mut run = String::new();
        let mut run_selected: Option<bool> = None;
        let flush = |out: &mut Vec<Span<'static>>, run: &mut String, sel: Option<bool>| {
            if run.is_empty() {
                return;
            }
            let style: Style = if sel == Some(true) {
                span.style.add_modifier(Modifier::REVERSED)
            } else {
                span.style
            };
            out.push(Span::styled(std::mem::take(run), style));
        };
        for ch in span.content.chars() {
            let w = unicode_width::UnicodeWidthChar::width(ch)
                .unwrap_or(0)
                .max(1);
            let end = col + w - 1;
            let selected = col <= hi && end >= lo;
            if run_selected != Some(selected) {
                flush(&mut out, &mut run, run_selected);
                run_selected = Some(selected);
            }
            run.push(ch);
            col += w;
        }
        flush(&mut out, &mut run, run_selected);
    }
    Line::from(out)
}

#[cfg(test)]
mod tests {
    use super::{highlight_columns, text_between_columns};
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::{Line, Span};

    fn seg_line() -> Line<'static> {
        // Three styled segments, as the status bar builds them.
        Line::from(vec![
            Span::styled("spyc", Style::default().fg(Color::Red)),
            Span::styled("|", Style::default().fg(Color::Blue)),
            Span::styled("main*", Style::default().fg(Color::Green)),
        ])
    }

    /// A range can start and end mid-span — that's the point of substring
    /// selection: copy just the branch name, not the whole line.
    #[test]
    fn text_between_columns_crosses_span_boundaries() {
        let l = seg_line();
        assert_eq!(text_between_columns(&l, 0, 3), "spyc");
        assert_eq!(text_between_columns(&l, 5, 9), "main*", "just the branch");
        assert_eq!(text_between_columns(&l, 2, 6), "yc|ma", "across two seams");
        assert_eq!(text_between_columns(&l, 0, 99), "spyc|main*", "clamped");
    }

    #[test]
    fn text_between_columns_single_column() {
        assert_eq!(text_between_columns(&seg_line(), 4, 4), "|");
    }

    /// Columns are display widths, not char indices. With a 2-cell leading glyph,
    /// column 2 must be the character AFTER it — this is the drift that would make
    /// the highlight and the copy disagree.
    ///
    /// Uses CJK, which is unambiguously width 2 in `unicode-width`. Emoji width is
    /// table- and version-dependent (bare U+1F336 `🌶` measures 1, not 2), and
    /// pinning that here would test the width crate rather than this walk.
    #[test]
    fn columns_are_display_widths_not_char_indices() {
        let l = Line::from(vec![Span::raw("中ab")]);
        assert_eq!(text_between_columns(&l, 0, 1), "中");
        assert_eq!(text_between_columns(&l, 2, 2), "a");
        assert_eq!(text_between_columns(&l, 3, 3), "b");
    }

    /// A wide glyph only half-covered by the range is still included — a partial
    /// cell is a character the user pointed at, and half a glyph isn't a thing.
    #[test]
    fn a_straddled_wide_char_is_included() {
        let l = Line::from(vec![Span::raw("a中b")]);
        // `中` occupies columns 1-2; a range of just column 2 covers its right half.
        assert_eq!(text_between_columns(&l, 2, 2), "中", "right half only");
        assert_eq!(text_between_columns(&l, 1, 1), "中", "left half only");
    }

    /// The highlight reverses exactly the selected columns and preserves each
    /// piece's original style, so a segment's own colours survive.
    #[test]
    fn highlight_reverses_only_the_selected_columns() {
        let out = highlight_columns(&seg_line(), 5, 9);
        // Reconstruct which columns came back REVERSED.
        let mut reversed_cols = Vec::new();
        let mut col = 0usize;
        for s in &out.spans {
            let rev = s.style.add_modifier.contains(Modifier::REVERSED);
            for _ in s.content.chars() {
                if rev {
                    reversed_cols.push(col);
                }
                col += 1;
            }
        }
        assert_eq!(reversed_cols, vec![5, 6, 7, 8, 9]);
        // Text is unchanged.
        let text: String = out.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "spyc|main*");
        // The green of the branch segment survives under the reverse.
        let branch = out
            .spans
            .iter()
            .find(|s| s.content.contains("main"))
            .expect("branch span");
        assert_eq!(branch.style.fg, Some(Color::Green));
    }

    #[test]
    fn highlight_with_a_range_past_the_end_is_harmless() {
        let out = highlight_columns(&seg_line(), 50, 60);
        let any_rev = out
            .spans
            .iter()
            .any(|s| s.style.add_modifier.contains(Modifier::REVERSED));
        assert!(!any_rev, "nothing on the line is in range");
    }
}
