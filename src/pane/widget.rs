//! ratatui widget that draws a `vt100::Screen` into a frame.
//!
//! Each cell becomes a single styled character in the buffer. We map
//! vt100's color model onto ratatui's, preserving bold / italic /
//! underline / reverse.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::Widget,
};

pub struct PaneWidget<'a> {
    pub screen: &'a vt100::Screen,
    pub focused: bool,
}

impl Widget for PaneWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let (screen_rows, screen_cols) = self.screen.size();
        let draw_rows = area.height.min(screen_rows);
        let draw_cols = area.width.min(screen_cols);

        for row in 0..draw_rows {
            for col in 0..draw_cols {
                let Some(cell) = self.screen.cell(row, col) else {
                    continue;
                };
                let contents = cell.contents();
                let ch: &str = if contents.is_empty() { " " } else { &contents };
                let style = cell_style(cell);
                let x = area.x + col;
                let y = area.y + row;
                buf.set_string(x, y, ch, style);
            }
        }

        // Overlay a cursor block at the pty cursor position.
        //  - Focused: bright reverse-video block.
        //  - Unfocused: static dim reverse-video block.
        {
            let (cy, cx) = self.screen.cursor_position();
            if cy < draw_rows && cx < draw_cols {
                let x = area.x + cx;
                let y = area.y + cy;
                if let Some(cell_ref) = buf.cell_mut((x, y)) {
                    let mut s = cell_ref.style().add_modifier(Modifier::REVERSED);
                    if !self.focused {
                        s = s.add_modifier(Modifier::DIM);
                    }
                    cell_ref.set_style(s);
                }
            }
        }
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

const fn convert_color(c: vt100::Color) -> Color {
    match c {
        vt100::Color::Default => Color::Reset,
        vt100::Color::Idx(i) => Color::Indexed(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}
