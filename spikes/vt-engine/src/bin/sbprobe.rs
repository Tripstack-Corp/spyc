//! What unit is each engine's scrollback budget in?
//!
//! spyc passes `10_000` to `vt100::Parser::new` meaning ten thousand LINES
//! (`Pane::spawn_with_env`). If a candidate reads that number as bytes, the
//! same constant buys a different amount of history — which is a migration
//! detail, not a footnote, because `^a v` is built on that history.
use vt_engine_spike::engine::Engine;
use vt_engine_spike::engines::Vt100Engine;

fn feed_lines(n: usize) -> Vec<u8> {
    let mut v = Vec::new();
    for i in 0..n {
        v.extend_from_slice(format!("line {i:05} with about forty characters of text\r\n").as_bytes());
    }
    v
}

fn main() {
    let bytes = feed_lines(5000);
    println!("input: 5000 lines, {} bytes, 24x80 viewport", bytes.len());
    println!("{:-<78}", "");
    for budget in [1_000usize, 10_000, 100_000, 1_000_000] {
        let mut v = Vt100Engine::create(24, 80, budget);
        v.feed(&bytes);
        print!("budget {budget:>9}  vt100={:>6}", v.scrollback_rows());
        #[cfg(feature = "ghostty")]
        {
            use vt_engine_spike::engines::GhosttyEngine;
            let mut g = GhosttyEngine::create(24, 80, budget);
            g.feed(&bytes);
            print!("  ghostty={:>6}", g.scrollback_rows());
        }
        #[cfg(feature = "wezterm")]
        {
            use vt_engine_spike::engines::WeztermEngine;
            let mut w = WeztermEngine::create(24, 80, budget);
            w.feed(&bytes);
            print!("  wezterm={:>6}", w.scrollback_rows());
        }
        println!();
    }
    println!("{:-<78}", "");
    println!("A count that scales with the budget linearly in BYTES rather than");
    println!("saturating at 4976 (5000 fed - 24 on screen) reads it as a byte cap.");
}
