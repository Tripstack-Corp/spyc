//! In-app scrollable pager overlay with incremental search.
//!
//! Used for cspy-internal content where shelling out to `less` would be
//! overkill — long listings, file contents, captured `!` output, version
//! info. Arbitrary terminal-output viewing lives here too, with ANSI
//! colors preserved via `ansi-to-tui`.

use ansi_to_tui::IntoText;
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use crate::ui::theme::Theme;

/// Search mode inside the pager.
enum Search {
    /// No search in progress; j/k scroll normally.
    Off,
    /// The user is typing a query (triggered by `/`).
    Typing(String),
    /// A query has been committed. `matches` holds line indices that
    /// contain the query; `cursor` is an index into `matches`.
    Active {
        query: String,
        matches: Vec<usize>,
        cursor: usize,
    },
}

pub struct PagerView {
    pub title: String,
    /// Pre-styled lines. ANSI escapes in source are already converted to
    /// styled spans; plain text becomes a single unstyled span per line.
    pub lines: Vec<Line<'static>>,
    /// Top line currently shown in the viewport (0-indexed).
    pub scroll: u16,
    search: Search,
    /// When true, show whitespace markers + line numbers.
    pub show_whitespace: bool,
    /// When true, `s` saves the content to a file. Only for command
    /// output — not for files the user opened with `d`/Enter (they
    /// already exist on disk).
    pub saveable: bool,
    /// When true, the pager fills the entire terminal instead of the
    /// centered 90×92% box. Toggled with `f`.
    pub full_width: bool,
    /// Number of columns for multi-column layout (1 = normal single column).
    /// Lines flow top-to-bottom within each column, then left-to-right.
    pub columns: u8,
    /// When set, the pager is showing a file on disk. `v` opens this path
    /// directly in `$EDITOR`. When `None`, content is a buffer (command
    /// output, help, etc.) and `v` uses a temp file.
    pub source_path: Option<std::path::PathBuf>,
    /// When set, the pager acts as a picker: j/k move a highlighted cursor
    /// instead of scrolling, and Enter selects. The value is the 0-based
    /// line index of the highlighted row.
    pub picker_cursor: Option<usize>,
    /// When true, suppress [EOF] and tilde markers (content is still arriving).
    pub streaming: bool,
}

impl PagerView {
    /// Build a pager from plain strings. Each string becomes one
    /// unstyled line.
    /// Build a pager from pre-styled lines (e.g. the help overlay).
    pub fn new_styled(title: impl Into<String>, lines: Vec<Line<'static>>) -> Self {
        Self {
            title: title.into(),
            lines,
            scroll: 0,
            search: Search::Off,
            show_whitespace: false,
            saveable: false,
            full_width: false,
            columns: 1,
            source_path: None,
            picker_cursor: None,
            streaming: false,
        }
    }

    pub fn new_plain(title: impl Into<String>, lines: Vec<String>) -> Self {
        Self {
            title: title.into(),
            lines: lines.into_iter().map(Line::from).collect(),
            scroll: 0,
            search: Search::Off,
            show_whitespace: false,
            saveable: false,
            full_width: false,
            columns: 1,
            source_path: None,
            picker_cursor: None,
            streaming: false,
        }
    }

    /// Build a pager from raw bytes that may contain ANSI escape
    /// sequences. Colors, bold, underline etc. are preserved.
    /// Saveable by default (command output).
    pub fn new_ansi(title: impl Into<String>, bytes: &[u8]) -> Self {
        let text = bytes.into_text().unwrap_or_default();
        Self {
            title: title.into(),
            lines: text.lines,
            scroll: 0,
            search: Search::Off,
            show_whitespace: false,
            saveable: true,
            full_width: false,
            columns: 1,
            source_path: None,
            picker_cursor: None,
            streaming: false,
        }
    }

    /// All lines as plain text (ANSI stripped), joined with newlines.
    fn plain_text(&self) -> String {
        self.lines
            .iter()
            .map(line_plain_text)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Save the plain-text content to a timestamped file in the current
    /// directory. Returns the path on success.
    pub fn save_to_file(&self) -> std::io::Result<std::path::PathBuf> {
        let now = crate::sysinfo::format_now().replace([' ', ':'], "_");
        let stamp = now.trim_end_matches("_UTC");
        let filename = format!("cspy_output_{stamp}.txt");
        let path = std::env::current_dir()?.join(&filename);
        std::fs::write(&path, self.plain_text() + "\n")?;
        Ok(path)
    }

    /// Write the plain-text content to a temp file for editing.
    pub fn write_to_temp(&self) -> std::io::Result<std::path::PathBuf> {
        let dir = std::env::temp_dir();
        let filename = format!("cspy_pager_{}.txt", std::process::id());
        let path = dir.join(filename);
        std::fs::write(&path, self.plain_text() + "\n")?;
        Ok(path)
    }

    /// Move picker cursor up/down (only when `picker_cursor` is set).
    pub fn picker_move(&mut self, delta: isize, viewport_height: u16) {
        let Some(cur) = self.picker_cursor.as_mut() else {
            return;
        };
        let n = self.lines.len();
        if n == 0 {
            return;
        }
        let new = (*cur as isize + delta).clamp(0, n as isize - 1) as usize;
        *cur = new;
        // Auto-scroll to keep the cursor visible.
        let top = self.scroll as usize;
        let bot = top + viewport_height as usize;
        if new < top {
            self.scroll = new as u16;
        } else if new >= bot {
            self.scroll = (new + 1).saturating_sub(viewport_height as usize) as u16;
        }
    }

    pub fn toggle_full_width(&mut self) {
        self.full_width = !self.full_width;
    }

    /// Yank the full pager content to the system clipboard via pbcopy.
    pub fn yank_to_clipboard(&self) -> std::io::Result<()> {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let text = self.plain_text();
        let mut child = Command::new("pbcopy")
            .stdin(Stdio::piped())
            .spawn()?;
        if let Some(stdin) = child.stdin.as_mut() {
            stdin.write_all(text.as_bytes())?;
        }
        child.wait()?;
        Ok(())
    }

    pub fn toggle_whitespace(&mut self) {
        self.show_whitespace = !self.show_whitespace;
    }

    pub fn line_count(&self) -> u16 {
        u16::try_from(self.lines.len()).unwrap_or(u16::MAX)
    }

    /// Lines visible per "page" — viewport_height * columns.
    pub fn page_lines(&self, viewport_height: u16) -> u16 {
        viewport_height.saturating_mul(self.columns.max(1) as u16)
    }

    fn clamp_scroll(&mut self, viewport_height: u16) {
        let total = self.line_count();
        let max_scroll = total.saturating_sub(self.page_lines(viewport_height).max(1));
        if self.scroll > max_scroll {
            self.scroll = max_scroll;
        }
    }

    pub fn scroll_by(&mut self, delta: i32, viewport_height: u16) {
        let current = i32::from(self.scroll);
        let new = (current + delta).max(0);
        self.scroll = u16::try_from(new).unwrap_or(u16::MAX);
        self.clamp_scroll(viewport_height);
    }

    pub fn scroll_to_top(&mut self) {
        self.scroll = 0;
    }

    pub fn scroll_to_bottom(&mut self, viewport_height: u16) {
        self.scroll = self.line_count().saturating_sub(self.page_lines(viewport_height).max(1));
    }

    /// Position indicator: "Top", "Bot", "All", or "NN%".
    /// Percentage is based on the bottom visible line relative to the total
    /// (like `less`), so it reflects how much of the document you've seen.
    pub fn position_indicator(&self, viewport_height: u16) -> String {
        let total = self.line_count();
        let page = self.page_lines(viewport_height);
        if total <= page {
            return "All".to_string();
        }
        if self.scroll == 0 {
            return "Top".to_string();
        }
        let bottom = u32::from(self.scroll) + u32::from(page);
        let total32 = u32::from(total);
        if bottom >= total32 {
            return "Bot".to_string();
        }
        let pct = (bottom * 100) / total32;
        format!("{pct}%")
    }

    // ---- Search ----------------------------------------------------------

    /// True when the pager is capturing text input for a `/` search.
    pub const fn is_typing_search(&self) -> bool {
        matches!(self.search, Search::Typing(_))
    }

    pub fn begin_search(&mut self) {
        self.search = Search::Typing(String::new());
    }

    /// Append a char to the search buffer (only meaningful while typing).
    pub fn search_push_char(&mut self, c: char) {
        if let Search::Typing(buf) = &mut self.search {
            buf.push(c);
        }
    }

    pub fn search_backspace(&mut self) {
        if let Search::Typing(buf) = &mut self.search {
            buf.pop();
        }
    }

    /// Cancel an in-progress search and clear any active match state.
    pub fn cancel_search(&mut self) {
        self.search = Search::Off;
    }

    /// Commit the typed query: find matching lines, jump to the first.
    /// No matches → revert to Off and return false so the caller can flash.
    pub fn commit_search(&mut self, viewport_height: u16) -> bool {
        let query = match std::mem::replace(&mut self.search, Search::Off) {
            Search::Typing(q) => q,
            other => {
                self.search = other;
                return true;
            }
        };
        if query.is_empty() {
            return true;
        }
        let needle = query.to_lowercase();
        let matches: Vec<usize> = self
            .lines
            .iter()
            .enumerate()
            .filter(|(_, line)| line_plain_text(line).to_lowercase().contains(&needle))
            .map(|(i, _)| i)
            .collect();
        if matches.is_empty() {
            return false;
        }
        self.scroll_to_match(matches[0], viewport_height);
        self.search = Search::Active {
            query,
            matches,
            cursor: 0,
        };
        true
    }

    /// Move to the next match (wraps). No-op when no search is active.
    pub fn search_next(&mut self, viewport_height: u16) {
        let Search::Active {
            matches, cursor, ..
        } = &mut self.search
        else {
            return;
        };
        if matches.is_empty() {
            return;
        }
        *cursor = (*cursor + 1) % matches.len();
        let line_idx = matches[*cursor];
        self.scroll_to_match(line_idx, viewport_height);
    }

    /// Move to the previous match (wraps).
    pub fn search_prev(&mut self, viewport_height: u16) {
        let Search::Active {
            matches, cursor, ..
        } = &mut self.search
        else {
            return;
        };
        if matches.is_empty() {
            return;
        }
        *cursor = if *cursor == 0 {
            matches.len() - 1
        } else {
            *cursor - 1
        };
        let line_idx = matches[*cursor];
        self.scroll_to_match(line_idx, viewport_height);
    }

    /// Scroll the viewport so `line_idx` is roughly a third of the way
    /// down — gives context above and more content below.
    fn scroll_to_match(&mut self, line_idx: usize, viewport_height: u16) {
        let third = i64::from(viewport_height) / 3;
        let target = line_idx as i64 - third;
        let scroll = target.max(0);
        self.scroll = u16::try_from(scroll).unwrap_or(u16::MAX);
        self.clamp_scroll(viewport_height);
    }

    /// For the render layer: is the given line index one of the search
    /// matches? (Returns (is_match, is_current_match).)
    fn match_state(&self, line_idx: usize) -> (bool, bool) {
        match &self.search {
            Search::Active {
                matches, cursor, ..
            } => (
                matches.binary_search(&line_idx).is_ok(),
                matches.get(*cursor) == Some(&line_idx),
            ),
            _ => (false, false),
        }
    }

    /// Current search status for the footer line (e.g. `/foo 3/17`).
    fn status_text(&self) -> Option<String> {
        match &self.search {
            Search::Off => None,
            Search::Typing(buf) => Some(format!("/{buf}_")),
            Search::Active {
                query,
                matches,
                cursor,
            } => Some(format!("/{query}  {}/{}", cursor + 1, matches.len())),
        }
    }
}

/// Flatten styled spans back to plain text (for case-insensitive matching).
fn line_plain_text(line: &Line) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

pub fn render(frame: &mut Frame, area: Rect, view: &PagerView, theme: &Theme) {
    let inner_area = if view.full_width { area } else { centered_rect(area, 90, 92) };

    frame.render_widget(Clear, inner_area);

    let pos = view.position_indicator(inner_area.height.saturating_sub(2));
    let title = format!(
        "  {}   ({} lines)  ",
        view.title,
        view.lines.len()
    );
    let title_right = format!("  {pos}  ");
    let block = if view.full_width {
        // No border in full-width mode so terminal text selection is clean.
        Block::default()
    } else {
        Block::default()
            .borders(Borders::ALL)
            .title(Span::styled(
                title,
                Style::default()
                    .fg(theme.prompt_prefix)
                    .add_modifier(Modifier::BOLD),
            ))
            .title_bottom(Line::from(Span::styled(
                title_right,
                Style::default()
                    .fg(theme.status_suffix)
                    .add_modifier(Modifier::BOLD),
            )).right_aligned())
    };
    let body_area = block.inner(inner_area);
    frame.render_widget(block, inner_area);

    // Reserve the bottom row of the body for the search status when any
    // search state is visible; otherwise the whole body shows content.
    let (content_area, search_area) = if view.status_text().is_some() {
        (
            Rect {
                x: body_area.x,
                y: body_area.y,
                width: body_area.width,
                height: body_area.height.saturating_sub(1),
            },
            Some(Rect {
                x: body_area.x,
                y: body_area.y + body_area.height.saturating_sub(1),
                width: body_area.width,
                height: 1,
            }),
        )
    } else {
        (body_area, None)
    };

    let ncols = view.columns.max(1) as usize;
    if ncols > 1 {
        render_multi_column(frame, content_area, view, theme, ncols);
    } else {
        render_single_column(frame, content_area, view, theme);
    }

    if let (Some(rect), Some(text)) = (search_area, view.status_text()) {
        let style = Style::default()
            .fg(theme.prompt_prefix)
            .add_modifier(Modifier::BOLD);
        frame.render_widget(Paragraph::new(Line::from(Span::styled(text, style))), rect);
    }
}

fn render_single_column(frame: &mut Frame, content_area: Rect, view: &PagerView, theme: &Theme) {
    let viewport_h = content_area.height as usize;
    let start = view.scroll as usize;
    let content_end = view.lines.len();
    let slice_end = (start + viewport_h).min(content_end);

    let total_lines = view.lines.len();
    let gutter_w = if view.show_whitespace {
        total_lines.max(1).ilog10() as usize + 2
    } else {
        0
    };
    let ln_style = Style::default()
        .fg(theme.status_suffix)
        .add_modifier(Modifier::DIM);

    let mut display_lines: Vec<Line<'static>> = view.lines[start..slice_end]
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let abs_idx = start + i;
            let mut styled = styled_line_for_render(line, view, abs_idx, theme);
            // Highlight the picker cursor row.
            if view.picker_cursor == Some(abs_idx) {
                styled = Line::from(
                    styled
                        .spans
                        .into_iter()
                        .map(|s| {
                            Span::styled(
                                s.content,
                                s.style
                                    .bg(theme.cursor_bg)
                                    .fg(theme.cursor_fg)
                                    .add_modifier(Modifier::BOLD),
                            )
                        })
                        .collect::<Vec<_>>(),
                );
            }
            let styled = if view.show_whitespace {
                apply_whitespace_markers(&styled, theme)
            } else {
                styled
            };
            if gutter_w > 0 {
                let num = format!("{:>width$} ", abs_idx + 1, width = gutter_w - 1);
                let mut spans = vec![Span::styled(num, ln_style)];
                spans.extend(styled.spans);
                Line::from(spans)
            } else {
                styled
            }
        })
        .collect();

    let eof_style = Style::default()
        .fg(theme.status_suffix)
        .add_modifier(Modifier::DIM);
    if slice_end >= content_end && display_lines.len() < viewport_h && !view.streaming {
        display_lines.push(Line::from(Span::styled("[EOF]", eof_style)));
        while display_lines.len() < viewport_h {
            display_lines.push(Line::from(Span::styled("~", eof_style)));
        }
    }

    let paragraph = Paragraph::new(display_lines).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, content_area);
}

fn render_multi_column(
    frame: &mut Frame,
    content_area: Rect,
    view: &PagerView,
    theme: &Theme,
    ncols: usize,
) {
    let viewport_h = content_area.height as usize;
    let start = view.scroll as usize;
    let content_end = view.lines.len();
    let col_gap = 2u16;
    // Divide available width evenly (minus gaps between columns).
    let total_gap = col_gap * (ncols as u16).saturating_sub(1);
    let col_w = content_area.width.saturating_sub(total_gap) / ncols as u16;

    let eof_style = Style::default()
        .fg(theme.status_suffix)
        .add_modifier(Modifier::DIM);

    for col in 0..ncols {
        let col_start = start + col * viewport_h;
        let col_end = (col_start + viewport_h).min(content_end);
        let x = content_area.x + (col as u16) * (col_w + col_gap);
        let col_rect = Rect {
            x,
            y: content_area.y,
            width: col_w,
            height: content_area.height,
        };

        let mut display_lines: Vec<Line<'static>> = if col_start < content_end {
            view.lines[col_start..col_end]
                .iter()
                .enumerate()
                .map(|(i, line)| {
                    let abs_idx = col_start + i;
                    styled_line_for_render(line, view, abs_idx, theme)
                })
                .collect()
        } else {
            Vec::new()
        };

        // Fill remaining rows with tilde markers.
        if col_end >= content_end && display_lines.len() < viewport_h && !view.streaming {
            if col_start < content_end {
                display_lines.push(Line::from(Span::styled("[EOF]", eof_style)));
            }
            while display_lines.len() < viewport_h {
                display_lines.push(Line::from(Span::styled("~", eof_style)));
            }
        }

        let paragraph = Paragraph::new(display_lines);
        frame.render_widget(paragraph, col_rect);

        // Draw a thin separator between columns.
        if col + 1 < ncols {
            let sep_x = x + col_w;
            let sep_style = Style::default()
                .fg(theme.status_suffix)
                .add_modifier(Modifier::DIM);
            for row in 0..content_area.height {
                let buf = frame.buffer_mut();
                buf.set_string(sep_x, content_area.y + row, "│", sep_style);
            }
        }
    }
}

/// Apply match highlighting to a line when a search is active. The
/// current match gets the cursor-bg color for max pop; other matches get
/// a softer bg tint.
fn styled_line_for_render(
    line: &Line<'static>,
    view: &PagerView,
    idx: usize,
    theme: &Theme,
) -> Line<'static> {
    let (is_match, is_current) = view.match_state(idx);
    if !is_match {
        return line.clone();
    }
    let bg = if is_current {
        theme.cursor_bg
    } else {
        theme.other
    };
    // Apply the background across every span in the line so the whole
    // row reads as "a hit" without clobbering existing fg colors.
    let spans = line
        .spans
        .iter()
        .map(|s| {
            let mut style = s.style;
            style = style.bg(bg);
            if is_current {
                style = style.add_modifier(Modifier::BOLD);
            }
            Span::styled(s.content.clone(), style)
        })
        .collect::<Vec<_>>();
    Line::from(spans)
}

/// Vim-style whitespace substitution. Applied per span to keep existing
/// colors. Visual cues:
///   `→`  tab
///   `·`  trailing space
///   `^M` carriage return
///   `$`  end-of-line (non-empty lines only — blank lines are obviously blank)
fn apply_whitespace_markers(line: &Line<'static>, theme: &Theme) -> Line<'static> {
    // Warm amber-ish so markers are visible against dark backgrounds
    // without fighting the content. Uses the pick color (amber) dimmed.
    let ws_style = Style::default().fg(theme.pick).add_modifier(Modifier::DIM);

    // Check if the whole line is empty / whitespace-only.
    let plain = line_plain_text(line);
    if plain.trim().is_empty() {
        // Don't clutter blank lines with `$`.
        return line.clone();
    }

    let mut out: Vec<Span<'static>> = Vec::new();
    for span in &line.spans {
        let text: &str = &span.content;
        let mut segment = String::new();
        for ch in text.chars() {
            match ch {
                '\t' => {
                    if !segment.is_empty() {
                        out.push(Span::styled(std::mem::take(&mut segment), span.style));
                    }
                    out.push(Span::styled("→", ws_style));
                }
                '\r' => {
                    if !segment.is_empty() {
                        out.push(Span::styled(std::mem::take(&mut segment), span.style));
                    }
                    out.push(Span::styled("^M", ws_style));
                }
                _ => segment.push(ch),
            }
        }
        if !segment.is_empty() {
            out.push(Span::styled(segment, span.style));
        }
    }

    // Replace trailing spaces with `·` for visibility.
    if let Some(last) = out.last_mut() {
        let content: &str = &last.content;
        if content.ends_with(' ') {
            let trimmed = content.trim_end();
            let trailing_count = content.len() - trimmed.len();
            let style = last.style;
            *last = Span::styled(trimmed.to_string(), style);
            let dots: String = "·".repeat(trailing_count);
            out.push(Span::styled(dots, ws_style));
        }
    }

    out.push(Span::styled("$", ws_style));
    Line::from(out)
}

const fn centered_rect(area: Rect, percent_w: u16, percent_h: u16) -> Rect {
    let w = area.width * percent_w / 100;
    let h = area.height * percent_h / 100;
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect {
        x,
        y,
        width: w,
        height: h,
    }
}
