//! Focused ghostty round trip: does the cursor survive, and which option set
//! gives the best reconstruction? Checks the `rehydrate.rs` numbers rather
//! than trusting them.
#[cfg(feature = "ghostty")]
fn main() {
    use libghostty_vt::fmt::{Format, Formatter, FormatterOptions};
    use libghostty_vt::{Terminal, TerminalOptions};

    let feed: &[u8] = b"\x1b[31mred\x1b[0m line\r\nsecond\r\nthird\r\n";
    let mk = || {
        let mut t = Terminal::new(TerminalOptions { cols: 20, rows: 6, max_scrollback: 1000 }).unwrap();
        t.vt_write(feed);
        t
    };

    type Cfg = (&'static str, fn(FormatterOptions<'static, 'static>) -> FormatterOptions<'static, 'static>);
    let cfgs: &[Cfg] = &[
        ("vt+style+cursor",      |o| o.with_format(Format::Vt).with_style(true).with_cursor(true)),
        ("+modes+region+tabs",   |o| o.with_format(Format::Vt).with_style(true).with_cursor(true)
                                      .with_modes(true).with_scrolling_region(true).with_tabstops(true)),
        ("everything (mine)",    |o| o.with_format(Format::Vt).with_palette(true).with_modes(true)
                                      .with_scrolling_region(true).with_tabstops(true).with_cursor(true)
                                      .with_style(true).with_hyperlink(true).with_protection(true)
                                      .with_kitty_keyboard(true).with_charsets(true)),
    ];

    let orig = mk();
    println!("original: cursor=({:?},{:?}) rows={:?} total={:?}",
        orig.cursor_y(), orig.cursor_x(), orig.rows(), orig.total_rows());
    println!("{:-<96}", "");
    for (label, f) in cfgs {
        let t = mk();
        let mut fm = Formatter::new(&t, f(FormatterOptions::new())).unwrap();
        let len = fm.format_len().unwrap();
        let mut buf = vec![0u8; len];
        let n = fm.format_buf(&mut buf).unwrap();
        buf.truncate(n);

        let mut fresh = Terminal::new(TerminalOptions { cols: 20, rows: 6, max_scrollback: 1000 }).unwrap();
        fresh.vt_write(&buf);
        println!(
            "{label:<20} dump={n:>6}B  replay cursor=({:?},{:?})  tail={:?}",
            fresh.cursor_y(), fresh.cursor_x(),
            String::from_utf8_lossy(&buf[buf.len().saturating_sub(24)..]).escape_debug().to_string()
        );
    }
}
#[cfg(not(feature = "ghostty"))]
fn main() {}
