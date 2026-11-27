#!/usr/bin/env python3
"""Drive every released `spyc-jar` with this build (the 3.0 review's A8, F4).

A released jar is an old image for as long as its program runs, so each
stable release's `spyc-jar` from 3.0.0 on is a fixture: fetched from the
release's own tarball (stable releases are never pruned, and GitHub keeps
their assets as published), kept under `--cache`, and handed to
`tests/jar_compat.rs` as `SPYC_JAR_OLD_BIN`, one `cargo test` each. A
release whose tarball or jar can't be had fails the run rather than skip it.
`make test-jar-compat` runs it; it needs `gh`.
"""

import argparse
import json
import os
import platform
import re
import subprocess
import sys
import tarfile
import tempfile
from pathlib import Path

REPO = "Tripstack-Corp/spyc"
# The variable tests/jar_compat.rs reads for the jar to drive.
ENV = "SPYC_JAR_OLD_BIN"
# The first release that ships a jar.
FIRST = (3, 0, 0)


def version_of(tag):
    m = re.fullmatch(r"v(\d+)\.(\d+)\.(\d+)", tag)
    return tuple(int(n) for n in m.groups()) if m else None


def fixture_tags(releases):
    """The stable releases from `FIRST` on, oldest first, from
    `gh release list --json tagName,isPrerelease,isDraft`."""
    tags = [r["tagName"] for r in releases
            if not r.get("isPrerelease") and not r.get("isDraft")
            and (version_of(r["tagName"]) or (0,)) >= FIRST]
    return sorted(tags, key=version_of)


def asset(tag, system, machine):
    """The release tarball that holds this machine's `spyc-jar`."""
    if system == "Darwin":
        return f"spyc-{tag}-macos-universal.tar.gz"
    arch = {"x86_64": "x86_64", "amd64": "x86_64", "aarch64": "aarch64", "arm64": "aarch64"}.get(machine)
    if system == "Linux" and arch:
        return f"spyc-{tag}-linux-{arch}.tar.gz"
    raise SystemExit(f"no release tarball for {system} {machine}")


def fetch(tag, cache):
    """`tag`'s `spyc-jar`, fetched into `cache` once."""
    jar = cache / tag / "spyc-jar"
    if jar.is_file():
        return jar
    name = asset(tag, platform.system(), platform.machine())
    with tempfile.TemporaryDirectory() as d:
        subprocess.run(["gh", "release", "download", tag, "--repo", REPO,
                        "--pattern", name, "--dir", d], check=True)
        with tarfile.open(Path(d) / name) as t:
            try:
                member = t.extractfile("spyc-jar")
            except KeyError:
                member = None
            if member is None:
                raise SystemExit(f"{tag}: {name} holds no spyc-jar")
            jar.parent.mkdir(parents=True, exist_ok=True)
            part = jar.with_suffix(".part")
            part.write_bytes(member.read())
            part.chmod(0o755)
            part.rename(jar)
    return jar


def main():
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--cache", required=True, type=Path)
    args = p.parse_args()
    listing = subprocess.run(["gh", "release", "list", "--repo", REPO, "--limit", "1000",
                              "--json", "tagName,isPrerelease,isDraft"],
                             check=True, capture_output=True, text=True).stdout
    tags = fixture_tags(json.loads(listing))
    if not tags:
        print(f"no stable release from {'.'.join(map(str, FIRST))} on yet: no released jar to drive")
        return 0
    for tag in tags:
        jar = fetch(tag, args.cache)
        print(f"→ {tag}: {jar}", flush=True)
        env = dict(os.environ, **{ENV: str(jar)})
        r = subprocess.run(["cargo", "test", "--locked", "--test", "jar_compat"], env=env)
        if r.returncode != 0:
            print(f"{tag}'s spyc-jar failed jar_compat", file=sys.stderr)
            return r.returncode
    return 0


if __name__ == "__main__":
    sys.exit(main())
