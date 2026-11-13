"""Tests for scripts/unexport-libc.py's choice of names and its check of
`llvm-objcopy`'s result, on small Mach-O and ELF objects built here."""

import importlib.util
from pathlib import Path
import struct
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location(
    "unexport_libc", Path(__file__).with_name("unexport-libc.py"))
tool = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(tool)

# (name, kind): "global" is defined and exported, "local" defined and not,
# "undef" a reference.
SYMBOLS = [
    ("memset", "global"),
    ("__stack_chk_guard", "global"),
    ("__memcpy_chk", "global"),
    ("__udivti3", "global"),
    ("sinf", "local"),
    ("strlen", "undef"),
]


def strings(symbols):
    table, at = bytearray(b"\0"), []
    for name, _ in symbols:
        at.append(len(table))
        table += name.encode() + b"\0"
    return bytes(table), at


def macho_obj(symbols):
    """A Mach-O object holding only an LC_SYMTAB; Mach-O names carry a leading `_`."""
    strtab, at = strings([("_" + n, k) for n, k in symbols])
    symoff = 32 + 24
    stroff = symoff + 16 * len(symbols)
    out = struct.pack("<IiiIIIII", 0xFEEDFACF, 0x0100000C, 0, 1, 1, 24, 0, 0)
    out += struct.pack("<IIIIII", 0x2, 24, symoff, len(symbols), stroff, len(strtab))
    for (_, kind), strx in zip(symbols, at):
        n_type = {"global": 0x0F, "local": 0x0E, "undef": 0x01}[kind]
        out += struct.pack("<IBBHQ", strx, n_type, 0 if kind == "undef" else 1, 0, 0)
    return out + strtab


def elf(symbols):
    """An ELF object with a null section, .symtab, .strtab and .shstrtab."""
    strtab, at = strings(symbols)
    # Locals first, as ELF requires; sh_info names the first non-local.
    order = sorted(range(len(symbols)), key=lambda i: symbols[i][1] != "local")
    symtab = bytes(24)
    for i in order:
        kind = symbols[i][1]
        bind = 0 if kind == "local" else 1
        symtab += struct.pack("<IBBHQQ", at[i], bind << 4 | 2, 0, 0 if kind == "undef" else 1, 0, 0)
    shstrtab = b"\0.symtab\0.strtab\0.shstrtab\0"
    body_off = 64
    sym_off = body_off
    str_off = sym_off + len(symtab)
    shs_off = str_off + len(strtab)
    shoff = shs_off + len(shstrtab)
    header = b"\x7fELF\x02\x01\x01" + bytes(9)
    header += struct.pack("<HHIQQQIHHHHHH", 1, 62, 1, 0, 0, shoff, 0, 64, 0, 0, 64, 4, 3)
    nlocal = 1 + sum(k == "local" for _, k in symbols)
    shdrs = bytes(64)
    shdrs += struct.pack("<IIQQQQIIQQ", 1, 2, 0, 0, sym_off, len(symtab), 2, nlocal, 8, 24)
    shdrs += struct.pack("<IIQQQQIIQQ", 9, 3, 0, 0, str_off, len(strtab), 0, 0, 1, 0)
    shdrs += struct.pack("<IIQQQQIIQQ", 17, 3, 0, 0, shs_off, len(shstrtab), 0, 0, 1, 0)
    return header + symtab + strtab + shstrtab + shdrs


class Unexport(unittest.TestCase):
    def symbols_of(self, build, symbols):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "compiler_rt.o"
            path.write_bytes(build(symbols))
            return tool.symbols(str(path))

    def localized(self, names):
        return [(n, "local" if n in names else k) for n, k in SYMBOLS]

    def test_the_c_librarys_names_are_chosen_and_the_compiler_helpers_are_not(self):
        for build, prefix in ((macho_obj, b"_"), (elf, b"")):
            with self.subTest(build.__name__):
                chosen = tool.libc_names(self.symbols_of(build, SYMBOLS))
                self.assertEqual(chosen, sorted(prefix + n for n in
                                                [b"memset", b"__stack_chk_guard", b"__memcpy_chk"]))

    def test_the_listed_names_are_localized_when_the_rule_agrees_with_the_list(self):
        listed = ["memset", "__stack_chk_guard", "__memcpy_chk"]
        for build, macho in ((macho_obj, True), (elf, False)):
            with self.subTest(build.__name__):
                names = tool.names_to_localize(self.symbols_of(build, SYMBOLS), listed, macho, "x.o")
                prefix = b"_" if macho else b""
                self.assertEqual(names, sorted(prefix + n.encode() for n in listed))

    def test_a_c_library_export_the_list_lacks_stops_the_edit(self):
        for build, macho in ((macho_obj, True), (elf, False)):
            with self.subTest(build.__name__), self.assertRaises(SystemExit):
                tool.names_to_localize(self.symbols_of(build, SYMBOLS), ["memset", "__memcpy_chk"], macho, "x.o")

    def test_a_listed_name_the_object_does_not_export_stops_the_edit(self):
        listed = ["memset", "__stack_chk_guard", "__memcpy_chk", "strlen"]
        for build, macho in ((macho_obj, True), (elf, False)):
            with self.subTest(build.__name__), self.assertRaises(SystemExit):
                tool.names_to_localize(self.symbols_of(build, SYMBOLS), listed, macho, "x.o")

    def test_the_committed_list_reads(self):
        names = tool.listed()
        self.assertIn("memset", names)
        self.assertIn("__stack_chk_guard", names)
        self.assertEqual(len(names), len(set(names)))

    def test_the_check_passes_when_exactly_the_c_librarys_names_went_local(self):
        gone = {"memset", "__stack_chk_guard", "__memcpy_chk"}
        for build in (macho_obj, elf):
            with self.subTest(build.__name__):
                before = self.symbols_of(build, SYMBOLS)
                tool.check(before, self.symbols_of(build, self.localized(gone)), "x.o")

    def test_the_check_stops_on_a_c_library_name_left_exported(self):
        for build in (macho_obj, elf):
            with self.subTest(build.__name__), self.assertRaises(SystemExit):
                before = self.symbols_of(build, SYMBOLS)
                tool.check(before, self.symbols_of(build, self.localized({"__stack_chk_guard"})), "x.o")

    def test_the_check_stops_when_a_compiler_helper_went_local(self):
        gone = {"memset", "__stack_chk_guard", "__memcpy_chk", "__udivti3"}
        for build in (macho_obj, elf):
            with self.subTest(build.__name__), self.assertRaises(SystemExit):
                before = self.symbols_of(build, SYMBOLS)
                tool.check(before, self.symbols_of(build, self.localized(gone)), "x.o")


if __name__ == "__main__":
    unittest.main()
