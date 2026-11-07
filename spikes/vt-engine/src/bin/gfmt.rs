//! What does libghostty's VT formatter emit, and what does each option toggle
//! cost in bytes?
//!
//! Rewritten for the Stage-5 gate against `spyc-vt-sys` — the shipping pin,
//! through the same FFI production uses. The pre-pin version of this probe used
//! the published `libghostty-vt` wrapper, whose bindings are ABI-incompatible
//! with this pin.
//!
//! The per-toggle byte cost is a gate figure, not trivia: it is what makes
//! "capability-parameterized emission" a measurable claim rather than a
//! brochure line. A reattaching client that cannot use a capability should not
//! pay for it.
#[cfg(feature = "ghostty")]
fn main() {
    use spyc_vt_sys::ffi;

    use vt_engine_spike::engines::ghostty_engine::vt_format;

    fn emit(t: ffi::GhosttyTerminal, extras: u32) -> Vec<u8> {
        vt_format(t, extras).unwrap_or_default()
    }

    let mk = || {
        let mut t: ffi::GhosttyTerminal = std::ptr::null_mut();
        assert_eq!(
            unsafe { ffi::ghostty_terminal_new(std::ptr::null(), &raw mut t, 20, 4) },
            spyc_vt_sys::SUCCESS
        );
        let feed = b"\x1b[31mred\x1b[0m line\r\nsecond\r\n";
        unsafe { ffi::ghostty_terminal_vt_write(t, feed.as_ptr(), feed.len()) };
        t
    };

    let toggles: &[(&str, u32)] = &[
        ("bare", ffi::GHOSTTY_FORMATTER_EXTRAS_NONE),
        ("+palette", ffi::GHOSTTY_FORMATTER_EXTRA_PALETTE),
        ("+modes", ffi::GHOSTTY_FORMATTER_EXTRA_MODES),
        ("+scrolling_region", ffi::GHOSTTY_FORMATTER_EXTRA_SCROLLING_REGION),
        ("+tabstops", ffi::GHOSTTY_FORMATTER_EXTRA_TABSTOPS),
        ("+pwd", ffi::GHOSTTY_FORMATTER_EXTRA_PWD),
        ("+keyboard", ffi::GHOSTTY_FORMATTER_EXTRA_KEYBOARD),
        ("+cursor", ffi::GHOSTTY_FORMATTER_EXTRA_CURSOR),
        ("+style", ffi::GHOSTTY_FORMATTER_EXTRA_STYLE),
        ("+hyperlink", ffi::GHOSTTY_FORMATTER_EXTRA_HYPERLINK),
        ("+protection", ffi::GHOSTTY_FORMATTER_EXTRA_PROTECTION),
        ("+kitty_keyboard", ffi::GHOSTTY_FORMATTER_EXTRA_KITTY_KEYBOARD),
        ("+charsets", ffi::GHOSTTY_FORMATTER_EXTRA_CHARSETS),
    ];

    println!(
        "formatter option costs at libghostty-vt {} ({})",
        spyc_vt_sys::pin::GHOSTTY_VT_VERSION,
        spyc_vt_sys::pin::GHOSTTY_COMMIT
    );
    println!("20x4 terminal, two styled lines; each row is `bare` plus ONE toggle");
    println!("{:-<86}", "");
    let base = {
        let t = mk();
        let n = emit(t, ffi::GHOSTTY_FORMATTER_EXTRAS_NONE).len();
        unsafe { ffi::ghostty_terminal_free(t) };
        n
    };
    println!("{:<20} {:>7} {:>9}  {}", "options", "bytes", "delta", "first 46 bytes");
    for &(label, extras) in toggles {
        let t = mk();
        let out = emit(t, extras);
        unsafe { ffi::ghostty_terminal_free(t) };
        let show: String = String::from_utf8_lossy(&out[..out.len().min(46)])
            .escape_debug()
            .collect();
        let delta = out.len() as i64 - base as i64;
        println!("{label:<20} {:>7} {delta:>+9}  {show}", out.len());
    }
    println!("{:-<86}", "");
    println!("A toggle costing 0 has nothing to say about THIS terminal's state;");
    println!("the point is that each is independently switchable for a client that");
    println!("cannot consume it.");
}

#[cfg(not(feature = "ghostty"))]
fn main() {
    eprintln!("build with --features ghostty");
}
