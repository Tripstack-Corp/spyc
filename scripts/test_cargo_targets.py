"""Tests for scripts/cargo_targets.py: `make test-scripts`."""

import contextlib
import fcntl
import io
import os
import sys
import tempfile
import time
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))

import cargo_targets as ct  # noqa: E402

GIB = ct.GIB
NOW = 1_800_000_000.0
LIMITS = ct.Limits(cap=10 * GIB, idle=2 * 86400, grace=3600, min_free=0)


def entry(name, size=GIB, age=7200.0, **kw):
    return ct.Entry(path=Path(name), checkout=name, last_used=NOW - age, size=size, **kw)


def names(removals):
    return [(e.path.name, reason) for e, reason in removals]


class Plan(unittest.TestCase):
    def test_a_directory_whose_worktree_is_gone_goes_at_once(self):
        removals, _, _ = ct.plan([entry("a", age=60, gone=True)], LIMITS, NOW, 100 * GIB)
        self.assertEqual(names(removals), [("a", "worktree gone")])

    def test_a_directory_unused_for_the_idle_days_goes_under_the_cap(self):
        removals, total, _ = ct.plan(
            [entry("old", age=2 * 86400 + 1), entry("new", age=2 * 86400 - 1)], LIMITS, NOW, 0
        )
        self.assertEqual(names(removals), [("old", "idle")])
        self.assertEqual(total, GIB)

    def test_over_the_cap_the_least_recently_used_go_first_until_under_it(self):
        dirs = [
            entry("c", size=4 * GIB, age=7000),
            entry("a", size=4 * GIB, age=9000),
            entry("b", size=4 * GIB, age=8000),
        ]
        removals, total, _ = ct.plan(dirs, LIMITS, NOW, 100 * GIB)
        self.assertEqual(names(removals), [("a", "over the cap")])
        self.assertEqual(total, 8 * GIB)

    def test_nothing_used_within_the_grace_period_goes_even_over_the_cap(self):
        dirs = [entry("a", size=8 * GIB, age=3599), entry("b", size=8 * GIB, age=60)]
        removals, total, _ = ct.plan(dirs, LIMITS, NOW, 100 * GIB)
        self.assertEqual(removals, [])
        self.assertEqual(total, 16 * GIB)

    def test_the_current_and_a_building_directory_never_go(self):
        dirs = [
            entry("current", size=20 * GIB, age=10 * 86400, current=True),
            entry("building", age=10 * 86400, gone=True, locked=True),
        ]
        removals, _, _ = ct.plan(dirs, LIMITS, NOW, 100 * GIB)
        self.assertEqual(removals, [])

    def test_a_disk_below_the_floor_evicts_until_it_is_back_above(self):
        limits = ct.Limits(cap=100 * GIB, idle=2 * 86400, grace=3600, min_free=20 * GIB)
        dirs = [entry(n, size=4 * GIB, age=a) for n, a in (("a", 9000), ("b", 8000), ("c", 7000))]
        removals, _, free = ct.plan(dirs, limits, NOW, 13 * GIB)
        self.assertEqual(names(removals), [("a", "disk low"), ("b", "disk low")])
        self.assertEqual(free, 21 * GIB)


def build_dir(root, name, checkout=None):
    d = Path(root) / name
    (d / "debug" / "deps").mkdir(parents=True)
    (d / "CACHEDIR.TAG").write_text("Signature: 8a477f597d28d172789f06886806bc55\n")
    if checkout is not None:
        (d / ct.STAMP).write_text(f"{checkout}\n")
    return d


def age(path, seconds):
    t = time.time() - seconds
    os.utime(path, (t, t))


def run(argv):
    out = io.StringIO()
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(io.StringIO()):
        ct.main(argv)
    return out.getvalue()


class OnDisk(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def enforce_argv(self, cache, current, *extra):
        return [
            "enforce", "--root", str(cache), "--current", str(current), "--cap-gib", "0",
            "--idle-days", "2", "--grace-min", "60", "--min-free-gib", "0", *extra,
        ]

    def test_size_counts_a_hard_linked_file_once(self):
        d = build_dir(self.root, "a")
        (d / "debug" / "deps" / "bin-0123").write_bytes(b"x" * 100_000)
        os.link(d / "debug" / "deps" / "bin-0123", d / "debug" / "bin")
        one = ct.dir_size(d)
        (d / "debug" / "other").write_bytes(b"x" * 100_000)
        self.assertGreaterEqual(one, 100_000)
        self.assertLess(one, 200_000)
        self.assertGreaterEqual(ct.dir_size(d), one + 100_000)

    def test_last_use_is_the_newest_of_the_stamp_and_the_directories_cargo_writes(self):
        d = build_dir(self.root, "a", checkout=self.root)
        for p in (d / ct.STAMP, d / "debug" / "deps", d / "debug", d):
            age(p, 5000)
        self.assertLess(ct.last_activity(d), time.time() - 4000)
        age(d / "debug" / "deps", 10)
        self.assertGreater(ct.last_activity(d), time.time() - 100)
        age(d / "debug" / "deps", 5000)
        age(d / "debug", 5000)
        (d / ct.STAMP).touch()
        self.assertGreater(ct.last_activity(d), time.time() - 100)

    def test_a_held_cargo_lock_marks_a_directory_in_use_and_it_is_not_removed(self):
        d = build_dir(self.root, "a")
        lock = d / "debug" / ".cargo-lock"
        lock.touch()
        self.assertFalse(ct.in_use(d))
        with open(lock, "rb") as f:
            fcntl.flock(f, fcntl.LOCK_SH)
            self.assertTrue(ct.in_use(d))
            self.assertFalse(ct.remove(d, self.root))
            self.assertTrue(d.is_dir())
        self.assertTrue(ct.remove(d, self.root))
        self.assertFalse(d.exists())

    def test_only_build_directories_count_and_a_gone_checkout_is_seen(self):
        live = self.root / "live-checkout"
        live.mkdir()
        cache = self.root / "cache"
        build_dir(cache, "live-1", checkout=live)
        build_dir(cache, "gone-2", checkout=self.root / "removed-worktree")
        (cache / "not-a-build-dir").mkdir()
        (cache / ct.TRASH).mkdir()
        got = {e.path.name: e for e in ct.entries_under(cache, cache / "live-1")}
        self.assertEqual(sorted(got), ["gone-2", "live-1"])
        self.assertTrue(got["live-1"].current)
        self.assertFalse(got["live-1"].gone)
        self.assertTrue(got["gone-2"].gone)

    def test_enforce_removes_a_gone_checkouts_directory_and_keeps_the_current(self):
        checkout = self.root / "checkout"
        checkout.mkdir()
        cache = self.root / "cache"
        current = build_dir(cache, "checkout-1", checkout=checkout)
        gone = build_dir(cache, "removed-2", checkout=self.root / "removed")
        out = run(self.enforce_argv(cache, current))
        self.assertFalse(gone.exists())
        self.assertTrue(current.is_dir())
        self.assertIn("worktree gone", out)
        self.assertEqual(list((cache / ct.TRASH).iterdir()), [])

    def test_a_dry_run_names_what_it_would_remove_and_removes_nothing(self):
        cache = self.root / "cache"
        current = build_dir(cache, "checkout-1", checkout=self.root)
        gone = build_dir(cache, "removed-2", checkout=self.root / "removed")
        out = run(self.enforce_argv(cache, current, "--dry-run"))
        self.assertTrue(gone.is_dir())
        self.assertIn("would remove", out)

    def test_object_pruning_skips_a_removed_and_a_building_directory(self):
        cache = self.root / "cache"
        current = build_dir(cache, "checkout-1", checkout=self.root)
        building = build_dir(cache, "building-2", checkout=self.root)
        gone = build_dir(cache, "removed-3", checkout=self.root / "removed")
        (building / "debug" / ".cargo-lock").touch()
        pruned = []

        def fake_run(cmd, check):
            pruned.append(Path(cmd[cmd.index("--target") + 1]).name)

        with open(building / "debug" / ".cargo-lock", "rb") as f, mock.patch.object(
            ct.subprocess, "run", side_effect=fake_run
        ):
            fcntl.flock(f, fcntl.LOCK_SH)
            run(self.enforce_argv(cache, current, "--prune-objects", "--prune-days", "1"))
        self.assertFalse(gone.exists())
        self.assertEqual(pruned, ["checkout-1"])


if __name__ == "__main__":
    unittest.main()
