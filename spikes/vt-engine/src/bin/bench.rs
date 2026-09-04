//! Throughput and resident-memory per pane.
//!
//! Throughput replays the `big-spew` fixture (a build-log-shaped stream) end to
//! end, in 8 KiB chunks — the same chunk size `PtyHost`'s reader thread uses —
//! because feeding one giant slice measures a code path production never takes.
//!
//! Memory is measured as process RSS growth across N live terminals at spyc's
//! real scrollback depth, which is what a tabbed pane set actually costs. RSS
//! is coarse (allocator-dependent, page-granular), so it is reported as a
//! per-pane average over enough panes that the fixed cost amortizes, and the
//! baseline is taken after the fixture is loaded so the fixture is not counted.
//!
//! Run:  cargo run --release --features ghostty,wezterm --bin bench

use std::time::Instant;

use vt_engine_spike::engine::Engine;
use vt_engine_spike::engines::Vt100Engine;

const CHUNK: usize = 8192; // PtyHost reader-thread buffer size
const SCROLLBACK: usize = 10_000; // Pane::spawn_with_env
const PANES: usize = 24;

/// Process RSS in bytes, via `ps` — coarse but dependency-free and the same
/// number a user watching Activity Monitor would see.
fn rss_bytes() -> u64 {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output();
    out.ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(|kb| kb * 1024)
        .unwrap_or(0)
}

fn throughput<E: Engine>(name: &str, data: &[u8], reps: usize) {
    // Warm once so the first run's page faults don't land in the timing.
    {
        let mut e = E::create(24, 80, SCROLLBACK);
        e.feed(&data[..data.len().min(CHUNK)]);
    }
    let mut e = E::create(24, 80, SCROLLBACK);
    let t0 = Instant::now();
    let mut total = 0usize;
    for _ in 0..reps {
        for chunk in data.chunks(CHUNK) {
            e.feed(chunk);
            total += chunk.len();
        }
    }
    let dt = t0.elapsed();
    let mbps = (total as f64 / (1024.0 * 1024.0)) / dt.as_secs_f64();
    println!(
        "  {name:<8} {:>7.1} MiB/s   ({} MiB in {:.3}s, {} chunks of {CHUNK}B)",
        mbps,
        total / (1024 * 1024),
        dt.as_secs_f64(),
        total / CHUNK
    );
}

fn memory<E: Engine>(name: &str, data: &[u8]) {
    let before = rss_bytes();
    let mut panes: Vec<E> = Vec::with_capacity(PANES);
    for _ in 0..PANES {
        let mut e = E::create(24, 80, SCROLLBACK);
        for chunk in data.chunks(CHUNK) {
            e.feed(chunk);
        }
        panes.push(e);
    }
    let after = rss_bytes();
    let delta = after.saturating_sub(before);
    // Per-ROW cost as well as per-pane: the engines do not all retain the same
    // amount of history from the same input (see the report's note on
    // ghostty's inert `max_scrollback` at the matched commit), so a bare
    // per-pane figure rewards whichever one stored least.
    let rows_held = panes.last_mut().map_or(0, |e| e.scrollback_rows()).max(1);
    println!(
        "  {name:<8} {:>7.2} MiB / {PANES} panes = {:>6.0} KiB/pane | {:>5.2} KiB per retained row ({rows_held} rows held)",
        delta as f64 / (1024.0 * 1024.0),
        delta as f64 / PANES as f64 / 1024.0,
        delta as f64 / PANES as f64 / rows_held as f64 / 1024.0,
    );
    // Keep them alive until after the measurement.
    std::hint::black_box(&panes);
}

fn main() {
    let data = std::fs::read("fixtures/big-spew.bin").expect("fixtures/big-spew.bin");
    let reps: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(40);
    println!(
        "fixture: fixtures/big-spew.bin, {} KiB, replayed {reps}x in {CHUNK}B chunks",
        data.len() / 1024
    );
    println!("geometry 24x80, scrollback budget {SCROLLBACK}");
    println!();
    println!("THROUGHPUT");
    throughput::<Vt100Engine>("vt100", &data, reps);
    #[cfg(feature = "ghostty")]
    throughput::<vt_engine_spike::engines::GhosttyEngine>("ghostty", &data, reps);
    #[cfg(feature = "wezterm")]
    throughput::<vt_engine_spike::engines::WeztermEngine>("wezterm", &data, reps);

    println!();
    println!("PER-CELL FOOTPRINT (the fixed cost behind the per-row numbers)");
    println!(
        "  vt100    size_of::<vt100::Cell>() = {} B  -> {} B per 80-col row",
        std::mem::size_of::<vt100::Cell>(),
        std::mem::size_of::<vt100::Cell>() * 80
    );

    println!();
    println!("RESIDENT MEMORY (RSS delta, {PANES} panes each fed the whole fixture)");
    memory::<Vt100Engine>("vt100", &data);
    #[cfg(feature = "ghostty")]
    memory::<vt_engine_spike::engines::GhosttyEngine>("ghostty", &data);
    #[cfg(feature = "wezterm")]
    memory::<vt_engine_spike::engines::WeztermEngine>("wezterm", &data);
}
