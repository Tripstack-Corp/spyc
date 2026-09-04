//! What does libghostty's Formatter actually emit, and what do the option
//! toggles cost? Written to check whether the rehydration score in
//! `rehydrate.rs` is measuring the engine or measuring my configuration.
#[cfg(feature = "ghostty")]
fn main() {
    use libghostty_vt::fmt::{Format, Formatter, FormatterOptions};
    use libghostty_vt::{Terminal, TerminalOptions};

    let mk = || {
        let mut t = Terminal::new(TerminalOptions { cols: 20, rows: 4, max_scrollback: 1000 }).unwrap();
        t.vt_write(b"\x1b[31mred\x1b[0m line\r\nsecond\r\n");
        t
    };

    let variants: &[(&str, fn(FormatterOptions<'static, 'static>) -> FormatterOptions<'static, 'static>)] = &[
        ("plain",            |o| o.with_format(Format::Plain)),
        ("vt bare",          |o| o.with_format(Format::Vt)),
        ("vt +style",        |o| o.with_format(Format::Vt).with_style(true)),
        ("vt +cursor",       |o| o.with_format(Format::Vt).with_cursor(true)),
        ("vt +modes",        |o| o.with_format(Format::Vt).with_modes(true)),
        ("vt +palette",      |o| o.with_format(Format::Vt).with_palette(true)),
        ("vt +tabstops",     |o| o.with_format(Format::Vt).with_tabstops(true)),
        ("vt +scrollregion", |o| o.with_format(Format::Vt).with_scrolling_region(true)),
        ("vt +charsets",     |o| o.with_format(Format::Vt).with_charsets(true)),
        ("vt +kittykbd",     |o| o.with_format(Format::Vt).with_kitty_keyboard(true)),
        ("vt +hyperlink",    |o| o.with_format(Format::Vt).with_hyperlink(true)),
        ("vt style+cursor",  |o| o.with_format(Format::Vt).with_style(true).with_cursor(true)),
        ("vt +trim",         |o| o.with_format(Format::Vt).with_style(true).with_cursor(true).with_trim(true)),
    ];

    println!("{:<18} {:>7}  first 90 bytes", "options", "len");
    println!("{:-<110}", "");
    for (label, f) in variants {
        let t = mk();
        let opts = f(FormatterOptions::new());
        let mut fm = Formatter::new(&t, opts).unwrap();
        let len = fm.format_len().unwrap();
        let mut buf = vec![0u8; len];
        let n = fm.format_buf(&mut buf).unwrap();
        buf.truncate(n);
        let show: String = String::from_utf8_lossy(&buf[..buf.len().min(90)])
            .escape_debug()
            .collect();
        println!("{label:<18} {n:>7}  {show}");
    }
}
#[cfg(not(feature = "ghostty"))]
fn main() {}
