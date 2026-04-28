//! Render Markdown source as styled `Line`s for the pager.
//!
//! Walks `pulldown-cmark` events, accumulating spans into the current
//! line and pushing the line on block boundaries. Targets a readable
//! visual — not a perfect typesetter — for READMEs, design docs, and
//! changelogs viewed in the pager. The pager's `m` toggle swaps
//! between this rendering and the syntect-highlighted source.
//!
//! Out of scope for v1: tables (TUI tables look mediocre), embedded
//! HTML (passed through as text), images (alt text only). Footnotes
//! and task lists work because pulldown-cmark's defaults handle
//! them as inline events.
//!
//! Code blocks fall through to syntect when a language hint is given
//! and the language is recognized; unrecognized languages render
//! plain in the code-block style.

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::ui::theme::Theme;

/// Render a Markdown source string into styled lines suitable for
/// the pager's `lines` field.
pub fn render(source: &str, theme: &Theme) -> Vec<Line<'static>> {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_FOOTNOTES);
    let parser = Parser::new_ext(source, opts);
    let mut r = Renderer::new(theme);
    for event in parser {
        r.handle(event);
    }
    r.finish()
}

struct Renderer<'t> {
    theme: &'t Theme,
    lines: Vec<Line<'static>>,
    current: Vec<Span<'static>>,
    /// Active emphasis modifiers applied to subsequent text spans.
    style_mods: Modifier,
    /// Nested-list bullet indent. 0 = top-level.
    list_indent: usize,
    /// True while inside any blockquote (single level — nested
    /// blockquotes render with the same `┃ ` prefix).
    in_blockquote: bool,
    /// When inside a fenced code block, accumulate body here so we
    /// can hand the whole thing to syntect (or render plain) on End.
    code_block: Option<CodeBlockState>,
    /// Last text span saw was a Start(Link); store the destination so
    /// we can append it dimly after the link's text.
    pending_link_url: Option<String>,
}

struct CodeBlockState {
    lang: String,
    body: String,
}

impl<'t> Renderer<'t> {
    const fn new(theme: &'t Theme) -> Self {
        Self {
            theme,
            lines: Vec::new(),
            current: Vec::new(),
            style_mods: Modifier::empty(),
            list_indent: 0,
            in_blockquote: false,
            code_block: None,
            pending_link_url: None,
        }
    }

    fn finish(mut self) -> Vec<Line<'static>> {
        if !self.current.is_empty() {
            self.flush_line();
        }
        self.lines
    }

    fn flush_line(&mut self) {
        let prefix = if self.in_blockquote {
            Some(Span::styled(
                "\u{2503} ".to_string(), // ┃
                Style::default()
                    .fg(self.theme.status_suffix)
                    .add_modifier(Modifier::DIM),
            ))
        } else {
            None
        };
        let mut spans = std::mem::take(&mut self.current);
        if let Some(p) = prefix {
            spans.insert(0, p);
        }
        self.lines.push(Line::from(spans));
    }

    fn push_blank(&mut self) {
        if !self.current.is_empty() {
            self.flush_line();
        }
        // Avoid stacking empty lines.
        if !self.lines.last().is_some_and(|l| l.spans.is_empty()) {
            self.lines.push(Line::from(Vec::<Span<'static>>::new()));
        }
    }

    fn push_text(&mut self, text: &str, base_style: Style) {
        // Preserve internal newlines as line boundaries (paragraphs
        // with hard line breaks render as separate visual lines).
        let mut first = true;
        for chunk in text.split('\n') {
            if !first {
                self.flush_line();
            }
            first = false;
            if !chunk.is_empty() {
                let style = base_style.add_modifier(self.style_mods);
                self.current.push(Span::styled(chunk.to_string(), style));
            }
        }
    }

    // The `if !self.current.is_empty() { self.flush_line() }` guards
    // below look collapsible to clippy, but they're not -- flush_line
    // unconditionally pushes a Line, so calling it on empty content
    // would emit a stray blank row.
    #[allow(clippy::collapsible_if, clippy::collapsible_match)]
    fn handle(&mut self, event: Event<'_>) {
        // Code block accumulates everything between Start and End.
        if let Some(cb) = self.code_block.as_mut() {
            match event {
                Event::Text(t) | Event::Code(t) => {
                    cb.body.push_str(&t);
                }
                Event::End(TagEnd::CodeBlock) => self.end_code_block(),
                Event::SoftBreak | Event::HardBreak => cb.body.push('\n'),
                _ => {}
            }
            return;
        }

        match event {
            Event::Start(tag) => self.start_tag(tag),
            Event::End(tag) => self.end_tag(tag),
            Event::Text(t) => self.push_text(&t, Style::default()),
            Event::Code(t) => {
                let style = Style::default()
                    .fg(self.theme.status_suffix)
                    .add_modifier(Modifier::DIM);
                self.current.push(Span::styled(format!("`{t}`"), style));
            }
            Event::SoftBreak => {
                // Soft break → space (paragraphs flow).
                self.current.push(Span::raw(" ".to_string()));
            }
            Event::HardBreak => self.flush_line(),
            Event::Rule => {
                if !self.current.is_empty() {
                    self.flush_line();
                }
                let dim = Style::default()
                    .fg(self.theme.status_suffix)
                    .add_modifier(Modifier::DIM);
                self.lines
                    .push(Line::from(Span::styled("\u{2500}".repeat(40), dim)));
                self.lines.push(Line::from(Vec::<Span<'static>>::new()));
            }
            Event::TaskListMarker(checked) => {
                let glyph = if checked { "[x] " } else { "[ ] " };
                self.current.push(Span::styled(
                    glyph.to_string(),
                    Style::default().fg(self.theme.pick),
                ));
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                // Render raw HTML as dim text — not a goal to interpret it.
                let style = Style::default().add_modifier(Modifier::DIM);
                self.push_text(&html, style);
            }
            Event::FootnoteReference(name) => {
                self.current.push(Span::styled(
                    format!("[^{name}]"),
                    Style::default().fg(self.theme.status_suffix),
                ));
            }
            _ => {}
        }
    }

    #[allow(clippy::collapsible_if, clippy::collapsible_match)]
    fn start_tag(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => {
                if !self.current.is_empty() {
                    self.flush_line();
                }
            }
            Tag::Heading { level, .. } => {
                if !self.current.is_empty() || !self.lines.is_empty() {
                    self.push_blank();
                }
                let prefix = "#".repeat(heading_depth(level));
                let style = Style::default()
                    .fg(self.theme.status_user)
                    .add_modifier(Modifier::BOLD);
                self.current
                    .push(Span::styled(format!("{prefix} "), style));
                // Subsequent text in the heading inherits BOLD via style_mods.
                self.style_mods |= Modifier::BOLD;
            }
            Tag::BlockQuote(_) => {
                self.in_blockquote = true;
                if !self.current.is_empty() {
                    self.flush_line();
                }
            }
            Tag::CodeBlock(kind) => {
                if !self.current.is_empty() {
                    self.flush_line();
                }
                let lang = match kind {
                    CodeBlockKind::Fenced(s) => s.into_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                self.code_block = Some(CodeBlockState {
                    lang,
                    body: String::new(),
                });
            }
            Tag::List(_) => {
                if !self.current.is_empty() {
                    self.flush_line();
                }
                self.list_indent += 1;
            }
            Tag::Item => {
                if !self.current.is_empty() {
                    self.flush_line();
                }
                let indent = "  ".repeat(self.list_indent.saturating_sub(1));
                self.current.push(Span::styled(
                    format!("{indent}\u{2022} "),
                    Style::default().fg(self.theme.status_path),
                ));
            }
            Tag::Emphasis => {
                self.style_mods |= Modifier::ITALIC;
            }
            Tag::Strong => {
                self.style_mods |= Modifier::BOLD;
            }
            Tag::Strikethrough => {
                self.style_mods |= Modifier::CROSSED_OUT;
            }
            Tag::Link { dest_url, .. } => {
                self.pending_link_url = Some(dest_url.into_string());
                self.style_mods |= Modifier::UNDERLINED;
            }
            Tag::Image { dest_url, .. } => {
                // Render as `[image: url]` placeholder. Alt text
                // (if any) flows in as Text events between Start
                // and End; we let those render under italic.
                let style = Style::default()
                    .fg(self.theme.status_suffix)
                    .add_modifier(Modifier::DIM);
                self.current
                    .push(Span::styled(format!("[image: {dest_url}] "), style));
            }
            Tag::FootnoteDefinition(name) => {
                if !self.current.is_empty() {
                    self.flush_line();
                }
                self.current.push(Span::styled(
                    format!("[^{name}]: "),
                    Style::default().fg(self.theme.status_suffix),
                ));
            }
            // Tables fall through unstyled — rendering them as
            // ASCII-aligned content is out of scope for v1.
            _ => {}
        }
    }

    #[allow(clippy::collapsible_if, clippy::collapsible_match)]
    fn end_tag(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                if !self.current.is_empty() {
                    self.flush_line();
                }
                self.lines.push(Line::from(Vec::<Span<'static>>::new()));
            }
            TagEnd::Heading(_) => {
                self.style_mods.remove(Modifier::BOLD);
                if !self.current.is_empty() {
                    self.flush_line();
                }
                self.lines.push(Line::from(Vec::<Span<'static>>::new()));
            }
            TagEnd::BlockQuote(_) => {
                if !self.current.is_empty() {
                    self.flush_line();
                }
                self.in_blockquote = false;
                self.lines.push(Line::from(Vec::<Span<'static>>::new()));
            }
            TagEnd::List(_) => {
                self.list_indent = self.list_indent.saturating_sub(1);
                if self.list_indent == 0 {
                    self.lines.push(Line::from(Vec::<Span<'static>>::new()));
                }
            }
            TagEnd::Item => {
                if !self.current.is_empty() {
                    self.flush_line();
                }
            }
            TagEnd::Emphasis => self.style_mods.remove(Modifier::ITALIC),
            TagEnd::Strong => self.style_mods.remove(Modifier::BOLD),
            TagEnd::Strikethrough => self.style_mods.remove(Modifier::CROSSED_OUT),
            TagEnd::Link => {
                self.style_mods.remove(Modifier::UNDERLINED);
                if let Some(url) = self.pending_link_url.take() {
                    let dim = Style::default()
                        .fg(self.theme.status_suffix)
                        .add_modifier(Modifier::DIM);
                    self.current
                        .push(Span::styled(format!(" \u{2192} {url}"), dim));
                }
            }
            _ => {}
        }
    }

    fn end_code_block(&mut self) {
        let Some(state) = self.code_block.take() else {
            return;
        };
        let body = state.body.trim_end_matches('\n');
        // Try syntect highlighting if a language is given; fall
        // back to plain dim text otherwise. We synthesize a fake
        // filename for highlight_to_lines's extension-based lookup
        // when the language tag matches a known extension.
        let highlighted = if state.lang.is_empty() {
            None
        } else {
            let fake_name = format!("snippet.{}", state.lang);
            crate::ui::syntax::highlight_to_lines(&fake_name, body)
        };
        let dim = Style::default()
            .fg(self.theme.status_suffix)
            .add_modifier(Modifier::DIM);
        // Top fence line (dim ───).
        self.lines
            .push(Line::from(Span::styled("\u{2500}".repeat(40), dim)));
        if let Some(lines) = highlighted {
            self.lines.extend(lines);
        } else {
            for raw in body.lines() {
                self.lines.push(Line::from(Span::styled(
                    raw.to_string(),
                    Style::default().fg(self.theme.other),
                )));
            }
        }
        // Bottom fence line.
        self.lines
            .push(Line::from(Span::styled("\u{2500}".repeat(40), dim)));
        self.lines.push(Line::from(Vec::<Span<'static>>::new()));
    }
}

const fn heading_depth(level: HeadingLevel) -> usize {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// True if `path` looks like a Markdown file we should render. The
/// pager checks this when opening a file: if true, both the source
/// and rendered views are pre-computed and `m` toggles between them.
pub fn is_markdown_path(path: &std::path::Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("md" | "markdown")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::Theme;

    fn render_plain(src: &str) -> Vec<String> {
        let theme = Theme::default();
        render(src, &theme)
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn renders_heading_with_hash_prefix() {
        let lines = render_plain("# Title\n");
        assert!(lines.iter().any(|l| l == "# Title"));
    }

    #[test]
    fn renders_paragraph_text_inline() {
        let lines = render_plain("hello world\n");
        assert!(lines.iter().any(|l| l == "hello world"));
    }

    #[test]
    fn renders_bullet_list_with_indent() {
        let lines = render_plain("- alpha\n- beta\n");
        assert!(lines.iter().any(|l| l == "\u{2022} alpha"));
        assert!(lines.iter().any(|l| l == "\u{2022} beta"));
    }

    #[test]
    fn renders_blockquote_with_left_rule() {
        let lines = render_plain("> quoted\n");
        assert!(lines.iter().any(|l| l.starts_with("\u{2503} ") && l.contains("quoted")));
    }

    #[test]
    fn fenced_code_block_emits_fence_lines() {
        let lines = render_plain("```\nfoo\n```\n");
        // Top + bottom fence rows, plus body.
        assert!(lines.iter().filter(|l| l.starts_with("\u{2500}")).count() >= 2);
        assert!(lines.iter().any(|l| l == "foo"));
    }

    #[test]
    fn link_renders_text_with_url_after() {
        let lines = render_plain("see [docs](https://example.com)\n");
        assert!(
            lines
                .iter()
                .any(|l| l.contains("docs") && l.contains("https://example.com"))
        );
    }

    #[test]
    fn is_markdown_path_matches_md_and_markdown() {
        use std::path::Path;
        assert!(is_markdown_path(Path::new("README.md")));
        assert!(is_markdown_path(Path::new("notes.markdown")));
        assert!(!is_markdown_path(Path::new("main.rs")));
    }
}
