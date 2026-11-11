#!/bin/sh
# Build the `spyc-jar` of an earlier commit, for the Q4 test
# (crates/spyc-jar/tests/compat.rs): `old_jar.sh COMMIT OUTDIR` leaves
# OUTDIR/spyc-jar. The crate builds on its own, outside the workspace.
set -eu
commit=$1 out=$2
src=$(mktemp -d)
root=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
git -C "$root" archive "$commit" crates/spyc-jar | tar -x -C "$src"
CARGO_TARGET_DIR="$out/target" cargo build -q --release --manifest-path "$src/crates/spyc-jar/Cargo.toml"
cp "$out/target/release/spyc-jar" "$out/spyc-jar"
rm -rf "$src"
echo "$out/spyc-jar"
