//! In-app scrollable pager overlay with incremental search.
//!
//! Used for spyc-internal content where shelling out to `less` would be
//! overkill — long listings, file contents, captured `!` output, version
//! info. Arbitrary terminal-output viewing lives here too, with ANSI
//! colors preserved via `ansi-to-tui`.

use ansi_to_tui::IntoText;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::ui::theme::Theme;

/// Search mode inside the pager.
enum Search {
    /// No search in progress; j/k scroll normally.
    Off,
    /// The user is typing a query (triggered by `/`).
    Typing(String),
    /// A query has been committed. `matches` holds line indices that
    /// contain the query; `cursor` is an index into `matches`.
    Active {
        query: String,
        matches: Vec<usize>,
        cursor: usize,
    },
}

#[allow(clippy::struct_excessive_bools)]
pub struct PagerView {
    pub title: String,
    /// Pre-styled lines. ANSI escapes in source are already converted to
    /// styled spans; plain text becomes a single unstyled span per line.
    pub lines: Vec<Line<'static>>,
    /// Top line currently shown in the viewport (0-indexed).
    pub scroll: u16,
    search: Search,
    /// When true, show line numbers in the gutter.
    pub show_line_numbers: bool,
    /// When true, show whitespace markers (·, ↲, etc.).
    pub show_whitespace: bool,
    /// When true, `s` saves the content to a file. Only for command
    /// output — not for files the user opened with `d`/Enter (they
    /// already exist on disk).
    pub saveable: bool,
    /// When true, the pager fills the entire terminal instead of the
    /// centered 90×92% box. Toggled with `f`.
    pub full_width: bool,
    /// When true (and not `full_width`), shrink the pager box to fit its
    /// content -- height grows with line count, width grows with the
    /// widest line, both clamped to the centered 90×92% bound and floored
    /// at a usable minimum. For short summaries (single-file long
    /// listing, version info) so a 5-line block doesn't sit inside a
    /// nearly-full-screen frame.
    pub fit_to_content: bool,
    /// When true, this view should NOT be saved to the buffer-history
    /// stack on close. Used for the help overlay so accidentally hitting
    /// `[b` doesn't surface a stale help screen and confuse "what page
    /// am I on?".
    pub no_history: bool,
    /// When set, this pager view is a "task viewer" -- a peek into the
    /// buffered output of a backgrounded shell task. `[t`/`]t` cycles
    /// among task viewers; the main loop refreshes the contents from
    /// the task buffer while the task is running.
    pub task_id: Option<u32>,
    /// When set, this pager view is a streaming `:grep` result. The
    /// main tick loop drains pending matches into `lines` while the
    /// id matches the active grep session; when the pager is replaced
    /// or its id is cleared, the worker is dropped and the view
    /// freezes at whatever was collected.
    pub grep_id: Option<u32>,
    /// Number of columns for multi-column layout (1 = normal single column).
    /// Lines flow top-to-bottom within each column, then left-to-right.
    pub columns: u8,
    /// When set, the pager is showing a file on disk. `v` opens this path
    /// directly in `$EDITOR`. When `None`, content is a buffer (command
    /// output, help, etc.) and `v` uses a temp file.
    pub source_path: Option<std::path::PathBuf>,
    /// When set, the pager acts as a picker: j/k move a highlighted cursor
    /// instead of scrolling, and Enter selects. The value is the 0-based
    /// line index of the highlighted row.
    pub picker_cursor: Option<usize>,
    /// When set, render a vi cursor on the picker line at this column offset.
    /// Used by the history editor to show the editing cursor inline.
    pub picker_edit_cursor: Option<(usize, crate::ui::line_edit::Mode)>,
    /// When true, suppress [EOF] and tilde markers (content is still arriving).
    pub streaming: bool,
    /// Lower bound for the line-number gutter width. Streaming views
    /// use this to lock the gutter at the expected final size so it
    /// doesn't widen mid-scan as `ilog10(lines.len())` grows -- which
    /// would otherwise shift visible content right by one column each
    /// time the result count crossed a power of 10. `None` means
    /// "size the gutter to current line count" (the default).
    pub line_count_hint: Option<usize>,
    /// When set, show `:` + digits at the bottom of the pager (inline jump prompt).
    pub jump_buf: Option<String>,
    /// Temporary message shown in the title bar (e.g. "yanked to clipboard").
    /// Cleared on the next keypress.
    pub flash: Option<String>,
}

impl PagerView {
    /// Build a pager from plain strings. Each string becomes one
    /// unstyled line.
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
            columns: 1,
            source_path: None,
            picker_cursor: None,
            picker_edit_cursor: None,
            streaming: false,
            line_count_hint: None,
            jump_buf: None,
            flash: None,
        }
    }

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
            columns: 1,
            source_path: None,
            picker_cursor: None,
            picker_edit_cursor: None,
            streaming: false,
            line_count_hint: None,
            jump_buf: None,
            flash: None,
        }
    }

    /// Build a pager from raw bytes that may contain ANSI escape
    /// sequences. Colors, bold, underline etc. are preserved.
    /// Saveable by default (command output).
    pub fn new_ansi(title: impl Into<String>, bytes: &[u8]) -> Self {
        let text = bytes.into_text().unwrap_or_default();
        Self {
            title: title.into(),
            lines: text.lines,
            scroll: 0,
            search: Search::Off,
            show_line_numbers: true,
            show_whitespace: false,
            saveable: true,
            full_width: false,
            fit_to_content: false,
            no_history: false,
            task_id: None,
            grep_id: None,
            columns: 1,
            source_path: None,
            picker_cursor: None,
            picker_edit_cursor: None,
            streaming: false,
            line_count_hint: None,
            jump_buf: None,
            flash: None,
        }
    }

    /// All lines as plain text (ANSI stripped), joined with newlines.
    fn plain_text(&self) -> String {
        self.lines
            .iter()
            .map(line_plain_text)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Save the plain-text content to a timestamped file in the current
    /// directory. Returns the path on success.
    pub fn save_to_file(&self) -> std::io::Result<std::path::PathBuf> {
        let now = crate::sysinfo::format_now().replace([' ', ':'], "_");
        let stamp = now.trim_end_matches("_UTC");
        let filename = format!("spyc_output_{stamp}.txt");
        let path = std::env::current_dir()?.join(&filename);
        std::fs::write(&path, self.plain_text() + "\n")?;
        Ok(path)
    }

    /// Write the plain-text content to a temp file for editing.
    pub fn write_to_temp(&self) -> std::io::Result<std::path::PathBuf> {
        let dir = std::env::temp_dir();
        let filename = format!("spyc_pager_{}.txt", std::process::id());
        let path = dir.join(filename);
        std::fs::write(&path, self.plain_text() + "\n")?;
        Ok(path)
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

    /// Yank the full pager content to the system clipboard via pbcopy.
    pub fn yank_to_clipboard(&self) -> std::io::Result<()> {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let text = self.plain_text();
        let mut child = Command::new("pbcopy").stdin(Stdio::piped()).spawn()?;
        if let Some(stdin) = child.stdin.as_mut() {
            stdin.write_all(text.as_bytes())?;
        }
        child.wait()?;
        Ok(())
    }

    pub const fn toggle_line_numbers(&mut self) {
        self.show_line_numbers = !self.show_line_numbers;
    }

    pub const fn toggle_whitespace(&mut self) {
        self.show_whitespace = !self.show_whitespace;
    }

    pub fn line_count(&self) -> u16 {
        u16::try_from(self.lines.len()).unwrap_or(u16::MAX)
    }

    /// Lines visible per "page" — viewport_height * columns.
    pub fn page_lines(&self, viewport_height: u16) -> u16 {
        viewport_height.saturating_mul(u16::from(self.columns.max(1)))
    }

    /// Maximum useful `scroll` value for the current layout. In multi-col
    /// the static partition means each column has its own chunk; the
    /// visible range is capped by the longest chunk minus viewport_h.
    /// In single-col it's simply `lines - viewport_h`.
    fn scroll_max(&self, viewport_height: u16) -> u16 {
        let ncols = self.columns.max(1) as usize;
        let longest = if ncols <= 1 {
            self.lines.len()
        } else {
            partition_lines_static(&self.lines, ncols)
                .into_iter()
                .map(|(s, e)| e - s)
                .max()
                .unwrap_or(0)
        };
        u16::try_from(longest.saturating_sub(viewport_height.into())).unwrap_or(u16::MAX)
    }

    fn clamp_scroll(&mut self, viewport_height: u16) {
        let max_scroll = self.scroll_max(viewport_height);
        if self.scroll > max_scroll {
            self.scroll = max_scroll;
        }
    }

    pub fn scroll_by(&mut self, delta: i32, viewport_height: u16) {
        let current = i32::from(self.scroll);
        let new = (current + delta).max(0);
        self.scroll = u16::try_from(new).unwrap_or(u16::MAX);
        self.clamp_scroll(viewport_height);
    }

    pub const fn scroll_to_top(&mut self) {
        self.scroll = 0;
    }

    pub fn scroll_to_bottom(&mut self, viewport_height: u16) {
        self.scroll = self.scroll_max(viewport_height);
    }

    /// Position indicator: "Top", "Bot", "All", or "NN%".
    /// Percentage is based on scroll progress through the "effective"
    /// document length — in multi-col that's the longest chunk, not the
    /// total line count, since each column's chunk scrolls independently.
    pub fn position_indicator(&self, viewport_height: u16) -> String {
        let max_scroll = self.scroll_max(viewport_height);
        if max_scroll == 0 {
            return "All".to_string();
        }
        if self.scroll == 0 {
            return "Top".to_string();
        }
        if self.scroll >= max_scroll {
            return "Bot".to_string();
        }
        let pct = (u32::from(self.scroll) * 100) / u32::from(max_scroll);
        format!("{pct}%")
    }

    // ---- Search ----------------------------------------------------------

    /// True when the pager is capturing text input for a `/` search.
    pub const fn is_typing_search(&self) -> bool {
        matches!(self.search, Search::Typing(_))
    }

    pub fn begin_search(&mut self) {
        self.search = Search::Typing(String::new());
    }

    /// Append a char to the search buffer (only meaningful while typing).
    pub fn search_push_char(&mut self, c: char) {
        if let Search::Typing(buf) = &mut self.search {
            buf.push(c);
        }
    }

    pub fn search_backspace(&mut self) {
        if let Search::Typing(buf) = &mut self.search {
            buf.pop();
        }
    }

    /// Cancel an in-progress search and clear any active match state.
    pub fn cancel_search(&mut self) {
        self.search = Search::Off;
    }

    /// Commit the typed query: find matching lines, jump to the first.
    /// No matches → revert to Off and return false so the caller can flash.
    pub fn commit_search(&mut self, viewport_height: u16) -> bool {
        let query = match std::mem::replace(&mut self.search, Search::Off) {
            Search::Typing(q) => q,
            other => {
                self.search = other;
                return true;
            }
        };
        if query.is_empty() {
            return true;
        }
        let needle = query.to_lowercase();
        let matches: Vec<usize> = self
            .lines
            .iter()
            .enumerate()
            .filter(|(_, line)| line_plain_text(line).to_lowercase().contains(&needle))
            .map(|(i, _)| i)
            .collect();
        if matches.is_empty() {
            return false;
        }
        self.scroll_to_match(matches[0], viewport_height);
        self.search = Search::Active {
            query,
            matches,
            cursor: 0,
        };
        true
    }

    /// Move to the next match (wraps). No-op when no search is active.
    pub fn search_next(&mut self, viewport_height: u16) {
        let Search::Active {
            matches, cursor, ..
        } = &mut self.search
        else {
            return;
        };
        if matches.is_empty() {
            return;
        }
        *cursor = (*cursor + 1) % matches.len();
        let line_idx = matches[*cursor];
        self.scroll_to_match(line_idx, viewport_height);
    }

    /// Move to the previous match (wraps).
    pub fn search_prev(&mut self, viewport_height: u16) {
        let Search::Active {
            matches, cursor, ..
        } = &mut self.search
        else {
            return;
        };
        if matches.is_empty() {
            return;
        }
        *cursor = if *cursor == 0 {
            matches.len() - 1
        } else {
            *cursor - 1
        };
        let line_idx = matches[*cursor];
        self.scroll_to_match(line_idx, viewport_height);
    }

    /// Returns the line index of the current search match, if any.
    pub fn current_match_line(&self) -> Option<usize> {
        if let Search::Active {
            matches, cursor, ..
        } = &self.search
        {
            matches.get(*cursor).copied()
        } else {
            None
        }
    }

    /// Scroll the viewport so `line_idx` is roughly a third of the way
    /// down — gives context above and more content below.
    fn scroll_to_match(&mut self, line_idx: usize, viewport_height: u16) {
        let third = i64::from(viewport_height) / 3;
        let target = line_idx as i64 - third;
        let scroll = target.max(0);
        self.scroll = u16::try_from(scroll).unwrap_or(u16::MAX);
        self.clamp_scroll(viewport_height);
    }

    /// For the render layer: is the given line index one of the search
    /// matches? (Returns (is_match, is_current_match).)
    fn match_state(&self, line_idx: usize) -> (bool, bool) {
        match &self.search {
            Search::Active {
                matches, cursor, ..
            } => (
                matches.binary_search(&line_idx).is_ok(),
                matches.get(*cursor) == Some(&line_idx),
            ),
            _ => (false, false),
        }
    }

    /// Current search status for the footer line (e.g. `/foo 3/17`).
    fn status_text(&self) -> Option<String> {
        if let Some(ref buf) = self.jump_buf {
            return Some(format!(":{buf}_"));
        }
        match &self.search {
            Search::Off => None,
            Search::Typing(buf) => Some(format!("/{buf}_")),
            Search::Active {
                query,
                matches,
                cursor,
            } => Some(format!("/{query}  {}/{}", cursor + 1, matches.len())),
        }
    }
}

/// Flatten styled spans back to plain text (for case-insensitive matching).
fn line_plain_text(line: &Line) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

/// Centered pager occupies this percent of the terminal width.
/// Exposed so callers (help content generation) can compute the same
/// column width the pager will actually render at.
const CENTERED_W_PCT: u16 = 90;
/// Gap (in cells) between columns in multi-column mode.
const COL_GAP: u16 = 2;

/// Column width a centered pager will use for `ncols` columns at the
/// given terminal width. Mirrors the render-path math: centered rect
/// → minus 2 for block borders → divided evenly across columns.
#[must_use]
pub const fn centered_col_width(term_w: u16, ncols: u16) -> u16 {
    let body_w = centered_body_width(term_w);
    let ncols = if ncols < 1 { 1 } else { ncols };
    let gaps = COL_GAP * ncols.saturating_sub(1);
    body_w.saturating_sub(gaps) / ncols
}

/// Body width inside the centered pager (useful for deciding how many
/// columns actually fit before calling `centered_col_width`).
#[must_use]
pub const fn centered_body_width(term_w: u16) -> u16 {
    (term_w * CENTERED_W_PCT / 100).saturating_sub(2)
}

pub fn render(frame: &mut Frame, area: Rect, view: &PagerView, theme: &Theme) {
    let inner_area = if view.full_width {
        area
    } else if view.fit_to_content {
        fit_height_rect(area, view)
    } else {
        centered_rect(area, CENTERED_W_PCT, 92)
    };

    frame.render_widget(Clear, inner_area);

    let pos = view.position_indicator(inner_area.height.saturating_sub(2));
    let title = if let Some(ref msg) = view.flash {
        format!("  {} — {}  ", view.title, msg)
    } else {
        format!("  {}   ({} lines)  ", view.title, view.lines.len())
    };
    let title_right = format!("  {pos}  ");
    let block = if view.full_width {
        // No border in full-width mode so terminal text selection is clean.
        Block::default()
    } else {
        Block::default()
            .borders(Borders::ALL)
            .title(Span::styled(
                title,
                Style::default()
                    .fg(theme.prompt_prefix)
                    .add_modifier(Modifier::BOLD),
            ))
            .title_bottom(
                Line::from(Span::styled(
                    title_right,
                    Style::default()
                        .fg(theme.status_suffix)
                        .add_modifier(Modifier::BOLD),
                ))
                .right_aligned(),
            )
    };
    let body_area = block.inner(inner_area);
    frame.render_widget(block, inner_area);

    // Reserve the bottom row for the search/status bar. In multi-column
    // views (help) the row is always reserved so the viewport height stays
    // constant when search is activated — otherwise the column layout
    // would reflow. In single-column views it's only shown when active.
    let ncols = view.columns.max(1) as usize;
    let show_search_row = view.status_text().is_some() || ncols > 1;
    let (content_area, search_area) = if show_search_row {
        (
            Rect {
                x: body_area.x,
                y: body_area.y,
                width: body_area.width,
                height: body_area.height.saturating_sub(1),
            },
            Some(Rect {
                x: body_area.x,
                y: body_area.y + body_area.height.saturating_sub(1),
                width: body_area.width,
                height: 1,
            }),
        )
    } else {
        (body_area, None)
    };

    if ncols > 1 {
        render_multi_column(frame, content_area, view, theme, ncols);
    } else {
        render_single_column(frame, content_area, view, theme);
    }

    if let Some(rect) = search_area {
        if let Some(text) = view.status_text() {
            let style = Style::default()
                .fg(theme.prompt_prefix)
                .add_modifier(Modifier::BOLD);
            frame.render_widget(Paragraph::new(Line::from(Span::styled(text, style))), rect);
        }
    }
}

fn render_single_column(frame: &mut Frame, content_area: Rect, view: &PagerView, theme: &Theme) {
    let viewport_h = content_area.height as usize;
    let start = view.scroll as usize;
    let content_end = view.lines.len();
    let slice_end = (start + viewport_h).min(content_end);

    let total_lines = view.lines.len();
    // Streaming views can grow during render; clamp to the caller's
    // expected upper bound so the gutter doesn't widen mid-scan.
    let gutter_basis = total_lines.max(view.line_count_hint.unwrap_or(0));
    let gutter_w = if view.show_line_numbers {
        gutter_basis.max(1).ilog10() as usize + 2
    } else {
        0
    };
    let ln_style = Style::default()
        .fg(theme.status_suffix)
        .add_modifier(Modifier::DIM);

    let mut display_lines: Vec<Line<'static>> = view.lines[start..slice_end]
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let abs_idx = start + i;
            let mut styled = styled_line_for_render(line, view, abs_idx, theme);
            // Highlight the picker cursor row.
            if view.picker_cursor == Some(abs_idx) {
                if let Some((col, vi_mode)) = view.picker_edit_cursor {
                    // History editor: show editing cursor on this line.
                    let plain: String = styled.spans.iter().map(|s| s.content.as_ref()).collect();
                    let row_style = Style::default().bg(theme.cursor_bg).fg(theme.cursor_fg);
                    let before: String = plain.chars().take(col).collect();
                    let cursor_ch: String = plain
                        .chars()
                        .nth(col)
                        .map_or_else(|| " ".into(), |c| c.to_string());
                    let after: String = plain.chars().skip(col + 1).collect();
                    let cursor_style = if vi_mode == crate::ui::line_edit::Mode::Normal {
                        row_style.add_modifier(Modifier::REVERSED)
                    } else {
                        row_style.add_modifier(Modifier::UNDERLINED)
                    };
                    styled = Line::from(vec![
                        Span::styled(before, row_style),
                        Span::styled(cursor_ch, cursor_style),
                        Span::styled(after, row_style),
                    ]);
                } else {
                    styled = Line::from(
                        styled
                            .spans
                            .into_iter()
                            .map(|s| {
                                Span::styled(
                                    s.content,
                                    s.style
                                        .bg(theme.cursor_bg)
                                        .fg(theme.cursor_fg)
                                        .add_modifier(Modifier::BOLD),
                                )
                            })
                            .collect::<Vec<_>>(),
                    );
                }
            }
            let styled = if view.show_whitespace {
                apply_whitespace_markers(&styled, theme)
            } else {
                styled
            };
            if gutter_w > 0 {
                let num = format!("{:>width$} ", abs_idx + 1, width = gutter_w - 1);
                let mut spans = vec![Span::styled(num, ln_style)];
                spans.extend(styled.spans);
                Line::from(spans)
            } else {
                styled
            }
        })
        .collect();

    let eof_style = Style::default()
        .fg(theme.status_suffix)
        .add_modifier(Modifier::DIM);
    if slice_end >= content_end && display_lines.len() < viewport_h && !view.streaming {
        display_lines.push(Line::from(Span::styled("[EOF]", eof_style)));
        while display_lines.len() < viewport_h {
            display_lines.push(Line::from(Span::styled("~", eof_style)));
        }
    }

    // No wrap: long Lines truncate at the right edge of the body. Wrap
    // was previously on (Wrap { trim: false }) but it interacted poorly
    // with the line-number gutter and the `$` whitespace marker, since
    // ratatui hard-breaks long unbreakable "words" (paths, log lines)
    // mid-character and continuation rows don't carry their own gutter.
    // The result was visible misalignment ("Builde$.cs"-style mid-row
    // line-end markers) on long paths in `git log` output. Behavior now
    // matches the multi-column path and `less -S`. Yank / save / search
    // still operate on `view.lines` so they get the untruncated source.
    let paragraph = Paragraph::new(display_lines);
    frame.render_widget(paragraph, content_area);
}

/// Partition lines into `ncols` chunks at section boundaries (blank lines),
/// targeting roughly equal chunk sizes. The partition is **static** — it
/// does not depend on the current scroll position. Callers apply the
/// user's scroll offset independently within each chunk so the content-
/// to-column mapping stays fixed as the user scrolls.
fn partition_lines_static(lines: &[Line<'static>], ncols: usize) -> Vec<(usize, usize)> {
    let total = lines.len();
    if ncols <= 1 || total == 0 {
        return vec![(0, total)];
    }
    let target = total / ncols;
    let mut chunks = Vec::with_capacity(ncols);
    let mut cursor = 0usize;
    for c in 0..ncols {
        if c + 1 == ncols {
            chunks.push((cursor, total));
            break;
        }
        let ideal = cursor + target;
        // Search within a window ±(target/2) of the ideal break for the
        // closest blank line. Fall back to the ideal cut if no blank
        // exists in the window (rare: implies a single section >target).
        let window_lo = cursor + 1;
        let window_hi = (ideal + target / 2).min(total);
        let mut best = ideal.min(total);
        let mut best_dist = usize::MAX;
        for (i, line_or_end) in (window_lo..=window_hi).map(|idx| (idx, lines.get(idx))) {
            let is_break = line_or_end.is_none_or(is_blank_line);
            if !is_break {
                continue;
            }
            let dist = i.abs_diff(ideal);
            if dist < best_dist {
                best_dist = dist;
                best = i;
            }
        }
        chunks.push((cursor, best));
        cursor = best;
        while cursor < total && is_blank_line(&lines[cursor]) {
            cursor += 1;
        }
    }
    chunks
}

fn is_blank_line(line: &Line<'static>) -> bool {
    line.spans.iter().all(|s| s.content.trim().is_empty())
}

fn render_multi_column(
    frame: &mut Frame,
    content_area: Rect,
    view: &PagerView,
    theme: &Theme,
    ncols: usize,
) {
    let viewport_h = content_area.height as usize;
    let scroll = view.scroll as usize;
    let content_end = view.lines.len();
    // Divide available width evenly (minus gaps between columns).
    let total_gap = COL_GAP * (ncols as u16).saturating_sub(1);
    let col_w = content_area.width.saturating_sub(total_gap) / ncols as u16;

    let eof_style = Style::default()
        .fg(theme.status_suffix)
        .add_modifier(Modifier::DIM);

    // Static partition: content-to-column mapping is fixed (doesn't shift
    // as the user scrolls). Each column then applies the scroll offset
    // independently within its own chunk.
    let chunks = partition_lines_static(&view.lines, ncols);

    for (col, (chunk_start, chunk_end)) in chunks.into_iter().enumerate() {
        let chunk_len = chunk_end - chunk_start;
        let local_scroll = scroll.min(chunk_len);
        let col_start = chunk_start + local_scroll;
        let col_end = (col_start + viewport_h).min(chunk_end);
        let x = content_area.x + (col as u16) * (col_w + COL_GAP);
        let col_rect = Rect {
            x,
            y: content_area.y,
            width: col_w,
            height: content_area.height,
        };

        let mut display_lines: Vec<Line<'static>> = if col_start < chunk_end {
            view.lines[col_start..col_end]
                .iter()
                .enumerate()
                .map(|(i, line)| {
                    let abs_idx = col_start + i;
                    styled_line_for_render(line, view, abs_idx, theme)
                })
                .collect()
        } else {
            Vec::new()
        };

        // Pad with tilde markers when this column has fewer lines than the
        // viewport. Only mark [EOF] on the last column — per-column EOFs
        // would wrongly imply the overall document ended early.
        if display_lines.len() < viewport_h && !view.streaming {
            let is_last_col = col + 1 == ncols;
            if is_last_col && col_start < content_end {
                display_lines.push(Line::from(Span::styled("[EOF]", eof_style)));
            }
            while display_lines.len() < viewport_h {
                display_lines.push(Line::from(Span::styled("~", eof_style)));
            }
        }

        let paragraph = Paragraph::new(display_lines);
        frame.render_widget(paragraph, col_rect);

        // Draw a thin separator between columns.
        if col + 1 < ncols {
            let sep_x = x + col_w;
            let sep_style = Style::default()
                .fg(theme.status_suffix)
                .add_modifier(Modifier::DIM);
            for row in 0..content_area.height {
                let buf = frame.buffer_mut();
                buf.set_string(sep_x, content_area.y + row, "│", sep_style);
            }
        }
    }
}

/// Apply match highlighting to a line when a search is active. The
/// current match gets the cursor-bg color for max pop; other matches get
/// a softer bg tint.
fn styled_line_for_render(
    line: &Line<'static>,
    view: &PagerView,
    idx: usize,
    theme: &Theme,
) -> Line<'static> {
    let (is_match, is_current) = view.match_state(idx);
    if !is_match {
        return line.clone();
    }
    let bg = if is_current {
        theme.cursor_bg
    } else {
        theme.other
    };
    // Apply the background across every span in the line so the whole
    // row reads as "a hit" without clobbering existing fg colors.
    let spans = line
        .spans
        .iter()
        .map(|s| {
            let mut style = s.style;
            style = style.bg(bg);
            if is_current {
                style = style.add_modifier(Modifier::BOLD);
            }
            Span::styled(s.content.clone(), style)
        })
        .collect::<Vec<_>>();
    Line::from(spans)
}

/// Vim-style whitespace substitution. Applied per span to keep existing
/// colors. Visual cues:
///   `→`  tab
///   `·`  trailing space
///   `^M` carriage return
///   `$`  end-of-line (non-empty lines only — blank lines are obviously blank)
fn apply_whitespace_markers(line: &Line<'static>, theme: &Theme) -> Line<'static> {
    // Warm amber-ish so markers are visible against dark backgrounds
    // without fighting the content. Uses the pick color (amber) dimmed.
    let ws_style = Style::default().fg(theme.pick).add_modifier(Modifier::DIM);

    // Check if the whole line is empty / whitespace-only.
    let plain = line_plain_text(line);
    if plain.trim().is_empty() {
        // Don't clutter blank lines with `$`.
        return line.clone();
    }

    let mut out: Vec<Span<'static>> = Vec::new();
    for span in &line.spans {
        let text: &str = &span.content;
        let mut segment = String::new();
        for ch in text.chars() {
            match ch {
                '\t' => {
                    if !segment.is_empty() {
                        out.push(Span::styled(std::mem::take(&mut segment), span.style));
                    }
                    out.push(Span::styled("→", ws_style));
                }
                '\r' => {
                    if !segment.is_empty() {
                        out.push(Span::styled(std::mem::take(&mut segment), span.style));
                    }
                    out.push(Span::styled("^M", ws_style));
                }
                _ => segment.push(ch),
            }
        }
        if !segment.is_empty() {
            out.push(Span::styled(segment, span.style));
        }
    }

    // Replace trailing spaces with `·` for visibility.
    if let Some(last) = out.last_mut() {
        let content: &str = &last.content;
        if content.ends_with(' ') {
            let trimmed = content.trim_end();
            // Trailing spaces are always ASCII, so byte len == display width.
            let trailing_count = content.len() - trimmed.len();
            let style = last.style;
            *last = Span::styled(trimmed.to_string(), style);
            let dots: String = "·".repeat(trailing_count);
            out.push(Span::styled(dots, ws_style));
        }
    }

    out.push(Span::styled("$", ws_style));
    Line::from(out)
}

/// Build a pager help overlay showing all pager-specific keybindings.
pub fn build_pager_help(theme: &super::theme::Theme) -> PagerView {
    use super::display_pad_right;

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
                ("w", "toggle whitespace markers (·, ↲, $)"),
                ("f", "toggle full-width / centered"),
            ],
        ),
        (
            "Actions",
            &[
                ("v", "open in $EDITOR"),
                ("y", "yank to clipboard"),
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

    let mut view = PagerView::new_styled("Pager help", lines);
    view.show_line_numbers = false;
    view
}

const fn centered_rect(area: Rect, percent_w: u16, percent_h: u16) -> Rect {
    let w = area.width * percent_w / 100;
    let h = area.height * percent_h / 100;
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect {
        x,
        y,
        width: w,
        height: h,
    }
}

/// Same x / y / width as the standard centered pager, but shrinks from
/// the bottom: height = lines + borders + status row, capped at the
/// standard 92% height. Top edge stays where the user expects (matching
/// the regular pager origin); short summaries don't sit inside a
/// near-full-screen frame.
fn fit_height_rect(area: Rect, view: &PagerView) -> Rect {
    const MIN_H: u16 = 5;

    let centered = centered_rect(area, CENTERED_W_PCT, 92);
    let need_h = (view.lines.len() as u16).saturating_add(3);
    let height = need_h.clamp(MIN_H.min(centered.height), centered.height);

    Rect {
        x: centered.x,
        y: centered.y,
        width: centered.width,
        height,
    }
}
