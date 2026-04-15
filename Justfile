# cspy build recipes. Install `just` (https://github.com/casey/just) to use.

# Default: fast dev build.
default: build

build:
    cargo build

run:
    cargo run

test:
    cargo test

clippy:
    cargo clippy --all-targets -- -D warnings

fmt:
    cargo fmt --all

# --- Static release builds -------------------------------------------------

# Linux x86_64, statically linked against musl.
build-linux-x86:
    rustup target add x86_64-unknown-linux-musl
    cargo build --release --target x86_64-unknown-linux-musl
    @echo "==> target/x86_64-unknown-linux-musl/release/cspy"

# Linux aarch64 (ARM64), statically linked against musl.
build-linux-arm:
    rustup target add aarch64-unknown-linux-musl
    cargo build --release --target aarch64-unknown-linux-musl
    @echo "==> target/aarch64-unknown-linux-musl/release/cspy"

# macOS universal binary (x86_64 + aarch64 fat).
build-macos-universal:
    rustup target add x86_64-apple-darwin aarch64-apple-darwin
    cargo build --release --target x86_64-apple-darwin
    cargo build --release --target aarch64-apple-darwin
    mkdir -p target/universal-apple-darwin/release
    lipo -create \
        target/x86_64-apple-darwin/release/cspy \
        target/aarch64-apple-darwin/release/cspy \
        -output target/universal-apple-darwin/release/cspy
    @echo "==> target/universal-apple-darwin/release/cspy"

# Build everything Linux and macOS release artifacts.
release-all: build-linux-x86 build-linux-arm build-macos-universal
