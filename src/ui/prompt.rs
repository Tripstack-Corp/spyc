use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::Paragraph,
};

use crate::ui::line_edit::Mode as ViMode;
use crate::ui::theme::Theme;
use crate::ui::wrap::word_wrap_ranges;

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
    /// The prompt as styled text runs (mode tag, prefix, buffer split around the
    /// cursor) concatenated left-to-right. Wrapping slices across these, so a
    /// long line keeps its prefix/cursor styling on whatever row it lands on.
    fn runs(&self) -> Vec<(String, Style)> {
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

        let mode_tag = match self.vi_mode {
            Some(ViMode::Normal) => "[V] ",
            Some(ViMode::Insert) => "[I] ",
            None => "",
        };

        let mut runs = vec![
            (mode_tag.to_string(), mode_style),
            (self.prefix.to_string(), prefix_style),
        ];

        if let Some(pos) = self.cursor_pos {
            // Vi-mode prompt: highlight the char under `pos` (block in Normal,
            // underline in Insert); the rest is plain buffer text.
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
            runs.push((before, text_style));
            runs.push((cursor_char, cursor_style));
            runs.push((after, text_style));
        } else {
            // Simple prompt: cursor is a blinking underscore at the end.
            runs.push((self.buffer.to_string(), text_style));
            runs.push((
                "_".to_string(),
                Style::default()
                    .fg(self.theme.status_suffix)
                    .add_modifier(Modifier::SLOW_BLINK),
            ));
        }
        runs
    }

    /// Word-wrap the styled runs to `width` columns, slicing each run across
    /// line breaks so styling survives the wrap. The single source of truth for
    /// both the drawn rows and the reserved height ([`Self::line_count`]).
    fn wrapped_lines(&self, width: u16) -> Vec<Line<'static>> {
        let runs = self.runs();
        // Byte spans of each run within the concatenated text.
        let mut full = String::new();
        let mut spans: Vec<(usize, usize, Style)> = Vec::with_capacity(runs.len());
        for (text, style) in &runs {
            let start = full.len();
            full.push_str(text);
            spans.push((start, full.len(), *style));
        }
        word_wrap_ranges(&full, width.max(1) as usize)
            .into_iter()
            .map(|(ls, le)| {
                let line: Vec<Span<'static>> = spans
                    .iter()
                    .filter_map(|&(rs, re, style)| {
                        let s = ls.max(rs);
                        let e = le.min(re);
                        (s < e).then(|| Span::styled(full[s..e].to_string(), style))
                    })
                    .collect();
                Line::from(line)
            })
            .collect()
    }

    /// How many rows the prompt needs at `width` columns once wrapped — so the
    /// layout can grow the prompt rect upward to fit a long command line
    /// instead of truncating it. Always ≥ 1.
    pub fn line_count(&self, width: u16) -> u16 {
        u16::try_from(self.wrapped_lines(width).len())
            .unwrap_or(u16::MAX)
            .max(1)
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        // Pre-wrapped to `area.width`, so each line already fits — render them
        // verbatim (no further ratatui wrapping) into the rows the layout
        // reserved. A long command line spills downward over the list bottom.
        frame.render_widget(
            Paragraph::new(Text::from(self.wrapped_lines(area.width))),
            area,
        );
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

    /// Render across `h` rows and return the non-empty rows joined by `\n` —
    /// for asserting a long line actually wraps.
    fn render_prompt_rows(prompt: &PromptLine<'_>, w: u16, h: u16) -> Vec<String> {
        let backend = TestBackend::new(w, h);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| prompt.render(f, Rect::new(0, 0, w, h)))
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        (0..h)
            .map(|y| {
                let mut row = String::new();
                for x in 0..w {
                    row.push_str(buf.cell((x, y)).map_or(" ", |c| c.symbol()));
                }
                row.trim_end().to_string()
            })
            .filter(|r| !r.is_empty())
            .collect()
    }

    #[test]
    fn line_count_grows_with_a_long_command() {
        let theme = Theme::default();
        let prompt = PromptLine {
            prefix: "!",
            buffer: "a really long command line blah blah blah lkjasldkfjlaksdjf",
            theme: &theme,
            cursor_pos: Some(10),
            vi_mode: Some(ViMode::Insert),
        };
        assert_eq!(prompt.line_count(200), 1, "fits on one row when wide");
        let narrow = prompt.line_count(20);
        assert!(
            narrow >= 3,
            "wraps to multiple rows when narrow (got {narrow})"
        );
    }

    #[test]
    fn long_command_wraps_instead_of_truncating() {
        let theme = Theme::default();
        let prompt = PromptLine {
            prefix: "!",
            buffer: "alpha bravo charlie delta echo foxtrot golf hotel india",
            theme: &theme,
            cursor_pos: None,
            vi_mode: None,
        };
        let rows = render_prompt_rows(&prompt, 20, prompt.line_count(20));
        assert!(
            rows.len() >= 3,
            "expected multiple wrapped rows, got {rows:?}"
        );
        // Every word survives across the wrap (nothing truncated away).
        let joined = rows.join(" ");
        for word in ["alpha", "foxtrot", "india"] {
            assert!(joined.contains(word), "{word} missing from {rows:?}");
        }
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
