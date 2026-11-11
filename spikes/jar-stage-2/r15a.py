import os, signal, subprocess, time
from common import *
T = open(f"{D}/target_dir.txt").read().strip()
env = env_for(f"{D}/home3")
env["SPYC_JARS"] = "1"
env["SPYC_JAR_BIN"] = f"{T}/debug/spyc-jar"
env["SPYC_JAR_TRACE"] = f"{D}/jartrace.log"
a = Term([f"{T}/debug/spyc"], env, f"{D}/work", rows=40, cols=120)
a.pump(8); a.send(b"\x01j"); a.pump(0.8); a.send(f"{D}/probe.sh\r".encode()); a.pump(4)
print("A:", [l for l in a.text().splitlines() if l.startswith("first-ok")])
time.sleep(2.5)  # past the autosave debounce
os.kill(a.pid, signal.SIGKILL); time.sleep(1)
b = Term([f"{T}/debug/spyc", "-r"], env, f"{D}/work", rows=40, cols=120)
b.pump(4); b.send(b"j"); b.pump(0.5); b.send(b"\r"); b.pump(5)
b.send(b"\x01j"); b.pump(0.8); b.send(b"X"); b.pump(2)
print("B:", [l for l in b.text().splitlines() if l.startswith("second=")])
os.kill(b.pid, signal.SIGKILL)
