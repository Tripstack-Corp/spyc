//! Render Markdown source as styled `Line`s for the pager.
//!
//! Walks `pulldown-cmark` events, accumulating spans into the current
//! line and pushing the line on block boundaries. Targets a readable
//! visual — not a perfect typesetter — for READMEs, design docs, and
//! changelogs viewed in the pager. The pager's `m` toggle swaps
//! between this rendering and the syntect-highlighted source.
//!
//! Out of scope for v1: embedded HTML (passed through as text),
//! images (alt text only). Footnotes and task lists work because
//! pulldown-cmark's defaults handle them as inline events. Tables
//! are supported as ASCII-bordered blocks; column widths adapt to
//! the pager body width via the renderer's `table_width_hint`.
//!
//! Code blocks fall through to syntect when a language hint is given
//! and the language is recognized; unrecognized languages render
//! plain in the code-block style.

use pulldown_cmark::{HeadingLevel, Options, Parser};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};

use crate::ui::theme::Theme;

/// Per-column upper bound when the caller didn't supply a width hint
/// (tests, programmatic use). Real renders pass an actual pager body
/// width and the per-column cap is computed from it. See [`render`].
const TABLE_MAX_COL_WIDTH_FALLBACK: usize = 24;

/// Hard ceiling on a single table column even with vast amounts of
/// terminal real estate. A 200-char-wide single column on an
/// ultrawide monitor is unreadable; prefer wrapping past this.
const TABLE_MAX_COL_WIDTH_CEILING: usize = 60;

/// Fallback prose-wrap target when no width hint is supplied
/// (tests, programmatic use). Real renders pass an actual pager
/// body width via [`render`] and prose reflows at that width.
const CONTENT_WIDTH: usize = 80;

/// Lower bound for the prose-wrap width even when the caller's
/// hint is smaller. A 30-cell terminal wrapping prose at 30 chars
/// per row is unreadable; clamp to something that holds a sentence.
const PROSE_WRAP_MIN: usize = 40;

/// Render a Markdown source string into styled lines suitable for
/// the pager's `lines` field. `width_hint` is the available pager
/// body width in cells; when supplied, both prose paragraphs and
/// tables reflow at that width instead of the [`CONTENT_WIDTH`]
/// fallback. Source-wrapped-at-80 prose then flows naturally at
/// whatever pager width is available, instead of being broken at
/// the source's awkward 80-col split points.
///
/// Naming kept as `table_width_hint` for back-compat with v1.50.48
/// callers, but the hint now also drives prose wrap.
pub fn render(source: &str, theme: &Theme, table_width_hint: Option<usize>) -> Vec<Line<'static>> {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_FOOTNOTES);
    opts.insert(Options::ENABLE_TABLES);
    let prepared = force_hard_breaks_before_keyed_lines(source);
    let parser = Parser::new_ext(&prepared, opts);
    let mut r = Renderer::new(theme, table_width_hint);
    for event in parser {
        r.handle(event);
    }
    r.finish()
}

/// Reference-counted text modifiers. Markdown spans nest — e.g. `**bold**`
/// inside an already-bold heading — and a plain `Modifier` bitflag can't track
/// that: the inner Strong's close would `remove(BOLD)` and un-bold the rest of
/// the heading. Counting each modifier (active while its count > 0) makes
/// overlapping start/end pairs nest correctly.
#[derive(Default)]
struct StyleMods {
    bold: u16,
    italic: u16,
    crossed_out: u16,
    underlined: u16,
}

impl StyleMods {
    /// Enter a span carrying modifier `m`.
    const fn push(&mut self, m: Modifier) {
        if m.contains(Modifier::BOLD) {
            self.bold = self.bold.saturating_add(1);
        }
        if m.contains(Modifier::ITALIC) {
            self.italic = self.italic.saturating_add(1);
        }
        if m.contains(Modifier::CROSSED_OUT) {
            self.crossed_out = self.crossed_out.saturating_add(1);
        }
        if m.contains(Modifier::UNDERLINED) {
            self.underlined = self.underlined.saturating_add(1);
        }
    }

    /// Leave a span carrying modifier `m`. Saturating so an unbalanced close
    /// can't wrap a counter below zero.
    const fn pop(&mut self, m: Modifier) {
        if m.contains(Modifier::BOLD) {
            self.bold = self.bold.saturating_sub(1);
        }
        if m.contains(Modifier::ITALIC) {
            self.italic = self.italic.saturating_sub(1);
        }
        if m.contains(Modifier::CROSSED_OUT) {
            self.crossed_out = self.crossed_out.saturating_sub(1);
        }
        if m.contains(Modifier::UNDERLINED) {
            self.underlined = self.underlined.saturating_sub(1);
        }
    }

    /// The modifier set currently active (every counter that is > 0).
    fn current(&self) -> Modifier {
        let mut m = Modifier::empty();
        if self.bold > 0 {
            m |= Modifier::BOLD;
        }
        if self.italic > 0 {
            m |= Modifier::ITALIC;
        }
        if self.crossed_out > 0 {
            m |= Modifier::CROSSED_OUT;
        }
        if self.underlined > 0 {
            m |= Modifier::UNDERLINED;
        }
        m
    }
}

struct Renderer<'t> {
    theme: &'t Theme,
    lines: Vec<Line<'static>>,
    current: Vec<Span<'static>>,
    /// Active emphasis modifiers (reference-counted so nested spans, like
    /// `**bold**` inside a bold heading, restore correctly on close).
    style_mods: StyleMods,
    /// Nested-list bullet indent. 0 = top-level.
    list_indent: usize,
    /// True while inside any blockquote (single level — nested
    /// blockquotes render with the same `┃ ` prefix).
    in_blockquote: bool,
    /// When inside a fenced code block, accumulate body here so we
    /// can hand the whole thing to syntect (or render plain) on End.
    code_block: Option<CodeBlockState>,
    /// Last text span saw was a Start(Link); store the destination so
    /// we can append it dimly after the link's text.
    pending_link_url: Option<String>,
    /// Active table state. While `Some`, cell-text events (`Text`,
    /// `Code`, emphasis spans, etc.) are routed into the current
    /// cell buffer instead of `current`. On `End(Table)` we render
    /// the collected rows into `lines` as an ASCII-aligned table.
    table: Option<TableBuilder>,
    /// True for exactly one event after `Tag::Item` — long enough to
    /// suppress the paragraph-start flush that would otherwise dump
    /// the bullet glyph (`•`) onto its own line and leave the item's
    /// text on the next line. pulldown-cmark wraps loose-list items
    /// in `Paragraph` events; without this guard the bullet and
    /// text get separated visually. Cleared on the next event
    /// (whether it's the paragraph open we're guarding against or
    /// a direct text event in a tight list).
    just_started_item: bool,
    /// Target total width for tables, in cells. At least `CONTENT_WIDTH`;
    /// larger when the caller hinted a wider pager. Drives both the
    /// proportional-trim ceiling and the dynamic per-column cap in
    /// `end_table`.
    table_width: usize,
    /// Target wrap width for prose paragraphs. Tracks the caller's
    /// hint when supplied (so source-wrapped-at-80 paragraphs flow
    /// to fill the pager body), or falls back to `CONTENT_WIDTH`.
    /// Clamped to [`PROSE_WRAP_MIN`] so a tiny terminal doesn't
    /// produce 30-char rows of mangled prose.
    prose_width: usize,
}

/// Source-level preprocessor: insert markdown's two-space hard-break
/// marker before any line that starts with `**Word(s):**`. CommonMark
/// would otherwise collapse a stack like
///
/// ```text
/// **To:** Alice
/// **From:** Bob
/// **Status:** Draft
/// ```
///
/// into one wrapped paragraph (`**To:** Alice **From:** Bob ...`) —
/// the canonical reflow loses the metadata semantics. Two-space-EOL
/// is the standard markdown way to force a line break inside a
/// paragraph, so this preprocessor opts each `**Key:**` line into
/// that behavior automatically while leaving regular prose alone.
///
/// Pattern: a newline immediately followed by `**`, then 1+ chars
/// that are neither `*` nor newline ending with `:`, then `**`.
/// This catches `**Word:**`, `**Multi word:**`, `**With_under:**`,
/// etc. It does NOT catch `**Bold without colon**` (no `:`) or
/// `**Bold**: value` (colon outside the bold).
fn force_hard_breaks_before_keyed_lines(source: &str) -> std::borrow::Cow<'_, str> {
    let re = regex::Regex::new(r"\n(\*\*[^*\n]+:\*\*)").expect("static regex compiles");
    re.replace_all(source, "  \n$1")
}

struct TableBuilder {
    /// Header cells (one row). Set on `End(TableHead)`.
    head: Option<Vec<Vec<Span<'static>>>>,
    /// Body rows.
    body: Vec<Vec<Vec<Span<'static>>>>,
    /// Currently in `TableHead`? If true, the row being built lands
    /// in `head` on `End(TableHead)`; else it lands in `body` on
    /// `End(TableRow)`.
    in_head: bool,
    /// Cells of the row currently under construction.
    cur_row: Vec<Vec<Span<'static>>>,
    /// Where outer `current` lived before we entered the active
    /// cell. Restored on `End(TableCell)`. Always empty in practice
    /// because tables only nest after a paragraph flush, but keeping
    /// the stash makes the swap symmetric.
    stashed_current: Vec<Span<'static>>,
}

struct CodeBlockState {
    lang: String,
    body: String,
}

mod renderer;
mod wrap;

const fn heading_depth(level: HeadingLevel) -> usize {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// True if `path` looks like a Markdown file we should render. The
/// pager checks this when opening a file: if true, both the source
/// and rendered views are pre-computed and `m` toggles between them.
pub fn is_markdown_path(path: &std::path::Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("md" | "markdown")
    )
}

#[cfg(test)]
mod tests;
