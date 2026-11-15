//! A pane's engine as a jar snapshot (`JAR_PROTOCOL.md`, R17 to R17c), in both
//! forms every pane keeps (decision I).
//!
//! The binary form is libghostty's own snapshot of the whole terminal,
//! scrollback included. It is taken under the pane's parser lock, and only the
//! same build and pin can decode it (R17a). The portable form is VT that any
//! version replays: the formatter's emit, plus what its extras don't carry
//! (R17c). It is made from the binary form in a scratch engine, so the pane's
//! own engine stays unlocked meanwhile. `jar_fidelity` checks it against the
//! binary form case by case.
#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "no pane takes or restores a snapshot in this build"
    )
)]

use spyc_vt_sys::ffi;

use super::{Data, GhosttyEngine, GhosttyScreen, MODE_SYNCHRONIZED_OUTPUT, configure};
use crate::pane::engine::{Engine, TerminalScreen};

impl GhosttyEngine {
    /// The binary form, or `None` when libghostty can't encode the terminal.
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

    /// The portable form of the binary snapshot `blob`, or `None` when `blob`
    /// doesn't decode in this build. `scrollback_rows` is the history the pane
    /// keeps.
    ///
    /// The formatter writes only the screen showing, so under a full-screen
    /// program the primary screen and its history go first, then the
    /// alternate screen, whose emit opens by switching to it. Each screen's
    /// saved cursor goes ahead of its emit. The cursor shape and the title,
    /// which no extra carries, come last, read back from the scratch engine.
    pub fn portable_from_binary(blob: &[u8], scrollback_rows: usize) -> Option<Vec<u8>> {
        let mut e = <Self as Engine>::new(1, 1, scrollback_rows);
        if !e.restore_snapshot(blob) {
            return None;
        }
        const ASK_SHAPE: &[u8] = b"\x1bP$q q\x1b\\";
        e.process(ASK_SHAPE);
        let shape = cursor_shape(&e.take_replies());
        let mut fresh = <Self as Engine>::new(1, 1, 0);
        fresh.process(ASK_SHAPE);
        let shape = shape.filter(|n| Some(*n) != cursor_shape(&fresh.take_replies()));
        let title = e.inner.title();
        let mut out = if e.inner.alternate_screen() {
            let alt = e.encode_portable()?;
            let alt_saved = e.saved_cursor();
            e.process(b"\x1b[?1049l");
            let primary = e.encode_portable()?;
            let mut out = with_saved(primary, &e.saved_cursor());
            out.extend(with_saved(alt, &alt_saved));
            out
        } else {
            let emit = e.encode_portable()?;
            with_saved(emit, &e.saved_cursor())
        };
        if let Some(n) = shape {
            out.extend(format!("\x1b[{n} q").into_bytes());
        }
        if !title.is_empty() && !title.contains(|c: char| c.is_control()) {
            out.extend(format!("\x1b]2;{title}\x07").into_bytes());
        }
        Some(out)
    }

    /// Replace the engine's terminal with the one the binary snapshot `blob`
    /// holds, at the size it was taken at, keeping spyc's own settings
    /// (`configure`). `false`, and the engine unchanged, when `blob` doesn't
    /// decode in this build.
    pub fn restore_snapshot(&mut self, blob: &[u8]) -> bool {
        let Some(t) = decode(blob) else {
            return false;
        };
        configure(t, self.inner.budget, self.inner.replies);
        let old = std::mem::replace(&mut self.inner.t, t);
        // SAFETY: `old` was the engine's live terminal, and nothing refers to
        // it now: the render state is handed the terminal on each update.
        unsafe { ffi::ghostty_terminal_free(old) };
        let s = &mut self.inner;
        // The size is read from the terminal; these back that read.
        (s.rows, s.cols) = s.geometry();
        s.view = 0;
        s.synchronized = s.mode(MODE_SYNCHRONIZED_OUTPUT);
        s.frame_live.set(false);
        s.invalidate();
        true
    }

    /// Replay the portable form `blob`, taken at `rows` by `cols`, into this
    /// engine, which is a new one. The answers to the queries it repeats are
    /// dropped: the program asked them long ago.
    pub fn restore_portable(&mut self, blob: &[u8], rows: u16, cols: u16) {
        self.inner.set_size(rows, cols);
        self.process(blob);
        let _ = self.take_replies();
    }

    /// The formatter's VT emit of the screen showing, scrollback included, as
    /// logical lines. The palette and working directory are left out: they
    /// describe the client, not the terminal.
    pub(super) fn encode_portable(&self) -> Option<Vec<u8>> {
        use ffi::GhosttyFormatterOption as FOpt;
        let extras = ffi::GHOSTTY_FORMATTER_EXTRAS_ALL
            & !ffi::GHOSTTY_FORMATTER_EXTRA_PALETTE
            & !ffi::GHOSTTY_FORMATTER_EXTRA_PWD;
        let mut f: ffi::GhosttyFormatter = std::ptr::null_mut();
        // SAFETY: `t` is live; a null allocator selects the default one, and
        // `f` is a writable slot.
        if unsafe {
            ffi::ghostty_formatter_terminal_new(std::ptr::null(), &raw mut f, self.inner.t)
        } != spyc_vt_sys::SUCCESS
        {
            return None;
        }
        let emit: ffi::GhosttyFormatterFormat::Type =
            ffi::GhosttyFormatterFormat::GHOSTTY_FORMATTER_FORMAT_VT;
        let off = false;
        // Logical lines, so the restoring terminal wraps them again and its
        // rows keep their soft-wrap flags.
        let unwrap = true;
        let set: [(FOpt::Type, *const std::ffi::c_void); 4] = [
            (FOpt::GHOSTTY_FORMATTER_OPT_EMIT, (&raw const emit).cast()),
            (
                FOpt::GHOSTTY_FORMATTER_OPT_UNWRAP,
                (&raw const unwrap).cast(),
            ),
            (FOpt::GHOSTTY_FORMATTER_OPT_TRIM, (&raw const off).cast()),
            (
                FOpt::GHOSTTY_FORMATTER_OPT_EXTRAS,
                (&raw const extras).cast(),
            ),
        ];
        let mut ok = true;
        for (opt, v) in set {
            // SAFETY: `f` is live, and each option reads its value during the call.
            ok &= unsafe { ffi::ghostty_formatter_set(f, opt, v) } == spyc_vt_sys::SUCCESS;
        }
        let mut need: usize = 0;
        let mut buf = Vec::new();
        let mut wrote: usize = 0;
        if ok {
            // SAFETY: a null buffer of length 0 asks only for the size.
            let _ = unsafe {
                ffi::ghostty_formatter_format_buf(f, std::ptr::null_mut(), 0, &raw mut need)
            };
            buf = vec![0u8; need.max(1)];
            // SAFETY: `buf` is writable for its length.
            ok = unsafe {
                ffi::ghostty_formatter_format_buf(f, buf.as_mut_ptr(), buf.len(), &raw mut wrote)
            } == spyc_vt_sys::SUCCESS;
        }
        // SAFETY: `f` is live and freed once.
        unsafe { ffi::ghostty_formatter_free(f) };
        ok.then(|| {
            buf.truncate(wrote);
            buf
        })
    }

    /// What DECSC saved on the screen showing, as sequences that save it
    /// again; empty when it is the default a DECRC would find. It asks the
    /// engine with DECRC, which moves its cursor, so it comes after the
    /// screen's emit is taken.
    fn saved_cursor(&mut self) -> Vec<u8> {
        self.process(b"\x1b8\x1bP$qm\x1b\\");
        let pen = sgr(&self.take_replies());
        let (y, x) = self.inner.cursor_position();
        if (y, x) == (0, 0) && pen.as_deref().is_none_or(|p| p == "0") {
            return Vec::new();
        }
        format!(
            "\x1b[{}m\x1b[{};{}H\x1b7\x1b[0m",
            pen.as_deref().unwrap_or("0"),
            y + 1,
            x + 1
        )
        .into_bytes()
    }
}

impl GhosttyScreen {
    /// The title the program set, if any.
    pub(super) fn title(&self) -> String {
        let mut s = ffi::GhosttyString {
            ptr: std::ptr::null(),
            len: 0,
        };
        // SAFETY: `t` is live and TITLE writes a `GhosttyString`, whose bytes
        // stay valid until the terminal next changes; they are copied at once.
        let ok = unsafe {
            ffi::ghostty_terminal_get(
                self.t,
                Data::GHOSTTY_TERMINAL_DATA_TITLE,
                (&raw mut s).cast(),
            )
        };
        if ok != spyc_vt_sys::SUCCESS || s.ptr.is_null() {
            return String::new();
        }
        // SAFETY: as above, `len` readable bytes at `ptr`.
        String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(s.ptr, s.len) }).into_owned()
    }
}

/// Whether the binary snapshot `blob` decodes in this build.
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

/// The cursor shape in a DECRQSS reply (`DCS 1 $ r N SP q ST`).
fn cursor_shape(reply: &[u8]) -> Option<u8> {
    let s = std::str::from_utf8(reply).ok()?;
    s.split_once("$r")?.1.split_once(" q")?.0.parse().ok()
}

/// `saved` ahead of a screen's `emit`, so the emit's own cursor and pen come
/// after it; for the alternate screen, just after the switch it opens with,
/// since switching saves the cursor too.
fn with_saved(emit: Vec<u8>, saved: &[u8]) -> Vec<u8> {
    const ALT: &[u8] = b"\x1b[?1049h";
    let at = if emit.starts_with(ALT) { ALT.len() } else { 0 };
    let mut out = emit[..at].to_vec();
    out.extend_from_slice(saved);
    out.extend_from_slice(&emit[at..]);
    out
}

/// The parameters in a DECRQSS reply for SGR (`DCS 1 $ r Ps m ST`).
fn sgr(reply: &[u8]) -> Option<String> {
    let s = std::str::from_utf8(reply).ok()?;
    let p = s.split_once("$r")?.1.split_once('m')?.0;
    Some(if p.is_empty() {
        "0".into()
    } else {
        p.to_owned()
    })
}

/// What each snapshot form costs at spyc's 10,000-row scrollback, by kind of
/// output and pane width, against the largest frame a jar takes. Measurements,
/// run by hand in release:
/// `cargo test --release --lib jar_snapshot::costs -- --ignored --nocapture`.
#[cfg(test)]
mod costs {
    use std::fmt::Write as _;
    use std::time::{Duration, Instant};

    use super::GhosttyEngine;
    use crate::pane::engine::Engine;

    const ROWS: u16 = 40;
    const HISTORY: usize = 10_000;

    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> u32 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            (self.0 >> 33) as u32
        }
        fn pick<'a>(&mut self, from: &[&'a str]) -> &'a str {
            from[self.next() as usize % from.len()]
        }
    }

    const WORDS: &[&str] = &[
        "src/pane/mod.rs",
        "the",
        "jar",
        "snapshot",
        "cargo",
        "build",
        "warning:",
        "unused",
        "variable",
        "fn",
        "let",
        "Ok(())",
        "->",
        "42",
        "commit",
        "main",
    ];

    /// `lines` lines of one kind of output, `cols` wide.
    fn workload(kind: &str, lines: usize, cols: u16) -> Vec<u8> {
        let mut r = Lcg(7);
        let mut s = String::new();
        let width = usize::from(cols);
        for i in 0..lines {
            let start = s.len();
            match kind {
                // `cat` of a log: no attributes.
                "plain" => {
                    let _ = write!(s, "{i:6} ");
                    while s.len() - start < width.saturating_sub(16) {
                        s.push_str(r.pick(WORDS));
                        s.push(' ');
                    }
                }
                // `ls --color`, `git log`, a compiler: a few 16-colour runs.
                "shell" => {
                    let _ = write!(s, "\x1b[33m{:08x}\x1b[0m ", r.next());
                    for _ in 0..(r.next() % 8 + 4) * u32::from(cols) / 120 {
                        let fg = 31 + r.next() % 7;
                        let _ = write!(s, "\x1b[{fg}m{}\x1b[0m {} ", r.pick(WORDS), r.pick(WORDS));
                    }
                }
                // An inline agent: truecolour, bold and dim, box drawing,
                // emoji and wide CJK.
                "agent" => {
                    s.push_str("\x1b[38;2;120;120;140m│\x1b[0m ");
                    for _ in 0..(r.next() % 10 + 6) * u32::from(cols) / 120 {
                        let (red, green, blue) = (r.next() % 256, r.next() % 256, r.next() % 256);
                        let attr = ["1", "2", "3", "0"][r.next() as usize % 4];
                        let _ = write!(
                            s,
                            "\x1b[{attr};38;2;{red};{green};{blue}m{}\x1b[0m ",
                            r.pick(WORDS)
                        );
                        if r.next().is_multiple_of(9) {
                            s.push_str(r.pick(&["✅ ", "⏺ ", "漢字 ", "🌶 ", "→ "]));
                        }
                    }
                }
                // Every cell its own colours.
                _ => {
                    for col in 0..cols {
                        let _ = write!(
                            s,
                            "\x1b[38;2;{};{};{};48;2;{};{};{}m{}",
                            r.next() % 256,
                            r.next() % 256,
                            r.next() % 256,
                            r.next() % 256,
                            r.next() % 256,
                            r.next() % 256,
                            char::from(b'a' + (col % 26) as u8)
                        );
                    }
                    s.push_str("\x1b[0m");
                }
            }
            s.push_str("\r\n");
        }
        s.into_bytes()
    }

    fn timed<T>(f: impl FnOnce() -> T) -> (T, Duration) {
        let t0 = Instant::now();
        let out = f();
        (out, t0.elapsed())
    }

    fn mib(n: usize) -> f64 {
        n as f64 / 1_048_576.0
    }

    /// Each form's size, the binary encode's time under the lock, and the
    /// portable form's time off it, at a full scrollback, from a 120-column
    /// pane to a 600-column one.
    #[test]
    #[ignore = "a measurement, run in release by hand"]
    fn snapshot_costs_by_width() {
        let limit = spyc_jar::proto::MAX_FRAME;
        println!("frame limit {:.1} MiB", mib(limit));
        println!(
            "{:<6} {:>5} {:>8} | {:>8} {:>8} | {:>8} {:>8}",
            "kind", "cols", "history", "bin MiB", "locked", "port MiB", "off lock"
        );
        for cols in [120, 240, 400, 600] {
            for kind in ["plain", "shell", "agent", "worst"] {
                let mut e = <GhosttyEngine as Engine>::new(ROWS, cols, HISTORY);
                e.process(&workload(kind, 12_000, cols));
                let history = e.inner.history_rows_raw();
                let (bin, locked) = timed(|| e.encode_snapshot().expect("binary"));
                let (port, off) =
                    timed(|| GhosttyEngine::portable_from_binary(&bin, HISTORY).expect("portable"));
                let over = |n: usize| if n > limit { " OVER" } else { "" };
                println!(
                    "{kind:<6} {cols:>5} {history:>8} | {:>8.2} {:>6.1}ms | {:>8.2} {:>6.1}ms{}{}",
                    mib(bin.len()),
                    locked.as_secs_f64() * 1000.0,
                    mib(port.len()),
                    off.as_secs_f64() * 1000.0,
                    over(bin.len()),
                    over(port.len()),
                );
            }
        }
    }

    /// Whatever its width, a full pane's snapshot fits in one frame, in either
    /// form, with the byte its `snapshot` reply adds: the scrollback's byte
    /// ceiling bounds the terminal, and so what encodes it. The output is the
    /// costliest kind, every cell its own colours, in a pane wider than any
    /// screen gives, fed until its history stops growing.
    #[test]
    fn a_full_pane_snapshot_fits_in_one_frame_at_any_width() {
        const COLS: u16 = 1_000;
        let mut e = <GhosttyEngine as Engine>::new(ROWS, COLS, HISTORY);
        let batch = workload("worst", 64, COLS);
        let mut most = 0;
        for _ in 0..64 {
            e.process(&batch);
            let now = e.inner.history_rows_raw();
            if now <= most {
                break;
            }
            most = now;
        }
        assert!(most > 0, "premise: the pane has a history");
        let bin = e.encode_snapshot().expect("binary");
        let port = GhosttyEngine::portable_from_binary(&bin, HISTORY).expect("portable");
        for (form, len) in [("binary", bin.len()), ("portable", port.len())] {
            assert!(
                len < spyc_jar::proto::MAX_FRAME - 64,
                "the {form} form, {:.1} MiB, fits in a frame",
                mib(len)
            );
        }
    }

    /// Thirty full agent panes go quiet at the same moment (a broadcast), each
    /// snapshotting on its own thread: binary under the lock, then the
    /// portable form from it, off the lock (R17d).
    #[test]
    #[ignore = "a measurement, run in release by hand"]
    fn thirty_panes_at_once() {
        let bytes = std::sync::Arc::new(workload("agent", 12_000, 120));
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(30));
        // Collected: every pane starts before any is joined.
        #[allow(clippy::needless_collect)]
        let handles: Vec<_> = (0..30)
            .map(|_| {
                let (bytes, barrier) = (bytes.clone(), barrier.clone());
                std::thread::spawn(move || {
                    let mut e = <GhosttyEngine as Engine>::new(ROWS, 120, HISTORY);
                    e.process(&bytes);
                    barrier.wait();
                    let (bin, locked) = timed(|| e.encode_snapshot().expect("binary"));
                    let (_, off) = timed(|| {
                        GhosttyEngine::portable_from_binary(&bin, HISTORY).expect("portable")
                    });
                    (locked, locked + off)
                })
            })
            .collect();
        let r: Vec<_> = handles
            .into_iter()
            .map(|h| h.join().expect("pane thread"))
            .collect();
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        println!(
            "30 full agent panes at once: lock held at most {:.1}ms; every snapshot done within {:.1}ms",
            r.iter().map(|x| ms(x.0)).fold(0.0, f64::max),
            r.iter().map(|x| ms(x.1)).fold(0.0, f64::max),
        );
    }
}
