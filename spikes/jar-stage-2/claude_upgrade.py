# Q9: a jar replaces its own binary (R33) under a streaming Claude tab in spyc.
# Claude prints 1..300; once it's past 30, the tab's jar is upgraded to a copy
# of its binary. The same jar process and the same Claude should run on, the
# turn finish, and a new prompt get an answer.
import os, re, shutil, subprocess, time
from common import *
T = open(f"{D}/target_dir.txt").read().strip()
J = f"{T}/debug/spyc-jar"
REAL_HOME = os.environ.get("REAL_HOME", os.path.expanduser("~"))
env = env_for(f"{D}/home5")
env["SPYC_JARS"] = "1"
env["SPYC_JAR_BIN"] = J
NEXT = f"{D}/jar-next"
shutil.copy(J, NEXT)

def numbers(t):
    return [int(x) for x in re.findall(r"(?m)^\s*│?\s*(\d{1,3})\s*│?\s*$", t.text())]

def jars():
    out = subprocess.run(["ps", "-axo", "pid=,command="], capture_output=True, text=True).stdout
    return [(int(l.split()[0]), l) for l in out.splitlines()
            if len(l.split()) > 2 and l.split()[1] in (J, NEXT) and l.split()[2] in ("--socket", "resume-handover")]

def claude_pids():
    return set(subprocess.run(["pgrep", "-f", "-x", "claude"], capture_output=True, text=True).stdout.split())

def kill_tree(pid):
    for c in subprocess.run(["pgrep", "-P", str(pid)], capture_output=True, text=True).stdout.split():
        kill_tree(int(c))
    try:
        os.kill(pid, 9)
    except ProcessLookupError:
        pass

before = claude_pids()
old_jars = {p for p, _ in jars()}
a = None
try:
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
    ours = claude_pids() - before
    [(jar_pid, line)] = [j for j in jars() if j[0] not in old_jars]
    socket = line.split(" --socket ")[1].split()[0]
    print("claude up:", ours, "| its jar:", jar_pid)
    a.send(b"Print the integers from 1 to 300, one per line, with no other text.")
    a.pump(0.5); a.send(b"\r")
    end = time.monotonic() + 60
    while time.monotonic() < end:
        a.pump(0.3)
        n = numbers(a)
        if n and max(n) >= 30:
            break
    print("upgrading mid-turn; highest number on screen:", max(numbers(a) or [0]))
    r = subprocess.run([J, "upgrade", socket, NEXT], capture_output=True, text=True, timeout=30)
    print("upgrade to a binary never run before:", (r.stdout or r.stderr).strip())
    a.pump(1)
    r = subprocess.run([J, "upgrade", socket, J], capture_output=True, text=True, timeout=30)
    print("upgrade back to a binary already run:", (r.stdout or r.stderr).strip())
    r = subprocess.run([J, "upgrade", socket, NEXT], capture_output=True, text=True, timeout=30)
    print("and to jar-next again:", (r.stdout or r.stderr).strip())
    after = jars()
    mine = [l for p, l in after if p == jar_pid]
    print("same jar process, now running jar-next:", bool(mine) and mine[0].split()[1] == NEXT)
    end = time.monotonic() + 120
    while time.monotonic() < end:
        a.pump(0.5)
        if 300 in numbers(a):
            break
    print("the turn finished (300 on screen):", 300 in numbers(a), "| same claude:", ours <= claude_pids())
    a.send(b"Reply with just the word pong."); a.pump(0.5); a.send(b"\r")
    end = time.monotonic() + 60
    got = False
    while time.monotonic() < end:
        a.pump(0.5)
        if re.search(r"(?m)^\W*pong\W*$", a.text(), re.I):
            got = True; break
    print("claude answered a new prompt through the upgraded jar:", got)
finally:
    if a is not None:
        kill_and_reap(a)
    for p, _ in jars():
        if p not in old_jars:
            kill_tree(p)
