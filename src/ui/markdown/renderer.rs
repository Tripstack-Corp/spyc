//! The `pulldown-cmark` event → styled-`Line` state machine.
//!
//! `Renderer` and its `TableBuilder` / `CodeBlockState` companions are
//! defined in the parent module; this file holds the `impl` that walks
//! events and accumulates spans. `super::render` constructs a `Renderer`,
//! feeds it events, and calls `finish`. Split out of `markdown.rs`
//! verbatim during the 800-LoC decomposition — behavior-identical.

use pulldown_cmark::{CodeBlockKind, Event, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use super::wrap::{slice_spans, spans_visual_width, wrap_spans_to_width};
use super::{
    CONTENT_WIDTH, CodeBlockState, MarkdownDoc, MermaidBlock, PROSE_WRAP_MIN, Renderer, StyleMods,
    TABLE_MAX_COL_WIDTH_CEILING, TABLE_MAX_COL_WIDTH_FALLBACK, TableBuilder, heading_depth,
};
use crate::ui::theme::Theme;
use crate::ui::wrap::word_wrap_ranges;

impl<'t> Renderer<'t> {
    pub(super) fn new(theme: &'t Theme, table_width_hint: Option<usize>) -> Self {
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
            style_mods: StyleMods::default(),
            list_indent: 0,
            in_blockquote: false,
            code_block: None,
            pending_link_url: None,
            table: None,
            just_started_item: false,
            table_width,
            prose_width,
            mermaid_blocks: Vec::new(),
        }
    }

    pub(super) fn finish_doc(mut self) -> MarkdownDoc {
        if !self.current.is_empty() {
            self.flush_line();
        }
        MarkdownDoc {
            lines: self.lines,
            mermaid_blocks: self.mermaid_blocks,
        }
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
                let style = base_style.add_modifier(self.style_mods.current());
                self.current.push(Span::styled(chunk.to_string(), style));
            }
        }
    }

    // The `if !self.current.is_empty() { self.flush_line() }` guards
    // below look collapsible to clippy, but they're not -- flush_line
    // unconditionally pushes a Line, so calling it on empty content
    // would emit a stray blank row.
    #[allow(clippy::collapsible_if, clippy::collapsible_match)]
    pub(super) fn handle(&mut self, event: Event<'_>) {
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
                self.style_mods.push(Modifier::BOLD);
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
                self.style_mods.push(Modifier::ITALIC);
            }
            Tag::Strong => {
                self.style_mods.push(Modifier::BOLD);
            }
            Tag::Strikethrough => {
                self.style_mods.push(Modifier::CROSSED_OUT);
            }
            Tag::Link { dest_url, .. } => {
                self.pending_link_url = Some(dest_url.into_string());
                self.style_mods.push(Modifier::UNDERLINED);
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
            Tag::Table(_alignments) => {
                if !self.current.is_empty() {
                    self.flush_line();
                }
                self.table = Some(TableBuilder {
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
                self.style_mods.pop(Modifier::BOLD);
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
            TagEnd::Emphasis => self.style_mods.pop(Modifier::ITALIC),
            TagEnd::Strong => self.style_mods.pop(Modifier::BOLD),
            TagEnd::Strikethrough => self.style_mods.pop(Modifier::CROSSED_OUT),
            TagEnd::Link => {
                self.style_mods.pop(Modifier::UNDERLINED);
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
    /// are computed from natural cell widths, capped per-column
    /// (TABLE_MAX_COL_WIDTH_FALLBACK..TABLE_MAX_COL_WIDTH_CEILING,
    /// scaled by the table-width budget) and trimmed proportionally so
    /// the total fits inside `self.table_width`. Cells longer than the
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
            self.lines
                .push(Line::from(Span::styled(mid.clone(), frame_style)));
        }
        for (i, row) in t.body.iter().enumerate() {
            // Fence each body row with the same `├─┼─┤` separator that follows
            // the header, so rows read as distinct grid cells rather than a
            // run-together block. Skipped before the first body row (the header
            // separator already sits there; for a headerless table the top
            // border does).
            if i > 0 {
                self.lines
                    .push(Line::from(Span::styled(mid.clone(), frame_style)));
            }
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
        // A ```mermaid block is a diagram, not code: record it (source + the
        // rendered-line range) so the pager can open/inline it, and emit a
        // discoverable placeholder rather than syntax-highlighting the source.
        if state.lang == "mermaid" {
            self.emit_mermaid_block(body);
            return;
        }
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

    /// Emit a ` ```mermaid ` block's placeholder and record it in
    /// `mermaid_blocks`, showing the source under a discoverable header.
    fn emit_mermaid_block(&mut self, body: &str) {
        let start = self.lines.len();
        let header = Style::default()
            .fg(self.theme.prompt_prefix)
            .add_modifier(Modifier::BOLD);
        let dim = Style::default()
            .fg(self.theme.status_suffix)
            .add_modifier(Modifier::DIM);
        self.lines.push(Line::from(Span::styled(
            "\u{25a3} mermaid diagram \u{2014} i: view in terminal \u{00b7} o: open in image viewer"
                .to_string(),
            header,
        )));
        self.lines
            .push(Line::from(Span::styled("\u{2500}".repeat(40), dim)));
        for raw in body.lines() {
            self.lines.push(Line::from(Span::styled(
                raw.to_string(),
                Style::default().fg(self.theme.other),
            )));
        }
        self.lines
            .push(Line::from(Span::styled("\u{2500}".repeat(40), dim)));
        self.lines.push(Line::from(Vec::<Span<'static>>::new()));
        self.mermaid_blocks.push(MermaidBlock {
            line_range: start..self.lines.len(),
            source: body.to_string(),
        });
    }
}
