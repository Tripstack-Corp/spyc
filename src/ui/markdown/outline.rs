//! The document outline behind the pager's Markdown folding.
//!
//! Pure: headings + a fold set in, a line mapping out. No `PagerView`, no
//! `App` — the `route.rs` / `focus.rs` template (a snapshot, a pure fn, unit
//! tests), because "which lines does this section own" is the part that is
//! easy to get subtly wrong and impossible to see in a screenshot.
//!
//! Folding rewrites the pager's flat `lines` buffer rather than filtering at
//! draw time. Every consumer in `ui::pager` — scroll clamp, search, wrap, yank,
//! the mermaid hit-test — indexes `lines` directly, so a hidden-line set would
//! have to be threaded through all of them and any one that forgot would be a
//! silent off-by-N. A rebuilt buffer is still a flat buffer; it just has fewer
//! lines, and [`Folded::kept`] carries what moved where so the ranges that
//! index it can be remapped.

use ratatui::style::Style;
use ratatui::text::{Line, Span};

/// One heading, as rendered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    /// ATX level, 1-6.
    pub level: u8,
    /// Index into the *unfolded* rendered lines of the heading's own row.
    pub line: usize,
    /// Heading text without the `#` prefix, for the fold placeholder.
    pub text: String,
}

/// The result of applying a fold set to a rendered document.
pub struct Folded {
    pub lines: Vec<Line<'static>>,
    /// For each emitted line, the unfolded index it came from. A placeholder
    /// row maps to the heading it replaced, so remapping a range through this
    /// never lands on nothing.
    pub kept: Vec<usize>,
}

/// Lines belonging to heading `i` — its body, NOT its own row.
///
/// Runs to the next heading of the same level or shallower; a deeper heading is
/// nested content and folds with its parent. Comparing on `<=` is what makes
/// `##` collapse the `###`s under it while stopping at the next `##`.
pub fn section_body(headings: &[Heading], i: usize, total_lines: usize) -> std::ops::Range<usize> {
    let Some(h) = headings.get(i) else {
        return 0..0;
    };
    let start = h.line.saturating_add(1);
    let end = headings[i + 1..]
        .iter()
        .find(|next| next.level <= h.level)
        .map_or(total_lines, |next| next.line);
    start..end.max(start)
}

/// Which heading owns `line` — the nearest one at or above it.
///
/// `None` only for a line above the first heading (a preamble, or front
/// matter), which belongs to no section and therefore cannot be folded.
pub fn heading_at_or_above(headings: &[Heading], line: usize) -> Option<usize> {
    headings.iter().rposition(|h| h.line <= line)
}

/// Rebuild `full` with every folded section's body removed.
///
/// A folded heading keeps its own row and gains a `▸` marker plus a line count,
/// so a collapsed section is visibly collapsed rather than just absent. Nested
/// folds inside an already-folded parent contribute nothing extra — the
/// parent's range already covers them — which is why this walks line indices
/// once instead of unioning per-heading ranges.
pub fn apply(
    full: &[Line<'static>],
    headings: &[Heading],
    folded: &std::collections::BTreeSet<usize>,
    marker: Style,
) -> Folded {
    let mut out = Folded {
        lines: Vec::with_capacity(full.len()),
        kept: Vec::with_capacity(full.len()),
    };
    let mut i = 0usize;
    while i < full.len() {
        let fold_here = heading_at_or_above(headings, i)
            .filter(|h| headings[*h].line == i && folded.contains(h));
        let Some(h) = fold_here else {
            out.lines.push(full[i].clone());
            out.kept.push(i);
            i += 1;
            continue;
        };
        let body = section_body(headings, h, full.len());
        let hidden = body.len();
        let mut spans = full[i].spans.clone();
        spans.push(Span::styled(
            format!(
                "  \u{25b8} {hidden} line{}",
                if hidden == 1 { "" } else { "s" }
            ),
            marker,
        ));
        out.lines.push(Line::from(spans));
        out.kept.push(i);
        // Skip the body wholesale. Anything folded *inside* it is already gone,
        // so a nested fold needs no separate handling.
        i = body.end.max(i + 1);
    }
    out
}

/// Remap a range that indexed the unfolded lines into the folded ones.
///
/// `None` when the range's start was folded away — the caller (the mermaid
/// hit-test) then correctly sees no block there, because there is none on
/// screen.
pub fn remap_range(
    kept: &[usize],
    range: &std::ops::Range<usize>,
) -> Option<std::ops::Range<usize>> {
    let start = kept.iter().position(|&k| k == range.start)?;
    // The end is exclusive and may itself be folded; take the last kept line
    // strictly inside the range, or collapse to a single row.
    let end = kept
        .iter()
        .rposition(|&k| k < range.end && k >= range.start)
        .map_or(start + 1, |p| p + 1);
    Some(start..end.max(start + 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn h(level: u8, line: usize) -> Heading {
        Heading {
            level,
            line,
            text: format!("h{level}@{line}"),
        }
    }

    /// `# 0 / ## 4 / ### 8 / ## 12`, 15 lines — the shape the doc probe renders.
    fn doc() -> Vec<Heading> {
        vec![h(1, 0), h(2, 4), h(3, 8), h(2, 12)]
    }

    /// A section owns its nested children and stops at the next sibling. The
    /// `<=` comparison is the whole rule: `<` would run a `##` past the next
    /// `##`, and `==` would stop it at the first `###` inside it.
    #[test]
    fn a_section_swallows_deeper_headings_and_stops_at_the_next_sibling() {
        let hs = doc();
        assert_eq!(section_body(&hs, 1, 15), 5..12, "## Two owns ### Three");
        assert_eq!(
            section_body(&hs, 2, 15),
            9..12,
            "### Three owns only its own"
        );
        assert_eq!(
            section_body(&hs, 3, 15),
            13..15,
            "the last section runs to EOF"
        );
        assert_eq!(
            section_body(&hs, 0, 15),
            1..15,
            "# One owns the whole document"
        );
    }

    /// Out of range, and a heading whose body is empty (two headings in a row).
    #[test]
    fn section_body_is_empty_rather_than_inverted_at_the_edges() {
        assert_eq!(section_body(&doc(), 99, 15), 0..0, "no such heading");
        // Same level and adjacent — the only way a body is genuinely empty. A
        // DEEPER heading on the next line would be nested content, so that
        // section correctly runs on rather than measuring zero.
        let back_to_back = vec![h(2, 0), h(2, 1)];
        assert_eq!(
            section_body(&back_to_back, 0, 2),
            1..1,
            "an empty body is empty, never a reversed range"
        );
        let nested = vec![h(1, 0), h(2, 1)];
        assert_eq!(
            section_body(&nested, 0, 2),
            1..2,
            "a deeper heading right below is content, not a terminator"
        );
        // A `total_lines` shorter than the heading line can't produce end < start.
        let r = section_body(&doc(), 3, 5);
        assert!(r.start <= r.end, "range stayed ordered: {r:?}");
    }

    /// Lines above the first heading belong to no section — a preamble or, now
    /// that front matter renders, the YAML block. Folding must not claim them.
    #[test]
    fn lines_before_the_first_heading_belong_to_no_section() {
        let hs = vec![h(1, 3)];
        assert_eq!(heading_at_or_above(&hs, 0), None);
        assert_eq!(heading_at_or_above(&hs, 2), None);
        assert_eq!(
            heading_at_or_above(&hs, 3),
            Some(0),
            "the heading's own row"
        );
        assert_eq!(heading_at_or_above(&hs, 9), Some(0));
    }

    fn lines(n: usize) -> Vec<Line<'static>> {
        (0..n)
            .map(|i| Line::from(Span::raw(format!("L{i}"))))
            .collect()
    }

    fn text_of(l: &Line<'static>) -> String {
        l.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    /// Folding a section hides its body, keeps its heading, and annotates it.
    #[test]
    fn apply_hides_the_body_and_marks_the_heading() {
        let hs = doc();
        let full = lines(15);
        let folded = BTreeSet::from([1]);
        let out = apply(&full, &hs, &folded, Style::default());

        assert_eq!(out.lines.len(), 8, "7 body lines went away");
        assert_eq!(out.kept, vec![0, 1, 2, 3, 4, 12, 13, 14]);
        assert!(
            text_of(&out.lines[4]).contains("\u{25b8} 7 lines"),
            "the collapsed heading says how much is hidden: {:?}",
            text_of(&out.lines[4])
        );
        assert_eq!(text_of(&out.lines[5]), "L12", "the next sibling survives");
    }

    /// A fold nested inside an already-folded parent contributes nothing extra.
    /// Unioning per-heading ranges would double-count and could skip past the
    /// parent's end; walking line indices once cannot.
    #[test]
    fn a_fold_inside_a_folded_parent_changes_nothing() {
        let hs = doc();
        let full = lines(15);
        let parent_only = apply(&full, &hs, &BTreeSet::from([1]), Style::default());
        let parent_and_child = apply(&full, &hs, &BTreeSet::from([1, 2]), Style::default());
        assert_eq!(
            parent_and_child.kept, parent_only.kept,
            "the child is already hidden by its parent"
        );
    }

    #[test]
    fn folding_everything_leaves_only_headings() {
        let hs = doc();
        let out = apply(&lines(15), &hs, &(0..hs.len()).collect(), Style::default());
        // `# One` owns the entire document, so collapsing all of it leaves the
        // one top-level heading — not four.
        assert_eq!(out.kept, vec![0], "the outermost fold subsumes the rest");
    }

    /// The mermaid hit-test indexes rendered lines, so a fold has to move its
    /// ranges with the content — and drop the ones that are no longer on screen.
    #[test]
    fn remap_moves_surviving_ranges_and_drops_hidden_ones() {
        let kept = vec![0, 1, 2, 3, 4, 12, 13, 14];
        assert_eq!(
            remap_range(&kept, &(12..15)),
            Some(5..8),
            "a block after the fold shifts up by the hidden count"
        );
        assert_eq!(
            remap_range(&kept, &(6..8)),
            None,
            "a block inside the folded section is not on screen"
        );
        assert_eq!(
            remap_range(&kept, &(0..3)),
            Some(0..3),
            "a block before the fold is untouched"
        );
        // A range whose start survives but whose body is folded collapses to the
        // single row it still occupies, never to an empty or inverted range.
        let r = remap_range(&kept, &(4..12)).expect("start survives");
        assert!(r.end > r.start, "range stayed non-empty: {r:?}");
    }
}
