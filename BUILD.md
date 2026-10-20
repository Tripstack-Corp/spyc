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
# Use single-branch to avoid downloading the heavy gh-pages branch
git clone --single-branch --branch main https://github.com/Tripstack-Corp/spyc.git
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

## Running CURRENT (the development stream)

Every channel in [INSTALL.md](INSTALL.md) installs a **RELEASE** — tagged,
signed, frozen. `main` is spyc's **CURRENT** stream: every merged change
lands there behind CI. It is what the author dog-foods daily, and where a
fix reaches you first.

It is also unreleased by definition — rolling, and it may break. There are
no prebuilt binaries, so CURRENT is source-only and the clone + `make
install` above *is* the install. Update by pulling and rebuilding:

```sh
git pull && make install
```

A CURRENT build says so: the version carries a `-CURRENT` suffix naming the
minor it is heading for.

```sh
$ spyc --version
spyc <x.y.z>-CURRENT (<sha>)
```

`2.1.0-CURRENT` is *on the way to* 2.1.0, never 2.1.0 itself. That suffix is
static for the whole minor cycle, so the trailing SHA is the only thing that
tells two CURRENT builds apart — quote it in a bug report. `--verbose` prints
it alongside the toolchain and terminal spyc saw:

```sh
$ spyc --verbose
🌶️ spyc 2.1.0-CURRENT
  git:     69a5ff1
  built:   …
  rustc:   …
```

To go back to a release, install through any channel in
[INSTALL.md](INSTALL.md) — a tagged build simply has no suffix. The full
stream model (CURRENT, STABLE, RELEASE, and how releases are cut) is in
[docs/RELEASE_ENGINEERING.md](docs/RELEASE_ENGINEERING.md).

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

## Build output and disk

**Build output is disk-expensive: one gate run leaves ~4 GB, and worktree-per-PR multiplies it** — thirteen parked worktrees reached **57 GB**. Two rules follow, and which one applies depends on whether the machine shares a build directory (`[build] target-dir` in `~/.cargo/config.toml`, or `CARGO_TARGET_DIR`; `make` asks cargo via `TARGET_DIR`, so never hardcode `target/` in a recipe):

- **Sharing one** (recommended, and what this developer runs): dependencies compile once for every worktree. **Never `cargo clean` — there is nothing local to clean, so it deletes the whole shared cache** for every other worktree and agent; `make clean` refuses on a target dir outside the checkout for that reason (`FORCE=1` overrides). **`make prune-target` is the cleanup, and it has to run on a schedule** (a nightly launchd or cron job), because the shared cache leaks two ways that nothing else reclaims: `remove_worktree` leaves the removed worktree's build set behind (each worktree path hashes into its own `<crate>-<hash>` names), and on macOS every incremental rebuild leaves the previous build's per-codegen-unit debug objects in `deps/` (one test binary had 131k of them while its debug map read 256; the cache reached 150 GB). It deletes sets untouched for `PRUNE_DAYS` and objects no executable's debug map references, and never a third-party dependency. `cargo sweep` can't do the second, since it judges whole sets. Cargo holds an exclusive lock on the directory, so concurrent builds serialize; that's the accepted trade, and `rust-analyzer.toml` keeps the language server out of the same lock. (Clean-before-teardown is strong muscle memory from the per-worktree layout: it wiped this cache twice in one afternoon, once by the agent that had just written this rule. Check what `cargo clean` resolves to before running it, or don't run it.)
- **Per-worktree `target/`**: `cargo clean` when you're done with a worktree — the rebuild is worth the disk. **`fuzz/` is a separate workspace, so the root clean doesn't touch `fuzz/target`**, another ~2.6 GB that survives the obvious command: `cargo clean && cargo clean --manifest-path fuzz/Cargo.toml`.

Either way, `remove_worktree` as soon as a PR merges — thirteen live worktrees is the anomaly no cache layout fixes.

## Crate shape

spyc is a **lib + bin** crate: `src/lib.rs` owns every module and the
`run()` entry point; `src/main.rs` is a thin shim. The split lets
`fuzz/` (a standalone nightly workspace) link the library. New fuzz
entry points go through the `pub mod fuzz` facade in `lib.rs` rather
than widening module visibility.
