import os, signal, subprocess, time
from common import *
T = open(f"{D}/target_dir.txt").read().strip()
env = env_for(f"{D}/home2")
env["SPYC_JARS"] = "1"
env["SPYC_JAR_BIN"] = f"{T}/debug/spyc-jar"
env["SPYC_JAR_TRACE"] = f"{D}/jartrace.log"
a = Term([f"{T}/debug/spyc"], env, f"{D}/work", rows=40, cols=120)
a.pump(8); a.send(b"\x01j"); a.pump(0.8)
a.send(b"export MARK=survived-$$; echo before-crash\r"); a.pump(4)
zsh_before = subprocess.run(["pgrep", "-f", "-l", "zsh"], capture_output=True, text=True).stdout
print("A: typed; saw before-crash:", "before-crash" in a.text())
os.kill(a.pid, signal.SIGKILL); os.waitpid(a.pid, 0); time.sleep(1)
jars = subprocess.run(["pgrep", "-f", "spyc-jar --socket"], capture_output=True, text=True).stdout.split()
print("after kill -9 of spyc A, jars alive:", jars)

b = Term([f"{T}/debug/spyc", "-r"], env, f"{D}/work", rows=40, cols=120)
wait_for(b, "sessions —")
b.send(b"j"); b.pump(0.5); b.send(b"\r"); b.pump(6)
print("B: restored screen shows before-crash (replayed):", "before-crash" in b.text())
b.send(b"\x01j"); b.pump(0.8)
b.send(b"echo mark=$MARK\r"); b.pump(3)
txt = b.text()
line = [l for l in txt.splitlines() if l.startswith("mark=")]
print("B: the restored tab says:", line)
jars_after = subprocess.run(["pgrep", "-f", "spyc-jar --socket"], capture_output=True, text=True).stdout.split()
print("jars now:", jars_after, "(same as before:", jars_after == jars, ")")
os.kill(b.pid, signal.SIGKILL)
