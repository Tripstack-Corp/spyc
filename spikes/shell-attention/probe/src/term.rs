//! S1, the terminal settings: `tcgetattr` through rustix, the call the jar
//! would make, cross-checked against the C library's on the same descriptor.

use std::os::fd::{AsFd, AsRawFd};

use rustix::termios::{InputModes, LocalModes, OutputModes, SpecialCodeIndex};
use serde_json::{Value, json};

/// The settings on `fd`: the flags the rules read, the raw words, and whether
/// libc's `tcgetattr` read the same `c_lflag`.
pub fn settings(fd: impl AsFd) -> Value {
    crate::count::add(1);
    let t = match rustix::termios::tcgetattr(&fd) {
        Ok(t) => t,
        Err(e) => return json!({"error": crate::count::errno(&e.into())}),
    };
    let l = t.local_modes;
    let i = t.input_modes;
    json!({
        "echo": l.contains(LocalModes::ECHO),
        "icanon": l.contains(LocalModes::ICANON),
        "isig": l.contains(LocalModes::ISIG),
        "iexten": l.contains(LocalModes::IEXTEN),
        "echonl": l.contains(LocalModes::ECHONL),
        "icrnl": i.contains(InputModes::ICRNL),
        "ixon": i.contains(InputModes::IXON),
        "opost": t.output_modes.contains(OutputModes::OPOST),
        "vmin": t.special_codes[SpecialCodeIndex::VMIN],
        "vtime": t.special_codes[SpecialCodeIndex::VTIME],
        "lflag": format!("{:#x}", l.bits()),
        "iflag": format!("{:#x}", i.bits()),
        "oflag": format!("{:#x}", t.output_modes.bits()),
        "libc_lflag_agrees": libc_lflag(fd.as_fd().as_raw_fd()).map(|b| b as u64 == l.bits() as u64),
    })
}

fn libc_lflag(fd: i32) -> Option<libc::tcflag_t> {
    let mut t = std::mem::MaybeUninit::<libc::termios>::uninit();
    // SAFETY: `t` is a writable `termios`; `tcgetattr` only writes it.
    if unsafe { libc::tcgetattr(fd, t.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: `tcgetattr` returned 0, so it filled `t`.
    Some(unsafe { t.assume_init() }.c_lflag)
}
