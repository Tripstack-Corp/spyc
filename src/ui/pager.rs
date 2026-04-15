//! In-app scrollable pager overlay.
//!
//! Used for cspy-internal content where shelling out to `less` would be
//! overkill — long listings, version info, future command output. For
//! arbitrary file viewing we still defer to `$PAGER` because the user
//! probably has it configured the way they like.

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::ui::theme;

pub struct PagerView {
    pub title: String,
    pub lines: Vec<String>,
    /// Top line currently shown in the viewport (0-indexed).
    pub scroll: u16,
}

impl PagerView {
    pub fn new(title: impl Into<String>, lines: Vec<String>) -> Self {
        Self {
            title: title.into(),
            lines,
            scroll: 0,
        }
    }

    pub fn line_count(&self) -> u16 {
        u16::try_from(self.lines.len()).unwrap_or(u16::MAX)
    }

    /// Clamp `scroll` so we never scroll past the last page of content.
    pub fn clamp_scroll(&mut self, viewport_height: u16) {
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
}

pub fn render(frame: &mut Frame, area: Rect, view: &PagerView) {
    let inner_area = centered_rect(area, 90, 92);

    frame.render_widget(Clear, inner_area);

    let title = format!(
        "  {}   ({} lines, j/k scroll, g/G ends, q to close)  ",
        view.title,
        view.lines.len()
    );
    let block = Block::default().borders(Borders::ALL).title(Span::styled(
        title,
        Style::default()
            .fg(theme::PROMPT_PREFIX)
            .add_modifier(Modifier::BOLD),
    ));
    let body_area = block.inner(inner_area);
    frame.render_widget(block, inner_area);

    let lines: Vec<Line> = view
        .lines
        .iter()
        .map(|text| {
            Line::from(Span::styled(
                text.clone(),
                Style::default().fg(theme::STATUS_PATH),
            ))
        })
        .collect();
    let paragraph = Paragraph::new(lines).scroll((view.scroll, 0));
    frame.render_widget(paragraph, body_area);
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
