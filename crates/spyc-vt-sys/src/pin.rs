//! The pinned ghostty commit, and the policy for moving it.

/// The ghostty commit every vendored archive and the checked-in bindings were
/// built from.
///
/// **Why a bare commit and not a tag.** No released ghostty tag carries the C
/// API: `v1.3.1`, the latest release at the time of pinning, has no
/// `include/ghostty/vt/` at all and requires Zig 0.15.2, which cannot link on
/// macOS 26. Ghostty publishes a rolling `tip` tag, but a tag that moves is
/// strictly worse than a hash for a pin — it had already moved off this commit
/// by the time the pin was recorded.
pub const GHOSTTY_COMMIT: &str = "1f5bb5769fbb5e717546073d33d3985604a315b2";

/// The pin's author date, so the pin's age is readable without a network round
/// trip. `2026-09-04`.
pub const GHOSTTY_COMMIT_DATE: &str = "2026-09-04";

/// The Zig version the pin's `build.zig.zon` requires. ghostty's `requireZig`
/// compares **major and minor for equality**, so this is exact, not a floor.
pub const REQUIRED_ZIG: &str = "0.16.0";

/// Bump policy — deliberate, gated, and never tracking `main`.
///
/// libghostty's C API is pre-1.0 and its own documentation says breaking
/// changes are expected. It has already broken once in a way that a C ABI
/// cannot report: between the commit the published `libghostty-vt-sys 0.2.1`
/// bindings target and a commit eight weeks later, `ghostty_terminal_new` lost
/// its options struct in favour of two scalar parameters. The published
/// bindings still compile against the newer library and return garbage —
/// `rows()` reported the scrollback budget. Nothing warned.
///
/// So the pin moves only on purpose:
///
/// 1. Regenerate the bindings from the new pin's headers with
///    `tools/gen_bindings.rs`. The generated file carries ~371 compile-time
///    layout assertions; an ABI change that alters a struct fails the build
///    rather than returning nonsense.
/// 2. Rebuild every vendored archive at the new pin and refresh `CHECKSUMS`.
/// 3. Re-run the spike harness (`spikes/vt-engine/`) in full and append a dated
///    addendum to `docs/drafts/VT_ENGINE_SPIKE.md`. Scrollback retention,
///    panic count, rehydration fidelity and throughput are all pin-dependent
///    and none of them are assumed to carry over.
/// 4. Re-measure `scrollback::BYTES_PER_ROW_HEAVY` the same way, since the
///    byte ceiling is derived from it.
///
/// A pin bump that skips step 3 is the failure this crate exists to prevent.
pub const BUMP_POLICY: () = ();
