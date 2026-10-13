#!/usr/bin/env python3
"""Prune this workspace's dead build output from its cargo target directory.

Two leaks, neither of which cargo ever cleans up:

* Every worktree path hashes into a new build set (`<crate>-<hash>.*`), and
  removing the worktree leaves its set behind. A set none of whose files were
  touched in `--days` is deleted whole.
* macOS keeps debug info in loose per-codegen-unit `.rcgu.o` files beside each
  executable, and every incremental rebuild writes a fresh set under new names.
  The executable's debug map (its `OSO` stabs) lists the objects it still
  reads; any other object of that set over an hour old is deleted. One test
  binary had accumulated 131k objects while referencing 256.

Third-party dependencies are never touched, so nothing has to recompile them.
Ages use the later of mtime and ctime: rustc hardlinks reused objects out of the
incremental cache, which keeps the old mtime but moves the ctime.
"""

import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time

OBJECT_GRACE_SECS = 3600


def workspace(manifest_dir):
    meta = json.loads(subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        cwd=manifest_dir, check=True, capture_output=True, text=True).stdout)
    names = {
        t["name"].replace("-", "_")
        for p in meta["packages"] for t in p["targets"]
        if "custom-build" not in t["kind"]
    }
    return Path(meta["target_directory"]), sorted(names)


def touched(st):
    return max(st.st_mtime, st.st_ctime)


def referenced_objects(exe):
    """File names of the loose objects in `exe`'s debug map, or None when it
    can't be read. Names, not paths, so a symlinked target dir still matches;
    `lib.rlib(member.o)` entries are archive members, not loose files."""
    try:
        out = subprocess.run(["nm", "-ap", str(exe)], check=True,
                             capture_output=True, text=True).stdout
    except (OSError, subprocess.CalledProcessError):
        return None
    refs = set()
    for line in out.splitlines():
        parts = line.split(None, 5)
        if len(parts) == 6 and parts[4] == "OSO" and not parts[5].endswith(")"):
            refs.add(os.path.basename(parts[5]))
    return refs


class Tally:
    def __init__(self):
        self.sets = self.objects = self.files = self.freed = 0

    def take(self, entry, st, dry):
        self.files += 1
        if st.st_nlink == 1:
            self.freed += st.st_blocks * 512
        if not dry:
            try:
                os.unlink(entry.path)
            except FileNotFoundError:
                pass


def prune_deps(deps, pattern, cutoff, dry):
    groups = {}
    with os.scandir(deps) as it:
        for e in it:
            m = pattern.match(e.name)
            if m and not e.is_dir(follow_symlinks=False):
                groups.setdefault(m.group(1, 2), []).append(e)

    now, tally = time.time(), Tally()
    for (crate, digest), entries in groups.items():
        stats = []
        for e in entries:
            try:
                stats.append((e, e.stat(follow_symlinks=False)))
            except FileNotFoundError:
                pass
        if not stats:
            continue
        if max(touched(st) for _, st in stats) < cutoff:
            tally.sets += 1
            for e, st in stats:
                tally.take(e, st, dry)
            continue

        objects = [(e, st) for e, st in stats if e.name.endswith(".rcgu.o")]
        exe = deps / f"{crate}-{digest}"
        if not objects or not os.access(exe, os.X_OK):
            continue
        refs = referenced_objects(exe)
        # An executable naming none of its own objects is not one this rule
        # understands; keeping everything is the safe answer.
        if not refs or not any(e.name in refs for e, _ in objects):
            continue
        for e, st in objects:
            if e.name not in refs and touched(st) < now - OBJECT_GRACE_SECS:
                tally.objects += 1
                tally.take(e, st, dry)
    return tally


def prune_incremental(inc, pattern, cutoff, dry):
    removed = 0
    with os.scandir(inc) as it:
        for e in it:
            if pattern.match(e.name) and touched(e.stat(follow_symlinks=False)) < cutoff:
                removed += 1
                if not dry:
                    shutil.rmtree(e.path, ignore_errors=True)
    return removed


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--days", type=float, default=3,
                        help="delete build sets untouched for this long (default 3)")
    parser.add_argument("--dry-run", action="store_true", help="report, delete nothing")
    args = parser.parse_args()
    if args.days <= 0:
        parser.error("--days must be positive")

    target, crates = workspace(Path(__file__).resolve().parent.parent)
    names = "|".join(map(re.escape, crates))
    deps_pattern = re.compile(rf"^(?:lib)?({names})-([0-9a-f]{{16}})(?:\..*)?$")
    inc_pattern = re.compile(rf"^({names})-[0-9a-z]+$")
    cutoff = time.time() - args.days * 86400
    verb = "would free" if args.dry_run else "freed"

    print(f"{target}: {', '.join(crates)}; sets untouched {args.days:g} days")
    total = 0
    for deps in sorted(target.glob("*/deps")) + sorted(target.glob("*/*/deps")):
        t = prune_deps(deps, deps_pattern, cutoff, args.dry_run)
        total += t.freed
        if t.files:
            print(f"  {deps.relative_to(target)}: {t.sets} stale sets, {t.objects} "
                  f"unreferenced objects, {t.files} files; {verb} {t.freed / 2**30:.1f} GiB")
    for inc in sorted(target.glob("*/incremental")) + sorted(target.glob("*/*/incremental")):
        n = prune_incremental(inc, inc_pattern, cutoff, args.dry_run)
        if n:
            print(f"  {inc.relative_to(target)}: {n} stale incremental caches")
    print(f"{verb} at least {total / 2**30:.1f} GiB (hardlinked files not counted)")


if __name__ == "__main__":
    sys.exit(main())
