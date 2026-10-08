//! Pure renderer: a [`DiffModel`] / [`CommitMeta`] → styled `ratatui` lines.
//!
//! This is the in-house replacement for piping `git diff --color=always`
//! bytes through the pager. It produces `Vec<Line<'static>>` from the
//! structured `DiffModel` (`crate::git::model`) — so search, wrap,
//! line-numbers, and visual-yank all work, and (crucially) we can lay the
//! same model out
//! either **unified** or **side-by-side**, which coloured-byte output can't.
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
//! tint overlaid (syntect sets only `fg`, so language colours survive).
//!
//! Pure: `model + &Theme → lines`, no IO, no gix, no `&mut self`. Wired into
//! the pager via the git-view session in `app/git_view_session.rs`.

use std::ops::Range;

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use crate::config::Intraline;
use crate::git::model::{CommitMeta, DiffKind, DiffModel, FileDiff, FileStatus, Hunk, LineOrigin};
use crate::ui::theme::Theme;

mod intraline;
mod split;

/// How a diff is laid out in the pager.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLayout {
    /// One column, git-style `+`/`-` gutter.
    Unified,
    /// Two columns: old (left) vs new (right), paired and aligned.
    SideBySide,
}

/// A diff's syntax highlight, computed once and reused across every
/// layout/width re-render. syntect (in [`highlight_side`]) is by far the most
/// expensive part of rendering a diff, and it depends only on the model — not
/// on the theme (syntect carries its own palette; the diff wash/word colours are
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

/// Render-time knobs the app threads in from config. Bundled rather than passed
/// as loose parameters: `render_show_highlighted` was already at seven
/// arguments, so a second scalar would have pushed the whole chain over
/// `clippy::too_many_arguments`.
#[derive(Debug, Clone, Copy)]
pub struct DiffOpts {
    /// `[pager] tab_width` — columns a `\t` expands to.
    pub tab_width: usize,
    /// `[diff] intraline` — granularity of the changed-region highlight.
    pub intraline: Intraline,
}

/// `[pager] tab_width`'s default, used by the test-only render helpers so the
/// existing cases don't all have to name it.
#[cfg(test)]
const TEST_TAB_WIDTH: usize = 4;

#[cfg(test)]
impl Default for DiffOpts {
    fn default() -> Self {
        Self {
            tab_width: TEST_TAB_WIDTH,
            intraline: Intraline::default(),
        }
    }
}

/// Render a whole diff to styled lines, highlighting inline. A test convenience
/// that pairs [`highlight_diff`] with [`render_diff_highlighted`] in one call;
/// production always splits the two so the highlight is computed once off-thread
/// and the layout (width/`layout`-dependent) re-runs cheaply (see
/// [`crate::app::git_view_session`]).
#[cfg(test)]
pub fn render_diff(
    model: &DiffModel,
    theme: &Theme,
    layout: DiffLayout,
    width: usize,
) -> Vec<Line<'static>> {
    render_diff_tw(model, theme, layout, width, TEST_TAB_WIDTH)
}

/// [`render_diff`] with an explicit `tab_width` — for the alignment cases where
/// tab expansion is the thing under test.
#[cfg(test)]
pub fn render_diff_tw(
    model: &DiffModel,
    theme: &Theme,
    layout: DiffLayout,
    width: usize,
    tab_width: usize,
) -> Vec<Line<'static>> {
    render_diff_opts(
        model,
        theme,
        layout,
        width,
        DiffOpts {
            tab_width,
            ..DiffOpts::default()
        },
    )
}

/// [`render_diff`] with explicit [`DiffOpts`] — for the cases where tab
/// expansion or intraline granularity is the thing under test.
#[cfg(test)]
pub fn render_diff_opts(
    model: &DiffModel,
    theme: &Theme,
    layout: DiffLayout,
    width: usize,
    opts: DiffOpts,
) -> Vec<Line<'static>> {
    render_diff_highlighted(model, &highlight_diff(model), theme, layout, width, opts)
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
    opts: DiffOpts,
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
            DiffLayout::Unified => render_file_unified(file, fhl, theme, opts, &mut out),
            DiffLayout::SideBySide => {
                split::render_file_split(file, fhl, theme, width, opts, &mut out);
            }
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
/// commit's diff in the chosen `layout`. A test convenience that highlights
/// inline; production splits [`highlight_diff`] (once, off-thread) from
/// [`render_show_highlighted`] (the cached-highlight variant).
#[cfg(test)]
pub fn render_show(
    meta: &CommitMeta,
    model: &DiffModel,
    theme: &Theme,
    layout: DiffLayout,
    width: usize,
) -> Vec<Line<'static>> {
    render_show_highlighted(
        meta,
        model,
        &highlight_diff(model),
        theme,
        layout,
        width,
        DiffOpts::default(),
    )
}

/// Lay out `git show <rev>` using a precomputed [`DiffHighlight`].
pub fn render_show_highlighted(
    meta: &CommitMeta,
    model: &DiffModel,
    hl: &DiffHighlight,
    theme: &Theme,
    layout: DiffLayout,
    width: usize,
    opts: DiffOpts,
) -> Vec<Line<'static>> {
    let mut out = commit_header(meta, theme);
    out.extend(render_diff_highlighted(
        model, hl, theme, layout, width, opts,
    ));
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
    opts: DiffOpts,
    out: &mut Vec<Line<'static>>,
) {
    let Some(prep) = prepare_file(file, hl, theme, out) else {
        return;
    };
    let (new_ref, old_ref) = (prep.new_hl, prep.old_hl);

    let (mut oi, mut ni) = (0usize, 0usize);
    for h in prep.hunks {
        out.push(hunk_header_line(h, theme));
        let intra = compute_intra(&h.lines, opts.intraline);
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
                    let word = word_hl(&intra[j], theme.diff_word_bg(true));
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
                    let word = word_hl(&intra[j], theme.diff_word_bg(false));
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
/// one-line explanation and return `None`, signalling the caller to stop. The
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
/// caller then falls back to flat `+`/`-` coloured text).
fn highlight_side(filename: &str, lines: &[&str]) -> Option<Vec<Line<'static>>> {
    if lines.is_empty() {
        return Some(Vec::new());
    }
    crate::ui::syntax::highlight_to_lines(filename, &lines.join("\n"))
}

/// The highlighted spans for logical line `idx` on a side, or a flat fallback
/// span (using the +/- text colour for `kind = Some(is_add)`, plain for context
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

/// Overlay a background colour onto a style (non-destructive — syntect set only
/// `fg`, so language colours survive). No-op when `bg` is `None`.
fn apply_bg(style: Style, bg: Option<Color>) -> Style {
    bg.map_or(style, |c| style.bg(c))
}

/// Style a line's content spans for display: overlay the dim `row_bg` wash on
/// every span, then (for a modified line) overlay the brighter `word` bg on each
/// changed byte range. The caller prepends the gutter / line-number prefix.
fn styled_content(
    content: Vec<Span<'static>>,
    row_bg: Option<Color>,
    word: Option<(&[Range<usize>], Color)>,
) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = content
        .into_iter()
        .map(|mut sp| {
            sp.style = apply_bg(sp.style, row_bg);
            sp
        })
        .collect();
    if let Some((ranges, bg)) = word {
        // Ranges are disjoint and ascending, and each overlay preserves total
        // byte length, so they compose without disturbing each other's offsets.
        for range in ranges {
            spans = overlay_range_bg(spans, range, bg);
        }
    }
    spans
}

/// Overlay background `bg` on the byte sub-`range` of a styled span run,
/// splitting spans at the range boundaries. `range` must be on char
/// boundaries (it comes from [`intraline`], whose tokens split on chars).
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

/// Pair a line's changed-ranges with a word-highlight colour into the `word` arg
/// `styled_content` wants — `Some` only when both are present (no ranges, or
/// `mono` dropping the colour, yields `None`).
const fn word_hl(ranges: &[Range<usize>], bg: Option<Color>) -> Option<(&[Range<usize>], Color)> {
    match bg {
        Some(c) if !ranges.is_empty() => Some((ranges, c)),
        _ => None,
    }
}

/// Per-line changed byte-ranges for a hunk: for each modified line (a removed
/// line paired with its added counterpart within a change region), the byte
/// ranges that differ in *that line's own text* — one per changed word, so an
/// unchanged token between two changed ones stays unhighlighted. Context and
/// unpaired add/remove lines get an empty list. Drives the word-level highlight.
fn compute_intra(lines: &[crate::git::model::DiffLine], mode: Intraline) -> Vec<Vec<Range<usize>>> {
    let mut out = vec![Vec::new(); lines.len()];
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
        // Pair removes with adds 1:1; only paired lines get word ranges.
        for k in 0..(r_hi - r_lo).min(a_hi - a_lo) {
            let (old_r, new_r) =
                intraline::intra_change_ranges(&lines[r_lo + k].text, &lines[a_lo + k].text, mode);
            out[r_lo + k] = old_r;
            out[a_lo + k] = new_r;
        }
    }
    out
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
