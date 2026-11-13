//! The pinned ghostty commit, and the policy for moving it.

/// The ghostty commit every vendored archive and the checked-in bindings were
/// built from: the one ghostty's `vt-1.0.1` tag names.
///
/// **Why the commit and not the tag.** libghostty-vt has its own release
/// cycle, and its `vt-X.Y.Z` tags are separate from the app's `vX.Y.Z` ones.
/// A tag can still be re-pointed, and a hash cannot, so the hash is what the
/// vendor script checks out; [`GHOSTTY_VT_VERSION`] names the release it is.
pub const GHOSTTY_COMMIT: &str = "108cacb17ed8990840cd14abfce43d8ed8503ca1";

/// The libghostty-vt release [`GHOSTTY_COMMIT`] is tagged as.
pub const GHOSTTY_VT_VERSION: &str = "1.0.1";

/// The pin's author date, so the pin's age is readable without a network round
/// trip. `2026-11-11`.
pub const GHOSTTY_COMMIT_DATE: &str = "2026-11-11";

/// The Zig version the pin's `build.zig.zon` requires. ghostty's `requireZig`
/// wants **major and minor equal** and the patch at least as new; the vendor
/// recipe uses exactly this version, because another Zig compiles different
/// bytes and the archives are checked for reproducibility.
pub const REQUIRED_ZIG: &str = "0.17.0";

/// Bump policy — deliberate, gated, and never tracking `main`.
///
/// From 1.0 libghostty-vt promises semver and a stable C ABI: opaque handles,
/// enum-keyed getters and setters, size-prefixed structs, and
/// `GHOSTTY_UNSUPPORTED` rather than a crash when newer headers meet an older
/// library. Before 1.0 it broke in a way a C ABI cannot report: between the
/// commit the published `libghostty-vt-sys 0.2.1` bindings target and a commit
/// eight weeks later, `ghostty_terminal_new` lost its options struct in favour
/// of two scalar parameters, and the published bindings still compiled against
/// the newer library and returned garbage — `rows()` reported the scrollback
/// budget. The promise covers the ABI, not behaviour: scrollback retention and
/// snapshot fidelity are what spyc depends on, and no ABI check measures them.
///
/// So the pin moves only on purpose:
///
/// 1. Regenerate the bindings from the new pin's headers with
///    `tools/gen_bindings.rs`. The generated file carries ~371 compile-time
///    layout assertions; an ABI change that alters a struct fails the build
///    rather than returning nonsense.
/// 2. Rebuild every vendored archive at the new pin and refresh `CHECKSUMS`.
/// 3. Re-run the spike harness (`spikes/vt-engine/`) in full and append a dated
///    addendum to `docs/archive/VT_ENGINE_SPIKE.md`. Scrollback retention,
///    panic count, rehydration fidelity and throughput are all pin-dependent
///    and none of them are assumed to carry over.
/// 4. Re-measure `scrollback::BYTES_PER_ROW_HEAVY` the same way, since the
///    byte ceiling is derived from it.
///
/// A pin bump that skips step 3 is the failure this crate exists to prevent.
pub const BUMP_POLICY: () = ();

/// Defects reported upstream against an earlier pin, so a bump checks them
/// off instead of re-investigating them.
///
/// - **The VT formatter lost the last scroll on replay** — reported as
///   <https://github.com/ghostty-org/ghostty/discussions/14148>. **Fixed at
///   1.0.0**: `rehydrate` round-trips all 26 cases with no viewport offset and
///   `offset_isolate` no longer reproduces it. It never affected spyc, which
///   restores through `ghostty_snapshot_*`.
pub const REPORTED_UPSTREAM: () = ();
