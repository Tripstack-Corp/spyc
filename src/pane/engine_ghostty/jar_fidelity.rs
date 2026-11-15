//! What a restored snapshot brings back (decision I, R17b, R17c). Each
//! fidelity case sets up one piece of terminal state, restores a new engine
//! from each snapshot form, and compares what spyc and the program can
//! observe, before and after the same continuation bytes. The binary form is
//! the control: it must come back exact everywhere, or the comparison itself
//! is noisy.

use std::fmt::Write as _;

use super::{Data, GhosttyEngine, ffi, jar_snapshot::snapshot_decodes};
use crate::pane::engine::{Engine, TerminalScreen};

const ROWS: u16 = 24;
const COLS: u16 = 80;
const HISTORY: usize = 10_000;

/// Queries whose answers expose state no reader does: modes by DECRQM, the
/// cursor position, and DECRQSS for the pen, the scrolling region and the
/// cursor shape.
const PROBES: &[u8] = b"\x1b[?1$p\x1b[?6$p\x1b[?7$p\x1b[?25$p\x1b[?47$p\x1b[?66$p\x1b[?1000$p\x1b[?1002$p\x1b[?1004$p\x1b[?1006$p\x1b[?1049$p\x1b[?2004$p\x1b[4$p\x1b[20$p\x1b[6n\x1bP$qm\x1b\\\x1bP$qr\x1b\\\x1bP$q q\x1b\\";

/// Everything observable, one fact per line, so a difference names itself.
fn observe(e: &mut GhosttyEngine) -> Vec<String> {
    let s = &e.inner;
    let (rows, cols) = s.size();
    let mut out = vec![
        format!("size {rows}x{cols}"),
        format!("cursor {:?}", s.cursor_position()),
        format!(
            "pending wrap {:?}",
            s.get_bool(Data::GHOSTTY_TERMINAL_DATA_CURSOR_PENDING_WRAP)
        ),
        format!("hidden cursor {}", s.hide_cursor()),
        format!("alternate screen {}", s.alternate_screen()),
        format!("bracketed paste {}", s.bracketed_paste()),
        format!("application cursor {}", s.application_cursor()),
        format!("mouse {:?}", s.mouse_protocol()),
        format!("history rows {}", s.history_rows_raw()),
        format!("title {:?}", s.title()),
        format!("kitty keyboard flags {}", kitty_flags(e)),
    ];
    for row in 0..rows {
        let mut line = String::new();
        let mut styles = Vec::new();
        for col in 0..cols {
            let mut t = String::new();
            s.cell_text(row, col, &mut t);
            line.push_str(if t.is_empty() { " " } else { &t });
            styles.push(s.cell_style(row, col));
        }
        out.push(format!(
            "row {row:2} wrapped={} {:?}",
            s.row_wrapped(row),
            line.trim_end()
        ));
        out.push(format!("row {row:2} styles {styles:?}"));
    }
    out.push(format!(
        "history text {:?}",
        e.encode_portable().map(|v| text_only(&v))
    ));
    e.process(PROBES);
    let replies = e.take_replies();
    out.push(format!("replies {:?}", String::from_utf8_lossy(&replies)));
    out
}

/// The kitty keyboard flags in force, read from the terminal: spyc's engine
/// doesn't answer the program's `CSI ? u`, so the probes can't see them.
fn kitty_flags(e: &GhosttyEngine) -> u8 {
    let mut v: u8 = 0;
    // SAFETY: `t` is live, and KITTY_KEYBOARD_FLAGS writes a `uint8_t`.
    let ok = unsafe {
        ffi::ghostty_terminal_get(
            e.inner.t,
            Data::GHOSTTY_TERMINAL_DATA_KITTY_KEYBOARD_FLAGS,
            (&raw mut v).cast(),
        )
    };
    assert_eq!(ok, spyc_vt_sys::SUCCESS, "the kitty flags read");
    v
}

/// The emit with escape sequences dropped: the history's characters alone.
fn text_only(v: &[u8]) -> String {
    String::from_utf8_lossy(&strip_ansi_escapes::strip(v)).into_owned()
}

fn fresh() -> GhosttyEngine {
    <GhosttyEngine as Engine>::new(ROWS, COLS, HISTORY)
}

/// The lines of `a` and `b` that differ, side by side, at most `max`.
fn diff(a: &[String], b: &[String], max: usize) -> Vec<String> {
    a.iter()
        .zip(b)
        .filter(|(x, y)| x != y)
        .take(max)
        .map(|(x, y)| format!("      was {}\n      now {}", clip(x), clip(y)))
        .collect()
}

fn clip(s: &str) -> String {
    if s.chars().count() > 150 {
        format!("{}…", s.chars().take(150).collect::<String>())
    } else {
        s.to_owned()
    }
}

fn history(n: usize) -> String {
    (0..n).fold(String::new(), |mut s, i| {
        let _ = write!(
            s,
            "\x1b[3{}mline {i}\x1b[0m of the shell's history\r\n",
            i % 7 + 1
        );
        s
    })
}

/// `(name, setup, continuation)`.
fn cases() -> Vec<(&'static str, String, String)> {
    let full_row = "x".repeat(COLS as usize);
    vec![
        ("plain history", history(300), "next\r\n".into()),
        ("pen left on", "\x1b[1;4;31;48;5;22mstill on".into(), " more\r\nand more".into()),
        ("pending wrap", format!("{}\x1b[{ROWS};1H{full_row}", history(5)), "X".into()),
        ("saved cursor", "\x1b[5;10Hab\x1b7\x1b[20;1Hcd".into(), "\x1b8Z".into()),
        ("saved cursor, pen and a pending wrap", format!("\x1b[5;10H\x1b[1;35m\x1b7\x1b[0m\x1b[{ROWS};1H{}", "x".repeat(COLS as usize)), "Y\x1b8Z".into()),
        ("saved cursor in the alternate screen", "\x1b[?1049h\x1b[3;3H\x1b[4m\x1b7\x1b[0m\x1b[10;1Hbody".into(), "\x1b8Z\x1b[?1049lprimary".into()),
        ("scroll region, origin mode", "\x1b[5;10r\x1b[?6h\x1b[1;1Hin region".into(), "\x1b[6;1Hlast\n\n\n\n\nscrolled".into()),
        ("G0 line drawing left on", "\x1b(0lqk\x1b(B normal \x1b(0".into(), "lqqk".into()),
        ("G1 line drawing, shifted out", "\x1b)0\x0e".into(), "lqk\x0fabc".into()),
        ("custom tab stops", "\x1b[3g\x1b[1;5H\x1bH\x1b[1;30H\x1bH\r".into(), "\tA\tB".into()),
        ("insert mode", "abcdef\x1b[1;3H\x1b[4h".into(), "XY".into()),
        ("autowrap off", "\x1b[?7l".into(), "y".repeat(100)),
        ("modes: paste, cursor keys, mouse, focus, hidden cursor", "\x1b[?2004h\x1b[?1h\x1b[?1002h\x1b[?1006h\x1b[?1004h\x1b[?25l\x1b=".into(), String::new()),
        ("kitty keyboard flags, pushed twice", "\x1b[>1u\x1b[>5u".into(), "\x1b[<u".into()),
        ("kitty keyboard flags, pushed once", "\x1b[>1u".into(), "\x1b[<u".into()),
        ("a full-screen agent over history, cursor saved by entering", format!("{}\x1b[5;7H\x1b[?1049h\x1b[?1002h\x1b[?2004h\x1b[>1u\x1b[2J\x1b[3;1H\x1b[1m> prompt", history(300)), "\x1b[<u\x1b[?2004l\x1b[?1002l\x1b[?1049lnext".into()),
        ("vim's bar cursor in the alternate screen", "\x1b[?1049h\x1b[6 qinsert".into(), "\x1b[?1049l".into()),
        ("cursor shape", "\x1b[5 q".into(), String::new()),
        ("alternate screen over history", format!("{}\x1b[?1049h\x1b[2J\x1b[Hfull-screen program", history(300)), "\x1b[?1049lback on the shell\r\n".into()),
        ("hyperlinks", "\x1b]8;;https://example.com/a\x1b\\link\x1b]8;;\x1b\\ after\r\n\x1b]8;;https://example.com/open\x1b\\".into(), "linked?".into()),
        ("graphemes and wide", "family 👨\u{200d}👩\u{200d}👧 e\u{301} 漢字 ❤\u{fe0f}\r\n".into(), format!("\x1b[1;{COLS}H漢")),
        ("underline styles, strike, overline", "\x1b[4:3;58;2;255;0;0mcurly\x1b[0m \x1b[9mstrike\x1b[0m \x1b[53mover\x1b[0m \x1b[5mblink\x1b[8mhidden\x1b[0m".into(), String::new()),
        ("protected cells", "\x1b[1\"qkeep\x1b[0\"q erase".into(), "\x1b[1;1H\x1b[?2K".into()),
        ("title", "\x1b]2;a title\x07".into(), String::new()),
        ("palette and default colours set by the program", "\x1b]4;1;rgb:12/34/56\x1b\\\x1b]10;rgb:aa/bb/cc\x1b\\\x1b]11;rgb:01/02/03\x1b\\\x1b[31mred".into(), String::new()),
        ("long wrapped lines in history", (0..80).fold(String::new(), |mut s, i| {
            let _ = write!(s, "{i} {}\r\n", "w".repeat(250));
            s
        }), "tail\r\n".into()),
    ]
}

/// R17c: the binary form comes back exact in every case, and the portable form
/// in all but the two it is known to lose, neither of which spyc reads: kitty
/// keyboard flags below the top of their stack (spyc never encodes keys with
/// them) and DECSCA protection.
#[test]
fn both_snapshot_forms_bring_back_what_the_program_left() {
    let mut lossy = Vec::new();
    let mut reports = Vec::new();
    for (name, setup, cont) in cases() {
        let mut orig = fresh();
        orig.process(setup.as_bytes());
        let _ = orig.take_replies();
        let bin = orig.encode_snapshot().expect("binary");
        let port = GhosttyEngine::portable_from_binary(&bin, HISTORY).expect("portable");
        let mut via_port = fresh();
        via_port.restore_portable(&port, ROWS, COLS);
        let mut via_bin = fresh();
        assert!(
            via_bin.restore_snapshot(&bin),
            "{name}: the binary form decodes"
        );

        let mut report = Vec::new();
        for (when, extra) in [("restored", ""), ("after the continuation", cont.as_str())] {
            for e in [&mut orig, &mut via_port, &mut via_bin] {
                e.process(extra.as_bytes());
                let _ = e.take_replies();
            }
            let o = observe(&mut orig);
            let b = diff(&o, &observe(&mut via_bin), 3);
            let p = diff(&o, &observe(&mut via_port), 4);
            assert!(
                b.is_empty(),
                "{name}: the binary control differs {when}:\n{}",
                b.join("\n")
            );
            if !p.is_empty() {
                report.push(format!("    {when}:\n{}", p.join("\n")));
            }
        }
        if !report.is_empty() {
            lossy.push(name);
            reports.push(format!("{name}\n{}", report.join("\n")));
        }
    }
    assert_eq!(
        lossy,
        ["kitty keyboard flags, pushed twice", "protected cells"],
        "the portable form lost more than it is known to:\n{}",
        reports.join("\n")
    );
}

/// A snapshot comes back at the size it was taken at, whatever the size of the
/// engine it is restored into, and with spyc's own settings on the restored
/// terminal: its replies reach the pane, a grapheme cluster takes one cell
/// pair, and the history is held to the pane's budget.
#[test]
fn a_restored_engine_keeps_spycs_settings_at_the_snapshots_size() {
    let mut orig = <GhosttyEngine as Engine>::new(30, 100, HISTORY);
    orig.process(history(50).as_bytes());
    let bin = orig.encode_snapshot().expect("binary");

    let mut e = <GhosttyEngine as Engine>::new(ROWS, COLS, 500);
    assert!(e.restore_snapshot(&bin));
    assert_eq!(e.inner.size(), (30, 100), "the size it was taken at");

    e.process(b"\x1b[c");
    assert!(
        !e.take_replies().is_empty(),
        "a query after the restore is answered to the pane"
    );

    e.process("\r\n\u{1f1e8}\u{1f1e6}|".as_bytes());
    let (row, _) = e.inner.cursor_position();
    let mut bar = String::new();
    e.inner.cell_text(row, 2, &mut bar);
    assert_eq!(bar, "|", "a flag is one cluster of two cells");

    // The library keeps whole pages, so somewhat more than the budget.
    e.process(history(5_000).as_bytes());
    let kept = e.inner.history_rows_raw();
    assert!(kept < 1_000, "history held to the pane's budget: {kept}");
}

/// A blob this build can't decode, another build's or a damaged one, is
/// refused, and the engine keeps what it had.
#[test]
fn a_snapshot_this_build_cannot_decode_changes_nothing() {
    let mut orig = fresh();
    orig.process(b"the pane's own screen");
    let bin = orig.encode_snapshot().expect("binary");
    let before = observe(&mut orig);

    let mut versioned = bin.clone();
    versioned[..8].iter_mut().for_each(|b| *b ^= 0xff);
    for blob in [
        &bin[..bin.len() / 2],
        &versioned[..],
        b"",
        b"not a snapshot",
    ] {
        assert!(!snapshot_decodes(blob));
        assert!(!orig.restore_snapshot(blob));
        assert!(GhosttyEngine::portable_from_binary(blob, HISTORY).is_none());
        assert_eq!(observe(&mut orig), before);
    }
}

/// A portable restore replays what the program wrote long ago; a query in it
/// is answered to no one.
#[test]
fn a_portable_restore_answers_none_of_the_queries_it_repeats() {
    let mut e = fresh();
    e.restore_portable(b"before\x1b[c\x1b[6nafter", ROWS, COLS);
    assert_eq!(e.take_replies(), b"");
    e.process(b"\x1b[c");
    assert!(!e.take_replies().is_empty(), "a query after it is answered");
}
