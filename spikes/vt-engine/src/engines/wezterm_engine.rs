//! Candidate B adapter: wezterm-term.
//!
//! Reached through a **git dependency**, because `wezterm-term` is not
//! published to crates.io at any version and its manifest uses
//! `dependency.workspace = true` throughout, so it only resolves inside
//! wezterm's own workspace. A crate with a git dependency cannot be published
//! to crates.io — which is the whole distribution question for this option,
//! settled before any fidelity number is measured.

use wezterm_surface::CursorVisibility;
use wezterm_term::color::ColorAttribute;
use wezterm_term::{Intensity, Terminal, TerminalConfiguration, TerminalSize, Underline};

use crate::engine::{Cell, Color, Engine, Screen, Wide};

#[derive(Debug)]
struct Config {
    /// wezterm takes its scrollback budget from the configuration object, not
    /// from a constructor argument or a setter.
    scrollback: usize,
}

impl TerminalConfiguration for Config {
    fn color_palette(&self) -> wezterm_term::color::ColorPalette {
        wezterm_term::color::ColorPalette::default()
    }
    fn scrollback_size(&self) -> usize {
        self.scrollback
    }
}

/// Sink for the terminal's replies (DA/DSR/etc). The spike answers no queries,
/// matching how the other two adapters are driven.
struct Sink;
impl std::io::Write for Sink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub struct WeztermEngine {
    term: Terminal,
}

fn color(c: ColorAttribute) -> Color {
    match c {
        ColorAttribute::Default => Color::Default,
        ColorAttribute::PaletteIndex(i) => Color::Idx(i),
        ColorAttribute::TrueColorWithPaletteFallback(sr, _)
        | ColorAttribute::TrueColorWithDefaultFallback(sr) => {
            let (r, g, b, _) = sr.to_srgb_u8();
            Color::Rgb(r, g, b)
        }
    }
}

impl Engine for WeztermEngine {
    fn name(&self) -> &'static str {
        "wezterm"
    }

    fn create(rows: u16, cols: u16, scrollback: usize) -> Self {
        let size = TerminalSize {
            rows: rows as usize,
            cols: cols as usize,
            pixel_width: 0,
            pixel_height: 0,
            dpi: 0,
        };
        let term = Terminal::new(
            size,
            std::sync::Arc::new(Config { scrollback }),
            "vt-engine-spike",
            "0.0.0",
            Box::new(Sink),
        );
        Self { term }
    }

    fn feed(&mut self, bytes: &[u8]) {
        self.term.advance_bytes(bytes);
    }

    fn resize(&mut self, rows: u16, cols: u16) {
        self.term.resize(TerminalSize {
            rows: rows as usize,
            cols: cols as usize,
            pixel_width: 0,
            pixel_height: 0,
            dpi: 0,
        });
    }

    fn screen(&mut self) -> Screen {
        let size = self.term.get_size();
        let rows = size.rows as u16;
        let cols = size.cols as u16;
        let cursor = self.term.cursor_pos();
        let alt = self.term.is_alt_screen_active();
        // `visible_lines()` / `all_lines()` are `#[cfg(test)]` in wezterm-term,
        // so an embedder reads rows through the physical-range API instead.
        let visible = {
            let screen = self.term.screen();
            let phys = screen.phys_range(&(0..i64::from(rows)));
            screen.lines_in_phys_range(phys)
        };

        let mut cells = Vec::with_capacity(usize::from(rows) * usize::from(cols));
        let mut wrapped = Vec::with_capacity(usize::from(rows));
        for y in 0..rows {
            let line = visible.get(usize::from(y));
            let mut pushed = 0u16;
            if let Some(line) = line {
                wrapped.push(line.last_cell_was_wrapped());
                for c in line.visible_cells() {
                    // `visible_cells` skips continuation cells, so pad them in
                    // to keep the model's fixed rows*cols shape.
                    while pushed < c.cell_index() as u16 && pushed < cols {
                        cells.push(Cell {
                            wide: Wide::Tail,
                            ..Cell::default()
                        });
                        pushed += 1;
                    }
                    if pushed >= cols {
                        break;
                    }
                    let a = c.attrs();
                    let w = c.width();
                    cells.push(Cell {
                        text: {
                            let s = c.str();
                            if s == " " { String::new() } else { s.to_string() }
                        },
                        fg: color(a.foreground()),
                        bg: color(a.background()),
                        bold: a.intensity() == Intensity::Bold,
                        italic: a.italic(),
                        underline: a.underline() != Underline::None,
                        reverse: a.reverse(),
                        wide: if w > 1 { Wide::Head } else { Wide::Narrow },
                    });
                    pushed += 1;
                }
            } else {
                wrapped.push(false);
            }
            while pushed < cols {
                cells.push(Cell::default());
                pushed += 1;
            }
        }

        Screen {
            rows,
            cols,
            cursor: (cursor.y as u16, cursor.x as u16),
            cursor_visible: cursor.visibility == CursorVisibility::Visible,
            alt_screen: alt,
            cells,
            wrapped,
        }
    }

    fn scrollback_rows(&mut self) -> usize {
        let size = self.term.get_size();
        self.term
            .screen()
            .scrollback_rows()
            .saturating_sub(size.rows)
    }

    fn rehydrate(&mut self) -> Option<Vec<u8>> {
        // wezterm-term has no "emit escapes to reproduce my state" entry point
        // of its own; reconstruction goes through termwiz's `Surface` diffing,
        // which is a different shape (and a different crate). Reported as
        // absent rather than approximated, so the matrix is not flattered.
        None
    }

    fn full_text(&mut self) -> Option<Vec<String>> {
        // `all_lines` is scrollback + visible in one pass, oldest first — no
        // view-offset mutation, unlike the vt100 route.
        let screen = self.term.screen();
        let total = screen.scrollback_rows();
        Some(
            screen
                .lines_in_phys_range(0..total)
                .iter()
                .map(|l| l.as_str().trim_end().to_string())
                .collect(),
        )
    }
}
