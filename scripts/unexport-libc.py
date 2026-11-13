#!/usr/bin/env python3
"""Make an object's C library functions local, in place, with `llvm-objcopy`.

The vendored libghostty-vt archives bundle Zig's `compiler_rt.o`, which defines
the C library's string, math and stack-protector functions (`memset`,
`memcmp`, `strlen`, `sin`, `exp`, `__stack_chk_guard`, ...) as hidden globals.
A global that a loaded archive member defines takes part in the final link, and
Apple's `ld` binds to it before libSystem, weak or not, so on macOS those copies
replaced libSystem's for the whole of spyc: its `memset` was a byte-at-a-time
loop, and `ld` reported it twice, once from `compiler_rt.o` and once from
ghostty's own (#1129). spyc always links a C library, so the archive needs none
of them. `compiler_rt.o` keeps its compiler helpers (`__udivti3`,
`__extendxftf2`, ...), which the rest of the archive needs and Rust's
`compiler_builtins` lacks in part.

The names are `crates/spyc-vt-sys/vendor/LIBC_SYMBOLS`, the one list this and
the checks share. They are derived, not chosen: a name counts as the C
library's when, without the Mach-O underscore, it has no leading `_` (`memset`,
`sinf`), or is one of the stack-protector and fortify family
(`__stack_chk_guard`, `__chk_fail`, `__memcpy_chk`). The script applies that
rule to the object's exports and stops if the result differs from the list, so
a Zig that adds or drops one is a deliberate edit to the list, not a silent
change.

This script only chooses the names and checks the result; the edit is
`llvm-objcopy --localize-symbols`, the job ghostty's `libsystem_override.sh`
does with Apple's `nmedit`, which needs a Darwin host. It clears each symbol's
external bit and writes the symbol table again with locals first, as both
formats require, remapping every relocation's symbol index (and, in Mach-O,
`LC_DYSYMTAB`) to match; references inside the object still reach the localized
copy, while no other object can name it. Mach-O support for this needs LLVM 20:
it arrived in llvm/llvm-project#120895, and LLVM 18's `llvm-objcopy` refuses
the option for Mach-O. Its output is the same on arm64 and x86_64 hosts, and
from 20.1.2 to 20.1.8.

Afterwards the script checks that no C library name is still exported and that
every other export still is, and stops rather than leave an object it could not
fix.

Usage: unexport-libc.py OBJECT...  (64-bit little-endian Mach-O or ELF)
       unexport-libc.py --listed-only OBJECT...

The first form is for `compiler_rt.o`, whose C library exports must be exactly
the list. `--listed-only` is for the archive's other objects: it localizes
whichever listed names one exports and leaves its own API alone; libghostty-vt
1.0.1 still exports its vectorized `memset` on `aarch64-unknown-linux-musl`.
The `llvm-objcopy` run is `$LLVM_OBJCOPY`, by default `llvm-objcopy-20`.
"""

import os
from pathlib import Path
import re
import struct
import subprocess
import sys
import tempfile

LIBC_PREFIXED = re.compile(r"__(stack_chk_guard|stack_chk_fail|chk_fail|[a-z]+_chk)")
LLVM_MAJOR = 20
LIST = Path(__file__).resolve().parent.parent / "crates" / "spyc-vt-sys" / "vendor" / "LIBC_SYMBOLS"


def is_libc(name):
    return not name.startswith("_") or LIBC_PREFIXED.fullmatch(name) is not None


def cstring(data, start):
    return bytes(data[start:data.index(b"\0", start)])


def macho_symbols(data):
    """(name, exported, bare name) for each symbol of a Mach-O object."""
    ncmds, off = struct.unpack_from("<I", data, 16)[0], 32
    for _ in range(ncmds):
        cmd, size = struct.unpack_from("<II", data, off)
        if cmd == 0x2:  # LC_SYMTAB
            symoff, nsyms, stroff, _ = struct.unpack_from("<IIII", data, off + 8)
            for i in range(nsyms):
                strx, n_type = struct.unpack_from("<IB", data, symoff + 16 * i)
                name = cstring(data, stroff + strx)
                # Not a debug entry, N_SECT (defined in a section), N_EXT.
                exported = n_type & 0xE0 == 0 and n_type & 0x0E == 0x0E and n_type & 0x01
                yield name, bool(exported), name[1:] if name.startswith(b"_") else name
            return
        off += size
    raise SystemExit("no LC_SYMTAB")


def elf_symbols(data):
    """(name, exported, name) for each symbol of an ELF object."""
    shoff = struct.unpack_from("<Q", data, 0x28)[0]
    shentsize, shnum = struct.unpack_from("<HH", data, 0x3A)
    headers = [shoff + i * shentsize for i in range(shnum)]
    tables = [h for h in headers if struct.unpack_from("<I", data, h + 4)[0] == 2]  # SHT_SYMTAB
    if len(tables) != 1:
        raise SystemExit(f"expected one symbol table, found {len(tables)}")
    symoff, symsize = struct.unpack_from("<QQ", data, tables[0] + 24)
    strndx = struct.unpack_from("<I", data, tables[0] + 40)[0]
    stroff = struct.unpack_from("<Q", data, headers[strndx] + 24)[0]
    for i in range(1, symsize // 24):
        st_name, st_info, _, st_shndx = struct.unpack_from("<IBBH", data, symoff + 24 * i)
        name = cstring(data, stroff + st_name)
        # GLOBAL or WEAK binding, and defined.
        yield name, st_info >> 4 in (1, 2) and st_shndx != 0, name


def is_macho(path):
    with open(path, "rb") as f:
        return f.read(4) == b"\xcf\xfa\xed\xfe"


def symbols(path):
    with open(path, "rb") as f:
        data = f.read()
    if data[:4] == b"\xcf\xfa\xed\xfe":
        return list(macho_symbols(data))
    if data[:6] == b"\x7fELF\x02\x01":
        return list(elf_symbols(data))
    raise SystemExit(f"{path}: not a 64-bit little-endian Mach-O or ELF object")


def listed(path=LIST):
    """`LIBC_SYMBOLS`'s names, without Mach-O's underscore."""
    names = [l for l in Path(path).read_text().splitlines() if l and not l.startswith("#")]
    if len(names) < 50:
        raise SystemExit(f"{path}: {len(names)} names; the list is nearly empty")
    return names


def libc_names(syms):
    """The exported names, as the object spells them, that are the C library's."""
    return sorted({name for name, exported, bare in syms if exported and is_libc(bare.decode())})


def names_to_localize(syms, names, macho, path):
    """The listed names as this object spells them, if the rule picks exactly those."""
    want = {(("_" + n) if macho else n).encode() for n in names}
    chosen = set(libc_names(syms))
    if chosen != want:
        problems = [f"exported but not listed: {n.decode()}" for n in sorted(chosen - want)]
        problems += [f"listed but not exported: {n.decode()}" for n in sorted(want - chosen)]
        raise SystemExit(f"{path}: the C library exports differ from {LIST.name}; update the list on purpose:\n  "
                         + "\n  ".join(problems))
    return sorted(want)


def exported(syms):
    return {name for name, e, _ in syms if e}


def listed_exports(syms, names, macho):
    """The listed names this object exports, as it spells them."""
    prefix = b"_" if macho else b""
    return sorted({prefix + n.encode() for n in names} & exported(syms))


def check(before, after, names, path):
    """Stop unless `after` exports exactly `before`'s exports minus `names`."""
    want = exported(before) - set(names)
    got = exported(after)
    problems = [f"still exported: {n.decode()}" for n in sorted(got - want)]
    problems += [f"no longer exported: {n.decode()}" for n in sorted(want - got)]
    if sorted(n for n, _, _ in before) != sorted(n for n, _, _ in after):
        problems.append("the set of symbol names changed")
    if problems:
        raise SystemExit(f"{path}: llvm-objcopy did not localize as expected:\n  " + "\n  ".join(problems))


def llvm_objcopy():
    tool = os.environ.get("LLVM_OBJCOPY", "llvm-objcopy-20")
    try:
        out = subprocess.run([tool, "--version"], capture_output=True, text=True, check=True).stdout
    except (OSError, subprocess.CalledProcessError) as e:
        raise SystemExit(f"{tool} not runnable ({e}); install LLVM {LLVM_MAJOR} (apt: llvm-{LLVM_MAJOR})")
    version = re.search(r"LLVM version (\d+)\.", out)
    if not version or int(version.group(1)) != LLVM_MAJOR:
        raise SystemExit(f"{tool} is not LLVM {LLVM_MAJOR}:\n{out}")
    return tool


def unexport(path, tool, listed_names, listed_only=False):
    """Localize `path`'s exported C library symbols, as listed; returns their names.

    `listed_only` is for the archive's other objects, which export their own API
    beside any C library function: it localizes whichever listed names the object
    exports, if any, without requiring the rule to pick exactly the list."""
    before = symbols(path)
    if listed_only:
        names = listed_exports(before, listed_names, is_macho(path))
        if not names:
            return []
    else:
        names = names_to_localize(before, listed_names, is_macho(path), path)
    # Beside the object, so the result replaces it with a rename on one filesystem.
    with tempfile.TemporaryDirectory(dir=os.path.dirname(os.path.abspath(path))) as tmp:
        listing, out = os.path.join(tmp, "names.txt"), os.path.join(tmp, "out.o")
        with open(listing, "wb") as f:
            f.write(b"".join(n + b"\n" for n in names))
        subprocess.run([tool, f"--localize-symbols={listing}", path, out], check=True)
        check(before, symbols(out), names, path)
        os.replace(out, path)
    return [n.decode() for n in names]


if __name__ == "__main__":
    if len(sys.argv) < 2:
        raise SystemExit(__doc__)
    only = sys.argv[1] == "--listed-only"
    objcopy, names = llvm_objcopy(), listed()
    for p in sys.argv[2 if only else 1:]:
        done = unexport(p, objcopy, names, listed_only=only)
        if done or not only:
            print(f"{p}: localized {len(done)} C library names: {' '.join(done) if only else '...'}")
