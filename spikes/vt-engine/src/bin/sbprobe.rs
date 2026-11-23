//! Gate criteria for the scrollback budget, with the SHIPPED configuration.
//!
//! Two assertions, per `docs/archive/V2_2_PLAN.md` §8:
//!   1. the safety valve never binds on content a user could legitimately
//!      produce: fed past the budget a step at a time, realistic content keeps
//!      exactly the rows it keeps with no byte limit, after every step (#1147);
//!   2. a pathological stream demonstrably **does** bind it, page-granular
//!      truncation and all — the safety path is tested, not theoretical.
//!
//! "Approximately respected", never equality: libghostty prunes whole pages, so
//! the retained count quantises. Asserting an exact number would fail on a
//! correct implementation.

use vt_engine_spike::engine::Engine;
use vt_engine_spike::engines::Vt100Engine;

/// spyc's default, from `Pane::spawn_with_env`.
const BUDGET: usize = 10_000;

fn plain(n: usize) -> Vec<u8> {
    (0..n).flat_map(plain_row).collect()
}

fn plain_row(i: usize) -> Vec<u8> {
    format!("line {i:06} with about forty characters of text\r\n").into_bytes()
}

/// Heavy but legitimate: an agent CLI's styled frame.
fn heavy(n: usize) -> Vec<u8> {
    (0..n).flat_map(heavy_row).collect()
}

fn heavy_row(i: usize) -> Vec<u8> {
    format!(
        "\x1b[38;5;244m│\x1b[0m \x1b[1;36m{i:05}\x1b[0m \
\x1b[38;2;180;200;255mtool\x1b[0m\x1b[2m·\x1b[0m\x1b[33mRead\x1b[0m \
\u{2500}\u{2500} \u{3042}\u{1F600} e\u{0301} \x1b[4mpath/to/file.rs\x1b[0m\r\n"
    )
    .into_bytes()
}

/// Pathological: a distinct truecolour pair and a grapheme cluster in EVERY
/// cell. Not realistic — this is what the valve exists for.
fn pathological(n: usize, cols: usize) -> Vec<u8> {
    let mut v = Vec::new();
    for i in 0..n {
        for c in 0..cols {
            let (r, g, b) = (
                ((i * 7 + c * 13) % 256) as u8,
                ((i * 11 + c * 5) % 256) as u8,
                ((i * 3 + c * 17) % 256) as u8,
            );
            v.extend_from_slice(format!("\x1b[38;2;{r};{g};{b};48;2;{b};{r};{g}m").as_bytes());
            v.extend_from_slice("e\u{0301}\u{0308}".as_bytes());
        }
        v.extend_from_slice(b"\x1b[0m\r\n");
    }
    v
}

fn retained<E: Engine>(feed: &[u8]) -> usize {
    let mut e = E::create(24, 80, BUDGET);
    for c in feed.chunks(8192) {
        e.feed(c);
    }
    e.scrollback_rows()
}

fn main() {
    println!("scrollback budget at the SHIPPED configuration");
    #[cfg(feature = "ghostty")]
    {
        let l = spyc_vt_sys::scrollback::limits_for_row_budget(BUDGET);
        println!(
            "  budget {BUDGET} rows -> max_lines {} / max_bytes {} ({:.2} MiB)",
            l.max_lines,
            l.max_bytes,
            l.max_bytes as f64 / 1048576.0
        );
    }
    println!("  (libghostty prunes whole pages, so the retained count quantises)");
    println!();

    // 15,000 rows into a 10,000-row budget: both engines must discard.
    const FED: usize = 15_000;
    println!("ASSERTION 1 — realistic content must NOT bind the valve");
    println!("  vt100, for contrast: its line limit is exact");
    for (label, feed) in [("plain", plain(FED)), ("heavy", heavy(FED))] {
        let v = retained::<Vt100Engine>(&feed);
        println!("    {label:<6} {FED} rows fed, {v} kept");
    }
    #[cfg_attr(not(feature = "ghostty"), allow(unused_mut))]
    let mut failed = false;
    #[cfg(feature = "ghostty")]
    {
        let shipped = spyc_vt_sys::scrollback::limits_for_row_budget(BUDGET).max_bytes;
        println!(
            "  ghostty: {SWEEP} rows fed {STEP} at a time; after every step the rows kept at the"
        );
        println!("  shipped ceiling must equal the rows kept with no byte limit at all");
        let corpora: [(&str, fn(usize) -> Vec<u8>); 2] =
            [("plain", plain_row), ("heavy", heavy_row)];
        for (label, row) in corpora {
            let with_valve = ghostty_sweep(row, shipped);
            let without = ghostty_sweep(row, usize::MAX);
            let differs = with_valve.iter().zip(&without).position(|(a, b)| a != b);
            // Past the budget the line limit prunes whole pages, so the count
            // saws between the budget and about a page under it.
            let past = (BUDGET / STEP)..;
            let low = with_valve[past.clone()].iter().min().copied().unwrap_or(0);
            let high = with_valve[past].iter().max().copied().unwrap_or(0);
            match differs {
                None => println!(
                    "    {label:<6} ok: identical at all {} steps; kept {low}..={high} past the budget",
                    with_valve.len()
                ),
                Some(i) => {
                    failed = true;
                    println!(
                        "    {label:<6} BOUND at {} rows fed: {} kept, {} with no byte limit",
                        (i + 1) * STEP,
                        with_valve[i],
                        without[i]
                    );
                }
            }
        }
    }
    println!();
    println!("ASSERTION 2 — pathological content must bind it");
    let path = pathological(4_000, 80);
    println!(
        "  input: 4,000 rows, {} KiB ({} B/row)",
        path.len() / 1024,
        path.len() / 4_000
    );
    #[cfg(feature = "ghostty")]
    {
        let gh = retained::<vt_engine_spike::engines::GhosttyEngine>(&path);
        let available = 4_000 - 24;
        failed |= gh >= available;
        println!(
            "  ghostty retained {gh} of {available} available -> {}",
            if gh < available {
                "valve BOUND (the safety path works)"
            } else {
                "valve did NOT bind — the ceiling is too generous to be a valve"
            }
        );
        let l = spyc_vt_sys::scrollback::limits_for_row_budget(BUDGET);
        // Deliberately NOT reported as an input-bytes figure. The ceiling binds
        // on ghostty's STORAGE bytes, which the C API does not expose, and
        // storage per row for per-cell-styled content is several times the
        // input bytes per row. Dividing the ceiling by the retained count is
        // the only defensible direction.
        println!(
            "  {:.2} MiB ceiling / {gh} rows retained => ghostty stored ~{:.1} KiB per row",
            l.max_bytes as f64 / 1048576.0,
            l.max_bytes as f64 / gh as f64 / 1024.0
        );
        println!(
            "  (vs ~{:.1} KiB/row of INPUT — storage exceeds input for per-cell styling,",
            path.len() as f64 / 4_000.0 / 1024.0
        );
        println!("   which is exactly the case a line-only budget cannot bound)");
    }
    // vt100 has no byte limit at all, which is the contrast worth showing: it
    // will hold the full row budget of pathological content.
    let v = retained::<Vt100Engine>(&path);
    println!("  vt100 retained {v} — it has no byte ceiling to bind");
    if failed {
        std::process::exit(1);
    }
}

/// Rows of the sweep, fed this many at a time.
#[cfg(feature = "ghostty")]
const SWEEP: usize = 60_000;
#[cfg(feature = "ghostty")]
const STEP: usize = 250;

/// Rows ghostty keeps after each step of feeding `row`'s corpus, at the
/// shipped line limit and a byte ceiling of `max_bytes` (`usize::MAX` is none).
#[cfg(feature = "ghostty")]
fn ghostty_sweep(row: fn(usize) -> Vec<u8>, max_bytes: usize) -> Vec<usize> {
    use spyc_vt_sys::ffi::{self, GhosttyTerminalData as Data, GhosttyTerminalOption as Opt};
    let mut t: ffi::GhosttyTerminal = std::ptr::null_mut();
    // SAFETY: `t` is a valid out-pointer; a null allocator selects the default.
    let made = unsafe { ffi::ghostty_terminal_new(std::ptr::null(), &raw mut t, 80, 24) };
    assert_eq!(made, spyc_vt_sys::SUCCESS);
    let lines = BUDGET;
    // SAFETY: `t` is live; each option reads one `usize` through its pointer.
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
    let mut kept = Vec::with_capacity(SWEEP / STEP);
    for step in 0..SWEEP / STEP {
        let bytes: Vec<u8> = (step * STEP..(step + 1) * STEP).flat_map(row).collect();
        for c in bytes.chunks(8192) {
            // SAFETY: `t` is live and `c` is a valid slice for its length.
            unsafe { ffi::ghostty_terminal_vt_write(t, c.as_ptr(), c.len()) };
        }
        let mut rows: usize = 0;
        // SAFETY: `t` is live; the data is one `usize` written through the pointer.
        let ok = unsafe {
            ffi::ghostty_terminal_get(
                t,
                Data::GHOSTTY_TERMINAL_DATA_SCROLLBACK_ROWS,
                (&raw mut rows).cast(),
            )
        };
        assert_eq!(ok, spyc_vt_sys::SUCCESS);
        kept.push(rows);
    }
    // SAFETY: `t` came from `ghostty_terminal_new` and is not used again.
    unsafe { ffi::ghostty_terminal_free(t) };
    kept
}
