"""Tests for scripts/prune-cargo-target.py's Rust 1.100 layout: `build/<package>/<hash>/`."""

import importlib.util
import os
from pathlib import Path
import tempfile
import time
import unittest

SPEC = importlib.util.spec_from_file_location(
    "prune_cargo_target", Path(__file__).with_name("prune-cargo-target.py"))
prune = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(prune)

DAY = 86400


def unit(build, package, digest, age_days):
    """A build unit with an output and a fingerprint, last written `age_days` ago."""
    root = build / package / digest
    files = [root / "out" / f"lib{package}-{digest}.rlib", root / "fingerprint" / "lib"]
    when = time.time() - age_days * DAY
    for f in files:
        f.parent.mkdir(parents=True, exist_ok=True)
        f.write_bytes(b"x" * 64)
        os.utime(f, (when, when))
    return root


class PruneUnits(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.build = Path(self.tmp.name) / "debug" / "build"
        # The rule reads the later of mtime and ctime; a test can only set the first.
        self.real_touched = prune.touched
        prune.touched = lambda st: st.st_mtime
        self.cutoff = time.time() - 3 * DAY

    def tearDown(self):
        prune.touched = self.real_touched
        self.tmp.cleanup()

    def test_a_stale_unit_of_this_workspace_goes_whole_and_a_fresh_one_stays(self):
        stale = unit(self.build, "spyc", "0" * 16, 5)
        fresh = unit(self.build, "spyc", "1" * 16, 1)
        tally = prune.prune_units(self.build, ["spyc"], self.cutoff, dry=False)
        self.assertFalse(stale.exists())
        self.assertTrue(fresh.exists())
        self.assertEqual((tally.sets, tally.files), (1, 2))

    def test_one_fresh_file_keeps_its_unit(self):
        root = unit(self.build, "spyc", "2" * 16, 5)
        (root / "out" / "new").write_bytes(b"y")
        prune.prune_units(self.build, ["spyc"], self.cutoff, dry=False)
        self.assertTrue(root.exists())

    def test_third_party_units_and_the_old_layout_are_never_touched(self):
        dependency = unit(self.build, "serde", "3" * 16, 30)
        old_layout = self.build / f"spyc-{'4' * 16}"
        old_layout.mkdir(parents=True)
        os.utime(old_layout, (0, 0))
        not_a_unit = unit(self.build, "spyc", "not-a-hash", 30)
        prune.prune_units(self.build, ["spyc"], self.cutoff, dry=False)
        self.assertTrue(dependency.exists())
        self.assertTrue(old_layout.exists())
        self.assertTrue(not_a_unit.exists())

    def test_a_dry_run_counts_and_removes_nothing(self):
        stale = unit(self.build, "spyc-vt-sys", "5" * 16, 5)
        tally = prune.prune_units(self.build, ["spyc", "spyc-vt-sys"], self.cutoff, dry=True)
        self.assertTrue(stale.exists())
        self.assertEqual(tally.sets, 1)


if __name__ == "__main__":
    unittest.main()
