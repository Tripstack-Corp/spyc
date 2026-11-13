#!/usr/bin/env bash
# Reproduce every observation in README.md. Usage: ./run.sh [WORK_DIR]
#
# Downloads ziglang.org's builds of five Zig releases for this host, each
# checked against the sha256 ziglang.org publishes, into WORK_DIR (a fresh
# temporary directory by default), and builds the repro with each. The last
# section links with Apple's toolchain and runs only on macOS.
set -euo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
if [ -n "${1:-}" ]; then
    mkdir -p "$1"
    WORK=$(cd "$1" && pwd)
else
    WORK=$(mktemp -d)
fi
VERSIONS="0.13.0 0.14.1 0.15.2 0.16.0 0.17.1"
DUMP="python3 $HERE/dump.py"

case "$(uname -s)-$(uname -m)" in
    Darwin-arm64) host=aarch64-macos ;;
    Darwin-x86_64) host=x86_64-macos ;;
    Linux-aarch64) host=aarch64-linux ;;
    Linux-x86_64) host=x86_64-linux ;;
    *) echo "unsupported host $(uname -sm)"; exit 1 ;;
esac
echo "host: $(uname -sm), work: $WORK"

curl -fsSL https://ziglang.org/download/index.json -o "$WORK/index.json"
for v in $VERSIONS; do
    [ -x "$WORK/zig-$v/zig" ] && continue
    read -r url sum < <(python3 -c '
import json, sys
d = json.load(open(sys.argv[1]))[sys.argv[2]][sys.argv[3]]
print(d["tarball"], d["shasum"])' "$WORK/index.json" "$v" "$host")
    curl -fsSL "$url" -o "$WORK/zig-$v.tar.xz"
    echo "$sum  $WORK/zig-$v.tar.xz" | shasum -a 256 -c - >/dev/null
    mkdir -p "$WORK/zig-$v"
    tar -xJf "$WORK/zig-$v.tar.xz" -C "$WORK/zig-$v" --strip-components=1
done

# 0.13 spells the C calling convention `.C` and `@export` takes the function,
# not a pointer to it.
src_for() { [ "$1" = 0.13.0 ] && echo "$HERE/lib-0.13.zig" || echo "$HERE/lib.zig"; }

echo
echo "== 1. one weak, one weak hidden and one strong hidden export, per version"
for v in $VERSIONS; do
    echo "-- zig $("$WORK/zig-$v/zig" version)"
    for t in aarch64-macos x86_64-macos x86_64-linux; do
        d="$WORK/out/$v/$t"
        mkdir -p "$d"
        ( cd "$d" && "$WORK/zig-$v/zig" build-obj -target "$t" -O ReleaseSmall \
            -femit-bin="$d/lib.o" -femit-llvm-ir="$d/lib.ll" -femit-asm="$d/lib.s" "$(src_for "$v")" )
        $DUMP "$d/lib.o" | sed "s|$WORK/out/||"
    done
    echo "   LLVM IR (aarch64-macos):"
    grep -hE '^@(weak_|strong_)|^define .*@(weak_|strong_)' "$WORK/out/$v/aarch64-macos/lib.ll" | sed 's/^/     /'
done

echo
echo "== 2. the assembly LLVM writes for aarch64-macos"
for v in 0.15.2 0.17.1; do
    echo "-- zig $v"
    grep -nE 'weak_|strong_' "$WORK/out/$v/aarch64-macos/lib.s" | sed 's/^/   /'
done

echo
echo "== 3. zig 0.17.1 without LLVM (-fno-llvm), aarch64-macos"
mkdir -p "$WORK/out/nollvm"
( cd "$WORK/out/nollvm" && "$WORK/zig-0.17.1/zig" build-obj -target aarch64-macos -fno-llvm \
    -femit-bin="$WORK/out/nollvm/lib.o" "$HERE/lib.zig" )
$DUMP "$WORK/out/nollvm/lib.o" | sed "s|$WORK/out/||"

echo
echo "== 4. the same three in C, through zig 0.17.1's clang (zig cc), aarch64-macos"
"$WORK/zig-0.17.1/zig" cc -target aarch64-macos -Os -c "$HERE/lib.c" -o "$WORK/out/clang.o"
$DUMP "$WORK/out/clang.o" | sed "s|$WORK/out/||"
"$WORK/zig-0.17.1/zig" cc --version | sed -n 1p

echo
echo "== 5. compiler_rt bundled into a static library (-fcompiler-rt), aarch64-macos"
for v in 0.15.2 0.17.1; do
    d="$WORK/crt/$v"
    mkdir -p "$d/members"
    ( cd "$d" && "$WORK/zig-$v/zig" build-lib -target aarch64-macos -O ReleaseSmall -fcompiler-rt \
        -femit-bin="$d/liblib.a" "$(src_for "$v")" )
    ( cd "$d/members" && "$WORK/zig-0.17.1/zig" ar x "$d/liblib.a" && chmod 644 ./*.o )
    echo "-- zig $v"
    $DUMP --match '^(memset|memcpy|memmove|memcmp|strlen)$' "$d/members/compiler_rt.o" | sed "s|$WORK/crt/||"
done

echo
if [ "$(uname -s)" != Darwin ]; then
    echo "== 6. skipped: linking with Apple's ld needs macOS"
    exit 0
fi
echo "== 6. where Apple's ld binds memset: a C main linked against each archive"
ld -v 2>&1 | sed -n 1p
for v in 0.15.2 0.17.1; do
    d="$WORK/crt/$v"
    # Apple's ld rejects 0.15.2's archive as written (a member not 8-byte
    # aligned), so both are re-archived, members unchanged, by Apple's libtool.
    ( cd "$d/members" && libtool -static -o "$d/liblib-apple.a" ./*.o 2>/dev/null )
    echo "-- zig $v archive: $(nm -m "$d/liblib-apple.a" | grep -E ' _memset$' | sed 's/^[0-9a-f]* //')"
    for m in main_uses_lib main_memset_only; do
        cc -O1 "$HERE/control/$m.c" "$d/liblib-apple.a" -o "$d/$m"
        echo "   $m: $(nm -m "$d/$m" | grep -E ' _memset( |$)' | sed 's/^[0-9a-f]* *//')"
    done
done
