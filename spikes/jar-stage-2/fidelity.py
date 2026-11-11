import os, signal, subprocess, sys, time
from common import *
T = open(f"{D}/target_dir.txt").read().strip()
jars = sys.argv[1] == "jars"
home = f"{D}/home4"
env = env_for(home)
if jars:
    env["SPYC_JARS"] = "1"
    env["SPYC_JAR_BIN"] = f"{T}/debug/spyc-jar"
def pane_rows(t):
    rows = t.text().splitlines()
    i = next(k for k, l in enumerate(rows) if l.startswith("─[1]"))
    return [r.rstrip() for r in rows[i + 1:]]
a = Term([f"{T}/debug/spyc"], env, f"{D}/work", rows=40, cols=120)
a.pump(8); a.send(b"\x01j"); a.pump(0.8)
a.send(b"less long.txt\r"); a.pump(2); a.send(b"   "); a.pump(1.5)
before = pane_rows(a)
time.sleep(2.5)
os.kill(a.pid, signal.SIGKILL); os.waitpid(a.pid, 0); time.sleep(1)
b = Term([f"{T}/debug/spyc", "-r"], env, f"{D}/work", rows=40, cols=120)
wait_for(b, "sessions —"); b.send(b"j"); b.pump(0.3)
t0 = time.monotonic(); b.send(b"\r")
target = next((l for l in before if "line number" in l), "line number")
gap = None
while time.monotonic() - t0 < 15:
    b.pump(0.05)
    if target in b.text():
        gap = time.monotonic() - t0
        break
b.pump(2)
after = pane_rows(b)
same = sum(1 for x, y in zip(before, after) if x == y)
print(f"{'jars' if jars else 'no jars'}: pane content back {gap:.2f}s after picking the session" if gap else "content never came back")
print(f"  pane rows identical before/after: {same}/{max(len(before), len(after))}")
print("  before:", before[:3], "...")
print("  after: ", after[:3], "...")
os.kill(b.pid, signal.SIGKILL)
