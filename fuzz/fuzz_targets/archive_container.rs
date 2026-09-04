//! Coverage-guided fuzz target for the archive container parsers.
//!
//! `archive_name` covers one member name; this covers the code that eats bytes —
//! zip central directories, tar headers, the gz/zst streams — and the extraction
//! they feed. An archive is attacker-controlled input that spyc opens on a single
//! `Enter`, so the properties asserted are that no container panics the parser,
//! that nothing a mount writes lands outside the staging root, and that a member
//! read under a byte ceiling respects it — a header may understate its size, and
//! a cap that trusts the declaration hands back the real stream.
//!
//! The first byte of each input selects the container flavour, so one corpus
//! exercises zip, tar, tar.gz and tar.zst. Run on demand (needs nightly +
//! cargo-fuzz):
//!
//!   cargo +nightly fuzz run archive_container
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    spyc::fuzz::archive_container(data);
});
