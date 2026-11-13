"""Tests for scripts/linker-messages.py, on excerpts of real cargo build logs."""

import importlib.util
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location(
    "linker_messages", Path(__file__).with_name("linker-messages.py"))
tool = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(tool)

# spyc at f93f52f2 on macOS, built with every linker message forced on.
DUPLICATE = """\
   Compiling spyc v2.5.0-CURRENT (/src)
warning: linker stderr: ld: duplicate symbol '_memset' in:
         /t/debug/build/spyc-vt-sys/1692147fb8b8a966/out/libspyc_vt_sys-1692147fb8b8a966.rlib[7](compiler_rt.o)
         /t/debug/build/spyc-vt-sys/1692147fb8b8a966/out/libspyc_vt_sys-1692147fb8b8a966.rlib[8](ghostty-vt-static_zcu.o)
  |
  = note: requested on the command line with `--force-warn linker-messages`

warning: ld: warning: ignoring duplicate libraries: '-lSystem'
         ld: warning: object file (/t/debug/build/zstd-sys/26deac26b4e02183/out/libzstd_sys-26deac26b4e02183.rlib[6](44ff4c55aa9e5133-entropy_common.o)) was built for newer 'macOS' version (27.0) than being linked (11.0)
         ld: warning: object file (/t/debug/build/zstd-sys/26deac26b4e02183/out/libzstd_sys-26deac26b4e02183.rlib[7](44ff4c55aa9e5133-error_private.o)) was built for newer 'macOS' version (27.0) than being linked (11.0)
  |
  = note: requested on the command line with `--force-warn linker-info`

warning: `spyc` (bin "spyc") generated 2 warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 33.10s
"""

CLEAN = """\
   Compiling spyc v2.5.0-CURRENT (/src)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 33.10s
"""


class LinkerMessages(unittest.TestCase):
    def test_a_duplicate_symbol_is_flagged_under_linker_messages(self):
        summary, flagged = tool.report(DUPLICATE)
        self.assertEqual(flagged, ["linker stderr: ld: duplicate symbol '_memset' in:"])
        self.assertIn("| 1 | `linker-messages` | `linker stderr: ld: duplicate symbol '_memset' in:` |", summary)
        self.assertIn("`libspyc_vt_sys.rlib(compiler_rt.o)`", summary)

    def test_informational_output_is_listed_unflagged_and_grouped_per_archive(self):
        summary, _ = tool.report(DUPLICATE)
        self.assertIn("| 2 | `linker-info` | `ld: warning: object file (libzstd_sys.rlib(*.o)) was built for newer"
                      " 'macOS' version (27.0) than being linked (11.0)` |", summary)
        self.assertIn("| 1 | `linker-info` | `ld: warning: ignoring duplicate libraries: '-lSystem'` |", summary)

    def test_a_build_without_linker_output_says_so(self):
        summary, flagged = tool.report(CLEAN)
        self.assertEqual(flagged, [])
        self.assertIn("No linker output in this build.", summary)

    def test_it_never_fails_even_without_a_log(self):
        self.assertEqual(tool.main(["linker-messages.py", "/nonexistent/build.log"]), 0)
        self.assertEqual(tool.main(["linker-messages.py"]), 0)


if __name__ == "__main__":
    unittest.main()
