//! Link one engine, drive it enough that the linker cannot drop it, print a
//! digest of the result so nothing is dead code.
fn main() {
    let bytes = b"\x1b[31mhello\x1b[0m\r\n\x1b[?1049h\x1b[2Jalt\x1b[?1049l\xe3\x81\x82";
    let mut acc: u64 = 0;

    #[cfg(feature = "e-vt100")]
    {
        let mut p = vt100::Parser::new(24, 80, 10_000);
        p.process(bytes);
        let s = p.screen();
        for r in 0..24 {
            for c in 0..80 {
                if let Some(cell) = s.cell(r, c) {
                    acc = acc.wrapping_add(cell.contents().len() as u64);
                }
            }
        }
        acc = acc.wrapping_add(s.state_formatted().len() as u64);
    }

    #[cfg(feature = "e-ghostty")]
    {
        use libghostty_vt::terminal::{Point, PointCoordinate};
        use libghostty_vt::{Terminal, TerminalOptions};
        let mut t = Terminal::new(TerminalOptions { cols: 80, rows: 24, max_scrollback: 10_000 }).unwrap();
        t.vt_write(bytes);
        for y in 0..24u32 {
            for x in 0..80u16 {
                if let Ok(gr) = t.grid_ref(Point::Viewport(PointCoordinate { x, y })) {
                    let mut buf = [char::from(0u8); 8];
                    acc = acc.wrapping_add(gr.graphemes(&mut buf).unwrap_or(0) as u64);
                }
            }
        }
        acc = acc.wrapping_add(t.rows().unwrap_or(0) as u64);
    }

    #[cfg(feature = "e-wezterm")]
    {
        use wezterm_term::{Terminal, TerminalConfiguration, TerminalSize};
        #[derive(Debug)]
        struct C;
        impl TerminalConfiguration for C {
            fn color_palette(&self) -> wezterm_term::color::ColorPalette {
                wezterm_term::color::ColorPalette::default()
            }
        }
        struct W;
        impl std::io::Write for W {
            fn write(&mut self, b: &[u8]) -> std::io::Result<usize> { Ok(b.len()) }
            fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
        }
        let size = TerminalSize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0, dpi: 0 };
        let mut t = Terminal::new(size, std::sync::Arc::new(C), "p", "0", Box::new(W));
        t.advance_bytes(bytes);
        let sc = t.screen();
        let phys = sc.phys_range(&(0..24));
        for l in sc.lines_in_phys_range(phys) {
            acc = acc.wrapping_add(l.as_str().len() as u64);
        }
    }

    println!("{acc}");
}
