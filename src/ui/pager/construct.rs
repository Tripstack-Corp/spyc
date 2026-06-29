//! `PagerView` construction + content I/O: builders, source-text extraction,
//! save/temp-write, clipboard yanks, picker movement, the full-width /
//! line-number toggles, and `build_pager_help`. Split from `pager` verbatim.

use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::ui::theme::Theme;

use super::{Mount, PagerView, Search};

use super::layout::line_plain_text;
use super::{PAGER_HELP_TITLE, SCROLLBACK_HELP_TITLE};

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
            tab_width: 4,
            saveable: false,
            full_width: false,
            fit_to_content: false,
            no_history: false,
            task_id: None,
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
            mermaid_blocks: Vec::new(),
        }
    }

    /// Build a pager from plain strings. Each string becomes one
    /// unstyled line. Delegates to [`Self::new_styled`] after converting
    /// the strings to unstyled `Line`s — the field defaults are identical.
    pub fn new_plain(title: impl Into<String>, lines: Vec<String>) -> Self {
        Self::new_styled(title, lines.into_iter().map(Line::from).collect())
    }

    /// The first ` ```mermaid ` block whose rendered-line range overlaps the
    /// current viewport (`[scroll, scroll + last_viewport_h)`), for the
    /// `o`-to-open hook. `None` when no diagram is on screen (or this isn't a
    /// markdown view). Uses the viewport height cached by the last render.
    pub fn visible_mermaid_block(&self) -> Option<&crate::ui::markdown::MermaidBlock> {
        // The ranges index the *rendered* markdown lines; ignore them while the
        // source view is showing (the `m` toggle), where line numbers differ.
        if !self.markdown_rendered {
            return None;
        }
        let top = self.scroll;
        let bottom = top + (self.last_viewport_h.get() as usize).max(1);
        self.mermaid_blocks
            .iter()
            .find(|b| b.line_range.start < bottom && b.line_range.end > top)
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

    /// The source content + trailing newline, for `Effect::SavePagerOutput`.
    /// The executor owns the timestamped filename, the cwd, and the write, so
    /// the file IO stays out of the pure pager type and off the input thread's
    /// inline path.
    pub fn save_content(&self) -> String {
        self.source_text() + "\n"
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
        let Some(cur) = self.picker_cursor else {
            return;
        };
        let n = self.lines.len();
        if n == 0 {
            return;
        }
        let new = (cur as isize + delta).clamp(0, n as isize - 1) as usize;
        self.picker_cursor = Some(new);
        self.scroll_to_keep_visible(new, viewport_height);
    }

    pub const fn toggle_full_width(&mut self) {
        self.full_width = !self.full_width;
    }

    pub const fn toggle_line_numbers(&mut self) {
        self.show_line_numbers = !self.show_line_numbers;
    }

    // ---- Visual line mode ------------------------------------------------

    /// Extract the *source* content for clipboard yank (`y` key). For Markdown
    /// views this is always the underlying markdown text (POLA for "paste into
    /// chat"). `include_title` prepends a `# {title}` header when true.
    /// Returns the string without touching the clipboard — callers route through
    /// `Effect::CopyToClipboard` so the copy stays in `run_effects`.
    pub fn source_yank_text(&self, include_title: bool) -> String {
        self.with_title_header(self.source_text(), include_title)
    }

    /// Extract the *visible* content for clipboard yank (`Y` key). For Markdown
    /// in rendered mode this is the styled-but-plain-text rendering; in source
    /// mode it equals `source_yank_text`. Returns the string without copying.
    pub fn visible_yank_text(&self, include_title: bool) -> String {
        let text = self
            .lines
            .iter()
            .map(line_plain_text)
            .collect::<Vec<_>>()
            .join("\n");
        self.with_title_header(text, include_title)
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
                ("/", "search forward (from current position)"),
                ("?", "search backward (from current position)"),
                ("n", "repeat search (same direction)"),
                ("N", "repeat search (opposite direction)"),
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
        (
            "Exit",
            &[("q  Q  Esc", "close pager"), ("H  F1", "this help")],
        ),
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

/// The dedicated help for the `^a v` scrollback / transcript view. Shown in the
/// bottom `scroll_pager` slot (over the stashed scrollback); `H` toggles it with
/// [`build_pager_help`] — separate-but-linked — and `Esc`/`q` restores the
/// scrollback. Leads with a short blurb on the transcript engine, then the
/// scrollback-specific keys.
pub fn build_scrollback_help(theme: &Theme) -> PagerView {
    use crate::ui::display_pad_right;

    let key_style = Style::default().fg(theme.pick).add_modifier(Modifier::BOLD);
    let desc_style = Style::default().fg(theme.status_path);
    let section_style = Style::default()
        .fg(theme.status_user)
        .add_modifier(Modifier::BOLD);
    let blurb_style = Style::default().fg(theme.status_suffix);

    // Intro blurb — the "extended support" the generic pager help doesn't cover.
    let blurb: &[&str] = &[
        "Scrollback shows the pane's history in the in-app pager.",
        "For an agent tab (Claude / codex / agy) it engages the agent's",
        "on-disk transcript JSONL instead — real text, searchable, not a",
        "terminal-grid snapshot. Line numbers are on by default here so it",
        "reads as scrolled-back, not live. The pty keeps running off-screen;",
        "Esc snaps back to live.",
    ];

    let sections: &[(&str, &[(&str, &str)])] = &[
        (
            "Transcript",
            &[
                ("t", "show / hide agent tool-use & tool-result lines"),
                ("r", "reload (a full-screen agent keeps appending)"),
            ],
        ),
        (
            "Jump / search",
            &[
                ("gf", "jump to a file path referenced in the output"),
                ("gF", "jump + open the pager at file:line"),
                (":N", "jump to line N"),
                ("/  ?  n  N", "search forward / backward, repeat"),
            ],
        ),
        (
            "Select / yank",
            &[
                (
                    "V",
                    "visual line mode — double-tap to arm (place, then anchor)",
                ),
                ("^v", "visual block mode"),
                ("y  Y", "yank source / visible to clipboard"),
            ],
        ),
        (
            "Display",
            &[
                ("l", "toggle line numbers"),
                ("w", "toggle whitespace markers (·, ↲, $, → tab)"),
                ("W", "toggle line wrap"),
            ],
        ),
        (
            "Navigation",
            &[
                ("j  k", "scroll a line"),
                ("^D  ^U", "half page"),
                ("^F  ^B  Space", "page"),
                ("g  G", "top / bottom"),
            ],
        ),
        (
            "Exit",
            &[
                ("q  Q  Esc", "back to the live pane"),
                ("H", "toggle the full pager-keys help"),
            ],
        ),
    ];

    let mut lines: Vec<Line<'static>> = Vec::new();
    for b in blurb {
        lines.push(Line::from(Span::styled((*b).to_string(), blurb_style)));
    }
    for (title, rows) in sections {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(*title, section_style)));
        for (keys, desc) in *rows {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(display_pad_right(keys, 16), key_style),
                Span::raw("  "),
                Span::styled((*desc).to_string(), desc_style),
            ]));
        }
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  press H for all pager keys · Esc to return".to_string(),
        blurb_style,
    )));

    let mut view = PagerView::new_styled(SCROLLBACK_HELP_TITLE, lines);
    view.show_line_numbers = false;
    view.no_history = true;
    view.wrap = false;
    // Renders in the bottom pane region where the scrollback was.
    view.mount = Mount::LowerPane;
    view
}
