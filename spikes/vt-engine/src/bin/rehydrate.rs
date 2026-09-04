//! The 3.0 criterion: parse a stream, dump state, replay into a fresh engine,
//! diff against the original.
//!
//! This is the zmx pattern's core operation. Today the engine renders a pane;
//! after 3.0 it is the source of truth a reattaching client is reconstructed
//! from, so "can it emit its own state, and is the replay faithful?" stops
//! being a nice-to-have.
//!
//! Graded on the three things that actually matter for a reattach:
//!   1. fidelity of the reconstruction (visible screen, cell for cell)
//!   2. whether the emission can be parameterized by target capabilities
//!   3. scrollback reconstruction depth, not just the visible screen
//!
//! Run:  cargo run --release --features ghostty,wezterm --bin rehydrate

use vt_engine_spike::engine::{Engine, Screen};
use vt_engine_spike::engines::Vt100Engine;
use vt_engine_spike::fixtures;

const SCROLLBACK: usize = 10_000;

/// Cell-for-cell agreement between the original and the replayed screen.
struct Fidelity {
    cells_total: usize,
    cells_text_ok: usize,
    cells_attr_ok: usize,
    cursor_ok: bool,
    alt_ok: bool,
    rows_text_ok: usize,
    rows_total: usize,
    /// Rows matched after allowing the best small viewport shift.
    rows_aligned_ok: usize,
    /// The shift that achieved it (0 = no shift needed).
    shift: i32,
}

/// Rows matched when the replayed viewport is allowed to sit `shift` rows off.
/// Separates "the emit lost content" from "the emit put the same content one
/// row up", which score identically on an exact compare and mean very
/// different things for a reattach.
fn rows_matched_at(orig: &Screen, replayed: &Screen, shift: i32) -> usize {
    let mut ok = 0;
    for r in 0..orig.rows {
        let src = i32::from(r) + shift;
        if src < 0 || src >= i32::from(replayed.rows) {
            continue;
        }
        if orig.row_text(r) == replayed.row_text(src as u16) {
            ok += 1;
        }
    }
    ok
}

/// Best row agreement over a small shift window, and the shift that achieved it.
fn best_alignment(orig: &Screen, replayed: &Screen) -> (usize, i32) {
    (-2..=2)
        .map(|s| (rows_matched_at(orig, replayed, s), s))
        .max_by_key(|(n, s)| (*n, -s.abs()))
        .unwrap_or((0, 0))
}

fn grade(orig: &Screen, replayed: &Screen) -> Fidelity {
    let mut f = Fidelity {
        cells_total: 0,
        cells_text_ok: 0,
        cells_attr_ok: 0,
        cursor_ok: orig.cursor == replayed.cursor,
        alt_ok: orig.alt_screen == replayed.alt_screen,
        rows_text_ok: 0,
        rows_total: usize::from(orig.rows),
        rows_aligned_ok: 0,
        shift: 0,
    };
    let (aligned, shift) = best_alignment(orig, replayed);
    f.rows_aligned_ok = aligned;
    f.shift = shift;
    for r in 0..orig.rows {
        if orig.row_text(r) == replayed.row_text(r) {
            f.rows_text_ok += 1;
        }
        for c in 0..orig.cols {
            let (Some(a), Some(b)) = (orig.cell(r, c), replayed.cell(r, c)) else {
                continue;
            };
            f.cells_total += 1;
            // Normalize blank-vs-space: the grid is space-padded, and an
            // emitted redraw legitimately writes one or the other.
            let ta = if a.text == " " { "" } else { a.text.as_str() };
            let tb = if b.text == " " { "" } else { b.text.as_str() };
            if ta == tb {
                f.cells_text_ok += 1;
            }
            if (a.fg, a.bg, a.bold, a.italic, a.underline, a.reverse)
                == (b.fg, b.bg, b.bold, b.italic, b.underline, b.reverse)
            {
                f.cells_attr_ok += 1;
            }
        }
    }
    f
}

fn pct(n: usize, d: usize) -> f64 {
    if d == 0 { 100.0 } else { 100.0 * n as f64 / d as f64 }
}

/// Run the round trip for one engine over one case.
fn round_trip<E: Engine>(rows: u16, cols: u16, bytes: &[u8]) -> Option<(Fidelity, usize, usize, usize)> {
    let mut a = E::create(rows, cols, SCROLLBACK);
    a.feed(bytes);
    let orig = a.screen();
    let orig_sb = a.scrollback_rows();
    let dump = a.rehydrate()?;

    let mut b = E::create(rows, cols, SCROLLBACK);
    b.feed(&dump);
    let replayed = b.screen();
    let new_sb = b.scrollback_rows();
    Some((grade(&orig, &replayed), dump.len(), orig_sb, new_sb))
}

fn report<E: Engine>(name: &str, cases: &[(String, Vec<u8>, (u16, u16))]) {
    let mut any = false;
    let mut tot_cells = 0usize;
    let mut ok_text = 0usize;
    let mut ok_attr = 0usize;
    let mut ok_rows = 0usize;
    let mut tot_rows = 0usize;
    let mut ok_rows_aligned = 0usize;
    let mut shifted_cases: std::collections::BTreeMap<i32, usize> = std::collections::BTreeMap::new();
    let mut cursor_ok = 0usize;
    let mut alt_ok = 0usize;
    let mut dump_bytes = 0usize;
    let mut sb_before = 0usize;
    let mut sb_after = 0usize;
    let mut n = 0usize;

    for (case, bytes, (rows, cols)) in cases {
        match round_trip::<E>(*rows, *cols, bytes) {
            None => {
                println!("  {name}: NO REHYDRATION API — nothing to measure");
                return;
            }
            Some((f, dl, sbb, sba)) => {
                any = true;
                n += 1;
                tot_cells += f.cells_total;
                ok_text += f.cells_text_ok;
                ok_attr += f.cells_attr_ok;
                ok_rows += f.rows_text_ok;
                ok_rows_aligned += f.rows_aligned_ok;
                if f.shift != 0 {
                    *shifted_cases.entry(f.shift).or_insert(0) += 1;
                }
                tot_rows += f.rows_total;
                cursor_ok += usize::from(f.cursor_ok);
                alt_ok += usize::from(f.alt_ok);
                dump_bytes += dl;
                sb_before += sbb;
                sb_after += sba;
                // Name the cases that do not round-trip cleanly; an average
                // hides exactly the ones a reattach would show the user.
                if f.rows_text_ok < f.rows_total || !f.cursor_ok || !f.alt_ok {
                    println!(
                        "    {case:<30} rows {}/{} (={}/{} at shift {:+})  cursor {}  alt {}  dump {dl}B",
                        f.rows_text_ok,
                        f.rows_total,
                        f.rows_aligned_ok,
                        f.rows_total,
                        f.shift,
                        if f.cursor_ok { "ok" } else { "LOST" },
                        if f.alt_ok { "ok" } else { "LOST" },
                    );
                }
            }
        }
    }
    if !any {
        return;
    }
    println!(
        "  {name}: rows-exact {:.1}%  rows-shift-tolerant {:.1}%  cell-text {:.1}%  cell-attrs {:.1}%  cursor {}/{n}  alt-screen {}/{n}",
        pct(ok_rows, tot_rows),
        pct(ok_rows_aligned, tot_rows),
        pct(ok_text, tot_cells),
        pct(ok_attr, tot_cells),
        cursor_ok,
        alt_ok,
    );
    if !shifted_cases.is_empty() {
        println!("  {name}: viewport OFFSET on {shifted_cases:?} (shift -> case count)");
    }
    println!(
        "  {name}: scrollback {sb_before} rows in  ->  {sb_after} rows out ({:.1}% preserved); {dump_bytes}B emitted total",
        pct(sb_after, sb_before)
    );
}

fn main() {
    let mut cases: Vec<(String, Vec<u8>, (u16, u16))> = fixtures::SYNTHETIC
        .iter()
        .map(|s| (s.name.to_string(), s.bytes.to_vec(), s.size))
        .collect();
    for (name, bytes) in fixtures::captured() {
        cases.push((format!("captured/{name}"), bytes, (24, 80)));
    }

    println!("rehydration round trip over {} cases", cases.len());
    println!("(feed -> dump state -> replay into a fresh engine -> diff)");
    println!("{:=<100}", "");
    println!("cases that did NOT round-trip cleanly are named under each engine:");
    println!();

    report::<Vt100Engine>("vt100  ", &cases);
    println!();
    #[cfg(feature = "ghostty")]
    {
        use vt_engine_spike::engines::GhosttyEngine;
        report::<GhosttyEngine>("ghostty", &cases);
        println!();
    }
    #[cfg(feature = "wezterm")]
    {
        use vt_engine_spike::engines::WeztermEngine;
        report::<WeztermEngine>("wezterm", &cases);
    }
}
