"""Tests for scripts/linker-messages.py, on excerpts of real cargo build logs."""

import contextlib
import importlib.util
import io
from pathlib import Path
import tempfile
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

# main's link of spyc as the gate builds it, with both lints forced on: one
# informational line, then a line of blanks, kept as `ld` printed it.
INFO_ONLY = (
    "   Compiling spyc v2.5.0-CURRENT (/src)\n"
    "warning: ld: warning: ignoring duplicate libraries: '-lSystem'\n"
    "         \n"
    "  |\n"
    "  = note: requested on the command line with `--force-warn linker-info`\n"
    "\n"
    "warning: `spyc` (bin \"spyc\") generated 1 warning\n"
    "    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.23s\n"
)

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

    def test_without_deny_it_never_fails_even_without_a_log(self):
        self.assertEqual(self.run_on(DUPLICATE), 0)
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(tool.main(["linker-messages.py", "/nonexistent/build.log"]), 0)
            self.assertEqual(tool.main(["linker-messages.py"]), 0)

    def test_deny_fails_on_a_message_no_exception_matches(self):
        self.assertEqual(self.run_on(DUPLICATE, exceptions=""), 1)
        self.assertIn("- `linker-messages`: linker stderr: ld: duplicate symbol '_memset' in:", self.out.getvalue())
        # One error for the message; the objects it names continue it.
        self.assertEqual(self.err.getvalue().count("::error"), 1)

    def test_an_exception_with_an_issue_lets_its_message_through(self):
        self.assertEqual(self.run_on(DUPLICATE, exceptions="1129 ld: duplicate symbol '_memset'\n"), 0)
        self.assertIn("- `linker stderr: ld: duplicate symbol '_memset' in:` (#1129)", self.out.getvalue())

    def test_an_exception_lets_through_only_what_it_matches(self):
        self.assertEqual(self.run_on(DUPLICATE, exceptions="1129 undefined symbol\n"), 1)
        second = DUPLICATE.replace("  |\n  = note: requested on the command line with `--force-warn linker-messages`",
                                   "         ld: warning: a new message\n  |\n"
                                   "  = note: requested on the command line with `--force-warn linker-messages`", 1)
        self.assertEqual(self.run_on(second, exceptions="1129 duplicate symbol '_memset'\n"), 1)
        self.assertIn("- `linker-messages`: ld: warning: a new message", self.out.getvalue())

    def test_an_exception_without_an_issue_is_an_error(self):
        for bad in ["ld: duplicate symbol '_memset'", "#1129 ld: duplicate symbol '_memset'", "1129", "1129 ("]:
            with self.subTest(entry=bad):
                self.assertEqual(self.run_on(INFO_ONLY, exceptions=f"# a comment\n{bad}\n"), 1)
                self.assertIn("linker-exceptions.txt:2:", self.err.getvalue())

    def test_deny_passes_informational_output(self):
        self.assertEqual(self.run_on(INFO_ONLY, exceptions=""), 0)
        self.assertIn("| 1 | `linker-info` | `ld: warning: ignoring duplicate libraries: '-lSystem'` |",
                      self.out.getvalue())

    def test_deny_fails_on_a_link_it_parsed_no_output_from(self):
        for log in [CLEAN, ""]:
            with self.subTest(log=log):
                self.assertEqual(self.run_on(log, exceptions=""), 1)
                self.assertIn("the log format may have changed", self.out.getvalue())

    def test_deny_fails_on_output_it_cannot_attribute(self):
        changed_note = INFO_ONLY.replace(
            "requested on the command line with `--force-warn linker-info`", "a note rustc reworded")
        self.assertEqual(self.run_on(changed_note, exceptions=""), 1)
        cut_short = INFO_ONLY + DUPLICATE[:DUPLICATE.index("  |")]
        self.assertEqual(self.run_on(cut_short, exceptions=""), 1)

    def test_deny_fails_without_a_log_or_an_exceptions_file(self):
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(tool.main(["linker-messages.py", "--deny", "/dev/null", "/nonexistent/build.log"]), 1)
            self.assertEqual(tool.main(["linker-messages.py", "--deny", "/dev/null"]), 1)
        self.assertEqual(self.run_on(INFO_ONLY, exceptions=None, deny_path="/nonexistent/exceptions.txt"), 1)

    def run_on(self, log, exceptions=None, deny_path=None):
        """`main` on `log`, with `--deny` when given an exceptions text or path;
        its stdout and stderr in self.out and self.err."""
        with tempfile.TemporaryDirectory() as d:
            path = Path(d, "build.log")
            path.write_text(log, encoding="utf-8")
            flags = []
            if exceptions is not None:
                deny_path = Path(d, "linker-exceptions.txt")
                deny_path.write_text(exceptions, encoding="utf-8")
            if deny_path is not None:
                flags = ["--deny", str(deny_path)]
            self.out, self.err = io.StringIO(), io.StringIO()
            with contextlib.redirect_stdout(self.out), contextlib.redirect_stderr(self.err):
                return tool.main(["linker-messages.py", *flags, str(path)])


if __name__ == "__main__":
    unittest.main()
