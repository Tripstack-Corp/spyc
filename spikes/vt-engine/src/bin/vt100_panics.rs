//! Adversarial probe: which of vt100 0.16.2's reported panic classes are live?
//!
//! Written before any conclusion about the incumbent. Each case runs under
//! `catch_unwind` — the same net `pane::Pane::process_bytes_safe` puts around
//! the parser in production — so one panic doesn't end the run.
//!
//! Every case records whether the geometry it needs is REACHABLE in spyc:
//! `Pane::resize` and `pane_spawn_size` clamp both dimensions with `.max(1)`,
//! so rows/cols of 0 are unreachable and rows/cols of exactly 1 are not.

use std::panic::{AssertUnwindSafe, catch_unwind};

/// `spyc` reachability of the geometry a case needs.
#[derive(Clone, Copy)]
enum Reach {
    /// `.max(1)` in `Pane::resize` / `pane_spawn_size` permits this.
    Live,
    /// `.max(1)` forbids a zero dimension.
    ClampedOut,
}

impl Reach {
    fn label(self) -> &'static str {
        match self {
            Self::Live => "REACHABLE in spyc (.max(1) permits it)",
            Self::ClampedOut => "unreachable (clamped by .max(1))",
        }
    }
}

struct Case {
    id: &'static str,
    upstream: &'static str,
    reach: Reach,
    run: fn(),
}

fn main() {
    // Cases are the upstream reports, transcribed from the issue text, plus the
    // one the Cargo.toml comment describes (DECRC-after-resize).
    let cases: &[Case] = &[
        Case {
            id: "zero-rows-process",
            upstream: "issue #37 (taylordotfish)",
            reach: Reach::ClampedOut,
            run: || {
                let mut p = vt100::Parser::new(0, 10, 0);
                p.process(b"a");
            },
        },
        Case {
            id: "one-row-wrap",
            upstream: "issue #37 (taylordotfish)",
            reach: Reach::Live,
            run: || {
                let mut p = vt100::Parser::new(1, 10, 0);
                p.process(b"aaaaaaaaaaa");
            },
        },
        Case {
            id: "one-col-wide-char",
            upstream: "issue #37 (taylordotfish)",
            reach: Reach::Live,
            run: || {
                let mut p = vt100::Parser::new(10, 1, 0);
                p.process("\u{3042}".as_bytes());
            },
        },
        Case {
            id: "wide-char-split-by-shrink",
            upstream: "issue #37 / #28 (same Row::resize root cause)",
            reach: Reach::Live,
            run: || {
                let mut p = vt100::Parser::new(3, 10, 0);
                p.process("aaaaaaaa\u{3042}".as_bytes());
                p.screen_mut().set_size(3, 9); // splits the wide char
                p.process(b"\x1b[1;9Hz"); // CUP onto the dangling cell + write
            },
        },
        Case {
            id: "wide-char-split-then-erase",
            upstream: "issue #28 (Row::clear_wide)",
            reach: Reach::Live,
            run: || {
                let mut p = vt100::Parser::new(3, 10, 0);
                p.process("aaaaaaaa\u{3042}".as_bytes());
                p.screen_mut().set_size(3, 9);
                p.process(b"\x1b[1;9H\x1b[1X"); // ECH over the dangling cell
            },
        },
        Case {
            id: "decrc-after-resize",
            upstream: "issue #13 — the class Cargo.toml's `panic = unwind` guards",
            reach: Reach::Live,
            run: || {
                let mut p = vt100::Parser::new(24, 80, 0);
                p.process(b"foo\x1b[20;70Hbar\x1b7"); // DECSC at row 20
                p.screen_mut().set_size(15, 60); // shrink below the saved row
                p.process(b"y\x1b8z"); // DECRC into the out-of-range row
            },
        },
        Case {
            id: "scrollback-larger-than-rows",
            upstream: "issue #5 (closed) — regression probe",
            reach: Reach::Live,
            run: || {
                let mut p = vt100::Parser::new(1, 10, 100);
                for _ in 0..50 {
                    p.process(b"line\r\n");
                }
                let mut s = p.screen().clone();
                s.set_scrollback(usize::MAX);
            },
        },
    ];

    let overflow_checks = cfg!(debug_assertions);
    println!(
        "vt100 {} | overflow-checks(proxy: debug_assertions) = {overflow_checks}",
        env!("VT100_VERSION")
    );
    println!("{:-<100}", "");

    let mut panicked = 0usize;
    for c in cases {
        // Silence the default hook so the report is one line per case.
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let res = catch_unwind(AssertUnwindSafe(c.run));
        std::panic::set_hook(prev);

        let verdict = match &res {
            Ok(()) => "ok     ".to_string(),
            Err(e) => {
                panicked += 1;
                let msg = e
                    .downcast_ref::<String>()
                    .map(String::as_str)
                    .or_else(|| e.downcast_ref::<&str>().copied())
                    .unwrap_or("<non-string payload>");
                format!("PANIC  {msg}")
            }
        };
        println!("{:<28} {verdict}", c.id);
        println!("{:<28}   {} | {}", "", c.reach.label(), c.upstream);
    }
    println!("{:-<100}", "");
    println!("{panicked}/{} cases panicked", cases.len());
}
