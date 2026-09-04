//! Geometry readback — the evidence for the silent C ABI mismatch.
//!
//! Against a ghostty commit that matches the published bindings this prints
//! `rows()=24`. Against ghostty `main` — which replaced
//! `ghostty_terminal_new(alloc, out, GhosttyTerminalOptions)` with
//! `ghostty_terminal_new(alloc, out, uint16_t cols, uint16_t rows)` — the same
//! bindings compile, run, and print `rows()=10000` (the `max_scrollback` field
//! reinterpreted). A C ABI carries no version check, so nothing warns.
#[cfg(feature = "ghostty")]
fn main() {
    use libghostty_vt::{Terminal, TerminalOptions};
    for (rows, cols, sb) in [(24u16, 80u16, 10_000usize), (6, 60, 0), (24, 80, 100)] {
        let mut t = Terminal::new(TerminalOptions { cols, rows, max_scrollback: sb }).unwrap();
        t.vt_write(b"hello\r\nworld\r\n");
        println!(
            "asked rows={rows} cols={cols} sb={sb} -> rows()={:?} cols()={:?} total_rows()={:?} scrollback_rows()={:?} cursor=({:?},{:?})",
            t.rows(), t.cols(), t.total_rows(), t.scrollback_rows(), t.cursor_y(), t.cursor_x()
        );
    }
}
#[cfg(not(feature = "ghostty"))]
fn main() { eprintln!("build with --features ghostty"); }
