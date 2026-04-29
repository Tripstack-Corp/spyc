//! Render Markdown source as styled `Line`s for the pager.
//!
//! Walks `pulldown-cmark` events, accumulating spans into the current
//! line and pushing the line on block boundaries. Targets a readable
//! visual — not a perfect typesetter — for READMEs, design docs, and
//! changelogs viewed in the pager. The pager's `m` toggle swaps
//! between this rendering and the syntect-highlighted source.
//!
//! Out of scope for v1: tables (TUI tables look mediocre), embedded
//! HTML (passed through as text), images (alt text only). Footnotes
//! and task lists work because pulldown-cmark's defaults handle
//! them as inline events.
//!
//! Code blocks fall through to syntect when a language hint is given
//! and the language is recognized; unrecognized languages render
//! plain in the code-block style.

use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::ui::theme::Theme;

/// Maximum visual width of a single table column. Caps a runaway
/// "very long content in one cell" from blowing past CONTENT_WIDTH.
/// Per-column widths are *also* capped to fit the overall table
/// inside CONTENT_WIDTH; this is the upper bound regardless of
/// column count.
const TABLE_MAX_COL_WIDTH: usize = 24;

/// Target visual width for wrapped Markdown content, before any
/// blockquote rule or list-item indent is added. 80 columns is the
/// standard prose-readability target -- READMEs and design docs
/// land in that range and stay scannable. The pager pane itself
/// stays full-width so the user can still see other surrounding UI;
/// just the content body is bounded.
const CONTENT_WIDTH: usize = 80;

/// Render a Markdown source string into styled lines suitable for
/// the pager's `lines` field.
pub fn render(source: &str, theme: &Theme) -> Vec<Line<'static>> {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_FOOTNOTES);
    opts.insert(Options::ENABLE_TABLES);
    let parser = Parser::new_ext(source, opts);
    let mut r = Renderer::new(theme);
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
    const fn new(theme: &'t Theme) -> Self {
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
        // indent so the body still hits the 80-col target.
        let wrap_w = CONTENT_WIDTH.saturating_sub(bq_w + cont_w).max(20);

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
            Event::SoftBreak => {
                // Soft break → space (paragraphs flow).
                self.current.push(Span::raw(" ".to_string()));
            }
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
                if !self.current.is_empty() {
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
                self.current
                    .push(Span::styled(format!("{prefix} "), style));
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
        let n_cols = head.map_or(0, Vec::len).max(
            t.body.iter().map(Vec::len).max().unwrap_or(0),
        );
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
        // Per-column cap.
        for w in &mut widths {
            *w = (*w).clamp(3, TABLE_MAX_COL_WIDTH);
        }
        // Proportional trim if total > CONTENT_WIDTH. Each cell takes
        // `width + 2` columns of frame (space-content-space) plus one
        // border char between cells (`│`) plus the two outer borders.
        // total = sum(w+2) + (n+1) = sum(w) + 3n + 1.
        let total_with_frame = |widths: &[usize]| widths.iter().sum::<usize>() + 3 * n_cols + 1;
        while total_with_frame(&widths) > CONTENT_WIDTH {
            // Shrink the widest column by one. Stop if everything is
            // already at the floor of 3.
            let Some((idx, _)) = widths
                .iter()
                .enumerate()
                .max_by_key(|(_, w)| **w)
            else {
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

        self.lines
            .push(Line::from(Span::styled(top, frame_style)));
        if let Some(h) = head {
            self.lines
                .push(self.render_table_row(h, &widths, true, frame_style));
            self.lines
                .push(Line::from(Span::styled(mid, frame_style)));
        }
        for row in &t.body {
            self.lines
                .push(self.render_table_row(row, &widths, false, frame_style));
        }
        self.lines
            .push(Line::from(Span::styled(bot, frame_style)));
        self.lines.push(Line::from(Vec::<Span<'static>>::new()));
    }

    fn render_table_row(
        &self,
        row: &[Vec<Span<'static>>],
        widths: &[usize],
        is_header: bool,
        frame_style: Style,
    ) -> Line<'static> {
        let mut spans: Vec<Span<'static>> = Vec::new();
        spans.push(Span::styled("\u{2502} ".to_string(), frame_style)); // "│ "
        for (i, w) in widths.iter().enumerate() {
            let empty = Vec::new();
            let cell_spans = row.get(i).unwrap_or(&empty);
            let truncated = truncate_spans_to_width(cell_spans, *w);
            let used = spans_visual_width(&truncated);
            for s in truncated {
                let mut style = s.style;
                if is_header {
                    style = style.add_modifier(Modifier::BOLD);
                }
                spans.push(Span::styled(s.content, style));
            }
            if used < *w {
                spans.push(Span::raw(" ".repeat(*w - used)));
            }
            if i + 1 < widths.len() {
                spans.push(Span::styled(" \u{2502} ".to_string(), frame_style));
            } else {
                spans.push(Span::styled(" \u{2502}".to_string(), frame_style));
            }
        }
        Line::from(spans)
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
    spans
        .iter()
        .map(|s| s.content.as_ref().width())
        .sum()
}

/// Truncate a styled span sequence to fit within `max_w` visual
/// columns, appending `…` as a marker if truncation occurred. The
/// per-span style is preserved up to the truncation boundary;
/// content past the cap is dropped.
fn truncate_spans_to_width(spans: &[Span<'static>], max_w: usize) -> Vec<Span<'static>> {
    use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
    let mut out: Vec<Span<'static>> = Vec::with_capacity(spans.len());
    let mut used = 0usize;
    for span in spans {
        let span_w = span.content.as_ref().width();
        if used + span_w <= max_w {
            out.push(span.clone());
            used += span_w;
            continue;
        }
        // Truncate this span to the remaining budget, leaving room
        // for the … marker.
        let remaining = max_w.saturating_sub(used);
        if remaining < 2 {
            if remaining >= 1 {
                out.push(Span::styled("\u{2026}".to_string(), span.style));
            }
            return out;
        }
        let target_w = remaining - 1;
        let mut buf = String::new();
        let mut buf_w = 0usize;
        for ch in span.content.chars() {
            let cw = ch.width().unwrap_or(0);
            if buf_w + cw > target_w {
                break;
            }
            buf.push(ch);
            buf_w += cw;
        }
        buf.push('\u{2026}');
        out.push(Span::styled(buf, span.style));
        return out;
    }
    out
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
        render(src, &theme)
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

    #[test]
    fn renders_blockquote_with_left_rule() {
        let lines = render_plain("> quoted\n");
        assert!(lines.iter().any(|l| l.starts_with("\u{2503} ") && l.contains("quoted")));
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
        assert!(body_lines >= 2, "expected wrap to produce multiple lines, got {lines:?}");
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
        assert!(body[0].starts_with("\u{2022} "), "first line: {:?}", body[0]);
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
            lines.iter().any(|l| l.contains('\u{250c}') && l.contains('\u{2510}')),
            "missing top border in {lines:?}"
        );
        // Bottom border.
        assert!(
            lines.iter().any(|l| l.contains('\u{2514}') && l.contains('\u{2518}')),
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
    fn table_truncates_overlong_cells() {
        // Cell longer than TABLE_MAX_COL_WIDTH should be truncated
        // with `…` to keep the table within content width.
        let long = "x".repeat(super::TABLE_MAX_COL_WIDTH + 20);
        let src = format!("| Header |\n|--------|\n| {long} |\n");
        let lines = render_plain(&src);
        assert!(
            lines.iter().any(|l| l.contains('\u{2026}')),
            "expected … truncation marker in {lines:?}"
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
