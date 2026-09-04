//! Input shaping for the `pane_engine` fuzz target.
//!
//! Promoted from the VT-engine spike's differential fuzz generator
//! (`spikes/vt-engine/src/bin/fuzz_diff.rs`), which found the incumbent's live
//! panic classes and wezterm's off-by-one. Uniform random bytes are almost all
//! printable-or-ignored and never reach the interesting state transitions; a
//! generator that emits real CSI / OSC / APC / SGR / charset shapes does.
//!
//! The spike drove it from a seeded PRNG so findings reproduced from a seed.
//! Here the fuzzer's own bytes are the script instead: consuming them directly
//! means libFuzzer's coverage feedback steers the *shape* of the escape stream,
//! where seeding a PRNG would leave it steering an opaque hash. Reproduction
//! comes from cargo-fuzz's artifact file rather than a seed.

/// Take one byte off the front, cycling when the script runs out so a short
/// input still produces a stream rather than stopping at its first choice.
struct Script<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Script<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    fn byte(&mut self) -> u8 {
        if self.bytes.is_empty() {
            return 0;
        }
        let b = self.bytes[self.pos % self.bytes.len()];
        self.pos += 1;
        b
    }

    fn below(&mut self, n: u8) -> u8 {
        if n == 0 { 0 } else { self.byte() % n }
    }

    /// True once the script has been read through roughly twice — the cap that
    /// stops a one-byte input generating forever.
    const fn spent(&self) -> bool {
        self.bytes.is_empty() || self.pos >= self.bytes.len() * 2
    }
}

/// Build a terminal-shaped byte stream from `script`.
///
/// Bounded: at most `MAX_LEN` bytes, and it stops once the script is spent, so
/// a fuzz iteration cannot run away.
pub fn escape_stream(script: &[u8]) -> Vec<u8> {
    /// Enough to cover a scrollback budget's worth of transitions without
    /// making a single iteration slow.
    const MAX_LEN: usize = 8 * 1024;

    /// CSI finals, including the ones whose handlers touch geometry:
    /// `r` DECSTBM, `H`/`f` CUP, `X`/`P`/`@`/`L`/`M` the edit family.
    const CSI_FINAL: &[u8] = b"ABCDEFGHJKLMPSTXZdfghlmnrstu@`";
    /// Private modes spyc's own predicates read, plus the mouse family.
    const MODES: &[&[u8]] = &[
        b"1", b"7", b"12", b"25", b"47", b"1000", b"1002", b"1003", b"1006", b"1049", b"2004",
        b"2026",
    ];
    /// Text with awkward width: double-width, ZWJ, VS16, combining, and tag
    /// characters, which is where a fixed-size cell buffer truncates.
    const PRINTABLE: &[&str] = &[
        "a",
        "Z",
        " ",
        "~",
        "\u{3042}",
        "\u{4e2d}",
        "\u{1F600}",
        "\u{1F468}\u{200D}\u{1F469}",
        "e\u{0301}",
        "\u{2764}\u{FE0F}",
        "\u{1F3F4}\u{E0067}\u{E0062}",
    ];

    let mut s = Script::new(script);
    let mut out: Vec<u8> = Vec::new();
    while out.len() < MAX_LEN && !s.spent() {
        match s.below(12) {
            0 => {
                out.extend_from_slice(b"\x1b[");
                for i in 0..s.below(4) {
                    if i > 0 {
                        out.push(b';');
                    }
                    // Params well past the grid, since out-of-range is where
                    // the clamping bugs live.
                    let n = u16::from(s.byte()) * 4;
                    out.extend_from_slice(n.to_string().as_bytes());
                }
                let f = CSI_FINAL[s.below(CSI_FINAL.len() as u8) as usize];
                out.push(f);
            }
            1 => {
                out.extend_from_slice(b"\x1b[?");
                out.extend_from_slice(MODES[s.below(MODES.len() as u8) as usize]);
                out.push(if s.below(2) == 0 { b'h' } else { b'l' });
            }
            2 => {
                out.extend_from_slice(b"\x1b[");
                match s.below(4) {
                    0 => out.extend_from_slice(s.byte().to_string().as_bytes()),
                    1 => {
                        out.extend_from_slice(format!("38;5;{}", s.byte()).as_bytes());
                    }
                    2 => {
                        let (r, g, b) = (s.byte(), s.byte(), s.byte());
                        out.extend_from_slice(format!("48;2;{r};{g};{b}").as_bytes());
                    }
                    _ => out.push(b'0'),
                }
                out.push(b'm');
            }
            3 => {
                let p = PRINTABLE[s.below(PRINTABLE.len() as u8) as usize];
                out.extend_from_slice(p.as_bytes());
            }
            4 => out.extend_from_slice(b"\r\n"),
            5 => {
                let c = b"\r\n\t\x08\x07\x0b\x0c"[s.below(7) as usize];
                out.push(c);
            }
            6 => out.extend_from_slice(b"\x1b7"),
            7 => out.extend_from_slice(b"\x1b8"),
            8 => {
                // OSC, sometimes unterminated — a real risk, since an agent's
                // title write can split across pty reads.
                out.extend_from_slice(b"\x1b]");
                out.extend_from_slice(s.byte().to_string().as_bytes());
                out.extend_from_slice(b";payload");
                match s.below(3) {
                    0 => out.push(0x07),
                    1 => out.extend_from_slice(b"\x1b\\"),
                    _ => {}
                }
            }
            9 => {
                out.extend_from_slice(b"\x1b_Ga=T,f=24,s=1,v=1;AAAA");
                if s.below(2) == 0 {
                    out.extend_from_slice(b"\x1b\\");
                }
            }
            10 => {
                out.extend_from_slice(b"\x1b(");
                out.push(b"0BAU"[s.below(4) as usize]);
            }
            _ => out.push(s.byte()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::escape_stream;

    /// A generator that returns nothing tests nothing, and the bug would be
    /// invisible — the fuzz target would pass on every input.
    #[test]
    fn a_nonempty_script_produces_a_nonempty_stream() {
        assert!(!escape_stream(&[1, 2, 3, 4, 5, 6, 7, 8]).is_empty());
    }

    /// Bounded in both directions: an empty script stops immediately, and a
    /// short one cannot generate forever.
    #[test]
    fn generation_is_bounded() {
        assert!(
            escape_stream(&[]).is_empty(),
            "an empty script generates nothing"
        );
        for script in [vec![0u8], vec![7u8; 3], vec![255u8; 64]] {
            let out = escape_stream(&script);
            assert!(out.len() <= 8 * 1024, "ran away to {} bytes", out.len());
        }
    }

    /// The fuzz target's property, exercised in the ordinary gate rather than
    /// only in the weekly fuzz job.
    ///
    /// A target that first runs a week after it lands, on a machine nobody is
    /// watching, is a target whose first finding is a mystery. This walks a
    /// spread of scripts through the same entry point so the invariant has
    /// coverage now, and so a regression shows up in `make check` instead of in
    /// a Monday-morning artifact.
    #[test]
    fn the_pane_engine_property_holds_over_a_spread_of_scripts() {
        // Deterministic spread rather than a PRNG: this is a gate test, and a
        // gate test that fails on a different input each run is not a gate.
        let mut scripts: Vec<Vec<u8>> = Vec::new();
        for a in 0u8..=15 {
            for b in 0u8..=15 {
                // Two geometry bytes then a short script; `a`/`b` sweep the
                // geometry, including the 1x1 corner the incumbent panics at.
                scripts.push(vec![
                    a * 17,
                    b * 17,
                    a,
                    b,
                    a ^ b,
                    b.wrapping_add(a),
                    0xff,
                    0x1b,
                ]);
            }
        }
        // A few longer, denser ones.
        scripts.push((0u8..=255).collect());
        scripts.push((0u8..=255).rev().collect());
        scripts.push(vec![0x1b; 300]);
        scripts.push(vec![0; 300]);

        for (i, script) in scripts.iter().enumerate() {
            crate::fuzz::pane_engine(script);
            // Reaching here at all is the assertion — `pane_engine` panics on
            // violation. The index is for the failure message a panic prints.
            let _ = i;
        }
    }

    /// The point of the generator: it must emit escape sequences, not just
    /// text. If this ever passes trivially the target has quietly become a
    /// plain-bytes fuzzer.
    #[test]
    fn the_stream_actually_contains_escape_sequences() {
        let out = escape_stream(&(0u8..=255).collect::<Vec<_>>());
        assert!(out.contains(&0x1b), "no ESC in {} bytes", out.len());
        let csi = out.windows(2).filter(|w| w == b"\x1b[").count();
        assert!(csi > 0, "no CSI sequences generated");
    }
}
