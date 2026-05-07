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

        // When the pane isn't the input target, fade the whole content
        // so the user can tell at a glance which side has focus. SGR 2
        // (Modifier::DIM) is the cheap way: every modern terminal we
        // support renders it as ~50% lightness on the affected cells.
        // Combined with the dimmed cursor block below, the unfocused
        // pane looks visibly muted vs. the focused list above.
        let dim = if self.focused {
            Modifier::empty()
        } else {
            Modifier::DIM
        };

        for row in 0..draw_rows {
            for col in 0..draw_cols {
                let Some(cell) = self.screen.cell(row, col) else {
                    continue;
                };
                let contents = cell.contents();
                let ch: &str = if contents.is_empty() { " " } else { contents };
                let style = cell_style(cell).add_modifier(dim);
                let x = area.x + col;
                let y = area.y + row;
                buf.set_string(x, y, ch, style);
            }
        }

        // Overlay a reverse-block cursor at the pty cursor position —
        // but only when spyc has business doing so:
        //
        // 1. Pane is focused. Otherwise the user's eye isn't here and a
        //    block in an unfocused pane is just visual clutter / a
        //    pseudo-second-cursor that competes with the real input
        //    target above (the file list).
        // 2. Child hasn't switched to the alternate screen. Full-screen
        //    TUIs (nvim, vim, less, htop, lazygit, claude in TUI mode)
        //    paint their own cursor in their own shape — beam in nvim
        //    insert mode, e.g. — and our hard-coded block clobbers it
        //    with the wrong shape and color.
        // 3. Child hasn't explicitly hidden the cursor (DEC ?25l).
        //
        // Net effect: a plain shell / REPL on the main screen still
        // gets the visibility cue (where the next char will land);
        // alt-screen TUIs and unfocused panes show their natural state.
        let want_block_cursor =
            self.focused && !self.screen.alternate_screen() && !self.screen.hide_cursor();
        if want_block_cursor {
            let (cy, cx) = self.screen.cursor_position();
            if cy < draw_rows && cx < draw_cols {
                let x = area.x + cx;
                let y = area.y + cy;
                if let Some(cell_ref) = buf.cell_mut((x, y)) {
                    let s = cell_ref.style().add_modifier(Modifier::REVERSED);
                    cell_ref.set_style(s);
                }
            }
        }
    }
}

pub fn cell_style(cell: &vt100::Cell) -> Style {
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

pub const fn convert_color(c: vt100::Color) -> Color {
    match c {
        vt100::Color::Default => Color::Reset,
        vt100::Color::Idx(i) => Color::Indexed(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}
