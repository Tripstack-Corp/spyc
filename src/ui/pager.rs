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

/// Where to render the pager. v1.5 introduces this so the same
/// `PagerView` can be a centered popup, embedded into the top-pane
/// slot (replacing the file list — like `;less` does today via the
/// pty overlay path), or embedded into the lower-pane slot
/// (replacing the pty pane — used by `^a-v` scrollback in v1.5).
///
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Mount {
    /// Centered (or full-width / fit-to-content) overlay drawn on
    /// top of the file list and pane. Default; matches every
    /// pre-v1.5 caller.
    #[default]
    Overlay,
    /// Mounted in place of the file list. The bottom pane is still
    /// visible below; focus-switching with `^a-j` / `^a-k` works
    /// the same way it does for the `;cmd` top overlay.
    TopPane,
    /// Mounted in place of the lower pane (the pty). Used by
    /// pane-scrollback view: the pty keeps running, we just stop
    /// drawing it while the pager is up.
    LowerPane,
}

/// What flavor of visual selection is active.
///
/// - `Line` — vi's `V`: whole rows from `min(anchor, cursor)` to
///   `max(anchor, cursor)`. The column fields on
///   `VisualSelection` are ignored.
/// - `Block` — vi's `^v`: a rectangular slice of rows × columns.
///   Both axes (line and column) read off the anchor / cursor
///   pair. Wrap is suppressed during block mode so the rectangle
///   maps cleanly onto on-screen rows; vim does the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisualKind {
    Line,
    Block,
}

/// vi-style visual selection inside the pager. Active when
/// `PagerView::visual` is `Some(_)`. `V` toggles `Line` mode,
/// `^v` toggles `Block`. `j`/`k`/`G`/etc. move the row cursor;
/// in block mode, `h`/`l` extend the column cursor. `y` yanks
/// the selection (lines for `Line`, the rectangular slice for
/// `Block`) and exits.
///
/// Column indices are character indices (chars(), not display
/// columns), so wide-char glyphs (CJK / emoji) count as 1 in
/// the rectangle even though they paint as 2 cells. Vim does the
/// same — full display-width-aware block selection is future
/// work.
#[derive(Debug, Clone, Copy)]
pub struct VisualSelection {
    pub anchor: usize,
    pub cursor: usize,
    /// Anchor column (character index). Meaningful only in
    /// `Block` mode; ignored in `Line` mode.
    pub anchor_col: usize,
    /// Cursor column (character index). Meaningful only in
    /// `Block` mode; ignored in `Line` mode.
    pub cursor_col: usize,
    pub kind: VisualKind,
}

impl VisualSelection {
    /// Inclusive `(low, high)` line indices.
    pub const fn range(&self) -> (usize, usize) {
        if self.anchor <= self.cursor {
            (self.anchor, self.cursor)
        } else {
            (self.cursor, self.anchor)
        }
    }

    /// Inclusive `(low, high)` column indices. Only meaningful
    /// in `Block` mode.
    pub const fn col_range(&self) -> (usize, usize) {
        if self.anchor_col <= self.cursor_col {
            (self.anchor_col, self.cursor_col)
        } else {
            (self.cursor_col, self.anchor_col)
        }
    }
}

/// A pre-visual-block "navigation cursor" used to position the
/// anchor before committing to a visual block selection. The user
/// presses `^v` once to enter this state, moves the cursor with vi
/// motions, then presses `^v` again to commit at the current
/// position (or `V` to commit to Line visual at the current row).
/// `Esc` cancels. Stored on [`PagerView::placement`].
#[derive(Debug, Clone, Copy)]
pub struct PlacementCursor {
    /// 0-indexed line in `lines`.
    pub row: usize,
    /// 0-indexed character column within that line. Column 0 is
    /// the leftmost cell.
    pub col: usize,
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
    /// True when the last line of `lines` is already an EOF marker
    /// (appended by the capture-finish / task-viewer paths). When
    /// set, the render-time `[EOF]` push is suppressed so it doesn't
    /// double up; tilde fill below short content still happens.
    /// Stays false for file pagers and any other source where the
    /// "below content" `[EOF]` is the only marker.
    pub eof_in_content: bool,
    /// When true, long lines wrap at the right edge instead of being
    /// truncated. Continuation rows get a gutter-width indent (no
    /// line number, no whitespace marker) so the wrap doesn't break
    /// alignment. Default true for content pagers; false for picker
    /// UIs (find finder, task viewer) where each source line maps to
    /// a single selectable row.
    pub wrap: bool,
    /// Alternate-view buffer used by the Markdown viewer. When
    /// `Some`, `m` toggles `lines` ↔ `alt_lines` (rendered ↔
    /// source). `markdown_rendered` tracks which side is active so
    /// `yank` / `save` always work on the source.
    pub alt_lines: Option<Vec<Line<'static>>>,
    /// True when `lines` currently holds the rendered Markdown view
    /// and `alt_lines` holds the source. Flipped by `toggle_markdown`.
    pub markdown_rendered: bool,
    /// Scroll position remembered for the *other* side of the
    /// Markdown toggle. `m` saves the just-departed side's scroll
    /// here and restores it next time the user comes back to that
    /// side, so round-tripping rendered ↔ source preserves the
    /// reading position. `None` until the first toggle (we fall
    /// back to a proportional projection of the departing scroll
    /// in that case — better than always resetting to the top).
    saved_alt_scroll: Option<u16>,
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
    /// Last viewport height the renderer drew this view into (in
    /// rows). Cached on `&Cell` so callers using `&PagerView` (e.g.
    /// the streaming-capture tick loop) can auto-scroll-to-bottom
    /// using a real number instead of a hard-coded estimate. 0 until
    /// the first render. Updated from `render_single_column` /
    /// `render_multi_column` each frame.
    pub last_viewport_h: std::cell::Cell<u16>,
    /// Last content width the renderer used for line wrapping
    /// (terminal width minus line-number gutter, etc.). 0 until
    /// the first render. Used by `scroll_max` to size the bottom
    /// of the document in *visual* rows when wrap is on, so long
    /// lines that wrap to multiple rows don't cause the trailing
    /// logical lines to fall off the viewport at "Bot".
    pub last_body_w: std::cell::Cell<u16>,
    /// vi-style visual line selection. `None` outside the mode;
    /// `Some({ anchor, cursor })` while the user is selecting a
    /// range with `V` + `j`/`k`/`G`/etc. `y` yanks the inclusive
    /// range `[min..=max]` and exits. Mutually exclusive with the
    /// search/jump prompts (entering them cancels visual mode).
    pub visual: Option<VisualSelection>,
    /// Pre-visual-block placement cursor. While `Some`, the user is
    /// positioning the anchor before entering visual block mode —
    /// `hjkl` / `w`/`b` / `0`/`$` / `gg`/`G` move the cursor without
    /// yet defining a selection. A second `^v` commits the position
    /// as the anchor and transitions to `visual: Some(Block)`;
    /// `V` commits to Line visual at the cursor row; `Esc` cancels.
    /// Mutually exclusive with `visual`. A reverse-video cell at
    /// `(row, col)` and a footer flash give the visual cue.
    pub placement: Option<PlacementCursor>,
    /// Where this pager renders — overlay / top pane / lower pane.
    /// Default `Mount::Overlay` for every pre-v1.5 caller (set in
    /// each `new_*` constructor); v1.5 phases swap callers over to
    /// `TopPane` (`D`) and `LowerPane` (`^a-v`).
    pub mount: Mount,
    /// True when this pager is the v1.5 pane-scrollback view
    /// (opened via `^a-v`). Drives a couple of pieces of behavior
    /// that differ from the regular pager: Esc tells the underlying
    /// pty pane to exit scroll mode (so the divider's `[SCROLL]`
    /// indicator clears), and the view is never pushed to buffer
    /// history (it's an ephemeral snapshot of pane state, not a
    /// page-of-content the user wants to revisit).
    pub pane_scroll: bool,
    /// Set true by callers that want the pager to land at the
    /// *bottom* of the buffer on its first render — `^a-v` does
    /// this so the recent output is visible immediately, without
    /// computing the scroll value before the actual viewport
    /// height is known. The renderer reads the live viewport,
    /// calls `scroll_to_bottom(viewport)`, and clears the flag
    /// (interior mutability via `Cell`). Subsequent frames don't
    /// re-snap, so user scrolling is preserved.
    pub pending_scroll_to_bottom: std::cell::Cell<bool>,
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
    fn source_text(&self) -> String {
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

    /// Build the optional header + body string handed to the clipboard helper.
    /// Empty title or `include_title == false` ⇒ body returned as-is.
    /// Header format: `# {title}` then one blank line, so a paste
    /// into code/chat reads as a comment line followed by content.
    fn with_title_header(&self, body: String, include_title: bool) -> String {
        if !include_title || self.title.is_empty() {
            return body;
        }
        format!("# {}\n\n{body}", self.title)
    }

    pub const fn toggle_line_numbers(&mut self) {
        self.show_line_numbers = !self.show_line_numbers;
    }

    // ---- Visual line mode ------------------------------------------------

    /// True while the user is selecting a line range with `V`.
    pub const fn is_visual(&self) -> bool {
        self.visual.is_some()
    }

    /// Enter visual line mode, anchoring the selection at the top
    /// visible line. `j`/`k`/`G`/etc. then move the cursor end (with
    /// auto-scroll) and `y` yanks the inclusive range. No-op on an
    /// empty buffer (nothing to select).
    pub fn enter_visual(&mut self) {
        self.enter_visual_with_kind(VisualKind::Line);
    }

    /// Enter (or upgrade to) `Block` visual mode. Anchors at the
    /// top visible line, column 0. If a `Line` selection is
    /// already active, preserve its anchor / cursor and just
    /// flip the kind — vim does the same when you press `^v`
    /// inside an active `V` selection.
    pub fn enter_visual_block(&mut self) {
        if let Some(sel) = self.visual.as_mut() {
            sel.kind = VisualKind::Block;
            // Keep anchor/cursor lines as-is. Columns default to
            // 0/0 if they were never set (Line mode ignored them).
        } else {
            self.enter_visual_with_kind(VisualKind::Block);
        }
    }

    /// Enter pre-visual-block "placement" state. A navigation
    /// cursor lands at (top visible line, col 0); the user can
    /// then move it with vi motions (`hjkl`, `w`/`b`, `0`/`$`,
    /// `gg`/`G`) before committing to a visual block selection
    /// via a second `^v` or to Line visual via `V`. `Esc` cancels.
    pub fn enter_placement(&mut self) {
        if self.lines.is_empty() {
            return;
        }
        // Clear any active selection so placement and visual are
        // mutually exclusive (they share the cursor highlight).
        self.visual = None;
        let row = (self.scroll as usize).min(self.lines.len() - 1);
        self.placement = Some(PlacementCursor { row, col: 0 });
    }

    pub const fn cancel_placement(&mut self) {
        self.placement = None;
    }

    pub const fn is_placement(&self) -> bool {
        self.placement.is_some()
    }

    /// Commit placement → visual block. Anchor lands at the
    /// placement cursor; initial selection is the single cell.
    pub const fn commit_placement_to_visual_block(&mut self) {
        let Some(p) = self.placement.take() else {
            return;
        };
        self.visual = Some(VisualSelection {
            anchor: p.row,
            cursor: p.row,
            anchor_col: p.col,
            cursor_col: p.col,
            kind: VisualKind::Block,
        });
    }

    /// Commit placement → visual line at the placement cursor row.
    /// `V` from placement: skip block setup, start a line-visual
    /// selection from the row the cursor is on.
    pub const fn commit_placement_to_visual_line(&mut self) {
        let Some(p) = self.placement.take() else {
            return;
        };
        self.visual = Some(VisualSelection {
            anchor: p.row,
            cursor: p.row,
            anchor_col: 0,
            cursor_col: 0,
            kind: VisualKind::Line,
        });
    }

    /// Number of characters in `lines[row]` for cursor-clamp math.
    /// Returns 0 if the row is out of range or empty.
    fn placement_row_len(&self, row: usize) -> usize {
        self.lines.get(row).map_or(0, |l| {
            l.spans.iter().map(|s| s.content.chars().count()).sum()
        })
    }

    /// Plain-text content of a line, joined across spans. Used for
    /// vi-style word motions where styling is irrelevant.
    fn placement_row_text(&self, row: usize) -> String {
        self.lines.get(row).map_or_else(String::new, |l| {
            l.spans.iter().map(|s| s.content.as_ref()).collect()
        })
    }

    pub fn placement_move(&mut self, delta_row: isize, delta_col: isize, viewport_height: u16) {
        let n = self.lines.len();
        if n == 0 {
            return;
        }
        let Some(p) = self.placement.as_mut() else {
            return;
        };
        let new_row = (p.row as isize + delta_row).clamp(0, n as isize - 1) as usize;
        p.row = new_row;
        let row_len = self.lines.get(new_row).map_or(0, |l| {
            l.spans
                .iter()
                .map(|s| s.content.chars().count())
                .sum::<usize>()
        });
        let max_col = row_len.saturating_sub(1);
        let new_col = (p.col as isize + delta_col).max(0).min(max_col as isize) as usize;
        p.col = new_col;
        self.scroll_to_keep_visible(new_row, viewport_height);
    }

    pub const fn placement_line_start(&mut self) {
        if let Some(p) = self.placement.as_mut() {
            p.col = 0;
        }
    }

    pub fn placement_line_end(&mut self) {
        let Some(row) = self.placement.as_ref().map(|p| p.row) else {
            return;
        };
        let row_len = self.placement_row_len(row);
        if let Some(p) = self.placement.as_mut() {
            p.col = row_len.saturating_sub(1);
        }
    }

    /// Vi `w`: jump to the next word start on the current row.
    /// Wraps to col 0 of the next non-empty row when no word
    /// remains. Word characters are alphanumeric + `_` (vi's
    /// default `iskeyword`); transitions in/out of that class
    /// count as a word boundary.
    pub fn placement_word_forward(&mut self) {
        let n = self.lines.len();
        if n == 0 {
            return;
        }
        let (row, col) = match self.placement {
            Some(p) => (p.row, p.col),
            None => return,
        };
        let chars: Vec<char> = self.placement_row_text(row).chars().collect();
        let target = next_word_start(&chars, col);
        // Read the next row's text up front if we'll need to wrap,
        // so the second `placement.as_mut()` borrow below doesn't
        // overlap with an immutable borrow of `self`.
        let next_row_text = if target.is_none() && row + 1 < n {
            Some(self.placement_row_text(row + 1))
        } else {
            None
        };
        let Some(p) = self.placement.as_mut() else {
            return;
        };
        if let Some(next_col) = target {
            p.col = next_col;
        } else if let Some(next_text) = next_row_text {
            p.row = row + 1;
            p.col = next_text
                .chars()
                .position(|c| !c.is_whitespace())
                .unwrap_or(0);
        }
    }

    /// Vi `b`: jump to the previous word start on the current row.
    /// Wraps to the last word of the previous row when no word
    /// precedes the cursor on this row.
    pub fn placement_word_backward(&mut self) {
        if self.lines.is_empty() {
            return;
        }
        let (row, col) = match self.placement {
            Some(p) => (p.row, p.col),
            None => return,
        };
        let chars: Vec<char> = self.placement_row_text(row).chars().collect();
        let target = prev_word_start(&chars, col);
        let prev_row_chars: Option<Vec<char>> = if target.is_none() && row > 0 {
            Some(self.placement_row_text(row - 1).chars().collect())
        } else {
            None
        };
        let Some(p) = self.placement.as_mut() else {
            return;
        };
        if let Some(prev_col) = target {
            p.col = prev_col;
        } else if let Some(prev_chars) = prev_row_chars {
            p.row = row - 1;
            p.col = last_word_start(&prev_chars).unwrap_or(0);
        }
    }

    pub fn placement_jump_to(&mut self, row: usize, viewport_height: u16) {
        let n = self.lines.len();
        if n == 0 {
            return;
        }
        let target = row.min(n - 1);
        let row_len = self.placement_row_len(target);
        let Some(p) = self.placement.as_mut() else {
            return;
        };
        p.row = target;
        p.col = p.col.min(row_len.saturating_sub(1));
        self.scroll_to_keep_visible(target, viewport_height);
    }

    fn enter_visual_with_kind(&mut self, kind: VisualKind) {
        if self.lines.is_empty() {
            return;
        }
        let max = self.lines.len() - 1;
        let start = (self.scroll as usize).min(max);
        self.visual = Some(VisualSelection {
            anchor: start,
            cursor: start,
            anchor_col: 0,
            cursor_col: 0,
            kind,
        });
    }

    pub const fn cancel_visual(&mut self) {
        self.visual = None;
    }

    /// Move the visual-mode cursor by `delta` lines (clamped to the
    /// buffer), and auto-scroll the viewport so the cursor stays
    /// visible. No-op when not in visual mode.
    pub fn visual_move(&mut self, delta: isize, viewport_height: u16) {
        let Some(sel) = self.visual.as_mut() else {
            return;
        };
        let n = self.lines.len();
        if n == 0 {
            return;
        }
        let new = (sel.cursor as isize + delta).clamp(0, n as isize - 1) as usize;
        sel.cursor = new;
        self.scroll_to_keep_visible(new, viewport_height);
    }

    /// Jump the visual-mode cursor to a specific line, scrolling as
    /// needed. Used by `g`/`G`/`:N` while a selection is active.
    pub fn visual_jump_to(&mut self, line: usize, viewport_height: u16) {
        let Some(sel) = self.visual.as_mut() else {
            return;
        };
        let n = self.lines.len();
        if n == 0 {
            return;
        }
        let target = line.min(n - 1);
        sel.cursor = target;
        self.scroll_to_keep_visible(target, viewport_height);
    }

    /// Adjust `scroll` so `line` is in the viewport. Visual cursor
    /// helper, factored out so both `visual_move` and `visual_jump_to`
    /// share the same edge logic.
    const fn scroll_to_keep_visible(&mut self, line: usize, viewport_height: u16) {
        let top = self.scroll as usize;
        let vh = viewport_height as usize;
        if vh == 0 {
            return;
        }
        let bot = top + vh;
        if line < top {
            self.scroll = line as u16;
        } else if line >= bot {
            self.scroll = (line + 1).saturating_sub(vh) as u16;
        }
    }

    /// Move the block-mode column cursor by `delta` characters.
    /// Clamped at column 0 on the left; uncapped on the right
    /// (selection past the line end is allowed — vim does the same;
    /// short rows in the rectangle just contribute fewer chars to
    /// the yanked output). No-op outside block mode.
    pub fn visual_col_move(&mut self, delta: isize) {
        let Some(sel) = self.visual.as_mut() else {
            return;
        };
        if sel.kind != VisualKind::Block {
            return;
        }
        let new = (sel.cursor_col as isize + delta).max(0) as usize;
        sel.cursor_col = new;
    }

    /// Yank the visual-mode selection to the clipboard and exit.
    /// `Line` mode joins whole rows with newlines; `Block` mode
    /// joins the rectangular slice (rows × columns), where each
    /// row contributes `line[lo_col..=hi_col]` (character indices,
    /// not display columns) — rows shorter than the range
    /// contribute their available chars and stop. Returns the
    /// number of rows yanked. The header rule is the same as the
    /// full-buffer yank — when partial-range, the source context
    /// is *more* useful, not less.
    pub fn yank_visual_to_clipboard(&mut self, include_title: bool) -> std::io::Result<usize> {
        let Some(sel) = self.visual else {
            return Ok(0);
        };
        let (lo, hi) = sel.range();
        let hi = hi.min(self.lines.len().saturating_sub(1));
        let text = match sel.kind {
            VisualKind::Line => self.lines[lo..=hi]
                .iter()
                .map(line_plain_text)
                .collect::<Vec<_>>()
                .join("\n"),
            VisualKind::Block => {
                let (lo_col, hi_col) = sel.col_range();
                self.lines[lo..=hi]
                    .iter()
                    .map(|line| {
                        let plain = line_plain_text(line);
                        plain
                            .chars()
                            .skip(lo_col)
                            .take(hi_col + 1 - lo_col)
                            .collect::<String>()
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        };
        crate::clipboard::copy(&self.with_title_header(text, include_title))?;
        let count = hi - lo + 1;
        self.visual = None;
        Ok(count)
    }
}

impl PagerView {
    pub const fn toggle_whitespace(&mut self) {
        self.show_whitespace = !self.show_whitespace;
    }

    pub const fn toggle_wrap(&mut self) {
        self.wrap = !self.wrap;
    }

    /// Toggle Markdown rendered ↔ source view. No-op (returns false)
    /// if this view doesn't have an alternate buffer (i.e. wasn't
    /// opened on a `.md`/`.markdown` file).
    ///
    /// **Scroll preservation:** the two views have different line
    /// counts (one rendered line ≠ one source line) so a literal
    /// scroll-index carryover would land arbitrarily. Instead we
    /// remember each side's last scroll position in `saved_alt_scroll`
    /// and restore it when the user comes back. The first time a
    /// view is visited there's no memory yet, so we fall back to a
    /// proportional projection of the departing scroll — close to
    /// the right neighborhood, never worse than the old "always
    /// reset to top" behavior.
    pub fn toggle_markdown(&mut self) -> bool {
        let Some(alt) = self.alt_lines.take() else {
            return false;
        };
        let old_scroll = self.scroll;
        let old_total = self.lines.len();
        let new_total = alt.len();
        let current = std::mem::replace(&mut self.lines, alt);
        self.alt_lines = Some(current);
        self.markdown_rendered = !self.markdown_rendered;

        let restored = self.saved_alt_scroll.take().unwrap_or_else(|| {
            // First visit: project proportionally so a user halfway
            // down the source lands halfway down the rendered view
            // (and vice versa). Bottom of one side maps to bottom of
            // the other.
            if old_total <= 1 || new_total == 0 {
                0
            } else {
                let num = u32::from(old_scroll) * (new_total - 1) as u32;
                let denom = (old_total - 1) as u32;
                u16::try_from(num / denom).unwrap_or(u16::MAX)
            }
        });
        let max_index = u16::try_from(new_total.saturating_sub(1)).unwrap_or(u16::MAX);
        self.scroll = restored.min(max_index);
        self.saved_alt_scroll = Some(old_scroll);
        true
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
    /// In single-col, the obvious answer is `lines - viewport_h`, but
    /// that's wrong when `wrap` is on and lines exceed `body_w` —
    /// each wrapped line consumes multiple visual rows, and stopping
    /// at logical-line distance `viewport_h` from the end leaves the
    /// trailing lines invisible (the renderer fills the viewport with
    /// the wrapped portions of earlier lines and runs out of space
    /// before reaching them). When wrap is on and we have a cached
    /// `body_w` from the most recent render, we walk lines from the
    /// end summing visual rows; max_scroll = the highest logical line
    /// index whose inclusion still fits the viewport.
    pub fn scroll_max(&self, viewport_height: u16) -> u16 {
        let ncols = self.columns.max(1) as usize;
        if ncols > 1 {
            // Multi-col: keep the prior partition-based bound. Wrap
            // is irrelevant here because multi-col is only used for
            // pickers (find finder, task viewer) where wrap is off.
            let longest = partition_lines_static(&self.lines, ncols)
                .into_iter()
                .map(|(s, e)| e - s)
                .max()
                .unwrap_or(0);
            return u16::try_from(longest.saturating_sub(viewport_height.into()))
                .unwrap_or(u16::MAX);
        }
        let logical_max = u16::try_from(self.lines.len().saturating_sub(viewport_height.into()))
            .unwrap_or(u16::MAX);
        let body_w = self.last_body_w.get() as usize;
        if !self.wrap || body_w == 0 || viewport_height == 0 {
            return logical_max;
        }
        // Walk from the end backwards, accumulating visual rows.
        // The first logical line index `i` whose visual-row sum
        // (including itself) reaches `viewport_height` is the
        // greatest scroll value that still keeps the last line
        // visible: starting from `i`, the renderer fills exactly
        // viewport_h rows ending at the document's last line.
        let vh = u32::from(viewport_height);
        let mut acc = 0u32;
        for (i, line) in self.lines.iter().enumerate().rev() {
            let rows = u32::try_from(visual_rows(line, body_w)).unwrap_or(u32::MAX);
            acc = acc.saturating_add(rows);
            if acc >= vh {
                return u16::try_from(i).unwrap_or(u16::MAX);
            }
        }
        // Whole document fits in the viewport — no scrolling needed.
        0
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

    /// Scroll-to-bottom using the viewport height the most recent
    /// render observed (cached in `last_viewport_h`). For
    /// streaming-capture auto-tail: the tick loop appends new
    /// output and wants to keep showing the latest, but it doesn't
    /// have direct access to terminal geometry. Falls back to a
    /// 40-row guess when nothing's been rendered yet (first frame).
    pub fn scroll_to_bottom_auto(&mut self) {
        let h = self.last_viewport_h.get();
        let h = if h == 0 { 40 } else { h };
        self.scroll_to_bottom(h);
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
    ///
    /// In multi-column mode `scroll` is interpreted per-column (each
    /// column applies the same offset within its own chunk), so a
    /// match in column 2+ has to be translated to a chunk-local
    /// offset before being assigned to `self.scroll` — otherwise the
    /// global line index gets clamped to `scroll_max` (= longest
    /// chunk minus viewport_h) and every column pins to the bottom
    /// of its chunk, hiding the match. Symptom: `/show` then `n n n`
    /// in the help pager left the view stuck at the bottom.
    fn scroll_to_match(&mut self, line_idx: usize, viewport_height: u16) {
        let third = i64::from(viewport_height) / 3;
        let ncols = self.columns.max(1) as usize;
        let local_idx = if ncols > 1 {
            partition_lines_static(&self.lines, ncols)
                .into_iter()
                .find(|(s, e)| (*s..*e).contains(&line_idx))
                .map_or(line_idx, |(s, _)| line_idx - s)
        } else {
            line_idx
        };
        let target = local_idx as i64 - third;
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
        if let Some(sel) = self.visual {
            let (lo, hi) = sel.range();
            let count = hi - lo + 1;
            return Some(match sel.kind {
                VisualKind::Line => format!(
                    "-- VISUAL --  L{}-L{}  ({count} line{})",
                    lo + 1,
                    hi + 1,
                    if count == 1 { "" } else { "s" },
                ),
                VisualKind::Block => {
                    let (lo_col, hi_col) = sel.col_range();
                    let cols = hi_col - lo_col + 1;
                    format!(
                        "-- VISUAL BLOCK --  L{}-L{} C{}-C{}  ({count}×{cols})",
                        lo + 1,
                        hi + 1,
                        lo_col + 1,
                        hi_col + 1,
                    )
                }
            });
        }
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
    let inner_area = pager_inner_area(area, view);

    frame.render_widget(Clear, inner_area);

    let pos = view.position_indicator(inner_area.height.saturating_sub(2));
    let title_style = Style::default()
        .fg(theme.prompt_prefix)
        .add_modifier(Modifier::BOLD);
    // Flash is teal + BOLD against the amber title so help notices
    // (e.g. "truncated at 5000 lines · press p for full file in
    // $PAGER") stand out clearly as a separate piece of info, not as
    // an extension of the filename.
    let flash_style = Style::default().fg(theme.take).add_modifier(Modifier::BOLD);
    let title_line: Line<'static> = if let Some(ref msg) = view.flash {
        Line::from(vec![
            Span::styled(format!("  {}  ", view.title), title_style),
            Span::styled(format!(" {msg} "), flash_style),
            Span::styled("  ", title_style),
        ])
    } else {
        Line::from(Span::styled(
            format!("  {}   ({} lines)  ", view.title, view.lines.len()),
            title_style,
        ))
    };
    let title_right = format!("  {pos}  ");
    // Borderless when:
    //   - `full_width` (current behavior — terminal text selection
    //     is clean without the box drawing).
    //   - `Mount::LowerPane` — the pager occupies the bottom-pane
    //     slot, which the pty renders into without a border. Drawing
    //     a border eats two rows of usable content and visually
    //     disrupts the layout the user just had on-screen.
    let borderless = view.full_width || matches!(view.mount, Mount::LowerPane);
    let block = if borderless {
        Block::default()
    } else {
        Block::default()
            .borders(Borders::ALL)
            .title(title_line)
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

    // Cache the viewport height the renderer is using *now* so the
    // tick-loop streaming-capture path can call scroll_to_bottom_auto
    // with a real number (instead of the v1.20-era hardcoded 40 that
    // caused the auto-tail to under-shoot on tall terminals --
    // showing only the top half of the pager filled with content
    // and the rest with `~` until the user manually scrolled).
    view.last_viewport_h.set(content_area.height);

    if ncols > 1 {
        render_multi_column(frame, content_area, view, theme, ncols);
    } else {
        render_single_column(frame, content_area, view, theme);
    }

    if let Some(rect) = search_area
        && let Some(text) = view.status_text()
    {
        let style = Style::default()
            .fg(theme.prompt_prefix)
            .add_modifier(Modifier::BOLD);
        frame.render_widget(Paragraph::new(Line::from(Span::styled(text, style))), rect);
    }
}

fn render_single_column(frame: &mut Frame, content_area: Rect, view: &PagerView, theme: &Theme) {
    let viewport_h = content_area.height as usize;
    let start = view.scroll as usize;
    let content_end = view.lines.len();

    let total_lines = view.lines.len();
    // Streaming views can grow during render; clamp to the caller's
    // expected upper bound so the gutter doesn't widen mid-scan.
    let gutter_basis = total_lines.max(view.line_count_hint.unwrap_or(0));
    let gutter_w = if view.show_line_numbers {
        gutter_basis.max(1).ilog10() as usize + 2
    } else {
        0
    };
    // Line-number gutter: muted but readable. Previously DIM-on-top
    // of status_suffix which left it almost invisible against dark
    // backgrounds; dropped the DIM modifier so the digits actually
    // register.
    let ln_style = Style::default().fg(theme.status_suffix);

    // Width available for content (after the line-number gutter).
    // Used by wrap to decide where to break visual rows. We render
    // body + gutter into the same Paragraph so the visual budget
    // matches the actual area ratatui will draw into.
    let body_w = (content_area.width as usize).saturating_sub(gutter_w);
    // Cache for `scroll_max` so the wrap-aware bound stays
    // accurate across keystrokes. 0 stays as "unknown" (e.g.
    // multi-col) so the wrap-aware path correctly falls back to
    // the logical-line bound there.
    view.last_body_w
        .set(u16::try_from(body_w).unwrap_or(u16::MAX));

    let mut display_lines: Vec<Line<'static>> = Vec::with_capacity(viewport_h);
    let mut src_idx = start;
    while src_idx < content_end && display_lines.len() < viewport_h {
        let line = &view.lines[src_idx];
        let abs_idx = src_idx;
        // Apply per-source-line styling: match highlight, picker
        // cursor highlight, optional whitespace markers. The `$`
        // end-of-line marker naturally ends up on the last wrapped
        // piece because we apply markers *before* wrap.
        let styled = apply_row_styling(line, view, abs_idx, theme);
        let styled = if view.show_whitespace {
            apply_whitespace_markers(&styled, theme)
        } else {
            styled
        };
        // Split into 1+ visual rows. wrap=false ⇒ exactly one piece;
        // wrap=true with body_w available width gives a Vec of
        // visually-bounded chunks, preserving styling per-span.
        // Block visual mode forces wrap off — the column-based
        // selection rectangle only makes sense against logical
        // rows; with wrap on, a "row" the user is selecting could
        // be split across multiple visual rows and the rectangle
        // would smear. Vim does the same.
        let block_mode = view.visual.is_some_and(|v| v.kind == VisualKind::Block);
        let pieces = if view.wrap && body_w > 0 && !block_mode {
            wrap_line(&styled, body_w)
        } else {
            vec![styled]
        };
        for (piece_idx, piece) in pieces.into_iter().enumerate() {
            if display_lines.len() >= viewport_h {
                break;
            }
            if gutter_w > 0 {
                let gutter_text = if piece_idx == 0 {
                    format!("{:>width$} ", abs_idx + 1, width = gutter_w - 1)
                } else {
                    // Continuation row: blank gutter so wrap pieces
                    // visually align with the source line's indent.
                    " ".repeat(gutter_w)
                };
                let mut spans = vec![Span::styled(gutter_text, ln_style)];
                spans.extend(piece.spans);
                display_lines.push(Line::from(spans));
            } else {
                display_lines.push(piece);
            }
        }
        src_idx += 1;
    }
    let reached_end = src_idx >= content_end;

    let eof_style = Style::default()
        .fg(theme.status_suffix)
        .add_modifier(Modifier::DIM);
    if reached_end && display_lines.len() < viewport_h && !view.streaming {
        if !view.eof_in_content {
            display_lines.push(Line::from(Span::styled("[EOF]", eof_style)));
        }
        while display_lines.len() < viewport_h {
            display_lines.push(Line::from(Span::styled("~", eof_style)));
        }
    }

    // Wrap is handled by `wrap_line` above; ratatui's Paragraph::wrap
    // is *not* used because it hard-breaks unbreakable "words" mid-
    // character and continuation rows wouldn't carry the gutter.
    // Yank / save / search operate on `view.lines` so they always
    // see the untruncated source regardless of wrap state.
    let paragraph = Paragraph::new(display_lines);
    frame.render_widget(paragraph, content_area);
}

/// Apply match-highlight + picker-cursor styling to a source line.
/// Extracted from `render_single_column` so wrap can re-use it
/// (styling decisions happen before the visual split).
fn apply_row_styling(
    line: &Line<'static>,
    view: &PagerView,
    abs_idx: usize,
    theme: &Theme,
) -> Line<'static> {
    let mut styled = styled_line_for_render(line, view, abs_idx, theme);
    // Visual selection: paint a muted background across the
    // selected region so the user can see what `y` will yank.
    // - Line mode (`V`): whole rows in `[lo..=hi]`. Cursor row
    //   gets the brighter cursor_bg, others cursor_bg_dim.
    // - Block mode (`^v`): only the rectangular slice
    //   `[lo_col..=hi_col]` of each row in `[lo..=hi]` is
    //   highlighted, painted character-by-character. The cursor
    //   *cell* (cursor_line, cursor_col) gets the brighter bg.
    // Applied before the picker-cursor branch so visual mode wins
    // when both would coincide.
    if let Some(sel) = view.visual {
        let (lo, hi) = sel.range();
        if (lo..=hi).contains(&abs_idx) {
            match sel.kind {
                VisualKind::Line => {
                    let bg = if abs_idx == sel.cursor {
                        theme.cursor_bg
                    } else {
                        theme.cursor_bg_dim
                    };
                    styled = Line::from(
                        styled
                            .spans
                            .into_iter()
                            .map(|s| Span::styled(s.content, s.style.bg(bg)))
                            .collect::<Vec<_>>(),
                    );
                }
                VisualKind::Block => {
                    let (lo_col, hi_col) = sel.col_range();
                    let cursor_col = if abs_idx == sel.cursor {
                        Some(sel.cursor_col)
                    } else {
                        None
                    };
                    styled = paint_block_selection(
                        &styled,
                        lo_col,
                        hi_col,
                        cursor_col,
                        theme.cursor_bg_dim,
                        theme.cursor_bg,
                    );
                }
            }
        }
    }
    // Placement cursor: single reverse-video cell at the current
    // (row, col). The visual cue the user asked for so they can see
    // where the anchor will land when they commit with `^v` / `V`.
    if let Some(p) = view.placement
        && p.row == abs_idx
    {
        let plain: String = styled.spans.iter().map(|s| s.content.as_ref()).collect();
        let row_style = styled.style;
        let before: String = plain.chars().take(p.col).collect();
        let cursor_ch: String = plain
            .chars()
            .nth(p.col)
            .map_or_else(|| " ".into(), |c| c.to_string());
        let after: String = plain.chars().skip(p.col + 1).collect();
        let cursor_style = Style::default()
            .bg(theme.cursor_bg)
            .fg(theme.cursor_fg)
            .add_modifier(Modifier::REVERSED | Modifier::BOLD);
        styled = Line::from(vec![
            Span::styled(before, row_style),
            Span::styled(cursor_ch, cursor_style),
            Span::styled(after, row_style),
        ]);
    }

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
    styled
}

/// Paint a block-selection rectangle's row contribution onto a
/// styled line: chars in `[lo_col..=hi_col]` get
/// `selection_bg` overlaid on their existing style; the
/// `cursor_col` cell (when set) gets `cursor_bg` instead so it
/// reads like vi's "I'm here" cell. Char index is character-
/// based (Unicode scalars), not display-width, so wide-glyph
/// (CJK / emoji) counts as 1 — same convention vim uses for
/// block mode.
///
/// Adjacent characters with identical styles merge into a
/// single span so we don't explode the renderer with one span
/// per char.
fn paint_block_selection(
    line: &Line<'static>,
    lo_col: usize,
    hi_col: usize,
    cursor_col: Option<usize>,
    selection_bg: ratatui::style::Color,
    cursor_bg: ratatui::style::Color,
) -> Line<'static> {
    let mut new_spans: Vec<Span<'static>> = Vec::new();
    let mut current_text = String::new();
    let mut current_style: Option<Style> = None;
    let mut col = 0usize;

    for span in &line.spans {
        for ch in span.content.chars() {
            let in_sel = col >= lo_col && col <= hi_col;
            let style = if in_sel {
                if cursor_col == Some(col) {
                    span.style.bg(cursor_bg)
                } else {
                    span.style.bg(selection_bg)
                }
            } else {
                span.style
            };
            if Some(style) != current_style {
                if !current_text.is_empty() {
                    new_spans.push(Span::styled(
                        std::mem::take(&mut current_text),
                        current_style.unwrap_or_default(),
                    ));
                }
                current_style = Some(style);
            }
            current_text.push(ch);
            col += 1;
        }
    }
    if !current_text.is_empty() {
        new_spans.push(Span::styled(
            current_text,
            current_style.unwrap_or_default(),
        ));
    }
    Line::from(new_spans)
}

/// Split a styled line into 1+ visual rows, each at most `width`
/// columns wide. Hard-break at width if no whitespace boundary is
/// nearby (paths, long single tokens). Preserves per-span styling
/// across the break by splitting the span at the chosen byte
/// offset. Width is in unicode display columns, so wide CJK
/// characters and emoji count as 2 — same units ratatui uses for
/// layout.
/// Count the number of visual rows `line` will occupy when wrapped
/// at `width`. Mirrors `wrap_line`'s greedy hard-break policy
/// (cells are filled left-to-right, breaks happen at the first
/// char that would overflow), but doesn't allocate — used by
/// `scroll_max` on every keystroke.
///
/// Empty lines render as one visual row (a blank line); this
/// matches the renderer's behavior so the math is symmetric.
/// `width == 0` yields one row to match `wrap_line`'s short-circuit.
/// Vi `w`-style word class: alphanumeric + `_`. Whitespace and
/// punctuation each form their own class — a transition between
/// any two of {word, punct, whitespace} counts as a word
/// boundary for forward/backward motion.
fn word_class(c: char) -> u8 {
    if c.is_whitespace() {
        0
    } else if c.is_alphanumeric() || c == '_' {
        1
    } else {
        2
    }
}

/// Index of the next word-start char strictly after `col` in
/// `chars`. Returns `None` when no such position exists.
/// Mirrors vim's `w` motion within a single line: skip the rest
/// of the current word, then any whitespace, land on the first
/// non-whitespace character.
fn next_word_start(chars: &[char], col: usize) -> Option<usize> {
    if col >= chars.len() {
        return None;
    }
    let start_class = word_class(chars[col]);
    let mut i = col + 1;
    // Skip the rest of the current run (same class as the start).
    while i < chars.len() && word_class(chars[i]) == start_class && start_class != 0 {
        i += 1;
    }
    // Skip whitespace.
    while i < chars.len() && word_class(chars[i]) == 0 {
        i += 1;
    }
    if i < chars.len() { Some(i) } else { None }
}

/// Index of the previous word-start char strictly before `col` in
/// `chars`. Returns `None` when the cursor is already at the
/// first word of the line.
fn prev_word_start(chars: &[char], col: usize) -> Option<usize> {
    if col == 0 {
        return None;
    }
    let mut i = col.saturating_sub(1);
    // Skip whitespace backwards.
    while i > 0 && word_class(chars[i]) == 0 {
        i -= 1;
    }
    if word_class(chars[i]) == 0 {
        return None;
    }
    // Walk back to the start of the current run.
    let cur_class = word_class(chars[i]);
    while i > 0 && word_class(chars[i - 1]) == cur_class {
        i -= 1;
    }
    Some(i)
}

/// Index of the last word-start char in `chars`. Used by `b` when
/// the cursor wraps to the previous row.
fn last_word_start(chars: &[char]) -> Option<usize> {
    if chars.is_empty() {
        return None;
    }
    let mut i = chars.len() - 1;
    while i > 0 && word_class(chars[i]) == 0 {
        i -= 1;
    }
    if word_class(chars[i]) == 0 {
        return None;
    }
    let cur_class = word_class(chars[i]);
    while i > 0 && word_class(chars[i - 1]) == cur_class {
        i -= 1;
    }
    Some(i)
}

fn visual_rows(line: &Line<'_>, width: usize) -> usize {
    if width == 0 {
        return 1;
    }
    let total: usize = line
        .spans
        .iter()
        .flat_map(|s| s.content.chars())
        .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(0))
        .sum();
    if total == 0 {
        return 1;
    }
    total.div_ceil(width)
}

fn wrap_line(line: &Line<'static>, width: usize) -> Vec<Line<'static>> {
    if width == 0 {
        return vec![line.clone()];
    }
    let mut pieces: Vec<Vec<Span<'static>>> = vec![Vec::new()];
    let mut current_w = 0usize;
    for span in &line.spans {
        let mut rest: &str = span.content.as_ref();
        while !rest.is_empty() {
            let remaining = width.saturating_sub(current_w);
            if remaining == 0 {
                pieces.push(Vec::new());
                current_w = 0;
                continue;
            }
            let mut consumed_bytes = 0usize;
            let mut visual = 0usize;
            for (idx, ch) in rest.char_indices() {
                let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
                if visual + w > remaining {
                    break;
                }
                consumed_bytes = idx + ch.len_utf8();
                visual += w;
            }
            // Force at least one char even if it's wider than the
            // remaining budget (tiny pager boxes shouldn't infinite
            // loop on a 2-col emoji in a 1-col viewport).
            if consumed_bytes == 0
                && let Some(first) = rest.chars().next()
            {
                consumed_bytes = first.len_utf8();
                visual = unicode_width::UnicodeWidthChar::width(first).unwrap_or(1);
            }
            let chunk = rest[..consumed_bytes].to_string();
            rest = &rest[consumed_bytes..];
            if !chunk.is_empty() {
                pieces
                    .last_mut()
                    .unwrap()
                    .push(Span::styled(chunk, span.style));
                current_w += visual;
            }
            if !rest.is_empty() {
                pieces.push(Vec::new());
                current_w = 0;
            }
        }
    }
    // Drop trailing empty piece (from a span that exactly hit width
    // and started a new row that never got content).
    if pieces.last().is_some_and(Vec::is_empty) && pieces.len() > 1 {
        pieces.pop();
    }
    pieces.into_iter().map(Line::from).collect()
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
            if is_last_col && col_start < content_end && !view.eof_in_content {
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

/// Sentinel title used to identify the pager-help overlay so the
/// `Esc` handler can dismiss just the help and pop back to the
/// underlying pager that was active when `?` was pressed.
pub const PAGER_HELP_TITLE: &str = "Pager help";

/// Where the pager's outer block should draw, given the parent
/// `area` (whatever rect the caller hands to `render`) and the
/// view's `mount` / sizing flags.
///
/// - `Mount::Overlay` keeps the pre-v1.5 dispatch: full-width if
///   the user toggled it, fit-to-content for short summaries,
///   else the centered 90×92 % box.
/// - `Mount::TopPane` / `Mount::LowerPane` use `area` as-is — the
///   caller (App::render) passes the slot's rect directly so the
///   pager fills it without extra centering.
///
/// `full_width` and `fit_to_content` are deliberately ignored for
/// the pane mounts because the slot's rect already defines the
/// pager's footprint there. We could honor them later if a use
/// case demands it.
fn pager_inner_area(area: Rect, view: &PagerView) -> Rect {
    match view.mount {
        Mount::Overlay => {
            if view.full_width {
                area
            } else if view.fit_to_content {
                fit_height_rect(area, view)
            } else {
                centered_rect(area, CENTERED_W_PCT, 92)
            }
        }
        Mount::TopPane | Mount::LowerPane => area,
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn plain_text(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    /// Regression test for the wrap-vs-bottom bug: a file with
    /// long lines that wrap to multiple visual rows would lose the
    /// trailing logical lines when scrolled to "Bot". Reported on
    /// `docs/spyc-logo.svg` (154 logical lines, several wrap to 2
    /// rows each, viewport ~40 rows). The user saw "Bot" but lines
    /// 151-154 never appeared.
    ///
    /// Cause: `scroll_max` computed the cap from logical line
    /// count, ignoring that wrapped lines consume extra visual
    /// rows. Fix: when wrap is on and `body_w` is known, walk the
    /// lines from the end summing visual rows; the highest scroll
    /// value that still includes the last line in the viewport is
    /// the true max.
    #[test]
    fn scroll_max_accounts_for_wrapped_visual_rows() {
        // 5 logical lines; each one is 60 chars wide. With body_w=20
        // each line takes 3 visual rows. Viewport is 6 visual rows
        // (= 2 logical lines fully unwrapped). Without the fix,
        // scroll_max = 5 - 6 = 0 (saturating; "All"); with the fix
        // we should be able to scroll through ~3 logical lines so
        // line 5's content lands in the last visible row.
        let view = PagerView::new_plain("test", vec!["x".repeat(60); 5]);
        view.last_body_w.set(20);
        assert!(
            view.scroll_max(6) >= 3,
            "scroll_max({}) too small — content past visual viewport \
             will be unreachable",
            view.scroll_max(6),
        );
    }

    /// Regression test for the "stuck at bottom" search bug in the
    /// help pager (which is multi-column). With `ncols >= 2`, `scroll`
    /// is interpreted per-column (each column applies the same offset
    /// within its own chunk). `scroll_to_match` used to feed the
    /// global line index straight into `self.scroll`, so a match in
    /// column 2+ overshot `scroll_max` (= longest-chunk - vh) and got
    /// clamped to the bottom — hiding the match. Symptom users hit:
    /// `/show` in the help overlay then `n n n n` left the view stuck
    /// at the bottom.
    #[test]
    fn scroll_to_match_translates_to_chunk_local_offset_in_multi_col() {
        // 200 lines, no blank lines so partition_lines_static cuts at
        // exactly idx 100 (blank-line search finds nothing in the
        // window and falls back to the ideal cut). Matches every 50:
        // {0, 50, 100, 150}. col1 chunk = [0, 100), col2 = [100, 200).
        let lines: Vec<String> = (0..200)
            .map(|i| {
                if i % 50 == 0 {
                    format!("line {i} show")
                } else {
                    format!("line {i}")
                }
            })
            .collect();
        let mut view = PagerView::new_plain("help", lines);
        view.columns = 2;
        let viewport = 24u16;

        view.begin_search();
        for c in "show".chars() {
            view.search_push_char(c);
        }
        assert!(view.commit_search(viewport));

        // After commit: cursor=0 (line 0), scroll=0.
        assert_eq!(view.scroll, 0);

        // n → line 50 in col1 chunk. Chunk-local idx = 50, scroll = 50 - 8 = 42.
        view.search_next(viewport);
        assert_eq!(
            view.scroll, 42,
            "n into mid-col1 should land near the match"
        );

        // n → line 100 (start of col2 chunk). Chunk-local idx = 0, scroll = 0.
        // Pre-fix: target = 100 - 8 = 92, clamped to scroll_max = 100 - 24 = 76 → bottom.
        view.search_next(viewport);
        assert_eq!(
            view.scroll, 0,
            "n onto first col2 match should reset scroll to the top of col2's chunk, \
             not pin to scroll_max"
        );

        // n → line 150 in col2 chunk. Chunk-local idx = 50, scroll = 42.
        // Pre-fix: target = 142, clamped to 76 → "stuck at bottom".
        view.search_next(viewport);
        assert_eq!(
            view.scroll, 42,
            "n onto mid-col2 match should land mid-chunk, not pin to scroll_max"
        );
    }

    #[test]
    fn scroll_max_logical_when_no_wrap_or_no_body_w() {
        let mut view = PagerView::new_plain("test", vec!["x".repeat(60); 10]);
        // wrap off → logical-line behavior
        view.wrap = false;
        assert_eq!(view.scroll_max(4), 6); // 10 - 4
        // wrap on but body_w = 0 (e.g. before first render) →
        // fall back to logical-line behavior so we don't return a
        // bogus value when the wrap-aware path can't compute.
        view.wrap = true;
        view.last_body_w.set(0);
        assert_eq!(view.scroll_max(4), 6);
    }

    #[test]
    fn wrap_short_line_returns_one_piece() {
        let line = Line::from("hello");
        let pieces = wrap_line(&line, 80);
        assert_eq!(pieces.len(), 1);
        assert_eq!(plain_text(&pieces[0]), "hello");
    }

    #[test]
    fn wrap_long_line_hard_breaks() {
        let line = Line::from("aaaaabbbbbcccccddddd");
        let pieces = wrap_line(&line, 5);
        assert_eq!(pieces.len(), 4);
        assert_eq!(plain_text(&pieces[0]), "aaaaa");
        assert_eq!(plain_text(&pieces[1]), "bbbbb");
        assert_eq!(plain_text(&pieces[2]), "ccccc");
        assert_eq!(plain_text(&pieces[3]), "ddddd");
    }

    #[test]
    fn wrap_preserves_styled_spans_across_break() {
        let red = Style::default().fg(ratatui::style::Color::Red);
        let blue = Style::default().fg(ratatui::style::Color::Blue);
        let line = Line::from(vec![
            Span::styled("aaaaa", red),
            Span::styled("BBBBB", blue),
        ]);
        let pieces = wrap_line(&line, 4);
        // 10 chars at width 4 ⇒ 3 visual rows (4+4+2). Spans split
        // across the break preserve their style on each side.
        assert_eq!(pieces.len(), 3);
        assert_eq!(plain_text(&pieces[0]), "aaaa");
        assert_eq!(pieces[0].spans[0].style, red);
        assert_eq!(plain_text(&pieces[1]), "aBBB");
        assert_eq!(pieces[1].spans[0].style, red);
        assert_eq!(pieces[1].spans[1].style, blue);
        assert_eq!(plain_text(&pieces[2]), "BB");
        assert_eq!(pieces[2].spans[0].style, blue);
    }

    #[test]
    fn wrap_handles_wide_chars() {
        // A single CJK char is 2 cols wide; in a 3-col viewport
        // we fit one per row.
        let line = Line::from("漢字漢");
        let pieces = wrap_line(&line, 3);
        assert_eq!(pieces.len(), 3);
        assert_eq!(plain_text(&pieces[0]), "漢");
        assert_eq!(plain_text(&pieces[1]), "字");
        assert_eq!(plain_text(&pieces[2]), "漢");
    }

    #[test]
    fn wrap_zero_width_returns_clone() {
        let line = Line::from("anything");
        let pieces = wrap_line(&line, 0);
        assert_eq!(pieces.len(), 1);
        assert_eq!(plain_text(&pieces[0]), "anything");
    }

    // ── Visual line mode ─────────────────────────────────────────────────

    fn sample_view() -> PagerView {
        PagerView::new_plain("v", (0..20).map(|i| format!("line {i}")).collect())
    }

    #[test]
    fn enter_visual_anchors_at_top_visible_line() {
        let mut view = sample_view();
        view.scroll = 5;
        view.enter_visual();
        let sel = view.visual.expect("should be in visual mode");
        assert_eq!(sel.anchor, 5);
        assert_eq!(sel.cursor, 5);
        assert!(view.is_visual());
    }

    #[test]
    fn enter_visual_on_empty_buffer_is_noop() {
        let mut view = PagerView::new_plain("v", Vec::<String>::new());
        view.enter_visual();
        assert!(view.visual.is_none());
    }

    #[test]
    fn visual_move_extends_cursor_and_clamps() {
        let mut view = sample_view();
        view.enter_visual();
        view.visual_move(3, 10);
        assert_eq!(view.visual.unwrap().cursor, 3);
        // Clamp at the bottom — buffer has 20 lines (idx 0..=19).
        view.visual_move(100, 10);
        assert_eq!(view.visual.unwrap().cursor, 19);
        // And at the top.
        view.visual_move(-100, 10);
        assert_eq!(view.visual.unwrap().cursor, 0);
        // Anchor is unchanged through movement.
        assert_eq!(view.visual.unwrap().anchor, 0);
    }

    #[test]
    fn visual_range_is_inclusive_and_order_independent() {
        let sel = VisualSelection {
            anchor: 5,
            cursor: 10,
            anchor_col: 0,
            cursor_col: 0,
            kind: VisualKind::Line,
        };
        assert_eq!(sel.range(), (5, 10));
        let sel = VisualSelection {
            anchor: 10,
            cursor: 5,
            anchor_col: 0,
            cursor_col: 0,
            kind: VisualKind::Line,
        };
        // Cursor moved up past the anchor — range still goes low → high.
        assert_eq!(sel.range(), (5, 10));
    }

    #[test]
    fn visual_move_auto_scrolls_when_cursor_leaves_viewport() {
        let mut view = sample_view();
        view.scroll = 0;
        view.enter_visual();
        // Viewport = 5 rows. Move cursor past the bottom edge — scroll
        // should advance so the cursor stays visible.
        view.visual_move(7, 5);
        assert_eq!(view.visual.unwrap().cursor, 7);
        // cursor=7, vh=5 → scroll = 7 + 1 - 5 = 3
        assert_eq!(view.scroll, 3);
        // Move back up past the top — scroll should retreat.
        view.visual_move(-7, 5);
        assert_eq!(view.visual.unwrap().cursor, 0);
        assert_eq!(view.scroll, 0);
    }

    #[test]
    fn visual_jump_to_clamps_and_scrolls() {
        let mut view = sample_view();
        view.enter_visual();
        view.visual_jump_to(15, 5);
        assert_eq!(view.visual.unwrap().cursor, 15);
        assert_eq!(view.scroll, 11);
        // Beyond the end is clamped.
        view.visual_jump_to(999, 5);
        assert_eq!(view.visual.unwrap().cursor, 19);
    }

    #[test]
    fn cancel_visual_clears_state() {
        let mut view = sample_view();
        view.enter_visual();
        assert!(view.is_visual());
        view.cancel_visual();
        assert!(!view.is_visual());
    }

    #[test]
    fn visual_move_outside_visual_mode_is_noop() {
        let mut view = sample_view();
        view.scroll = 4;
        view.visual_move(5, 10);
        // No selection started, no scroll change.
        assert!(view.visual.is_none());
        assert_eq!(view.scroll, 4);
    }

    #[test]
    fn visual_status_text_reports_range_and_count() {
        let mut view = sample_view();
        view.enter_visual();
        view.visual_move(4, 10);
        let s = view.status_text().expect("status while visual");
        assert!(s.contains("VISUAL"), "expected VISUAL marker, got: {s}");
        assert!(s.contains("L1-L5"), "expected L1-L5, got: {s}");
        assert!(s.contains("5 lines"), "expected count, got: {s}");
    }

    #[test]
    fn visual_status_pluralizes_correctly_for_single_line() {
        let mut view = sample_view();
        view.enter_visual();
        // anchor == cursor → single-line range.
        let s = view.status_text().expect("status while visual");
        assert!(s.contains("(1 line)"), "expected singular, got: {s}");
    }

    // ── v1.5 Phase 4: visual block (columnar) mode ─────────────────

    fn block_view_with(content: &[&str]) -> PagerView {
        PagerView::new_plain("v", content.iter().map(|&s| s.to_string()).collect())
    }

    #[test]
    fn placement_move_then_commit_anchors_at_cursor() {
        let mut view = block_view_with(&["abcdef", "ghi jkl", "mnopqr"]);
        view.enter_placement();
        let p = view.placement.expect("placement active");
        assert_eq!((p.row, p.col), (0, 0));
        // hjkl-style motion: down 1, right 2.
        view.placement_move(1, 2, 5);
        // Word forward from "ghi jkl" col 2 ('i') → 'j' at col 4.
        view.placement_word_forward();
        let p = view.placement.expect("still placement");
        assert_eq!((p.row, p.col), (1, 4));
        // Second ^v commits to block visual at the cursor.
        view.commit_placement_to_visual_block();
        assert!(view.placement.is_none(), "placement consumed on commit");
        let sel = view.visual.expect("block visual");
        assert_eq!(sel.kind, VisualKind::Block);
        assert_eq!(sel.anchor, 1);
        assert_eq!(sel.cursor, 1);
        assert_eq!(sel.anchor_col, 4);
        assert_eq!(sel.cursor_col, 4);
    }

    #[test]
    fn placement_uppercase_v_commits_to_line_at_cursor_row() {
        let mut view = block_view_with(&["aaa", "bbb", "ccc"]);
        view.enter_placement();
        view.placement_move(2, 0, 5);
        view.commit_placement_to_visual_line();
        let sel = view.visual.expect("line visual");
        assert_eq!(sel.kind, VisualKind::Line);
        assert_eq!(sel.anchor, 2);
        assert_eq!(sel.cursor, 2);
    }

    #[test]
    fn placement_esc_clears_without_starting_visual() {
        let mut view = block_view_with(&["a", "b"]);
        view.enter_placement();
        view.placement_move(1, 0, 5);
        view.cancel_placement();
        assert!(view.placement.is_none());
        assert!(view.visual.is_none());
    }

    #[test]
    fn enter_visual_block_starts_in_block_mode() {
        let mut view = block_view_with(&["abc", "def", "ghi"]);
        view.enter_visual_block();
        let sel = view.visual.expect("visual active");
        assert_eq!(sel.kind, VisualKind::Block);
        assert_eq!(sel.anchor_col, 0);
        assert_eq!(sel.cursor_col, 0);
    }

    #[test]
    fn enter_visual_block_upgrades_existing_line_visual() {
        let mut view = block_view_with(&["abcdef", "ghijkl", "mnopqr"]);
        view.enter_visual();
        view.visual_move(2, 5);
        let pre = view.visual.expect("line visual");
        assert_eq!(pre.kind, VisualKind::Line);
        view.enter_visual_block();
        let post = view.visual.expect("block visual");
        assert_eq!(post.kind, VisualKind::Block);
        // Anchor / cursor lines preserved through the upgrade.
        assert_eq!(post.anchor, pre.anchor);
        assert_eq!(post.cursor, pre.cursor);
    }

    #[test]
    fn col_range_is_inclusive_and_order_independent() {
        let sel = VisualSelection {
            anchor: 0,
            cursor: 0,
            anchor_col: 2,
            cursor_col: 7,
            kind: VisualKind::Block,
        };
        assert_eq!(sel.col_range(), (2, 7));
        let sel = VisualSelection {
            anchor: 0,
            cursor: 0,
            anchor_col: 7,
            cursor_col: 2,
            kind: VisualKind::Block,
        };
        // Cursor moved left past anchor — range still goes low→high.
        assert_eq!(sel.col_range(), (2, 7));
    }

    #[test]
    fn visual_col_move_extends_and_clamps_at_zero() {
        let mut view = block_view_with(&["abcdef"]);
        view.enter_visual_block();
        view.visual_col_move(3);
        assert_eq!(view.visual.unwrap().cursor_col, 3);
        // Clamp at 0 on the left.
        view.visual_col_move(-100);
        assert_eq!(view.visual.unwrap().cursor_col, 0);
        // Anchor unchanged.
        assert_eq!(view.visual.unwrap().anchor_col, 0);
    }

    #[test]
    fn visual_col_move_is_noop_outside_block_mode() {
        // Line mode: visual_col_move must not touch the cursor_col
        // (it's stored but ignored, by design).
        let mut view = block_view_with(&["abcdef"]);
        view.enter_visual();
        view.visual_col_move(3);
        assert_eq!(view.visual.unwrap().cursor_col, 0);
    }

    #[test]
    fn block_yank_extracts_rectangular_slice() {
        // 4-line CSV-ish grid, yank a 3×3 rectangle (rows 0..=2,
        // cols 1..=3) → "bcd / fgh / jkl".
        let mut view = block_view_with(&["abcde", "efghi", "ijklm", "mnopq"]);
        view.enter_visual_block();
        view.visual_move(2, 5); // rows 0..=2
        view.visual_col_move(3); // cols 0..=3 inclusive...
        // Wait: anchor_col=0, cursor_col=3 → col_range = (0,3) → 4 chars
        // So yank picks chars 0..=3 of each row.
        let sel = view.visual.unwrap();
        let (lo_col, hi_col) = sel.col_range();
        assert_eq!((lo_col, hi_col), (0, 3));
        // We can't exercise the system-clipboard side from a unit test, but
        // the slice math is what we want to verify. Reproduce the
        // same logic the yank uses:
        let plain: Vec<String> = view
            .lines
            .iter()
            .take(3)
            .map(|l| {
                line_plain_text(l)
                    .chars()
                    .skip(lo_col)
                    .take(hi_col + 1 - lo_col)
                    .collect()
            })
            .collect();
        assert_eq!(plain, vec!["abcd", "efgh", "ijkl"]);
    }

    #[test]
    fn block_yank_handles_short_rows_gracefully() {
        // The middle row is shorter than the column range — yank
        // takes whatever chars are available and stops, doesn't
        // pad or panic.
        let mut view = block_view_with(&["abcdefgh", "xy", "1234567"]);
        view.enter_visual_block();
        view.visual_move(2, 5);
        view.visual_col_move(5); // col_range = (0, 5) → 6 chars wanted

        let sel = view.visual.unwrap();
        let (lo_col, hi_col) = sel.col_range();
        let plain: Vec<String> = view
            .lines
            .iter()
            .take(3)
            .map(|l| {
                line_plain_text(l)
                    .chars()
                    .skip(lo_col)
                    .take(hi_col + 1 - lo_col)
                    .collect()
            })
            .collect();
        assert_eq!(plain, vec!["abcdef", "xy", "123456"]);
    }

    #[test]
    fn block_status_text_reports_rect_dimensions() {
        let mut view = block_view_with(&["abcdef", "ghijkl", "mnopqr"]);
        view.enter_visual_block();
        view.visual_move(2, 5);
        view.visual_col_move(3);
        let s = view.status_text().expect("status while visual block");
        assert!(s.contains("VISUAL BLOCK"), "got: {s}");
        assert!(s.contains("L1-L3"), "got: {s}");
        assert!(s.contains("C1-C4"), "got: {s}");
        assert!(s.contains("(3×4)"), "got: {s}");
    }

    #[test]
    fn block_range_stays_inclusive_when_anchor_higher_than_cursor() {
        // Direct construction so we can pin both axes — the
        // public API only ever sets `anchor_col = 0` at entry.
        // Anchor at (line 5, col 7), cursor dragged up-and-left
        // to (line 2, col 3). Both range helpers must still
        // return low → high so the renderer and yank get a
        // sensible rectangle.
        let sel = VisualSelection {
            anchor: 5,
            cursor: 2,
            anchor_col: 7,
            cursor_col: 3,
            kind: VisualKind::Block,
        };
        assert_eq!(sel.range(), (2, 5));
        assert_eq!(sel.col_range(), (3, 7));
    }

    // ── v1.5 Phase 3 polish ────────────────────────────────────────

    #[test]
    fn pending_scroll_to_bottom_default_is_false() {
        let view = sample_view();
        assert!(
            !view.pending_scroll_to_bottom.get(),
            "constructors should leave the flag off by default"
        );
    }

    #[test]
    fn scroll_to_bottom_with_viewport_lands_in_bottom_window() {
        // 20 lines, viewport=5 → scroll_max = 15 (last 5 lines visible).
        let mut view = sample_view();
        view.scroll_to_bottom(5);
        assert_eq!(view.scroll, 15);
    }

    #[test]
    fn lower_pane_mount_renders_borderless() {
        // Render-side check (no actual frame): pager_inner_area for
        // LowerPane mount returns the area as-is — and the borderless
        // branch of the render block uses `Block::default()` with no
        // borders. Verify the rect helper still uses the rect as-is.
        let mut view = sample_view();
        view.mount = Mount::LowerPane;
        let slot = Rect::new(0, 21, 100, 19);
        assert_eq!(pager_inner_area(slot, &view), slot);
    }

    // ── v1.5 Phase 1: Mount enum & rect dispatch ───────────────────

    #[test]
    fn mount_default_is_overlay() {
        let view = sample_view();
        assert_eq!(view.mount, Mount::Overlay);
    }

    #[test]
    fn pager_inner_area_overlay_centers() {
        // 100x40 frame, default Mount::Overlay → centered 90×92 %.
        let view = sample_view();
        let frame = Rect::new(0, 0, 100, 40);
        let inner = pager_inner_area(frame, &view);
        assert!(inner.width < frame.width, "should be narrower than frame");
        assert!(inner.height < frame.height, "should be shorter than frame");
        assert!(inner.x > frame.x, "should be inset from left");
        assert!(inner.y > frame.y, "should be inset from top");
    }

    #[test]
    fn pager_inner_area_overlay_full_width_uses_whole_area() {
        let mut view = sample_view();
        view.full_width = true;
        let frame = Rect::new(0, 0, 100, 40);
        assert_eq!(pager_inner_area(frame, &view), frame);
    }

    #[test]
    fn pager_inner_area_top_pane_uses_area_as_is() {
        let mut view = sample_view();
        view.mount = Mount::TopPane;
        // Caller would pass the top-pane slot rect; pager must
        // honor it verbatim (no extra centering / fit logic).
        let slot = Rect::new(0, 0, 100, 20);
        assert_eq!(pager_inner_area(slot, &view), slot);
    }

    #[test]
    fn pager_inner_area_top_pane_ignores_full_width_and_fit() {
        // Pane mounts deliberately ignore the overlay sizing
        // flags — the slot's rect already defines the footprint.
        let mut view = sample_view();
        view.mount = Mount::TopPane;
        view.full_width = true;
        view.fit_to_content = true;
        let slot = Rect::new(5, 2, 80, 15);
        assert_eq!(pager_inner_area(slot, &view), slot);
    }

    #[test]
    fn pager_inner_area_lower_pane_uses_area_as_is() {
        let mut view = sample_view();
        view.mount = Mount::LowerPane;
        let slot = Rect::new(0, 21, 100, 19);
        assert_eq!(pager_inner_area(slot, &view), slot);
    }

    // ── snapshot tests (TestBackend) ──────────────────────────────
    //
    // Glyph-level snapshots of the pager's four interesting modes:
    // ANSI input (color-tagged source), hex dump styling, line-number
    // gutter, and search highlight. We capture symbols only (no
    // styling) — same trade-off as `ui::status::tests`. A regression
    // that breaks layout, gutter width, search-bar formatting, or
    // hex-dump structure will diff visibly.

    use crate::ui::theme::Theme;
    use ratatui::{Terminal, backend::TestBackend};

    fn render_pager_to_string(view: &PagerView, w: u16, h: u16) -> String {
        let backend = TestBackend::new(w, h);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::default();
        terminal
            .draw(|f| {
                let area = Rect::new(0, 0, w, h);
                super::render(f, area, view, &theme);
            })
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let mut out = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                out.push_str(buf.cell((x, y)).map_or(" ", |c| c.symbol()));
            }
            out.push('\n');
        }
        // Trim trailing whitespace per line and drop trailing blank
        // lines so the snapshot stays tight.
        out.lines()
            .map(str::trim_end)
            .collect::<Vec<_>>()
            .join("\n")
            .trim_end()
            .to_string()
    }

    #[test]
    fn snapshot_pager_ansi() {
        // ANSI escapes are parsed into styled spans; the snapshot
        // captures glyphs, so this is mostly a layout/structure check
        // that ANSI input doesn't bleed escape bytes into the buffer.
        let bytes = b"\x1b[31mred line\x1b[0m\n\x1b[1;32mbold green\x1b[0m\nplain\n";
        let mut view = PagerView::new_ansi("ansi.txt", bytes);
        view.full_width = true;
        let out = render_pager_to_string(&view, 60, 8);
        insta::assert_snapshot!(out);
    }

    #[test]
    fn snapshot_pager_hex() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bin");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"\x7fELF\x02\x01\x01\x00hello, spyc!").unwrap();
        let lines = crate::fs::ops::hex_dump_lines(&path, &Theme::default()).unwrap();
        let mut view = PagerView::new_styled("bin", lines);
        view.full_width = true;
        let out = render_pager_to_string(&view, 80, 6);
        insta::assert_snapshot!(out);
    }

    #[test]
    fn snapshot_pager_line_numbers() {
        // 12 lines so the gutter is at least 2 digits wide, which is
        // the case the renderer has to right-align.
        let lines: Vec<String> = (1..=12).map(|i| format!("line {i}")).collect();
        let mut view = PagerView::new_plain("notes.txt", lines);
        view.full_width = true;
        // show_line_numbers is on by default in new_plain; assert.
        assert!(view.show_line_numbers);
        let out = render_pager_to_string(&view, 40, 14);
        insta::assert_snapshot!(out);
    }

    #[test]
    fn snapshot_pager_search_highlight() {
        let mut view = PagerView::new_plain(
            "search.txt",
            vec![
                "alpha".to_string(),
                "beta needle".to_string(),
                "gamma".to_string(),
                "delta needle".to_string(),
                "epsilon".to_string(),
            ],
        );
        view.full_width = true;
        view.begin_search();
        for c in "needle".chars() {
            view.search_push_char(c);
        }
        // Viewport height matches what we'll render with.
        let committed = view.commit_search(8);
        assert!(committed, "search query should match");
        let out = render_pager_to_string(&view, 50, 8);
        insta::assert_snapshot!(out);
    }

    // ── yank title header ─────────────────────────────────────────

    #[test]
    fn title_header_prepended_when_include_true() {
        let view = PagerView::new_plain("!cargo build", vec!["hello".into(), "world".into()]);
        let out = view.with_title_header(view.source_text(), true);
        assert_eq!(out, "# !cargo build\n\nhello\nworld");
    }

    #[test]
    fn title_header_skipped_when_include_false() {
        let view = PagerView::new_plain("!cargo build", vec!["hello".into()]);
        let out = view.with_title_header(view.source_text(), false);
        assert_eq!(out, "hello");
    }

    #[test]
    fn title_header_skipped_when_title_empty() {
        // Empty title (rare but possible) ⇒ no header even with
        // include_title = true — pasting "# \n\n..." is uglier than
        // pasting just the content.
        let view = PagerView::new_plain("", vec!["hello".into()]);
        let out = view.with_title_header(view.source_text(), true);
        assert_eq!(out, "hello");
    }

    // ── markdown toggle scroll preservation ───────────────────────

    /// Build a markdown-enabled pager: `lines` is the rendered side
    /// (10 entries), `alt_lines` is the source side (5 entries).
    /// The two sides have intentionally different sizes — that's the
    /// whole reason the old "reset to 0" rule existed.
    fn md_view() -> PagerView {
        let rendered: Vec<Line<'static>> = (0..10)
            .map(|i| Line::from(format!("rendered{i}")))
            .collect();
        let source: Vec<Line<'static>> = (0..5).map(|i| Line::from(format!("source{i}"))).collect();
        let mut v = PagerView::new_styled("README.md", rendered);
        v.alt_lines = Some(source);
        v
    }

    #[test]
    fn toggle_markdown_first_time_projects_proportionally() {
        let mut v = md_view();
        // 10 rendered lines, currently at line 8 (≈ 89% down).
        v.scroll = 8;
        assert!(v.toggle_markdown());
        // Source side has 5 lines (max scroll = 4). 8/9 * 4 ≈ 3.55 → 3.
        assert_eq!(v.scroll, 3);
    }

    #[test]
    fn toggle_markdown_round_trip_restores_exact_position() {
        let mut v = md_view();
        v.scroll = 7;
        // rendered → source (proportional projection)
        v.toggle_markdown();
        let source_landing = v.scroll;
        // user reads source, scrolls a bit
        v.scroll = 1;
        // source → rendered (must restore the user's *original* 7, not
        // the proportional projection of 1)
        v.toggle_markdown();
        assert_eq!(v.scroll, 7, "rendered side should restore prior position");
        // rendered → source again (must restore the 1 we left at)
        v.toggle_markdown();
        assert_eq!(
            v.scroll, 1,
            "source side should restore the position we left it at"
        );
        // sanity: source_landing wasn't already 1 (otherwise the test
        // would falsely pass even with broken memory).
        assert_ne!(source_landing, 1);
    }

    #[test]
    fn toggle_markdown_clamps_restored_scroll_to_new_bounds() {
        // If the saved value is past the end of the new buffer (can
        // happen if a buffer gets shorter between visits — not common
        // for markdown but the clamp is cheap insurance), we should
        // land at the last valid index, not panic or sit past EOF.
        let mut v = md_view();
        v.saved_alt_scroll = Some(99);
        v.scroll = 0;
        v.toggle_markdown();
        // Source side has 5 lines → max scroll index 4.
        assert_eq!(v.scroll, 4);
    }

    #[test]
    fn toggle_markdown_no_alt_returns_false() {
        let mut v = PagerView::new_plain("plain.txt", vec!["hi".into()]);
        assert!(!v.toggle_markdown());
        assert_eq!(v.scroll, 0);
    }
}
