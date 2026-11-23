#!/usr/bin/env bash
# Fail unless a glibc Linux spyc binary takes the C library functions in
# crates/spyc-vt-sys/vendor/LIBC_SYMBOLS from the C library.
#
# No listed name may be defined in the binary as a global, and the five spyc's
# own code always calls must be imports from glibc, `memset@GLIBC_2.2.5`. The
# one exception is Rust's own `compiler_builtins`, which defines some of the
# maths functions (`ceilf`, `fmaf`) as weak globals on Linux: a listed name
# the binary defines passes when `compiler_builtins` for the target defines it
# at the same size. The vendored archive's copies differ in size.
#
# SPYC_RUST_TARGET names the target when it isn't rustc's host.
#
# The musl targets link statically, so every function is defined inside the
# binary and its name says nothing about where it came from: ARCHITECTURE.md
# (vendored-libc-symbols) records how those were checked.
#
# usage: check-linux-libc-binding.sh BINARY
set -euo pipefail

bin="${1:?usage: check-linux-libc-binding.sh BINARY}"
list="$(cd "$(dirname "$0")/.." && pwd)/crates/spyc-vt-sys/vendor/LIBC_SYMBOLS"
[ "$(grep -cv '^#' "$list")" -gt 50 ] || { echo "too few names in $list"; exit 1; }

# Fail closed: a missing binary, or one not linked against glibc, is an error.
[ -f "$bin" ] || { echo "no such binary: $bin"; exit 1; }
needed=$(readelf -d "$bin") || { echo "readelf cannot read $bin"; exit 1; }
grep -q 'Shared library: \[libc\.so' <<<"$needed" || {
    echo "$bin is not dynamically linked against glibc"
    exit 1
}

# name -> size for each global `compiler_builtins` defines.
sysroot=$(rustc --print sysroot)
target="${SPYC_RUST_TARGET:-$(rustc -vV | sed -n 's/^host: //p')}"
builtins=( "$sysroot/lib/rustlib/$target/lib/"libcompiler_builtins-*.rlib )
[ -f "${builtins[0]}" ] || { echo "no compiler_builtins for $target under $sysroot"; exit 1; }
rust_own=$(nm -P "${builtins[@]}" 2>/dev/null | awk '$2 ~ /^[A-TV-Z]$/ { print $1, $4 }')
[ -n "$rust_own" ] || { echo "nm found no globals in ${builtins[*]}"; exit 1; }

# Every symbol nm shows, as "name type": the dynamic table always, and the full
# table when the binary still has one.
symbols=$( {
    nm -D -P "$bin"
    nm -P "$bin" 2>/dev/null || true
} | awk '{ print $1, $2, $4 }')

status=0
defined=$(awk '
    FILENAME == ARGV[1] { if ($0 !~ /^#/) want[$0] = 1; next }
    FILENAME == ARGV[2] { rust[$1] = $2; next }
    { name = $1; sub(/@.*/, "", name) }
    (name in want) && $2 ~ /^[A-TV-Z]$/ && rust[name] != $3 { print "    " $1 " " $2 " size " $3 }' \
    "$list" <(echo "$rust_own") - <<<"$symbols" | sort -u)
if [ -n "$defined" ]; then
    echo "defined inside the binary:"
    echo "$defined"
    status=1
fi
for name in memset memcpy memmove memcmp strlen; do
    if ! grep -qE "^$name@GLIBC_[0-9.]+ U ?$" <<<"$symbols"; then
        echo "$name is not imported from glibc"
        status=1
    fi
done
[ "$status" = 0 ] && echo "$bin: no function in LIBC_SYMBOLS is defined inside it, and memset, memcpy, memmove, memcmp and strlen come from glibc"
exit "$status"
