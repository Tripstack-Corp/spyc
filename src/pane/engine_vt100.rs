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
pub type PaneEngine = super::engine_ghostty::GhosttyEngine;

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
    use super::super::engine::conformance;

    // The contract suite, against the fallback engine. If this file's impl
    // ever stops satisfying the seam, the escape hatch #453 weighs is gone and
    // nothing else would say so.
    #[test]
    fn reports_what_the_engine_holds() {
        conformance::reports_what_the_engine_holds::<vt100::Parser>();
    }

    #[test]
    fn past_the_edge_is_absence_not_blankness() {
        conformance::past_the_edge_is_absence_not_blankness::<vt100::Parser>();
    }

    #[test]
    fn mouse_protocol_maps_through_the_model_enums() {
        conformance::mouse_protocol_maps_through_the_model_enums::<vt100::Parser>();
    }

    #[test]
    fn set_scrollback_clamps_to_the_real_length() {
        conformance::set_scrollback_clamps_to_the_real_length::<vt100::Parser>();
    }

    #[test]
    fn reports_the_modes_the_pane_branches_on() {
        conformance::reports_the_modes_the_pane_branches_on::<vt100::Parser>();
    }
}
