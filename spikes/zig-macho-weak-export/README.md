# Zig, Mach-O and weak exports: the record

Why spyc's vendored libghostty-vt archive defined `memset` twice on macOS, and
what does and doesn't follow from it. This is spyc's own record, kept with the
commands that show each fact. It is not an upstream report: Zig's code of
conduct does not accept bugs found by an LLM or text an LLM wrote, and none of
this has been filed anywhere.

`./run.sh [WORK_DIR]` reproduces sections 1 to 6 below. It downloads
ziglang.org's builds of Zig 0.13.0, 0.14.1, 0.15.2, 0.16.0 and 0.17.1 for the
host, each checked against the sha256 ziglang.org publishes, and builds
[`lib.zig`](lib.zig) (with [`lib-0.13.zig`](lib-0.13.zig), the same source in
0.13's syntax), [`lib.c`](lib.c) and the two [`control/`](control/) programs.
[`dump.py`](dump.py) reads the symbol tables directly, so a Linux host needs no
Mach-O tools; section 6 links with Apple's toolchain and runs only on macOS.
The outputs quoted here are from a run on macOS 26.7.2 (arm64) on 2026-11-13; a
run in an `ubuntu:24.04` container (Linux aarch64) on the same day printed
sections 1 to 5 byte for byte the same.

## What happens

[`lib.zig`](lib.zig) exports three functions: `weak_default` (weak),
`weak_hidden` (weak, hidden visibility) and `strong_hidden` (hidden).

**1. Zig 0.16.0 and later write a weak export to Mach-O without its weak
flag, and a weak export of default visibility as a local symbol.** 0.13.0 to
0.15.2 are correct. ELF output is correct in every version.

```
-- zig 0.15.2
  0.15.2/aarch64-macos/lib.o: _strong_hidden   n_type=0x1f n_desc=0x0000  private external
  0.15.2/aarch64-macos/lib.o: _weak_default    n_type=0x0f n_desc=0x0080  weak external
  0.15.2/aarch64-macos/lib.o: _weak_hidden     n_type=0x1f n_desc=0x0080  weak private external
  ...
  0.15.2/x86_64-linux/lib.o: weak_default     WEAK DEFAULT
  0.15.2/x86_64-linux/lib.o: weak_hidden      WEAK HIDDEN
-- zig 0.16.0
  0.16.0/aarch64-macos/lib.o: _weak_default    n_type=0x0e n_desc=0x0000  non-external
  0.16.0/aarch64-macos/lib.o: _strong_hidden   n_type=0x1f n_desc=0x0000  private external
  0.16.0/aarch64-macos/lib.o: _weak_hidden     n_type=0x1f n_desc=0x0000  private external
  ...
  0.16.0/x86_64-linux/lib.o: weak_default     WEAK DEFAULT
  0.16.0/x86_64-linux/lib.o: weak_hidden      WEAK HIDDEN
```

`x86_64-macos` matches `aarch64-macos` in every version, and 0.17.1 matches
0.16.0. `n_desc` `0x0080` is `N_WEAK_DEF`; `n_type` `0x0e` has no `N_EXT`.

**The LLVM IR changed between 0.15.2 and 0.16.0:** a weak export went from a
weak definition to a weak alias of a private function.

```
-- zig 0.15.2
     define weak_odr dso_local i32 @weak_default(i32 %0) local_unnamed_addr #0 {
     define weak_odr hidden i32 @weak_hidden(i32 %0) local_unnamed_addr #0 {
-- zig 0.16.0
     @weak_default = weak_odr alias i32 (i32), ptr @lib.a
     @weak_hidden = weak_odr hidden alias i32 (i32), ptr @lib.b
```

The Zig change is `d28c5069b` "llvm: rework handling of globals" (2026-03-24),
the one commit between the two releases that adds or removes `addAlias` in
`src/codegen/llvm.zig`. Its message says "lean into aliases for exports", and
its diff replaces "Successive exports become aliases" with "All exports are
represented as aliases to the original global":

```sh
git clone --filter=blob:none --no-checkout https://codeberg.org/ziglang/zig.git
git -C zig log --format='%h %ad %s' --date=short -S'addAlias' 0.15.2..0.16.0 -- src/codegen/llvm.zig
# d28c5069b 2026-03-24 llvm: rework handling of globals
```

**2. LLVM writes such an alias to Mach-O as a weak reference, not a weak
definition.** Section 2 of `run.sh`:

```
-- zig 0.15.2
   3:	.globl	_weak_default
   4:	.weak_definition	_weak_default
   ...
-- zig 0.17.1
   27:	.weak_reference _weak_default
   28:	_weak_default = l_lib.a
   29:	.weak_reference _weak_hidden
   30:	.private_extern	_weak_hidden
   31:	_weak_hidden = l_lib.b
```

There is no `.globl` and no `.weak_definition`. This is
[llvm/llvm-project#229283](https://github.com/llvm/llvm-project/issues/229283),
"Weak aliases to private storage are emitted as local symbols"; its sibling
is #229282. The same three functions written in C and compiled by Zig 0.17.1's
own clang (`clang version 22.1.8`) come out correct, because clang emits a weak
definition, not an alias:

```
  clang.o: _strong_hidden   n_type=0x1f n_desc=0x0000  private external
  clang.o: _weak_default    n_type=0x0f n_desc=0x0080  weak external
  clang.o: _weak_hidden     n_type=0x1f n_desc=0x0080  weak private external
```

**3. Without LLVM (`-fno-llvm`) the weak flag survives, but no export keeps
hidden visibility**, `strong_hidden` included. That is a separate defect:

```
  nollvm/lib.o: _weak_default    n_type=0x0f n_desc=0x0080  weak external
  nollvm/lib.o: _weak_hidden     n_type=0x0f n_desc=0x0080  weak external
  nollvm/lib.o: _strong_hidden   n_type=0x0f n_desc=0x0000  external
```

**4. compiler_rt exports through the same path**, so in a static library
built with it (`-fcompiler-rt`) its C library functions lose their weak flag
from 0.16.0 on:

```
-- zig 0.15.2
  0.15.2/members/compiler_rt.o: _memset          n_type=0x1f n_desc=0x0080  weak private external
-- zig 0.17.1
  0.17.1/members/compiler_rt.o: _memset          n_type=0x1f n_desc=0x0000  private external
```

## What it did to spyc

libghostty-vt 1.0.0 (`0fa3d577`) exports its own `memset` (`quirks_memset.zig`)
as weak, beside compiler_rt's. Under Zig 0.17.0 both are strong, so every macOS
link of spyc printed, with `--force-warn linker_messages` (`ld-27037.1`):

```
warning: linker stderr: ld: duplicate symbol '_memset' in:
         .../libspyc_vt_sys-827506ec7968117a.rlib[7](compiler_rt.o)
         .../libspyc_vt_sys-827506ec7968117a.rlib[8](ghostty-vt-static_zcu.o)
```

`ld` reports it as a warning (`ld: warning: duplicate symbol '_memset' in:` when
the link command is run directly, exit status 0) and keeps one copy:
compiler_rt's, the member listed first. In spyc's debug build at `f93f52f2` all
five functions are defined inside spyc, and `memset` is a byte-at-a-time loop:

```
$ B=$(make -s target-dir)/debug/spyc
$ nm -m $B | grep -E ' _(memset|memcpy|memmove|memcmp|strlen)( |$)'
0000000101266c44 (__TEXT,__text) non-external (was a private external) _memcmp
0000000101266d20 (__TEXT,__text) non-external (was a private external) _memcpy
0000000101266cdc (__TEXT,__text) non-external (was a private external) _memmove
0000000101266c68 (__TEXT,__text) non-external (was a private external) _memset
0000000101266c2c (__TEXT,__text) non-external (was a private external) _strlen
$ otool -tV -p _memset $B
_memset:
0000000101266c68	cbz	x2, 0x101266c7c
0000000101266c6c	mov	x8, x0
0000000101266c70	strb	w1, [x8], #0x1
0000000101266c74	subs	x2, x2, #0x1
0000000101266c78	b.ne	0x101266c70
```

## What it cost: macOS arm64, timed

Does spyc run measurably faster with libSystem's memory and string functions
than with the archive's copies? Two release builds of main `cd413b07` for
`aarch64-apple-darwin`, differing only in the vendored archive:

- **A**, main's archive as committed: all five bind to libSystem.
- **B**, main's archive with the localize step undone by
  [`libc-bench/scripts/globalize.sh`](libc-bench/scripts/globalize.sh):
  `llvm-objcopy --globalize-symbol --weaken-symbol` (Homebrew LLVM 22.1.8) on
  each name in `vendor/LIBC_SYMBOLS` that a member defines as a local symbol,
  then the members re-archived in their original order. In `compiler_rt.o`, the
  92 listed names go from `91 t` and `1 d` to `91 T` and `1 D` in `llvm-nm`;
  the member list and order are identical. The archives' SHA-256 are
  `8f08c8a7…a752` (A) and `1568b306…8514` (B).

B was made this way rather than rebuilt from the pin without the localize step
because the rebuild failed: Zig's package fetch broke in the build container in
three ways, recorded in #1153. B's code bytes are A's; only symbol binding and
visibility differ. It was not compared with a real rebuild.

The five bind differently in the two binaries (`nm -m spyc | grep -E
' _(memset|memcpy|memmove|memcmp|strlen)( |$)'`):

```
A:                 (undefined) external _memset (from libSystem)
B: 0000000100f6bcb4 (__TEXT,__text) non-external (was a private external) _memset
```

and the same for `memcpy`, `memmove`, `memcmp` and `strlen`.

[`libc-bench/scripts/build.sh`](libc-bench/scripts/build.sh) builds spyc and
`spikes/vt-engine`'s `bench` (`--features ghostty`) with `cargo build --release`,
strip off. Three workloads, all from what the repo already has:

- **engine**: `bench 800`, which feeds `big-spew.bin` through libghostty-vt and
  through vt100 (pure Rust) in 8 KiB chunks, 800 times, and reports each
  engine's time.
- **bigdir**: spyc started in a pty in a folder of 100,000 empty files with
  `-c quit -c quit`, wall time from fork to exit
  ([`drive.py`](libc-bench/scripts/drive.py)).
- **merge**: `spyc --merge-driver` on 1,000,000-line base, ours and theirs files
  (about 50 MB each) whose edits don't overlap
  ([`mkdata.py`](libc-bench/scripts/mkdata.py)).

[`timeit.py`](libc-bench/scripts/timeit.py) runs 2 warm-up rounds of each
binary, then 10 rounds that alternate the order (A then B, B then A).
[`macos-session.sh`](libc-bench/scripts/macos-session.sh) ran it on
2026-11-14, 06:42 to 06:43, on Darwin 25.6.0 arm64, on battery at 100% and under
`caffeinate -i`. Before timing, `pgrep -fl 'zig|cargo|rustc|clang|cc1plus|ld64|podman|vfkit'`
showed only the idle podman VM. Seconds, lower is better:

| workload | A mean | A sd | A min | B mean | B sd | B min | B/A | Welch t |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| engine, libghostty-vt | 0.4129 | 0.0079 | 0.4020 | 0.4108 | 0.0120 | 0.3990 | 0.99 | -0.46 |
| engine, vt100 | 1.2354 | 0.0233 | 1.1940 | 1.7840 | 0.0490 | 1.7320 | 1.44 | 32.00 |
| bigdir | 0.1843 | 0.0261 | 0.1568 | 0.2066 | 0.0240 | 0.1752 | 1.12 | 1.99 |
| merge | 0.2575 | 0.0102 | 0.2381 | 0.4354 | 0.0285 | 0.3947 | 1.69 | 18.57 |

With libSystem's functions, the vt100 engine run is 44% faster and the 50 MB
merge 69% faster. libghostty-vt's throughput is the same, and so is starting in
the 100,000-file folder: its 12% gap is smaller than one standard deviation.

The raw runs are
[`libc-bench/raw-macos-arm64.tsv`](libc-bench/raw-macos-arm64.tsv), SHA-256
`3b315afed9b0a681ad0620486147e728fd59f40568d5d2b8fc64d2307c2a052e`. The file the
session wrote, `60f8cc772974528021c1b7eaace3019a9196367adf762ed72dfe325181b31378`,
differs only in the binaries' local path, which the copy replaces with
`$BENCH`. The scripts are the copies that ran, with local paths replaced by
`${BENCH:?}`, `${SPYC_B:?}`, `${SPYC_MAIN:?}` and `${OUT:?}`.

## What does not follow from it

**The weak flag does not decide whether libSystem is used.** Section 6 links
the same C program, which calls `memset`, against a 0.15.2 archive (weak
`memset`) and a 0.17.1 one (strong `memset`). Apple's `ld` binds to the
archive's copy in all four links, whether or not the program calls into the
library:

```
-- zig 0.15.2 archive: (__TEXT,__text) weak private external _memset
   main_uses_lib: (__TEXT,__text) non-external (was a private external) _memset
   main_memset_only: (__TEXT,__text) non-external (was a private external) _memset
-- zig 0.17.1 archive: (__TEXT,__text) private external _memset
   main_uses_lib: (__TEXT,__text) non-external (was a private external) _memset
   main_memset_only: (__TEXT,__text) non-external (was a private external) _memset
```

So bundling compiler_rt is what keeps a program off libSystem's `memset`,
under any Zig. The weak-flag bug turns two copies into a duplicate and decides
which copy wins.

## The fixes, and where they live

- **ghostty `9899c07`** "build: don't export the memset quirk on Darwin"
  (2026-11-10, in `vt-1.0.1`). `quirks_memset.zig`'s "Why Darwin is excluded"
  describes this bug.
- **ghostty's `libsystem_override.sh`** localizes compiler_rt's C library
  symbols with Apple's `nmedit`. `LibsystemOverrideStep.zig` makes it a no-op
  unless both the target and the build host are Darwin, and spyc builds its
  archives on Linux, so it has never run for spyc.
- **spyc's `scripts/unexport-libc.py`** does that job on a Linux host: LLVM
  20's `llvm-objcopy --localize-symbols` makes the C library symbols listed in
  `crates/spyc-vt-sys/vendor/LIBC_SYMBOLS` local in compiler_rt.o. LLVM 18's
  refuses the option for Mach-O (`option is not supported for MachO`).

## Upstream status, 2026-11-13

```sh
for n in 229283 229282 111321 231380 198148 212009; do
  gh api repos/llvm/llvm-project/issues/$n \
    --jq '"#\(.number) \(.state) merged=\(.pull_request.merged_at // "-") \(.title)"'
done
```

| | state | |
|---|---|---|
| #229283 | open | Weak aliases to private storage are emitted as local symbols |
| #229282 | open | Explicit weak-definition flags are lost on aliases to strong symbols |
| #111321 | open | Weak aliases on MacOS aren't made global/public |
| PR #231380 | open | Emit definition linkage for global aliases |
| PR #198148 | merged 2026-07-22, reverted 2026-07-25 by #212009 | Preserve weak linkage for aliases |

#198148 merged after the 23.x branch point (`llvmorg-24-init`, 2026-07-14)
and was reverted three days later, so it is in no release.
