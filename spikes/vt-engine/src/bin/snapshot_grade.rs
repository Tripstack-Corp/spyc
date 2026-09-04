//! Gate criteria for the SNAPSHOT mechanism (Stage 5).
//!
//! The spike graded rehydration through the VT formatter — emit escape
//! sequences, replay them. The shipping pin also carries a purpose-built
//! snapshot API, and if 3.0 attach and 2.3 recovery will use that, re-running
//! only the formatter measurement grades the wrong mechanism.
//!
//! Four criteria, per `V2_2_PLAN.md` §8:
//!   1. fidelity — encode, decode, row-by-row diff, over the SAME corpus the
//!      formatter grading uses so the two are comparable
//!   2. encoded size on that corpus
//!   3. the continuation round trip — cut a stream mid-escape-sequence
//!   4. format stability, read from the pin's headers
//!
//! Run: cargo run --release --features ghostty --bin snapshot_grade

#[cfg(not(feature = "ghostty"))]
fn main() {
    eprintln!("build with --features ghostty");
}

#[cfg(feature = "ghostty")]
fn main() {
    use vt_engine_spike::engine::{Engine, Screen};
    use vt_engine_spike::engines::GhosttyEngine;
    use vt_engine_spike::fixtures;

    const BUDGET: usize = 10_000;

    /// Rows matching when the replay is allowed to sit `shift` rows off, so a
    /// viewport offset is distinguishable from lost content — the distinction
    /// that turned the formatter's "57.3%" into "98.0% at shift +1".
    fn rows_at(a: &Screen, b: &Screen, shift: i32) -> usize {
        (0..a.rows)
            .filter(|r| {
                let src = i32::from(*r) + shift;
                src >= 0
                    && src < i32::from(b.rows)
                    && a.row_text(*r) == b.row_text(src as u16)
            })
            .count()
    }

    struct Grade {
        case: String,
        bytes: usize,
        rows_exact: usize,
        rows_shifted: usize,
        shift: i32,
        rows_total: usize,
        cells_text: usize,
        cells_attr: usize,
        cells_total: usize,
        cursor_ok: bool,
        alt_ok: bool,
        sb_in: usize,
        sb_out: usize,
    }

    let mut cases: Vec<(String, Vec<u8>, (u16, u16))> = fixtures::SYNTHETIC
        .iter()
        .map(|s| (s.name.to_string(), s.bytes.to_vec(), s.size))
        .collect();
    for (n, b) in fixtures::captured() {
        cases.push((format!("captured/{n}"), b, (24, 80)));
    }

    println!("SNAPSHOT MECHANISM — gate criteria 1 and 2");
    println!("pin {}", spyc_vt_sys::pin::GHOSTTY_COMMIT);
    println!("{} cases, the same corpus the formatter grading uses", cases.len());
    println!("{:=<98}", "");

    let mut g: Vec<Grade> = Vec::new();
    for (case, bytes, (rows, cols)) in &cases {
        let mut a = GhosttyEngine::create_with_budget(*rows, *cols, BUDGET);
        a.feed(bytes);
        let orig = a.screen();
        let sb_in = a.scrollback_rows();
        let Some(snap) = a.snapshot() else {
            println!("  {case:<30} ENCODE FAILED");
            continue;
        };
        let Some(mut b) = GhosttyEngine::from_snapshot(&snap, *rows, *cols) else {
            println!("  {case:<30} DECODE FAILED ({} bytes)", snap.len());
            continue;
        };
        let re = b.screen();
        let sb_out = b.scrollback_rows();

        let (mut ct, mut ca, mut tot) = (0usize, 0usize, 0usize);
        for r in 0..orig.rows {
            for c in 0..orig.cols {
                let (Some(x), Some(y)) = (orig.cell(r, c), re.cell(r, c)) else { continue };
                tot += 1;
                let (tx, ty) = (
                    if x.text == " " { "" } else { x.text.as_str() },
                    if y.text == " " { "" } else { y.text.as_str() },
                );
                if tx == ty {
                    ct += 1;
                }
                if (x.fg, x.bg, x.bold, x.italic, x.underline, x.reverse)
                    == (y.fg, y.bg, y.bold, y.italic, y.underline, y.reverse)
                {
                    ca += 1;
                }
            }
        }
        let (best, shift) = (-2..=2)
            .map(|s| (rows_at(&orig, &re, s), s))
            .max_by_key(|(n, s)| (*n, -s.abs()))
            .unwrap_or((0, 0));
        g.push(Grade {
            case: case.clone(),
            bytes: snap.len(),
            rows_exact: rows_at(&orig, &re, 0),
            rows_shifted: best,
            shift,
            rows_total: usize::from(orig.rows),
            cells_text: ct,
            cells_attr: ca,
            cells_total: tot,
            cursor_ok: orig.cursor == re.cursor,
            alt_ok: orig.alt_screen == re.alt_screen,
            sb_in,
            sb_out,
        });
    }

    for x in &g {
        if x.rows_exact < x.rows_total || !x.cursor_ok || !x.alt_ok || x.sb_in != x.sb_out {
            println!(
                "  {:<30} rows {}/{} (={} at {:+})  cursor {}  alt {}  sb {}->{}  {} B",
                x.case, x.rows_exact, x.rows_total, x.rows_shifted, x.shift,
                if x.cursor_ok { "ok" } else { "LOST" },
                if x.alt_ok { "ok" } else { "LOST" },
                x.sb_in, x.sb_out, x.bytes
            );
        }
    }
    let sum = |f: fn(&Grade) -> usize| g.iter().map(f).sum::<usize>();
    let pct = |n: usize, d: usize| if d == 0 { 100.0 } else { 100.0 * n as f64 / d as f64 };
    println!("{:-<98}", "");
    println!(
        "  rows-exact {:.1}%  rows-shift-tolerant {:.1}%  cell-text {:.1}%  cell-attrs {:.1}%",
        pct(sum(|x| x.rows_exact), sum(|x| x.rows_total)),
        pct(sum(|x| x.rows_shifted), sum(|x| x.rows_total)),
        pct(sum(|x| x.cells_text), sum(|x| x.cells_total)),
        pct(sum(|x| x.cells_attr), sum(|x| x.cells_total)),
    );
    println!(
        "  cursor {}/{}  alt-screen {}/{}  scrollback {} rows in -> {} out ({:.1}%)",
        g.iter().filter(|x| x.cursor_ok).count(), g.len(),
        g.iter().filter(|x| x.alt_ok).count(), g.len(),
        sum(|x| x.sb_in), sum(|x| x.sb_out),
        pct(sum(|x| x.sb_out), sum(|x| x.sb_in)),
    );
    println!("  encoded {} bytes total across {} cases", sum(|x| x.bytes), g.len());

    // ---- criterion 3: the continuation round trip --------------------------
    println!();
    println!("SNAPSHOT MECHANISM — gate criterion 3: the continuation round trip");
    println!("{:=<98}", "");
    continuation_round_trip();

    // ---- criterion 4: format stability ------------------------------------
    println!();
    println!("SNAPSHOT MECHANISM — gate criterion 4: format stability");
    println!("{:=<98}", "");
    let snap = {
        let mut t = GhosttyEngine::create_with_budget(4, 20, BUDGET);
        t.feed(b"hello\r\n");
        t.snapshot().unwrap_or_default()
    };
    if snap.len() >= 10 {
        let magic = String::from_utf8_lossy(&snap[..8]).to_string();
        let version = u16::from_le_bytes([snap[8], snap[9]]);
        println!("  envelope magic   {magic:?}");
        println!("  format version   {version}");
        println!("  -> the version is IN the stream, so a decoder meeting an unknown one");
        println!("     rejects it rather than misparsing it. Per snapshot.h, version 1");
        println!("     'does not yet carry a binary-compatibility guarantee', so:");
        println!("     snapshots are TRANSPORT-ONLY — same binary, same pin.");
        println!("     Never at-rest persistence across an upgrade.");
    }
    // A truncated stream must be refused, not partially applied — the property
    // that makes "detectable" more than a claim about a header field.
    let truncated = &snap[..snap.len() / 2];
    println!(
        "  truncated stream ({} of {} bytes) decodes: {}",
        truncated.len(),
        snap.len(),
        GhosttyEngine::from_snapshot(truncated, 4, 20).is_some()
    );
    let mut corrupt = snap.clone();
    if corrupt.len() > 40 {
        corrupt[35] ^= 0xff; // inside a record payload, under its CRC32C
    }
    println!(
        "  CRC-corrupted stream decodes: {}",
        GhosttyEngine::from_snapshot(&corrupt, 4, 20).is_some()
    );
    let mut wrong_version = snap.clone();
    if wrong_version.len() > 10 {
        wrong_version[8] = 0xff;
        wrong_version[9] = 0xff;
    }
    println!(
        "  version-65535 stream decodes: {}",
        GhosttyEngine::from_snapshot(&wrong_version, 4, 20).is_some()
    );
    println!("  (all three must be `false` for 'detectable and discardable' to hold)");

    /// Cut a stream mid-escape-sequence, snapshot with the continuation
    /// retained, decode, re-feed the exported continuation plus the rest, and
    /// compare against the uncut run.
    ///
    /// Crash recovery does not get to wait for a clean parser boundary, so this
    /// is the property that decides whether the snapshot API can back it.
    fn continuation_round_trip() {
        use spyc_vt_sys::ffi;
        // A stream whose cut lands INSIDE a CSI sequence.
        let full: &[u8] = b"before\r\n\x1b[38;2;10;20;30mcoloured text\x1b[0m\r\nafter\r\n";
        let cut = 8 + 6; // partway through "\x1b[38;2;..."
        assert!(full[cut - 1] != b'm', "the cut must land mid-sequence");

        // Reference: the uncut run.
        let mut want = GhosttyEngine::create_with_budget(6, 40, 10_000);
        want.feed(full);
        let want_screen = want.screen();

        // Feed only the prefix, then snapshot WITH continuation retained.
        //
        // Tracking has to be enabled BEFORE the input that leaves the parser
        // unfinished. `snapshot.h`: "A terminal can be encoded with tracking
        // disabled when its VT parser and UTF-8 decoder are both at ground. If
        // either is unfinished, tracking must have been enabled before the
        // input that produced that state was written; otherwise this returns
        // GHOSTTY_INVALID_VALUE."
        //
        // That is a 3.0 design constraint, not a probe detail: a daemon that
        // intends to snapshot must pay for tracking continuously, because it
        // cannot opt in at crash time — by then the partial sequence is gone.
        let mut a = GhosttyEngine::create_with_budget(6, 40, 10_000);
        a.enable_continuation_tracking(64 * 1024);
        a.feed(&full[..cut]);
        let Some(snap) = a.snapshot() else {
            println!("  encode failed");
            return;
        };

        // Decode with RETAIN_CONTINUATION so the unfinished bytes come back.
        let mut dec: ffi::GhosttySnapshotDecoder = std::ptr::null_mut();
        if unsafe {
            ffi::ghostty_snapshot_decoder_new_buf(
                std::ptr::null(),
                &raw mut dec,
                snap.as_ptr(),
                snap.len(),
            )
        } != spyc_vt_sys::SUCCESS
        {
            println!("  decoder_new failed");
            return;
        }
        let max: usize = 64 * 1024;
        let retain = true;
        unsafe {
            ffi::ghostty_snapshot_decoder_set(
                dec,
                ffi::GhosttySnapshotDecoderOption::GHOSTTY_SNAPSHOT_DECODER_OPT_MAX_CONTINUATION_BYTES,
                (&raw const max).cast(),
            );
            ffi::ghostty_snapshot_decoder_set(
                dec,
                ffi::GhosttySnapshotDecoderOption::GHOSTTY_SNAPSHOT_DECODER_OPT_RETAIN_CONTINUATION,
                (&raw const retain).cast(),
            );
        }
        let mut t: ffi::GhosttyTerminal = std::ptr::null_mut();
        let r = unsafe { ffi::ghostty_snapshot_decoder_decode(dec, &raw mut t) };
        unsafe { ffi::ghostty_snapshot_decoder_free(dec) };
        if r != spyc_vt_sys::SUCCESS || t.is_null() {
            println!("  decode with RETAIN_CONTINUATION failed ({r})");
            return;
        }

        // Export the unfinished bytes the snapshot restored.
        let mut need: usize = 0;
        let _ = unsafe {
            ffi::ghostty_terminal_continuation_buf(t, std::ptr::null_mut(), 0, &raw mut need)
        };
        let mut cont = vec![0u8; need.max(1)];
        let mut wrote: usize = 0;
        let cr = unsafe {
            ffi::ghostty_terminal_continuation_buf(t, cont.as_mut_ptr(), cont.len(), &raw mut wrote)
        };
        cont.truncate(if cr == spyc_vt_sys::SUCCESS { wrote } else { 0 });
        println!(
            "  cut {cut} bytes into a CSI sequence; exported continuation = {} bytes {:?}",
            cont.len(),
            String::from_utf8_lossy(&cont)
        );

        // The API's own caveat: exporting an empty continuation does not
        // disable tracking, so the max must go back to zero before any
        // post-snapshot input.
        let zero: usize = 0;
        let z = unsafe {
            ffi::ghostty_terminal_set(
                t,
                ffi::GhosttyTerminalOption::GHOSTTY_TERMINAL_OPT_CONTINUATION_MAX_BYTES,
                (&raw const zero).cast(),
            )
        };
        println!("  continuation tracking reset to zero before post-snapshot input: {}", z == spyc_vt_sys::SUCCESS);

        // Re-feed the continuation, then the remaining bytes.
        unsafe {
            ffi::ghostty_terminal_vt_write(t, cont.as_ptr(), cont.len());
            ffi::ghostty_terminal_vt_write(t, full[cut..].as_ptr(), full.len() - cut);
        }

        // Compare against the uncut run, row by row.
        let mut got = GhosttyEngine::from_raw_for_compare(t, 6, 40);
        let got_screen = got.screen();
        let rows_match = (0..want_screen.rows)
            .filter(|r| want_screen.row_text(*r) == got_screen.row_text(*r))
            .count();
        let attrs_match = (0..want_screen.rows).all(|r| {
            (0..want_screen.cols).all(|c| match (want_screen.cell(r, c), got_screen.cell(r, c)) {
                (Some(a), Some(b)) => (a.fg, a.bg, a.bold, a.underline) == (b.fg, b.bg, b.bold, b.underline),
                _ => true,
            })
        });
        println!(
            "  rows identical to the uncut run: {rows_match}/{}   attributes identical: {attrs_match}",
            want_screen.rows
        );
        println!(
            "  cursor uncut {:?} vs resumed {:?}",
            want_screen.cursor, got_screen.cursor
        );
        println!(
            "  => the parser boundary {}",
            if rows_match == usize::from(want_screen.rows) && attrs_match {
                "SURVIVES a snapshot taken mid-sequence"
            } else {
                "is NOT preserved — see the rows above"
            }
        );
    }
}
