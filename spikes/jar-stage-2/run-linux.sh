#!/bin/sh
# The jar spike on Linux, from a Mac or Linux host with podman (or docker):
# an Alpine Rust image, whose native target is the musl one the vendored
# libghostty archives cover. The worktree is mounted read-only; the target
# dir, the cargo registry and the pinned toolchain are kept in volumes.
set -e
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
engine=$(command -v podman || command -v docker) || { echo "needs podman or docker"; exit 1; }
for v in spyc-jar-target spyc-jar-cargo spyc-jar-rustup; do "$engine" volume create "$v" >/dev/null 2>&1 || true; done
"$engine" run --rm -v spyc-jar-rustup:/r docker.io/library/rust:1-alpine sh -c '[ -e /r/settings.toml ] || cp -a /usr/local/rustup/. /r/'
exec "$engine" run --rm \
    -v "$root":/src:ro -v "$here":/linux:ro \
    -v spyc-jar-target:/target -v spyc-jar-cargo:/usr/local/cargo/registry -v spyc-jar-rustup:/usr/local/rustup \
    -e CARGO_TARGET_DIR=/target docker.io/library/rust:1-alpine /linux/linux.sh
