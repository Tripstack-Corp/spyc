//! Resident memory at a FULL row budget, not a partially-filled one.
//!
//! `bench` feeds a fixture that fills ~4,000 of the 10,000-row budget, which
//! answers "what does a typical pane cost" but not "what does the budget cost".
//! Those differ by more than a factor for vt100, which allocates its grid
//! eagerly at 32 B/cell whether or not the rows hold anything, while ghostty
//! allocates pages as content arrives and stops at the derived byte ceiling.
//!
//! Written because the two were being compared across different row counts: a
//! ghostty *ceiling* against a vt100 *measurement* at 4,000 rows. Same budget,
//! same input, or the comparison means nothing.

use vt_engine_spike::engine::Engine;
use vt_engine_spike::engines::Vt100Engine;

const BUDGET: usize = 10_000;
const PANES: usize = 12;

fn rss() -> u64 {
    std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(|kb| kb * 1024)
        .unwrap_or(0)
}

/// Enough rows to overfill the budget, in the heavy shape the byte ceiling was
/// derived from — so this is the worst realistic case, not the average one.
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

fn measure<E: Engine>(name: &str, feed: &[u8]) {
    let before = rss();
    let mut panes: Vec<E> = Vec::with_capacity(PANES);
    for _ in 0..PANES {
        let mut e = E::create(24, 80, BUDGET);
        for c in feed.chunks(8192) {
            e.feed(c);
        }
        panes.push(e);
    }
    let after = rss();
    let delta = after.saturating_sub(before);
    let rows = panes.last_mut().map_or(0, Engine::scrollback_rows);
    println!(
        "  {name:<8} {:>8.2} MiB / {PANES} panes = {:>6.2} MiB/pane   rows retained {rows:>6}",
        delta as f64 / 1048576.0,
        delta as f64 / PANES as f64 / 1048576.0,
    );
    std::hint::black_box(&panes);
}

fn main() {
    // 15,000 heavy rows into a 10,000-row budget: both engines are asked to
    // discard, so both are measured AT their budget rather than under it.
    let feed = heavy(15_000);
    println!("full-budget memory: 15,000 heavy rows into a {BUDGET}-row budget, 24x80");
    println!("(vt100 allocates its grid eagerly; ghostty allocates pages on demand)");
    println!();
    measure::<Vt100Engine>("vt100", &feed);
    #[cfg(feature = "ghostty")]
    {
        measure::<vt_engine_spike::engines::GhosttyEngine>("ghostty", &feed);
        let l = vt_engine_spike::engines::GhosttyEngine::shipped_limits(BUDGET);
        println!();
        println!(
            "  ghostty's derived byte CEILING is {:.2} MiB/pane — a cap, not consumption.",
            l.max_bytes as f64 / 1048576.0
        );
    }
    #[cfg(feature = "wezterm")]
    measure::<vt_engine_spike::engines::WeztermEngine>("wezterm", &feed);
}
