# Building spyc from source

Most users don't need this — install a pre-built binary with Homebrew,
`apt`, or a Release tarball (see [INSTALL.md](INSTALL.md)). Build from
source if you want to hack on spyc, run the tip of `main`, or target a
platform we don't ship binaries for.

For the contributor workflow (branching, the quality gate, commit
conventions), see [CONTRIBUTING.md](CONTRIBUTING.md).

## Rust toolchain

spyc is written in Rust. Install the toolchain via rustup:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Minimum supported Rust version: **1.88** (for `if let` chains).

The repo pins an exact toolchain in `rust-toolchain.toml` (currently
**1.96.0**), so rustup auto-installs and selects it on first build —
no manual `rustup default` needed. Bump that file to move the project
to a newer release.

## Build and install

```sh
git clone https://github.com/Tripstack-Corp/spyc.git
cd spyc
make install          # builds release + copies to ~/.local/bin (no sudo)
```

Make sure `~/.local/bin` is on your `PATH`. Most Linux distros include
it by default; on macOS add this to `~/.zshrc` (or `~/.bash_profile`):

```sh
export PATH="$HOME/.local/bin:$PATH"
```

To install system-wide instead, override `PREFIX` and use sudo:

```sh
sudo make install PREFIX=/usr/local
```

Or build and copy manually:

```sh
cargo build --release
install -m 755 target/release/spyc ~/.local/bin/
```

`make doctor` checks that the toolchain and cross-compile prerequisites
are present before you start.

## Development build

For iterating on the code, a debug build is faster:

```sh
make            # or: cargo build     — debug build
make run        # cargo run
make check      # fmt + clippy + test + deny (the CI gate)
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for the full quality gate,
`make lint-linux` for OS-gated code, and the code conventions.

## Cross-compilation

To build for a platform other than your own — or to reproduce the
release artifacts locally — you need a cross-linker. spyc uses Zig via
`cargo-zigbuild`:

```sh
brew install zig
cargo install cargo-zigbuild
rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl
rustup target add x86_64-apple-darwin aarch64-apple-darwin
```

Then use the Makefile targets:

```sh
make dist                    # all platforms → dist/
make release-macos-universal # macOS universal (arm64 + x86_64)
make release-linux-x86       # Linux x86_64 (static, musl)
make release-linux-arm       # Linux aarch64 (static, musl)
make dist-checksums          # SHA-256 over the dist/ artifacts
make dist-sign               # GPG-sign the checksums (set GPG_KEY=<id>)
```

Linux binaries are statically linked against musl, so they run on any
distro without a libc dependency.

## Debian package

`make deb` wraps a built Linux binary into a `.deb` (requires
`dpkg-deb`, so run it on Linux or `brew install dpkg` on macOS). Build
the target binary first, then package it:

```sh
make release-linux-x86 && make deb-x86    # → dist/spyc_<version>_amd64.deb
make release-linux-arm && make deb-arm    # → dist/spyc_<version>_arm64.deb
make deb                                  # both (binaries must already exist)
```

These are the same packages the release pipeline publishes to the
`apt` repository — see
[docs/RELEASE_ENGINEERING.md](docs/RELEASE_ENGINEERING.md) for how the
hosted apt repo is built and signed.

## Crate shape

spyc is a **lib + bin** crate: `src/lib.rs` owns every module and the
`run()` entry point; `src/main.rs` is a thin shim. The split lets
`fuzz/` (a standalone nightly workspace) link the library. New fuzz
entry points go through the `pub mod fuzz` facade in `lib.rs` rather
than widening module visibility.
