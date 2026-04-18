//! Syntax highlighting for the pager via `syntect`.
//!
//! Lazy-loads the default syntax and theme sets once, then converts
//! syntect's highlighting output into ratatui `Line`s with RGB colors.

use std::sync::LazyLock;

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use syntect::{
    easy::HighlightLines,
    highlighting::{FontStyle, ThemeSet},
    parsing::SyntaxSet,
    util::LinesWithEndings,
};

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);
static THEME_SET: LazyLock<ThemeSet> = LazyLock::new(ThemeSet::load_defaults);

/// Theme name from syntect's bundled defaults. Dark theme that pairs
/// well with spyc's Tokyo Night palette.
const THEME_NAME: &str = "base16-eighties.dark";

/// Highlight a file's content and return ratatui `Line`s.
/// Returns `None` if the file type isn't recognized by syntect.
pub fn highlight_to_lines(filename: &str, content: &str) -> Option<Vec<Line<'static>>> {
    let ss = &*SYNTAX_SET;
    let ts = &*THEME_SET;

    // Detect syntax from file extension, then try first line.
    let ext = std::path::Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    let syntax = ss.find_syntax_by_extension(ext).or_else(|| {
        content
            .lines()
            .next()
            .and_then(|first| ss.find_syntax_by_first_line(first))
    })?;

    let theme = ts.themes.get(THEME_NAME)?;
    let mut highlighter = HighlightLines::new(syntax, theme);

    let mut lines = Vec::new();
    for line in LinesWithEndings::from(content) {
        let ranges = highlighter.highlight_line(line, ss).ok()?;
        let spans: Vec<Span<'static>> = ranges
            .into_iter()
            .map(|(style, text)| {
                let fg = Color::Rgb(style.foreground.r, style.foreground.g, style.foreground.b);
                let mut modifier = Modifier::empty();
                if style.font_style.contains(FontStyle::BOLD) {
                    modifier |= Modifier::BOLD;
                }
                if style.font_style.contains(FontStyle::ITALIC) {
                    modifier |= Modifier::ITALIC;
                }
                if style.font_style.contains(FontStyle::UNDERLINE) {
                    modifier |= Modifier::UNDERLINED;
                }
                Span::styled(
                    text.trim_end_matches('\n').to_string(),
                    Style::default().fg(fg).add_modifier(modifier),
                )
            })
            .collect();
        lines.push(Line::from(spans));
    }
    Some(lines)
}
