//! In-app scrollable pager overlay with incremental search.
//!
//! Used for spyc-internal content where shelling out to `less` would be
//! overkill — long listings, file contents, captured `!` output, version
//! info. Arbitrary terminal-output viewing lives here too, with ANSI
//! colors preserved via `ansi-to-tui`.

use ratatui::text::Line;

mod construct;
mod layout;
mod render;
mod scroll_search;
mod selection;

pub use construct::build_pager_help;
pub use layout::{centered_body_width, centered_col_width};
pub use render::render;

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
    /// When set, this pager view is backed by a [`crate::app`] *pager stream*
    /// (the unified worker→pager abstraction: grep / git-view diff·show·blame /
    /// agent transcript). The main tick loop drains the active stream into this
    /// view while the id matches; a wake for a replaced / closed / stashed
    /// pager self-discards.
    pub stream_id: Option<u32>,
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
    /// ` ```mermaid ` blocks in this view (rendered-line range + source),
    /// collected by `markdown::render_doc`. Empty for non-markdown views.
    /// Drives the `o`-to-open hook (and later the inline image). See
    /// `docs/MERMAID_PAGER_PLAN.md`.
    pub mermaid_blocks: Vec<crate::ui::markdown::MermaidBlock>,
}

/// Sentinel title used to identify the pager-help overlay so the
/// `Esc` handler can dismiss just the help and pop back to the
/// underlying pager that was active when `?` was pressed.
pub const PAGER_HELP_TITLE: &str = "Pager help";

#[cfg(test)]
mod tests;
