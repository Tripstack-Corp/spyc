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

    /// The engine's state as VT sequences any version can replay (R17a's
    /// portable form): the formatter's emit, scrollback included. The palette
    /// and working directory are left out, as the engine spike's rehydration
    /// did: they describe the client, not the terminal.
    pub fn encode_portable(&self) -> Option<Vec<u8>> {
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
        let set: [(FOpt::Type, *const std::ffi::c_void); 4] = [
            (FOpt::GHOSTTY_FORMATTER_OPT_EMIT, (&raw const emit).cast()),
            (FOpt::GHOSTTY_FORMATTER_OPT_UNWRAP, (&raw const off).cast()),
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

/// Decision I's measurement: what each snapshot form costs at spyc's default
/// 10,000-row scrollback. `cargo test --release --lib snapshot_costs --
/// --ignored --nocapture`.
#[cfg(test)]
mod bench {
    use super::super::GhosttyEngine;
    use crate::pane::engine::Engine;
    use std::fmt::Write as _;
    use std::time::Instant;

    const ROWS: u16 = 40;
    const COLS: u16 = 120;

    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> u32 {
            self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            (self.0 >> 33) as u32
        }
        fn pick<'a>(&mut self, from: &[&'a str]) -> &'a str {
            from[self.next() as usize % from.len()]
        }
    }

    const WORDS: &[&str] = &[
        "src/pane/mod.rs", "the", "jar", "snapshot", "cargo", "build", "warning:",
        "unused", "variable", "fn", "let", "Ok(())", "->", "42", "commit", "main",
    ];

    /// One kind of output, `lines` lines of it.
    fn workload(kind: &str, lines: usize) -> Vec<u8> {
        let mut r = Lcg(7);
        let mut s = String::new();
        for i in 0..lines {
            match kind {
                // `cat` of a log: no attributes.
                "plain" => {
                    let _ = write!(s, "{i:6} ");
                    while s.len() % 4096 < 4000 && s.rsplit('\n').next().map_or(0, str::len) < 90 {
                        s.push_str(r.pick(WORDS));
                        s.push(' ');
                    }
                }
                // `ls --color`, `git log`, a compiler: a few 16-colour runs.
                "shell" => {
                    let _ = write!(s, "\x1b[33m{:08x}\x1b[0m ", r.next());
                    for _ in 0..(r.next() % 8 + 4) {
                        let c = 31 + r.next() % 7;
                        let _ = write!(s, "\x1b[{c}m{}\x1b[0m {} ", r.pick(WORDS), r.pick(WORDS));
                    }
                }
                // An inline agent: truecolour, bold and dim, box drawing,
                // emoji and wide CJK.
                "agent" => {
                    s.push_str("\x1b[38;2;120;120;140m│\x1b[0m ");
                    for _ in 0..(r.next() % 10 + 6) {
                        let (a, b, c) = (r.next() % 256, r.next() % 256, r.next() % 256);
                        let attr = ["1", "2", "3", "0"][r.next() as usize % 4];
                        let _ = write!(s, "\x1b[{attr};38;2;{a};{b};{c}m{}\x1b[0m ", r.pick(WORDS));
                        if r.next() % 9 == 0 {
                            s.push_str(r.pick(&["✅ ", "⏺ ", "漢字 ", "🌶 ", "→ "]));
                        }
                    }
                }
                // Every cell its own colours.
                _ => {
                    for col in 0..COLS {
                        let _ = write!(
                            s,
                            "\x1b[38;2;{};{};{};48;2;{};{};{}m{}",
                            r.next() % 256, r.next() % 256, r.next() % 256,
                            r.next() % 256, r.next() % 256, r.next() % 256,
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

    fn median_ms(mut f: impl FnMut()) -> f64 {
        let mut t: Vec<f64> = (0..5)
            .map(|_| {
                let t0 = Instant::now();
                f();
                t0.elapsed().as_secs_f64() * 1000.0
            })
            .collect();
        t.sort_by(f64::total_cmp);
        t[2]
    }

    #[test]
    #[ignore = "a measurement, run in release by hand"]
    fn snapshot_costs() {
        println!(
            "{:<6} {:>6} {:>9} | {:>8} {:>9} {:>9} | {:>8} {:>9} {:>10}",
            "kind", "lines", "history", "bin MB", "bin enc", "bin rest",
            "port MB", "port enc", "port rest"
        );
        for lines in [1_000, 12_000] {
            for kind in ["plain", "shell", "agent", "worst"] {
                let bytes = workload(kind, lines);
                let mut e = <GhosttyEngine as Engine>::new(ROWS, COLS, 10_000);
                e.process(&bytes);
                let history = e.inner.history_rows_raw();
                let bin = e.encode_snapshot().expect("binary snapshot");
                let port = e.encode_portable().expect("portable snapshot");
                let bin_enc = median_ms(|| drop(e.encode_snapshot()));
                let port_enc = median_ms(|| drop(e.encode_portable()));
                let bin_rest = median_ms(|| {
                    let mut f = <GhosttyEngine as Engine>::new(ROWS, COLS, 10_000);
                    assert!(f.restore_snapshot(&bin, ROWS, COLS));
                });
                let mut port_history = 0;
                let port_rest = median_ms(|| {
                    let mut f = <GhosttyEngine as Engine>::new(ROWS, COLS, 10_000);
                    f.process(&port);
                    port_history = f.inner.history_rows_raw();
                });
                // Off the parser's lock: the portable form made from the
                // binary snapshot, in a scratch engine.
                let mut via_bin = Vec::new();
                let off_lock = median_ms(|| {
                    let mut f = <GhosttyEngine as Engine>::new(ROWS, COLS, 10_000);
                    assert!(f.restore_snapshot(&bin, ROWS, COLS));
                    via_bin = f.encode_portable().expect("portable from binary");
                });
                let mut again = <GhosttyEngine as Engine>::new(ROWS, COLS, 10_000);
                again.process(&port);
                let round_trip = again.encode_portable().expect("portable again") == port;
                println!(
                    "       off-lock portable {off_lock:.1}ms, same bytes as on-lock: {}; portable round-trips to itself: {round_trip}",
                    via_bin == port
                );
                let mb = |n: usize| n as f64 / 1_048_576.0;
                println!(
                    "{kind:<6} {lines:>6} {history:>9} | {:>8.2} {bin_enc:>7.1}ms {bin_rest:>7.1}ms | {:>8.2} {port_enc:>7.1}ms {port_rest:>8.1}ms  (portable restores {port_history} history rows; log was {:.2} MB)",
                    mb(bin.len()), mb(port.len()), mb(bytes.len())
                );
            }
        }
    }

    /// Thirty full agent panes go quiet at the same moment (a broadcast), each
    /// snapshotting on its own parser thread: binary under the lock, then the
    /// portable form from it, off the lock.
    #[test]
    #[ignore = "a measurement, run in release by hand"]
    fn thirty_panes_at_once() {
        let bytes = std::sync::Arc::new(workload("agent", 12_000));
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(30));
        let handles: Vec<_> = (0..30)
            .map(|_| {
                let (bytes, barrier) = (bytes.clone(), barrier.clone());
                std::thread::spawn(move || {
                    let mut e = <GhosttyEngine as Engine>::new(ROWS, COLS, 10_000);
                    e.process(&bytes);
                    barrier.wait();
                    let t0 = Instant::now();
                    let bin = e.encode_snapshot().expect("binary");
                    let locked = t0.elapsed();
                    let mut f = <GhosttyEngine as Engine>::new(ROWS, COLS, 10_000);
                    assert!(f.restore_snapshot(&bin, ROWS, COLS));
                    let port = f.encode_portable().expect("portable");
                    (locked, t0.elapsed(), bin.len() + port.len())
                })
            })
            .collect();
        let r: Vec<_> = handles.into_iter().map(|h| h.join().expect("pane thread")).collect();
        let ms = |d: std::time::Duration| d.as_secs_f64() * 1000.0;
        let worst_lock = r.iter().map(|x| ms(x.0)).fold(0.0, f64::max);
        let worst_all = r.iter().map(|x| ms(x.1)).fold(0.0, f64::max);
        let held: usize = r.iter().map(|x| x.2).sum();
        println!(
            "30 full agent panes at once: lock held at most {worst_lock:.1}ms; every snapshot done within {worst_all:.1}ms; both forms held: {:.0} MB",
            held as f64 / 1_048_576.0
        );
    }
}
