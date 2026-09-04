# spyc-vt-sys

Raw FFI to [libghostty-vt](https://libghostty.tip.ghostty.org/) — the terminal
state machine extracted from [Ghostty](https://ghostty.org) — at a spyc-owned
pinned commit, with the static archives vendored.

This crate exists so that everything C has one home: the pin, the generated
bindings, the prebuilt archives and their checksums. The safe engine wrapper
lives above it in spyc.

## Why vendored archives rather than building from source

`libghostty-vt-sys 0.2.1`, the published bindings, run `zig build` from their
build script **and `git clone` ghostty at build time**. That makes a Zig
toolchain and network access requirements of `cargo install spyc`, which is not
acceptable for a distribution channel. It is also fragile in a way that bites
immediately: the commit those bindings pin requires Zig **0.15.x exactly**
(ghostty's `requireZig` compares major and minor for equality), and Zig 0.15.2
cannot link at all on macOS 26.

So the archives are built out of band, committed, and checksummed. A user
building spyc needs no Zig, no network for this crate, and no git.

## Why the bindings are generated here rather than reused

The published bindings are **ABI-incompatible** with the pin. Between the commit
they target and a commit eight weeks later, `ghostty_terminal_new` lost its
options struct in favour of two scalar parameters. Those bindings still compile
against the newer library and return garbage — `rows()` reports the scrollback
budget. A C ABI has no version handshake, so nothing warns.

`src/bindings.rs` is generated from **this pin's** headers by
`tools/gen_bindings.rs` and checked in, so neither bindgen nor libclang is a
dependency of this crate. The generated file carries ~371 compile-time layout
assertions; a struct that changes shape fails the build.

`tools/gen_bindings.rs` is not a crate target — it is the recipe, run by hand on
a pin bump. It needs `bindgen` and libclang, which is exactly why it does not
live in the dependency graph.

## Vendored targets

Five, not four. The release matrix is macOS arm64/x86_64 and musl
aarch64/x86_64 — but **CI's Lint and Tests jobs run on `ubuntu-latest`, whose
host target is `x86_64-unknown-linux-gnu` and is not a release artifact.** A
four-archive set fails the gate on the PR that adds it.

| target | why |
|---|---|
| `aarch64-apple-darwin` | release; the dev machine |
| `x86_64-apple-darwin` | release |
| `x86_64-unknown-linux-musl` | release (static) |
| `aarch64-unknown-linux-musl` | release (static) |
| `x86_64-unknown-linux-gnu` | **CI test/lint host only** |

Adding a target means adding an archive and a `vendor/CHECKSUMS` line, or the
build fails with a message that says so.

## Rebuilding the archives

```sh
make vendor-ghostty              # needs zig 0.16.0 (see REQUIRED_ZIG)
```

Zig cross-compiles all five from one host. The recipe uses `ReleaseSmall`,
which is the size lever; it does **not** strip, because at `ReleaseSmall` the
archives carry no debug sections and a strip step measurably changes nothing.
(An earlier version of this recipe did strip, and on Linux the step was a silent
no-op because `llvm-strip` was absent and the failure was swallowed. A step that
does nothing is worse than no step.)

## Bump policy

Deliberate, gated, never tracking `main` — the full procedure is on
[`pin::BUMP_POLICY`](src/pin.rs). The short version: regenerate the bindings,
rebuild every archive, re-run the spike harness in full, append a dated addendum
to `docs/drafts/VT_ENGINE_SPIKE.md`, and re-measure the bytes-per-row constant
the byte ceiling is derived from. A bump that skips the harness re-run is the
failure this crate exists to prevent.

## Scrollback limits

[`scrollback`](src/scrollback.rs) turns spyc's row budget into libghostty's two
limits. **Rows are the UX contract; bytes are a safety valve.** Neither is left
at its default — the default byte cap truncates retained history to ~840 rows
*irrespective of the line limit*, so a 10,000-row budget would silently deliver
8% of itself.

## Licensing

This crate is BSD-3-Clause like the rest of spyc. The vendored archives are
built from Ghostty, which is MIT — see `LICENSE-ghostty`. The generated
bindings are derived from Ghostty's headers and carry the same attribution.
