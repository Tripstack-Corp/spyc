//! The corpus.
//!
//! Two halves. **Synthetic** cases live here as code so a reviewer can read the
//! exact bytes being asserted on instead of hexdumping a blob; each one targets
//! a named capability. **Captured** cases are real PTY streams in `fixtures/`,
//! produced by `src/bin/capture.rs` (see the README for the exact commands) —
//! those carry the messy interleaving no hand-written case reproduces.

/// One synthetic case: a name, the bytes, and what it is meant to exercise.
pub struct Synthetic {
    pub name: &'static str,
    pub what: &'static str,
    pub bytes: &'static [u8],
    /// Geometry the case is meaningful at.
    pub size: (u16, u16),
}

pub const SYNTHETIC: &[Synthetic] = &[
    Synthetic {
        name: "sgr-basic",
        what: "16-colour SGR, bold/italic/underline/reverse, and reset",
        bytes: b"\x1b[31mred\x1b[0m \x1b[1mbold\x1b[22m \x1b[3mital\x1b[23m \
\x1b[4munder\x1b[24m \x1b[7mrev\x1b[27m \x1b[42;97mgreen-bg\x1b[0m\r\n",
        size: (6, 60),
    },
    Synthetic {
        name: "sgr-truecolor",
        what: "24-bit fg/bg — the path spyc's colour_depth degrade consumes",
        bytes: b"\x1b[38;2;255;100;0mrgb-fg\x1b[48;2;0;40;80mrgb-bg\x1b[0m\r\n\
\x1b[38;5;208m256-fg\x1b[48;5;22m256-bg\x1b[0m\r\n",
        size: (6, 60),
    },
    Synthetic {
        name: "sgr-dim-strike",
        what: "SGR 2 (dim) and SGR 9 (strikethrough) — attributes spyc's \
               `cell_style` does not read",
        bytes: b"\x1b[2mdim\x1b[22m \x1b[9mstrike\x1b[29m normal\r\n",
        size: (4, 40),
    },
    Synthetic {
        name: "wide-cjk",
        what: "double-width CJK, including one landing on the last column",
        bytes: "\u{3042}\u{3044}\u{3046} ascii\r\nxxxxxxxx\u{3042}\r\n".as_bytes(),
        size: (6, 10),
    },
    Synthetic {
        name: "zwj-emoji",
        what: "ZWJ family sequence + VS16 presentation selector — one grapheme \
               cluster spanning many codepoints",
        bytes: "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}|\u{2764}\u{FE0F}|\u{1F3F4}\u{E0067}\u{E0062}\u{E0073}\u{E0063}\u{E0074}\u{E007F}|end\r\n".as_bytes(),
        size: (4, 40),
    },
    Synthetic {
        name: "combining-marks",
        what: "combining diacritics stacked on one base character",
        bytes: "e\u{0301}\u{0302}\u{0303}base a\u{0308}o\u{030A}\r\n".as_bytes(),
        size: (4, 40),
    },
    Synthetic {
        name: "osc8-hyperlink",
        what: "OSC 8 hyperlinks — vt100 has no cell-level URI concept at all",
        bytes: b"plain \x1b]8;;https://example.com/a\x1b\\linked\x1b]8;;\x1b\\ plain\r\n",
        size: (4, 40),
    },
    Synthetic {
        name: "kitty-graphics-apc",
        what: "kitty graphics APC frame — must be consumed, not printed as text",
        bytes: b"before\x1b_Ga=T,f=24,s=2,v=2,m=0;AAECAwQFBgcICQoLDA0ODw==\x1b\\after\r\n",
        size: (4, 40),
    },
    Synthetic {
        name: "bracketed-paste-mode",
        what: "DECSET 2004 on/off — gates spyc's paste wrapping",
        bytes: b"\x1b[?2004htext\x1b[?2004l",
        size: (4, 40),
    },
    Synthetic {
        name: "kitty-keyboard-negotiation",
        what: "kitty keyboard protocol push/pop + query",
        bytes: b"\x1b[>1u\x1b[=5;1u\x1b[?u\x1b[<u",
        size: (4, 40),
    },
    Synthetic {
        name: "scroll-region-decstbm",
        what: "DECSTBM scroll region — the shape codex uses, which is why \
               spyc's vt100 scrollback stays empty for it",
        bytes: b"\x1b[2J\x1b[H\x1b[3;6rheader\r\n\x1b[3H\
one\r\ntwo\r\nthree\r\nfour\r\nfive\r\nsix\r\nseven\r\neight\r\n",
        size: (8, 20),
    },
    Synthetic {
        name: "alt-screen-roundtrip",
        what: "the nvim shape from Cargo.toml's panic comment: enter alt screen, \
               set a scroll region, save cursor, leave",
        bytes: b"main line one\r\nmain line two\r\n\x1b[?1049h\x1b[2J\x1b[H\
\x1b[1;10ralt content\r\n\x1b[8;40H\x1b7\x1b[?1049l\x1b8tail\r\n",
        size: (12, 60),
    },
    Synthetic {
        name: "decsc-resize-decrc",
        what: "DECSC, shrink below the saved row, DECRC — issue #13's class",
        bytes: b"foo\x1b[20;70Hbar\x1b7",
        size: (24, 80),
    },
    Synthetic {
        name: "soft-wrap-long-line",
        what: "a line longer than the row, so `row_wrapped` must distinguish \
               a soft wrap from a hard newline",
        bytes: b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\r\nbbb\r\n",
        size: (6, 10),
    },
    Synthetic {
        name: "erase-and-insert",
        what: "ICH/DCH/ECH/EL/ED against a populated grid",
        bytes: b"abcdefghij\r\n\x1b[1;3H\x1b[2@\x1b[1;7H\x1b[2P\x1b[2;1H\
klmnopqrst\x1b[2;5H\x1b[3X\x1b[2;8H\x1b[0K",
        size: (6, 12),
    },
    Synthetic {
        name: "tabs-and-cr",
        what: "HTS/TBC custom tab stops plus bare CR overwrite",
        bytes: b"\x1b[3g\x1b[1;5H\x1bH\x1b[1;9H\x1bH\x1b[1;1H\
a\tb\tc\rZ\r\n",
        size: (4, 20),
    },
    Synthetic {
        name: "charset-decgraphics",
        what: "DEC special graphics charset (box drawing via SCS)",
        bytes: b"\x1b(0lqqqk\r\nx  x\r\nmqqqj\x1b(B\r\n",
        size: (5, 20),
    },
    Synthetic {
        name: "cursor-style-decscusr",
        what: "DECSCUSR cursor shapes + DECTCEM hide/show",
        bytes: b"\x1b[?25l\x1b[3 qhidden\x1b[?25h\x1b[1 qshown",
        size: (4, 20),
    },
];

/// Load every captured fixture from `fixtures/*.bin`, sorted by name.
pub fn captured() -> Vec<(String, Vec<u8>)> {
    let dir = std::path::Path::new("fixtures");
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return out;
    };
    let mut paths: Vec<_> = rd
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "bin"))
        .collect();
    paths.sort();
    for p in paths {
        if let Ok(bytes) = std::fs::read(&p) {
            let name = p
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("?")
                .to_string();
            out.push((name, bytes));
        }
    }
    out
}
