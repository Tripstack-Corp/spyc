#!/usr/bin/env bash
# Fail unless a macOS spyc binary takes the C library functions in
# crates/spyc-vt-sys/vendor/LIBC_SYMBOLS from libSystem, in every architecture
# it holds.
#
# No listed name may be defined in the binary, and every listed name it refers
# to must be an import, `(undefined) external _memset (from libSystem)`. The
# five spyc's own code always calls must be there as imports: that is what makes
# this work on a stripped release build, where a copy defined inside the binary
# leaves no symbol at all, only a missing import.
#
# usage: check-libsystem-binding.sh BINARY
set -euo pipefail

bin="${1:?usage: check-libsystem-binding.sh BINARY}"
list="$(cd "$(dirname "$0")/.." && pwd)/crates/spyc-vt-sys/vendor/LIBC_SYMBOLS"
[ "$(grep -cv '^#' "$list")" -gt 50 ] || { echo "too few names in $list"; exit 1; }

# Fail closed: a missing or unreadable binary is an error, never zero archs.
[ -f "$bin" ] || { echo "no such binary: $bin"; exit 1; }
archs=$(lipo -archs "$bin") || { echo "lipo cannot read $bin"; exit 1; }
[ -n "$archs" ] || { echo "no architectures in $bin"; exit 1; }

status=0
for arch in $archs; do
    # name -> "import" or "defined", for every listed name nm shows.
    found=$(nm -m -arch "$arch" "$bin" | awk '
        NR == FNR { if ($0 !~ /^#/) want["_" $0] = 1; next }
        {
            for (i = 1; i <= NF; i++) if ($i in want) {
                if ($0 ~ /\(undefined\) external .* \(from libSystem\)$/) print $i, "import"
                else print $i, "defined: " $0
            }
        }' "$list" -)
    if grep -q ' defined: ' <<<"$found"; then
        echo "$arch: defined inside the binary:"
        grep ' defined: ' <<<"$found" | sed 's/^[^ ]* defined: /    /'
        status=1
    fi
    for name in _memset _memcpy _memmove _memcmp _strlen; do
        if ! grep -qx "$name import" <<<"$found"; then
            echo "$arch: $name is not imported from libSystem"
            status=1
        fi
    done
done
[ "$status" = 0 ] && echo "$bin: no function in LIBC_SYMBOLS is defined inside it, and memset, memcpy, memmove, memcmp and strlen come from libSystem ($archs)"
exit "$status"
