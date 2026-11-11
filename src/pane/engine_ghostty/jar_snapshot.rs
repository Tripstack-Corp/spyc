//! Jar spike: the engine's whole state, scrollback included, as libghostty's
//! binary snapshot, and back. A snapshot is valid only for the same build and
//! pin (`JAR_PROTOCOL.md`, R17a); the decoder refuses another version's.

use spyc_vt_sys::{ffi, scrollback};

use super::{GhosttyEngine, GhosttyTerminalModeConfig, MODE_GRAPHEME_CLUSTER, Opt, replies};

impl GhosttyEngine {
    /// The engine's state, or `None` when libghostty can't encode it.
    pub fn encode_snapshot(&self) -> Option<Vec<u8>> {
        let mut ptr: *mut u8 = std::ptr::null_mut();
        let mut len = 0usize;
        // SAFETY: `t` is live; a null allocator selects the default one, and
        // `ptr` and `len` are writable slots.
        let r = unsafe {
            ffi::ghostty_snapshot_encode_alloc(
                self.inner.t,
                std::ptr::null(),
                &raw mut ptr,
                &raw mut len,
            )
        };
        if r != spyc_vt_sys::SUCCESS || ptr.is_null() {
            return None;
        }
        // SAFETY: a successful encode left `len` readable bytes at `ptr`.
        let out = unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec();
        // SAFETY: freed once, with the default allocator it was made with.
        unsafe { ffi::ghostty_free(std::ptr::null(), ptr, len) };
        Some(out)
    }

    /// The engine's size in rows and columns.
    pub const fn size(&self) -> (u16, u16) {
        (self.inner.rows, self.inner.cols)
    }

    /// Replace the engine's terminal with the one `blob` encodes, taken at
    /// `rows` by `cols`, keeping spyc's own settings: scrollback limits,
    /// grapheme clustering and the reply callbacks. `false`, and the engine
    /// unchanged, when the snapshot doesn't decode.
    pub fn restore_snapshot(&mut self, blob: &[u8], rows: u16, cols: u16) -> bool {
        let Some(t) = decode(blob) else {
            return false;
        };
        let cluster = GhosttyTerminalModeConfig {
            mode: MODE_GRAPHEME_CLUSTER,
            value: true,
        };
        let limits = scrollback::limits_for_row_budget(self.inner.budget.max(1));
        let (bytes, lines) = (limits.max_bytes, limits.max_lines);
        // SAFETY: `t` is live from `decode`; each option reads its value
        // during the call.
        unsafe {
            ffi::ghostty_terminal_set(
                t,
                Opt::GHOSTTY_TERMINAL_OPT_MODE_DEFAULT,
                (&raw const cluster).cast(),
            );
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
        replies::install(t, self.inner.replies);
        let old = std::mem::replace(&mut self.inner.t, t);
        // SAFETY: `old` was the engine's live terminal and nothing refers to
        // it now; it is freed once.
        unsafe { ffi::ghostty_terminal_free(old) };
        self.inner.rows = rows;
        self.inner.cols = cols;
        self.inner.view = 0;
        self.inner.frame_valid.set(false);
        self.inner.frame_live.set(false);
        self.inner.synchronized = false;
        true
    }
}

/// Whether `blob` decodes, without keeping what it decodes to.
pub fn snapshot_decodes(blob: &[u8]) -> bool {
    decode(blob).is_some_and(|t| {
        // SAFETY: `t` came live from `decode` and is freed once.
        unsafe { ffi::ghostty_terminal_free(t) };
        true
    })
}

fn decode(blob: &[u8]) -> Option<ffi::GhosttyTerminal> {
    let mut dec: ffi::GhosttySnapshotDecoder = std::ptr::null_mut();
    // SAFETY: a null allocator selects the default one; `blob` is readable for
    // its length during the call, and `dec` is a writable slot.
    let r = unsafe {
        ffi::ghostty_snapshot_decoder_new_buf(
            std::ptr::null(),
            &raw mut dec,
            blob.as_ptr(),
            blob.len(),
        )
    };
    if r != spyc_vt_sys::SUCCESS || dec.is_null() {
        return None;
    }
    let mut t: ffi::GhosttyTerminal = std::ptr::null_mut();
    // SAFETY: `dec` is live and `t` is a writable slot.
    let r = unsafe { ffi::ghostty_snapshot_decoder_decode(dec, &raw mut t) };
    // SAFETY: `dec` is live and freed once; the terminal it decoded is ours.
    unsafe { ffi::ghostty_snapshot_decoder_free(dec) };
    (r == spyc_vt_sys::SUCCESS && !t.is_null()).then_some(t)
}
