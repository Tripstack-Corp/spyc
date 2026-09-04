//! Coverage-guided fuzz target for the pane's byte-processing and recovery path.
//!
//! Promoted from the VT-engine spike's differential fuzz generator, which found
//! the incumbent's live panic classes and an off-by-one in wezterm-term. The
//! generator is now `spyc::fuzz_support::escape_stream`, driven by the fuzzer's
//! own bytes so libFuzzer's coverage feedback steers the SHAPE of the escape
//! stream rather than an opaque PRNG seed.
//!
//! **The property is recovery, not absence of panics.** `pane::parser_worker`
//! wraps `process()` in `catch_unwind`, rebuilds a torn parser and clears the
//! mutex poison, precisely because the current engine panics on some valid
//! input at reachable geometries. This asserts what production depends on and
//! what holds for any engine: after any byte stream the terminal is still there
//! and still answers questions. It is also the only thing that exercises that
//! recovery branch, which by construction no ordinary test reaches.
//!
//! Run on demand (needs nightly + cargo-fuzz):
//!
//!   cargo +nightly fuzz run pane_engine
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    spyc::fuzz::pane_engine(data);
});
