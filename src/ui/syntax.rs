//! Syntax highlighting for the pager via `syntect` + `two-face`.
//!
//! Lazy-loads the syntax and theme sets once, then converts syntect's
//! highlighting output into ratatui `Line`s with RGB colors.
//!
//! The base syntax set comes from `two_face::syntax::extra_newlines()`:
//! `bat`'s curated grammar collection (TypeScript, TOML, Dockerfile,
//! Kotlin, and ~100 more) in syntect's own dump format (~0.6 MiB binary
//! increase). It is a *differently-curated* set, not a strict superset of
//! syntect's bundled defaults — bat trims a few promiscuous extension
//! claims, so e.g. `.tmpl`/`.tpl` no longer force-map to HTML and `.s` no
//! longer maps to R. Net of TypeScript + ~100 languages, a clear win.
//!
//! ## User-supplied grammars
//!
//! Additional `.sublime-syntax` files can be layered on top by dropping
//! them into one of these directories (first hit wins for a given scope):
//!
//! - `<config_root>/syntaxes/`, i.e. `$XDG_CONFIG_HOME/spyc/syntaxes/` or
//!   `~/.config/spyc/syntaxes/` — resolved by [`crate::state::config_root`],
//!   the single config-root resolver, so it honors the same precedence (and the
//!   same test override) as `init.lua` and the `lua/` script dir.
//!
//! Files are best-effort; a malformed grammar is logged via
//! `spyc_debug!` and the rest of the directory is still loaded.

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

/// Load the two-face extended syntax set, then layer any user-supplied
/// `.sublime-syntax` files on top.
fn build_syntax_set() -> SyntaxSet {
    let defaults = two_face::syntax::extra_newlines();
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

/// Resolve the user syntaxes directory: `<config_root>/syntaxes/`.
///
/// Goes through [`crate::state::config_root`] — the single config-root resolver
/// — rather than reading `XDG_CONFIG_HOME` / `HOME` again. Resolving it twice
/// meant syntax loading silently ignored the resolver's thread-local test
/// override, so a test that redirected the config root still read the real
/// `~/.config/spyc/syntaxes`. `None` only where the resolver finds no root.
fn user_syntaxes_dir() -> Option<PathBuf> {
    crate::state::config_root().map(|r| r.join("syntaxes"))
}

/// Theme name from syntect's bundled defaults. Dark theme that pairs
/// well with spyc's Tokyo Night palette.
const THEME_NAME: &str = "base16-eighties.dark";

/// Highlight a file's content and return ratatui `Line`s.
/// Returns `None` if the file type isn't recognized by syntect.
pub fn highlight_to_lines(filename: &str, content: &str) -> Option<Vec<Line<'static>>> {
    let ss = &*SYNTAX_SET;

    // Detect syntax from file extension, then by bare filename (so
    // `Makefile` resolves — it has no extension but syntect's
    // bundled `Makefile.sublime-syntax` lists the filename itself
    // in `file_extensions`. Same path will pick up any other
    // bundled or user-supplied syntax keyed on a bare filename).
    // Final fallback: first-line shebang.
    let path = std::path::Path::new(filename);
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let syntax = ss
        .find_syntax_by_extension(ext)
        .or_else(|| {
            path.file_name()
                .and_then(|n| n.to_str())
                .and_then(|name| ss.find_syntax_by_extension(name))
        })
        .or_else(|| {
            content
                .lines()
                .next()
                .and_then(|first| ss.find_syntax_by_first_line(first))
        })?;

    highlight_with(syntax, content)
}

/// Highlight `content` as `lang`, a Markdown fence's info string
/// (` ```rust `, ` ```py `, ` ```Bash `).
///
/// A separate entry point from [`highlight_to_lines`] because a fence tag is a
/// LANGUAGE, not a filename, and syntect looks the two up differently.
/// Synthesizing `snippet.<lang>` and going through the extension table meant
/// every full language name missed: ` ```rust `, ` ```python `, ` ```javascript `
/// and ` ```typescript ` all rendered unhighlighted, while their extension
/// spellings (`rs`, `py`, `js`, `ts`) worked. That is backwards from how people
/// write Markdown — spyc's own docs carry 28 ` ```rust ` blocks and no ` ```rs `.
///
/// `find_syntax_by_token` tries the extension table first and then matches
/// syntax names case-insensitively, so both spellings resolve and ` ```Rust `
/// does too. Only the first whitespace- or comma-separated word is used: mdBook
/// and rustdoc write attributes into the info string (` ```rust,ignore `,
/// ` ```rust no_run `), and those are not part of the language name.
pub fn highlight_lang_to_lines(lang: &str, content: &str) -> Option<Vec<Line<'static>>> {
    let token = lang
        .split(|c: char| c.is_whitespace() || c == ',')
        .find(|t| !t.is_empty())?;
    let ss = &*SYNTAX_SET;
    highlight_with(ss.find_syntax_by_token(token)?, content)
}

/// The shared highlight loop: a resolved syntax + content -> styled lines.
fn highlight_with(
    syntax: &syntect::parsing::SyntaxReference,
    content: &str,
) -> Option<Vec<Line<'static>>> {
    let ss = &*SYNTAX_SET;
    let ts = &*THEME_SET;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The point of routing through `config_root`: the resolver's thread-local
    /// test override now reaches syntax loading. Reading `XDG_CONFIG_HOME`
    /// directly here meant an override was silently ignored, so a test that
    /// redirected the config root still pointed at the real `~/.config/spyc`.
    #[test]
    fn syntaxes_dir_follows_the_config_root_override() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let got = crate::state::with_config_root(tmp.path(), user_syntaxes_dir);
        assert_eq!(
            got,
            Some(tmp.path().join("syntaxes")),
            "syntax loading must honor the shared resolver's override"
        );
    }

    /// And it is the *same* root the rest of the config layer resolves, not a
    /// parallel answer that happens to agree today.
    #[test]
    fn syntaxes_dir_hangs_off_the_shared_config_root() {
        assert_eq!(
            user_syntaxes_dir(),
            crate::state::config_root().map(|r| r.join("syntaxes"))
        );
    }

    #[test]
    fn highlights_makefile_by_bare_filename() {
        let content = "all:\n\techo hello\n";
        let lines = highlight_to_lines("Makefile", content);
        assert!(lines.is_some(), "Makefile should resolve a syntax");
    }

    #[test]
    fn highlights_by_extension_still_works() {
        let lines = highlight_to_lines("main.rs", "fn main() {}\n");
        assert!(lines.is_some());
    }

    #[test]
    fn unknown_filename_returns_none() {
        let lines = highlight_to_lines("nofile-xyz-zzz", "plain bytes\n");
        assert!(lines.is_none());
    }

    #[test]
    fn highlights_typescript() {
        let lines = highlight_to_lines("component.ts", "const x: number = 42;\n");
        assert!(lines.is_some(), ".ts should resolve TypeScript syntax");
    }

    #[test]
    fn highlights_tsx() {
        let lines = highlight_to_lines("app.tsx", "const el = <div />;\n");
        assert!(
            lines.is_some(),
            ".tsx should resolve TypescriptReact syntax"
        );
    }
}
