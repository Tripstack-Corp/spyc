//! Differential fuzz: structured-random escape streams into every built
//! engine; a panic or a screen divergence is a finding.
//!
//! Written as an adversary and run BEFORE the report's conclusions, per the
//! house rule. Stable-toolchain and seeded rather than a `cargo-fuzz` target
//! so it runs in the same `cargo run` as everything else and every finding
//! reproduces from its printed seed; the generator below is the part worth
//! promoting into `fuzz/fuzz_targets/` (see the README).
//!
//! Panics are caught the way production catches them
//! (`pane::Pane::process_bytes_safe`), so one engine dying does not end the run.
//!
//! Run:  cargo run --release --features ghostty,wezterm --bin fuzz_diff -- [iters] [seed]

use std::panic::{AssertUnwindSafe, catch_unwind};

use vt_engine_spike::engine::{Engine, Screen};
use vt_engine_spike::engines::Vt100Engine;

/// xorshift64* — a seeded PRNG so every finding reproduces from its seed
/// without pulling a dependency in.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next() % n }
    }
    /// Return by value (not by reference) so the borrow of `self` ends before
    /// the caller uses the result in another `self` method.
    fn pick<T: Copy>(&mut self, xs: &[T]) -> T {
        xs[self.below(xs.len() as u64) as usize]
    }
}

/// Build a random byte stream out of terminal-shaped pieces rather than
/// uniform noise. Uniform bytes are almost all printable-or-ignored and never
/// reach the interesting state transitions; a generator that emits real CSI /
/// OSC / APC / SGR / charset / resize shapes does.
fn gen_stream(rng: &mut Rng, max_len: usize) -> Vec<u8> {
    // Final bytes that terminate a CSI sequence, including the ones whose
    // handlers touch the grid geometry (r = DECSTBM, H = CUP, X/P/@/L/M = edits).
    const CSI_FINAL: &[u8] = b"ABCDEFGHJKLMPSTXZdfghlmnrstu@`";
    const PRINTABLE: &[&str] = &[
        "a", "Z", "0", " ", "~",
        "\u{3042}", "\u{4e2d}",              // double-width
        "\u{1F600}",                          // emoji
        "\u{1F468}\u{200D}\u{1F469}",         // ZWJ
        "e\u{0301}",                          // combining
        "\u{2764}\u{FE0F}",                   // VS16
        "\u{1F3F4}\u{E0067}\u{E0062}",        // tag chars
    ];

    let mut out = Vec::with_capacity(max_len);
    while out.len() < max_len {
        match rng.below(12) {
            0 => {
                // CSI with 0..3 numeric params
                out.extend_from_slice(b"\x1b[");
                let n = rng.below(4);
                for i in 0..n {
                    if i > 0 {
                        out.push(b';');
                    }
                    out.extend_from_slice(rng.below(300).to_string().as_bytes());
                }
                out.push(rng.pick(CSI_FINAL));
            }
            1 => {
                // private-mode set/reset — alt screen, bracketed paste, mouse,
                // DECCKM, DECTCEM all live here
                out.extend_from_slice(b"\x1b[?");
                let modes: &[u16] = &[1, 7, 12, 25, 47, 1000, 1002, 1003, 1006, 1049, 2004, 2026];
                let m = rng.pick(modes);
                out.extend_from_slice(m.to_string().as_bytes());
                out.push(if rng.below(2) == 0 { b'h' } else { b'l' });
            }
            2 => {
                // SGR
                out.extend_from_slice(b"\x1b[");
                match rng.below(4) {
                    0 => out.extend_from_slice(rng.below(30).to_string().as_bytes()),
                    1 => out.extend_from_slice(
                        format!("38;5;{}", rng.below(256)).as_bytes(),
                    ),
                    2 => out.extend_from_slice(
                        format!(
                            "48;2;{};{};{}",
                            rng.below(256),
                            rng.below(256),
                            rng.below(256)
                        )
                        .as_bytes(),
                    ),
                    _ => out.extend_from_slice(b"0"),
                }
                out.push(b'm');
            }
            3 => { let p = rng.pick(PRINTABLE); out.extend_from_slice(p.as_bytes()); }
            4 => out.extend_from_slice(b"\r\n"),
            5 => out.push(rng.pick(b"\r\n\t\x08\x07\x0b\x0c")),
            6 => out.extend_from_slice(b"\x1b7"), // DECSC
            7 => out.extend_from_slice(b"\x1b8"), // DECRC
            8 => {
                // OSC, sometimes unterminated (a real risk: an agent's title
                // write can be split across pty reads)
                out.extend_from_slice(b"\x1b]");
                out.extend_from_slice(rng.below(200).to_string().as_bytes());
                out.push(b';');
                out.extend_from_slice(b"payload");
                match rng.below(3) {
                    0 => out.push(0x07),
                    1 => out.extend_from_slice(b"\x1b\\"),
                    _ => {} // deliberately unterminated
                }
            }
            9 => {
                // APC (kitty graphics shape)
                out.extend_from_slice(b"\x1b_G");
                out.extend_from_slice(b"a=T,f=24,s=1,v=1;AAAA");
                if rng.below(2) == 0 {
                    out.extend_from_slice(b"\x1b\\");
                }
            }
            10 => {
                // charset select — the DEC special graphics path
                out.extend_from_slice(b"\x1b(");
                out.push(rng.pick(b"0BAU"));
            }
            _ => {
                // raw byte, including invalid UTF-8 continuation bytes
                out.push(rng.below(256) as u8);
            }
        }
    }
    out
}

/// Compare only what all engines model identically; a divergence here is worth
/// a human look. Text is compared per row, attributes are not (the corpus
/// differential already pins those, and including them here would bury a
/// panic under thousands of style nits).
fn diverges(a: &Screen, b: &Screen) -> Option<(&'static str, String)> {
    if (a.rows, a.cols) != (b.rows, b.cols) {
        return Some((
            "geometry",
            format!("{}x{} vs {}x{}", a.rows, a.cols, b.rows, b.cols),
        ));
    }
    for r in 0..a.rows {
        let (ta, tb) = (a.row_text(r), b.row_text(r));
        if ta != tb {
            return Some(("row-text", format!("row {r}: {ta:?} vs {tb:?}")));
        }
    }
    if a.cursor != b.cursor {
        return Some((
            "cursor",
            format!("{:?} vs {:?}", a.cursor, b.cursor),
        ));
    }
    None
}

/// Feed one engine under a panic net; `Err` carries the panic message.
fn run_engine<E: Engine>(rows: u16, cols: u16, bytes: &[u8], resize: Option<(u16, u16)>) -> Result<Screen, String> {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let res = catch_unwind(AssertUnwindSafe(|| {
        let mut e = E::create(rows, cols, 1000);
        // Split the feed so a sequence straddles a chunk boundary, the way pty
        // reads actually deliver them.
        let mid = bytes.len() / 2;
        e.feed(&bytes[..mid]);
        if let Some((r, c)) = resize {
            e.resize(r, c);
        }
        e.feed(&bytes[mid..]);
        e.screen()
    }));
    std::panic::set_hook(prev);
    res.map_err(|e| {
        e.downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| e.downcast_ref::<&str>().copied())
            .unwrap_or("<non-string payload>")
            .to_string()
    })
}

fn main() {
    let mut args = std::env::args().skip(1);
    let first = args.next();

    // `--dump <seed>` prints one iteration's geometry and bytes as a
    // standalone reproducer. A fuzz finding is only useful upstream if it can
    // be handed over without this crate attached.
    if first.as_deref() == Some("--dump") {
        let seed: u64 = std::env::args()
            .nth(2)
            .and_then(|s| s.trim_start_matches("0x").parse().ok().or_else(|| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok()))
            .expect("--dump <seed>");
        let mut rng = Rng(seed | 1);
        let rows = 1 + rng.below(30) as u16;
        let cols = 1 + rng.below(40) as u16;
        let len = 64 + rng.below(700) as usize;
        let bytes = gen_stream(&mut rng, len);
        let resize = if rng.below(3) == 0 {
            Some((1 + rng.below(30) as u16, 1 + rng.below(40) as u16))
        } else {
            None
        };
        println!("seed      {seed:#x}");
        println!("geometry  {rows} rows x {cols} cols");
        println!("resize    {resize:?}   (applied at the midpoint of the feed)");
        println!("bytes     {} total, fed as two halves", bytes.len());
        println!();
        println!("// Rust byte literal:");
        print!("let input: &[u8] = &[");
        for (i, b) in bytes.iter().enumerate() {
            if i % 16 == 0 {
                print!("\n    ");
            }
            print!("0x{b:02x}, ");
        }
        println!("\n];");
        return;
    }

    // `--shrink <seed>` delta-debugs a panicking stream down to something a
    // maintainer can read. A 700-byte reproducer is technically valid and
    // practically ignored.
    #[cfg(feature = "wezterm")]
    if first.as_deref() == Some("--shrink") {
        use vt_engine_spike::engines::WeztermEngine;
        let seed: u64 = std::env::args()
            .nth(2)
            .and_then(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok())
            .expect("--shrink <hex seed>");
        let mut rng = Rng(seed | 1);
        let rows = 1 + rng.below(30) as u16;
        let cols = 1 + rng.below(40) as u16;
        let len = 64 + rng.below(700) as usize;
        let original = gen_stream(&mut rng, len);
        let panics = |b: &[u8]| run_engine::<WeztermEngine>(rows, cols, b, None).is_err();
        assert!(panics(&original), "seed {seed:#x} does not panic wezterm");

        let mut cur = original.clone();
        // Repeatedly try deleting a chunk; keep any deletion that still panics.
        let mut chunk = cur.len() / 2;
        while chunk >= 1 {
            let mut i = 0;
            while i < cur.len() {
                let end = (i + chunk).min(cur.len());
                let mut cand = cur[..i].to_vec();
                cand.extend_from_slice(&cur[end..]);
                if !cand.is_empty() && panics(&cand) {
                    cur = cand;
                } else {
                    i += chunk;
                }
            }
            chunk /= 2;
        }
        println!("seed     {seed:#x}");
        println!("geometry {rows} rows x {cols} cols");
        println!("shrunk   {} bytes -> {} bytes", original.len(), cur.len());
        println!("escaped  {:?}", String::from_utf8_lossy(&cur));
        print!("bytes    &[");
        for b in &cur {
            print!("0x{b:02x}, ");
        }
        println!("]");
        let msg = run_engine::<WeztermEngine>(rows, cols, &cur, None).err().unwrap_or_default();
        println!("panic    {msg}");
        return;
    }

    // `--variants` isolates WHICH part of a shrunk reproducer causes the
    // panic, so the report can explain the mechanism instead of just handing
    // over bytes.
    #[cfg(feature = "wezterm")]
    if first.as_deref() == Some("--variants") {
        use vt_engine_spike::engines::WeztermEngine;
        let cases: &[(&[u8], &str)] = &[
        (b"\x1b[;53H\x1b\x88", "the shrunk reproducer"),
        (b"\x1b[;53H\x88", "same, without the ESC (bare C1 HTS)"),
        (b"\x1b[;9H\x88", "CUP to col 9 on an 8-col screen"),
        (b"\x1b[;8H\x88", "CUP to col 8 (in range)"),
        (b"\x1b[;53H", "CUP alone, no HTS"),
        (b"\x88", "HTS alone at col 0"),
        ];
        for (bytes, desc) in cases {
            let r = run_engine::<WeztermEngine>(28, 8, bytes, None);
            println!(
                "  {:<38} {}",
                desc,
                r.err().map_or_else(|| "ok".to_string(), |m| format!("PANIC {m}"))
            );
        }
        return;
    }

    let iters: u64 = first.and_then(|s| s.parse().ok()).unwrap_or(20_000);
    let seed0: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(0x5157_C0DE);

    println!("differential fuzz: {iters} iterations, base seed {seed0:#x}");
    println!("engines: vt100{}{}",
        if cfg!(feature = "ghostty") { ", ghostty" } else { "" },
        if cfg!(feature = "wezterm") { ", wezterm" } else { "" });
    println!("{:-<100}", "");

    let mut panics: Vec<(u64, &str, String)> = Vec::new();
    #[allow(unused_mut, reason = "only mutated when both engine features are on")]
    let mut compared = 0usize;
    #[allow(unused_mut, reason = "only mutated when both engine features are on")]
    let mut agreed = 0usize;
    let mut vt100_outlier = 0usize;
    let mut three_way = 0usize;
    let mut outlier_kinds: std::collections::BTreeMap<&str, (usize, String)> =
        std::collections::BTreeMap::new();
    let mut gw_kinds: std::collections::BTreeMap<&str, (usize, u64, String)> =
        std::collections::BTreeMap::new();

    for i in 0..iters {
        let seed = seed0.wrapping_add(i.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let mut rng = Rng(seed | 1);
        // Geometries include the degenerate ones spyc's `.max(1)` clamp permits.
        let rows = 1 + rng.below(30) as u16;
        let cols = 1 + rng.below(40) as u16;
        let len = 64 + rng.below(700) as usize;
        let bytes = gen_stream(&mut rng, len);
        let resize = if rng.below(3) == 0 {
            Some((1 + rng.below(30) as u16, 1 + rng.below(40) as u16))
        } else {
            None
        };

        let v = run_engine::<Vt100Engine>(rows, cols, &bytes, resize);
        if let Err(ref m) = v {
            panics.push((seed, "vt100", m.clone()));
        }

        #[cfg(feature = "ghostty")]
        let g = {
            use vt_engine_spike::engines::GhosttyEngine;
            let g = run_engine::<GhosttyEngine>(rows, cols, &bytes, resize);
            if let Err(ref m) = g {
                panics.push((seed, "ghostty", m.clone()));
            }
            g
        };
        #[cfg(feature = "wezterm")]
        let w = {
            use vt_engine_spike::engines::WeztermEngine;
            let w = run_engine::<WeztermEngine>(rows, cols, &bytes, resize);
            if let Err(ref m) = w {
                panics.push((seed, "wezterm", m.clone()));
            }
            w
        };

        // All three pairings, so "the two modern engines agree and vt100 does
        // not" is countable rather than inferred.
        #[cfg(all(feature = "ghostty", feature = "wezterm"))]
        {
            let gw = match (&g, &w) {
                (Ok(gs), Ok(ws)) => diverges(gs, ws),
                _ => None,
            };
            let vg = match (&v, &g) {
                (Ok(vs), Ok(gs)) => diverges(vs, gs),
                _ => None,
            };
            let vw = match (&v, &w) {
                (Ok(vs), Ok(ws)) => diverges(vs, ws),
                _ => None,
            };
            let all_ok = v.is_ok() && g.is_ok() && w.is_ok();
            if all_ok {
                compared += 1;
                match (&gw, &vg, &vw) {
                    // Every engine agreed.
                    (None, None, None) => agreed += 1,
                    // ghostty == wezterm, vt100 differs: vt100 is the outlier.
                    (None, Some((k, ex)), Some(_)) => {
                        vt100_outlier += 1;
                        *outlier_kinds.entry(*k).or_insert((0usize, String::new())) =
                            (outlier_kinds.get(k).map_or(0, |e| e.0) + 1, ex.clone());
                    }
                    // The modern pair disagreed too — no majority.
                    _ => three_way += 1,
                }
            }
            if let Some((k, ex)) = gw {
                let e = gw_kinds.entry(k).or_insert((0usize, seed, ex.clone()));
                e.0 += 1;
            }
        }
    }

    // Group panics by message so a single root cause reads as one line.
    let mut by_msg: std::collections::BTreeMap<(&str, String), (usize, u64)> =
        std::collections::BTreeMap::new();
    for (seed, eng, msg) in &panics {
        let e = by_msg.entry((eng, msg.clone())).or_insert((0, *seed));
        e.0 += 1;
    }
    println!(
        "PANICS over {iters} iterations: {} total, {} distinct message(s)",
        panics.len(),
        by_msg.len()
    );
    for ((eng, msg), (count, first_seed)) in &by_msg {
        println!(
            "  {eng:<8} x{count:<6} ({:.3}% of iters)  first seed {first_seed:#x}  {msg}",
            100.0 * *count as f64 / iters as f64
        );
    }
    let per_engine = |name: &str| -> usize { panics.iter().filter(|(_, e, _)| *e == name).count() };
    println!(
        "  ---- totals: vt100={} ghostty={} wezterm={}",
        per_engine("vt100"),
        per_engine("ghostty"),
        per_engine("wezterm")
    );

    println!();
    println!("AGREEMENT over the {compared} iterations no engine panicked on:");
    println!(
        "  all three identical        {agreed:>7}  ({:.1}%)",
        100.0 * agreed as f64 / compared.max(1) as f64
    );
    println!(
        "  ghostty==wezterm, vt100 differs {vt100_outlier:>2}  ({:.1}%)  <- vt100 is the outlier",
        100.0 * vt100_outlier as f64 / compared.max(1) as f64
    );
    println!(
        "  no two agreed              {three_way:>7}  ({:.1}%)",
        100.0 * three_way as f64 / compared.max(1) as f64
    );
    println!();
    println!("  vt100-outlier divergences by kind:");
    for (k, (count, ex)) in &outlier_kinds {
        println!("    {k:<10} x{count:<7} e.g. {ex}");
    }
    println!();
    println!("  ghostty vs wezterm divergences by kind (the modern-pair baseline):");
    for (k, (count, seed, ex)) in &gw_kinds {
        println!("    {k:<10} x{count:<7} first seed {seed:#x}  e.g. {ex}");
    }
}
