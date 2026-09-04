//! Candidate C adapter: libghostty-vt via `libghostty-vt-sys`.
//!
//! Reads cells through `Terminal::grid_ref(Point)`. That is the API difference
//! that matters most for 3.0: `Point::Viewport` addresses the visible screen
//! and `Point::Screen` / `Point::History` address scrollback by coordinate, so
//! reading history needs no view-offset mutation of the kind
//! `src/ui/scrollback.rs` is built around.

use libghostty_vt::screen::CellWide;
use libghostty_vt::style::StyleColor;
use libghostty_vt::terminal::{Point, PointCoordinate};
use libghostty_vt::{Terminal, TerminalOptions};

use crate::engine::{Cell, Color, Engine, Screen, Wide};

pub struct GhosttyEngine {
    term: Terminal<'static, 'static>,
    rows: u16,
    cols: u16,
}

fn color(c: StyleColor) -> Color {
    match c {
        StyleColor::None => Color::Default,
        StyleColor::Palette(i) => Color::Idx(i.0),
        StyleColor::Rgb(rgb) => Color::Rgb(rgb.r, rgb.g, rgb.b),
    }
}

impl GhosttyEngine {
    /// One cell at a coordinate in the given point space.
    fn read_cell(&self, point: Point) -> Cell {
        let Ok(gr) = self.term.grid_ref(point) else {
            return Cell::default();
        };
        let mut buf = [char::from(0u8); 32];
        let text = match gr.graphemes(&mut buf) {
            Ok(n) => buf[..n].iter().collect::<String>(),
            Err(_) => String::new(),
        };
        let wide = match gr.cell().and_then(|c| c.wide()) {
            Ok(CellWide::Wide) => Wide::Head,
            Ok(CellWide::SpacerTail) => Wide::Tail,
            // `SpacerHead` is ghostty's marker for the blank cell left at a
            // row end that cannot fit a wide glyph — narrow for our purposes.
            _ => Wide::Narrow,
        };
        let st = gr.style().unwrap_or_default();
        Cell {
            // A `Spacer_tail` carries no glyph of its own; normalize it to
            // empty so the comparison with vt100 (which reports an empty
            // continuation cell) is like-for-like.
            text: if wide == Wide::Tail { String::new() } else { text },
            fg: color(st.fg_color),
            bg: color(st.bg_color),
            bold: st.bold,
            italic: st.italic,
            underline: st.underline != libghostty_vt::style::Underline::None,
            reverse: st.inverse,
            wide,
        }
    }
}

impl Engine for GhosttyEngine {
    fn name(&self) -> &'static str {
        "ghostty"
    }

    fn create(rows: u16, cols: u16, scrollback: usize) -> Self {
        let term = Terminal::new(TerminalOptions {
            cols,
            rows,
            max_scrollback: scrollback,
        })
        .expect("libghostty terminal allocation");
        Self { term, rows, cols }
    }

    fn feed(&mut self, bytes: &[u8]) {
        self.term.vt_write(bytes);
    }

    fn resize(&mut self, rows: u16, cols: u16) {
        // Cell pixel metrics only matter to image protocols and size reports;
        // the spike drives no graphics client, so a fixed 8x16 is fine.
        let _ = self.term.resize(cols, rows, 8, 16);
        self.rows = rows;
        self.cols = cols;
    }

    fn screen(&mut self) -> Screen {
        let rows = self.term.rows().unwrap_or(self.rows);
        let cols = self.term.cols().unwrap_or(self.cols);
        let mut cells = Vec::with_capacity(usize::from(rows) * usize::from(cols));
        let mut wrapped = Vec::with_capacity(usize::from(rows));
        for y in 0..rows {
            for x in 0..cols {
                cells.push(self.read_cell(Point::Viewport(PointCoordinate { x, y: u32::from(y) })));
            }
            let w = self
                .term
                .grid_ref(Point::Viewport(PointCoordinate { x: 0, y: u32::from(y) }))
                .and_then(|gr| gr.row())
                .and_then(|r| r.is_wrapped())
                .unwrap_or(false);
            wrapped.push(w);
        }
        Screen {
            rows,
            cols,
            cursor: (
                self.term.cursor_y().unwrap_or(0),
                self.term.cursor_x().unwrap_or(0),
            ),
            cursor_visible: self.term.is_cursor_visible().unwrap_or(true),
            // `Screen::Alternate` is the discriminant; compare rather than
            // reaching for a bool the API doesn't expose.
            alt_screen: matches!(
                self.term.active_screen(),
                Ok(libghostty_vt::screen::Screen::Alternate)
            ),
            cells,
            wrapped,
        }
    }

    fn scrollback_rows(&mut self) -> usize {
        self.term.scrollback_rows().unwrap_or(0)
    }

    fn rehydrate(&mut self) -> Option<Vec<u8>> {
        use libghostty_vt::fmt::{Format, Formatter, FormatterOptions};
        // The set that maximizes RECONSTRUCTION, which is not the set that
        // turns everything on:
        //
        //   * `with_palette(true)` emits all 256 OSC 4 entries — a fixed
        //     5,522 bytes measured by `src/bin/gfmt.rs`. That restores the
        //     *client's* palette, which is a capability choice, not terminal
        //     state, so it is off here and priced separately in the report.
        //   * `with_tabstops(true)` is OFF because it is BUGGY: the emit
        //     places the cursor restore BEFORE the tab-stop reconstruction,
        //     and setting a tab stop moves the cursor (`CSI 9 G`, `HTS`), so
        //     the restored cursor is left parked at the last tab column.
        //     Demonstrated by `src/bin/grt.rs`: the same state round-trips to
        //     cursor (3,0) without it and (3,16) with it.
        let opts = FormatterOptions::new()
            .with_format(Format::Vt)
            .with_modes(true)
            .with_scrolling_region(true)
            .with_cursor(true)
            .with_style(true)
            .with_hyperlink(true)
            .with_protection(true)
            .with_kitty_keyboard(true)
            .with_charsets(true);
        let mut f = Formatter::new(&self.term, opts).ok()?;
        let len = f.format_len().ok()?;
        let mut buf = vec![0u8; len];
        let n = f.format_buf(&mut buf).ok()?;
        buf.truncate(n);
        Some(buf)
    }

    fn full_text(&mut self) -> Option<Vec<String>> {
        let cols = self.term.cols().unwrap_or(self.cols);
        let total = self.term.total_rows().unwrap_or(0);
        let mut out = Vec::with_capacity(total);
        for y in 0..total {
            let y32 = u32::try_from(y).ok()?;
            let mut line = String::new();
            for x in 0..cols {
                let c = self.read_cell(Point::Screen(PointCoordinate { x, y: y32 }));
                match c.wide {
                    Wide::Tail => {}
                    _ if c.text.is_empty() => line.push(' '),
                    _ => line.push_str(&c.text),
                }
            }
            out.push(line.trim_end().to_string());
        }
        Some(out)
    }
}
