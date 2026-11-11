import os, re, signal, subprocess, time
from common import *
T = open(f"{D}/target_dir.txt").read().strip()
REAL_HOME = "/Users/derekmarshall"
env = env_for(f"{D}/home5")
env["SPYC_JARS"] = "1"
env["SPYC_JAR_BIN"] = f"{T}/debug/spyc-jar"
env["SPYC_JAR_TRACE"] = f"{D}/claudetrace.log"

def numbers(t):
    return [int(x) for x in re.findall(r"(?m)^\s*│?\s*(\d{1,3})\s*│?\s*$", t.text())]

def claude_pids():
    out = subprocess.run(["pgrep", "-f", "-x", "claude"], capture_output=True, text=True).stdout.split()
    return set(out)

before_pids = claude_pids()
a = Term([f"{T}/debug/spyc"], env, f"{D}/work", rows=45, cols=130)
a.pump(8); a.send(b"\x01j"); a.pump(0.8)
a.send(f"HOME={REAL_HOME} claude\r".encode())
end = time.monotonic() + 40
while time.monotonic() < end:
    a.pump(0.5)
    txt = a.text()
    if "I trust this folder" in txt:
        a.send(b"\x1b[B"); a.pump(0.5); a.send(b"\r"); a.pump(3)
    if "for shortcuts" in txt or "? for" in txt or "INSERT" in txt:
        break
ours = claude_pids() - before_pids
print("A: claude up; its pid:", ours)
a.send(b"Print the integers from 1 to 300, one per line, with no other text.")
a.pump(0.5); a.send(b"\r")
seen = 0
end = time.monotonic() + 60
while time.monotonic() < end:
    a.pump(0.3)
    n = numbers(a)
    if n and max(n) >= 30:
        seen = max(n); break
print("A: killing spyc mid-turn; highest number on screen:", seen)
time.sleep(2.5)
kill_and_reap(a); time.sleep(1)
print("claude still running after spyc's kill -9:", ours <= claude_pids())

b = Term([f"{T}/debug/spyc", "-r"], env, f"{D}/work", rows=45, cols=130)
wait_for(b, "sessions —")
b.send(b"j"); b.pump(0.5); b.send(b"\r"); b.pump(8)
n = numbers(b)
print("B: numbers on the restored screen:", (min(n), max(n)) if n else None)
print("B: same claude process:", ours <= claude_pids() and len(claude_pids() - before_pids) == len(ours))
b.send(b"\x01j"); b.pump(0.8)
b.send(b"Reply with just the word pong."); b.pump(0.5); b.send(b"\r")
end = time.monotonic() + 60
got = False
while time.monotonic() < end:
    b.pump(0.5)
    if re.search(r"(?m)^\W*pong\W*$", b.text(), re.I):
        got = True; break
print("B: claude answered a new prompt after the restore:", got)
lines = [l for l in b.text().splitlines() if l.strip()]
print("B: pane tail:"); print("\n".join(lines[-12:]))
kill_and_reap(b)
