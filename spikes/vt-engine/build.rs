//! Bake the RESOLVED dependency versions into the binaries, read from this
//! crate's own Cargo.lock. A report that quotes a version it didn't build
//! against is the easiest way to be confidently wrong, so the number and the
//! measurement come from the same place.
use std::fmt::Write as _;

fn main() {
    println!("cargo:rerun-if-changed=Cargo.lock");
    let lock = std::fs::read_to_string("Cargo.lock").unwrap_or_default();
    for want in ["vt100", "wezterm-term", "libghostty-vt", "libghostty-vt-sys"] {
        let ver = resolved(&lock, want).unwrap_or_else(|| "absent".to_string());
        let mut key = String::new();
        let _ = write!(key, "{}_VERSION", want.replace('-', "_").to_uppercase());
        println!("cargo:rustc-env={key}={ver}");
    }
}

/// Pull `version` out of the `[[package]]` block whose `name` is `want`.
fn resolved(lock: &str, want: &str) -> Option<String> {
    let mut lines = lock.lines();
    while let Some(l) = lines.next() {
        if l.trim() == format!("name = \"{want}\"") {
            for l2 in lines.by_ref().take(3) {
                if let Some(v) = l2.trim().strip_prefix("version = ") {
                    return Some(v.trim_matches('"').to_string());
                }
            }
        }
    }
    None
}
