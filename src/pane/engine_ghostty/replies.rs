//! The pane's answers to the child's terminal queries (#486).
//!
//! libghostty-vt answers a query by calling `write_pty` from inside
//! `ghostty_terminal_vt_write`, with the pane's engine lock held. The callback
//! only buffers: the parser worker takes the buffer once the write returns and
//! queues it on the pty's input writer, so nothing here touches the pty.
//!
//! Only answers true of spyc's pane are sent; see [`answered`] for the ones
//! libghostty-vt could give that would not be.

use std::os::raw::c_void;

use spyc_vt_sys::ffi::{
    self, GhosttyDeviceAttributes, GhosttyDeviceAttributesPrimary,
    GhosttyDeviceAttributesSecondary, GhosttyDeviceAttributesTertiary, GhosttyString,
    GhosttyTerminalOption as Opt,
};

/// The most reply bytes one `process` may leave pending, so a child flooding
/// queries can't grow the buffer without limit.
const MAX_PENDING: usize = 64 * 1024;

/// XTVERSION's name. Not `ghostty` or `kitty`: children key image support off
/// those names, and spyc draws no images.
const XTVERSION: &str = concat!("spyc ", env!("CARGO_PKG_VERSION"));

/// Replies buffered by `write_pty`, waiting for the parser worker.
#[derive(Default)]
pub(super) struct Sink {
    buf: Vec<u8>,
}

impl Sink {
    pub(super) fn take(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.buf)
    }
}

/// Whether spyc sends a reply libghostty-vt produced. An allowlist, so an answer
/// a pin bump teaches the engine stays unsent until someone judges it here.
///
/// Left out on purpose: the kitty keyboard flags (spyc encodes legacy keys
/// only), XTGETTCAP (answered from Ghostty's terminfo, which advertises OSC 52
/// clipboard spyc lacks), colour reports (Ghostty's palette, not the host's),
/// size reports (spyc's cell pixel size is a placeholder), and DA3 (Ghostty
/// doesn't answer it either).
pub(super) fn answered(reply: &[u8]) -> bool {
    if let Some(body) = reply.strip_prefix(b"\x1b[") {
        return match body.split_last() {
            // DECRPM, the answer to DECRQM.
            Some((b'y', rest)) => rest.ends_with(b"$"),
            // Cursor position.
            Some((b'R', rest)) => rest.iter().all(|b| b.is_ascii_digit() || *b == b';'),
            // Primary and secondary device attributes.
            Some((b'c', rest)) => matches!(rest.first(), Some(b'?' | b'>')),
            // Operating status; the other `n` reports are colour scheme and visibility.
            Some((b'n', rest)) => rest == b"0",
            _ => false,
        };
    }
    // XTVERSION and DECRQSS.
    reply.starts_with(b"\x1bP>|")
        || reply.starts_with(b"\x1bP1$r")
        || reply.starts_with(b"\x1bP0$r")
}

/// Point `t`'s replies at `sink`, and switch off the image protocols spyc can't
/// draw, so the engine doesn't claim them by answering their queries.
pub(super) fn install(t: ffi::GhosttyTerminal, sink: *mut Sink) {
    let no_images: u64 = 0;
    let no_glyphs = false;
    unsafe {
        ffi::ghostty_terminal_set(
            t,
            Opt::GHOSTTY_TERMINAL_OPT_USERDATA,
            sink.cast_const().cast(),
        );
        ffi::ghostty_terminal_set(
            t,
            Opt::GHOSTTY_TERMINAL_OPT_WRITE_PTY,
            write_pty as *const c_void,
        );
        ffi::ghostty_terminal_set(
            t,
            Opt::GHOSTTY_TERMINAL_OPT_XTVERSION,
            xtversion as *const c_void,
        );
        ffi::ghostty_terminal_set(
            t,
            Opt::GHOSTTY_TERMINAL_OPT_DEVICE_ATTRIBUTES,
            device_attributes as *const c_void,
        );
        ffi::ghostty_terminal_set(
            t,
            Opt::GHOSTTY_TERMINAL_OPT_KITTY_IMAGE_STORAGE_LIMIT,
            (&raw const no_images).cast(),
        );
        ffi::ghostty_terminal_set(
            t,
            Opt::GHOSTTY_TERMINAL_OPT_GLYPH_PROTOCOL,
            (&raw const no_glyphs).cast(),
        );
    }
}

unsafe extern "C" fn write_pty(
    _t: ffi::GhosttyTerminal,
    userdata: *mut c_void,
    data: *const u8,
    len: usize,
) {
    if data.is_null() || len == 0 {
        return;
    }
    // SAFETY: `userdata` is the engine's `Sink`, alive as long as the terminal.
    // libghostty-vt calls this only inside `vt_write`, which runs under
    // `&mut GhosttyEngine`, so nothing else holds a reference to the sink.
    let sink = unsafe { &mut *userdata.cast::<Sink>() };
    let reply = unsafe { std::slice::from_raw_parts(data, len) };
    if answered(reply) && sink.buf.len() + reply.len() <= MAX_PENDING {
        sink.buf.extend_from_slice(reply);
    }
}

const unsafe extern "C" fn xtversion(
    _t: ffi::GhosttyTerminal,
    _userdata: *mut c_void,
) -> GhosttyString {
    GhosttyString {
        ptr: XTVERSION.as_ptr(),
        len: XTVERSION.len(),
    }
}

/// Ghostty's own answers less clipboard access (52), which spyc doesn't offer:
/// a level 2 (VT220) terminal with colour.
const unsafe extern "C" fn device_attributes(
    _t: ffi::GhosttyTerminal,
    _userdata: *mut c_void,
    out: *mut GhosttyDeviceAttributes,
) -> bool {
    if out.is_null() {
        return false;
    }
    let mut features = [0u16; 64];
    features[0] = 22;
    let attrs = GhosttyDeviceAttributes {
        primary: GhosttyDeviceAttributesPrimary {
            conformance_level: 62,
            features,
            num_features: 1,
        },
        secondary: GhosttyDeviceAttributesSecondary {
            device_type: 1,
            firmware_version: 10,
            rom_cartridge: 0,
        },
        tertiary: GhosttyDeviceAttributesTertiary { unit_id: 0 },
    };
    // SAFETY: libghostty-vt passes a valid, writable struct.
    unsafe { out.write(attrs) };
    true
}

#[cfg(test)]
mod tests {
    use super::answered;

    #[test]
    fn the_allowlist_admits_only_true_answers() {
        let sent: &[&[u8]] = &[
            b"\x1b[?2027;1$y",
            b"\x1b[4;2$y",
            b"\x1b[12;40R",
            b"\x1b[?62;22c",
            b"\x1b[>1;10;0c",
            b"\x1b[0n",
            b"\x1bP>|spyc 1.0\x1b\\",
            b"\x1bP1$r2;10r\x1b\\",
            b"\x1bP0$r\x1b\\",
        ];
        for r in sent {
            assert!(answered(r), "dropped {:?}", String::from_utf8_lossy(r));
        }
        let unsent: &[&[u8]] = &[
            b"\x1b[?0u",
            b"\x1b[?997;1n",
            b"\x1b[8;24;80t",
            b"\x1b[4;384;640t",
            b"\x1b[48;24;80;384;640t",
            b"\x1bP1+r4d73=\x1b\\",
            b"\x1bP0+r\x1b\\",
            b"\x1bP!|00000000\x1b\\",
            b"\x1b]4;1;rgb:cc/00/00\x1b\\",
            b"\x1b]11;rgb:00/00/00\x1b\\",
            b"\x1b]l title\x1b\\",
            b"\x1b_Gi=31;OK\x1b\\",
            b"\x1b_25a1;s;fmt=glyf\x1b\\",
            b"\x1b[",
            b"",
        ];
        for r in unsent {
            assert!(!answered(r), "sent {:?}", String::from_utf8_lossy(r));
        }
    }
}
