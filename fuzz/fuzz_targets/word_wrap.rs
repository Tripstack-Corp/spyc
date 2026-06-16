//! Coverage-guided fuzz target for the word-wrap routine. Splits the input
//! into a width (first byte) + arbitrary UTF-8 text, and asserts every wrap
//! range lands on a char boundary (a mid-codepoint range would panic the
//! pager's slicing). Run with:  cargo +nightly fuzz run word_wrap
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Some((&w, rest)) = data.split_first() {
        if let Ok(text) = std::str::from_utf8(rest) {
            spyc::fuzz::word_wrap(text, w as usize);
        }
    }
});
