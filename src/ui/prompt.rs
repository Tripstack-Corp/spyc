use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::ui::theme;

pub struct PromptLine<'a> {
    pub prefix: &'a str,
    pub buffer: &'a str,
}

impl<'a> PromptLine<'a> {
    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let line = Line::from(vec![
            Span::styled(
                self.prefix.to_string(),
                Style::default()
                    .fg(theme::PROMPT_PREFIX)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                self.buffer.to_string(),
                Style::default().fg(theme::STATUS_PATH),
            ),
            Span::styled(
                "_".to_string(),
                Style::default()
                    .fg(theme::STATUS_SUFFIX)
                    .add_modifier(Modifier::SLOW_BLINK),
            ),
        ]);
        frame.render_widget(Paragraph::new(line), area);
    }
}
