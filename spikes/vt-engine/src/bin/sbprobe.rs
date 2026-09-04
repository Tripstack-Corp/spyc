//! Gate criteria for the scrollback budget, with the SHIPPED configuration.
//!
//! Two assertions, per `V2_2_PLAN.md` §8:
//!   1. realistic content retains **≥ budget minus one page** — the safety
//!      valve never binds on content a user could legitimately produce;
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

/// One page of libghostty grid space, per its header ("about 400KB").
const PAGE_BYTES: usize = 400 * 1024;

/// Rows in a page at the heavy rate the ceiling is derived from — the
/// tolerance for "approximately respected".
fn page_rows() -> usize {
    PAGE_BYTES / spyc_vt_sys_rate()
}

/// Kept as a function so the constant has one home even in this probe.
#[cfg(feature = "ghostty")]
fn spyc_vt_sys_rate() -> usize {
    spyc_vt_sys::scrollback::BYTES_PER_ROW_HEAVY
}
#[cfg(not(feature = "ghostty"))]
fn spyc_vt_sys_rate() -> usize {
    929
}

fn plain(n: usize) -> Vec<u8> {
    let mut v = Vec::new();
    for i in 0..n {
        v.extend_from_slice(
            format!("line {i:06} with about forty characters of text\r\n").as_bytes(),
        );
    }
    v
}

/// Heavy but legitimate: an agent CLI's styled frame.
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
    let tolerance = page_rows();
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
    println!("  tolerance for \"approximately respected\": one page = {tolerance} rows");
    println!("  (libghostty prunes whole pages, so the retained count quantises)");
    println!();

    // 15,000 rows into a 10,000-row budget: both engines must discard.
    const FED: usize = 15_000;
    println!("ASSERTION 1 — realistic content must NOT bind the valve");
    println!("  {:<28} {:>10} {:>10} {:>8}", "corpus / engine", "retained", "floor", "verdict");
    let floor = BUDGET - tolerance;
    for (label, feed) in [("plain", plain(FED)), ("heavy", heavy(FED))] {
        let v = retained::<Vt100Engine>(&feed);
        println!(
            "  {:<28} {v:>10} {floor:>10} {:>8}",
            format!("{label} / vt100"),
            if v >= floor { "ok" } else { "UNDER" }
        );
        #[cfg(feature = "ghostty")]
        {
            let gh = retained::<vt_engine_spike::engines::GhosttyEngine>(&feed);
            println!(
                "  {:<28} {gh:>10} {floor:>10} {:>8}",
                format!("{label} / ghostty"),
                if gh >= floor { "ok" } else { "UNDER" }
            );
        }
    }

    println!();
    println!("ASSERTION 2 — pathological content must bind it");
    let path = pathological(4_000, 80);
    println!("  input: 4,000 rows, {} KiB ({} B/row)", path.len() / 1024, path.len() / 4_000);
    #[cfg(feature = "ghostty")]
    {
        let gh = retained::<vt_engine_spike::engines::GhosttyEngine>(&path);
        let available = 4_000 - 24;
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
}
