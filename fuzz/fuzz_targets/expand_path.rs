//! Coverage-guided fuzz target for path expansion (`~` / `$VAR` / `${VAR}`).
//! Run with:  cargo +nightly fuzz run expand_path
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(input) = std::str::from_utf8(data) {
        spyc::fuzz::expand_path(input);
    }
});
