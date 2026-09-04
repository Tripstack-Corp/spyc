//! Show the original vs replayed viewport for one case, row by row, so a
//! "0/24 rows matched" score can be read as either an offset or a real loss.
use vt_engine_spike::engine::Engine;
use vt_engine_spike::fixtures;

fn main() {
    let want = std::env::args().nth(1).unwrap_or_else(|| "big-spew".into());
    let bytes = fixtures::captured()
        .into_iter()
        .find(|(n, _)| *n == want)
        .map(|(_, b)| b)
        .unwrap_or_else(|| panic!("no fixture {want}"));

    #[cfg(feature = "ghostty")]
    {
        use vt_engine_spike::engines::GhosttyEngine;
        let mut a = GhosttyEngine::create(24, 80, 10_000);
        a.feed(&bytes);
        let orig = a.screen();
        let dump = a.rehydrate().expect("ghostty emits");
        let mut b = GhosttyEngine::create(24, 80, 10_000);
        b.feed(&dump);
        let re = b.screen();
        println!("case {want}: dump {} bytes", dump.len());
        println!("orig cursor {:?}  replay cursor {:?}", orig.cursor, re.cursor);
        println!("orig scrollback {}  replay scrollback {}", a.scrollback_rows(), b.scrollback_rows());
        println!("{:-<100}", "");
        for r in 0..24u16 {
            let (o, n) = (orig.row_text(r), re.row_text(r));
            let mark = if o == n { "  " } else { "!!" };
            println!("{mark} {r:>2} orig  {:.66}", o);
            if o != n {
                println!("      {r:>2} repl  {:.66}", n);
            }
        }
    }
    #[cfg(not(feature = "ghostty"))]
    println!("build with --features ghostty");
}
