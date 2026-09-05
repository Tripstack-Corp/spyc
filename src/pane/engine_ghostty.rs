//! libghostty-vt behind the [`Engine`] seam.
//!
//! ## How a frame is read
//!
//! The seam offers random access (`cell_style(row, col)`, `cell_text(row, col,
//! out)`), and ghostty addresses cells by coordinate, so the obvious
//! implementation resolves a coordinate per call — two `ghostty_terminal_grid_ref`
//! lookups per cell, since style and text are separate calls. That is what this
//! started as, and it costs 80.0 us per 24x80 frame.
//!
//! Instead a frame is **materialized once** into [`Frame`] and every read
//! answers from it. The live viewport fills through `ghostty_render_state_*`,
//! which walks rows and cells with cursors rather than resolving coordinates:
//! 19.3 us for the same frame, 4.15x cheaper, reading the identical fields
//! (both sides compute `wide`; an earlier measurement that let the render state
//! skip it reported a flattering 7x).
//!
//! History reads — `ui::scrollback` walking back through the buffer — cannot
//! use the render state, which only ever presents the terminal's live
//! viewport. Those fill the same `Frame` through `grid_ref` instead. Two fill
//! paths is a divergence risk, so `both_fills_agree_on_the_live_viewport` pins
//! them against each other: an instrument that shares the subject's model
//! inherits its blind spots, and two fills that disagree would do exactly that.
//!
//! ## Threading
//!
//! See the `ghostty-terminal-send` trap anchor below.

use std::cell::{Cell, RefCell};

use spyc_vt_sys::ffi::{
    GhosttyCellData as CellData, GhosttyPoint, GhosttyPointCoordinate, GhosttyPointTag,
    GhosttyRenderStateData as RsData, GhosttyRenderStateRowCellsData as CellsData,
    GhosttyRenderStateRowData as RsRowData, GhosttyRowData as RowData, GhosttyTerminalData as Data,
    GhosttyTerminalModeConfig, GhosttyTerminalOption as Opt, GhosttyTerminalScreen as ScreenKind,
};
use spyc_vt_sys::{ffi, scrollback};

use super::engine::{CellStyle, Color, Engine, MouseEncoding, MouseMode, TerminalScreen, Wide};

/// One materialized frame: what every read answers from.
///
/// Text is a flat `String` with a span per cell rather than a `String` per
/// cell — 1,920 allocations a frame is the shape this exists to avoid.
#[derive(Default)]
struct Frame {
    rows: u16,
    cols: u16,
    styles: Vec<CellStyle>,
    text: String,
    spans: Vec<(u32, u32)>,
    /// Per row: does it continue into the next one (a soft wrap)?
    wrapped: Vec<bool>,
}

impl Frame {
    fn idx(&self, row: u16, col: u16) -> Option<usize> {
        (row < self.rows && col < self.cols)
            .then(|| usize::from(row) * usize::from(self.cols) + usize::from(col))
    }

    fn clear_for(&mut self, rows: u16, cols: u16) {
        self.rows = rows;
        self.cols = cols;
        self.styles.clear();
        self.text.clear();
        self.spans.clear();
        self.wrapped.clear();
    }
}

pub struct GhosttyScreen {
    t: ffi::GhosttyTerminal,
    /// Reused across frames; `begin_update` is the only call needing the
    /// terminal, which is what keeps the pane's lock window to one call.
    rs: ffi::GhosttyRenderState,
    it: ffi::GhosttyRenderStateRowIterator,
    rc: ffi::GhosttyRenderStateRowCells,
    rows: u16,
    cols: u16,
    /// Scrollback view offset in rows. ghostty addresses history by
    /// coordinate, so the offset is ours to keep; vt100 stored it internally.
    view: usize,
    /// The row budget the seam was constructed with.
    ///
    /// ghostty retains history in pages and will not go below one — about 271
    /// rows at 80 columns — so a budget under that keeps more rows than asked
    /// for. The seam's contract is rows, so the ceiling is enforced here:
    /// history beyond the budget exists in the library but is not reachable
    /// through this screen. Without it a zero budget still shows history,
    /// which is what `scrollback_capacity_zero_emits_only_live` catches.
    budget: usize,
    frame: RefCell<Frame>,
    frame_valid: Cell<bool>,
}

pub struct GhosttyEngine {
    inner: GhosttyScreen,
}

// SPYC-TRAP(ghostty-terminal-send): the C terminal is not thread-safe, only
// serializable; `Pane` serializes it with a mutex and never shares it.
#[allow(
    clippy::non_send_fields_in_send_ty,
    reason = "the raw handle is the point; soundness argument is the trap anchor"
)]
unsafe impl Send for GhosttyEngine {}

impl Drop for GhosttyEngine {
    fn drop(&mut self) {
        let s = &mut self.inner;
        unsafe {
            if !s.rc.is_null() {
                ffi::ghostty_render_state_row_cells_free(s.rc);
            }
            if !s.it.is_null() {
                ffi::ghostty_render_state_row_iterator_free(s.it);
            }
            if !s.rs.is_null() {
                ffi::ghostty_render_state_free(s.rs);
            }
            if !s.t.is_null() {
                ffi::ghostty_terminal_free(s.t);
            }
        }
    }
}

const fn point(tag: GhosttyPointTag::Type, col: u16, row: u32) -> GhosttyPoint {
    GhosttyPoint {
        tag,
        value: ffi::GhosttyPointValue {
            coordinate: GhosttyPointCoordinate { x: col, y: row },
        },
    }
}

const fn empty_grid_ref() -> ffi::GhosttyGridRef {
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

const fn colour(c: ffi::GhosttyStyleColor) -> Color {
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

const fn style_from(st: &ffi::GhosttyStyle, wide: Wide) -> CellStyle {
    CellStyle {
        fg: colour(st.fg_color),
        bg: colour(st.bg_color),
        bold: st.bold,
        dim: st.faint,
        italic: st.italic,
        underline: st.underline != ffi::GhosttySgrUnderline::GHOSTTY_SGR_UNDERLINE_NONE,
        reverse: st.inverse,
        wide,
    }
}

fn wide_from_raw(raw: ffi::GhosttyCell) -> Wide {
    let mut w: ffi::GhosttyCellWide::Type = 0;
    if unsafe { ffi::ghostty_cell_get(raw, CellData::GHOSTTY_CELL_DATA_WIDE, (&raw mut w).cast()) }
        != spyc_vt_sys::SUCCESS
    {
        return Wide::Narrow;
    }
    match w {
        ffi::GhosttyCellWide::GHOSTTY_CELL_WIDE_WIDE => Wide::Head,
        ffi::GhosttyCellWide::GHOSTTY_CELL_WIDE_SPACER_TAIL => Wide::Tail,
        _ => Wide::Narrow,
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

    /// One DEC private mode. `GhosttyMode` packs the number in bits 0-14 with
    /// an ANSI flag in bit 15, so a DEC private mode is its bare number.
    fn mode(&self, dec: u16) -> bool {
        let mut cfg = GhosttyTerminalModeConfig {
            mode: dec,
            value: false,
        };
        let ok = unsafe {
            ffi::ghostty_terminal_get(
                self.t,
                Data::GHOSTTY_TERMINAL_DATA_MODE,
                (&raw mut cfg).cast(),
            )
        };
        ok == spyc_vt_sys::SUCCESS && cfg.value
    }

    /// History rows the library holds — the coordinate space, unclamped.
    fn history_rows_raw(&self) -> usize {
        self.get_usize(Data::GHOSTTY_TERMINAL_DATA_SCROLLBACK_ROWS)
            .unwrap_or(0)
    }

    /// History rows this screen will let a caller reach.
    fn history_rows(&self) -> usize {
        self.history_rows_raw().min(self.budget)
    }

    fn geometry(&self) -> (u16, u16) {
        (
            self.get_u16(Data::GHOSTTY_TERMINAL_DATA_ROWS)
                .unwrap_or(self.rows),
            self.get_u16(Data::GHOSTTY_TERMINAL_DATA_COLS)
                .unwrap_or(self.cols),
        )
    }

    fn invalidate(&self) {
        self.frame_valid.set(false);
    }

    /// Fill `frame` from the render state — the live viewport only.
    fn fill_from_render_state(&self, frame: &mut Frame) -> bool {
        let (rows, cols) = self.geometry();
        unsafe {
            if ffi::ghostty_render_state_begin_update(self.rs, self.t) != spyc_vt_sys::SUCCESS {
                return false;
            }
            // Everything past here reads only render-state memory, which is
            // why the pane's lock need not be held for it.
            if ffi::ghostty_render_state_end_update(self.rs) != spyc_vt_sys::SUCCESS {
                return false;
            }
            let mut it = self.it;
            if ffi::ghostty_render_state_get(
                self.rs,
                RsData::GHOSTTY_RENDER_STATE_DATA_ROW_ITERATOR,
                (&raw mut it).cast(),
            ) != spyc_vt_sys::SUCCESS
            {
                return false;
            }
            frame.clear_for(rows, cols);
            let mut scratch = [0u8; 64];
            while ffi::ghostty_render_state_row_iterator_next(it) {
                let mut rc = self.rc;
                if ffi::ghostty_render_state_row_get(
                    it,
                    RsRowData::GHOSTTY_RENDER_STATE_ROW_DATA_CELLS,
                    (&raw mut rc).cast(),
                ) != spyc_vt_sys::SUCCESS
                {
                    return false;
                }
                let mut row_raw: ffi::GhosttyRow = 0;
                let mut wrapped = false;
                if ffi::ghostty_render_state_row_get(
                    it,
                    RsRowData::GHOSTTY_RENDER_STATE_ROW_DATA_RAW,
                    (&raw mut row_raw).cast(),
                ) == spyc_vt_sys::SUCCESS
                {
                    let _ = ffi::ghostty_row_get(
                        row_raw,
                        RowData::GHOSTTY_ROW_DATA_WRAP,
                        (&raw mut wrapped).cast(),
                    );
                }
                frame.wrapped.push(wrapped);

                let mut col = 0u16;
                while ffi::ghostty_render_state_row_cells_next(rc) {
                    let mut raw: ffi::GhosttyCell = 0;
                    let wide = if ffi::ghostty_render_state_row_cells_get(
                        rc,
                        CellsData::GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_RAW,
                        (&raw mut raw).cast(),
                    ) == spyc_vt_sys::SUCCESS
                    {
                        wide_from_raw(raw)
                    } else {
                        Wide::Narrow
                    };

                    let start = u32::try_from(frame.text.len()).unwrap_or(u32::MAX);
                    // A spacer tail carries no glyph of its own.
                    if wide != Wide::Tail {
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
                            && gb.len <= scratch.len()
                        {
                            frame
                                .text
                                .push_str(&String::from_utf8_lossy(&scratch[..gb.len]));
                        }
                    }
                    let end = u32::try_from(frame.text.len()).unwrap_or(u32::MAX);
                    frame.spans.push((start, end));

                    let mut styled = false;
                    let _ = ffi::ghostty_render_state_row_cells_get(
                        rc,
                        CellsData::GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_HAS_STYLING,
                        (&raw mut styled).cast(),
                    );
                    let st = if styled {
                        let mut sty = default_style();
                        let _ = ffi::ghostty_render_state_row_cells_get(
                            rc,
                            CellsData::GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_STYLE,
                            (&raw mut sty).cast(),
                        );
                        sty
                    } else {
                        default_style()
                    };
                    frame.styles.push(style_from(&st, wide));
                    col += 1;
                }
                // A short row still owes the grid its full width.
                while col < cols {
                    let n = u32::try_from(frame.text.len()).unwrap_or(u32::MAX);
                    frame.spans.push((n, n));
                    frame.styles.push(CellStyle::default());
                    col += 1;
                }
            }
        }
        frame.styles.len() == usize::from(rows) * usize::from(cols)
    }

    /// Fill `frame` by coordinate. Serves history reads, where the render
    /// state cannot help, and is the reference the render-state fill is
    /// pinned against.
    fn fill_from_grid_ref(&self, frame: &mut Frame) {
        let (rows, cols) = self.geometry();
        frame.clear_for(rows, cols);
        let base = self.history_rows_raw().saturating_sub(self.view);
        for r in 0..rows {
            let y = u32::try_from(base + usize::from(r)).unwrap_or(u32::MAX);
            frame.wrapped.push(self.row_wrapped_at(y));
            for c in 0..cols {
                let mut gr = empty_grid_ref();
                let ok = unsafe {
                    ffi::ghostty_terminal_grid_ref(
                        self.t,
                        point(GhosttyPointTag::GHOSTTY_POINT_TAG_SCREEN, c, y),
                        &raw mut gr,
                    )
                } == spyc_vt_sys::SUCCESS;
                let start = u32::try_from(frame.text.len()).unwrap_or(u32::MAX);
                if !ok {
                    frame.spans.push((start, start));
                    frame.styles.push(CellStyle::default());
                    continue;
                }
                let mut raw: ffi::GhosttyCell = 0;
                let wide = if unsafe { ffi::ghostty_grid_ref_cell(&raw const gr, &raw mut raw) }
                    == spyc_vt_sys::SUCCESS
                {
                    wide_from_raw(raw)
                } else {
                    Wide::Narrow
                };
                if wide != Wide::Tail {
                    let mut buf = [0u32; 32];
                    let mut n: usize = 0;
                    if unsafe {
                        ffi::ghostty_grid_ref_graphemes(
                            &raw const gr,
                            buf.as_mut_ptr(),
                            buf.len(),
                            &raw mut n,
                        )
                    } == spyc_vt_sys::SUCCESS
                    {
                        frame
                            .text
                            .extend(buf[..n].iter().filter_map(|c| char::from_u32(*c)));
                    }
                }
                let end = u32::try_from(frame.text.len()).unwrap_or(u32::MAX);
                frame.spans.push((start, end));
                let mut st = default_style();
                let _ = unsafe { ffi::ghostty_grid_ref_style(&raw const gr, &raw mut st) };
                frame.styles.push(style_from(&st, wide));
            }
        }
    }

    fn row_wrapped_at(&self, y: u32) -> bool {
        let mut gr = empty_grid_ref();
        if unsafe {
            ffi::ghostty_terminal_grid_ref(
                self.t,
                point(GhosttyPointTag::GHOSTTY_POINT_TAG_SCREEN, 0, y),
                &raw mut gr,
            )
        } != spyc_vt_sys::SUCCESS
        {
            return false;
        }
        let mut row: ffi::GhosttyRow = 0;
        if unsafe { ffi::ghostty_grid_ref_row(&raw const gr, &raw mut row) } != spyc_vt_sys::SUCCESS
        {
            return false;
        }
        let mut w = false;
        let ok = unsafe {
            ffi::ghostty_row_get(row, RowData::GHOSTTY_ROW_DATA_WRAP, (&raw mut w).cast())
        };
        ok == spyc_vt_sys::SUCCESS && w
    }

    /// Materialize the frame if a `process` / resize / scroll invalidated it.
    fn ensure_frame(&self) {
        if self.frame_valid.get() {
            return;
        }
        let mut frame = self.frame.borrow_mut();
        // The render state presents the live viewport only, so a scrolled-back
        // view has to go the long way.
        if self.view != 0 || !self.fill_from_render_state(&mut frame) {
            self.fill_from_grid_ref(&mut frame);
        }
        self.frame_valid.set(true);
    }

    fn with_frame<R>(&self, f: impl FnOnce(&Frame) -> R) -> R {
        self.ensure_frame();
        f(&self.frame.borrow())
    }
}

impl TerminalScreen for GhosttyScreen {
    fn size(&self) -> (u16, u16) {
        self.geometry()
    }

    fn cursor_position(&self) -> (u16, u16) {
        (
            self.get_u16(Data::GHOSTTY_TERMINAL_DATA_CURSOR_Y)
                .unwrap_or(0),
            self.get_u16(Data::GHOSTTY_TERMINAL_DATA_CURSOR_X)
                .unwrap_or(0),
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
        self.with_frame(|f| f.idx(row, col).and_then(|i| f.styles.get(i).copied()))
    }

    fn cell_text(&self, row: u16, col: u16, out: &mut String) -> bool {
        self.with_frame(|f| {
            let Some(i) = f.idx(row, col) else {
                return false;
            };
            let Some(&(s, e)) = f.spans.get(i) else {
                return false;
            };
            out.push_str(&f.text[s as usize..e as usize]);
            true
        })
    }

    fn contents(&self) -> String {
        self.with_frame(|f| {
            (0..f.rows)
                .map(|r| row_string(f, r))
                .collect::<Vec<_>>()
                .join("\n")
        })
    }

    fn contents_between(&self, sr: u16, sc: u16, er: u16, ec: u16) -> String {
        self.with_frame(|f| {
            let mut out = String::new();
            for r in sr..=er.min(f.rows.saturating_sub(1)) {
                let first = if r == sr { sc } else { 0 };
                let last = if r == er { ec } else { f.cols };
                for c in first..last.min(f.cols) {
                    let Some(i) = f.idx(r, c) else { break };
                    let (s, e) = f.spans[i];
                    if s == e {
                        if f.styles[i].wide != Wide::Tail {
                            out.push(' ');
                        }
                    } else {
                        out.push_str(&f.text[s as usize..e as usize]);
                    }
                }
                // A soft wrap continues the line; only a hard end breaks it.
                if r != er && !f.wrapped.get(usize::from(r)).copied().unwrap_or(false) {
                    out.push('\n');
                }
            }
            out
        })
    }

    fn scrollback(&self) -> usize {
        self.view
    }

    fn set_scrollback(&mut self, rows: usize) {
        let clamped = rows.min(self.history_rows());
        if clamped != self.view {
            self.view = clamped;
            self.invalidate();
        }
    }
}

fn row_string(frame: &Frame, row: u16) -> String {
    let mut out = String::with_capacity(usize::from(frame.cols));
    for col in 0..frame.cols {
        let Some(i) = frame.idx(row, col) else { break };
        let (start, end) = frame.spans[i];
        if start == end {
            if frame.styles[i].wide != Wide::Tail {
                out.push(' ');
            }
        } else {
            out.push_str(&frame.text[start as usize..end as usize]);
        }
    }
    out.trim_end().to_string()
}

impl Engine for GhosttyEngine {
    type Screen = GhosttyScreen;

    fn new(rows: u16, cols: u16, scrollback_rows: usize) -> Self {
        let mut t: ffi::GhosttyTerminal = std::ptr::null_mut();
        assert_eq!(
            unsafe { ffi::ghostty_terminal_new(std::ptr::null(), &raw mut t, cols, rows) },
            spyc_vt_sys::SUCCESS,
            "ghostty_terminal_new"
        );
        // Both limits, always: rows are the UX contract, bytes the safety
        // valve, and leaving either at its default truncates history.
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
        let mut rs: ffi::GhosttyRenderState = std::ptr::null_mut();
        let mut it: ffi::GhosttyRenderStateRowIterator = std::ptr::null_mut();
        let mut rc: ffi::GhosttyRenderStateRowCells = std::ptr::null_mut();
        unsafe {
            let _ = ffi::ghostty_render_state_new(std::ptr::null(), &raw mut rs);
            let _ = ffi::ghostty_render_state_row_iterator_new(std::ptr::null(), &raw mut it);
            let _ = ffi::ghostty_render_state_row_cells_new(std::ptr::null(), &raw mut rc);
        }
        Self {
            inner: GhosttyScreen {
                t,
                rs,
                it,
                rc,
                rows,
                cols,
                view: 0,
                budget: scrollback_rows,
                frame: RefCell::new(Frame::default()),
                frame_valid: Cell::new(false),
            },
        }
    }

    fn process(&mut self, bytes: &[u8]) {
        unsafe { ffi::ghostty_terminal_vt_write(self.inner.t, bytes.as_ptr(), bytes.len()) };
        self.inner.invalidate();
    }

    fn screen(&self) -> &Self::Screen {
        &self.inner
    }

    fn screen_mut(&mut self) -> &mut Self::Screen {
        &mut self.inner
    }
}

impl GhosttyScreen {
    /// The pane resizes through the screen (vt100's shape); kept so the flip
    /// needs no call-site change.
    pub fn set_size(&mut self, rows: u16, cols: u16) {
        let _ = unsafe { ffi::ghostty_terminal_resize(self.t, cols, rows, 8, 16) };
        self.rows = rows;
        self.cols = cols;
        self.view = 0;
        self.invalidate();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pane::engine::conformance;

    fn fed(rows: u16, cols: u16, bytes: &[u8]) -> GhosttyEngine {
        let mut e = <GhosttyEngine as Engine>::new(rows, cols, 10_000);
        e.process(bytes);
        e
    }

    // The seam's contract suite, against the shipped engine. The same five
    // run against vt100 in `engine_vt100`; one contract, both impls.
    #[test]
    fn reports_what_the_engine_holds() {
        conformance::reports_what_the_engine_holds::<GhosttyEngine>();
    }

    #[test]
    fn past_the_edge_is_absence_not_blankness() {
        conformance::past_the_edge_is_absence_not_blankness::<GhosttyEngine>();
    }

    #[test]
    fn mouse_protocol_maps_through_the_model_enums() {
        conformance::mouse_protocol_maps_through_the_model_enums::<GhosttyEngine>();
    }

    #[test]
    fn set_scrollback_clamps_to_the_real_length() {
        conformance::set_scrollback_clamps_to_the_real_length::<GhosttyEngine>();
    }

    #[test]
    fn reports_the_modes_the_pane_branches_on() {
        conformance::reports_the_modes_the_pane_branches_on::<GhosttyEngine>();
    }

    /// A frame filled by the render state and one filled by coordinate must be
    /// the same frame.
    ///
    /// Two fill paths exist because the render state only ever presents the
    /// live viewport, so history reads cannot use it. Two paths that drift
    /// would give the pane one grid and the scrollback pager another, and
    /// nothing else in the tree would notice — the render is snapshot-tested
    /// against ITSELF. This is the check that does not share their model.
    #[test]
    fn both_fills_agree_on_the_live_viewport() {
        let e = fed(
            6,
            24,
            "\x1b[31mred\x1b[0m \x1b[1;4mbold-u\x1b[0m\r\n\
             wide \u{3042}\u{3044}\u{3046} tail\r\n\
             \x1b[2mdim\x1b[0m \x1b[7mrev\x1b[0m\r\n\
             plain\r\n"
                .as_bytes(),
        );
        let s = e.screen();

        let mut via_rs = Frame::default();
        assert!(
            s.fill_from_render_state(&mut via_rs),
            "the render state fill must succeed on a live viewport"
        );
        let mut via_grid = Frame::default();
        s.fill_from_grid_ref(&mut via_grid);

        assert_eq!(
            (via_rs.rows, via_rs.cols),
            (via_grid.rows, via_grid.cols),
            "geometry"
        );
        assert_eq!(via_rs.styles, via_grid.styles, "cell styles");
        assert_eq!(via_rs.wrapped, via_grid.wrapped, "row wrap flags");
        let text_of = |f: &Frame| {
            (0..f.rows)
                .map(|r| row_string(f, r))
                .collect::<Vec<_>>()
                .join("\n")
        };
        assert_eq!(text_of(&via_rs), text_of(&via_grid), "cell text");
    }

    /// A scrolled-back view reads history, and the offset clamps to what
    /// exists — `ui::scrollback` discovers the length by asking for
    /// `usize::MAX` and reading back.
    #[test]
    fn the_view_offset_walks_history_and_clamps() {
        let mut e = <GhosttyEngine as Engine>::new(3, 12, 10_000);
        for i in 0..20 {
            e.process(format!("L{i:02}\r\n").as_bytes());
        }
        let s = e.screen_mut();
        assert_eq!(s.scrollback(), 0, "starts at the live edge");
        s.set_scrollback(usize::MAX);
        let len = s.scrollback();
        assert!(len > 0 && len <= 20, "clamped to real history, got {len}");
        s.set_scrollback(2);
        assert_eq!(s.scrollback(), 2);
        let text = s.contents();
        assert!(
            text.contains("L16") || text.contains("L15"),
            "a 2-row scrollback shows older rows, got {text:?}"
        );
        s.set_scrollback(0);
        assert!(s.contents().contains("L19"), "back at the live edge");
    }
}

/// The four engine-side defects [#34](https://github.com/Tripstack-Corp/spyc/issues/34)
/// turned out to be, each pinned against the shipped engine.
///
/// The spike found these by differential against the incumbent and named them
/// in `docs/drafts/VT_ENGINE_SPIKE.md`; the bytes here are its fixtures. They
/// are asserted in-crate rather than in `spikes/vt-engine/`, which is excluded
/// from the workspace and so runs only when someone remembers — a defect that
/// took an engine swap to fix deserves a test that runs on every push.
#[cfg(test)]
mod issue_34_engine_defects {
    use super::*;

    fn screen_of(rows: u16, cols: u16, bytes: &[u8]) -> GhosttyEngine {
        let mut e = <GhosttyEngine as Engine>::new(rows, cols, 10_000);
        e.process(bytes);
        e
    }

    /// `ESC ( 0` selects DEC special graphics, so `lqqqk` is a box, not
    /// letters. vt100 does not implement SCS at all and renders the literal
    /// text — garbage in any pane whose child draws boxes.
    #[test]
    fn scs_box_drawing_draws_boxes() {
        let e = screen_of(5, 20, b"\x1b(0lqqqk\r\nx  x\r\nmqqqj\x1b(B\r\n");
        let text = e.screen().contents();
        let row0 = text.lines().next().unwrap_or_default();
        assert_eq!(row0, "┌───┐", "SCS box drawing, got {row0:?}");
        assert!(
            text.lines().nth(2).unwrap_or_default().starts_with('└'),
            "the bottom edge too, got {text:?}"
        );
    }

    /// A row written before a DECSTBM scroll region is set must survive it.
    /// vt100 loses it.
    #[test]
    fn a_row_written_before_decstbm_survives() {
        let e = screen_of(
            8,
            20,
            b"\x1b[2J\x1b[H\x1b[3;6rheader\r\n\x1b[3Hone\r\ntwo\r\nthree\r\nfour\r\nfive\r\nsix\r\nseven\r\neight\r\n",
        );
        let text = e.screen().contents();
        assert!(
            text.contains("header"),
            "the pre-region row must survive, got {text:?}"
        );
    }

    /// Content scrolled out of a TOP-ANCHORED DECSTBM region reaches history.
    /// vt100 retains zero rows, which is why a codex pane's scrollback was
    /// always empty — codex confines its transcript to a scroll region.
    ///
    /// Top-anchored is the whole point, and the first version of this test got
    /// it wrong by reusing the spike's `3;6` fixture. A region that does not
    /// start at row 1 scrolls its lines within the screen, so none of them
    /// leave it and history correctly stays empty — measured at `2;6` and
    /// `3;6`, both 0 rows, against 15 at `1;6`. The engine was right and the
    /// test was wrong.
    #[test]
    fn a_top_anchored_scroll_region_accumulates_scrollback() {
        let mut e = <GhosttyEngine as Engine>::new(8, 20, 10_000);
        e.process(b"\x1b[2J\x1b[H\x1b[1;6r");
        for i in 0..20 {
            e.process(format!("line {i:02}\r\n").as_bytes());
        }
        let s = e.screen_mut();
        s.set_scrollback(usize::MAX);
        assert!(
            s.scrollback() > 0,
            "a scroll-region child must accumulate scrollback, got {}",
            s.scrollback()
        );
    }

    /// A tag-sequence grapheme survives past 18 bytes. vt100's `Cell` has
    /// `CONTENT_BYTES = 22` and drops silently at 18; the Scotland flag needs
    /// 28, so it lost two of its six tag characters.
    #[test]
    fn a_tag_sequence_grapheme_survives_past_eighteen_bytes() {
        let flag = "\u{1F3F4}\u{E0067}\u{E0062}\u{E0073}\u{E0063}\u{E0074}\u{E007F}";
        assert!(flag.len() > 18, "the fixture must exceed the old limit");
        let e = screen_of(3, 20, format!("{flag}|end\r\n").as_bytes());
        let mut got = String::new();
        e.screen().cell_text(0, 0, &mut got);
        assert_eq!(
            got.chars().count(),
            flag.chars().count(),
            "every codepoint of the cluster survives: {got:?}"
        );
        assert_eq!(got, flag);
    }
}
