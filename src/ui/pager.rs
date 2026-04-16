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
    widgets::{Block, Borders, Clear, Paragraph},
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
    /// When true, show whitespace markers like vim's `:set list`:
    /// `^I` for tab, `$` for EOL, `^M` for CR.
    pub show_whitespace: bool,
}

impl PagerView {
    /// Build a pager from plain strings. Each string becomes one
    /// unstyled line.
    pub fn new_plain(title: impl Into<String>, lines: Vec<String>) -> Self {
        Self {
            title: title.into(),
            lines: lines.into_iter().map(Line::from).collect(),
            scroll: 0,
            search: Search::Off,
            show_whitespace: false,
        }
    }

    /// Build a pager from raw bytes that may contain ANSI escape
    /// sequences. Colors, bold, underline etc. are preserved.
    pub fn new_ansi(title: impl Into<String>, bytes: &[u8]) -> Self {
        let text = bytes.into_text().unwrap_or_default();
        Self {
            title: title.into(),
            lines: text.lines,
            scroll: 0,
            search: Search::Off,
            show_whitespace: false,
        }
    }

    pub fn toggle_whitespace(&mut self) {
        self.show_whitespace = !self.show_whitespace;
    }

    pub fn line_count(&self) -> u16 {
        u16::try_from(self.lines.len()).unwrap_or(u16::MAX)
    }

    fn clamp_scroll(&mut self, viewport_height: u16) {
        let total = self.line_count();
        let max_scroll = total.saturating_sub(viewport_height.max(1));
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
        self.scroll = self.line_count().saturating_sub(viewport_height.max(1));
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
            } => Some(format!(
                "/{query}  {}/{}",
                cursor + 1,
                matches.len()
            )),
        }
    }
}

/// Flatten styled spans back to plain text (for case-insensitive matching).
fn line_plain_text(line: &Line) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

pub fn render(frame: &mut Frame, area: Rect, view: &PagerView, theme: &Theme) {
    let inner_area = centered_rect(area, 90, 92);

    frame.render_widget(Clear, inner_area);

    let title = format!(
        "  {}   ({} lines, / search, n/N match, j/k scroll, q to close)  ",
        view.title,
        view.lines.len()
    );
    let block = Block::default().borders(Borders::ALL).title(Span::styled(
        title,
        Style::default()
            .fg(theme.prompt_prefix)
            .add_modifier(Modifier::BOLD),
    ));
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

    // Build display lines — highlight matches and apply whitespace markers.
    let mut display_lines: Vec<Line<'static>> = view
        .lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let styled = styled_line_for_render(line, view, i, theme);
            if view.show_whitespace {
                apply_whitespace_markers(&styled, theme)
            } else {
                styled
            }
        })
        .collect();

    // EOF marker after the last content line.
    let eof_style = Style::default()
        .fg(theme.status_suffix)
        .add_modifier(Modifier::DIM);
    display_lines.push(Line::from(Span::styled("[EOF]", eof_style)));

    // Fill remaining viewport rows with `~` markers.
    let visible_start = usize::from(view.scroll);
    let visible_content = display_lines.len().saturating_sub(visible_start);
    let viewport_h = usize::from(content_area.height);
    if visible_content < viewport_h {
        let tilde_count = viewport_h - visible_content;
        for _ in 0..tilde_count {
            display_lines.push(Line::from(Span::styled("~", eof_style)));
        }
    }

    let paragraph = Paragraph::new(display_lines).scroll((view.scroll, 0));
    frame.render_widget(paragraph, content_area);

    if let (Some(rect), Some(text)) = (search_area, view.status_text()) {
        let style = Style::default()
            .fg(theme.prompt_prefix)
            .add_modifier(Modifier::BOLD);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(text, style))),
            rect,
        );
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
    let ws_style = Style::default()
        .fg(theme.pick)
        .add_modifier(Modifier::DIM);

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
                        out.push(Span::styled(
                            std::mem::take(&mut segment),
                            span.style,
                        ));
                    }
                    out.push(Span::styled("→", ws_style));
                }
                '\r' => {
                    if !segment.is_empty() {
                        out.push(Span::styled(
                            std::mem::take(&mut segment),
                            span.style,
                        ));
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
