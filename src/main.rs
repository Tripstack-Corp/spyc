//! spyc binary entry point — a thin shim over the `spyc` library.
//!
//! All startup logic lives in `src/lib.rs` (`spyc::run`); this exists only so
//! the crate also builds as a library, which the `cargo-fuzz` targets in
//! `fuzz/` link against. Keep it trivial.

fn main() -> anyhow::Result<()> {
    spyc::run()
}
