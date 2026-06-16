//! Coverage-guided fuzz target for the `%`-template expander (the `unix CMD`
//! substitution). Run with:  cargo +nightly fuzz run expand_percent
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(template) = std::str::from_utf8(data) {
        spyc::fuzz::expand_percent(template);
    }
});
