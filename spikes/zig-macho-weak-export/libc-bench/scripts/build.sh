#!/usr/bin/env bash
# usage: build.sh A|B  — builds spyc and the spike's bench for macOS arm64 and
# aarch64-linux-musl from the worktree, as the worktree's archives stand.
set -euxo pipefail
V=$1
B=${BENCH:?}
W=${SPYC_B:?}
ulimit -n 8192
export CARGO_PROFILE_RELEASE_STRIP=none
export PATH="$B/tools/bin:$PATH"
cargo-zigbuild --version
mkdir -p "$B/bin/$V"
cd "$W"
export CARGO_TARGET_DIR="$B/target-spyc"
cargo build --locked --release
cp "$CARGO_TARGET_DIR/release/spyc" "$B/bin/$V/spyc-macos"
cargo zigbuild --locked --release --target aarch64-unknown-linux-musl
cp "$CARGO_TARGET_DIR/aarch64-unknown-linux-musl/release/spyc" "$B/bin/$V/spyc-musl"
cd "$W/spikes/vt-engine"
export CARGO_TARGET_DIR="$B/target-spike"
cargo build --locked --release --features ghostty --bin bench
cp "$CARGO_TARGET_DIR/release/bench" "$B/bin/$V/bench-macos"
cargo zigbuild --locked --release --features ghostty --bin bench --target aarch64-unknown-linux-musl
cp "$CARGO_TARGET_DIR/aarch64-unknown-linux-musl/release/bench" "$B/bin/$V/bench-musl"
shasum -a 256 "$W"/crates/spyc-vt-sys/vendor/*/libghostty-vt.a
echo BUILD-DONE $V
