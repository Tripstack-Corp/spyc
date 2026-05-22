//! Syntax highlighting for the pager via `syntect`.
//!
//! Lazy-loads the default syntax and theme sets once, then converts
//! syntect's highlighting output into ratatui `Line`s with RGB colors.
//!
//! ## User-supplied grammars
//!
//! syntect's `default-fancy` bundle covers ~90 languages but has
//! notable gaps (TypeScript, Zig, …). Instead of shipping every
//! grammar we ever might want, spyc merges `.sublime-syntax` files
//! the user drops into one of these directories (first hit wins for
//! a given scope):
//!
//! - `$XDG_CONFIG_HOME/spyc/syntaxes/` (XDG default)
//! - `~/.config/spyc/syntaxes/` (fallback when `XDG_CONFIG_HOME` is unset)
//!
//! Files are best-effort; a malformed grammar is logged via
//! `spyc_debug!` and the rest of the directory is still loaded.
//! Permissively-licensed grammars are widely available — Sublime's
//! TypeScript package, `bat`'s assets, the `syntect/syntaxes` repo.

use std::path::PathBuf;
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

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(build_syntax_set);
static THEME_SET: LazyLock<ThemeSet> = LazyLock::new(ThemeSet::load_defaults);

/// Load syntect's bundled defaults, then layer any user-supplied
/// `.sublime-syntax` files on top.
fn build_syntax_set() -> SyntaxSet {
    let defaults = SyntaxSet::load_defaults_newlines();
    let Some(dir) = user_syntaxes_dir() else {
        return defaults;
    };
    if !dir.is_dir() {
        return defaults;
    }
    let mut builder = defaults.into_builder();
    match builder.add_from_folder(&dir, true) {
        Ok(()) => crate::spyc_debug!("loaded user syntaxes from {}", dir.display()),
        Err(e) => crate::spyc_debug!(
            "syntax: failed to load user syntaxes from {}: {e}",
            dir.display()
        ),
    }
    builder.build()
}

/// Resolve the user syntaxes directory. Honors `XDG_CONFIG_HOME`,
/// falls back to `~/.config/spyc/syntaxes/`. Returns `None` only on
/// exotic systems where neither `XDG_CONFIG_HOME` nor `HOME` is set.
fn user_syntaxes_dir() -> Option<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(xdg).join("spyc").join("syntaxes"));
    }
    std::env::var_os("HOME").map(|h| {
        PathBuf::from(h)
            .join(".config")
            .join("spyc")
            .join("syntaxes")
    })
}

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
