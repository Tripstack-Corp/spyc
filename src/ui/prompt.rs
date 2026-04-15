use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::ui::theme::Theme;

pub struct PromptLine<'a> {
    pub prefix: &'a str,
    pub buffer: &'a str,
    pub theme: &'a Theme,
}

impl PromptLine<'_> {
    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let line = Line::from(vec![
            Span::styled(
                self.prefix.to_string(),
                Style::default()
                    .fg(self.theme.prompt_prefix)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                self.buffer.to_string(),
                Style::default().fg(self.theme.status_path),
            ),
            Span::styled(
                "_".to_string(),
                Style::default()
                    .fg(self.theme.status_suffix)
                    .add_modifier(Modifier::SLOW_BLINK),
            ),
        ]);
        frame.render_widget(Paragraph::new(line), area);
    }
}
