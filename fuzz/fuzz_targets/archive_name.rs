//! Coverage-guided fuzz target for archive member-name normalization.
//!
//! Every name in a downloaded archive is attacker-controlled, and this
//! normalizer is what stands between one of them and a write outside the mount.
//! The target asserts it never panics and never yields a path that could escape.
//! Run on demand (needs nightly + cargo-fuzz):
//!
//!   cargo +nightly fuzz run archive_name
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Lossy rather than a UTF-8 gate: real archives carry names in CP437, in a
    // local codepage, or simply mangled, and those are exactly the inputs worth
    // reaching.
    spyc::fuzz::normalize_archive_name(&String::from_utf8_lossy(data));
});
