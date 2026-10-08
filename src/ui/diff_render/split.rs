//! The side-by-side layout: old on the left, new on the right, each logical
//! line wrapped to its column and the shorter side padded so the two stay
//! aligned row for row.

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use unicode_segmentation::UnicodeSegmentation;

use super::{
    DiffOpts, FileHighlight, apply_bg, compute_intra, hunk_header_line, pick, prepare_file,
    styled_content, word_hl,
};
use crate::git::model::{FileDiff, Hunk, LineOrigin};
use crate::ui::display_width;
use crate::ui::theme::Theme;

/// Column separator for the side-by-side layout, and its display width.
const SEP: &str = " │ ";
const SEP_W: usize = 3;
/// Minimum width of the per-cell line-number field in the side-by-side
/// layout. Files whose largest line number needs more digits widen the
/// field to fit (see [`lnum_width`]) so the marker / separator / right
/// column stay aligned; smaller files keep this stable narrow gutter.
const LNUM_W: usize = 4;
/// Cap on the visual rows a single side-by-side cell wraps into (see
/// [`wrap_spans`]) — a backstop that bounds allocation on a pathological
/// line (e.g. minified JS on one diff line) without truncating any realistic
/// source line.
const MAX_WRAP_ROWS_PER_CELL: usize = 512;

/// Display columns `s` occupies once the pager has expanded its tabs.
///
/// The **single** width metric for the side-by-side layout: both the wrap scan
/// ([`wrap_spans`]) and the cell padding ([`split_cell_rows`]) measure with this,
/// and the layout is only correct while they agree. Two metrics that disagree by
/// one column produce exactly the raggedness this exists to prevent, so measure
/// per grapheme cluster and route it through here rather than adding a
/// per-`char` shortcut.
///
/// `unicode_width` scores a `\t` as ONE column, but the pager rewrites every
/// `\t` to `tab_width` spaces before drawing (`pager::render::expand_tabs`). The
/// side-by-side layout pads each cell to an exact width, so measuring with
/// `display_width` alone under-counts a tab-indented line by
/// `tab_width - 1` per tab — the cell then renders wider than its budget and
/// shoves the `│` separator (and the whole right column) right by that much.
/// Since indent depth varies line to line, so does the shove: the columns come
/// out visibly ragged on any tab-indented file (gofmt, Makefiles, plenty of C).
/// Tabs expand to a fixed width rather than to the next tab stop, which is what
/// makes a flat `tab_width` per tab exact.
fn tabbed_width(s: &str, tab_width: usize) -> usize {
    let tabs = s.bytes().filter(|b| *b == b'\t').count();
    display_width(s) + tabs * tab_width.max(1).saturating_sub(1)
}

pub(super) fn render_file_split(
    file: &FileDiff,
    hl: Option<&FileHighlight>,
    theme: &Theme,
    width: usize,
    opts: DiffOpts,
    out: &mut Vec<Line<'static>>,
) {
    let tab_width = opts.tab_width;
    let Some(prep) = prepare_file(file, hl, theme, out) else {
        return;
    };
    let (new_ref, old_ref) = (prep.new_hl, prep.old_hl);
    let col_w = width.saturating_sub(SEP_W) / 2;
    // Size the line-number field to the file's largest number so 5-digit+
    // files don't overflow the gutter and break column alignment.
    let lnum_w = lnum_width(prep.hunks);

    let (mut oi, mut ni) = (0usize, 0usize);
    for h in prep.hunks {
        out.push(hunk_header_line(h, theme));
        let intra = compute_intra(&h.lines, opts.intraline);
        let mut old_no = h.old_start;
        let mut new_no = h.new_start;
        let lines = &h.lines;
        let mut i = 0;
        while i < lines.len() {
            if lines[i].origin == LineOrigin::Context {
                let left_rows = split_cell_rows(
                    theme,
                    Some(old_no),
                    LineOrigin::Context,
                    styled_content(pick(old_ref, oi, &lines[i].text, theme, None), None, None),
                    col_w,
                    lnum_w,
                    tab_width,
                );
                let right_rows = split_cell_rows(
                    theme,
                    Some(new_no),
                    LineOrigin::Context,
                    styled_content(pick(new_ref, ni, &lines[i].text, theme, None), None, None),
                    col_w,
                    lnum_w,
                    tab_width,
                );
                push_split_rows(out, left_rows, right_rows, None, None, col_w, theme);
                old_no += 1;
                new_no += 1;
                oi += 1;
                ni += 1;
                i += 1;
                continue;
            }
            // A change region: the run of consecutive removes, then the run of
            // consecutive adds (git always emits removes before adds within a
            // region). Pair them row-for-row, padding the shorter side blank;
            // paired lines get the word-level highlight from `intra`. Each
            // logical diff line may wrap to multiple visual rows; the two sides
            // are zipped together, padding the shorter side with blank rows.
            let r_lo = i;
            while i < lines.len() && lines[i].origin == LineOrigin::Remove {
                i += 1;
            }
            let r_hi = i;
            let a_lo = i;
            while i < lines.len() && lines[i].origin == LineOrigin::Add {
                i += 1;
            }
            let a_hi = i;
            let pairs = (r_hi - r_lo).max(a_hi - a_lo);
            for k in 0..pairs {
                let has_left = r_lo + k < r_hi;
                let has_right = a_lo + k < a_hi;
                let left_rows = if has_left {
                    let word = word_hl(&intra[r_lo + k], theme.diff_word_bg(false));
                    let content = styled_content(
                        pick(old_ref, oi, &lines[r_lo + k].text, theme, Some(false)),
                        theme.diff_row_bg(false),
                        word,
                    );
                    let rows = split_cell_rows(
                        theme,
                        Some(old_no),
                        LineOrigin::Remove,
                        content,
                        col_w,
                        lnum_w,
                        tab_width,
                    );
                    old_no += 1;
                    oi += 1;
                    rows
                } else {
                    vec![blank_cell_row(col_w)]
                };
                let right_rows = if has_right {
                    let word = word_hl(&intra[a_lo + k], theme.diff_word_bg(true));
                    let content = styled_content(
                        pick(new_ref, ni, &lines[a_lo + k].text, theme, Some(true)),
                        theme.diff_row_bg(true),
                        word,
                    );
                    let rows = split_cell_rows(
                        theme,
                        Some(new_no),
                        LineOrigin::Add,
                        content,
                        col_w,
                        lnum_w,
                        tab_width,
                    );
                    new_no += 1;
                    ni += 1;
                    rows
                } else {
                    vec![blank_cell_row(col_w)]
                };
                let left_bg = has_left.then(|| theme.diff_row_bg(false)).flatten();
                let right_bg = has_right.then(|| theme.diff_row_bg(true)).flatten();
                push_split_rows(out, left_rows, right_rows, left_bg, right_bg, col_w, theme);
            }
        }
    }
}

/// Pair the wrapped visual rows of a left and right cell into `split_row`s,
/// padding the shorter side so both columns stay aligned. Consumes both row
/// vecs (no per-row clone).
///
/// `left_bg`/`right_bg` are that side's own row wash (`None` for a context
/// row, or for a side with no line at all this k — the caller already built
/// that as a bare [`blank_cell_row`]). They're used ONLY for the padding this
/// fn adds when one side's real wrapped rows run out before the other's: a
/// long removed line wrapping to 3 rows against a short added replacement's 1
/// row must not make that pair's bottom 2 rows go uncolored on the add side —
/// the wash should fill the whole paired block, same as `split_cell_rows`
/// washes a short row's own trailing columns instead of leaving them bare.
fn push_split_rows(
    out: &mut Vec<Line<'static>>,
    left_rows: Vec<Vec<Span<'static>>>,
    right_rows: Vec<Vec<Span<'static>>>,
    left_bg: Option<Color>,
    right_bg: Option<Color>,
    col_w: usize,
    theme: &Theme,
) {
    let n = left_rows.len().max(right_rows.len());
    let mut left = left_rows.into_iter();
    let mut right = right_rows.into_iter();
    for _ in 0..n {
        let l = left
            .next()
            .unwrap_or_else(|| washed_blank_row(col_w, left_bg));
        let r = right
            .next()
            .unwrap_or_else(|| washed_blank_row(col_w, right_bg));
        out.push(split_row(l, r, theme));
    }
}

/// One side-by-side cell split into wrapped visual rows. Each row is exactly
/// `col_w` columns wide. The first row carries `[lnum][space][marker]`;
/// continuation rows have a blank prefix of the same width so the content
/// indent stays consistent. `content` is already styled (wash + word highlight
/// via [`styled_content`]); background colours are applied to prefix + padding.
fn split_cell_rows(
    theme: &Theme,
    lnum: Option<u32>,
    origin: LineOrigin,
    content: Vec<Span<'static>>,
    col_w: usize,
    lnum_w: usize,
    tab_width: usize,
) -> Vec<Vec<Span<'static>>> {
    let (marker, row_bg, gutter_style) = match origin {
        LineOrigin::Context => (' ', None, Style::default()),
        LineOrigin::Add => ('+', theme.diff_row_bg(true), theme.diff_gutter_style(true)),
        LineOrigin::Remove => (
            '-',
            theme.diff_row_bg(false),
            theme.diff_gutter_style(false),
        ),
    };
    let lnum_str = lnum.map_or_else(|| " ".repeat(lnum_w), |n| format!("{n:>lnum_w$}"));
    let prefix_w = lnum_w + 2; // lnum + space + marker
    let content_w = col_w.saturating_sub(prefix_w);
    let pad_style = row_bg.map_or_else(Style::default, |c| Style::default().bg(c));

    wrap_spans(&content, content_w, tab_width)
        .into_iter()
        .enumerate()
        .map(|(i, row_spans)| {
            let mut spans = Vec::with_capacity(row_spans.len() + 3);
            if i == 0 {
                spans.push(Span::styled(
                    format!("{lnum_str} "),
                    apply_bg(theme.diff_meta_style(), row_bg),
                ));
                spans.push(Span::styled(
                    marker.to_string(),
                    apply_bg(gutter_style, row_bg),
                ));
            } else {
                spans.push(Span::styled(" ".repeat(prefix_w), pad_style));
            }
            let used: usize = row_spans
                .iter()
                .map(|s| tabbed_width(s.content.as_ref(), tab_width))
                .sum();
            spans.extend(row_spans);
            if used < content_w {
                spans.push(Span::styled(" ".repeat(content_w - used), pad_style));
            }
            spans
        })
        .collect()
}

/// A fully-blank side-by-side cell row: the absent side of an unbalanced
/// change region (no line at all for this k — genuinely nothing to wash).
fn blank_cell_row(col_w: usize) -> Vec<Span<'static>> {
    vec![Span::raw(" ".repeat(col_w))]
}

/// Like [`blank_cell_row`], but washed with `bg` — the shorter side's
/// continuation row when its paired line wrapped to fewer visual rows than
/// the other side's.
fn washed_blank_row(col_w: usize, bg: Option<Color>) -> Vec<Span<'static>> {
    vec![Span::styled(
        " ".repeat(col_w),
        apply_bg(Style::default(), bg),
    )]
}

/// Join a left + right cell with the column separator into one row.
fn split_row(left: Vec<Span<'static>>, right: Vec<Span<'static>>, theme: &Theme) -> Line<'static> {
    let mut spans = left;
    spans.push(Span::styled(SEP.to_string(), theme.diff_meta_style()));
    spans.extend(right);
    Line::from(spans)
}

/// Width of the line-number field for a file's side-by-side cells: the digit
/// count of the largest line number actually rendered, floored at [`LNUM_W`].
/// Without this, a number wider than the fixed field (≥ 10000 with `LNUM_W`
/// = 4) widened only that cell, shoving the marker, separator, and entire
/// right column out of alignment for the rest of the file.
pub(super) fn lnum_width(hunks: &[Hunk]) -> usize {
    let mut max_no = 0u32;
    for h in hunks {
        let (mut old, mut new) = (h.old_start, h.new_start);
        for line in &h.lines {
            match line.origin {
                LineOrigin::Context => {
                    max_no = max_no.max(old).max(new);
                    old += 1;
                    new += 1;
                }
                LineOrigin::Remove => {
                    max_no = max_no.max(old);
                    old += 1;
                }
                LineOrigin::Add => {
                    max_no = max_no.max(new);
                    new += 1;
                }
            }
        }
    }
    let digits = if max_no == 0 {
        1
    } else {
        (max_no.ilog10() + 1) as usize
    };
    digits.max(LNUM_W)
}

/// Split `spans` into visual rows of at most `width` display columns each,
/// preserving span styles across row boundaries. Returns at least one row
/// (an empty row when `spans` is empty or `width` is zero).
pub(super) fn wrap_spans(
    spans: &[Span<'static>],
    width: usize,
    tab_width: usize,
) -> Vec<Vec<Span<'static>>> {
    if width == 0 {
        return vec![spans.to_vec()];
    }
    let mut pieces: Vec<Vec<Span<'static>>> = vec![Vec::new()];
    let mut current_w = 0usize;
    'outer: for span in spans {
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
            // Advance by grapheme cluster, measured with `tabbed_width` — the
            // same fn the cell padding uses. A per-`char` walk under-counts an
            // emoji-presentation sequence (base + U+FE0F: 1 + 0 per char, 2 as
            // a cluster), so it would admit a column the padding then believes
            // is already occupied, and the cell would overrun its budget.
            for (idx, cluster) in rest.grapheme_indices(true) {
                let w = tabbed_width(cluster, tab_width);
                if visual + w > remaining {
                    break;
                }
                consumed_bytes = idx + cluster.len();
                visual += w;
            }
            // Force at least one cluster even if it's wider than `remaining`
            // so a 2-col glyph in a 1-col viewport doesn't infinite-loop.
            if consumed_bytes == 0
                && let Some(first) = rest.graphemes(true).next()
            {
                consumed_bytes = first.len();
                visual = tabbed_width(first, tab_width).max(1);
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
                if pieces.len() >= MAX_WRAP_ROWS_PER_CELL {
                    break 'outer;
                }
                pieces.push(Vec::new());
                current_w = 0;
            }
        }
    }
    // Drop a trailing empty piece created when a span exactly fills a row.
    if pieces.last().is_some_and(Vec::is_empty) && pieces.len() > 1 {
        pieces.pop();
    }
    pieces
}
