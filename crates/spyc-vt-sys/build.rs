//! Link the vendored libghostty-vt archive for the host target.
//!
//! No Zig, no network, no `git clone` at build time — that is the whole point
//! of vendoring, and it is what keeps `cargo install spyc` working on a machine
//! with no Zig toolchain. The archives are built out-of-band by
//! `make vendor-ghostty` at a pinned commit (see `src/pin.rs`) and committed.

// The SHA-256 round constants and initial state are quoted verbatim from
// FIPS 180-4. `unreadable_literal` wants `0x428a_2f98`; grouping them makes
// them HARDER to diff against the spec, which is the only way to check them.
#![allow(clippy::unreadable_literal)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Every archive's SHA-256, parsed from the committed `vendor/CHECKSUMS`.
/// Verified on every build: a corrupted or swapped archive is a supply-chain
/// event, and the checksum is the only thing standing between the committed
/// blob and the linker.
fn expected_checksums(vendor: &Path) -> Vec<(String, String)> {
    let path = vendor.join("CHECKSUMS");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let mut it = l.split_whitespace();
            let sum = it.next().unwrap_or_default().to_string();
            let file = it.next().unwrap_or_default().to_string();
            (file, sum)
        })
        .collect()
}

fn sha256_hex(bytes: &[u8]) -> String {
    // A local SHA-256 rather than a `sha2` build-dependency: a build script
    // that verifies a checksum should not itself pull a dependency tree.
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut msg = bytes.to_vec();
    let bitlen = (bytes.len() as u64) * 8;
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bitlen.to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for (i, wi) in w.iter_mut().enumerate().take(16) {
            let b = &chunk[i * 4..i * 4 + 4];
            *wi = u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ ((!v[4]) & v[6]);
            let t1 = v[7]
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [
                t1.wrapping_add(t2),
                v[0],
                v[1],
                v[2],
                v[3].wrapping_add(t1),
                v[4],
                v[5],
                v[6],
            ];
        }
        for (hi, vi) in h.iter_mut().zip(v.iter()) {
            *hi = hi.wrapping_add(*vi);
        }
    }
    h.iter().fold(String::with_capacity(64), |mut acc, x| {
        let _ = write!(acc, "{x:08x}");
        acc
    })
}

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let vendor = manifest.join("vendor");
    let target = std::env::var("TARGET").expect("TARGET");

    println!("cargo:rerun-if-changed=vendor/CHECKSUMS");
    println!("cargo:rerun-if-changed=build.rs");

    let dir = vendor.join(&target);
    let archive = dir.join("libghostty-vt.a");
    println!("cargo:rerun-if-changed={}", archive.display());

    assert!(
        archive.exists(),
        "spyc-vt-sys has no vendored libghostty-vt archive for target `{target}`.\n\
         Vendored targets: {}\n\
         Add one with `make vendor-ghostty` (needs zig {}) and refresh vendor/CHECKSUMS.\n\
         Note the CI test host is x86_64-unknown-linux-gnu, which is NOT a release\n\
         target — it still needs an archive or the gate cannot build this crate.",
        std::fs::read_dir(&vendor)
            .map(|rd| {
                let mut v: Vec<String> = rd
                    .filter_map(Result::ok)
                    .filter(|e| e.path().is_dir())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect();
                v.sort();
                v.join(", ")
            })
            .unwrap_or_default(),
        include_str!("REQUIRED_ZIG").trim(),
    );

    // Verify before linking, not after.
    let key = format!("./{target}/libghostty-vt.a");
    let expected = expected_checksums(&vendor);
    let Some((_, want)) = expected.iter().find(|(f, _)| *f == key) else {
        panic!("vendor/CHECKSUMS has no entry for `{key}`")
    };
    let bytes = std::fs::read(&archive)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", archive.display()));
    let got = sha256_hex(&bytes);
    assert_eq!(
        &got, want,
        "checksum mismatch for {key}\n  expected {want}\n  got      {got}\n\
         The vendored archive does not match vendor/CHECKSUMS. Do not link it."
    );

    println!("cargo:rustc-link-search=native={}", dir.display());
    println!("cargo:rustc-link-lib=static=ghostty-vt");
}
