#!/usr/bin/env bash
# Fallback B: main's vendored archive with the localize step inverted. Every
# LIBC_SYMBOLS name a member defines as a local symbol is made global again
# (and weak, as Zig's compiler_rt exports them: `common.linkage` is .weak),
# then the members are re-archived in their original order.
# usage: globalize.sh RUST_TARGET OUT_ARCHIVE
set -euo pipefail
T=$1 OUT=$2
W=${SPYC_B:?}
L=/opt/homebrew/opt/llvm@22/bin
IN=${SPYC_MAIN:?}/crates/spyc-vt-sys/vendor/$T/libghostty-vt.a
case "$T" in *-apple-darwin) fmt=darwin; pre=_ ;; *) fmt=gnu; pre= ;; esac
tmp=$(mktemp -d)
members=()
while IFS= read -r m; do members+=("$m"); done < <("$L/llvm-ar" t "$IN")
( cd "$tmp" && "$L/llvm-ar" x "$IN" && chmod 644 ./* )
want=$(grep -v '^#' "$W/crates/spyc-vt-sys/vendor/LIBC_SYMBOLS" | sed "s/^/$pre/")
for m in "${members[@]}"; do
    # locally defined (lower-case type letter other than U) listed names
    names=$("$L/llvm-nm" "$tmp/$m" 2>/dev/null | awk '$2 ~ /^[a-z]$/ && $2 != "u" {print $3}' | grep -Fxf <(echo "$want") || true)
    [ -n "$names" ] || continue
    args=()
    for n in $names; do args+=(--globalize-symbol="$n" --weaken-symbol="$n"); done
    "$L/llvm-objcopy" "${args[@]}" "$tmp/$m"
    echo "$T $m: globalized+weakened $(echo $names | wc -w) names"
done
rm -f "$OUT"; mkdir -p "$(dirname "$OUT")"
( cd "$tmp" && "$L/llvm-ar" rcsD --format="$fmt" "$OUT" "${members[@]}" )
rm -rf "$tmp"
shasum -a 256 "$IN" "$OUT"
