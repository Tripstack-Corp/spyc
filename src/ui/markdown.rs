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

use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
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

struct Renderer<'t> {
    theme: &'t Theme,
    lines: Vec<Line<'static>>,
    current: Vec<Span<'static>>,
    /// Active emphasis modifiers applied to subsequent text spans.
    style_mods: Modifier,
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
    #[allow(dead_code)]
    alignments: Vec<Alignment>,
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

impl<'t> Renderer<'t> {
    fn new(theme: &'t Theme, table_width_hint: Option<usize>) -> Self {
        // Tables expand up to the hinted width when it exceeds the
        // prose target. `max(CONTENT_WIDTH)` guarantees small hints
        // (e.g. a 60-col terminal) don't *shrink* tables below the
        // existing baseline — wider only.
        let hint = table_width_hint.unwrap_or(CONTENT_WIDTH);
        let table_width = hint.max(CONTENT_WIDTH);
        let prose_width = hint.max(PROSE_WRAP_MIN);
        Self {
            theme,
            lines: Vec::new(),
            current: Vec::new(),
            style_mods: Modifier::empty(),
            list_indent: 0,
            in_blockquote: false,
            code_block: None,
            pending_link_url: None,
            table: None,
            just_started_item: false,
            table_width,
            prose_width,
        }
    }

    fn finish(mut self) -> Vec<Line<'static>> {
        if !self.current.is_empty() {
            self.flush_line();
        }
        self.lines
    }

    fn flush_line(&mut self) {
        let bq_prefix = if self.in_blockquote {
            Some(Span::styled(
                "\u{2503} ".to_string(), // ┃
                Style::default().fg(self.theme.status_suffix),
            ))
        } else {
            None
        };
        let bq_w = if self.in_blockquote { 2 } else { 0 };
        let cont_indent = self.continuation_indent();
        let cont_w = cont_indent.chars().count();
        // Subtract the blockquote rule and any list-item continuation
        // indent so the wrap target reflects the actual body cells
        // available after the prefix glyphs are drawn.
        let wrap_w = self.prose_width.saturating_sub(bq_w + cont_w).max(20);

        let spans = std::mem::take(&mut self.current);
        if spans.is_empty() {
            // Caller must use push_blank() for spacing; flush_line is
            // a no-op when there's nothing to push so we don't emit
            // stray blockquote-only rows.
            return;
        }
        let plain: String = spans.iter().map(|s| s.content.as_ref()).collect();
        let ranges = word_wrap_ranges(&plain, wrap_w);
        for (piece_idx, (start, end)) in ranges.into_iter().enumerate() {
            let chunk_spans = slice_spans(&spans, start, end);
            let mut row: Vec<Span<'static>> = Vec::new();
            if let Some(p) = bq_prefix.as_ref() {
                row.push(p.clone());
            }
            // First piece keeps the original leading content (bullet,
            // text); continuation rows get a blank indent so wrapped
            // text aligns under the source line's content.
            if piece_idx > 0 && cont_w > 0 {
                row.push(Span::raw(cont_indent.clone()));
            }
            row.extend(chunk_spans);
            self.lines.push(Line::from(row));
        }
    }

    /// Indent for continuation rows when wrapping inside a list
    /// item. Top-level list ⇒ 2 spaces (under the `• `); nested
    /// items ⇒ deeper indent so wrapped text aligns under the
    /// item's content, not under outer-level bullets.
    fn continuation_indent(&self) -> String {
        if self.list_indent == 0 {
            String::new()
        } else {
            // Each list level adds 2 cols of indent; the bullet itself
            // takes 2 ("• "). Continuation should align under the text
            // start = (list_indent - 1) * 2 + 2 = list_indent * 2.
            " ".repeat(self.list_indent * 2)
        }
    }

    fn push_blank(&mut self) {
        if !self.current.is_empty() {
            self.flush_line();
        }
        // Avoid stacking empty lines.
        if !self.lines.last().is_some_and(|l| l.spans.is_empty()) {
            self.lines.push(Line::from(Vec::<Span<'static>>::new()));
        }
    }

    fn push_text(&mut self, text: &str, base_style: Style) {
        // Preserve internal newlines as line boundaries (paragraphs
        // with hard line breaks render as separate visual lines).
        let mut first = true;
        for chunk in text.split('\n') {
            if !first {
                self.flush_line();
            }
            first = false;
            if !chunk.is_empty() {
                let style = base_style.add_modifier(self.style_mods);
                self.current.push(Span::styled(chunk.to_string(), style));
            }
        }
    }

    // The `if !self.current.is_empty() { self.flush_line() }` guards
    // below look collapsible to clippy, but they're not -- flush_line
    // unconditionally pushes a Line, so calling it on empty content
    // would emit a stray blank row.
    #[allow(clippy::collapsible_if, clippy::collapsible_match)]
    fn handle(&mut self, event: Event<'_>) {
        // Code block accumulates everything between Start and End.
        if let Some(cb) = self.code_block.as_mut() {
            match event {
                Event::Text(t) | Event::Code(t) => {
                    cb.body.push_str(&t);
                }
                Event::End(TagEnd::CodeBlock) => self.end_code_block(),
                Event::SoftBreak | Event::HardBreak => cb.body.push('\n'),
                _ => {}
            }
            return;
        }

        match event {
            Event::Start(tag) => self.start_tag(tag),
            Event::End(tag) => self.end_tag(tag),
            Event::Text(t) => self.push_text(&t, Style::default()),
            Event::Code(t) => {
                // Inline `code`: teal-on-default reads as "code" the
                // way most monospace UIs render it. Previously
                // status_suffix + DIM, which was so dark on a black
                // pager background that the backticks blurred into
                // body text.
                let style = Style::default().fg(self.theme.take);
                self.current.push(Span::styled(format!("`{t}`"), style));
            }
            // CommonMark: soft break (single `\n` mid-paragraph)
            // is whitespace — the paragraph reflows at the pager's
            // wrap width. The earlier "soft-as-hard" override
            // helped one metadata-stacking case but broke prose
            // authored with 80-col source wrap (every short source
            // line became its own short row). Metadata lines are
            // now handled by a source-level preprocessor in
            // `force_hard_breaks_before_keyed_lines` that inserts
            // markdown's two-space hard-break marker before each
            // `**Key:**`-style line — so SoftBreak stays soft.
            Event::SoftBreak => self.current.push(Span::raw(" ".to_string())),
            Event::HardBreak => self.flush_line(),
            Event::Rule => {
                if !self.current.is_empty() {
                    self.flush_line();
                }
                let dim = Style::default()
                    .fg(self.theme.status_suffix)
                    .add_modifier(Modifier::DIM);
                self.lines
                    .push(Line::from(Span::styled("\u{2500}".repeat(40), dim)));
                self.lines.push(Line::from(Vec::<Span<'static>>::new()));
            }
            Event::TaskListMarker(checked) => {
                let glyph = if checked { "[x] " } else { "[ ] " };
                self.current.push(Span::styled(
                    glyph.to_string(),
                    Style::default().fg(self.theme.pick),
                ));
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                // Render raw HTML as dim text — not a goal to interpret it.
                let style = Style::default().add_modifier(Modifier::DIM);
                self.push_text(&html, style);
            }
            Event::FootnoteReference(name) => {
                self.current.push(Span::styled(
                    format!("[^{name}]"),
                    Style::default().fg(self.theme.status_suffix),
                ));
            }
            _ => {}
        }
    }

    #[allow(clippy::collapsible_if, clippy::collapsible_match)]
    fn start_tag(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => {
                // Inside a list item we want bullet + text on one
                // line. pulldown-cmark wraps loose-list item content
                // in a Paragraph; without this guard the flush at
                // paragraph-start would dump the bullet glyph alone
                // and put the item's text on the next line.
                if self.just_started_item {
                    self.just_started_item = false;
                } else if !self.current.is_empty() {
                    self.flush_line();
                }
            }
            Tag::Heading { level, .. } => {
                if !self.current.is_empty() || !self.lines.is_empty() {
                    self.push_blank();
                }
                let prefix = "#".repeat(heading_depth(level));
                let style = Style::default()
                    .fg(self.theme.status_user)
                    .add_modifier(Modifier::BOLD);
                self.current.push(Span::styled(format!("{prefix} "), style));
                // Subsequent text in the heading inherits BOLD via style_mods.
                self.style_mods |= Modifier::BOLD;
            }
            Tag::BlockQuote(_) => {
                self.in_blockquote = true;
                if !self.current.is_empty() {
                    self.flush_line();
                }
            }
            Tag::CodeBlock(kind) => {
                if !self.current.is_empty() {
                    self.flush_line();
                }
                let lang = match kind {
                    CodeBlockKind::Fenced(s) => s.into_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                self.code_block = Some(CodeBlockState {
                    lang,
                    body: String::new(),
                });
            }
            Tag::List(_) => {
                if !self.current.is_empty() {
                    self.flush_line();
                }
                self.list_indent += 1;
            }
            Tag::Item => {
                if !self.current.is_empty() {
                    self.flush_line();
                }
                let indent = "  ".repeat(self.list_indent.saturating_sub(1));
                self.current.push(Span::styled(
                    format!("{indent}\u{2022} "),
                    Style::default().fg(self.theme.status_path),
                ));
                // Tell the next paragraph-start to skip the flush so
                // the bullet glyph stays attached to the item's text.
                self.just_started_item = true;
            }
            Tag::Emphasis => {
                self.style_mods |= Modifier::ITALIC;
            }
            Tag::Strong => {
                self.style_mods |= Modifier::BOLD;
            }
            Tag::Strikethrough => {
                self.style_mods |= Modifier::CROSSED_OUT;
            }
            Tag::Link { dest_url, .. } => {
                self.pending_link_url = Some(dest_url.into_string());
                self.style_mods |= Modifier::UNDERLINED;
            }
            Tag::Image { dest_url, .. } => {
                // Render as `[image: url]` placeholder. Alt text
                // (if any) flows in as Text events between Start
                // and End; we let those render under italic.
                let style = Style::default()
                    .fg(self.theme.status_suffix)
                    .add_modifier(Modifier::DIM);
                self.current
                    .push(Span::styled(format!("[image: {dest_url}] "), style));
            }
            Tag::FootnoteDefinition(name) => {
                if !self.current.is_empty() {
                    self.flush_line();
                }
                self.current.push(Span::styled(
                    format!("[^{name}]: "),
                    Style::default().fg(self.theme.status_suffix),
                ));
            }
            Tag::Table(alignments) => {
                if !self.current.is_empty() {
                    self.flush_line();
                }
                self.table = Some(TableBuilder {
                    alignments,
                    head: None,
                    body: Vec::new(),
                    in_head: false,
                    cur_row: Vec::new(),
                    stashed_current: Vec::new(),
                });
            }
            Tag::TableHead => {
                if let Some(t) = self.table.as_mut() {
                    t.in_head = true;
                    t.cur_row.clear();
                }
            }
            Tag::TableRow => {
                if let Some(t) = self.table.as_mut() {
                    t.cur_row.clear();
                }
            }
            Tag::TableCell => {
                // Swap the active span buffer to capture cell content.
                // Inline emphasis / code / links etc. inside the cell
                // push into `current` per usual; we'll harvest it on
                // `End(TableCell)`.
                if let Some(t) = self.table.as_mut() {
                    t.stashed_current = std::mem::take(&mut self.current);
                }
            }
            // Other tags fall through unstyled.
            _ => {}
        }
    }

    #[allow(clippy::collapsible_if, clippy::collapsible_match)]
    fn end_tag(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                if !self.current.is_empty() {
                    self.flush_line();
                }
                self.lines.push(Line::from(Vec::<Span<'static>>::new()));
            }
            TagEnd::Heading(_) => {
                self.style_mods.remove(Modifier::BOLD);
                if !self.current.is_empty() {
                    self.flush_line();
                }
                self.lines.push(Line::from(Vec::<Span<'static>>::new()));
            }
            TagEnd::BlockQuote(_) => {
                if !self.current.is_empty() {
                    self.flush_line();
                }
                self.in_blockquote = false;
                self.lines.push(Line::from(Vec::<Span<'static>>::new()));
            }
            TagEnd::List(_) => {
                self.list_indent = self.list_indent.saturating_sub(1);
                if self.list_indent == 0 {
                    self.lines.push(Line::from(Vec::<Span<'static>>::new()));
                }
            }
            TagEnd::Item => {
                if !self.current.is_empty() {
                    self.flush_line();
                }
            }
            TagEnd::Emphasis => self.style_mods.remove(Modifier::ITALIC),
            TagEnd::Strong => self.style_mods.remove(Modifier::BOLD),
            TagEnd::Strikethrough => self.style_mods.remove(Modifier::CROSSED_OUT),
            TagEnd::Link => {
                self.style_mods.remove(Modifier::UNDERLINED);
                if let Some(url) = self.pending_link_url.take() {
                    let dim = Style::default()
                        .fg(self.theme.status_suffix)
                        .add_modifier(Modifier::DIM);
                    self.current
                        .push(Span::styled(format!(" \u{2192} {url}"), dim));
                }
            }
            TagEnd::TableCell => {
                if let Some(t) = self.table.as_mut() {
                    let cell = std::mem::take(&mut self.current);
                    self.current = std::mem::take(&mut t.stashed_current);
                    t.cur_row.push(cell);
                }
            }
            TagEnd::TableHead => {
                if let Some(t) = self.table.as_mut() {
                    let row = std::mem::take(&mut t.cur_row);
                    t.head = Some(row);
                    t.in_head = false;
                }
            }
            TagEnd::TableRow => {
                if let Some(t) = self.table.as_mut() {
                    let row = std::mem::take(&mut t.cur_row);
                    t.body.push(row);
                }
            }
            TagEnd::Table => {
                self.end_table();
            }
            _ => {}
        }
    }

    /// Render the collected `TableBuilder` into `self.lines` as an
    /// ASCII-aligned table with box-drawing borders. Column widths
    /// are computed from natural cell widths, capped per-column at
    /// `TABLE_MAX_COL_WIDTH` and trimmed proportionally so the
    /// total fits inside `CONTENT_WIDTH`. Cells longer than the
    /// allotted column width are truncated with `…`. Header cells
    /// render bold; borders render in dim slate (theme.status_suffix).
    fn end_table(&mut self) {
        let Some(t) = self.table.take() else {
            return;
        };
        let head = t.head.as_ref();
        let n_cols = head
            .map_or(0, Vec::len)
            .max(t.body.iter().map(Vec::len).max().unwrap_or(0));
        if n_cols == 0 {
            return;
        }

        // Natural widths per column, then cap.
        let mut widths = vec![0usize; n_cols];
        let update_widths = |row: &[Vec<Span<'static>>], widths: &mut [usize]| {
            for (i, cell) in row.iter().enumerate() {
                if i < widths.len() {
                    widths[i] = widths[i].max(spans_visual_width(cell));
                }
            }
        };
        if let Some(h) = head {
            update_widths(h, &mut widths);
        }
        for row in &t.body {
            update_widths(row, &mut widths);
        }
        // Per-column cap. When the caller supplied a wider table
        // budget (e.g. real pager body width on a 200-col terminal),
        // compute the cap proportionally so a few columns each get a
        // decent slice rather than every column getting hard-clamped
        // at 24. Fallback (no hint, or budget == CONTENT_WIDTH)
        // preserves the original tight cap so existing tests / small
        // terminals keep their behavior.
        let per_col_cap = if self.table_width > CONTENT_WIDTH {
            // Frame overhead: 3 cells per column + 1 outer border.
            let usable = self.table_width.saturating_sub(3 * n_cols + 1);
            (usable / n_cols.max(1))
                .clamp(TABLE_MAX_COL_WIDTH_FALLBACK, TABLE_MAX_COL_WIDTH_CEILING)
        } else {
            TABLE_MAX_COL_WIDTH_FALLBACK
        };
        for w in &mut widths {
            *w = (*w).clamp(3, per_col_cap);
        }
        // Proportional trim if total > table_width. Each cell takes
        // `width + 2` columns of frame (space-content-space) plus one
        // border char between cells (`│`) plus the two outer borders.
        // total = sum(w+2) + (n+1) = sum(w) + 3n + 1.
        let total_with_frame = |widths: &[usize]| widths.iter().sum::<usize>() + 3 * n_cols + 1;
        while total_with_frame(&widths) > self.table_width {
            // Shrink the widest column by one. Stop if everything is
            // already at the floor of 3.
            let Some((idx, _)) = widths.iter().enumerate().max_by_key(|(_, w)| **w) else {
                break;
            };
            if widths[idx] <= 3 {
                break;
            }
            widths[idx] -= 1;
        }

        let frame_style = Style::default().fg(self.theme.status_suffix);

        // Top, mid, bottom border strings.
        let mut top = String::from("\u{250c}"); // ┌
        let mut mid = String::from("\u{251c}"); // ├
        let mut bot = String::from("\u{2514}"); // └
        for (i, w) in widths.iter().enumerate() {
            for _ in 0..*w + 2 {
                top.push('\u{2500}'); // ─
                mid.push('\u{2500}');
                bot.push('\u{2500}');
            }
            if i + 1 < widths.len() {
                top.push('\u{252c}'); // ┬
                mid.push('\u{253c}'); // ┼
                bot.push('\u{2534}'); // ┴
            }
        }
        top.push('\u{2510}'); // ┐
        mid.push('\u{2524}'); // ┤
        bot.push('\u{2518}'); // ┘

        self.lines.push(Line::from(Span::styled(top, frame_style)));
        if let Some(h) = head {
            self.render_table_row(h, &widths, true, frame_style);
            self.lines.push(Line::from(Span::styled(mid, frame_style)));
        }
        for row in &t.body {
            self.render_table_row(row, &widths, false, frame_style);
        }
        self.lines.push(Line::from(Span::styled(bot, frame_style)));
        self.lines.push(Line::from(Vec::<Span<'static>>::new()));
    }

    /// Render one logical row of cells, wrapping each cell's content
    /// at its column width via the same `word_wrap_ranges` routine
    /// the paragraph renderer uses (par-style word wrap with
    /// hard-break fallback). The visual height of the row is the
    /// max wrap-rows across cells; cells that wrap to fewer rows
    /// are padded with blank cells for those visual rows.
    fn render_table_row(
        &mut self,
        row: &[Vec<Span<'static>>],
        widths: &[usize],
        is_header: bool,
        frame_style: Style,
    ) {
        // Wrap each cell into a Vec<Vec<Span>> (one inner Vec per
        // visual row of the cell). Empty cells get a single empty
        // visual row so the row-height math doesn't degenerate.
        let wrapped: Vec<Vec<Vec<Span<'static>>>> = widths
            .iter()
            .enumerate()
            .map(|(i, &w)| {
                let cell = row.get(i).cloned().unwrap_or_default();
                let mut rows = wrap_spans_to_width(&cell, w);
                if rows.is_empty() {
                    rows.push(Vec::new());
                }
                rows
            })
            .collect();

        let row_h = wrapped.iter().map(Vec::len).max().unwrap_or(1).max(1);
        let empty_row: Vec<Span<'static>> = Vec::new();

        for vr in 0..row_h {
            let mut line_spans: Vec<Span<'static>> = Vec::new();
            line_spans.push(Span::styled("\u{2502} ".to_string(), frame_style));
            for (i, w) in widths.iter().enumerate() {
                let cell_row = wrapped[i].get(vr).unwrap_or(&empty_row);
                let used = spans_visual_width(cell_row);
                for s in cell_row {
                    let mut style = s.style;
                    if is_header {
                        style = style.add_modifier(Modifier::BOLD);
                    }
                    line_spans.push(Span::styled(s.content.clone(), style));
                }
                if used < *w {
                    line_spans.push(Span::raw(" ".repeat(*w - used)));
                }
                if i + 1 < widths.len() {
                    line_spans.push(Span::styled(" \u{2502} ".to_string(), frame_style));
                } else {
                    line_spans.push(Span::styled(" \u{2502}".to_string(), frame_style));
                }
            }
            self.lines.push(Line::from(line_spans));
        }
    }

    fn end_code_block(&mut self) {
        let Some(state) = self.code_block.take() else {
            return;
        };
        let body = state.body.trim_end_matches('\n');
        // Try syntect highlighting if a language is given; fall
        // back to plain dim text otherwise. We synthesize a fake
        // filename for highlight_to_lines's extension-based lookup
        // when the language tag matches a known extension.
        let highlighted = if state.lang.is_empty() {
            None
        } else {
            let fake_name = format!("snippet.{}", state.lang);
            crate::ui::syntax::highlight_to_lines(&fake_name, body)
        };
        let dim = Style::default()
            .fg(self.theme.status_suffix)
            .add_modifier(Modifier::DIM);
        // Top fence line (dim ───).
        self.lines
            .push(Line::from(Span::styled("\u{2500}".repeat(40), dim)));
        if let Some(lines) = highlighted {
            self.lines.extend(lines);
        } else {
            for raw in body.lines() {
                self.lines.push(Line::from(Span::styled(
                    raw.to_string(),
                    Style::default().fg(self.theme.other),
                )));
            }
        }
        // Bottom fence line.
        self.lines
            .push(Line::from(Span::styled("\u{2500}".repeat(40), dim)));
        self.lines.push(Line::from(Vec::<Span<'static>>::new()));
    }
}

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

/// Compute byte-range break points for word-wrapping `text` at
/// `width` visual columns. Prefers breaks at whitespace; falls back
/// to a hard break when no whitespace exists in the budget. The
/// whitespace at break points is *consumed* — the next range starts
/// after it — so wrapped lines don't begin with a stray space.
fn word_wrap_ranges(text: &str, width: usize) -> Vec<(usize, usize)> {
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

/// Slice a sequence of styled spans by a byte range over the
/// concatenated plain text. Spans that fall outside the range are
/// dropped; spans that straddle the boundary are split at the byte
/// offset, preserving their style on the kept portion. Used to
/// reconstruct each wrapped row's spans from the original
/// paragraph's spans.
fn slice_spans(spans: &[Span<'static>], start: usize, end: usize) -> Vec<Span<'static>> {
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

fn floor_char_boundary(s: &str, mut idx: usize) -> usize {
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

/// Visual width (terminal columns) of a styled span sequence,
/// computed via `unicode-width`. Used by the table renderer to
/// size columns from natural cell content.
fn spans_visual_width(spans: &[Span<'static>]) -> usize {
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
fn wrap_spans_to_width(spans: &[Span<'static>], max_w: usize) -> Vec<Vec<Span<'static>>> {
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
mod tests {
    use super::*;
    use crate::ui::theme::Theme;

    fn render_plain(src: &str) -> Vec<String> {
        let theme = Theme::default();
        render(src, &theme, None)
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn renders_heading_with_hash_prefix() {
        let lines = render_plain("# Title\n");
        assert!(lines.iter().any(|l| l == "# Title"));
    }

    #[test]
    fn renders_paragraph_text_inline() {
        let lines = render_plain("hello world\n");
        assert!(lines.iter().any(|l| l == "hello world"));
    }

    #[test]
    fn renders_bullet_list_with_indent() {
        let lines = render_plain("- alpha\n- beta\n");
        assert!(lines.iter().any(|l| l == "\u{2022} alpha"));
        assert!(lines.iter().any(|l| l == "\u{2022} beta"));
    }

    /// Regression: a *loose* list (blank lines between items) wraps
    /// each item in a Paragraph at the pulldown-cmark event level.
    /// Before the `just_started_item` guard, the paragraph-start
    /// flush would dump the bullet glyph onto its own line and leave
    /// the item's text on the next line — visible as `•` + newline +
    /// `text` in the pager (reported against BUGS.md when viewed via
    /// the markdown viewer).
    #[test]
    fn loose_list_keeps_bullet_attached_to_item_text() {
        let src = "- alpha\n\n- beta\n";
        let lines = render_plain(src);
        // First and second item content must be on the same row as
        // their bullet — not orphaned to its own row.
        assert!(
            lines.iter().any(|l| l == "\u{2022} alpha"),
            "expected `• alpha` together on one line; got: {lines:?}"
        );
        assert!(
            lines.iter().any(|l| l == "\u{2022} beta"),
            "expected `• beta` together on one line; got: {lines:?}"
        );
        // And the bullet glyph must NOT appear as a standalone line.
        assert!(
            !lines.iter().any(|l| l == "\u{2022} " || l == "\u{2022}"),
            "bullet glyph should not be on its own line; got: {lines:?}"
        );
    }

    #[test]
    fn renders_blockquote_with_left_rule() {
        let lines = render_plain("> quoted\n");
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("\u{2503} ") && l.contains("quoted"))
        );
    }

    #[test]
    fn fenced_code_block_emits_fence_lines() {
        let lines = render_plain("```\nfoo\n```\n");
        // Top + bottom fence rows, plus body.
        assert!(lines.iter().filter(|l| l.starts_with("\u{2500}")).count() >= 2);
        assert!(lines.iter().any(|l| l == "foo"));
    }

    #[test]
    fn link_renders_text_with_url_after() {
        let lines = render_plain("see [docs](https://example.com)\n");
        assert!(
            lines
                .iter()
                .any(|l| l.contains("docs") && l.contains("https://example.com"))
        );
    }

    #[test]
    fn keyed_metadata_lines_stack() {
        // Lines that start with `**Word:**` should each render on
        // their own row, even without trailing two-space hard
        // breaks or blank lines between them. CommonMark would
        // collapse them into a single wrapped paragraph; our
        // `force_hard_breaks_before_keyed_lines` preprocessor
        // opts each such line into a markdown hard break.
        let src = "**To:** Alice\n**From:** Bob\n**Status:** Draft\n";
        let lines = render_plain(src);
        let non_empty: Vec<&String> = lines.iter().filter(|l| !l.is_empty()).collect();
        assert_eq!(non_empty.len(), 3, "got lines: {lines:?}");
        assert!(non_empty[0].contains("To:"), "{:?}", non_empty[0]);
        assert!(non_empty[1].contains("From:"), "{:?}", non_empty[1]);
        assert!(non_empty[2].contains("Status:"), "{:?}", non_empty[2]);
    }

    #[test]
    fn prose_reflows_across_source_line_breaks() {
        // Source authored with 80-col wrap should reflow at the
        // pager's width, not stick to the awkward source break
        // points. (Regression for "soft-breaks-as-hard-breaks"
        // which faithfully reproduced the source's 80-col splits
        // and broke at "...using a / new / Facade API ...".)
        let src = "Build direction §3.1 names Option A as \"build inside the IBE perimeter\n\
                   using a new Facade API and partner-facing GraphQL.\" A natural reading\n\
                   is that IBE's existing WEB GraphQL endpoint is the foundation to extend.";
        let lines = render_plain(src);
        // The whole paragraph reflows as one — no source line
        // ending with "a" stranded on its own row, etc. With a
        // 200-col hint we'd get 1-2 long rows; with the default
        // 80-col target we get a few rows but each ending at a
        // word boundary, not at the source's split points.
        for l in &lines {
            // The mid-paragraph fragments from the source ("using",
            // "is that") should never appear on their own line.
            assert_ne!(l.trim(), "using", "stranded source fragment: {lines:?}");
            assert_ne!(l.trim(), "is that", "stranded source fragment: {lines:?}");
        }
    }

    #[test]
    fn long_paragraph_wraps_at_content_width() {
        // Build a paragraph whose source is one line of >100 chars;
        // pulldown joins it as one logical paragraph, the renderer
        // should wrap at CONTENT_WIDTH (80) at word boundaries.
        let src = format!("{} word.\n", "lorem ".repeat(20));
        let lines = render_plain(&src);
        // Every non-empty body line should be <= CONTENT_WIDTH.
        for l in &lines {
            assert!(
                l.chars().count() <= super::CONTENT_WIDTH,
                "line {l:?} exceeded CONTENT_WIDTH"
            );
        }
        // And the paragraph should produce more than one line of
        // content (proves wrap actually happened).
        let body_lines = lines.iter().filter(|l| !l.is_empty()).count();
        assert!(
            body_lines >= 2,
            "expected wrap to produce multiple lines, got {lines:?}"
        );
    }

    #[test]
    fn wrapped_list_item_indents_continuation() {
        // List item whose content overflows 80 cols should wrap with
        // 2-space hanging indent so the continuation aligns under
        // the bullet's text.
        let src = format!("- {}\n", "alpha ".repeat(20));
        let lines = render_plain(&src);
        let body: Vec<&String> = lines.iter().filter(|l| !l.is_empty()).collect();
        assert!(body.len() >= 2, "expected wrap on long list item");
        // First line starts with "• ".
        assert!(
            body[0].starts_with("\u{2022} "),
            "first line: {:?}",
            body[0]
        );
        // Continuation starts with two spaces (matches bullet width).
        assert!(body[1].starts_with("  "), "continuation: {:?}", body[1]);
    }

    #[test]
    fn word_wrap_ranges_breaks_at_spaces() {
        let s = "hello world foo bar baz";
        let ranges = super::word_wrap_ranges(s, 11);
        let pieces: Vec<&str> = ranges.iter().map(|&(a, b)| &s[a..b]).collect();
        assert_eq!(pieces, vec!["hello world", "foo bar baz"]);
    }

    #[test]
    fn word_wrap_ranges_hard_breaks_when_no_space() {
        // No spaces ⇒ hard break at width.
        let s = "abcdefghijklmnopqrstuvwxyz";
        let ranges = super::word_wrap_ranges(s, 10);
        let pieces: Vec<&str> = ranges.iter().map(|&(a, b)| &s[a..b]).collect();
        assert_eq!(pieces, vec!["abcdefghij", "klmnopqrst", "uvwxyz"]);
    }

    #[test]
    fn renders_simple_table_with_borders() {
        // Standard GFM table: header row + separator + data rows.
        // Should render with box-drawing borders and the header
        // text appearing somewhere inside the table.
        let src = "| H1 | H2 |\n|----|----|\n| a  | b  |\n| c  | d  |\n";
        let lines = render_plain(src);
        // Top border with corner glyphs.
        assert!(
            lines
                .iter()
                .any(|l| l.contains('\u{250c}') && l.contains('\u{2510}')),
            "missing top border in {lines:?}"
        );
        // Bottom border.
        assert!(
            lines
                .iter()
                .any(|l| l.contains('\u{2514}') && l.contains('\u{2518}')),
            "missing bottom border in {lines:?}"
        );
        // Header separator with cross.
        assert!(
            lines.iter().any(|l| l.contains('\u{253c}')),
            "missing header separator in {lines:?}"
        );
        // Header and data text appear.
        assert!(lines.iter().any(|l| l.contains("H1") && l.contains("H2")));
        assert!(lines.iter().any(|l| l.contains('a') && l.contains('b')));
    }

    #[test]
    fn table_wraps_overlong_cells_to_multiple_visual_rows() {
        // A cell long enough that wrapping at column width produces
        // multiple visual rows. We should see the same column-border
        // glyph (`│`) on more than one line below the header
        // separator -- proving the cell spans multiple visual rows
        // rather than being truncated with `…`.
        let long = "alpha bravo ".repeat(20);
        let src = format!("| H |\n|---|\n| {long} |\n");
        let lines = render_plain(&src);
        // No truncation marker should appear (we wrap, not truncate).
        assert!(
            !lines.iter().any(|l| l.contains('\u{2026}')),
            "expected NO ellipsis (wrap, don't truncate); got {lines:?}"
        );
        // At least 3 rows of body content (the long string at narrow
        // width must wrap to multiple visual rows). Each body row
        // has a leading `│ `.
        let body_rows = lines.iter().filter(|l| l.starts_with("\u{2502} ")).count();
        assert!(
            body_rows >= 3,
            "expected ≥3 body rows from wrap, got {body_rows} in {lines:?}"
        );
    }

    #[test]
    fn is_markdown_path_matches_md_and_markdown() {
        use std::path::Path;
        assert!(is_markdown_path(Path::new("README.md")));
        assert!(is_markdown_path(Path::new("notes.markdown")));
        assert!(!is_markdown_path(Path::new("main.rs")));
    }
}
