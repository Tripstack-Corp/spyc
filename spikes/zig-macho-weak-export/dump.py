"""Print chosen symbols of Mach-O and ELF objects: Mach-O `n_type`/`n_desc`, ELF binding/visibility.

usage: dump.py [--match REGEX] OBJECT...

Without `--match`, the repro's three exports (`weak_default`, `weak_hidden`,
`strong_hidden`). Mach-O names carry their leading `_`; the pattern is matched
against the name without it.
"""

import re
import struct
import sys

N_EXT, N_PEXT, N_TYPE, N_SECT, N_WEAK_DEF = 0x01, 0x10, 0x0E, 0x0E, 0x0080


def macho(data, path, match):
    ncmds, off = struct.unpack_from("<I", data, 16)[0], 32
    for _ in range(ncmds):
        cmd, size = struct.unpack_from("<II", data, off)
        if cmd == 0x2:  # LC_SYMTAB
            symoff, nsyms, stroff, _ = struct.unpack_from("<IIII", data, off + 8)
            for i in range(nsyms):
                strx, n_type, _, n_desc, _ = struct.unpack_from("<IBBHQ", data, symoff + 16 * i)
                name = data[stroff + strx:data.index(b"\0", stroff + strx)].decode()
                if n_type & N_TYPE != N_SECT or not match.search(name.removeprefix("_")):
                    continue
                if not n_type & N_EXT:
                    kind = "non-external"
                else:
                    kind = "private external" if n_type & N_PEXT else "external"
                weak = "weak " if n_desc & N_WEAK_DEF else ""
                print(f"  {path}: {name:16} n_type={n_type:#04x} n_desc={n_desc:#06x}  {weak}{kind}")
        off += size


def elf(data, path, match):
    shoff = struct.unpack_from("<Q", data, 0x28)[0]
    shentsize, shnum = struct.unpack_from("<HH", data, 0x3A)
    headers = [struct.unpack_from("<IIQQQQIIQQ", data, shoff + i * shentsize) for i in range(shnum)]
    for h in headers:
        if h[1] != 2:  # SHT_SYMTAB
            continue
        stroff = headers[h[6]][4]
        for i in range(1, h[5] // 24):
            st_name, st_info, st_other, st_shndx = struct.unpack_from("<IBBH", data, h[4] + 24 * i)
            name = data[stroff + st_name:data.index(b"\0", stroff + st_name)].decode()
            if st_shndx == 0 or not match.search(name):
                continue
            bind = {0: "LOCAL", 1: "GLOBAL", 2: "WEAK"}.get(st_info >> 4, st_info >> 4)
            vis = {0: "DEFAULT", 1: "INTERNAL", 2: "HIDDEN", 3: "PROTECTED"}[st_other & 3]
            print(f"  {path}: {name:16} {bind} {vis}")


def main(argv):
    match = re.compile(r"^(weak_|strong_)")
    if argv[:1] == ["--match"]:
        match, argv = re.compile(argv[1]), argv[2:]
    for path in argv:
        data = open(path, "rb").read()
        if data[:4] == b"\xcf\xfa\xed\xfe":
            macho(data, path, match)
        elif data[:6] == b"\x7fELF\x02\x01":
            elf(data, path, match)
        else:
            raise SystemExit(f"{path}: not a 64-bit little-endian Mach-O or ELF object")


if __name__ == "__main__":
    main(sys.argv[1:])
