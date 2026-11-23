#![cfg(all(target_os = "linux", target_env = "gnu"))]

//! On glibc Linux, spyc's memory and string functions come from glibc, not
//! from a copy a vendored archive carries.
//!
//! The vendored libghostty-vt archive bundles Zig's `compiler_rt.o`, which
//! defines `memset`, `memcpy` and the maths functions. A copy left global in
//! the archive becomes the one the whole binary calls (#1148), the same
//! displacement `libsystem_binding.rs` guards against on macOS.
//!
//! `scripts/check-linux-libc-binding.sh` is the check; this runs it on the
//! binary `cargo test` builds. The musl targets link statically, so there is
//! nothing to import and this file builds no test for them.

use std::process::Command;

#[test]
fn spyc_takes_its_memory_functions_from_glibc() {
    let script = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/scripts/check-linux-libc-binding.sh"
    );
    let out = Command::new(script)
        .arg(env!("CARGO_BIN_EXE_spyc"))
        .output()
        .expect("run check-linux-libc-binding.sh");
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
