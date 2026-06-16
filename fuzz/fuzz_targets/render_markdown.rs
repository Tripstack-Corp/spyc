//! Coverage-guided fuzz target for the markdown renderer — the pager ingests
//! untrusted file content as markdown, so it must never panic. Run with:
//!   cargo +nightly fuzz run render_markdown
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(source) = std::str::from_utf8(data) {
        spyc::fuzz::render_markdown(source);
    }
});
