//! Is #34 an engine defect or an adapter defect?
//!
//! spyc's `mod pane` and `mod ui` are private in `src/lib.rs` (only
//! `pub mod fuzz` is exported), so this cannot link `PaneWidget` or
//! `lines_from_scrollback`. Both are TRANSCRIBED VERBATIM below from
//! `src/pane/widget.rs` and `src/ui/scrollback.rs` at the commit under test,
//! against the same ratatui 0.30 spyc builds with. If the production code
//! changes, this transcription goes stale — which is exactly why the report
//! recommends exposing them through the `fuzz` facade if this becomes a
//! permanent test.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

// ---- transcribed from src/pane/widget.rs ------------------------------------

fn convert_color(c: vt100::Color) -> Color {
    match c {
        vt100::Color::Default => Color::Reset,
        vt100::Color::Idx(i) => Color::Indexed(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

fn cell_style(cell: &vt100::Cell) -> Style {
    let mut style = Style::default();
    style = style.fg(convert_color(cell.fgcolor()));
    style = style.bg(convert_color(cell.bgcolor()));
    if cell.bold() {
        style = style.add_modifier(Modifier::BOLD);
    }
    if cell.italic() {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if cell.underline() {
        style = style.add_modifier(Modifier::UNDERLINED);
    }
    if cell.inverse() {
        style = style.add_modifier(Modifier::REVERSED);
    }
    style
}

/// The cell walk from `impl Widget for PaneWidget`, verbatim apart from the
/// selection/cursor overlay (not relevant to this question).
fn paint(screen: &vt100::Screen, area: Rect, buf: &mut Buffer) {
    let (screen_rows, screen_cols) = screen.size();
    let draw_rows = area.height.min(screen_rows);
    let draw_cols = area.width.min(screen_cols);
    for row in 0..draw_rows {
        for col in 0..draw_cols {
            let Some(cell) = screen.cell(row, col) else {
                continue;
            };
            let contents = cell.contents();
            let ch: &str = if contents.is_empty() { " " } else { contents };
            let style = cell_style(cell);
            let x = area.x + col;
            let y = area.y + row;
            buf.set_string(x, y, ch, style);
        }
    }
}

// ----------------------------------------------------------------------------

fn buffer_row(buf: &Buffer, row: u16, cols: u16) -> String {
    (0..cols)
        .map(|c| {
            buf.cell((c, row))
                .map(|cc| cc.symbol().to_string())
                .unwrap_or_default()
        })
        .collect()
}

fn probe(label: &str, bytes: &[u8], rows: u16, cols: u16) {
    let mut p = vt100::Parser::new(rows, cols, 100);
    p.process(bytes);
    let screen = p.screen();

    let area = Rect::new(0, 0, cols, rows);
    let mut buf = Buffer::empty(area);
    paint(screen, area, &mut buf);

    println!("--- {label}");
    println!("    engine row 0 (vt100 contents, continuations skipped): {:?}", {
        let mut t = String::new();
        for c in 0..cols {
            match screen.cell(0, c) {
                Some(cell) if cell.is_wide_continuation() => {}
                Some(cell) if cell.contents().is_empty() => t.push(' '),
                Some(cell) => t.push_str(cell.contents()),
                None => break,
            }
        }
        t.trim_end().to_string()
    });
    println!("    adapter row 0 (what PaneWidget put in the ratatui buffer):  {:?}", buffer_row(&buf, 0, cols).trim_end());
    // Per-cell dump so a clobbered wide half is visible rather than inferred.
    let mut per_cell = Vec::new();
    for c in 0..cols.min(12) {
        let eng = screen.cell(0, c);
        let sym = buf.cell((c, 0)).map(|x| x.symbol().to_string()).unwrap_or_default();
        per_cell.push(format!(
            "{c}:{}{}->{sym:?}",
            eng.map_or("-".into(), |e| format!("{:?}", e.contents())),
            match eng {
                Some(e) if e.is_wide() => "[W]",
                Some(e) if e.is_wide_continuation() => "[T]",
                _ => "",
            }
        ));
    }
    println!("    per-cell: {}", per_cell.join("  "));
}

fn main() {
    println!("spyc adapter probe: vt100 {} + ratatui 0.30", env!("VT100_VERSION"));
    println!("engine column = what vt100 holds; adapter column = what PaneWidget writes");
    println!("{:=<110}", "");

    probe("plain ascii", b"abcdef", 3, 12);
    probe("double-width CJK", "\u{3042}\u{3044}ab".as_bytes(), 3, 12);
    probe("wide char at last column", "xxxxxxxxxx\u{3042}".as_bytes(), 3, 12);
    probe("ZWJ emoji (>18 content bytes)", "\u{1F3F4}\u{E0067}\u{E0062}\u{E0073}\u{E0063}\u{E0074}\u{E007F}x".as_bytes(), 3, 12);
    probe("DEC special graphics (SCS)", b"\x1b(0lqqqk\x1b(B", 3, 12);
    probe("SGR 2 dim", b"\x1b[2mdim\x1b[0m", 3, 12);

    // The dim attribute exists on the engine in 0.16 but `cell_style` never
    // reads it, so state it explicitly rather than leaving it to the eye.
    let mut p = vt100::Parser::new(3, 12, 0);
    p.process(b"\x1b[2mdim");
    let c = p.screen().cell(0, 0).expect("cell 0,0 exists");
    println!();
    println!(
        "dim attribute: engine reports cell.dim()={}, adapter's cell_style() emits DIM modifier={}",
        c.dim(),
        cell_style(c).add_modifier.contains(Modifier::DIM)
    );
}
