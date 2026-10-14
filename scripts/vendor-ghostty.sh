#!/usr/bin/env bash
# Rebuild spyc-vt-sys's vendored libghostty-vt static archives at the pinned
# ghostty commit, for every target spyc needs, and refresh the checksums.
#
# Driven by `make vendor-ghostty`, which checks the zig version first. Run this
# only on a deliberate pin bump; see crates/spyc-vt-sys/src/pin.rs BUMP_POLICY
# for what else a bump owes.
set -euo pipefail

ROOT=$(pwd)
CRATE="crates/spyc-vt-sys"
PIN=$(sed -n 's/^pub const GHOSTTY_COMMIT: &str = "\(.*\)";$/\1/p' "$CRATE/src/pin.rs")
[ -n "$PIN" ] || { echo "could not read GHOSTTY_COMMIT from $CRATE/src/pin.rs"; exit 1; }
echo "pin: $PIN"

WORK="${GHOSTTY_WORK:-$(mktemp -d)}"
mkdir -p "$WORK" && WORK=$(cd "$WORK" && pwd)
SRC="$WORK/ghostty"
# A private global cache. zig keys its cache on the version string, so a shared
# one hands this build objects (compiler_rt, for one) that another build of the
# same version made earlier, and a different zig build makes different code.
export ZIG_GLOBAL_CACHE_DIR="$WORK/zig-global-cache"
if [ ! -d "$SRC/.git" ]; then
    echo "cloning ghostty into $SRC"
    git clone --filter=blob:none --no-checkout https://github.com/ghostty-org/ghostty "$SRC"
fi
git -C "$SRC" fetch --depth=1 origin "$PIN" 2>/dev/null || git -C "$SRC" fetch origin
git -C "$SRC" checkout -q "$PIN"

# Re-archive deterministically. zig's archive is not reproducible as built: a
# Linux one names each member by its absolute path in the build cache, which
# changes every run, and a darwin one's symbol table records the build time and
# the builder's uid/gid. The objects inside are byte-identical across runs,
# paths and caches, so the same members in the same order, under their
# basenames, re-archived by this same zig's `ar` in deterministic mode, give
# the same bytes on every host.
normalize() { # <in.a> <out.a> <gnu|darwin>
    local in="$1" out="$2" fmt="$3" tmp m members=()
    while IFS= read -r m; do members+=("$(basename "$m")"); done < <(zig ar t "$in")
    [ "${#members[@]}" -gt 0 ] || { echo "no members in $in"; exit 1; }
    if [ -n "$(printf '%s\n' "${members[@]}" | sort | uniq -d)" ]; then
        echo "two members of $in share a basename; renaming them would collide"
        exit 1
    fi
    tmp=$(mktemp -d)
    # Members come out unreadable (mode 0 in the archive's headers).
    ( cd "$tmp" && zig ar x "$in" && chmod 644 ./* )
    for m in "${members[@]}"; do
        [ -f "$tmp/$m" ] || { echo "member $m did not extract from $in"; exit 1; }
    done
    rm -f "$out"
    ( cd "$tmp" && zig ar rcsD --format="$fmt" "$out" "${members[@]}" )
    rm -rf "$tmp"
}

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
    case "$rust" in
        *-apple-darwin) fmt=darwin ;;
        *) fmt=gnu ;;
    esac
    normalize "$src" "$ROOT/$CRATE/vendor/$rust/libghostty-vt.a" "$fmt"
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
