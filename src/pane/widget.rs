//! ratatui widget that draws a `vt100::Screen` into a frame.
//!
//! Each cell becomes a single styled character in the buffer. We map
//! vt100's colour model onto ratatui's, preserving bold / italic /
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
    /// A spyc-side text selection over the visible grid, as ordered
    /// `((start_row, start_col), (end_row, end_col))` in SCREEN coordinates.
    ///
    /// Only ever set for a child that ignores mouse reports — one that speaks mouse
    /// draws its own selection, and painting ours on top would double it up.
    pub selection: Option<((u16, u16), (u16, u16))>,
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
                let mut style = cell_style(cell).add_modifier(dim);
                if selected(self.selection, row, col) {
                    // Reverse rather than a theme bg: the pane's cells carry the
                    // CHILD's colours, which can be anything, so a fixed background
                    // would vanish against a child that happens to use it. Reverse
                    // is relative to whatever the cell already is.
                    style = style.add_modifier(Modifier::REVERSED);
                }
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
        //    with the wrong shape and colour.
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
    // SGR 2. vt100 exposed `dim()` in 0.16, after this function was written
    // against 0.15, so a child's dim text used to arrive here and be dropped.
    // Note the channel is shared: the unfocused-pane fade in `PaneWidget`
    // also spends `Modifier::DIM`, so on an unfocused pane content-dim and
    // focus-dim are indistinguishable. That is accepted — SGR 2 is what a
    // terminal has for "dimmer", and there is no second one to move either
    // use onto.
    if cell.dim() {
        style = style.add_modifier(Modifier::DIM);
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

/// Whether `(row, col)` falls inside a charwise selection.
///
/// Charwise, not rectangular: the first row runs from its start column to the end
/// of the line, interior rows are whole, and the last row stops at its end column.
/// A rectangle would be wrong for prose — selecting three lines of a paragraph
/// would clip them all to the same columns.
const fn selected(sel: Option<((u16, u16), (u16, u16))>, row: u16, col: u16) -> bool {
    let Some(((sr, sc), (er, ec))) = sel else {
        return false;
    };
    if row < sr || row > er {
        return false;
    }
    let after_start = row > sr || col >= sc;
    let before_end = row < er || col <= ec;
    after_start && before_end
}

#[cfg(test)]
mod attribute_tests {
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::Modifier;
    use ratatui::widgets::Widget as _;

    use super::PaneWidget;

    /// Render `bytes` through the real widget and hand back the buffer, so the
    /// assertions below are about the widget's OUTPUT rather than about what
    /// the emulator holds. Only this helper knows which engine produced the
    /// screen; the tests do not, which is what lets them survive an engine
    /// swap unchanged.
    fn rendered(bytes: &[u8], rows: u16, cols: u16, focused: bool) -> Buffer {
        let mut parser = vt100::Parser::new(rows, cols, 0);
        parser.process(bytes);
        let area = Rect::new(0, 0, cols, rows);
        let mut buf = Buffer::empty(area);
        PaneWidget {
            screen: parser.screen(),
            focused,
            selection: None,
        }
        .render(area, &mut buf);
        buf
    }

    fn modifiers_at(buf: &Buffer, x: u16, y: u16) -> Modifier {
        buf.cell((x, y)).expect("cell in area").style().add_modifier
    }

    /// SGR 2 reaches the buffer as `Modifier::DIM`.
    ///
    /// vt100 exposed `Cell::dim()` in 0.16, after `cell_style` was written
    /// against 0.15, so the attribute was read by the emulator and dropped by
    /// the adapter — a child's dim text rendered at full weight.
    #[test]
    fn sgr_2_reaches_the_buffer_as_dim() {
        let buf = rendered(b"\x1b[2mdim", 1, 8, true);
        assert!(
            modifiers_at(&buf, 0, 0).contains(Modifier::DIM),
            "a cell the child marked SGR 2 must carry DIM; got {:?}",
            modifiers_at(&buf, 0, 0)
        );
    }

    /// …and only that cell. SGR 22 turns dim off, and a focused pane adds no
    /// dimming of its own, so the text after the reset must come through at
    /// full weight. Without this half the test would also pass if the widget
    /// dimmed everything unconditionally.
    #[test]
    fn sgr_22_clears_dim_on_a_focused_pane() {
        let buf = rendered(b"\x1b[2md\x1b[22mN", 1, 8, true);
        assert!(
            modifiers_at(&buf, 0, 0).contains(Modifier::DIM),
            "the dim run"
        );
        assert!(
            !modifiers_at(&buf, 1, 0).contains(Modifier::DIM),
            "after SGR 22 the cell must not be dim; got {:?}",
            modifiers_at(&buf, 1, 0)
        );
    }

    /// An UNFOCUSED pane fades wholesale, and it spends `Modifier::DIM` to do
    /// it — the same channel SGR 2 uses. So content-dim and focus-dim collapse
    /// there and cannot be told apart. Pinned deliberately: it is the reason
    /// the focused-pane assertion above specifies `focused: true`, and it stops
    /// a later reader "fixing" the collapse by moving content-dim to some other
    /// modifier the terminal would render differently.
    #[test]
    fn an_unfocused_pane_dims_everything_including_undimmed_cells() {
        let buf = rendered(b"\x1b[2md\x1b[22mN", 1, 8, false);
        assert!(modifiers_at(&buf, 0, 0).contains(Modifier::DIM));
        assert!(
            modifiers_at(&buf, 1, 0).contains(Modifier::DIM),
            "the focus fade covers the whole pane, dim content or not"
        );
    }
}

#[cfg(test)]
mod selection_tests {
    use super::selected;

    /// A multi-row selection takes the tail of the first row, all of the middle, and
    /// the head of the last — not a rectangle.
    #[test]
    fn charwise_spans_rows_without_clipping_to_a_column_box() {
        let sel = Some(((1, 5), (3, 2)));
        assert!(
            !selected(sel, 1, 4),
            "before the start column on the first row"
        );
        assert!(selected(sel, 1, 5), "the start cell");
        assert!(selected(sel, 1, 99), "to end of the first row");
        assert!(selected(sel, 2, 0), "a whole interior row");
        assert!(selected(sel, 2, 99), "…including its tail");
        assert!(selected(sel, 3, 2), "up to the end column");
        assert!(!selected(sel, 3, 3), "past the end column on the last row");
        assert!(!selected(sel, 0, 5), "above the selection");
        assert!(!selected(sel, 4, 0), "below the selection");
    }

    /// A single-row selection is bounded at BOTH ends.
    #[test]
    fn charwise_within_one_row_is_bounded_both_ends() {
        let sel = Some(((2, 3), (2, 6)));
        assert!(!selected(sel, 2, 2));
        assert!(selected(sel, 2, 3));
        assert!(selected(sel, 2, 6));
        assert!(!selected(sel, 2, 7));
    }

    #[test]
    fn no_selection_selects_nothing() {
        assert!(!selected(None, 0, 0));
    }
}
