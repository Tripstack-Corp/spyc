//! Pure renderer: a [`DiffModel`] / [`CommitMeta`] → styled `ratatui` lines.
//!
//! This is the in-house replacement for piping `git diff --color=always`
//! bytes through the pager. It produces `Vec<Line<'static>>` from the
//! structured model PR 7 built — so search, wrap, line-numbers, and
//! visual-yank all work, and (crucially) we can lay the same model out
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
//! the pager by PR 8b (via the git-view session in `app/git_view_session.rs`).

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use crate::git::model::{CommitMeta, DiffKind, DiffModel, FileDiff, FileStatus, Hunk, LineOrigin};
use crate::ui::theme::Theme;
use crate::ui::{display_truncate, display_width};

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
/// Width of the per-cell line-number field in the side-by-side layout.
const LNUM_W: usize = 4;
/// Per-cell prefix before content: line-number field + space + 1-char marker.
const CELL_PREFIX_W: usize = LNUM_W + 2;

/// Render a whole diff to styled lines in the chosen `layout`. `width` is the
/// total viewport width in columns (used only by [`DiffLayout::SideBySide`] to
/// size its two columns; ignored for unified).
pub fn render_diff(
    model: &DiffModel,
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
        match layout {
            DiffLayout::Unified => render_file_unified(file, theme, &mut out),
            DiffLayout::SideBySide => render_file_split(file, theme, width, &mut out),
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
/// commit's diff in the chosen `layout`.
pub fn render_show(
    meta: &CommitMeta,
    model: &DiffModel,
    theme: &Theme,
    layout: DiffLayout,
    width: usize,
) -> Vec<Line<'static>> {
    let mut out = commit_header(meta, theme);
    out.extend(render_diff(model, theme, layout, width));
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

fn render_file_unified(file: &FileDiff, theme: &Theme, out: &mut Vec<Line<'static>>) {
    out.push(file_header(file, theme));
    let hunks = match &file.kind {
        DiffKind::Text(hunks) => hunks,
        DiffKind::Binary => {
            out.push(Line::styled(
                "Binary file differs.",
                theme.diff_meta_style(),
            ));
            return;
        }
        DiffKind::Submodule { old, new } => {
            out.push(submodule_line(old, new, theme));
            return;
        }
    };

    let (new_text, old_text) = side_texts(hunks);
    let new_hl = highlight_side(file_name(file), &new_text);
    let old_hl = highlight_side(file_name(file), &old_text);
    let (new_ref, old_ref) = (new_hl.as_deref(), old_hl.as_deref());

    let (mut oi, mut ni) = (0usize, 0usize);
    for h in hunks {
        out.push(hunk_header_line(h, theme));
        for line in &h.lines {
            let row = match line.origin {
                LineOrigin::Context => {
                    let content = pick(new_ref, ni, &line.text, theme, None);
                    ni += 1;
                    oi += 1;
                    unified_row(' ', Style::default(), None, content)
                }
                LineOrigin::Add => {
                    let content = pick(new_ref, ni, &line.text, theme, Some(true));
                    ni += 1;
                    unified_row(
                        '+',
                        theme.diff_gutter_style(true),
                        theme.diff_row_bg(true),
                        content,
                    )
                }
                LineOrigin::Remove => {
                    let content = pick(old_ref, oi, &line.text, theme, Some(false));
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

/// One unified row: a `marker` gutter glyph + the (already highlighted)
/// content spans, with `row_bg` overlaid on every span.
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
    for mut sp in content {
        sp.style = apply_bg(sp.style, row_bg);
        spans.push(sp);
    }
    Line::from(spans)
}

// ── side-by-side layout ───────────────────────────────────────────────────

fn render_file_split(file: &FileDiff, theme: &Theme, width: usize, out: &mut Vec<Line<'static>>) {
    out.push(file_header(file, theme));
    let hunks = match &file.kind {
        DiffKind::Text(hunks) => hunks,
        DiffKind::Binary => {
            out.push(Line::styled(
                "Binary file differs.",
                theme.diff_meta_style(),
            ));
            return;
        }
        DiffKind::Submodule { old, new } => {
            out.push(submodule_line(old, new, theme));
            return;
        }
    };

    let (new_text, old_text) = side_texts(hunks);
    let new_hl = highlight_side(file_name(file), &new_text);
    let old_hl = highlight_side(file_name(file), &old_text);
    let (new_ref, old_ref) = (new_hl.as_deref(), old_hl.as_deref());
    let col_w = width.saturating_sub(SEP_W) / 2;

    let (mut oi, mut ni) = (0usize, 0usize);
    for h in hunks {
        out.push(hunk_header_line(h, theme));
        let mut old_no = h.old_start;
        let mut new_no = h.new_start;
        let lines = &h.lines;
        let mut i = 0;
        while i < lines.len() {
            if lines[i].origin == LineOrigin::Context {
                let left = split_cell(
                    theme,
                    Some(old_no),
                    LineOrigin::Context,
                    &pick(old_ref, oi, &lines[i].text, theme, None),
                    col_w,
                );
                let right = split_cell(
                    theme,
                    Some(new_no),
                    LineOrigin::Context,
                    &pick(new_ref, ni, &lines[i].text, theme, None),
                    col_w,
                );
                out.push(split_row(left, right, theme));
                old_no += 1;
                new_no += 1;
                oi += 1;
                ni += 1;
                i += 1;
                continue;
            }
            // A change region: the run of consecutive removes, then the run of
            // consecutive adds (PR 7 always emits removes before adds within a
            // region). Pair them row-for-row, padding the shorter side blank.
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
            let rows = (r_hi - r_lo).max(a_hi - a_lo);
            for k in 0..rows {
                let left = if r_lo + k < r_hi {
                    let cell = split_cell(
                        theme,
                        Some(old_no),
                        LineOrigin::Remove,
                        &pick(old_ref, oi, &lines[r_lo + k].text, theme, Some(false)),
                        col_w,
                    );
                    old_no += 1;
                    oi += 1;
                    cell
                } else {
                    blank_cell(col_w)
                };
                let right = if a_lo + k < a_hi {
                    let cell = split_cell(
                        theme,
                        Some(new_no),
                        LineOrigin::Add,
                        &pick(new_ref, ni, &lines[a_lo + k].text, theme, Some(true)),
                        col_w,
                    );
                    new_no += 1;
                    ni += 1;
                    cell
                } else {
                    blank_cell(col_w)
                };
                out.push(split_row(left, right, theme));
            }
        }
    }
}

/// One side-by-side cell: `[lnum][space][marker][content…]`, padded/truncated
/// to exactly `col_w` columns, with the origin's row-background overlaid.
fn split_cell(
    theme: &Theme,
    lnum: Option<u32>,
    origin: LineOrigin,
    content: &[Span<'static>],
    col_w: usize,
) -> Vec<Span<'static>> {
    let (marker, row_bg, gutter_style) = match origin {
        LineOrigin::Context => (' ', None, Style::default()),
        LineOrigin::Add => ('+', theme.diff_row_bg(true), theme.diff_gutter_style(true)),
        LineOrigin::Remove => (
            '-',
            theme.diff_row_bg(false),
            theme.diff_gutter_style(false),
        ),
    };
    let lnum_str = lnum.map_or_else(|| " ".repeat(LNUM_W), |n| format!("{n:>LNUM_W$}"));
    let content_w = col_w.saturating_sub(CELL_PREFIX_W);

    let mut spans = Vec::with_capacity(content.len() + 3);
    spans.push(Span::styled(
        format!("{lnum_str} "),
        apply_bg(theme.diff_meta_style(), row_bg),
    ));
    spans.push(Span::styled(
        marker.to_string(),
        apply_bg(gutter_style, row_bg),
    ));
    spans.extend(fit_spans(content, content_w, row_bg));
    spans
}

/// A fully-blank side-by-side cell (the absent side of an unbalanced change).
fn blank_cell(col_w: usize) -> Vec<Span<'static>> {
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

/// Truncate `spans` to at most `width` display columns and pad to exactly
/// `width` with trailing spaces, overlaying `bg` on everything. Keeps every
/// side-by-side cell the same width so columns stay aligned.
fn fit_spans(spans: &[Span<'static>], width: usize, bg: Option<Color>) -> Vec<Span<'static>> {
    let mut out = Vec::new();
    let mut used = 0usize;
    for sp in spans {
        if used >= width {
            break;
        }
        let content = sp.content.as_ref();
        let w = display_width(content);
        if used + w <= width {
            out.push(Span::styled(content.to_string(), apply_bg(sp.style, bg)));
            used += w;
        } else {
            let trunc = display_truncate(content, width - used);
            used += display_width(trunc);
            out.push(Span::styled(trunc.to_string(), apply_bg(sp.style, bg)));
            break;
        }
    }
    if used < width {
        out.push(Span::styled(
            " ".repeat(width - used),
            bg.map_or_else(Style::default, |c| Style::default().bg(c)),
        ));
    }
    out
}

/// Overlay a background color onto a style (non-destructive — syntect set only
/// `fg`, so language colors survive). No-op when `bg` is `None`.
fn apply_bg(style: Style, bg: Option<Color>) -> Style {
    bg.map_or(style, |c| style.bg(c))
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
mod tests {
    use super::{DiffLayout, render_diff, render_show};
    use crate::git::model::{
        CommitMeta, DiffKind, DiffLine, DiffModel, FileDiff, FileStatus, Hunk, LineOrigin,
    };
    use crate::ui::theme::Theme;
    use ratatui::text::Line;

    fn ctx(text: &str) -> DiffLine {
        DiffLine {
            origin: LineOrigin::Context,
            text: text.to_string(),
        }
    }
    fn add(text: &str) -> DiffLine {
        DiffLine {
            origin: LineOrigin::Add,
            text: text.to_string(),
        }
    }
    fn rem(text: &str) -> DiffLine {
        DiffLine {
            origin: LineOrigin::Remove,
            text: text.to_string(),
        }
    }

    /// A one-file modify diff (`c` → `C`) with surrounding context, in `f.txt`.
    fn modify_model() -> DiffModel {
        DiffModel {
            files: vec![FileDiff {
                old_path: Some("f.txt".into()),
                new_path: Some("f.txt".into()),
                status: FileStatus::Modified,
                lang_hint: "txt".into(),
                kind: DiffKind::Text(vec![Hunk {
                    old_start: 1,
                    old_lines: 5,
                    new_start: 1,
                    new_lines: 5,
                    lines: vec![ctx("a"), ctx("b"), rem("c"), add("C"), ctx("d"), ctx("e")],
                }]),
            }],
            truncated: false,
        }
    }

    /// Flatten styled lines to their glyph text (the layout/structure view).
    /// One added file (`added.txt`, two all-add lines).
    fn added_model() -> DiffModel {
        DiffModel {
            files: vec![FileDiff {
                old_path: None,
                new_path: Some("added.txt".into()),
                status: FileStatus::Added,
                lang_hint: "txt".into(),
                kind: DiffKind::Text(vec![Hunk {
                    old_start: 0,
                    old_lines: 0,
                    new_start: 1,
                    new_lines: 2,
                    lines: vec![add("new1"), add("new2")],
                }]),
            }],
            truncated: false,
        }
    }

    fn single_file(
        status: FileStatus,
        kind: DiffKind,
        old: Option<&str>,
        new: Option<&str>,
    ) -> DiffModel {
        DiffModel {
            files: vec![FileDiff {
                old_path: old.map(Into::into),
                new_path: new.map(Into::into),
                status,
                lang_hint: String::new(),
                kind,
            }],
            truncated: false,
        }
    }

    /// Flatten styled lines to their glyph text (the layout/structure view),
    /// trailing whitespace trimmed per line.
    fn text(lines: &[Line]) -> String {
        lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The concatenated glyph text of one rendered line.
    fn row_text(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn unified_modify_glyph_and_colors() {
        let theme = Theme::default();
        let out = render_diff(&modify_model(), &theme, DiffLayout::Unified, 80);
        assert_eq!(
            text(&out),
            "modified   f.txt\n@@ -1,5 +1,5 @@\n a\n b\n-c\n+C\n d\n e"
        );
        // Row 4 is `-c`, row 5 is `+C`; their gutter + content carry the tint.
        assert_eq!(out[4].spans[0].content.as_ref(), "-");
        assert_eq!(out[4].spans[0].style.bg, Some(theme.diff_del_bg));
        assert_eq!(
            out[4].spans.last().unwrap().style.bg,
            Some(theme.diff_del_bg)
        );
        assert_eq!(out[5].spans[0].content.as_ref(), "+");
        assert_eq!(out[5].spans[0].style.bg, Some(theme.diff_add_bg));
        // Context rows are untinted.
        assert_eq!(out[2].spans[0].style.bg, None);
    }

    #[test]
    fn split_modify_layout_and_colors() {
        let theme = Theme::default();
        let out = render_diff(&modify_model(), &theme, DiffLayout::SideBySide, 80);
        // header + hunk header + 5 rows: ctx a, ctx b, the paired `-c`/`+C`
        // change row (removes pair with adds side-by-side, unlike unified's two
        // separate rows), ctx d, ctx e.
        assert_eq!(out.len(), 7);
        // Every data row has the column separator.
        for row in &out[2..] {
            assert!(
                row_text(row).contains('│'),
                "row missing separator: {row:?}"
            );
        }
        // The change row pairs `-c` (left) with `+C` (right).
        let change = &out[4];
        let joined = row_text(change);
        assert!(
            joined.contains("-c") && joined.contains("+C"),
            "got: {joined}"
        );
        // Left gutter marker is `-` with the remove tint…
        assert_eq!(change.spans[1].content.as_ref(), "-");
        assert_eq!(change.spans[1].style.bg, Some(theme.diff_del_bg));
        // …and the right gutter marker (two spans past the separator) is `+`.
        let sep = change
            .spans
            .iter()
            .position(|s| s.content.contains('│'))
            .unwrap();
        assert_eq!(change.spans[sep + 2].content.as_ref(), "+");
        assert_eq!(change.spans[sep + 2].style.bg, Some(theme.diff_add_bg));
    }

    #[test]
    fn mono_drops_backgrounds_keeps_markers() {
        let theme = Theme::default().toggled(); // mono = true
        assert!(theme.mono);
        let out = render_diff(&modify_model(), &theme, DiffLayout::Unified, 80);
        // Glyphs (and so the +/- markers) are unchanged…
        assert_eq!(
            text(&out),
            "modified   f.txt\n@@ -1,5 +1,5 @@\n a\n b\n-c\n+C\n d\n e"
        );
        // …but the row backgrounds are gone.
        assert_eq!(out[4].spans[0].style.bg, None);
        assert_eq!(out[5].spans[0].style.bg, None);
    }

    #[test]
    fn added_file_is_all_adds() {
        let theme = Theme::default();
        let out = render_diff(&added_model(), &theme, DiffLayout::Unified, 80);
        assert_eq!(
            text(&out),
            "added      added.txt\n@@ -0,0 +1,2 @@\n+new1\n+new2"
        );
    }

    #[test]
    fn unknown_language_falls_back_to_plus_minus_color() {
        let theme = Theme::default();
        // `.xyzzy` isn't a syntect-known extension → flat fallback styling.
        let model = single_file(
            FileStatus::Modified,
            DiffKind::Text(vec![Hunk {
                old_start: 1,
                old_lines: 1,
                new_start: 1,
                new_lines: 1,
                lines: vec![rem("old"), add("new")],
            }]),
            Some("f.xyzzy"),
            Some("f.xyzzy"),
        );
        let out = render_diff(&model, &theme, DiffLayout::Unified, 80);
        // The `+new` content span uses the add foreground (no syntax colors).
        let add_row = out.iter().find(|l| row_text(l).starts_with('+')).unwrap();
        assert_eq!(
            add_row.spans.last().unwrap().style.fg,
            Some(theme.diff_add_fg)
        );
    }

    #[test]
    fn binary_file_line() {
        let theme = Theme::default();
        let model = single_file(
            FileStatus::Modified,
            DiffKind::Binary,
            Some("b.bin"),
            Some("b.bin"),
        );
        let out = render_diff(&model, &theme, DiffLayout::Unified, 80);
        assert_eq!(text(&out), "modified   b.bin\nBinary file differs.");
    }

    #[test]
    fn submodule_line_rendered() {
        let theme = Theme::default();
        let model = single_file(
            FileStatus::Modified,
            DiffKind::Submodule {
                old: "1111111aaa".into(),
                new: "2222222bbb".into(),
            },
            Some("dep"),
            Some("dep"),
        );
        let out = render_diff(&model, &theme, DiffLayout::Unified, 80);
        assert_eq!(text(&out), "modified   dep\nSubmodule 1111111 → 2222222");
    }

    #[test]
    fn rename_header_shows_similarity() {
        let theme = Theme::default();
        let model = single_file(
            FileStatus::Renamed { similarity: 87 },
            DiffKind::Text(Vec::new()),
            Some("old.rs"),
            Some("new.rs"),
        );
        let out = render_diff(&model, &theme, DiffLayout::Unified, 80);
        assert_eq!(text(&out), "renamed    old.rs → new.rs (87%)");
    }

    #[test]
    fn truncated_appends_banner() {
        let theme = Theme::default();
        let mut model = added_model();
        model.truncated = true;
        let rendered = text(&render_diff(&model, &theme, DiffLayout::Unified, 80));
        assert!(rendered.ends_with("… diff truncated (too large to display in full) …"));
    }

    #[test]
    fn empty_model_says_no_changes() {
        let theme = Theme::default();
        let out = render_diff(&DiffModel::default(), &theme, DiffLayout::Unified, 80);
        assert_eq!(text(&out), "No changes.");
    }

    #[test]
    fn side_by_side_rows_never_exceed_width() {
        // The pager must not wrap side-by-side rows — so every rendered row's
        // display width must be ≤ the width it was rendered for. (A row wider
        // than the pager body wraps, and the wrapped padding tail shows as a
        // stray tinted bar — the bug this guards against.)
        let theme = Theme::default();
        for width in [40usize, 60, 80, 81, 100, 137] {
            let out = render_diff(&modify_model(), &theme, DiffLayout::SideBySide, width);
            for line in &out {
                let w: usize = line
                    .spans
                    .iter()
                    .map(|s| crate::ui::display_width(s.content.as_ref()))
                    .sum();
                assert!(w <= width, "row width {w} exceeds {width}: {line:?}");
            }
        }
    }

    #[test]
    fn show_renders_commit_header_then_diff() {
        let theme = Theme::default();
        let meta = CommitMeta {
            id: "a".repeat(40),
            short_id: "aaaaaaa".into(),
            author: "Ada".into(),
            email: "ada@example.com".into(),
            time: "2026-06-06 10:00:00 -04:00".into(),
            subject: "tweak c".into(),
            body: "body line one\nbody line two".into(),
        };
        let out = render_show(&meta, &modify_model(), &theme, DiffLayout::Unified, 80);
        let rendered = text(&out);
        assert!(rendered.starts_with(&format!("commit {}", "a".repeat(40))));
        assert!(rendered.contains("Author: Ada <ada@example.com>"));
        assert!(rendered.contains("Date:   2026-06-06 10:00:00 -04:00"));
        assert!(rendered.contains("\n    tweak c\n"));
        assert!(rendered.contains("\n    body line one\n    body line two\n"));
        // The diff body follows.
        assert!(rendered.contains("@@ -1,5 +1,5 @@"));
    }
}
