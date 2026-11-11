//! The blob a new `spyc-jar` binary validates before the old one `exec`s it
//! (R33): the most dangerous parser in the jar's design, since a malformed
//! blob that gets through takes over a live PTY. It must never panic, and
//! whatever it accepts must encode to a blob it accepts again.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(h) = spyc_jar::handover::decode(data) {
        let again = spyc_jar::handover::encode(&h);
        assert!(spyc_jar::handover::decode(&again).is_ok());
    }
});
