//! `PagerView` construction + content I/O: builders, source-text extraction,
//! save/temp-write, clipboard yanks, picker movement, the full-width /
//! line-number toggles, and `build_pager_help`. Split from `pager` verbatim.

use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::ui::theme::Theme;

use super::{Mount, PagerView, Search};

use super::PAGER_HELP_TITLE;
use super::layout::line_plain_text;

impl PagerView {
    /// Build a pager from pre-styled lines (e.g. the help overlay).
    pub fn new_styled(title: impl Into<String>, lines: Vec<Line<'static>>) -> Self {
        Self {
            title: title.into(),
            lines,
            scroll: 0,
            search: Search::Off,
            show_line_numbers: true,
            show_whitespace: false,
            saveable: false,
            full_width: false,
            fit_to_content: false,
            no_history: false,
            task_id: None,
            grep_id: None,
            git_view_id: None,
            stream_id: None,
            columns: 1,
            source_path: None,
            picker_cursor: None,
            picker_edit_cursor: None,
            streaming: false,
            eof_in_content: false,
            wrap: true,
            alt_lines: None,
            markdown_rendered: false,
            saved_alt_scroll: None,
            line_count_hint: None,
            jump_buf: None,
            flash: None,
            last_viewport_h: std::cell::Cell::new(0),
            last_body_w: std::cell::Cell::new(0),
            visual: None,
            placement: None,
            mount: Mount::Overlay,
            pane_scroll: false,
            pending_scroll_to_bottom: std::cell::Cell::new(false),
        }
    }

    /// Build a pager from plain strings. Each string becomes one
    /// unstyled line.
    pub fn new_plain(title: impl Into<String>, lines: Vec<String>) -> Self {
        Self {
            title: title.into(),
            lines: lines.into_iter().map(Line::from).collect(),
            scroll: 0,
            search: Search::Off,
            show_line_numbers: true,
            show_whitespace: false,
            saveable: false,
            full_width: false,
            fit_to_content: false,
            no_history: false,
            task_id: None,
            grep_id: None,
            git_view_id: None,
            stream_id: None,
            columns: 1,
            source_path: None,
            picker_cursor: None,
            picker_edit_cursor: None,
            streaming: false,
            eof_in_content: false,
            wrap: true,
            alt_lines: None,
            markdown_rendered: false,
            saved_alt_scroll: None,
            line_count_hint: None,
            jump_buf: None,
            flash: None,
            last_viewport_h: std::cell::Cell::new(0),
            last_body_w: std::cell::Cell::new(0),
            visual: None,
            placement: None,
            mount: Mount::Overlay,
            pane_scroll: false,
            pending_scroll_to_bottom: std::cell::Cell::new(false),
        }
    }

    /// Plain text of the *source* view. For Markdown buffers this
    /// is always the raw markdown, never the rendered output --
    /// yanking a README to the clipboard or editing it should give
    /// you back the file contents, not the styled rendering. POLA.
    pub(super) fn source_text(&self) -> String {
        let lines = if self.markdown_rendered {
            self.alt_lines.as_deref().unwrap_or(&self.lines)
        } else {
            &self.lines
        };
        lines
            .iter()
            .map(line_plain_text)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Save the source content to a timestamped file in the current
    /// directory. Returns the path on success.
    pub fn save_to_file(&self) -> std::io::Result<std::path::PathBuf> {
        let now = crate::sysinfo::format_now().replace([' ', ':'], "_");
        let stamp = now.trim_end_matches("_UTC");
        let filename = format!("spyc_output_{stamp}.txt");
        let path = std::env::current_dir()?.join(&filename);
        std::fs::write(&path, self.source_text() + "\n")?;
        Ok(path)
    }

    /// Write the source content to a temp file for editing.
    pub fn write_to_temp(&self) -> std::io::Result<std::path::PathBuf> {
        let dir = std::env::temp_dir();
        let filename = format!("spyc_pager_{}.txt", std::process::id());
        let path = dir.join(filename);
        std::fs::write(&path, self.source_text() + "\n")?;
        Ok(path)
    }

    /// Build the optional header + body string handed to the clipboard helper.
    /// Empty title or `include_title == false` ⇒ body returned as-is.
    /// Header format: `# {title}` then one blank line, so a paste
    /// into code/chat reads as a comment line followed by content.
    pub(super) fn with_title_header(&self, body: String, include_title: bool) -> String {
        if !include_title || self.title.is_empty() {
            return body;
        }
        format!("# {}\n\n{body}", self.title)
    }

    /// Move picker cursor up/down (only when `picker_cursor` is set).
    pub fn picker_move(&mut self, delta: isize, viewport_height: u16) {
        let Some(cur) = self.picker_cursor.as_mut() else {
            return;
        };
        let n = self.lines.len();
        if n == 0 {
            return;
        }
        let new = (*cur as isize + delta).clamp(0, n as isize - 1) as usize;
        *cur = new;
        // Auto-scroll to keep the cursor visible.
        let top = self.scroll as usize;
        let bot = top + viewport_height as usize;
        if new < top {
            self.scroll = new as u16;
        } else if new >= bot {
            self.scroll = (new + 1).saturating_sub(viewport_height as usize) as u16;
        }
    }

    pub const fn toggle_full_width(&mut self) {
        self.full_width = !self.full_width;
    }

    pub const fn toggle_line_numbers(&mut self) {
        self.show_line_numbers = !self.show_line_numbers;
    }

    // ---- Visual line mode ------------------------------------------------

    /// Yank the *source* content to the system clipboard. For
    /// Markdown views this is always the underlying markdown text,
    /// even when the pager is showing the rendered view -- POLA for
    /// "I want to paste this README into a chat."
    ///
    /// When `include_title` is true, prepend the pager's title as a
    /// `# {title}` header (with one blank line of separation) so the
    /// pasted content keeps its source context.
    pub fn yank_to_clipboard(&self, include_title: bool) -> std::io::Result<()> {
        crate::clipboard::copy(&self.with_title_header(self.source_text(), include_title))
    }

    /// Yank the *visible* content to the system clipboard. For
    /// Markdown views in rendered mode this gives back the styled-
    /// but-plain-text rendering (headings with `#`, bullets, etc.,
    /// wrapped at 80 cols); in source mode it's identical to
    /// `yank_to_clipboard`. Useful when the rendered version is
    /// what you want to paste.
    pub fn yank_visible_to_clipboard(&self, include_title: bool) -> std::io::Result<()> {
        let text = self
            .lines
            .iter()
            .map(line_plain_text)
            .collect::<Vec<_>>()
            .join("\n");
        crate::clipboard::copy(&self.with_title_header(text, include_title))
    }
}

/// Build a pager help overlay showing all pager-specific keybindings.
pub fn build_pager_help(theme: &Theme) -> PagerView {
    use crate::ui::display_pad_right;

    let key_style = Style::default().fg(theme.pick).add_modifier(Modifier::BOLD);
    let desc_style = Style::default().fg(theme.status_path);
    let section_style = Style::default()
        .fg(theme.status_user)
        .add_modifier(Modifier::BOLD);

    let sections: &[(&str, &[(&str, &str)])] = &[
        (
            "Navigation",
            &[
                ("j  ↓", "scroll down one line"),
                ("k  ↑", "scroll up one line"),
                ("^D", "half page down"),
                ("^U", "half page up"),
                ("^F  Space  PageDn", "page down"),
                ("^B  b  PageUp", "page up"),
                ("g  Home", "top of file"),
                ("G  End", "bottom of file"),
            ],
        ),
        (
            "Search",
            &[
                ("/", "search forward"),
                ("n", "next match"),
                ("N", "previous match"),
                (":N", "jump to line N"),
            ],
        ),
        (
            "Display",
            &[
                ("l", "toggle line numbers"),
                ("|", "toggle diff layout (unified ⇄ side-by-side)"),
                ("w", "toggle whitespace markers (·, ↲, $)"),
                ("W", "toggle line wrap (default on for content pagers)"),
                (
                    "m",
                    "toggle alt view (.md render ↔ source, .json pretty ↔ raw)",
                ),
                ("f", "toggle full-width / centered"),
            ],
        ),
        (
            "Actions",
            &[
                ("v", "open in $EDITOR"),
                ("y", "yank source to clipboard"),
                (
                    "Y",
                    "yank visible to clipboard (rendered markdown / current view)",
                ),
                ("V", "enter visual line mode (j/k extend, y yanks range)"),
                (
                    "^v",
                    "enter visual block mode (j/k/h/l extend, y yanks rect)",
                ),
                (
                    "p",
                    "open in $PAGER (less, full-screen takeover — for huge files)",
                ),
                ("s", "save to file (command output only)"),
            ],
        ),
        (
            "Buffer history",
            &[("[b", "previous buffer"), ("]b", "next buffer")],
        ),
        ("Exit", &[("q  Q  Esc", "close pager"), ("?", "this help")]),
    ];

    let mut lines: Vec<Line<'static>> = Vec::new();
    for (i, (title, rows)) in sections.iter().enumerate() {
        if i > 0 {
            lines.push(Line::from(""));
        }
        lines.push(Line::from(Span::styled(*title, section_style)));
        for (keys, desc) in *rows {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(display_pad_right(keys, 24), key_style),
                Span::raw("  "),
                Span::styled((*desc).to_string(), desc_style),
            ]));
        }
    }

    let mut view = PagerView::new_styled(PAGER_HELP_TITLE, lines);
    view.show_line_numbers = false;
    // Help is a transient overlay -- never push it to buffer history,
    // and never word-wrap (its content is curated to fit).
    view.no_history = true;
    view.wrap = false;
    view
}
