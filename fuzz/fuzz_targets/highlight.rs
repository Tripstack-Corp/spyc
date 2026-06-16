//! Coverage-guided fuzz target for the syntax highlighter over arbitrary file
//! content. Run with:  cargo +nightly fuzz run highlight
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(content) = std::str::from_utf8(data) {
        spyc::fuzz::highlight(content);
    }
});
