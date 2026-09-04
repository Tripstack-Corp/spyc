//! Feed identical bytes to every built engine and report where they disagree.
//!
//! vt100 is the reference column only because it is the incumbent, NOT because
//! it is assumed correct — several divergences below are vt100 losing. Each
//! divergence is printed with enough context to adjudicate it by hand; the
//! report classifies them, this tool does not.
//!
//! Run:  cargo run --release --features ghostty,wezterm --bin differential

use vt_engine_spike::engine::{Engine, Screen};
use vt_engine_spike::engines::Vt100Engine;
use vt_engine_spike::fixtures;

/// Where two engines disagree about one case.
struct Divergence {
    kind: &'static str,
    detail: String,
}

fn compare(a_name: &str, a: &Screen, b_name: &str, b: &Screen) -> Vec<Divergence> {
    let mut out = Vec::new();

    if (a.rows, a.cols) != (b.rows, b.cols) {
        out.push(Divergence {
            kind: "geometry",
            detail: format!(
                "{a_name} {}x{} vs {b_name} {}x{}",
                a.rows, a.cols, b.rows, b.cols
            ),
        });
        return out; // nothing below is comparable at different geometry
    }

    if a.cursor != b.cursor {
        out.push(Divergence {
            kind: "cursor-pos",
            detail: format!("{a_name} {:?} vs {b_name} {:?}", a.cursor, b.cursor),
        });
    }
    if a.cursor_visible != b.cursor_visible {
        out.push(Divergence {
            kind: "cursor-vis",
            detail: format!(
                "{a_name} {} vs {b_name} {}",
                a.cursor_visible, b.cursor_visible
            ),
        });
    }
    if a.alt_screen != b.alt_screen {
        out.push(Divergence {
            kind: "alt-screen",
            detail: format!("{a_name} {} vs {b_name} {}", a.alt_screen, b.alt_screen),
        });
    }

    for r in 0..a.rows {
        let (ta, tb) = (a.row_text(r), b.row_text(r));
        if ta != tb {
            out.push(Divergence {
                kind: "row-text",
                detail: format!("row {r}\n      {a_name:>8}: {ta:?}\n      {b_name:>8}: {tb:?}"),
            });
        }
        if a.wrapped.get(usize::from(r)) != b.wrapped.get(usize::from(r)) {
            out.push(Divergence {
                kind: "row-wrapped",
                detail: format!(
                    "row {r}: {a_name} {:?} vs {b_name} {:?}",
                    a.wrapped.get(usize::from(r)),
                    b.wrapped.get(usize::from(r))
                ),
            });
        }
    }

    // Attributes, compared only on cells where BOTH engines drew the same text.
    // A cell whose text already diverged would report its attributes twice.
    let mut attr_diffs = Vec::new();
    for r in 0..a.rows {
        for c in 0..a.cols {
            let (Some(ca), Some(cb)) = (a.cell(r, c), b.cell(r, c)) else {
                continue;
            };
            if ca.text != cb.text || (ca.is_blank() && cb.is_blank()) {
                continue;
            }
            if (ca.fg, ca.bg, ca.bold, ca.italic, ca.underline, ca.reverse)
                != (cb.fg, cb.bg, cb.bold, cb.italic, cb.underline, cb.reverse)
            {
                attr_diffs.push(format!(
                    "({r},{c}) {:?} fg={:?} bg={:?} b={} i={} u={} rev={} | {:?} fg={:?} bg={:?} b={} i={} u={} rev={}",
                    ca.text, ca.fg, ca.bg, ca.bold, ca.italic, ca.underline, ca.reverse,
                    cb.text, cb.fg, cb.bg, cb.bold, cb.italic, cb.underline, cb.reverse
                ));
            }
            if ca.wide != cb.wide {
                attr_diffs.push(format!(
                    "({r},{c}) wide {a_name} {:?} vs {b_name} {:?}",
                    ca.wide, cb.wide
                ));
            }
        }
    }
    if !attr_diffs.is_empty() {
        let shown = attr_diffs.len().min(4);
        out.push(Divergence {
            kind: "cell-attrs",
            detail: format!(
                "{} cell(s); first {shown}:\n      {}",
                attr_diffs.len(),
                attr_diffs[..shown].join("\n      ")
            ),
        });
    }

    out
}

/// One case's bytes + geometry.
struct Case {
    name: String,
    what: String,
    bytes: Vec<u8>,
    size: (u16, u16),
}

fn cases() -> Vec<Case> {
    let mut v: Vec<Case> = fixtures::SYNTHETIC
        .iter()
        .map(|s| Case {
            name: s.name.to_string(),
            what: s.what.to_string(),
            bytes: s.bytes.to_vec(),
            size: s.size,
        })
        .collect();
    // Captured streams run at the geometry they were captured at (24x80 —
    // see the README's capture commands).
    for (name, bytes) in fixtures::captured() {
        v.push(Case {
            name: format!("captured/{name}"),
            what: "real PTY stream".to_string(),
            bytes,
            size: (24, 80),
        });
    }
    v
}

const SCROLLBACK: usize = 10_000; // what `Pane::spawn_with_env` uses

fn main() {
    let cases = cases();
    let mut engines_present = vec!["vt100"];
    #[cfg(feature = "ghostty")]
    engines_present.push("ghostty");
    #[cfg(feature = "wezterm")]
    engines_present.push("wezterm");

    println!("engines: {}", engines_present.join(", "));
    println!("cases:   {} ({} synthetic)", cases.len(), fixtures::SYNTHETIC.len());
    println!("{:=<100}", "");

    let mut total_div = 0usize;
    let mut clean = 0usize;

    for case in &cases {
        let (rows, cols) = case.size;

        let mut v = Vt100Engine::create(rows, cols, SCROLLBACK);
        v.feed(&case.bytes);
        let vs = v.screen();
        let v_sb = v.scrollback_rows();

        let mut divs: Vec<(String, Vec<Divergence>)> = Vec::new();
        let mut sb_line = format!("vt100={v_sb}");

        #[cfg(feature = "ghostty")]
        {
            use vt_engine_spike::engines::GhosttyEngine;
            let mut g = GhosttyEngine::create(rows, cols, SCROLLBACK);
            g.feed(&case.bytes);
            let gs = g.screen();
            let g_sb = g.scrollback_rows();
            sb_line.push_str(&format!(" ghostty={g_sb}"));
            divs.push(("vt100 vs ghostty".into(), compare("vt100", &vs, "ghostty", &gs)));
        }
        #[cfg(feature = "wezterm")]
        {
            use vt_engine_spike::engines::WeztermEngine;
            let mut w = WeztermEngine::create(rows, cols, SCROLLBACK);
            w.feed(&case.bytes);
            let ws = w.screen();
            let w_sb = w.scrollback_rows();
            sb_line.push_str(&format!(" wezterm={w_sb}"));
            divs.push(("vt100 vs wezterm".into(), compare("vt100", &vs, "wezterm", &ws)));
        }

        let n: usize = divs.iter().map(|(_, d)| d.len()).sum();
        total_div += n;
        if n == 0 {
            clean += 1;
            println!("PARITY  {:<34} [{rows}x{cols}] scrollback: {sb_line}", case.name);
            continue;
        }

        println!("DIVERGE {:<34} [{rows}x{cols}] scrollback: {sb_line}", case.name);
        println!("        {}", case.what);
        for (pair, ds) in &divs {
            for d in ds {
                println!("    [{}] {:<12} {}", pair, d.kind, d.detail);
            }
        }
    }

    println!("{:=<100}", "");
    println!(
        "{clean}/{} cases at exact parity across all built engines; {total_div} divergence(s)",
        cases.len()
    );
}
