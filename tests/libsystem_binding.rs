#![cfg(target_os = "macos")]

//! spyc's memory and string functions come from libSystem, not from a copy a
//! vendored archive carries.
//!
//! The vendored libghostty-vt archive bundles Zig's `compiler_rt.o`, and up to
//! libghostty-vt 1.0.0 ghostty's own unit also defined `memset`. Apple's `ld`
//! binds an undefined symbol to an archive member's definition before it
//! reaches libSystem, weak or not, so any such copy left global in the archive
//! becomes the one the whole binary calls.
//!
//! `scripts/check-libsystem-binding.sh` is the check; this runs it on the
//! binary `cargo test` builds.

use std::process::Command;

#[test]
fn spyc_takes_its_memory_functions_from_libsystem() {
    let script = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/scripts/check-libsystem-binding.sh"
    );
    let out = Command::new(script)
        .arg(env!("CARGO_BIN_EXE_spyc"))
        .output()
        .expect("run check-libsystem-binding.sh");
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
