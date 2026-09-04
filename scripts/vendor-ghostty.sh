#!/usr/bin/env bash
# Rebuild spyc-vt-sys's vendored libghostty-vt static archives at the pinned
# ghostty commit, for every target spyc needs, and refresh the checksums.
#
# Driven by `make vendor-ghostty`, which checks the zig version first. Run this
# only on a deliberate pin bump; see crates/spyc-vt-sys/src/pin.rs BUMP_POLICY
# for what else a bump owes.
set -euo pipefail

CRATE="crates/spyc-vt-sys"
PIN=$(sed -n 's/^pub const GHOSTTY_COMMIT: &str = "\(.*\)";$/\1/p' "$CRATE/src/pin.rs")
[ -n "$PIN" ] || { echo "could not read GHOSTTY_COMMIT from $CRATE/src/pin.rs"; exit 1; }
echo "pin: $PIN"

WORK="${GHOSTTY_WORK:-$(mktemp -d)}"
SRC="$WORK/ghostty"
if [ ! -d "$SRC/.git" ]; then
    echo "cloning ghostty into $SRC"
    git clone --filter=blob:none --no-checkout https://github.com/ghostty-org/ghostty "$SRC"
fi
git -C "$SRC" fetch --depth=1 origin "$PIN" 2>/dev/null || git -C "$SRC" fetch origin
git -C "$SRC" checkout -q "$PIN"

# Rust target : zig target. Five, not four — x86_64-unknown-linux-gnu is CI's
# test/lint host and is not a release artifact, but the gate cannot build the
# crate without it.
TARGETS="
aarch64-apple-darwin:aarch64-macos-none
x86_64-apple-darwin:x86_64-macos-none
x86_64-unknown-linux-gnu:x86_64-linux-gnu
x86_64-unknown-linux-musl:x86_64-linux-musl
aarch64-unknown-linux-musl:aarch64-linux-musl
"

for pair in $TARGETS; do
    rust="${pair%%:*}"; zt="${pair##*:}"
    echo "=== $rust (zig -Dtarget=$zt)"
    # ReleaseSmall is the size lever. Deliberately NO strip step: at
    # ReleaseSmall these archives carry no debug sections, and stripping them
    # measurably changes nothing. A previous version of this script did strip,
    # and on Linux it was a silent no-op (llvm-strip absent, failure swallowed).
    ( cd "$SRC" && zig build \
        -Demit-lib-vt=true \
        -Doptimize=ReleaseSmall \
        -Demit-xcframework=false \
        -Dapp-runtime=none \
        -Dtarget="$zt" \
        --prefix "$WORK/pfx-$rust" \
        --cache-dir "$WORK/zc-$rust" )
    src="$WORK/pfx-$rust/lib/libghostty-vt.a"
    [ -f "$src" ] || { echo "no archive at $src"; exit 1; }
    mkdir -p "$CRATE/vendor/$rust"
    cp "$src" "$CRATE/vendor/$rust/libghostty-vt.a"
    # A size budget rather than a cosmetic strip: this is what actually keeps the
    # published crate under crates.io's cap, so it fails loudly.
    sz=$(wc -c < "$CRATE/vendor/$rust/libghostty-vt.a")
    if [ "$sz" -gt 4194304 ]; then
        echo "archive for $rust is $sz B, over the 4 MiB/target budget"
        echo "  five targets must stay well inside the 10 MiB crates.io cap"
        exit 1
    fi
    printf "  ok %s B\n" "$sz"
done

( cd "$CRATE/vendor" && shasum -a 256 ./*/libghostty-vt.a > CHECKSUMS )
echo "refreshed $CRATE/vendor/CHECKSUMS:"
cat "$CRATE/vendor/CHECKSUMS"
echo
echo "Now: regenerate bindings, re-run the spike harness, append the addendum."
echo "See crates/spyc-vt-sys/src/pin.rs BUMP_POLICY."
