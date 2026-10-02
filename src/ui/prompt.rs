use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
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
    /// Drawn dimmed while `buffer` is empty: what Enter would submit.
    pub suggestion: Option<&'a str>,
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
        let ghost_style = text_style.add_modifier(Modifier::DIM);
        let ghost = self
            .suggestion
            .filter(|s| self.buffer.is_empty() && !s.is_empty());

        let mode_tag = match self.vi_mode {
            Some(ViMode::Normal) => "[V] ",
            Some(ViMode::Insert) => "[I] ",
            None => "",
        };

        let mut runs = vec![
            (mode_tag.to_string(), mode_style),
            (self.prefix.to_string(), prefix_style),
        ];

        if let (Some(_), Some(ghost)) = (self.cursor_pos, ghost) {
            // The cursor sits on the suggestion's first character, the way a
            // shell autosuggestion reads: typing starts here and replaces it.
            let mut chars = ghost.chars();
            let first = chars.next().map(String::from).unwrap_or_default();
            let cursor_style = match self.vi_mode {
                Some(ViMode::Normal) => ghost_style.bg(self.theme.cursor_bg),
                _ => ghost_style.add_modifier(Modifier::UNDERLINED),
            };
            runs.push((first, cursor_style));
            runs.push((chars.as_str().to_string(), ghost_style));
        } else if let Some(pos) = self.cursor_pos {
            // Vi-mode prompt: highlight the char under `pos` (block in Normal,
            // underline in Insert); the rest is plain buffer text.
            let chars: Vec<char> = self.buffer.chars().collect();
            let before: String = chars[..pos.min(chars.len())].iter().collect();
            let after: String = if pos + 1 < chars.len() {
                chars[pos + 1..].iter().collect()
            } else {
                String::new()
            };
            let normal = matches!(self.vi_mode, Some(ViMode::Normal));
            // Past the last character — the ordinary typing position — there is no
            // glyph to style, and an UNDERLINED blank is only as visible as the
            // terminal's willingness to underline an empty cell. Insert mode draws a
            // real `_` there instead. Normal mode keeps its blank: a background
            // paints an empty cell regardless.
            let (cursor_char, cursor_style) = if pos >= chars.len() && !normal {
                (
                    "_".to_string(),
                    Style::default().fg(self.theme.status_suffix),
                )
            } else {
                let ch = chars
                    .get(pos)
                    .map_or_else(|| " ".to_string(), ToString::to_string);
                let style = if normal {
                    text_style.bg(self.theme.cursor_bg)
                } else {
                    text_style.add_modifier(Modifier::UNDERLINED)
                };
                (ch, style)
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
            if let Some(ghost) = ghost {
                runs.push((ghost.to_string(), ghost_style));
            }
        }
        runs
    }

    /// Word-wrap the styled runs to `width` columns, slicing each run across
    /// line breaks so styling survives the wrap. The single source of truth for
    /// both the drawn rows and the reserved height ([`Self::line_count`]).
    pub(crate) fn wrapped_lines(&self, width: u16) -> Vec<Line<'static>> {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend, layout::Rect, text::Text, widgets::Paragraph};

    fn render_prompt_to_string(prompt: &PromptLine<'_>, w: u16) -> String {
        let backend = TestBackend::new(w, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let area = Rect::new(0, 0, w, 1);
                f.render_widget(Paragraph::new(Text::from(prompt.wrapped_lines(w))), area);
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
            .draw(|f| {
                f.render_widget(
                    Paragraph::new(Text::from(prompt.wrapped_lines(w))),
                    Rect::new(0, 0, w, h),
                );
            })
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
            suggestion: None,
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
            suggestion: None,
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
            suggestion: None,
        };
        let out = render_prompt_to_string(&prompt, 40);
        insta::assert_snapshot!(out);
    }

    /// The ordinary typing position must be visible. Past the last character
    /// there's no glyph to underline, and `render_prompt_to_string` trims the row
    /// — so an underlined trailing SPACE is not merely faint here, it's absent,
    /// the same way it was absent on screen for whatever the terminal decides
    /// about underlining an empty cell. A real `_` survives both.
    #[test]
    fn insert_cursor_past_the_last_char_draws_an_underscore() {
        let theme = Theme::default();
        let prompt = PromptLine {
            prefix: "!",
            buffer: "this is a test",
            theme: &theme,
            cursor_pos: Some(14), // one past the final `t`
            vi_mode: Some(ViMode::Insert),
            suggestion: None,
        };
        assert_eq!(render_prompt_to_string(&prompt, 40), "[I] !this is a test_");
    }

    /// ...and only there. On a character the cursor is styling, not a glyph, so
    /// nothing may be spliced into the text the user typed.
    #[test]
    fn insert_cursor_on_a_char_adds_no_glyph() {
        let theme = Theme::default();
        let prompt = PromptLine {
            prefix: "!",
            buffer: "this is a test",
            theme: &theme,
            cursor_pos: Some(5), // on the `i` of `is`
            vi_mode: Some(ViMode::Insert),
            suggestion: None,
        };
        assert_eq!(render_prompt_to_string(&prompt, 40), "[I] !this is a test");
    }

    /// Normal mode keeps the blank cell: its cursor is a background block, which
    /// paints an empty cell fine. An `_` here would be a character the buffer
    /// doesn't contain, sitting where vi shows a block.
    #[test]
    fn normal_cursor_never_draws_an_underscore() {
        let theme = Theme::default();
        for (buffer, pos) in [("this is a test", 13), ("", 0)] {
            let prompt = PromptLine {
                prefix: "!",
                buffer,
                theme: &theme,
                cursor_pos: Some(pos),
                vi_mode: Some(ViMode::Normal),
                suggestion: None,
            };
            let out = render_prompt_to_string(&prompt, 40);
            assert!(
                !out.contains('_'),
                "Normal mode drew an underscore: {out:?}"
            );
            assert_eq!(out, format!("[V] !{buffer}"));
        }
    }

    fn jump_prompt<'a>(theme: &'a Theme, buffer: &'a str, mode: ViMode) -> PromptLine<'a> {
        PromptLine {
            prefix: "jump to: ",
            buffer,
            theme,
            cursor_pos: Some(buffer.chars().count()),
            vi_mode: Some(mode),
            suggestion: Some("~/src/spyc/Cargo.toml"),
        }
    }

    /// The suggestion stands where the typing would go, with the cursor on its
    /// first character and no `_` beside it, and every glyph of it is dimmed.
    #[test]
    fn empty_buffer_draws_the_suggestion_dimmed_under_the_cursor() {
        let theme = Theme::default();
        for mode in [ViMode::Insert, ViMode::Normal] {
            let prompt = jump_prompt(&theme, "", mode);
            let out = render_prompt_to_string(&prompt, 60);
            assert!(
                out.ends_with("jump to: ~/src/spyc/Cargo.toml"),
                "{mode:?}: {out:?}"
            );
            let ghost: String = prompt.wrapped_lines(60)[0]
                .spans
                .iter()
                .filter(|s| s.style.add_modifier.contains(Modifier::DIM))
                .map(|s| s.content.as_ref())
                .collect();
            assert_eq!(ghost, "~/src/spyc/Cargo.toml", "{mode:?}");
        }
    }

    #[test]
    fn typing_hides_the_suggestion() {
        let theme = Theme::default();
        let out = render_prompt_to_string(&jump_prompt(&theme, "~/d", ViMode::Insert), 60);
        assert_eq!(out, "[I] jump to: ~/d_");
    }

    /// The layout reserves rows from the same runs, so a long suggestion grows
    /// the prompt instead of being cut off.
    #[test]
    fn a_long_suggestion_wraps_like_typed_text() {
        let theme = Theme::default();
        let prompt = jump_prompt(&theme, "", ViMode::Insert);
        let typed = PromptLine {
            buffer: "~/src/spyc/Cargo.toml",
            suggestion: None,
            ..jump_prompt(&theme, "", ViMode::Insert)
        };
        assert_eq!(prompt.line_count(12), typed.line_count(12));
        assert!(prompt.line_count(12) > 1);
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
            suggestion: None,
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
            suggestion: None,
        };
        let out = render_prompt_to_string(&prompt, 40);
        insta::assert_snapshot!(out);
    }
}
