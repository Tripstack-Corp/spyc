//! Raw FFI to libghostty-vt, at a spyc-owned pinned ghostty commit.
//!
//! This crate is deliberately thin: it owns everything C — the pin, the
//! generated bindings, the vendored static archives and their checksums — and
//! nothing about how spyc uses a terminal. The safe engine wrapper lives above
//! it, so that the whole `unsafe` surface has one home. That placement is the
//! decisions log's standing scope for unsafe: "exceptional and isolated (a
//! future crate split would give it a dedicated crate)".
//!
//! # What is checked at build time
//!
//! * The vendored archive for the host target exists, and its SHA-256 matches
//!   the committed `vendor/CHECKSUMS`. A mismatch fails the build.
//! * The generated bindings carry ~371 compile-time layout assertions — struct
//!   sizes, alignments and field offsets against the pin's headers. An ABI
//!   change that moves a field fails the build rather than returning nonsense,
//!   which is exactly how the published bindings failed silently against a
//!   newer ghostty (see [`pin::BUMP_POLICY`]).
//!
//! # What is NOT checked, and cannot be
//!
//! A C ABI has no version handshake. If a future pin keeps every struct layout
//! but changes a function's *meaning* — the same shape, different semantics —
//! nothing here catches it. That is what re-running the spike harness on a pin
//! bump is for, and why it is a gate rather than a courtesy.

#![allow(clippy::pedantic)]

pub mod pin;
pub mod scrollback;

/// Generated from the pin's headers by `tools/gen_bindings.rs`. Checked in, so
/// neither bindgen nor libclang is a dependency of this crate.
#[allow(
    non_upper_case_globals,
    non_camel_case_types,
    non_snake_case,
    dead_code,
    clippy::all,
    clippy::pedantic,
    unsafe_op_in_unsafe_fn,
    missing_docs
)]
#[rustfmt::skip]
pub mod ffi {
    include!("bindings.rs");
}

/// `GHOSTTY_SUCCESS`, the only result value that means the call worked.
pub const SUCCESS: ffi::GhosttyResult::Type = ffi::GhosttyResult::GHOSTTY_SUCCESS;

#[cfg(test)]
mod smoke {
    use super::{SUCCESS, ffi, scrollback};

    /// Read a `usize`-shaped terminal datum.
    ///
    /// # Safety
    /// `t` must be a live terminal and `tag` must be a datum the pin documents
    /// as `size_t *`.
    unsafe fn get_usize(
        t: ffi::GhosttyTerminal,
        tag: ffi::GhosttyTerminalData::Type,
    ) -> Option<usize> {
        let mut v: usize = 0;
        let r = unsafe { ffi::ghostty_terminal_get(t, tag, (&raw mut v).cast()) };
        (r == SUCCESS).then_some(v)
    }

    /// Read a `u16`-shaped terminal datum.
    ///
    /// # Safety
    /// As [`get_usize`], for a datum documented as `uint16_t *`.
    unsafe fn get_u16(t: ffi::GhosttyTerminal, tag: ffi::GhosttyTerminalData::Type) -> Option<u16> {
        let mut v: u16 = 0;
        let r = unsafe { ffi::ghostty_terminal_get(t, tag, (&raw mut v).cast()) };
        (r == SUCCESS).then_some(v)
    }

    /// Geometry survives the round trip through the constructor.
    ///
    /// The cheapest possible test and the one that matters most: this is
    /// precisely what the published `libghostty-vt-sys 0.2.1` bindings get
    /// WRONG against a post-refactor ghostty. They pass an options struct the
    /// library no longer accepts, so `rows()` comes back as the scrollback
    /// budget instead of the row count — compiling, running, and lying. If the
    /// pin ever moves under these bindings in the same way, this fails.
    #[test]
    fn geometry_round_trips_through_the_constructor() {
        for (cols, rows) in [(80u16, 24u16), (60, 6), (200, 50), (1, 1)] {
            let mut t: ffi::GhosttyTerminal = std::ptr::null_mut();
            let r = unsafe { ffi::ghostty_terminal_new(std::ptr::null(), &raw mut t, cols, rows) };
            assert_eq!(r, SUCCESS, "terminal_new({cols}, {rows}) failed with {r}");
            assert!(!t.is_null());
            unsafe { ffi::ghostty_terminal_vt_write(t, b"hi\r\n".as_ptr(), 4) };
            assert_eq!(
                unsafe { get_u16(t, ffi::GhosttyTerminalData::GHOSTTY_TERMINAL_DATA_COLS) },
                Some(cols),
                "cols round-trip at {cols}x{rows}"
            );
            assert_eq!(
                unsafe { get_u16(t, ffi::GhosttyTerminalData::GHOSTTY_TERMINAL_DATA_ROWS) },
                Some(rows),
                "rows round-trip at {cols}x{rows} — if this reports the scrollback \
                 budget instead, the bindings and the archive disagree"
            );
            unsafe { ffi::ghostty_terminal_free(t) };
        }
    }

    /// The derived limits are accepted by the library and read back verbatim.
    ///
    /// Pins the mapping end to end rather than just the arithmetic: a limit the
    /// library silently rejects would leave the defaults in place, and the
    /// default byte cap is what truncates history to ~840 rows.
    #[test]
    fn the_derived_limits_are_accepted_and_read_back() {
        let l = scrollback::limits_for_row_budget(10_000);
        let mut t: ffi::GhosttyTerminal = std::ptr::null_mut();
        assert_eq!(
            unsafe { ffi::ghostty_terminal_new(std::ptr::null(), &raw mut t, 80, 24) },
            SUCCESS
        );

        let bytes = l.max_bytes;
        assert_eq!(
            unsafe {
                ffi::ghostty_terminal_set(
                    t,
                    ffi::GhosttyTerminalOption::GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_BYTES,
                    (&raw const bytes).cast(),
                )
            },
            SUCCESS,
            "the byte ceiling must be accepted"
        );
        let lines = l.max_lines;
        assert_eq!(
            unsafe {
                ffi::ghostty_terminal_set(
                    t,
                    ffi::GhosttyTerminalOption::GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_LINES,
                    (&raw const lines).cast(),
                )
            },
            SUCCESS,
            "the line limit must be accepted"
        );

        assert_eq!(
            unsafe {
                get_usize(
                    t,
                    ffi::GhosttyTerminalData::GHOSTTY_TERMINAL_DATA_SCROLLBACK_MAX_LINES,
                )
            },
            Some(l.max_lines),
            "line limit reads back verbatim"
        );
        assert_eq!(
            unsafe {
                get_usize(
                    t,
                    ffi::GhosttyTerminalData::GHOSTTY_TERMINAL_DATA_SCROLLBACK_MAX_BYTES,
                )
            },
            Some(l.max_bytes),
            "byte ceiling reads back verbatim"
        );
        unsafe { ffi::ghostty_terminal_free(t) };
    }

    /// Malformed input does not crash the parser.
    ///
    /// The pin documents `vt_write` as never failing — "malformed input [cannot]
    /// corrupt or crash" — and that promise is a load-bearing part of why this
    /// engine was chosen over one that panics on 2.87% of random escape
    /// streams. Asserted here in miniature; the full 50k-iteration differential
    /// lives in `spikes/vt-engine/`.
    #[test]
    fn malformed_input_does_not_crash() {
        let mut t: ffi::GhosttyTerminal = std::ptr::null_mut();
        assert_eq!(
            unsafe { ffi::ghostty_terminal_new(std::ptr::null(), &raw mut t, 1, 1) },
            SUCCESS
        );
        for junk in [
            b"\x1b[999999999999;0;0H".as_slice(),
            b"\x1b[?\x1b[?\x1b[?".as_slice(),
            b"\x1b]52;;\x1b]52".as_slice(),
            b"\x1b_G\x1b_G\x1b_G".as_slice(),
            &[0xff, 0xfe, 0xfd, 0x00, 0x1b, 0x5b],
            "\u{3042}\u{1F600}e\u{0301}".as_bytes(),
        ] {
            unsafe { ffi::ghostty_terminal_vt_write(t, junk.as_ptr(), junk.len()) };
        }
        // Still answering questions afterwards is the actual assertion.
        assert_eq!(
            unsafe { get_u16(t, ffi::GhosttyTerminalData::GHOSTTY_TERMINAL_DATA_COLS) },
            Some(1)
        );
        unsafe { ffi::ghostty_terminal_free(t) };
    }
}
