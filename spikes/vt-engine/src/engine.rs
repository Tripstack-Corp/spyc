//! The one screen model all candidates are normalized into.
//!
//! Deliberately the *intersection* of what spyc actually reads today
//! (`src/pane/widget.rs` cell walk, `src/ui/scrollback.rs` span merge,
//! `Pane::with_screen` predicates), not the union of what the engines can do.
//! A differential comparison is only meaningful over a model every candidate
//! can populate faithfully; anything richer would report engine A's extra
//! capability as engine B's divergence.

/// A colour in the model spyc's renderer consumes (`convert_color` in
/// `src/pane/widget.rs` maps exactly these three cases onto ratatui).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Color {
    #[default]
    Default,
    Idx(u8),
    Rgb(u8, u8, u8),
}

/// Wide-character role. spyc's widget writes `" "` for an empty cell, so it
/// has to know a continuation cell from a genuinely blank one.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Wide {
    #[default]
    Narrow,
    /// First half of a double-width glyph; carries the text.
    Head,
    /// Second half; carries no text of its own.
    Tail,
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Cell {
    /// The grapheme cluster this cell draws. Empty for a blank cell and for a
    /// wide continuation.
    pub text: String,
    pub fg: Color,
    pub bg: Color,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub reverse: bool,
    pub wide: Wide,
}

impl Cell {
    /// True when the cell would draw as blank with default attributes — the
    /// state the grid is padded to. Used to trim so that "engine A padded the
    /// row and engine B left it short" isn't reported as a content divergence.
    pub fn is_blank(&self) -> bool {
        (self.text.is_empty() || self.text == " ")
            && self.fg == Color::Default
            && self.bg == Color::Default
            && !self.bold
            && !self.italic
            && !self.underline
            && !self.reverse
    }
}

/// A normalized snapshot of one engine's visible screen.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Screen {
    pub rows: u16,
    pub cols: u16,
    /// `(row, col)`, matching `vt100::Screen::cursor_position`'s order.
    pub cursor: (u16, u16),
    pub cursor_visible: bool,
    pub alt_screen: bool,
    /// Row-major, `rows * cols` entries.
    pub cells: Vec<Cell>,
    /// Per row: did this line soft-wrap into the next? `src/pane/mod.rs`'s
    /// `selection_text` depends on this to avoid a spurious newline mid-line.
    pub wrapped: Vec<bool>,
}

impl Screen {
    pub fn cell(&self, row: u16, col: u16) -> Option<&Cell> {
        if row >= self.rows || col >= self.cols {
            return None;
        }
        self.cells.get(usize::from(row) * usize::from(self.cols) + usize::from(col))
    }

    /// One row as text, trailing blanks trimmed — the same normalization
    /// `src/ui/scrollback.rs` applies before handing a line to the pager.
    pub fn row_text(&self, row: u16) -> String {
        let mut s = String::new();
        for col in 0..self.cols {
            match self.cell(row, col) {
                Some(c) if c.wide == Wide::Tail => {}
                Some(c) if c.text.is_empty() => s.push(' '),
                Some(c) => s.push_str(&c.text),
                None => break,
            }
        }
        s.trim_end().to_string()
    }

    pub fn text(&self) -> Vec<String> {
        (0..self.rows).map(|r| self.row_text(r)).collect()
    }
}

/// What the spike needs from a candidate engine.
///
/// `&mut self` on the readers is not an accident: vt100 can only reach its
/// scrollback by mutating the view offset (see the module docs of
/// `src/ui/scrollback.rs`), so a shared-reference reader would exclude the
/// incumbent from its own comparison.
pub trait Engine {
    fn name(&self) -> &'static str;
    fn create(rows: u16, cols: u16, scrollback: usize) -> Self
    where
        Self: Sized;
    fn feed(&mut self, bytes: &[u8]);
    fn resize(&mut self, rows: u16, cols: u16);
    fn screen(&mut self) -> Screen;

    /// Rows of history above the visible screen.
    fn scrollback_rows(&mut self) -> usize;

    /// Bytes that, replayed into a fresh engine of the same geometry, should
    /// reproduce this state. `None` when the engine exposes no such API.
    fn rehydrate(&mut self) -> Option<Vec<u8>>;

    /// Whole buffer as plain text, scrollback first. `None` when the engine
    /// cannot address history at all.
    fn full_text(&mut self) -> Option<Vec<String>>;
}
