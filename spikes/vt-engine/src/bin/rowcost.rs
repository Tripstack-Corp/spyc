//! What does a retained row cost ghostty, in bytes? The measurement behind
//! `spyc_vt_sys::scrollback::BYTES_PER_ROW_HEAVY`.
//!
//! The C API has no "bytes retained" readout, so the cost is read indirectly:
//! set a byte ceiling, leave the line limit out of reach, feed far more than
//! fits, and divide the ceiling by the rows that survive. Only a ceiling that
//! actually bound says anything, so each row of the sweep reports whether the
//! feed outran it.
//!
//! The corpora are `sbprobe`'s, so the rate this derives and the assertions
//! that use it read the same bytes.
#[cfg(feature = "ghostty")]
fn main() {
    use spyc_vt_sys::ffi::{self, GhosttyTerminalData as Data, GhosttyTerminalOption as Opt};

    fn plain(n: usize) -> Vec<u8> {
        let mut v = Vec::new();
        for i in 0..n {
            v.extend_from_slice(
                format!("line {i:06} with about forty characters of text\r\n").as_bytes(),
            );
        }
        v
    }

    fn heavy(n: usize) -> Vec<u8> {
        let mut v = Vec::new();
        for i in 0..n {
            v.extend_from_slice(
                format!(
                    "\x1b[38;5;244m│\x1b[0m \x1b[1;36m{i:05}\x1b[0m \
\x1b[38;2;180;200;255mtool\x1b[0m\x1b[2m·\x1b[0m\x1b[33mRead\x1b[0m \
\u{2500}\u{2500} \u{3042}\u{1F600} e\u{0301} \x1b[4mpath/to/file.rs\x1b[0m\r\n"
                )
                .as_bytes(),
            );
        }
        v
    }

    /// Rows retained under a `max_bytes` ceiling with the line limit out of
    /// reach.
    fn retained(feed: &[u8], max_bytes: usize) -> usize {
        let mut t: ffi::GhosttyTerminal = std::ptr::null_mut();
        assert_eq!(
            unsafe { ffi::ghostty_terminal_new(std::ptr::null(), &raw mut t, 80, 24) },
            spyc_vt_sys::SUCCESS
        );
        let lines: usize = usize::MAX / 2;
        unsafe {
            ffi::ghostty_terminal_set(
                t,
                Opt::GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_LINES,
                (&raw const lines).cast(),
            );
            ffi::ghostty_terminal_set(
                t,
                Opt::GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_BYTES,
                (&raw const max_bytes).cast(),
            );
        }
        for c in feed.chunks(8192) {
            unsafe { ffi::ghostty_terminal_vt_write(t, c.as_ptr(), c.len()) };
        }
        let mut rows: usize = 0;
        let ok = unsafe {
            ffi::ghostty_terminal_get(
                t,
                Data::GHOSTTY_TERMINAL_DATA_SCROLLBACK_ROWS,
                (&raw mut rows).cast(),
            )
        };
        unsafe { ffi::ghostty_terminal_free(t) };
        assert_eq!(ok, spyc_vt_sys::SUCCESS);
        rows
    }

    const FED: usize = 60_000;
    println!(
        "retained bytes per row at libghostty-vt {} ({})",
        spyc_vt_sys::pin::GHOSTTY_VT_VERSION,
        spyc_vt_sys::pin::GHOSTTY_COMMIT
    );
    println!(
        "80x24, {FED} rows fed per run; shipped constant BYTES_PER_ROW_HEAVY = {}",
        spyc_vt_sys::scrollback::BYTES_PER_ROW_HEAVY
    );
    println!("  {:<8} {:>9} {:>10} {:>9}  bound?", "corpus", "ceiling", "retained", "B/row");
    for (label, feed) in [("plain", plain(FED)), ("heavy", heavy(FED))] {
        for mib in [1usize, 2, 4, 8, 16] {
            let ceiling = mib * 1024 * 1024;
            let rows = retained(&feed, ceiling);
            let bound = rows + 24 < FED;
            println!(
                "  {label:<8} {:>6} MiB {rows:>10} {:>9}  {}",
                mib,
                if rows == 0 { 0 } else { ceiling / rows },
                if bound { "yes" } else { "NO — fed too little to read a rate" }
            );
        }
    }
}

#[cfg(not(feature = "ghostty"))]
fn main() {
    eprintln!("build with --features ghostty");
}
