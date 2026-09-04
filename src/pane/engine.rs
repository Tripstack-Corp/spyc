//! The terminal-engine seam: what spyc needs from a VT state machine, and
//! nothing about which one it is.
//!
//! Introduced by the engine series (`docs/drafts/V2_2_PLAN.md` §8) ahead of
//! swapping the implementation. vt100 sits behind it today; the point is that
//! swapping it becomes a change of one impl rather than a change of six files.
//!
//! ## Why the cell API looks like this
//!
//! Both cell walkers — `pane::widget`'s render and `ui::scrollback`'s span
//! merge — want the same two things per cell and hold neither beyond the
//! immediate push: the text, and a comparable style. So [`TerminalScreen`]
//! offers a `Copy` [`CellStyle`] plus a text call that **appends into a
//! caller-owned buffer**.
//!
//! That shape is not a stylistic preference. A borrowed `&str` per cell works
//! for an engine that stores UTF-8 inline and does not for one that hands back
//! codepoints needing assembly, so a borrowing accessor would quietly commit
//! the seam to the incumbent's storage layout — the thing this module exists to
//! stop. Appending also costs no allocation per cell: the render reuses one
//! buffer, and the span merge appends straight into the span it is building.

/// A colour in the model spyc's renderer consumes.
///
/// Exactly the three cases `pane::widget` maps onto ratatui. An engine with a
/// richer colour model narrows to this at the seam rather than widening the
/// seam to it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Color {
    #[default]
    Default,
    Idx(u8),
    Rgb(u8, u8, u8),
}

/// A cell's role in a double-width glyph.
///
/// The two walkers treat continuations differently — the render writes them,
/// the span merge skips them — so the seam has to distinguish a continuation
/// from a genuinely blank cell rather than reporting both as empty.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Wide {
    #[default]
    Narrow,
    /// First half of a double-width glyph; carries the text.
    Head,
    /// Second half; carries no text of its own.
    Tail,
}

/// Everything spyc draws about one cell except its text.
///
/// `Copy` and `PartialEq` on purpose: `ui::scrollback` merges adjacent cells
/// into one span by comparing styles, and that comparison is the hot inner
/// loop of building a scrollback pager.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct CellStyle {
    pub fg: Color,
    pub bg: Color,
    pub bold: bool,
    /// SGR 2. Read since #452 — the engine reported it and the adapter used to
    /// drop it.
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    /// SGR 7. Named `reverse` rather than the incumbent's `inverse` because
    /// that is what ratatui and the ECMA-48 text call it.
    pub reverse: bool,
    pub wide: Wide,
}

/// Mouse-reporting mode the child asked for (DECSET 9 / 1000 / 1002 / 1003).
///
/// The gate on forwarding: `None` means the child never opted in, and the
/// escape bytes would land as literal input at its prompt (#170).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MouseMode {
    #[default]
    None,
    /// DECSET 9 — X10 compatibility, press only.
    Press,
    /// DECSET 1000 — press and release.
    PressRelease,
    /// DECSET 1002 — plus motion while a button is held.
    ButtonMotion,
    /// DECSET 1003 — plus motion with no button held.
    AnyMotion,
}

/// How the child wants mouse events encoded (DECSET 1005 / 1006).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MouseEncoding {
    /// X10: a single byte per coordinate, offset by 32. Cannot address past
    /// column 223.
    #[default]
    Default,
    /// DECSET 1005 — UTF-8 coordinates.
    Utf8,
    /// DECSET 1006 — SGR. The only one that addresses a wide terminal.
    Sgr,
}

/// The read surface spyc's renderer and adapters need from a terminal screen.
///
/// `&mut self` on the scrollback pair is not an accident: an engine whose
/// history is reachable only by moving a view offset cannot offer it any other
/// way, and `ui::scrollback` is built around exactly that. An engine that
/// addresses history by coordinate satisfies these trivially.
pub trait TerminalScreen {
    /// `(rows, cols)`.
    fn size(&self) -> (u16, u16);
    /// `(row, col)` of the cursor.
    fn cursor_position(&self) -> (u16, u16);
    /// Child asked to hide the cursor (DECTCEM `?25l`).
    fn hide_cursor(&self) -> bool;
    /// Child is on the xterm alternate screen (`?1049h` / `?47h`).
    fn alternate_screen(&self) -> bool;
    /// Child enabled bracketed paste (DECSET 2004).
    fn bracketed_paste(&self) -> bool;
    /// Child switched cursor keys to application mode (DECCKM `?1h`).
    fn application_cursor(&self) -> bool;
    /// Mouse reporting the child asked for, and in which encoding.
    fn mouse_protocol(&self) -> (MouseMode, MouseEncoding);
    /// Style of one cell, or `None` past the grid edge.
    fn cell_style(&self, row: u16, col: u16) -> Option<CellStyle>;
    /// Append one cell's text to `out`. Returns `false` past the grid edge.
    ///
    /// Appends nothing for a blank cell, so a caller's blank-to-space policy
    /// stays the caller's.
    fn cell_text(&self, row: u16, col: u16, out: &mut String) -> bool;

    /// Visible screen as plain text, one line per row.
    fn contents(&self) -> String;
    /// Plain text between two positions.
    ///
    /// Honouring soft wraps is the implementation's job, not the caller's:
    /// `Pane::selection_text` needs a newline at a hard line end and none at a
    /// wrap, and an engine knows which its rows are. There is deliberately no
    /// `row_wrapped` on this trait — nothing in spyc asks the question
    /// directly, and a seam carrying methods no caller uses is speculative
    /// surface that the next engine has to satisfy for nobody.
    fn contents_between(
        &self,
        start_row: u16,
        start_col: u16,
        end_row: u16,
        end_col: u16,
    ) -> String;

    /// Current scrollback view offset in rows.
    fn scrollback(&self) -> usize;
    /// Move the scrollback view. Implementations clamp to the real length, so
    /// asking for `usize::MAX` and reading back is how the length is
    /// discovered.
    fn set_scrollback(&mut self, rows: usize);
}

/// A VT state machine: bytes in, a screen out.
///
/// `Send` because `pane::Pane` hands the parser to a dedicated worker thread
/// and locks it from the render pass. An engine that cannot satisfy this
/// forces the pane to a different threading shape, which is a decision for the
/// implementation swap rather than something to paper over here.
pub trait Engine: Send {
    /// The screen type this engine exposes.
    type Screen: TerminalScreen;

    /// Build an engine at `rows` x `cols` with a scrollback budget in **rows**.
    fn new(rows: u16, cols: u16, scrollback_rows: usize) -> Self
    where
        Self: Sized;
    /// Feed bytes from the child.
    fn process(&mut self, bytes: &[u8]);
    fn screen(&self) -> &Self::Screen;
    fn screen_mut(&mut self) -> &mut Self::Screen;
}
