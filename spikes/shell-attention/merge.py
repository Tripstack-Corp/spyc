"""Merge the Linux run's per-scope files into observation-linux.json,
access-linux.json and cost-linux.json. Usage: merge.py SPIKE_DIR."""
import json
import os
import sys

d = sys.argv[1]


def load(name):
    with open(os.path.join(d, name)) as f:
        return json.load(f)


obs = [load(f".obs-{s}.json") for s in "012"]
out = {"meta": {s: o["meta"] for s, o in zip("012", obs)},
       "not_covered": obs[0]["not_covered"],
       "rows": [r for o in obs for r in o["rows"]]}
with open(os.path.join(d, "observation-linux.json"), "w") as f:
    json.dump(out, f, indent=1)
acc = [load(f".access-{s}.json") for s in "012"]
with open(os.path.join(d, "access-linux.json"), "w") as f:
    json.dump({"meta": acc[0]["meta"], "rows": [r for a in acc for r in a["rows"]]}, f, indent=1)
with open(os.path.join(d, "cost-linux.json"), "w") as f:
    json.dump(load(".cost.json"), f, indent=1)
