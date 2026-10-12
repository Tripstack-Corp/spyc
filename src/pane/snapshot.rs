//! An owned visible grid for a mouse drag; the live parser keeps consuming output.

use super::engine::{CellStyle, MouseEncoding, MouseMode, TerminalScreen, Wide};

const MAX_CELLS: usize = 262_144;
const MAX_TEXT_BYTES: usize = 16 * 1024 * 1024;

struct Cell {
    style: CellStyle,
    start: usize,
    end: usize,
}

/// A bounded, immutable viewport with the same cell and wrap semantics as its source.
pub struct ScreenSnapshot {
    rows: u16,
    cols: u16,
    cells: Vec<Cell>,
    text: String,
    hard_ends: Vec<bool>,
    pad_blanks: bool,
    cursor: (u16, u16),
    hidden: bool,
    alternate: bool,
    paste: bool,
    app_cursor: bool,
    mouse: (MouseMode, MouseEncoding),
}

impl ScreenSnapshot {
    pub(crate) fn capture(screen: &impl TerminalScreen) -> Option<Self> {
        let (rows, cols) = screen.size();
        let count = usize::from(rows).checked_mul(usize::from(cols))?;
        if count == 0 || count > MAX_CELLS {
            return None;
        }
        let mut cells = Vec::with_capacity(count);
        let mut text = String::new();
        let mut hard_ends = Vec::with_capacity(usize::from(rows));
        for row in 0..rows {
            for col in 0..cols {
                let style = screen.cell_style(row, col)?;
                let start = text.len();
                screen.cell_text(row, col, &mut text);
                if text.len() > MAX_TEXT_BYTES {
                    return None;
                }
                cells.push(Cell {
                    style,
                    start,
                    end: text.len(),
                });
            }
            hard_ends.push(!screen.row_wrapped(row));
        }
        Some(Self {
            rows,
            cols,
            cells,
            text,
            hard_ends,
            pad_blanks: screen.copy_pads_blank_cells(),
            cursor: screen.cursor_position(),
            hidden: screen.hide_cursor(),
            alternate: screen.alternate_screen(),
            paste: screen.bracketed_paste(),
            app_cursor: screen.application_cursor(),
            mouse: screen.mouse_protocol(),
        })
    }

    fn write_row(&self, out: &mut String, row: u16, first: u16, last: u16) {
        let mut next_col = first;
        let mut skip_tail = false;
        for col in first..last {
            let Some(cell) = self.cell(row, col) else {
                break;
            };
            if self.pad_blanks {
                if cell.start != cell.end {
                    out.push_str(&self.text[cell.start..cell.end]);
                } else if cell.style.wide != Wide::Tail {
                    out.push(' ');
                }
                continue;
            }
            // #453 tracks removal of this branch with the vt100 fallback.
            // vt100 emits padding only when text follows it, including a range
            // beginning on a wide continuation. A wide head consumes its tail.
            if skip_tail {
                skip_tail = false;
                continue;
            }
            skip_tail = cell.style.wide == Wide::Head;
            if cell.start != cell.end {
                for _ in next_col..col {
                    out.push(' ');
                }
                out.push_str(&self.text[cell.start..cell.end]);
                next_col = col.saturating_add(if skip_tail { 2 } else { 1 });
            }
        }
    }

    fn cell(&self, row: u16, col: u16) -> Option<&Cell> {
        if row >= self.rows || col >= self.cols {
            return None;
        }
        self.cells
            .get(usize::from(row) * usize::from(self.cols) + usize::from(col))
    }
}

impl TerminalScreen for ScreenSnapshot {
    fn size(&self) -> (u16, u16) {
        (self.rows, self.cols)
    }
    fn cursor_position(&self) -> (u16, u16) {
        self.cursor
    }
    fn hide_cursor(&self) -> bool {
        self.hidden
    }
    fn alternate_screen(&self) -> bool {
        self.alternate
    }
    fn bracketed_paste(&self) -> bool {
        self.paste
    }
    fn application_cursor(&self) -> bool {
        self.app_cursor
    }
    fn mouse_protocol(&self) -> (MouseMode, MouseEncoding) {
        self.mouse
    }
    fn cell_style(&self, row: u16, col: u16) -> Option<CellStyle> {
        self.cell(row, col).map(|c| c.style)
    }
    fn cell_text(&self, row: u16, col: u16, out: &mut String) -> bool {
        let Some(cell) = self.cell(row, col) else {
            return false;
        };
        out.push_str(&self.text[cell.start..cell.end]);
        true
    }
    fn contents(&self) -> String {
        (0..self.rows)
            .map(|row| {
                self.contents_between(row, 0, row, self.cols)
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
    fn contents_between(&self, sr: u16, sc: u16, er: u16, ec: u16) -> String {
        let mut out = String::new();
        for row in sr..=er.min(self.rows.saturating_sub(1)) {
            let first = if row == sr { sc } else { 0 };
            let last = if row == er { ec } else { self.cols };
            self.write_row(&mut out, row, first, last.min(self.cols));
            if row != er && self.hard_ends[usize::from(row)] {
                out.push('\n');
            }
        }
        out
    }
    fn row_wrapped(&self, row: u16) -> bool {
        self.hard_ends
            .get(usize::from(row))
            .is_some_and(|hard| !hard)
    }
    fn copy_pads_blank_cells(&self) -> bool {
        self.pad_blanks
    }
    // A snapshot contains its visible rows only, with no navigable history.
    fn scrollback(&self) -> usize {
        0
    }
    fn set_scrollback(&mut self, _: usize) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pane::engine::Engine;

    fn extraction<E: Engine>() {
        let mut engine = E::new(5, 10, 20);
        engine
            .process("\x1b[?1049h\x1b[?25l\x1b[31m0123456789abcdef\r\nhard 界 e\u{301}".as_bytes());
        let snapshot = ScreenSnapshot::capture(engine.screen()).unwrap();
        // Both subjects pass through the same normalization used by mouse copy.
        for sr in 0..5 {
            for er in sr..5 {
                for sc in 0..10 {
                    for ec in 0..10 {
                        if sr == er && sc > ec {
                            continue;
                        }
                        assert_eq!(
                            crate::pane::widget::selection_text(&snapshot, (sr, sc), (er, ec)),
                            crate::pane::widget::selection_text(
                                engine.screen(),
                                (sr, sc),
                                (er, ec)
                            ),
                            "range {sr}:{sc}..{er}:{ec}",
                        );
                    }
                }
            }
        }
        engine.process(b"\x1b[2J\x1b[Hchanged");
        assert!(snapshot.contents().contains("0123456789"));
        assert!(!snapshot.contents().contains("changed"));
    }

    #[test]
    fn ghostty_snapshot_preserves_wraps_wide_cells_and_combining_text() {
        extraction::<crate::pane::engine_ghostty::GhosttyEngine>();
    }
    #[test]
    fn vt100_snapshot_preserves_wraps_wide_cells_and_combining_text() {
        extraction::<vt100::Parser>();
    }
    struct Probe {
        engine: crate::pane::engine_ghostty::GhosttyEngine,
        size: (u16, u16),
        reads: std::cell::Cell<usize>,
        large_text: bool,
    }
    impl Probe {
        fn new(size: (u16, u16), large_text: bool) -> Self {
            Self {
                engine: Engine::new(2, 3, 0),
                size,
                reads: std::cell::Cell::new(0),
                large_text,
            }
        }
    }
    impl TerminalScreen for Probe {
        fn size(&self) -> (u16, u16) {
            self.size
        }
        fn cursor_position(&self) -> (u16, u16) {
            self.engine.screen().cursor_position()
        }
        fn hide_cursor(&self) -> bool {
            self.engine.screen().hide_cursor()
        }
        fn alternate_screen(&self) -> bool {
            self.engine.screen().alternate_screen()
        }
        fn bracketed_paste(&self) -> bool {
            self.engine.screen().bracketed_paste()
        }
        fn application_cursor(&self) -> bool {
            self.engine.screen().application_cursor()
        }
        fn mouse_protocol(&self) -> (MouseMode, MouseEncoding) {
            self.engine.screen().mouse_protocol()
        }
        fn cell_style(&self, row: u16, col: u16) -> Option<CellStyle> {
            self.reads.set(self.reads.get() + 1);
            self.engine.screen().cell_style(row, col)
        }
        fn cell_text(&self, row: u16, col: u16, out: &mut String) -> bool {
            if self.large_text && row == 0 && col == 0 {
                out.push_str(&"x".repeat(MAX_TEXT_BYTES + 1));
                true
            } else {
                self.engine.screen().cell_text(row, col, out)
            }
        }
        fn contents(&self) -> String {
            self.engine.screen().contents()
        }
        fn contents_between(&self, sr: u16, sc: u16, er: u16, ec: u16) -> String {
            self.engine.screen().contents_between(sr, sc, er, ec)
        }
        fn row_wrapped(&self, row: u16) -> bool {
            self.engine.screen().row_wrapped(row)
        }
        fn copy_pads_blank_cells(&self) -> bool {
            self.engine.screen().copy_pads_blank_cells()
        }
        fn scrollback(&self) -> usize {
            0
        }
        fn set_scrollback(&mut self, _: usize) {}
    }

    #[test]
    fn oversized_geometry_is_refused_before_any_cell_read() {
        let probe = Probe::new((513, 512), false);
        assert!(ScreenSnapshot::capture(&probe).is_none());
        assert_eq!(probe.reads.get(), 0);
    }
    #[test]
    fn oversized_text_stops_capture_at_the_first_cell() {
        let probe = Probe::new((2, 3), true);
        assert!(ScreenSnapshot::capture(&probe).is_none());
        assert_eq!(probe.reads.get(), 1);
    }
}
