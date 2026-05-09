use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::ui::line_edit::Mode as ViMode;
use crate::ui::theme::Theme;

pub struct PromptLine<'a> {
    pub prefix: &'a str,
    pub buffer: &'a str,
    pub theme: &'a Theme,
    /// Cursor position within the buffer (None = simple prompt, cursor at end).
    pub cursor_pos: Option<usize>,
    /// Vi mode indicator (None = simple prompt).
    pub vi_mode: Option<ViMode>,
}

impl PromptLine<'_> {
    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let mode_tag = match self.vi_mode {
            Some(ViMode::Normal) => "[V] ",
            Some(ViMode::Insert) => "[I] ",
            None => "",
        };
        let mode_style = match self.vi_mode {
            Some(ViMode::Normal) => Style::default()
                .fg(self.theme.cursor_bg)
                .add_modifier(Modifier::BOLD),
            _ => Style::default()
                .fg(self.theme.status_suffix)
                .add_modifier(Modifier::BOLD),
        };

        let prefix_style = Style::default()
            .fg(self.theme.prompt_prefix)
            .add_modifier(Modifier::BOLD);
        let text_style = Style::default().fg(self.theme.status_path);

        if let Some(pos) = self.cursor_pos {
            // Vi-mode prompt: cursor_pos is a char index; ratatui
            // renders each Span at its natural display width.
            let chars: Vec<char> = self.buffer.chars().collect();
            let before: String = chars[..pos.min(chars.len())].iter().collect();
            let cursor_char = chars
                .get(pos)
                .map_or_else(|| " ".to_string(), ToString::to_string);
            let after: String = if pos + 1 < chars.len() {
                chars[pos + 1..].iter().collect()
            } else {
                String::new()
            };

            let cursor_style = match self.vi_mode {
                Some(ViMode::Normal) => text_style.bg(self.theme.cursor_bg),
                _ => text_style.add_modifier(Modifier::UNDERLINED),
            };

            let line = Line::from(vec![
                Span::styled(mode_tag.to_string(), mode_style),
                Span::styled(self.prefix.to_string(), prefix_style),
                Span::styled(before, text_style),
                Span::styled(cursor_char, cursor_style),
                Span::styled(after, text_style),
            ]);
            frame.render_widget(Paragraph::new(line), area);
        } else {
            // Simple prompt: cursor is a blinking underscore at the end.
            let line = Line::from(vec![
                Span::styled(self.prefix.to_string(), prefix_style),
                Span::styled(self.buffer.to_string(), text_style),
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    fn render_prompt_to_string(prompt: &PromptLine<'_>, w: u16) -> String {
        let backend = TestBackend::new(w, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let area = Rect::new(0, 0, w, 1);
                prompt.render(f, area);
            })
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let mut out = String::new();
        for x in 0..buf.area.width {
            out.push_str(buf.cell((x, 0)).map_or(" ", |c| c.symbol()));
        }
        out.trim_end().to_string()
    }

    #[test]
    fn snapshot_prompt_simple() {
        // No vi mode, no cursor_pos — the legacy command prompt with a
        // blinking underscore tail.
        let theme = Theme::default();
        let prompt = PromptLine {
            prefix: ":",
            buffer: "edit",
            theme: &theme,
            cursor_pos: None,
            vi_mode: None,
        };
        let out = render_prompt_to_string(&prompt, 40);
        insta::assert_snapshot!(out);
    }

    #[test]
    fn snapshot_prompt_insert_mode() {
        let theme = Theme::default();
        let prompt = PromptLine {
            prefix: "$ ",
            buffer: "hello world",
            theme: &theme,
            cursor_pos: Some(5),
            vi_mode: Some(ViMode::Insert),
        };
        let out = render_prompt_to_string(&prompt, 40);
        insta::assert_snapshot!(out);
    }

    #[test]
    fn snapshot_prompt_normal_mode() {
        let theme = Theme::default();
        let prompt = PromptLine {
            prefix: "$ ",
            buffer: "hello world",
            theme: &theme,
            cursor_pos: Some(0),
            vi_mode: Some(ViMode::Normal),
        };
        let out = render_prompt_to_string(&prompt, 40);
        insta::assert_snapshot!(out);
    }
}
