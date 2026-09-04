//! vt100 behind [`crate::pane::engine`].
//!
//! Every method is a delegation, so the seam costs nothing at runtime — the
//! calls monomorphise and inline exactly as the direct ones did. That is the
//! strangler-fig property this refactor is required to keep: the trait is
//! introduced, behaviour is unchanged, and the implementation swap is a
//! separate change.

use super::engine::{CellStyle, Color, Engine, MouseEncoding, MouseMode, TerminalScreen, Wide};

/// The engine `Pane` runs.
///
/// A type alias rather than a generic parameter on `Pane`, deliberately.
/// `Pane` is reached from `PaneTabs`, `App` and the render pass, so a
/// `Pane<E>` would propagate a parameter through all of them for no benefit
/// while this is the only implementation. Everything below the alias speaks
/// through [`Engine`] and [`TerminalScreen`], so swapping the engine is a
/// change to this line and the impls under it — not to a signature anywhere
/// else.
pub type PaneEngine = vt100::Parser;

/// The screen type that engine exposes, spelled once so call sites do not
/// repeat the projection.
pub type PaneScreen = <PaneEngine as Engine>::Screen;

const fn color(c: vt100::Color) -> Color {
    match c {
        vt100::Color::Default => Color::Default,
        vt100::Color::Idx(i) => Color::Idx(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

const fn mode(m: vt100::MouseProtocolMode) -> MouseMode {
    use vt100::MouseProtocolMode as M;
    match m {
        M::None => MouseMode::None,
        M::Press => MouseMode::Press,
        M::PressRelease => MouseMode::PressRelease,
        M::ButtonMotion => MouseMode::ButtonMotion,
        M::AnyMotion => MouseMode::AnyMotion,
    }
}

const fn encoding(e: vt100::MouseProtocolEncoding) -> MouseEncoding {
    use vt100::MouseProtocolEncoding as E;
    match e {
        E::Default => MouseEncoding::Default,
        E::Utf8 => MouseEncoding::Utf8,
        E::Sgr => MouseEncoding::Sgr,
    }
}

impl TerminalScreen for vt100::Screen {
    fn size(&self) -> (u16, u16) {
        Self::size(self)
    }

    fn cursor_position(&self) -> (u16, u16) {
        Self::cursor_position(self)
    }

    fn hide_cursor(&self) -> bool {
        Self::hide_cursor(self)
    }

    fn alternate_screen(&self) -> bool {
        Self::alternate_screen(self)
    }

    fn bracketed_paste(&self) -> bool {
        Self::bracketed_paste(self)
    }

    fn application_cursor(&self) -> bool {
        Self::application_cursor(self)
    }

    fn mouse_protocol(&self) -> (MouseMode, MouseEncoding) {
        (
            mode(self.mouse_protocol_mode()),
            encoding(self.mouse_protocol_encoding()),
        )
    }

    fn cell_style(&self, row: u16, col: u16) -> Option<CellStyle> {
        let c = self.cell(row, col)?;
        Some(CellStyle {
            fg: color(c.fgcolor()),
            bg: color(c.bgcolor()),
            bold: c.bold(),
            dim: c.dim(),
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
        })
    }

    fn cell_text(&self, row: u16, col: u16, out: &mut String) -> bool {
        let Some(c) = self.cell(row, col) else {
            return false;
        };
        out.push_str(c.contents());
        true
    }

    fn contents(&self) -> String {
        Self::contents(self)
    }

    fn contents_between(
        &self,
        start_row: u16,
        start_col: u16,
        end_row: u16,
        end_col: u16,
    ) -> String {
        Self::contents_between(self, start_row, start_col, end_row, end_col)
    }

    fn scrollback(&self) -> usize {
        Self::scrollback(self)
    }

    fn set_scrollback(&mut self, rows: usize) {
        Self::set_scrollback(self, rows);
    }
}

impl Engine for vt100::Parser {
    type Screen = vt100::Screen;

    fn new(rows: u16, cols: u16, scrollback_rows: usize) -> Self {
        Self::new(rows, cols, scrollback_rows)
    }

    fn process(&mut self, bytes: &[u8]) {
        Self::process(self, bytes);
    }

    fn screen(&self) -> &Self::Screen {
        Self::screen(self)
    }

    fn screen_mut(&mut self) -> &mut Self::Screen {
        Self::screen_mut(self)
    }
}

#[cfg(test)]
mod tests {
    use super::super::engine::{MouseEncoding, MouseMode, TerminalScreen, Wide};

    /// The delegations must report what the engine reports, including the two
    /// attributes the seam exists to keep engine-agnostic.
    #[test]
    fn the_seam_reports_what_the_engine_holds() {
        // SGR 1 and SGR 2 are the SAME intensity channel — "increased" and
        // "decreased" — so a cell cannot be both, and setting one replaces the
        // other. They get separate cells here rather than one combined run,
        // which an earlier version of this test got wrong.
        let mut parser = vt100::Parser::new(2, 12, 0);
        parser.process("\x1b[1;3;4;7;31;42mX\x1b[0m\x1b[2mD\x1b[0m\u{3042}".as_bytes());
        let screen = parser.screen();

        let styled = TerminalScreen::cell_style(screen, 0, 0).expect("cell 0,0");
        assert!(styled.bold, "SGR 1");
        assert!(styled.italic, "SGR 3");
        assert!(styled.underline, "SGR 4");
        assert!(styled.reverse, "SGR 7");
        assert_eq!(styled.fg, super::Color::Idx(1));
        assert_eq!(styled.bg, super::Color::Idx(2));
        assert!(
            !styled.dim,
            "SGR 1 and SGR 2 share a channel; this cell asked for bold"
        );

        let dimmed = TerminalScreen::cell_style(screen, 0, 1).expect("cell 0,1");
        assert!(
            dimmed.dim,
            "SGR 2 — dropped by the adapter before #452, and the seam must not re-drop it"
        );
        assert!(!dimmed.bold, "and dim is not bold");

        // The wide pair: head carries the glyph, tail carries nothing.
        let head = TerminalScreen::cell_style(screen, 0, 2).expect("cell 0,2");
        let tail = TerminalScreen::cell_style(screen, 0, 3).expect("cell 0,3");
        assert_eq!(head.wide, Wide::Head);
        assert_eq!(tail.wide, Wide::Tail);

        let mut text = String::new();
        assert!(TerminalScreen::cell_text(screen, 0, 2, &mut text));
        assert_eq!(text, "\u{3042}", "the head carries the glyph");
        text.clear();
        assert!(TerminalScreen::cell_text(screen, 0, 3, &mut text));
        assert!(text.is_empty(), "the tail carries no text of its own");
    }

    /// Past the grid edge both cell accessors report absence rather than
    /// inventing a blank — the distinction `line_from_visible_row` breaks on.
    #[test]
    fn past_the_edge_is_absence_not_blankness() {
        let p = vt100::Parser::new(2, 4, 0);
        let s = p.screen();
        assert!(TerminalScreen::cell_style(s, 0, 4).is_none());
        let mut t = String::from("kept");
        assert!(!TerminalScreen::cell_text(s, 0, 4, &mut t));
        assert_eq!(t, "kept", "a failed read must not disturb the buffer");
    }

    /// Mouse mode and encoding round-trip through the model enums, since these
    /// gate whether spyc forwards mouse bytes at all (#170).
    #[test]
    fn mouse_protocol_maps_through_the_model_enums() {
        let mut p = vt100::Parser::new(2, 8, 0);
        assert_eq!(
            TerminalScreen::mouse_protocol(p.screen()),
            (MouseMode::None, MouseEncoding::Default),
            "a child that never asked must report None, or spyc forwards bytes it shouldn't"
        );
        p.process(b"\x1b[?1002h\x1b[?1006h");
        assert_eq!(
            TerminalScreen::mouse_protocol(p.screen()),
            (MouseMode::ButtonMotion, MouseEncoding::Sgr)
        );
    }

    /// `set_scrollback` clamps, which is how the adapter discovers the real
    /// length. Pinned because the whole scrollback walk depends on it.
    #[test]
    fn set_scrollback_clamps_to_the_real_length() {
        let mut p = vt100::Parser::new(2, 8, 100);
        for _ in 0..20 {
            p.process(b"line\r\n");
        }
        let s = p.screen_mut();
        TerminalScreen::set_scrollback(s, usize::MAX);
        let len = TerminalScreen::scrollback(s);
        assert!(len > 0 && len < usize::MAX, "clamped to {len}");
    }
}
