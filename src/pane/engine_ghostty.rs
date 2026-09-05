//! SCRATCH: libghostty-vt behind the `Engine` seam, to price PR 15's threading
//! decision by running it. Not shippable. See `THREADING_INVENTORY.md`.
//!
//! Two things here are deliberate expedients, not proposals:
//!
//! 1. `unsafe impl Send` on the engine. Sound only if the library has no
//!    thread affinity, which is one of the things this branch exists to find
//!    out. The actor alternative is not built here.
//! 2. Cell reads go one FFI call at a time, the shape the seam's `cell_style` /
//!    `cell_text` pair implies. Counting those calls is the measurement.

use spyc_vt_sys::ffi::{
    GhosttyCellData as CellData, GhosttyPoint, GhosttyPointCoordinate, GhosttyPointTag,
    GhosttyTerminalData as Data, GhosttyTerminalModeConfig, GhosttyTerminalOption as Opt,
    GhosttyTerminalScreen as ScreenKind,
};
use spyc_vt_sys::{ffi, scrollback};

use super::engine::{CellStyle, Color, Engine, MouseEncoding, MouseMode, TerminalScreen, Wide};

/// The terminal handle plus the view offset the seam's scrollback pair moves.
///
/// ghostty addresses history by coordinate, so the offset is ours to keep; the
/// incumbent stores it inside the parser.
pub struct GhosttyScreen {
    t: ffi::GhosttyTerminal,
    rows: u16,
    cols: u16,
    view: usize,
}

pub struct GhosttyEngine {
    inner: GhosttyScreen,
}

// SCRATCH EXPEDIENT. The pane hands the engine to a worker thread and locks it
// from the render pass, so `Engine: Send` is required. Whether this is sound
// is exactly what the writeup has to answer.
unsafe impl Send for GhosttyEngine {}

impl Drop for GhosttyEngine {
    fn drop(&mut self) {
        if !self.inner.t.is_null() {
            unsafe { ffi::ghostty_terminal_free(self.inner.t) };
        }
    }
}

fn point(tag: GhosttyPointTag::Type, x: u16, y: u32) -> GhosttyPoint {
    GhosttyPoint {
        tag,
        value: ffi::GhosttyPointValue {
            coordinate: GhosttyPointCoordinate { x, y },
        },
    }
}

fn empty_grid_ref() -> ffi::GhosttyGridRef {
    ffi::GhosttyGridRef {
        size: size_of::<ffi::GhosttyGridRef>(),
        node: std::ptr::null_mut(),
        x: 0,
        y: 0,
    }
}

fn default_style() -> ffi::GhosttyStyle {
    let mut st: ffi::GhosttyStyle = unsafe { std::mem::zeroed() };
    st.size = size_of::<ffi::GhosttyStyle>();
    unsafe { ffi::ghostty_style_default(&raw mut st) };
    st
}

fn colour(c: ffi::GhosttyStyleColor) -> Color {
    match c.tag {
        ffi::GhosttyStyleColorTag::GHOSTTY_STYLE_COLOR_PALETTE => {
            Color::Idx(unsafe { c.value.palette })
        }
        ffi::GhosttyStyleColorTag::GHOSTTY_STYLE_COLOR_RGB => {
            let rgb = unsafe { c.value.rgb };
            Color::Rgb(rgb.r, rgb.g, rgb.b)
        }
        _ => Color::Default,
    }
}

impl GhosttyScreen {
    fn get_u16(&self, tag: Data::Type) -> Option<u16> {
        let mut v: u16 = 0;
        let ok = unsafe { ffi::ghostty_terminal_get(self.t, tag, (&raw mut v).cast()) };
        (ok == spyc_vt_sys::SUCCESS).then_some(v)
    }

    fn get_usize(&self, tag: Data::Type) -> Option<usize> {
        let mut v: usize = 0;
        let ok = unsafe { ffi::ghostty_terminal_get(self.t, tag, (&raw mut v).cast()) };
        (ok == spyc_vt_sys::SUCCESS).then_some(v)
    }

    fn get_bool(&self, tag: Data::Type) -> Option<bool> {
        let mut v = false;
        let ok = unsafe { ffi::ghostty_terminal_get(self.t, tag, (&raw mut v).cast()) };
        (ok == spyc_vt_sys::SUCCESS).then_some(v)
    }

    /// Read one DEC private mode. `GhosttyMode` packs the mode number in bits
    /// 0-14 with an ANSI flag in bit 15, so a DEC private mode is its bare
    /// number.
    fn mode(&self, dec: u16) -> bool {
        let mut cfg = GhosttyTerminalModeConfig {
            mode: dec,
            value: false,
        };
        let ok = unsafe {
            ffi::ghostty_terminal_get(self.t, Data::GHOSTTY_TERMINAL_DATA_MODE, (&raw mut cfg).cast())
        };
        ok == spyc_vt_sys::SUCCESS && cfg.value
    }

    /// History rows currently held.
    fn history_rows(&self) -> usize {
        self.get_usize(Data::GHOSTTY_TERMINAL_DATA_SCROLLBACK_ROWS)
            .unwrap_or(0)
    }

    /// Absolute screen-space `y` for a viewport row at the current view offset.
    ///
    /// Screen space runs history-first, so the top visible row sits at
    /// `history - view`.
    fn screen_y(&self, row: u16) -> u32 {
        let base = self.history_rows().saturating_sub(self.view);
        u32::try_from(base + usize::from(row)).unwrap_or(u32::MAX)
    }

    fn grid_ref(&self, row: u16, col: u16) -> Option<ffi::GhosttyGridRef> {
        let mut gr = empty_grid_ref();
        let p = point(
            GhosttyPointTag::GHOSTTY_POINT_TAG_SCREEN,
            col,
            self.screen_y(row),
        );
        (unsafe { ffi::ghostty_terminal_grid_ref(self.t, p, &raw mut gr) } == spyc_vt_sys::SUCCESS)
            .then_some(gr)
    }

    fn wide_of(gr: &ffi::GhosttyGridRef) -> Wide {
        let mut raw: ffi::GhosttyCell = 0;
        if unsafe { ffi::ghostty_grid_ref_cell(gr, &raw mut raw) } != spyc_vt_sys::SUCCESS {
            return Wide::Narrow;
        }
        let mut w: ffi::GhosttyCellWide::Type = 0;
        if unsafe {
            ffi::ghostty_cell_get(raw, CellData::GHOSTTY_CELL_DATA_WIDE, (&raw mut w).cast())
        } != spyc_vt_sys::SUCCESS
        {
            return Wide::Narrow;
        }
        match w {
            ffi::GhosttyCellWide::GHOSTTY_CELL_WIDE_WIDE => Wide::Head,
            ffi::GhosttyCellWide::GHOSTTY_CELL_WIDE_SPACER_TAIL => Wide::Tail,
            _ => Wide::Narrow,
        }
    }
}

impl TerminalScreen for GhosttyScreen {
    fn size(&self) -> (u16, u16) {
        (
            self.get_u16(Data::GHOSTTY_TERMINAL_DATA_ROWS).unwrap_or(self.rows),
            self.get_u16(Data::GHOSTTY_TERMINAL_DATA_COLS).unwrap_or(self.cols),
        )
    }

    fn cursor_position(&self) -> (u16, u16) {
        (
            self.get_u16(Data::GHOSTTY_TERMINAL_DATA_CURSOR_Y).unwrap_or(0),
            self.get_u16(Data::GHOSTTY_TERMINAL_DATA_CURSOR_X).unwrap_or(0),
        )
    }

    fn hide_cursor(&self) -> bool {
        !self
            .get_bool(Data::GHOSTTY_TERMINAL_DATA_CURSOR_VISIBLE)
            .unwrap_or(true)
    }

    fn alternate_screen(&self) -> bool {
        let mut active: ScreenKind::Type = 0;
        let ok = unsafe {
            ffi::ghostty_terminal_get(
                self.t,
                Data::GHOSTTY_TERMINAL_DATA_ACTIVE_SCREEN,
                (&raw mut active).cast(),
            )
        };
        ok == spyc_vt_sys::SUCCESS && active == ScreenKind::GHOSTTY_TERMINAL_SCREEN_ALTERNATE
    }

    fn bracketed_paste(&self) -> bool {
        self.mode(2004)
    }

    fn application_cursor(&self) -> bool {
        self.mode(1)
    }

    fn mouse_protocol(&self) -> (MouseMode, MouseEncoding) {
        let mode = if self.mode(1003) {
            MouseMode::AnyMotion
        } else if self.mode(1002) {
            MouseMode::ButtonMotion
        } else if self.mode(1000) {
            MouseMode::PressRelease
        } else if self.mode(9) {
            MouseMode::Press
        } else {
            MouseMode::None
        };
        let enc = if self.mode(1006) {
            MouseEncoding::Sgr
        } else if self.mode(1005) {
            MouseEncoding::Utf8
        } else {
            MouseEncoding::Default
        };
        (mode, enc)
    }

    fn cell_style(&self, row: u16, col: u16) -> Option<CellStyle> {
        if col >= self.cols {
            return None;
        }
        let gr = self.grid_ref(row, col)?;
        let mut st = default_style();
        let _ = unsafe { ffi::ghostty_grid_ref_style(&raw const gr, &raw mut st) };
        Some(CellStyle {
            fg: colour(st.fg_color),
            bg: colour(st.bg_color),
            bold: st.bold,
            dim: st.faint,
            italic: st.italic,
            underline: st.underline != ffi::GhosttySgrUnderline::GHOSTTY_SGR_UNDERLINE_NONE,
            reverse: st.inverse,
            wide: Self::wide_of(&gr),
        })
    }

    fn cell_text(&self, row: u16, col: u16, out: &mut String) -> bool {
        if col >= self.cols {
            return false;
        }
        let Some(gr) = self.grid_ref(row, col) else {
            return false;
        };
        // A spacer tail carries no glyph of its own.
        if Self::wide_of(&gr) == Wide::Tail {
            return true;
        }
        let mut buf = [0u32; 32];
        let mut n: usize = 0;
        if unsafe {
            ffi::ghostty_grid_ref_graphemes(&raw const gr, buf.as_mut_ptr(), buf.len(), &raw mut n)
        } == spyc_vt_sys::SUCCESS
        {
            out.extend(buf[..n].iter().filter_map(|c| char::from_u32(*c)));
        }
        true
    }

    fn contents(&self) -> String {
        let (rows, _) = self.size();
        (0..rows)
            .map(|r| {
                let mut s = String::new();
                for c in 0..self.cols {
                    let before = s.len();
                    if !self.cell_text(r, c, &mut s) {
                        break;
                    }
                    if s.len() == before {
                        s.push(' ');
                    }
                }
                s.trim_end().to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn contents_between(&self, sr: u16, sc: u16, er: u16, ec: u16) -> String {
        let mut out = String::new();
        for r in sr..=er {
            let first = if r == sr { sc } else { 0 };
            let last = if r == er { ec } else { self.cols };
            for c in first..last.min(self.cols) {
                let before = out.len();
                if !self.cell_text(r, c, &mut out) {
                    break;
                }
                if out.len() == before {
                    out.push(' ');
                }
            }
            if r != er {
                out.push('\n');
            }
        }
        out
    }

    fn scrollback(&self) -> usize {
        self.view
    }

    fn set_scrollback(&mut self, rows: usize) {
        self.view = rows.min(self.history_rows());
    }
}

impl Engine for GhosttyEngine {
    type Screen = GhosttyScreen;

    fn new(rows: u16, cols: u16, scrollback_rows: usize) -> Self {
        let mut t: ffi::GhosttyTerminal = std::ptr::null_mut();
        let ok = unsafe { ffi::ghostty_terminal_new(std::ptr::null(), &raw mut t, cols, rows) };
        assert_eq!(ok, spyc_vt_sys::SUCCESS, "ghostty_terminal_new failed");
        let limits = scrollback::limits_for_row_budget(scrollback_rows.max(1));
        let (bytes, lines) = (limits.max_bytes, limits.max_lines);
        unsafe {
            ffi::ghostty_terminal_set(
                t,
                Opt::GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_BYTES,
                (&raw const bytes).cast(),
            );
            ffi::ghostty_terminal_set(
                t,
                Opt::GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_LINES,
                (&raw const lines).cast(),
            );
        }
        Self {
            inner: GhosttyScreen {
                t,
                rows,
                cols,
                view: 0,
            },
        }
    }

    fn process(&mut self, bytes: &[u8]) {
        unsafe { ffi::ghostty_terminal_vt_write(self.inner.t, bytes.as_ptr(), bytes.len()) };
    }

    fn screen(&self) -> &Self::Screen {
        &self.inner
    }

    fn screen_mut(&mut self) -> &mut Self::Screen {
        &mut self.inner
    }
}

impl GhosttyScreen {
    /// The seam has no resize; the pane reaches it through `set_size` on the
    /// incumbent's screen. Kept here so the flip needs no call-site change.
    pub fn set_size(&mut self, rows: u16, cols: u16) {
        let _ = unsafe { ffi::ghostty_terminal_resize(self.t, cols, rows, 8, 16) };
        self.rows = rows;
        self.cols = cols;
        self.view = 0;
    }
}

/// PR 15 measurement: the two ways to read a frame, priced against each other.
#[cfg(test)]
mod render_state_probe {
    use super::*;
    use spyc_vt_sys::ffi::{
        GhosttyRenderStateData as RsData, GhosttyRenderStateRowCellsData as CellsData,
        GhosttyRenderStateRowData as RowData,
    };

    /// Read every visible cell through `grid_ref` — today's shape, two FFI
    /// coordinate resolutions per cell.
    fn walk_grid_ref(e: &GhosttyEngine) -> (usize, usize) {
        let s = e.screen();
        let (rows, cols) = TerminalScreen::size(s);
        let (mut cells, mut bytes) = (0, 0);
        let mut buf = String::new();
        for r in 0..rows {
            for c in 0..cols {
                if TerminalScreen::cell_style(s, r, c).is_some() {
                    cells += 1;
                }
                buf.clear();
                TerminalScreen::cell_text(s, r, c, &mut buf);
                bytes += buf.len();
            }
        }
        (cells, bytes)
    }

    /// Read every visible cell through the render state: one `begin_update`
    /// under the lock, then a cursor walk that never touches the terminal.
    fn walk_render_state(e: &GhosttyEngine, st: ffi::GhosttyRenderState) -> (usize, usize) {
        let (mut cells, mut bytes) = (0, 0);
        unsafe {
            assert_eq!(
                ffi::ghostty_render_state_begin_update(st, e.inner.t),
                spyc_vt_sys::SUCCESS
            );
            assert_eq!(
                ffi::ghostty_render_state_end_update(st),
                spyc_vt_sys::SUCCESS
            );

            let mut it: ffi::GhosttyRenderStateRowIterator = std::ptr::null_mut();
            assert_eq!(
                ffi::ghostty_render_state_row_iterator_new(std::ptr::null(), &raw mut it),
                spyc_vt_sys::SUCCESS
            );
            // The out param is the ADDRESS of the handle; passing the handle
            // itself returns GHOSTTY_INVALID_VALUE (measured, not assumed).
            let allocated = it;
            assert_eq!(
                ffi::ghostty_render_state_get(
                    st,
                    RsData::GHOSTTY_RENDER_STATE_DATA_ROW_ITERATOR,
                    (&raw mut it).cast(),
                ),
                spyc_vt_sys::SUCCESS,
                "binding the row iterator to the state"
            );
            debug_assert_eq!(allocated, it, "get populates in place");

            let mut rc: ffi::GhosttyRenderStateRowCells = std::ptr::null_mut();
            assert_eq!(
                ffi::ghostty_render_state_row_cells_new(std::ptr::null(), &raw mut rc),
                spyc_vt_sys::SUCCESS
            );

            while ffi::ghostty_render_state_row_iterator_next(it) {
                assert_eq!(
                    ffi::ghostty_render_state_row_get(
                        it,
                        RowData::GHOSTTY_RENDER_STATE_ROW_DATA_CELLS,
                        (&raw mut rc).cast(),
                    ),
                    spyc_vt_sys::SUCCESS,
                    "binding cells to the row"
                );
                while ffi::ghostty_render_state_row_cells_next(rc) {
                    cells += 1;
                    let mut scratch = [0u8; 32];
                    let mut gb = ffi::GhosttyBuffer {
                        ptr: scratch.as_mut_ptr(),
                        cap: scratch.len(),
                        len: 0,
                    };
                    if ffi::ghostty_render_state_row_cells_get(
                        rc,
                        CellsData::GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_UTF8,
                        (&raw mut gb).cast(),
                    ) == spyc_vt_sys::SUCCESS
                    {
                        bytes += gb.len;
                    }
                    let mut styled = false;
                    let _ = ffi::ghostty_render_state_row_cells_get(
                        rc,
                        CellsData::GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_HAS_STYLING,
                        (&raw mut styled).cast(),
                    );
                    if styled {
                        let mut sty = default_style();
                        let _ = ffi::ghostty_render_state_row_cells_get(
                            rc,
                            CellsData::GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_STYLE,
                            (&raw mut sty).cast(),
                        );
                    }
                }
            }
            ffi::ghostty_render_state_row_cells_free(rc);
            ffi::ghostty_render_state_row_iterator_free(it);
        }
        (cells, bytes)
    }

    #[test]
    fn price_the_two_read_paths() {
        const FRAMES: u32 = 200;
        for (rows, cols) in [(24u16, 80u16), (50, 200)] {
            let mut e = <GhosttyEngine as Engine>::new(rows, cols, 10_000);
            for i in 0..rows {
                e.process(
                    format!("\x1b[3{}mrow {i:02}\x1b[0m ascii \u{3042}\u{3044} \x1b[1mbold\x1b[0m tail\r\n", i % 8)
                        .as_bytes(),
                );
            }

            let t0 = std::time::Instant::now();
            let mut a = (0, 0);
            for _ in 0..FRAMES {
                a = walk_grid_ref(&e);
            }
            let grid = t0.elapsed() / FRAMES;

            let mut st: ffi::GhosttyRenderState = std::ptr::null_mut();
            assert_eq!(
                unsafe { ffi::ghostty_render_state_new(std::ptr::null(), &raw mut st) },
                spyc_vt_sys::SUCCESS
            );
            let t1 = std::time::Instant::now();
            let mut b = (0, 0);
            for _ in 0..FRAMES {
                b = walk_render_state(&e, st);
            }
            let rs = t1.elapsed() / FRAMES;
            unsafe { ffi::ghostty_render_state_free(st) };

            eprintln!(
                "{rows}x{cols}  grid_ref {:>8.1} us  render_state {:>8.1} us  ({:.2}x)  cells {}/{} bytes {}/{}",
                grid.as_secs_f64() * 1e6,
                rs.as_secs_f64() * 1e6,
                grid.as_secs_f64() / rs.as_secs_f64(),
                a.0, b.0, a.1, b.1
            );
        }
    }
}
