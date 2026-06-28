//! Pure renderer: a [`DiffModel`] / [`CommitMeta`] → styled `ratatui` lines.
//!
//! This is the in-house replacement for piping `git diff --color=always`
//! bytes through the pager. It produces `Vec<Line<'static>>` from the
//! structured `DiffModel` (`crate::git::model`) — so search, wrap,
//! line-numbers, and visual-yank all work, and (crucially) we can lay the
//! same model out
//! either **unified** or **side-by-side**, which colored-byte output can't.
//!
//! Two layouts over the same model:
//! * [`DiffLayout::Unified`] — git's classic one-column `+`/`-` view.
//! * [`DiffLayout::SideBySide`] — old on the left, new on the right, paired
//!   and aligned; needs a viewport width to size its columns.
//!
//! Syntax highlighting reuses [`crate::ui::syntax::highlight_to_lines`]
//! (syntect) — highlighted **once per side** (syntect is stateful across
//! lines, so per-line calls would break multi-line strings/comments), then
//! each line gets its `+`/`-` gutter and a non-destructive row-background
//! tint overlaid (syntect sets only `fg`, so language colors survive).
//!
//! Pure: `model + &Theme → lines`, no IO, no gix, no `&mut self`. Wired into
//! the pager via the git-view session in `app/git_view_session.rs`.

use std::ops::Range;

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthChar;

use crate::git::model::{CommitMeta, DiffKind, DiffModel, FileDiff, FileStatus, Hunk, LineOrigin};
use crate::ui::display_width;
use crate::ui::theme::Theme;

/// How a diff is laid out in the pager.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLayout {
    /// One column, git-style `+`/`-` gutter.
    Unified,
    /// Two columns: old (left) vs new (right), paired and aligned.
    SideBySide,
}

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

/// A diff's syntax highlight, computed once and reused across every
/// layout/width re-render. syntect (in [`highlight_side`]) is by far the most
/// expensive part of rendering a diff, and it depends only on the model — not
/// on the theme (syntect carries its own palette; the diff wash/word colors are
/// overlaid later) nor the width/layout. So the `|` layout toggle, `f`
/// full-width toggle, and a terminal resize re-lay-out from this cache instead
/// of re-highlighting up to [`crate::git::diff_model::MAX_DIFF_LINES`] lines
/// each time. Index-aligned with `model.files`. Build with [`highlight_diff`].
pub struct DiffHighlight {
    files: Vec<FileHighlight>,
}

/// One file's cached per-side highlight (`None` per side for an unknown
/// language or a non-text kind — callers fall back to flat `+`/`-` text).
struct FileHighlight {
    new_hl: Option<Vec<Line<'static>>>,
    old_hl: Option<Vec<Line<'static>>>,
}

/// Syntax-highlight every file in `model`, once, into a reusable [`DiffHighlight`].
/// This is the expensive (syntect) half of rendering; [`render_diff_highlighted`]
/// is the cheap width-dependent layout half that consumes the result.
pub fn highlight_diff(model: &DiffModel) -> DiffHighlight {
    DiffHighlight {
        files: model.files.iter().map(highlight_file).collect(),
    }
}

/// Highlight one file's two sides. Non-text kinds (binary / submodule / error)
/// carry no highlight — the layout pass renders their one-line explanation.
fn highlight_file(file: &FileDiff) -> FileHighlight {
    let DiffKind::Text(hunks) = &file.kind else {
        return FileHighlight {
            new_hl: None,
            old_hl: None,
        };
    };
    let (new_text, old_text) = side_texts(hunks);
    FileHighlight {
        new_hl: highlight_side(file_name(file), &new_text),
        old_hl: highlight_side(file_name(file), &old_text),
    }
}

/// Render a whole diff to styled lines in the chosen `layout`. `width` is the
/// total viewport width in columns (used only by [`DiffLayout::SideBySide`] to
/// size its two columns; ignored for unified). Highlights the diff inline;
/// callers that re-render the same model at multiple widths/layouts should
/// cache [`highlight_diff`] and call [`render_diff_highlighted`] instead.
pub fn render_diff(
    model: &DiffModel,
    theme: &Theme,
    layout: DiffLayout,
    width: usize,
) -> Vec<Line<'static>> {
    render_diff_highlighted(model, &highlight_diff(model), theme, layout, width)
}

/// Lay out a diff at `width`/`layout` using a precomputed [`DiffHighlight`].
/// `hl` must come from `highlight_diff(model)` (index-aligned with
/// `model.files`); a mismatched/short entry falls back to unhighlighted text.
pub fn render_diff_highlighted(
    model: &DiffModel,
    hl: &DiffHighlight,
    theme: &Theme,
    layout: DiffLayout,
    width: usize,
) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    if model.files.is_empty() {
        out.push(Line::styled("No changes.", theme.diff_meta_style()));
        return out;
    }
    for (i, file) in model.files.iter().enumerate() {
        if i > 0 {
            out.push(Line::default()); // blank separator between files
        }
        let fhl = hl.files.get(i);
        match layout {
            DiffLayout::Unified => render_file_unified(file, fhl, theme, &mut out),
            DiffLayout::SideBySide => render_file_split(file, fhl, theme, width, &mut out),
        }
    }
    if model.truncated {
        out.push(Line::default());
        out.push(Line::styled(
            "… diff truncated (too large to display in full) …",
            theme.diff_meta_style(),
        ));
    }
    out
}

/// Render `git show <rev>`: the commit-metadata header block followed by the
/// commit's diff in the chosen `layout`. Highlights inline; see
/// [`render_show_highlighted`] for the cached-highlight variant.
pub fn render_show(
    meta: &CommitMeta,
    model: &DiffModel,
    theme: &Theme,
    layout: DiffLayout,
    width: usize,
) -> Vec<Line<'static>> {
    render_show_highlighted(meta, model, &highlight_diff(model), theme, layout, width)
}

/// Lay out `git show <rev>` using a precomputed [`DiffHighlight`].
pub fn render_show_highlighted(
    meta: &CommitMeta,
    model: &DiffModel,
    hl: &DiffHighlight,
    theme: &Theme,
    layout: DiffLayout,
    width: usize,
) -> Vec<Line<'static>> {
    let mut out = commit_header(meta, theme);
    out.extend(render_diff_highlighted(model, hl, theme, layout, width));
    out
}

/// The `commit / Author / Date / message` header block for `show`.
fn commit_header(meta: &CommitMeta, theme: &Theme) -> Vec<Line<'static>> {
    let mut out = vec![Line::styled(
        format!("commit {}", meta.id),
        theme.diff_file_style(),
    )];
    if !meta.author.is_empty() || !meta.email.is_empty() {
        out.push(Line::styled(
            format!("Author: {} <{}>", meta.author, meta.email),
            theme.diff_meta_style(),
        ));
    }
    if !meta.time.is_empty() {
        out.push(Line::styled(
            format!("Date:   {}", meta.time),
            theme.diff_meta_style(),
        ));
    }
    out.push(Line::default());
    if !meta.subject.is_empty() {
        out.push(Line::from(format!("    {}", meta.subject)));
    }
    if !meta.body.is_empty() {
        out.push(Line::default());
        for line in meta.body.lines() {
            out.push(Line::from(format!("    {line}")));
        }
    }
    out.push(Line::default());
    out
}

// ── unified layout ──────────────────────────────────────────────────────

fn render_file_unified(
    file: &FileDiff,
    hl: Option<&FileHighlight>,
    theme: &Theme,
    out: &mut Vec<Line<'static>>,
) {
    let Some(prep) = prepare_file(file, hl, theme, out) else {
        return;
    };
    let (new_ref, old_ref) = (prep.new_hl, prep.old_hl);

    let (mut oi, mut ni) = (0usize, 0usize);
    for h in prep.hunks {
        out.push(hunk_header_line(h, theme));
        let intra = compute_intra(&h.lines);
        for (j, line) in h.lines.iter().enumerate() {
            let row = match line.origin {
                LineOrigin::Context => {
                    let content =
                        styled_content(pick(new_ref, ni, &line.text, theme, None), None, None);
                    ni += 1;
                    oi += 1;
                    unified_row(' ', Style::default(), None, content)
                }
                LineOrigin::Add => {
                    let word = word_hl(intra[j].as_ref(), theme.diff_word_bg(true));
                    let content = styled_content(
                        pick(new_ref, ni, &line.text, theme, Some(true)),
                        theme.diff_row_bg(true),
                        word,
                    );
                    ni += 1;
                    unified_row(
                        '+',
                        theme.diff_gutter_style(true),
                        theme.diff_row_bg(true),
                        content,
                    )
                }
                LineOrigin::Remove => {
                    let word = word_hl(intra[j].as_ref(), theme.diff_word_bg(false));
                    let content = styled_content(
                        pick(old_ref, oi, &line.text, theme, Some(false)),
                        theme.diff_row_bg(false),
                        word,
                    );
                    oi += 1;
                    unified_row(
                        '-',
                        theme.diff_gutter_style(false),
                        theme.diff_row_bg(false),
                        content,
                    )
                }
            };
            out.push(row);
        }
    }
}

/// One unified row: a `marker` gutter glyph (in `row_bg`) + the already-styled
/// content spans (wash + word highlight applied by [`styled_content`]).
fn unified_row(
    marker: char,
    gutter_style: Style,
    row_bg: Option<Color>,
    content: Vec<Span<'static>>,
) -> Line<'static> {
    let mut spans = Vec::with_capacity(content.len() + 1);
    spans.push(Span::styled(
        marker.to_string(),
        apply_bg(gutter_style, row_bg),
    ));
    spans.extend(content);
    Line::from(spans)
}

// ── side-by-side layout ───────────────────────────────────────────────────

fn render_file_split(
    file: &FileDiff,
    hl: Option<&FileHighlight>,
    theme: &Theme,
    width: usize,
    out: &mut Vec<Line<'static>>,
) {
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
        let intra = compute_intra(&h.lines);
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
                );
                let right_rows = split_cell_rows(
                    theme,
                    Some(new_no),
                    LineOrigin::Context,
                    styled_content(pick(new_ref, ni, &lines[i].text, theme, None), None, None),
                    col_w,
                    lnum_w,
                );
                push_split_rows(out, left_rows, right_rows, col_w, theme);
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
                let left_rows = if r_lo + k < r_hi {
                    let word = word_hl(intra[r_lo + k].as_ref(), theme.diff_word_bg(false));
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
                    );
                    old_no += 1;
                    oi += 1;
                    rows
                } else {
                    vec![blank_cell_row(col_w)]
                };
                let right_rows = if a_lo + k < a_hi {
                    let word = word_hl(intra[a_lo + k].as_ref(), theme.diff_word_bg(true));
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
                    );
                    new_no += 1;
                    ni += 1;
                    rows
                } else {
                    vec![blank_cell_row(col_w)]
                };
                push_split_rows(out, left_rows, right_rows, col_w, theme);
            }
        }
    }
}

/// Pair the wrapped visual rows of a left and right cell into `split_row`s,
/// padding the shorter side with a blank cell so both columns stay aligned.
/// Consumes both row vecs (no per-row clone).
fn push_split_rows(
    out: &mut Vec<Line<'static>>,
    left_rows: Vec<Vec<Span<'static>>>,
    right_rows: Vec<Vec<Span<'static>>>,
    col_w: usize,
    theme: &Theme,
) {
    let n = left_rows.len().max(right_rows.len());
    let mut left = left_rows.into_iter();
    let mut right = right_rows.into_iter();
    for _ in 0..n {
        let l = left.next().unwrap_or_else(|| blank_cell_row(col_w));
        let r = right.next().unwrap_or_else(|| blank_cell_row(col_w));
        out.push(split_row(l, r, theme));
    }
}

/// One side-by-side cell split into wrapped visual rows. Each row is exactly
/// `col_w` columns wide. The first row carries `[lnum][space][marker]`;
/// continuation rows have a blank prefix of the same width so the content
/// indent stays consistent. `content` is already styled (wash + word highlight
/// via [`styled_content`]); background colors are applied to prefix + padding.
fn split_cell_rows(
    theme: &Theme,
    lnum: Option<u32>,
    origin: LineOrigin,
    content: Vec<Span<'static>>,
    col_w: usize,
    lnum_w: usize,
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

    wrap_spans(&content, content_w)
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
                .map(|s| display_width(s.content.as_ref()))
                .sum();
            spans.extend(row_spans);
            if used < content_w {
                spans.push(Span::styled(" ".repeat(content_w - used), pad_style));
            }
            spans
        })
        .collect()
}

/// A fully-blank side-by-side cell row (the absent side of an unbalanced
/// change, or the shorter side when a paired line wraps to more rows).
fn blank_cell_row(col_w: usize) -> Vec<Span<'static>> {
    vec![Span::raw(" ".repeat(col_w))]
}

/// Join a left + right cell with the column separator into one row.
fn split_row(left: Vec<Span<'static>>, right: Vec<Span<'static>>, theme: &Theme) -> Line<'static> {
    let mut spans = left;
    spans.push(Span::styled(SEP.to_string(), theme.diff_meta_style()));
    spans.extend(right);
    Line::from(spans)
}

// ── shared helpers ────────────────────────────────────────────────────────

/// A text file's hunks plus borrowed references into the cached per-side
/// highlight, ready for layout.
struct PreparedFile<'a> {
    hunks: &'a [Hunk],
    /// New-side syntax highlight (context + adds), `None` if syntect didn't
    /// recognize the language — callers fall back to flat `+`/`-` text.
    new_hl: Option<&'a [Line<'static>]>,
    /// Old-side syntax highlight (context + removes).
    old_hl: Option<&'a [Line<'static>]>,
}

/// Shared prologue for both layouts: push the file header, then resolve the
/// hunks. The non-text kinds (binary / submodule / error) push their
/// one-line explanation and return `None`, signaling the caller to stop. The
/// per-side highlight comes from the precomputed `hl` (see [`highlight_diff`]);
/// a missing entry falls back to unhighlighted `+`/`-` text.
fn prepare_file<'a>(
    file: &'a FileDiff,
    hl: Option<&'a FileHighlight>,
    theme: &Theme,
    out: &mut Vec<Line<'static>>,
) -> Option<PreparedFile<'a>> {
    out.push(file_header(file, theme));
    let hunks = match &file.kind {
        DiffKind::Text(hunks) => hunks,
        DiffKind::Binary => {
            out.push(Line::styled(
                "Binary file differs.",
                theme.diff_meta_style(),
            ));
            return None;
        }
        DiffKind::Submodule { old, new } => {
            out.push(submodule_line(old, new, theme));
            return None;
        }
        DiffKind::Error(msg) => {
            out.push(Line::styled(
                format!("diff unavailable: {msg}"),
                theme.diff_error_style(),
            ));
            return None;
        }
    };
    Some(PreparedFile {
        hunks,
        new_hl: hl.and_then(|h| h.new_hl.as_deref()),
        old_hl: hl.and_then(|h| h.old_hl.as_deref()),
    })
}

/// Width of the line-number field for a file's side-by-side cells: the digit
/// count of the largest line number actually rendered, floored at [`LNUM_W`].
/// Without this, a number wider than the fixed field (≥ 10000 with `LNUM_W`
/// = 4) widened only that cell, shoving the marker, separator, and entire
/// right column out of alignment for the rest of the file.
fn lnum_width(hunks: &[Hunk]) -> usize {
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

/// Collect the new-side (context + adds) and old-side (context + removes)
/// line texts across all hunks, in order — the inputs we highlight once each.
fn side_texts(hunks: &[Hunk]) -> (Vec<&str>, Vec<&str>) {
    let mut new_text = Vec::new();
    let mut old_text = Vec::new();
    for h in hunks {
        for line in &h.lines {
            match line.origin {
                LineOrigin::Context => {
                    new_text.push(line.text.as_str());
                    old_text.push(line.text.as_str());
                }
                LineOrigin::Add => new_text.push(line.text.as_str()),
                LineOrigin::Remove => old_text.push(line.text.as_str()),
            }
        }
    }
    (new_text, old_text)
}

/// Syntax-highlight one side's reconstructed text, returning one styled line
/// per input line. `None` when syntect doesn't recognize the language (the
/// caller then falls back to flat `+`/`-` colored text).
fn highlight_side(filename: &str, lines: &[&str]) -> Option<Vec<Line<'static>>> {
    if lines.is_empty() {
        return Some(Vec::new());
    }
    crate::ui::syntax::highlight_to_lines(filename, &lines.join("\n"))
}

/// The highlighted spans for logical line `idx` on a side, or a flat fallback
/// span (using the +/- text color for `kind = Some(is_add)`, plain for context
/// `None`) when highlighting was unavailable or the index is past the end (the
/// trailing-empty-line case).
fn pick(
    hl: Option<&[Line<'static>]>,
    idx: usize,
    fallback: &str,
    theme: &Theme,
    kind: Option<bool>,
) -> Vec<Span<'static>> {
    if let Some(line) = hl.and_then(|lines| lines.get(idx)) {
        return line.spans.clone();
    }
    let style = kind.map_or_else(Style::default, |is_add| theme.diff_text_style(is_add));
    vec![Span::styled(fallback.to_string(), style)]
}

/// Split `spans` into visual rows of at most `width` display columns each,
/// preserving span styles across row boundaries. Returns at least one row
/// (an empty row when `spans` is empty or `width` is zero).
fn wrap_spans(spans: &[Span<'static>], width: usize) -> Vec<Vec<Span<'static>>> {
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
            for (idx, ch) in rest.char_indices() {
                let w = UnicodeWidthChar::width(ch).unwrap_or(0);
                if visual + w > remaining {
                    break;
                }
                consumed_bytes = idx + ch.len_utf8();
                visual += w;
            }
            // Force at least one char even if it's wider than `remaining`
            // so a 2-col glyph in a 1-col viewport doesn't infinite-loop.
            if consumed_bytes == 0
                && let Some(first) = rest.chars().next()
            {
                consumed_bytes = first.len_utf8();
                visual = UnicodeWidthChar::width(first).unwrap_or(1);
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

/// Overlay a background color onto a style (non-destructive — syntect set only
/// `fg`, so language colors survive). No-op when `bg` is `None`.
fn apply_bg(style: Style, bg: Option<Color>) -> Style {
    bg.map_or(style, |c| style.bg(c))
}

/// Style a line's content spans for display: overlay the dim `row_bg` wash on
/// every span, then (for a modified line) overlay the brighter `word` bg on the
/// changed byte range. The caller prepends the gutter / line-number prefix.
fn styled_content(
    content: Vec<Span<'static>>,
    row_bg: Option<Color>,
    word: Option<(Range<usize>, Color)>,
) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = content
        .into_iter()
        .map(|mut sp| {
            sp.style = apply_bg(sp.style, row_bg);
            sp
        })
        .collect();
    if let Some((range, bg)) = word {
        spans = overlay_range_bg(spans, &range, bg);
    }
    spans
}

/// Overlay background `bg` on the byte sub-`range` of a styled span run,
/// splitting spans at the range boundaries. `range` must be on char
/// boundaries (it comes from [`intra_change_range`], which trims on chars).
fn overlay_range_bg(
    spans: Vec<Span<'static>>,
    range: &Range<usize>,
    bg: Color,
) -> Vec<Span<'static>> {
    if range.is_empty() {
        return spans;
    }
    let mut out = Vec::with_capacity(spans.len());
    let mut pos = 0usize;
    for sp in spans {
        let Span { content, style } = sp;
        let text = content.into_owned();
        let (start, end) = (pos, pos + text.len());
        pos = end;
        let lo = range.start.max(start);
        let hi = range.end.min(end);
        if lo >= hi {
            out.push(Span::styled(text, style));
            continue;
        }
        let (rl, rh) = (lo - start, hi - start);
        if rl > 0 {
            out.push(Span::styled(text[..rl].to_string(), style));
        }
        out.push(Span::styled(text[rl..rh].to_string(), style.bg(bg)));
        if rh < text.len() {
            out.push(Span::styled(text[rh..].to_string(), style));
        }
    }
    out
}

/// Pair a changed-range with a word-highlight color into the `word` arg
/// `styled_content` wants — `Some` only when both are present (no range, or
/// `mono` dropping the color, yields `None`).
fn word_hl(range: Option<&Range<usize>>, bg: Option<Color>) -> Option<(Range<usize>, Color)> {
    match (range, bg) {
        (Some(r), Some(c)) => Some((r.clone(), c)),
        _ => None,
    }
}

/// Per-line changed byte-ranges for a hunk: for each modified line (a removed
/// line paired with its added counterpart within a change region), the byte
/// range of the differing middle in *that line's own text*. Context and
/// unpaired add/remove lines get `None`. Drives the word-level highlight.
fn compute_intra(lines: &[crate::git::model::DiffLine]) -> Vec<Option<Range<usize>>> {
    let mut out = vec![None; lines.len()];
    let mut i = 0;
    while i < lines.len() {
        if lines[i].origin == LineOrigin::Context {
            i += 1;
            continue;
        }
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
        // Pair removes with adds 1:1; only paired lines get a word range.
        for k in 0..(r_hi - r_lo).min(a_hi - a_lo) {
            if let Some((old_r, new_r)) =
                intra_change_range(&lines[r_lo + k].text, &lines[a_lo + k].text)
            {
                out[r_lo + k] = Some(old_r);
                out[a_lo + k] = Some(new_r);
            }
        }
    }
    out
}

/// The changed byte-ranges between a removed line `old` and its added
/// counterpart `new`: trim the longest common char prefix + suffix; the middle
/// is what changed. `None` when the lines are identical or share no
/// prefix/suffix at all (a uniform brighter line adds nothing over the wash).
fn intra_change_range(old: &str, new: &str) -> Option<(Range<usize>, Range<usize>)> {
    if old == new {
        return None;
    }
    let prefix = common_prefix_len(old, new);
    let suffix = common_suffix_len(&old[prefix..], &new[prefix..]);
    if prefix == 0 && suffix == 0 {
        return None;
    }
    let old_hi = (old.len() - suffix).max(prefix);
    let new_hi = (new.len() - suffix).max(prefix);
    Some((prefix..old_hi, prefix..new_hi))
}

/// Byte length of the longest common char prefix of `a` and `b`.
fn common_prefix_len(a: &str, b: &str) -> usize {
    let mut len = 0;
    for (ca, cb) in a.char_indices().zip(b.char_indices()) {
        if ca.1 != cb.1 {
            break;
        }
        len = ca.0 + ca.1.len_utf8();
    }
    len
}

/// Byte length of the longest common char suffix of `a` and `b`.
fn common_suffix_len(a: &str, b: &str) -> usize {
    let mut len = 0;
    for (ca, cb) in a.chars().rev().zip(b.chars().rev()) {
        if ca != cb {
            break;
        }
        len += ca.len_utf8();
    }
    len
}

/// The path to use for syntax detection: the new path, else the old.
fn file_name(file: &FileDiff) -> &str {
    file.new_path
        .as_deref()
        .or(file.old_path.as_deref())
        .unwrap_or("")
}

/// The `status   path` header line for one file.
fn file_header(file: &FileDiff, theme: &Theme) -> Line<'static> {
    let text = match file.status {
        FileStatus::Added => format!("{:<10} {}", "added", path_or(file.new_path.as_deref())),
        FileStatus::Deleted => format!("{:<10} {}", "deleted", path_or(file.old_path.as_deref())),
        FileStatus::Modified => format!("{:<10} {}", "modified", file_name(file)),
        FileStatus::TypeChange => format!("{:<10} {}", "typechange", file_name(file)),
        FileStatus::Renamed { similarity } => format!(
            "{:<10} {} → {} ({similarity}%)",
            "renamed",
            path_or(file.old_path.as_deref()),
            path_or(file.new_path.as_deref()),
        ),
        FileStatus::Copied { similarity } => format!(
            "{:<10} {} → {} ({similarity}%)",
            "copied",
            path_or(file.old_path.as_deref()),
            path_or(file.new_path.as_deref()),
        ),
    };
    Line::styled(text, theme.diff_file_style())
}

/// A `@@ -a,b +c,d @@` hunk header line.
fn hunk_header_line(h: &Hunk, theme: &Theme) -> Line<'static> {
    Line::styled(
        format!(
            "@@ -{},{} +{},{} @@",
            h.old_start, h.old_lines, h.new_start, h.new_lines
        ),
        theme.diff_hunk_style(),
    )
}

/// A `Submodule <old> → <new>` line (short ids; `(none)` for an empty side).
fn submodule_line(old: &str, new: &str, theme: &Theme) -> Line<'static> {
    let short = |h: &str| -> String {
        if h.is_empty() {
            "(none)".to_string()
        } else {
            h.chars().take(7).collect()
        }
    };
    Line::styled(
        format!("Submodule {} → {}", short(old), short(new)),
        theme.diff_meta_style(),
    )
}

fn path_or(opt: Option<&str>) -> &str {
    opt.unwrap_or("?")
}

#[cfg(test)]
mod tests;
