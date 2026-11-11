import os, sys, time
from common import *
T = open(f"{D}/target_dir.txt").read().strip()
build = sys.argv[1]
jars = sys.argv[2] == "jars"
env = env_for(f"{D}/home8")
if jars:
    env.update(SPYC_JARS="1", SPYC_JAR_BIN=f"{T}/{build}/spyc-jar")
out = []
for i in range(5):
    t0 = time.monotonic()
    t = Term([f"{T}/{build}/spyc"], env, f"{D}/work", rows=40, cols=120)
    first = prompt = None
    while time.monotonic() - t0 < 20:
        t.pump(0.01)
        if first is None and "menu" in t.text():
            first = time.monotonic() - t0
        if "work %" in t.text():
            prompt = time.monotonic() - t0
            break
    out.append((first, prompt))
    kill_and_reap(t)
    time.sleep(0.3)
fs = sorted(f for f, _ in out if f); ps = sorted(p for _, p in out if p)
print(f"{build} spyc, {'jars' if jars else 'no jars'}: first frame median {fs[len(fs)//2]*1000:.0f} ms, shell prompt in the pane median {ps[len(ps)//2]*1000:.0f} ms (5 runs)")
