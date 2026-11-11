import os, re, sys, time
from common import *
T = open(f"{D}/target_dir.txt").read().strip()
snap = sys.argv[1] == "snap"
env = env_for(f"{D}/home6")
env.update(SPYC_JARS="1", SPYC_JAR_BIN=f"{T}/debug/spyc-jar", SPYC_JAR_RING="8192")
if not snap:
    env["SPYC_JAR_NO_SNAPSHOT"] = "1"
a = Term([f"{T}/debug/spyc"], env, f"{D}/work", rows=40, cols=120)
a.pump(8); a.send(b"\x01j"); a.pump(0.8)
a.send(b"seq 1 3000; echo seq-done\r"); a.pump(3)
time.sleep(2.5)  # the quiet-time snapshot, then the autosave
kill_and_reap(a)
b = Term([f"{T}/debug/spyc", "-r"], env, f"{D}/work", rows=40, cols=120)
wait_for(b, "sessions —"); b.send(b"j"); b.pump(0.5); b.send(b"\r"); b.pump(6)
live = "seq-done" in b.text()
b.send(b"\x01j"); b.pump(0.5); b.send(b"\x01v"); b.pump(2); b.send(b"g"); b.pump(1.5)
rows = b.text().splitlines()
pane = rows[next(i for i, l in enumerate(rows) if "[SCROLL]" in l) + 1:]
print("  top of history:", [l.strip() for l in pane[:3]])
first = [int(m.group(1)) for l in pane for m in [re.match(r"\s*\d+ (\d{1,4})\s*$", l)] if m]
print(f"{'with' if snap else 'without'} snapshots: restored pane shows seq-done: {live}; "
      f"oldest number in its history: {min(first) if first else None}")
kill_and_reap(b)
