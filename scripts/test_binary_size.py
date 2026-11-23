"""Each shipped binary is held to its size budget (#933): scripts/binary-size.sh,
run on stand-in binaries of chosen sizes against a budgets file of the test's
own, and the checked-in budgets file names every binary a release ships.
"""

from pathlib import Path
import re
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent
SCRIPT = ROOT / "scripts/binary-size.sh"


def run(budgets, binaries, summary=None):
    with tempfile.TemporaryDirectory() as d:
        budget_file = Path(d, "budgets")
        budget_file.write_text(budgets, encoding="utf-8")
        args = []
        for name, size in binaries:
            path = Path(d, name)
            if size is not None:
                path.write_bytes(b"\0" * size)
            args.append(f"{name}={path}")
        env = {"PATH": "/usr/bin:/bin", "SPYC_SIZE_BUDGETS": str(budget_file)}
        if summary is not None:
            env["GITHUB_STEP_SUMMARY"] = str(Path(d, "summary"))
        r = subprocess.run(["bash", str(SCRIPT), *args], env=env, capture_output=True, text=True)
        written = Path(d, "summary").read_text(encoding="utf-8") if summary else ""
        return r, written


class BinarySizeBudget(unittest.TestCase):
    def test_a_binary_within_its_budget_passes_and_says_the_room_left(self):
        r, summary = run("spyc-x 2048\n", [("spyc-x", 1024)], summary=True)
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertIn("left", r.stdout)
        self.assertIn("`spyc-x`", summary)

    def test_a_binary_over_its_budget_fails(self):
        r, _ = run("spyc-x 1000\n", [("spyc-x", 1001)])
        self.assertEqual(r.returncode, 1, r.stdout)
        self.assertIn("OVER", r.stdout)

    def test_a_binary_with_no_budget_or_none_there_fails(self):
        r, _ = run("spyc-x 1000\n", [("spyc-y", 10)])
        self.assertEqual(r.returncode, 1, r.stdout)
        self.assertIn("no budget", r.stdout)
        r, _ = run("spyc-x 1000\n", [("spyc-x", None)])
        self.assertEqual(r.returncode, 1, r.stdout)
        self.assertIn("no binary", r.stdout)

    def test_every_binary_release_yml_ships_has_a_budget_and_is_checked(self):
        budgets = (ROOT / "scripts/binary-size-budgets").read_text(encoding="utf-8")
        named = {l.split()[0] for l in budgets.splitlines() if l.strip() and not l.startswith("#")}
        release = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
        checked = set(re.findall(r"\b(spyc(?:-jar)?-(?:macos-universal|linux-x86_64|linux-aarch64))=", release))
        shipped = {f"{b}-{p}" for b in ("spyc", "spyc-jar")
                   for p in ("macos-universal", "linux-x86_64", "linux-aarch64")}
        self.assertEqual(named, shipped, "a budget per shipped binary")
        self.assertEqual(checked, shipped, "release.yml checks every shipped binary")


if __name__ == "__main__":
    unittest.main()
