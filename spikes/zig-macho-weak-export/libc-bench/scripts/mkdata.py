#!/usr/bin/env python3
"""Deterministic workload data. usage: mkdata.py ROOT
ROOT/bigdir: 100,000 empty files. ROOT/merge/{base,ours,theirs}: 1,000,000-line
text files whose edits don't overlap, so the three-way merge is clean."""
import os, random, sys
root = sys.argv[1]
d = os.path.join(root, "bigdir"); os.makedirs(d, exist_ok=True)
for i in range(100_000):
    open(os.path.join(d, f"file-{i:06d}-{(i * 7919) % 100003:05d}.txt"), "w").close()
m = os.path.join(root, "merge"); os.makedirs(m, exist_ok=True)
rng = random.Random(1129)
words = ["alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota", "kappa"]
base = [" ".join(rng.choice(words) for _ in range(8)) + f" {i}" for i in range(1_000_000)]
ours = [l + " OURS" if i % 1000 == 0 else l for i, l in enumerate(base)]
theirs = [l + " THEIRS" if i % 1000 == 500 else l for i, l in enumerate(base)]
for name, lines in (("base", base), ("ours", ours), ("theirs", theirs)):
    with open(os.path.join(m, name), "w") as f:
        f.write("\n".join(lines) + "\n")
