//! Isolate ghostty's one-row viewport offset on scrollback-bearing state.
//!
//! The symptom: emit a terminal's state with the VT formatter, replay it into a
//! fresh terminal, and the viewport comes back one row higher than the
//! original. Content is present and correctly ordered; the window is shifted.
//!
//! Two candidate causes that look identical from outside and need different
//! fixes:
//!   A. the emit is INCOMPLETE — one row at the history/viewport boundary is
//!      never written;
//!   B. the emit is complete and the REPLAY lands differently, because the
//!      trailing row does not trigger the scroll the original did.
//!
//! This separates them by counting rows in the emitted bytes, which is
//! something only the emit can be wrong about.
#[cfg(not(feature = "ghostty"))]
fn main() {
    eprintln!("build with --features ghostty");
}

#[cfg(feature = "ghostty")]
fn main() {
    use vt_engine_spike::engine::Engine;
    use vt_engine_spike::engines::GhosttyEngine;

    const BUDGET: usize = 10_000;

    /// Feed `lines` numbered lines into a `rows`-row terminal, emit, replay,
    /// and report the alignment.
    fn probe(rows: u16, cols: u16, lines: usize) -> (usize, i32, usize, usize) {
        let mut a = GhosttyEngine::create_with_budget(rows, cols, BUDGET);
        let mut feed = Vec::new();
        for i in 0..lines {
            feed.extend_from_slice(format!("L{i:03}\r\n").as_bytes());
        }
        a.feed(&feed);
        let orig = a.screen();
        let sb_in = a.scrollback_rows();
        let dump = a.rehydrate().unwrap_or_default();

        let mut b = GhosttyEngine::create_with_budget(rows, cols, BUDGET);
        b.feed(&dump);
        let re = b.screen();
        let sb_out = b.scrollback_rows();

        // Best shift that aligns the two viewports.
        let best = (-2i32..=2)
            .map(|s| {
                let n = (0..rows)
                    .filter(|r| {
                        let src = i32::from(*r) + s;
                        src >= 0
                            && src < i32::from(rows)
                            && orig.row_text(*r) == re.row_text(src as u16)
                    })
                    .count();
                (n, s)
            })
            .max_by_key(|(n, s)| (*n, -s.abs()))
            .unwrap_or((0, 0));
        (best.0, best.1, sb_in, sb_out)
    }

    println!("ghostty VT-formatter viewport offset — isolation");
    println!("pin {}", spyc_vt_sys::pin::GHOSTTY_COMMIT);
    println!();

    // ---- 1. find the smallest case that shows it -------------------------
    println!("== 1. smallest reproducing geometry ==");
    println!("  {:>4} {:>4} {:>6}  {:>8} {:>6}  {:>9}", "rows", "cols", "lines", "aligned", "shift", "sb in/out");
    let mut smallest: Option<(u16, u16, usize)> = None;
    for rows in [2u16, 3, 4, 24] {
        for lines in [
            usize::from(rows),         // exactly fills, no scrollback
            usize::from(rows) + 1,     // one row of scrollback
            usize::from(rows) + 2,
            usize::from(rows) + 10,
        ] {
            let (aligned, shift, sb_in, sb_out) = probe(rows, 12, lines);
            let flag = if shift != 0 { "  <= OFFSET" } else { "" };
            println!(
                "  {rows:>4} {:>4} {lines:>6}  {aligned:>8} {shift:>+6}  {sb_in:>4}/{sb_out:<4}{flag}",
                12
            );
            if shift != 0 && smallest.is_none() {
                smallest = Some((rows, 12, lines));
            }
        }
    }

    // ---- 2. is the EMIT incomplete, or is the REPLAY misplacing it? ------
    println!();
    println!("== 2. cause A (emit drops a row) vs cause B (replay scrolls differently) ==");
    let Some((rows, cols, lines)) = smallest else {
        println!("  no offset reproduced — nothing to isolate");
        return;
    };
    println!("  using the smallest reproducer: {rows}x{cols}, {lines} lines fed");

    let mut a = GhosttyEngine::create_with_budget(rows, cols, BUDGET);
    let mut feed = Vec::new();
    for i in 0..lines {
        feed.extend_from_slice(format!("L{i:03}\r\n").as_bytes());
    }
    a.feed(&feed);
    let orig = a.screen();
    let sb_in = a.scrollback_rows();
    let dump = a.rehydrate().unwrap_or_default();

    println!();
    println!("  original viewport:");
    for r in 0..rows {
        println!("    row {r}: {:?}", orig.row_text(r));
    }
    println!("  original scrollback rows: {sb_in}");
    println!();
    println!("  emitted {} bytes:", dump.len());
    println!("    {:?}", String::from_utf8_lossy(&dump).escape_debug().to_string());

    // The decisive test is NOT a row count. An earlier version of this probe
    // counted `L###` labels against "scrollback + viewport rows" and concluded
    // the emit drops a row — wrong, because it counted a BLANK viewport row as
    // content the emit owes. The emit carries every row that has content.
    //
    // The real question is whether the emit reproduces the last SCROLL, and the
    // control for that is a feed with no trailing newline: nothing scrolled, so
    // nothing can be lost, so the offset must disappear.
    println!();
    println!("  control — same content, no trailing newline (so nothing scrolled):");
    {
        let mut ctl = GhosttyEngine::create_with_budget(rows, cols, BUDGET);
        let mut f = Vec::new();
        for i in 0..lines {
            f.extend_from_slice(format!("L{i:03}").as_bytes());
            if i + 1 < lines {
                f.extend_from_slice(b"\r\n");
            }
        }
        ctl.feed(&f);
        let o = ctl.screen();
        let sb = ctl.scrollback_rows();
        let d = ctl.rehydrate().unwrap_or_default();
        let mut r2 = GhosttyEngine::create_with_budget(rows, cols, BUDGET);
        r2.feed(&d);
        let re2 = r2.screen();
        let aligned = (0..rows).filter(|r| o.row_text(*r) == re2.row_text(*r)).count();
        println!("    scrollback {sb}, aligned {aligned}/{rows}  -> {}",
            if aligned == usize::from(rows) { "NO OFFSET" } else { "still offset" });
        println!("    emit: {:?}", String::from_utf8_lossy(&d).escape_debug().to_string());
    }

    // ---- 3. does an explicit trailing newline fix the replay? ------------
    println!();
    println!("== 3. confirm by construction: does one more newline align it? ==");
    let mut b = GhosttyEngine::create_with_budget(rows, cols, BUDGET);
    b.feed(&dump);
    let plain = b.screen();
    let plain_aligned = (0..rows).filter(|r| orig.row_text(*r) == plain.row_text(*r)).count();

    let mut patched = dump.clone();
    patched.extend_from_slice(b"\r\n");
    let mut c = GhosttyEngine::create_with_budget(rows, cols, BUDGET);
    c.feed(&patched);
    let fixed = c.screen();
    let fixed_aligned = (0..rows).filter(|r| orig.row_text(*r) == fixed.row_text(*r)).count();

    println!("  replay as emitted:        {plain_aligned}/{rows} rows aligned");
    println!("  replay + one trailing CRLF: {fixed_aligned}/{rows} rows aligned");
    println!();
    for r in 0..rows {
        println!(
            "    row {r}: orig {:?} | replay {:?} | +CRLF {:?}",
            orig.row_text(r),
            plain.row_text(r),
            fixed.row_text(r)
        );
    }

    // ---- 4. same at the library's own defaults ---------------------------
    // The upstream report has to be reproducible without spyc's scrollback
    // configuration in the picture, so re-run the smallest case with no
    // `ghostty_terminal_set` calls at all.
    println!();
    println!("== 4. at the library's default limits (no ghostty_terminal_set) ==");
    {
        let mut d = GhosttyEngine::create_at_defaults(rows, cols);
        d.feed(&feed);
        let o = d.screen();
        let sb = d.scrollback_rows();
        let dd = d.rehydrate().unwrap_or_default();
        let mut r2 = GhosttyEngine::create_at_defaults(rows, cols);
        r2.feed(&dd);
        let re2 = r2.screen();
        let aligned = (0..rows).filter(|r| o.row_text(*r) == re2.row_text(*r)).count();
        println!("    scrollback {sb}, aligned {aligned}/{rows}, emit {} bytes -> {}",
            dd.len(),
            if aligned == usize::from(rows) { "NO OFFSET" } else { "OFFSET" });
    }
}
