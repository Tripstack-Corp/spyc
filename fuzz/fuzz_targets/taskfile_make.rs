//! Coverage-guided fuzz target for Makefile target discovery: arbitrary bytes,
//! lossily decoded, through `taskfile::make::targets`, which must never panic
//! and must list only names a user can type as a goal. Run with:
//!
//!   make fuzz TARGET=taskfile_make
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    spyc::fuzz::taskfile_make(data);
});
