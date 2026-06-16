//! Coverage-guided fuzz target for the keymap-DSL line parser.
//!
//! Feeds arbitrary bytes (as UTF-8) to `config::dsl::parse` via the
//! `spyc::fuzz` facade and asserts only that it never panics — it must return
//! `Ok` / `Ok(None)` / `Err` for *everything*, never an unwrap / slice / index
//! panic. Run on demand (needs nightly + cargo-fuzz):
//!
//!   cargo +nightly fuzz run dsl_parse
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(line) = std::str::from_utf8(data) {
        spyc::fuzz::parse_keymap_line(line);
    }
});
