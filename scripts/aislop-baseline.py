#!/usr/bin/env python3
"""Count-based baseline wrapper around `aislop scan`.

aislop 0.10.2 has no native baseline/suppression. Its comment engine
(narrative-/trivial-/meta-comment) systematically over-fires on spyc's
mandated dense "why" docs, and its Rust test-detection misclassifies
unwraps inside large `#[cfg(test)]` blocks — so a raw scan drowns ~2 real
findings in ~78 accepted/false-positive ones. This wrapper records the
accepted findings as per-(rule, file) COUNTS in `.aislop/baseline.json`
and reports only NET-NEW findings, so `make aislop` becomes a real
regression gate.

Counts (not line numbers) are the baseline key on purpose: a comment that
moves or a file that reflows must not resurface as "new" — only an
*additional* finding in a rule/file bucket should. Regenerate the baseline
with `make aislop-baseline` after intentionally accepting new findings.

Usage:
    aislop-baseline.py check     # default: fail on net-new findings
    aislop-baseline.py update    # regenerate .aislop/baseline.json
"""

import json
import subprocess
import sys
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BASELINE = ROOT / ".aislop" / "baseline.json"


def scan():
    """Run aislop and return its parsed diagnostics list."""
    proc = subprocess.run(
        ["aislop", "scan", ".", "--json"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=False,
    )
    try:
        return json.loads(proc.stdout)["diagnostics"]
    except (json.JSONDecodeError, KeyError):
        sys.stderr.write("aislop produced no parseable JSON; raw stderr:\n")
        sys.stderr.write(proc.stderr or "(empty)\n")
        sys.exit(2)


def bucket(diags):
    """diagnostics -> {rule: {file: [line, ...]}} sorted for stable output."""
    out = defaultdict(lambda: defaultdict(list))
    for d in diags:
        out[d["rule"]][d["filePath"]].append(d.get("line", 0))
    return out


def counts(buckets):
    """{rule: {file: [lines]}} -> {rule: {file: count}}."""
    return {
        rule: {f: len(lines) for f, lines in sorted(files.items())}
        for rule, files in sorted(buckets.items())
    }


def cmd_update():
    diags = scan()
    payload = {
        "_comment": (
            "Accepted aislop findings as per-(rule, file) counts. `make "
            "aislop` fails only when a bucket grows beyond its count here. "
            "Regenerate with `make aislop-baseline`. See scripts/"
            "aislop-baseline.py for why counts (not lines) are the key."
        ),
        "counts": counts(bucket(diags)),
    }
    BASELINE.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")
    total = sum(n for files in payload["counts"].values() for n in files.values())
    print(f"wrote {BASELINE.relative_to(ROOT)} — {total} accepted findings")


def cmd_check():
    base = {}
    if BASELINE.exists():
        base = json.loads(BASELINE.read_text()).get("counts", {})
    else:
        print(f"no baseline at {BASELINE.relative_to(ROOT)} — run `make aislop-baseline` first")

    buckets = bucket(scan())
    accepted = sum(n for files in base.values() for n in files.values())

    new_buckets = []   # (rule, file, baseline_n, lines)
    for rule, files in sorted(buckets.items()):
        base_files = base.get(rule, {})
        for f, lines in sorted(files.items()):
            base_n = base_files.get(f, 0)
            if len(lines) > base_n:
                new_buckets.append((rule, f, base_n, sorted(lines)))

    print(f"baseline: {accepted} accepted ({BASELINE.relative_to(ROOT)})")
    if not new_buckets:
        print("✓ no new slop")
        return 0

    total_new = sum(len(lines) - base_n for _, _, base_n, lines in new_buckets)
    print(f"✗ {total_new} NEW finding(s) beyond baseline:\n")
    for rule, f, base_n, lines in new_buckets:
        locs = ", ".join(f"{f}:{ln}" for ln in lines)
        print(f"  {rule}  (baseline {base_n}, now {len(lines)})")
        print(f"    {locs}")
    print("\nreview, then `make aislop-baseline` to accept if intentional")
    return 1


def main():
    mode = sys.argv[1] if len(sys.argv) > 1 else "check"
    if mode == "update":
        cmd_update()
        return 0
    if mode == "check":
        return cmd_check()
    sys.stderr.write(f"unknown mode {mode!r} (expected check|update)\n")
    return 2


if __name__ == "__main__":
    sys.exit(main())
