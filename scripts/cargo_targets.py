"""spyc's cargo build directories: one per checkout under ~/.cache, kept under a cap.

The Makefile builds each checkout into a directory of its own,
`<root>/<checkout name>-<8 hex of sha256(checkout path)>`, with `<root>`
`~/.cache/spyc/target` (BUILD.md, "Build output and disk"). Worktrees building at
once then never share cargo's lock or each other's test binaries, which a shared
target directory gives every checkout at the same path. Before each cargo target
make runs `enforce`, which removes:

1. a directory whose checkout is gone (its worktree was removed), at once;
2. a directory unused for `--idle-days`;
3. the least recently used ones, while the directories total more than
   `--cap-gib` or the disk has less than `--min-free-gib` free.

It never removes the current checkout's directory, one a cargo process holds
(cargo locks `.cargo-lock` and its siblings for a whole build), or one used
within `--grace-min`. A directory's last use is the newest of its stamp, which
make touches at every cargo target, and the directories cargo writes into. A
directory is renamed aside under its locks before it is deleted, so a build that
starts meanwhile begins afresh rather than in a half-deleted directory.

With `--prune-objects`, every directory the cap keeps and no cargo holds then
gets `scripts/prune-cargo-target.py`: build sets untouched for `--prune-days`,
and on macOS the debug objects no executable reads any more.

Adapted from fare-forecast's `research/scripts/cargo_targets.py`, less its rule
for worktrees' own `target/`: spyc never built there, and rust-analyzer keeps
its own output in `target/rust-analyzer` (`rust-analyzer.toml`).

    python3 scripts/cargo_targets.py enforce|usage --root DIR --current DIR
        --cap-gib N --idle-days N --grace-min N --min-free-gib N
        [--dry-run] [--verbose] [--prune-objects --prune-days N]

Runs on the macOS system Python (3.9), which the nightly job finds first.
"""

import argparse
import contextlib
import fcntl
import os
import shutil
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Optional

# Written by make's `target-stamp`: the checkout a directory builds, touched at each use.
STAMP = ".spyc-checkout"
# Directories being deleted, renamed here first; cleared at the start of every run.
TRASH = ".removing"
LOCK_NAMES = (".cargo-lock", ".cargo-build-lock", ".cargo-artifact-lock")
BUILD_MARKERS = ("CACHEDIR.TAG", ".rustc_info.json", STAMP)
PRUNE_SCRIPT = Path(__file__).resolve().parent / "prune-cargo-target.py"
GIB = 2**30


@dataclass
class Limits:
    cap: float  # bytes the directories may total
    idle: float  # seconds unused before a directory goes regardless
    grace: float  # seconds after its last use a directory is never removed
    min_free: float  # bytes the disk keeps free


@dataclass
class Entry:
    path: Path
    checkout: Optional[str]
    last_used: float
    size: int = 0
    current: bool = False
    locked: bool = False
    gone: bool = False  # its checkout no longer exists


def dir_size(path):
    """Bytes on disk under `path`, a hard-linked file once (cargo links each binary from deps/)."""
    seen, total = set(), 0
    for root, _dirs, files in os.walk(path):
        for name in files:
            try:
                st = os.lstat(os.path.join(root, name))
            except FileNotFoundError:
                continue
            if st.st_nlink > 1:
                if (st.st_dev, st.st_ino) in seen:
                    continue
                seen.add((st.st_dev, st.st_ino))
            total += st.st_blocks * 512
    return total


def subdirs(path):
    with contextlib.suppress(OSError), os.scandir(path) as it:
        return [Path(e.path) for e in it if e.is_dir(follow_symlinks=False)]
    return []


def last_activity(path):
    """The newest mtime of `path`, its stamp, and the directories two levels down (a profile and
    its `build` and `incremental`, and before Rust 1.100 its `deps` and `.fingerprint`). Cargo
    1.100 writes a rebuild deeper (`build/<package>/<hash>/`), so make's stamp, touched at every
    use, is what dates a directory a rebuild changed nothing at those two levels of."""
    times = []
    for p in [path, path / STAMP, *subdirs(path)]:
        with contextlib.suppress(OSError):
            times.append(p.stat().st_mtime)
    for child in subdirs(path):
        for p in subdirs(child):
            with contextlib.suppress(OSError):
                times.append(p.stat().st_mtime)
    return max(times, default=0.0)


def lock_files(path):
    """Cargo's lock files under `path`: in the directory, each profile, and each target triple's
    profile."""
    dirs = [path, *subdirs(path)]
    dirs += [g for d in dirs[1:] for g in subdirs(d)]
    return [d / n for d in dirs for n in LOCK_NAMES if (d / n).is_file()]


@contextlib.contextmanager
def held(path):
    """Takes every cargo lock under `path` without waiting and yields True while holding them;
    yields False, holding nothing, when a cargo process holds any of them."""
    fds = []
    try:
        for f in lock_files(path):
            try:
                fd = os.open(f, os.O_RDONLY)
            except FileNotFoundError:
                continue
            fds.append(fd)
            try:
                fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                for held_fd in fds:
                    os.close(held_fd)
                fds = []
                yield False
                return
        yield True
    finally:
        for fd in fds:
            os.close(fd)


def in_use(path):
    with held(path) as free:
        return not free


def same(a, b):
    return os.path.realpath(a) == os.path.realpath(b)


def is_build_dir(path):
    """A directory cargo has built in: it holds cargo's CACHEDIR.TAG or .rustc_info.json, or the
    stamp (cargo writes no CACHEDIR.TAG in a directory that already existed)."""
    return not path.is_symlink() and any((path / n).is_file() for n in BUILD_MARKERS)


def entries_under(root, current):
    """The directories under `root`, one per checkout that has built there. Anything else under
    it is left alone, so a root set wrongly (`~`) removes nothing."""
    entries = []
    for child in sorted(subdirs(root)):
        if child.name.startswith(".") or not is_build_dir(child):
            continue
        try:
            checkout = (child / STAMP).read_text().strip() or None
        except OSError:
            checkout = None
        entries.append(
            Entry(
                path=child,
                checkout=checkout,
                last_used=last_activity(child),
                current=same(child, current),
                gone=checkout is not None and not Path(checkout).is_dir(),
            )
        )
    return entries


def survey(root, current):
    entries = entries_under(root, current)
    for e in entries:
        e.locked = in_use(e.path)
        e.size = dir_size(e.path)
    return entries


def first_reason(e, limits, now):
    """Why `e` goes whatever the totals, or None."""
    if e.current or e.locked:
        return None
    if e.gone:
        return "worktree gone"
    if now - e.last_used >= limits.idle:
        return "idle"
    return None


def plan(entries, limits, now, free):
    """What to remove, in order, as (entry, reason), and the directories' total and the disk's
    free bytes after it."""
    removals = [(e, r) for e in entries for r in [first_reason(e, limits, now)] if r]
    gone = {id(e) for e, _ in removals}
    kept = [e for e in entries if id(e) not in gone]
    total = sum(e.size for e in kept)
    free += sum(e.size for e, _ in removals)
    evictable = [
        e for e in kept if not e.current and not e.locked and now - e.last_used >= limits.grace
    ]
    for e in sorted(evictable, key=lambda e: e.last_used):
        if total <= limits.cap and free >= limits.min_free:
            break
        removals.append((e, "over the cap" if total > limits.cap else "disk low"))
        total -= e.size
        free += e.size
    return removals, total, free


def remove(path, root):
    """Renames `path` into `<root>/.removing` under its locks, then deletes it. False when a cargo
    process holds it, or another run took it first."""
    trash = root / TRASH
    trash.mkdir(parents=True, exist_ok=True)
    dest = trash / f"{path.name}-{os.getpid()}-{time.time_ns()}"
    with held(path) as free:
        if not free:
            return False
        try:
            path.rename(dest)
        except FileNotFoundError:
            return False
        except OSError:  # another filesystem: delete it where it is, still under its locks
            shutil.rmtree(path, ignore_errors=True)
            return True
    shutil.rmtree(dest, ignore_errors=True)
    return True


def gib(n):
    return f"{n / GIB:.1f} GiB"


def short(path):
    home = str(Path.home())
    s = str(path)
    return "~" + s[len(home) :] if s.startswith(home + os.sep) else s


def state(e, limits, now):
    if e.current:
        return "current"
    if e.locked:
        return "building"
    if e.gone:
        return "worktree gone"
    return "idle" if now - e.last_used >= limits.idle else "live"


def disk_free(root):
    path = root
    while not path.exists():
        path = path.parent
    return shutil.disk_usage(path).free


def prune_objects(args, entries, removed):
    """`prune-cargo-target.py` on every directory the cap kept that no cargo holds."""
    for e in entries:
        if id(e) in removed or not e.path.is_dir() or in_use(e.path):
            continue
        cmd = [sys.executable, str(PRUNE_SCRIPT), "--target", str(e.path)]
        cmd += ["--days", str(args.prune_days)]
        if args.dry_run:
            cmd.append("--dry-run")
        subprocess.run(cmd, check=False)


def enforce(args, limits):
    args.root.mkdir(parents=True, exist_ok=True)
    shutil.rmtree(args.root / TRASH, ignore_errors=True)
    entries = survey(args.root, args.current)
    removals, total, free = plan(entries, limits, time.time(), disk_free(args.root))
    for e, reason in removals:
        if args.dry_run:
            print(f"target-cap: would remove {short(e.path)} ({gib(e.size)}, {reason})")
        elif remove(e.path, args.root):
            print(f"target-cap: removed {short(e.path)} ({gib(e.size)}, {reason})")
    if total > limits.cap or free < limits.min_free:
        print(
            f"target-cap: build directories {gib(total)} (cap {gib(limits.cap)}), disk free "
            f"{gib(free)} (floor {gib(limits.min_free)}); the rest are in use or used within "
            f"the grace period: `make target-usage`",
            file=sys.stderr,
        )
    elif args.verbose:
        after = "would be" if args.dry_run else "now"
        print(
            f"target-cap: build directories {after} {gib(total)} (cap {gib(limits.cap)}), "
            f"disk free {gib(free)}"
        )
    if args.prune_objects:
        prune_objects(args, entries, {id(e) for e, _ in removals})


def usage(args, limits):
    entries = survey(args.root, args.current)
    now = time.time()
    removals, _, _ = plan(entries, limits, now, disk_free(args.root))
    why = {id(e): r for e, r in removals}
    print(
        f"{short(args.root)}: {gib(sum(e.size for e in entries))} of a {gib(limits.cap)} cap; "
        f"{gib(disk_free(args.root))} free on the disk (floor {gib(limits.min_free)})"
    )
    for e in sorted(entries, key=lambda e: -e.last_used):
        used = time.strftime("%Y-%m-%d %H:%M", time.localtime(e.last_used))
        note = f"  -> next build removes it ({why[id(e)]})" if id(e) in why else ""
        where = short(e.checkout or e.path)
        print(f"  {e.size / GIB:6.1f}G  {used}  {state(e, limits, now):13}  {where}{note}")


def main(argv=None):
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("command", choices=("enforce", "usage"))
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--current", type=Path, required=True)
    parser.add_argument("--cap-gib", type=float, required=True)
    parser.add_argument("--idle-days", type=float, required=True)
    parser.add_argument("--grace-min", type=float, required=True)
    parser.add_argument("--min-free-gib", type=float, required=True)
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--verbose", action="store_true")
    parser.add_argument("--prune-objects", action="store_true")
    parser.add_argument("--prune-days", type=float, default=3)
    args = parser.parse_args(argv)
    limits = Limits(
        cap=args.cap_gib * GIB,
        idle=args.idle_days * 86400,
        grace=args.grace_min * 60,
        min_free=args.min_free_gib * GIB,
    )
    if args.command == "enforce":
        enforce(args, limits)
    else:
        usage(args, limits)
    return 0


if __name__ == "__main__":
    sys.exit(main())
