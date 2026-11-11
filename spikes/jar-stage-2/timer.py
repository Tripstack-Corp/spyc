# R12c's timer: a pane printing steadily, never quiet for a second, still gets
# a fresh snapshot every five seconds. Watched through the jar's description.
import os, shutil, subprocess, time
from common import *
T = open(f"{D}/target_dir.txt").read().strip()
J = f"{T}/debug/spyc-jar"
home = f"{D}/home_timer"
shutil.rmtree(home, ignore_errors=True); os.makedirs(home)
open(f"{home}/.spycrc.toml", "w").write('[pane]\ntabs = ["zsh"]\n'); open(f"{home}/.zshrc", "w").close()
env = env_for(home); env.update(SPYC_JARS="1", SPYC_JAR_BIN=J)
def jars():
    out = subprocess.run(["ps", "-axo", "pid=,command="], capture_output=True, text=True).stdout
    return [(int(l.split()[0]), l) for l in out.splitlines() if len(l.split()) > 2 and l.split()[1] == J and l.split()[2] == "--socket"]
old = {p for p, _ in jars()}
a = Term([f"{T}/debug/spyc"], env, f"{D}/work", rows=40, cols=120)
try:
    a.pump(6); a.send(b"\x01j"); a.pump(0.8)
    [(jar, line)] = [j for j in jars() if j[0] not in old]
    sock = line.split(" --socket ")[1].split()[0]
    a.send(b"for i in $(seq 1 70); do echo tick $i; sleep 0.2; done\r")
    seen = []
    for _ in range(7):
        a.pump(2)
        out = subprocess.run([J, "describe", sock], capture_output=True, text=True).stdout
        snaps = dict(l.split(": ", 1) for l in out.splitlines() if ": " in l).get("snapshots", "")
        seen.append(max((int(s.split("@")[1]) for s in snaps.split()), default=0))
    print("newest snapshot offset every 2 s while printing:", seen)
    print("fresh snapshots during the stream:", len(set(seen)) - 1)
finally:
    kill_and_reap(a)
    for p, _ in jars():
        if p not in old:
            subprocess.run(["pkill", "-9", "-P", str(p)]); os.kill(p, 9)
