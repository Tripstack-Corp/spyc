"""The vendored libghostty-vt archives leave the C library's functions to the C library.

A global that a loaded archive member defines takes part in spyc's final link,
and Apple's `ld` binds to it before libSystem, weak or not. Zig's bundled
`compiler_rt.o` defined `memset`, `memcmp`, `strlen`, the maths functions and
`__stack_chk_guard`, and so on macOS spyc ran a byte-at-a-time `memset`, and
`ld` also reported `memset` twice, `compiler_rt.o`'s and ghostty's own (#1129).
"""

from pathlib import Path
import struct
import unittest

VENDOR = Path(__file__).resolve().parent.parent / "crates" / "spyc-vt-sys" / "vendor"
TARGETS = [
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "x86_64-unknown-linux-gnu",
    "x86_64-unknown-linux-musl",
    "aarch64-unknown-linux-musl",
]


def listed():
    """The C library names in `vendor/LIBC_SYMBOLS`, which the vendoring tool localizes."""
    names = [l for l in (VENDOR / "LIBC_SYMBOLS").read_text().splitlines() if l and not l.startswith("#")]
    assert len(names) > 50, "LIBC_SYMBOLS is nearly empty"
    return set(names)


def members(archive):
    """{name: bytes} for each object in a BSD (darwin) or GNU `ar` archive."""
    data = archive.read_bytes()
    assert data[:8] == b"!<arch>\n", archive
    out, longnames, off = {}, b"", 8
    while off < len(data):
        name = data[off:off + 16].decode().rstrip()
        size = int(data[off + 48:off + 58])
        body = data[off + 60:off + 60 + size]
        off += 60 + size + (size & 1)
        if name.startswith("#1/"):  # BSD: the name leads the body
            n = int(name[3:])
            name, body = body[:n].rstrip(b"\0").decode(), body[n:]
        elif name == "//":
            longnames = body
            continue
        elif name.startswith("/") and name[1:].isdigit():  # GNU: offset into `//`
            start = int(name[1:])
            name = longnames[start:longnames.index(b"/\n", start)].decode()
        else:
            name = name.rstrip("/")
        if name.endswith(".o"):
            out[name] = body
    return out


def exported(obj):
    """The names a Mach-O or ELF object defines as globals, without Mach-O's underscore."""
    names = []
    if obj[:4] == b"\xcf\xfa\xed\xfe":
        ncmds, off = struct.unpack_from("<I", obj, 16)[0], 32
        for _ in range(ncmds):
            cmd, size = struct.unpack_from("<II", obj, off)
            if cmd == 0x2:  # LC_SYMTAB
                symoff, nsyms, stroff, _ = struct.unpack_from("<IIII", obj, off + 8)
                for i in range(nsyms):
                    strx, n_type = struct.unpack_from("<IB", obj, symoff + 16 * i)
                    if n_type & 0xE0 == 0 and n_type & 0x0E == 0x0E and n_type & 0x01:
                        names.append(obj[stroff + strx:obj.index(b"\0", stroff + strx)][1:].decode())
            off += size
    else:
        assert obj[:6] == b"\x7fELF\x02\x01"
        shoff = struct.unpack_from("<Q", obj, 0x28)[0]
        shentsize, shnum = struct.unpack_from("<HH", obj, 0x3A)
        headers = [struct.unpack_from("<IIQQQQIIQQ", obj, shoff + i * shentsize) for i in range(shnum)]
        for h in headers:
            if h[1] == 2:  # SHT_SYMTAB
                stroff = headers[h[6]][4]
                for i in range(1, h[5] // 24):
                    st_name, st_info, _, st_shndx = struct.unpack_from("<IBBH", obj, h[4] + 24 * i)
                    if st_info >> 4 in (1, 2) and st_shndx != 0:  # GLOBAL or WEAK, defined
                        names.append(obj[stroff + st_name:obj.index(b"\0", stroff + st_name)].decode())
    return names


class VendoredArchives(unittest.TestCase):
    def setUp(self):
        self.archives = {t: members(VENDOR / t / "libghostty-vt.a") for t in TARGETS}
        for target, objs in self.archives.items():
            self.assertIn("compiler_rt.o", objs, target)
            self.assertIn("ghostty-vt-static_zcu.o", objs, target)
            self.assertGreater(len(exported(objs["compiler_rt.o"])), 100, target)

    def memset_owners(self, target):
        objs = self.archives[target]
        return [m for m, obj in sorted(objs.items()) if "memset" in exported(obj)]

    def test_no_darwin_archive_defines_memset(self):
        for target in TARGETS:
            if target.endswith("-apple-darwin"):
                self.assertEqual(self.memset_owners(target), [], target)

    def test_memset_is_at_most_ghosttys_own(self):
        for target in TARGETS:
            self.assertIn(self.memset_owners(target), ([], ["ghostty-vt-static_zcu.o"]), target)

    def test_compiler_rt_defines_no_c_library_function(self):
        names = listed()
        for target, objs in self.archives.items():
            self.assertEqual(sorted(names.intersection(exported(objs["compiler_rt.o"]))), [], target)

    def test_no_archive_member_exports_a_listed_name(self):
        names = listed()
        for target, objs in self.archives.items():
            for member, obj in sorted(objs.items()):
                self.assertEqual(sorted(names.intersection(exported(obj))), [], f"{target} {member}")

    def test_no_darwin_archive_exports_a_listed_name(self):
        names = listed()
        for target, objs in self.archives.items():
            if not target.endswith("-apple-darwin"):
                continue
            for member, obj in sorted(objs.items()):
                self.assertEqual(sorted(names.intersection(exported(obj))), [], f"{target} {member}")


if __name__ == "__main__":
    unittest.main()
