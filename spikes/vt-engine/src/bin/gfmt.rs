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

    /// Build formatter options with one extra toggle enabled.
    fn opts(f: impl Fn(&mut ffi::GhosttyFormatterTerminalExtra)) -> ffi::GhosttyFormatterTerminalOptions {
        let screen = ffi::GhosttyFormatterScreenExtra {
            size: size_of::<ffi::GhosttyFormatterScreenExtra>(),
            cursor: false,
            style: false,
            hyperlink: false,
            protection: false,
            kitty_keyboard: false,
            charsets: false,
        };
        let mut extra = ffi::GhosttyFormatterTerminalExtra {
            size: size_of::<ffi::GhosttyFormatterTerminalExtra>(),
            palette: false,
            modes: false,
            scrolling_region: false,
            tabstops: false,
            pwd: false,
            keyboard: false,
            screen,
        };
        f(&mut extra);
        ffi::GhosttyFormatterTerminalOptions {
            size: size_of::<ffi::GhosttyFormatterTerminalOptions>(),
            emit: ffi::GhosttyFormatterFormat::GHOSTTY_FORMATTER_FORMAT_VT,
            unwrap: false,
            trim: false,
            extra,
            selection: std::ptr::null(),
        }
    }

    fn emit(t: ffi::GhosttyTerminal, o: ffi::GhosttyFormatterTerminalOptions) -> Vec<u8> {
        let mut f: ffi::GhosttyFormatter = std::ptr::null_mut();
        if unsafe { ffi::ghostty_formatter_terminal_new(std::ptr::null(), &raw mut f, t, o) }
            != spyc_vt_sys::SUCCESS
        {
            return Vec::new();
        }
        let mut need: usize = 0;
        let _ =
            unsafe { ffi::ghostty_formatter_format_buf(f, std::ptr::null_mut(), 0, &raw mut need) };
        let mut buf = vec![0u8; need.max(1)];
        let mut wrote: usize = 0;
        let r = unsafe {
            ffi::ghostty_formatter_format_buf(f, buf.as_mut_ptr(), buf.len(), &raw mut wrote)
        };
        unsafe { ffi::ghostty_formatter_free(f) };
        if r == spyc_vt_sys::SUCCESS {
            buf.truncate(wrote);
            buf
        } else {
            Vec::new()
        }
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

    type Toggle = (&'static str, fn(&mut ffi::GhosttyFormatterTerminalExtra));
    let toggles: &[Toggle] = &[
        ("bare", |_| {}),
        ("+palette", |e| e.palette = true),
        ("+modes", |e| e.modes = true),
        ("+scrolling_region", |e| e.scrolling_region = true),
        ("+tabstops", |e| e.tabstops = true),
        ("+pwd", |e| e.pwd = true),
        ("+keyboard", |e| e.keyboard = true),
        ("+cursor", |e| e.screen.cursor = true),
        ("+style", |e| e.screen.style = true),
        ("+hyperlink", |e| e.screen.hyperlink = true),
        ("+protection", |e| e.screen.protection = true),
        ("+kitty_keyboard", |e| e.screen.kitty_keyboard = true),
        ("+charsets", |e| e.screen.charsets = true),
    ];

    println!("formatter option costs at pin {}", spyc_vt_sys::pin::GHOSTTY_COMMIT);
    println!("20x4 terminal, two styled lines; each row is `bare` plus ONE toggle");
    println!("{:-<86}", "");
    let base = {
        let t = mk();
        let n = emit(t, opts(|_| {})).len();
        unsafe { ffi::ghostty_terminal_free(t) };
        n
    };
    println!("{:<20} {:>7} {:>9}  {}", "options", "bytes", "delta", "first 46 bytes");
    for (label, f) in toggles {
        let t = mk();
        let out = emit(t, opts(f));
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
