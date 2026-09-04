//! Incumbent adapter: vt100 0.16.
//!
//! `full_text` reproduces the technique in spyc's `src/ui/scrollback.rs` —
//! walk the visible window backwards by mutating `scrollback_offset`, because
//! vt100's scrollback is not iterable through the public API. That is not a
//! stylistic choice in the adapter; it is the only route the API offers.

use crate::engine::{Cell, Color, Engine, Screen, Wide};

pub struct Vt100Engine {
    parser: vt100::Parser,
}

fn color(c: vt100::Color) -> Color {
    match c {
        vt100::Color::Default => Color::Default,
        vt100::Color::Idx(i) => Color::Idx(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

impl Engine for Vt100Engine {
    fn name(&self) -> &'static str {
        "vt100"
    }

    fn create(rows: u16, cols: u16, scrollback: usize) -> Self {
        Self {
            parser: vt100::Parser::new(rows, cols, scrollback),
        }
    }

    fn feed(&mut self, bytes: &[u8]) {
        self.parser.process(bytes);
    }

    fn resize(&mut self, rows: u16, cols: u16) {
        self.parser.screen_mut().set_size(rows, cols);
    }

    fn screen(&mut self) -> Screen {
        let s = self.parser.screen();
        let (rows, cols) = s.size();
        let mut cells = Vec::with_capacity(usize::from(rows) * usize::from(cols));
        for row in 0..rows {
            for col in 0..cols {
                cells.push(match s.cell(row, col) {
                    Some(c) => Cell {
                        text: c.contents().to_string(),
                        fg: color(c.fgcolor()),
                        bg: color(c.bgcolor()),
                        bold: c.bold(),
                        italic: c.italic(),
                        underline: c.underline(),
                        reverse: c.inverse(),
                        wide: if c.is_wide() {
                            Wide::Head
                        } else if c.is_wide_continuation() {
                            Wide::Tail
                        } else {
                            Wide::Narrow
                        },
                    },
                    None => Cell::default(),
                });
            }
        }
        Screen {
            rows,
            cols,
            cursor: s.cursor_position(),
            cursor_visible: !s.hide_cursor(),
            alt_screen: s.alternate_screen(),
            cells,
            wrapped: (0..rows).map(|r| s.row_wrapped(r)).collect(),
        }
    }

    fn scrollback_rows(&mut self) -> usize {
        let s = self.parser.screen_mut();
        let saved = s.scrollback();
        s.set_scrollback(usize::MAX); // clamps; read back to discover the cap
        let len = s.scrollback();
        s.set_scrollback(saved);
        len
    }

    fn rehydrate(&mut self) -> Option<Vec<u8>> {
        // The whole rehydration API vt100 offers: visible screen contents plus
        // four input modes. No scrollback, no scroll region, no alt-screen
        // flag, no capability parameterization.
        Some(self.parser.screen().state_formatted())
    }

    fn full_text(&mut self) -> Option<Vec<String>> {
        let s = self.parser.screen_mut();
        let saved = s.scrollback();
        let (rows, _cols) = s.size();
        let rows_len = usize::from(rows);
        if rows_len == 0 {
            return Some(Vec::new());
        }
        s.set_scrollback(usize::MAX);
        let sb_len = s.scrollback();

        let mut out = Vec::with_capacity(sb_len + rows_len);
        let mut remaining = sb_len;
        while remaining > 0 {
            let chunk = remaining.min(rows_len);
            s.set_scrollback(remaining);
            for row in 0..chunk {
                out.push(row_text(s, row as u16));
            }
            remaining -= chunk;
        }
        s.set_scrollback(0);
        for row in 0..rows_len {
            out.push(row_text(s, row as u16));
        }
        s.set_scrollback(saved);
        Some(out)
    }
}

fn row_text(s: &vt100::Screen, row: u16) -> String {
    let (_rows, cols) = s.size();
    let mut t = String::new();
    for col in 0..cols {
        match s.cell(row, col) {
            Some(c) if c.is_wide_continuation() => {}
            Some(c) if c.contents().is_empty() => t.push(' '),
            Some(c) => t.push_str(c.contents()),
            None => break,
        }
    }
    t.trim_end().to_string()
}
