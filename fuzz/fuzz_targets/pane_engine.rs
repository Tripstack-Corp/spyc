//! Coverage-guided fuzz target for the pane's byte-processing and recovery path.
//!
//! Promoted from the VT-engine spike's differential fuzz generator, which found
//! the incumbent's live panic classes and an off-by-one in wezterm-term. The
//! generator is now `spyc::fuzz_support::escape_stream`, driven by the fuzzer's
//! own bytes so libFuzzer's coverage feedback steers the SHAPE of the escape
//! stream rather than an opaque PRNG seed.
//!
//! **The property is that `process` never panics**, and that after any byte
//! stream the terminal still answers: geometry intact, cursor in bounds, every
//! in-bounds cell present and no cell past the last column.
//!
//! It asserted only recovery while the engine was vt100, which panics on valid
//! input at reachable geometries. `pane::parser_worker` keeps that net in
//! production — it is cheap, and the next engine's bugs are not known yet —
//! but a recovery property asserted against an engine that does not panic
//! asserts almost nothing.
//!
//! Run on demand (needs nightly + cargo-fuzz):
//!
//!   cargo +nightly fuzz run pane_engine
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    spyc::fuzz::pane_engine(data);
});
