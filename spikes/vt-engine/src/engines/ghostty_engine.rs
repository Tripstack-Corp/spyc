//! Candidate C adapter: libghostty-vt, through spyc's own `spyc-vt-sys`.
//!
//! Rewritten for the Stage-5 gate. The spike originally used the published
//! `libghostty-vt` wrapper at ghostty `f4c68d65`, chosen for ABI compatibility
//! with those bindings — and at that commit `max_scrollback` is inert, so every
//! figure taken there was measured against a configuration that cannot ship.
//! This adapter goes through `spyc-vt-sys`: the shipping pin, bindings generated
//! from that pin's headers, the same vendored archive production links. The
//! measured mechanism is the shipped mechanism.
//!
//! Raw FFI rather than a safe wrapper, because `spyc-vt-sys` is deliberately
//! thin and the safe engine wrapper is PR 15's job, not the harness's.

use spyc_vt_sys::ffi::{
    GhosttyCellData as CellData, GhosttyFormatterFormat as FmtFormat,
    GhosttyFormatterScreenExtra, GhosttyFormatterTerminalExtra,
    GhosttyFormatterTerminalOptions, GhosttyPoint, GhosttyPointCoordinate, GhosttyPointTag,
    GhosttyRowData as RowData, GhosttyTerminalData as Data, GhosttyTerminalOption as Opt,
    GhosttyTerminalScreen as ScreenKind,
};
use spyc_vt_sys::{ffi, scrollback};

use crate::engine::{Cell, Color, Engine, Screen, Wide};


pub struct GhosttyEngine {
    t: ffi::GhosttyTerminal,
    rows: u16,
    cols: u16,
}

impl Drop for GhosttyEngine {
    fn drop(&mut self) {
        if !self.t.is_null() {
            unsafe { ffi::ghostty_terminal_free(self.t) };
        }
    }
}

fn get_u16(t: ffi::GhosttyTerminal, tag: Data::Type) -> Option<u16> {
    let mut v: u16 = 0;
    let ok = unsafe { ffi::ghostty_terminal_get(t, tag, (&raw mut v).cast()) };
    (ok == spyc_vt_sys::SUCCESS).then_some(v)
}

fn get_usize(t: ffi::GhosttyTerminal, tag: Data::Type) -> Option<usize> {
    let mut v: usize = 0;
    let ok = unsafe { ffi::ghostty_terminal_get(t, tag, (&raw mut v).cast()) };
    (ok == spyc_vt_sys::SUCCESS).then_some(v)
}

fn get_bool(t: ffi::GhosttyTerminal, tag: Data::Type) -> Option<bool> {
    let mut v: bool = false;
    let ok = unsafe { ffi::ghostty_terminal_get(t, tag, (&raw mut v).cast()) };
    (ok == spyc_vt_sys::SUCCESS).then_some(v)
}

/// A zeroed `GhosttyGridRef` with its `size` set. ghostty's extensible structs
/// carry their own byte length as the first field and the callee reads it, so a
/// zero there would tell the library the struct has no fields.
fn empty_grid_ref() -> ffi::GhosttyGridRef {
    ffi::GhosttyGridRef {
        size: size_of::<ffi::GhosttyGridRef>(),
        node: std::ptr::null_mut(),
        x: 0,
        y: 0,
    }
}

/// The default style, initialised by the library rather than by us.
fn default_style() -> ffi::GhosttyStyle {
    let mut st: ffi::GhosttyStyle = unsafe { std::mem::zeroed() };
    st.size = size_of::<ffi::GhosttyStyle>();
    unsafe { ffi::ghostty_style_default(&raw mut st) };
    st
}

fn point(tag: GhosttyPointTag::Type, x: u16, y: u32) -> GhosttyPoint {
    GhosttyPoint {
        tag,
        value: ffi::GhosttyPointValue {
            coordinate: GhosttyPointCoordinate { x, y },
        },
    }
}

fn color(c: ffi::GhosttyStyleColor) -> Color {
    // GhosttyStyleColor is a tagged union: NONE / PALETTE / RGB.
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

impl GhosttyEngine {
    /// Read one cell in the given point space.
    fn cell_at(&self, tag: GhosttyPointTag::Type, x: u16, y: u32) -> Cell {
        let mut gr = empty_grid_ref();
        let p = point(tag, x, y);
        if unsafe { ffi::ghostty_terminal_grid_ref(self.t, p, &raw mut gr) } != spyc_vt_sys::SUCCESS
        {
            return Cell::default();
        }

        // Graphemes come back as codepoints, so a cluster is assembled here.
        let mut buf = [0u32; 32];
        let mut n: usize = 0;
        let text = if unsafe {
            ffi::ghostty_grid_ref_graphemes(&raw const gr, buf.as_mut_ptr(), buf.len(), &raw mut n)
        } == spyc_vt_sys::SUCCESS
        {
            buf[..n].iter().filter_map(|c| char::from_u32(*c)).collect()
        } else {
            String::new()
        };

        let mut raw_cell: ffi::GhosttyCell = 0;
        let wide = if unsafe { ffi::ghostty_grid_ref_cell(&raw const gr, &raw mut raw_cell) }
            == spyc_vt_sys::SUCCESS
        {
            let mut w: ffi::GhosttyCellWide::Type = 0;
            if unsafe {
                ffi::ghostty_cell_get(raw_cell, CellData::GHOSTTY_CELL_DATA_WIDE, (&raw mut w).cast())
            } == spyc_vt_sys::SUCCESS
            {
                match w {
                    ffi::GhosttyCellWide::GHOSTTY_CELL_WIDE_WIDE => Wide::Head,
                    ffi::GhosttyCellWide::GHOSTTY_CELL_WIDE_SPACER_TAIL => Wide::Tail,
                    _ => Wide::Narrow,
                }
            } else {
                Wide::Narrow
            }
        } else {
            Wide::Narrow
        };

        let mut st = default_style();
        // A cell with no explicit styling leaves the default in place.
        let _ = unsafe { ffi::ghostty_grid_ref_style(&raw const gr, &raw mut st) };

        Cell {
            // A spacer tail carries no glyph of its own; normalised to empty so
            // the comparison against vt100's empty continuation is like-for-like.
            text: if wide == Wide::Tail { String::new() } else { text },
            fg: color(st.fg_color),
            bg: color(st.bg_color),
            bold: st.bold,
            italic: st.italic,
            underline: st.underline != ffi::GhosttySgrUnderline::GHOSTTY_SGR_UNDERLINE_NONE,
            reverse: st.inverse,
            wide,
        }
    }

    fn row_wrapped(&self, y: u32) -> bool {
        let mut gr = empty_grid_ref();
        let p = point(GhosttyPointTag::GHOSTTY_POINT_TAG_VIEWPORT, 0, y);
        if unsafe { ffi::ghostty_terminal_grid_ref(self.t, p, &raw mut gr) } != spyc_vt_sys::SUCCESS
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

    /// Apply the SHIPPED scrollback configuration — both limits, derived from
    /// the row budget. Leaving either at its default is what truncates history
    /// to ~840 rows, and the gate measures the shipped configuration.
    fn apply_shipped_limits(t: ffi::GhosttyTerminal, budget: usize) {
        let l = scrollback::limits_for_row_budget(budget);
        let bytes = l.max_bytes;
        let lines = l.max_lines;
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
    }

    /// Expose the shipped limits so the probes can report what they ran with.
    pub fn shipped_limits(budget: usize) -> scrollback::Limits {
        scrollback::limits_for_row_budget(budget)
    }

    /// Create at the library's OWN defaults — no `ghostty_terminal_set` at
    /// all, so a probe can show behaviour that is not downstream of spyc's
    /// scrollback configuration.
    pub fn create_at_defaults(rows: u16, cols: u16) -> Self {
        let mut t: ffi::GhosttyTerminal = std::ptr::null_mut();
        let ok = unsafe { ffi::ghostty_terminal_new(std::ptr::null(), &raw mut t, cols, rows) };
        assert_eq!(ok, spyc_vt_sys::SUCCESS, "ghostty_terminal_new failed");
        Self { t, rows, cols }
    }

    /// Create at an explicit budget, for `sbprobe`'s sweep.
    pub fn create_with_budget(rows: u16, cols: u16, budget: usize) -> Self {
        let mut t: ffi::GhosttyTerminal = std::ptr::null_mut();
        let ok = unsafe { ffi::ghostty_terminal_new(std::ptr::null(), &raw mut t, cols, rows) };
        assert_eq!(ok, spyc_vt_sys::SUCCESS, "ghostty_terminal_new failed");
        Self::apply_shipped_limits(t, budget);
        Self { t, rows, cols }
    }

    /// Encode this terminal's state with the snapshot API.
    pub fn snapshot(&self) -> Option<Vec<u8>> {
        let mut need: usize = 0;
        let _ = unsafe {
            ffi::ghostty_snapshot_encode_buf(self.t, std::ptr::null_mut(), 0, &raw mut need)
        };
        let mut buf = vec![0u8; need.max(1)];
        let mut wrote: usize = 0;
        let r = unsafe {
            ffi::ghostty_snapshot_encode_buf(self.t, buf.as_mut_ptr(), buf.len(), &raw mut wrote)
        };
        (r == spyc_vt_sys::SUCCESS).then(|| {
            buf.truncate(wrote);
            buf
        })
    }

    /// Turn on continuation tracking, so an unfinished VT or UTF-8 sequence
    /// survives into a snapshot.
    ///
    /// Must be called BEFORE the input that leaves the parser unfinished:
    /// `ghostty_snapshot_encode_*` returns `GHOSTTY_INVALID_VALUE` for a
    /// non-ground parser whose tracking was off when that input arrived. So a
    /// consumer that wants crash-safe snapshots enables this up front and pays
    /// for it continuously — it cannot be switched on at snapshot time.
    pub fn enable_continuation_tracking(&mut self, max_bytes: usize) -> bool {
        let m = max_bytes;
        let r = unsafe {
            ffi::ghostty_terminal_set(
                self.t,
                Opt::GHOSTTY_TERMINAL_OPT_CONTINUATION_MAX_BYTES,
                (&raw const m).cast(),
            )
        };
        r == spyc_vt_sys::SUCCESS
    }

    /// Adopt a raw terminal handle the caller already owns.
    ///
    /// Only for probes that drive the C API directly and then want the trait's
    /// screen reader — the continuation round trip decodes with options the
    /// `from_snapshot` helper does not set. Takes ownership: `Drop` frees it.
    pub fn from_raw_for_compare(t: ffi::GhosttyTerminal, rows: u16, cols: u16) -> Self {
        Self { t, rows, cols }
    }

    /// Decode a snapshot into a fresh engine.
    pub fn from_snapshot(bytes: &[u8], rows: u16, cols: u16) -> Option<Self> {
        let mut dec: ffi::GhosttySnapshotDecoder = std::ptr::null_mut();
        let r = unsafe {
            ffi::ghostty_snapshot_decoder_new_buf(
                std::ptr::null(),
                &raw mut dec,
                bytes.as_ptr(),
                bytes.len(),
            )
        };
        if r != spyc_vt_sys::SUCCESS {
            return None;
        }
        let mut t: ffi::GhosttyTerminal = std::ptr::null_mut();
        let r = unsafe { ffi::ghostty_snapshot_decoder_decode(dec, &raw mut t) };
        unsafe { ffi::ghostty_snapshot_decoder_free(dec) };
        (r == spyc_vt_sys::SUCCESS && !t.is_null()).then_some(Self { t, rows, cols })
    }
}

impl Engine for GhosttyEngine {
    fn name(&self) -> &'static str {
        "ghostty"
    }

    fn create(rows: u16, cols: u16, _scrollback: usize) -> Self {
        // The trait's `scrollback` argument is a ROW budget (vt100's third
        // constructor parameter). ghostty is configured from the same number
        // through the derivation rather than handed it raw, which is the whole
        // point of `spyc_vt_sys::scrollback`.
        Self::create_with_budget(rows, cols, _scrollback.max(1))
    }

    fn feed(&mut self, bytes: &[u8]) {
        unsafe { ffi::ghostty_terminal_vt_write(self.t, bytes.as_ptr(), bytes.len()) };
    }

    fn resize(&mut self, rows: u16, cols: u16) {
        let _ = unsafe { ffi::ghostty_terminal_resize(self.t, cols, rows, 8, 16) };
        self.rows = rows;
        self.cols = cols;
    }

    fn screen(&mut self) -> Screen {
        let rows = get_u16(self.t, Data::GHOSTTY_TERMINAL_DATA_ROWS).unwrap_or(self.rows);
        let cols = get_u16(self.t, Data::GHOSTTY_TERMINAL_DATA_COLS).unwrap_or(self.cols);
        let mut cells = Vec::with_capacity(usize::from(rows) * usize::from(cols));
        let mut wrapped = Vec::with_capacity(usize::from(rows));
        for y in 0..rows {
            for x in 0..cols {
                cells.push(self.cell_at(
                    GhosttyPointTag::GHOSTTY_POINT_TAG_VIEWPORT,
                    x,
                    u32::from(y),
                ));
            }
            wrapped.push(self.row_wrapped(u32::from(y)));
        }
        let mut active: ScreenKind::Type = 0;
        let alt = unsafe {
            ffi::ghostty_terminal_get(
                self.t,
                Data::GHOSTTY_TERMINAL_DATA_ACTIVE_SCREEN,
                (&raw mut active).cast(),
            )
        } == spyc_vt_sys::SUCCESS
            && active == ScreenKind::GHOSTTY_TERMINAL_SCREEN_ALTERNATE;
        Screen {
            rows,
            cols,
            cursor: (
                get_u16(self.t, Data::GHOSTTY_TERMINAL_DATA_CURSOR_Y).unwrap_or(0),
                get_u16(self.t, Data::GHOSTTY_TERMINAL_DATA_CURSOR_X).unwrap_or(0),
            ),
            cursor_visible: get_bool(self.t, Data::GHOSTTY_TERMINAL_DATA_CURSOR_VISIBLE)
                .unwrap_or(true),
            alt_screen: alt,
            cells,
            wrapped,
        }
    }

    fn scrollback_rows(&mut self) -> usize {
        get_usize(self.t, Data::GHOSTTY_TERMINAL_DATA_SCROLLBACK_ROWS).unwrap_or(0)
    }

    fn rehydrate(&mut self) -> Option<Vec<u8>> {
        // The VT-formatter mechanism. `with_tabstops` is ON here, unlike the
        // pre-pin run: the bug that made it clobber the restored cursor is
        // fixed at this pin, re-checked before the gate ran. Palette stays OFF
        // — 256 OSC 4 sequences restore the CLIENT's palette, which is a
        // capability choice rather than terminal state.
        let screen = GhosttyFormatterScreenExtra {
            size: size_of::<GhosttyFormatterScreenExtra>(),
            cursor: true,
            style: true,
            hyperlink: true,
            protection: true,
            kitty_keyboard: true,
            charsets: true,
        };
        let extra = GhosttyFormatterTerminalExtra {
            size: size_of::<GhosttyFormatterTerminalExtra>(),
            palette: false,
            modes: true,
            scrolling_region: true,
            tabstops: true,
            pwd: false,
            keyboard: true,
            screen,
        };
        let opts = GhosttyFormatterTerminalOptions {
            size: size_of::<GhosttyFormatterTerminalOptions>(),
            emit: FmtFormat::GHOSTTY_FORMATTER_FORMAT_VT,
            unwrap: false,
            trim: false,
            extra,
            selection: std::ptr::null(),
        };
        let mut f: ffi::GhosttyFormatter = std::ptr::null_mut();
        if unsafe {
            ffi::ghostty_formatter_terminal_new(std::ptr::null(), &raw mut f, self.t, opts)
        } != spyc_vt_sys::SUCCESS
        {
            return None;
        }
        let mut need: usize = 0;
        let _ = unsafe {
            ffi::ghostty_formatter_format_buf(f, std::ptr::null_mut(), 0, &raw mut need)
        };
        let mut buf = vec![0u8; need.max(1)];
        let mut wrote: usize = 0;
        let r =
            unsafe { ffi::ghostty_formatter_format_buf(f, buf.as_mut_ptr(), buf.len(), &raw mut wrote) };
        unsafe { ffi::ghostty_formatter_free(f) };
        (r == spyc_vt_sys::SUCCESS).then(|| {
            buf.truncate(wrote);
            buf
        })
    }

    fn full_text(&mut self) -> Option<Vec<String>> {
        let cols = get_u16(self.t, Data::GHOSTTY_TERMINAL_DATA_COLS).unwrap_or(self.cols);
        let total = get_usize(self.t, Data::GHOSTTY_TERMINAL_DATA_TOTAL_ROWS).unwrap_or(0);
        let mut out = Vec::with_capacity(total);
        for y in 0..total {
            let y32 = u32::try_from(y).ok()?;
            let mut line = String::new();
            for x in 0..cols {
                let c = self.cell_at(GhosttyPointTag::GHOSTTY_POINT_TAG_SCREEN, x, y32);
                match c.wide {
                    Wide::Tail => {}
                    _ if c.text.is_empty() => line.push(' '),
                    _ => line.push_str(&c.text),
                }
            }
            out.push(line.trim_end().to_string());
        }
        Some(out)
    }
}
