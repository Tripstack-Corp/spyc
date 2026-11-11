# Q7: is a redraw enough after `drop` loses events? A Claude tab prints 1..600
# while spyc is dead and the jar's ring is small, so the jar drops the oldest
# bytes; a restored spyc then makes Claude repaint. Compared with a ring big
# enough to drop nothing, and between the two ways across the gap: a blank
# engine fed what's left (`blank`), or the newest snapshot under it (`stale`).
#
#   q7.py inline|fullscreen RING blank|stale
import json, os, re, shutil, subprocess, sys, time
from common import *
mode, ring, gap = sys.argv[1], sys.argv[2], sys.argv[3]
T = open(f"{D}/target_dir.txt").read().strip()
J = f"{T}/debug/spyc-jar"
REAL_HOME = os.environ.get("REAL_HOME", os.path.expanduser("~"))
home = f"{D}/home_q7"
shutil.rmtree(home, ignore_errors=True); os.makedirs(home)
open(f"{home}/.spycrc.toml", "w").write('[pane]\ntabs = ["zsh"]\n'); open(f"{home}/.zshrc", "w").close()
trace = f"{D}/q7trace.log"
if os.path.exists(trace): os.remove(trace)
env = env_for(home)
env.update(SPYC_JARS="1", SPYC_JAR_BIN=J, SPYC_JAR_RING=ring, SPYC_JAR_TRACE=trace, SPYC_JAR_GAP=gap)
tui = "fullscreen" if mode == "fullscreen" else "default"
claude = f"HOME={REAL_HOME} claude --settings '{{\"tui\":\"{tui}\"}}'"

def numbers(t):
    return [int(x) for x in re.findall(r"(?m)^\s*[│⏺]?\s*(\d{1,3})\s*│?\s*$", t.text())]

def jars():
    out = subprocess.run(["ps", "-axo", "pid=,command="], capture_output=True, text=True).stdout
    return [(int(l.split()[0]), l) for l in out.splitlines() if len(l.split()) > 2 and l.split()[1] == J and l.split()[2] == "--socket"]

def kill_tree(pid):
    for c in subprocess.run(["pgrep", "-P", str(pid)], capture_output=True, text=True).stdout.split():
        kill_tree(int(c))
    try: os.kill(pid, 9)
    except ProcessLookupError: pass

def describe(sock):
    out = subprocess.run([J, "describe", sock], capture_output=True, text=True).stdout
    return dict(l.split(": ", 1) for l in out.splitlines() if ": " in l)

old = {p for p, _ in jars()}
a = b = None
try:
    a = Term([f"{T}/debug/spyc"], env, f"{D}/work", rows=45, cols=130)
    a.pump(8); a.send(b"\x01j"); a.pump(0.8)
    a.send(claude.encode() + b"\r")
    end = time.monotonic() + 40
    while time.monotonic() < end:
        a.pump(0.5)
        txt = a.text()
        if "I trust this folder" in txt:
            a.send(b"\x1b[B"); a.pump(0.5); a.send(b"\r"); a.pump(3)
        if "for shortcuts" in txt or "? for" in txt:
            break
    a.pump(3)  # quiet, so a snapshot of the ready prompt is stored
    [(jar, line)] = [j for j in jars() if j[0] not in old]
    sock = line.split(" --socket ")[1].split()[0]
    a.send(b"Print the integers from 1 to 600, one per line, with no other text.")
    a.pump(0.5); a.send(b"\r")
    end = time.monotonic() + 60
    while time.monotonic() < end and not any(n >= 40 for n in numbers(a)):
        a.pump(0.3)
    kill_and_reap(a); a = None
    # Claude finishes with nobody watching: wait until it stops printing.
    last, still = None, 0
    end = time.monotonic() + 150
    while time.monotonic() < end and still < 4:
        time.sleep(1)
        log = describe(sock).get("log", "")
        still = still + 1 if log == last else 0
        last = log
    m = re.match(r"(\d+) of \d+ bytes held, (\d+) printed, from (\d+)", last or "")
    printed, base = (int(m.group(2)), int(m.group(3))) if m else (None, None)
    snaps = [int(x.split("@")[1]) for x in describe(sock).get("snapshots", "").split()]
    # Lost: what lies between the newest snapshot and the oldest byte held.
    lost = None if base is None else max(0, base - max(snaps, default=0))

    b = Term([f"{T}/debug/spyc", "-r"], env, f"{D}/work", rows=45, cols=130)
    wait_for(b, "sessions —"); b.send(b"\r"); b.pump(8)
    on_screen = numbers(b)
    modes = [l.split("modes: ", 1)[1].strip() for l in open(trace) if f"spyc {b.pid} modes:" in l]
    hist_top, oldest = [], None
    if mode == "inline":
        b.send(b"\x01j"); b.pump(0.5); b.send(b"\x01v"); b.pump(2); b.send(b"g"); b.pump(1.5)
        rows = b.text().splitlines()
        at = next((i for i, l in enumerate(rows) if "[SCROLL]" in l), None)
        pane = rows[at + 1:] if at is not None else []
        hist_top = [l.strip()[:50] for l in pane[:3]]
        nums = [int(m.group(1)) for l in pane for m in [re.match(r"\s*\d+\s+[⏺│ ]*(\d{1,3})\s*$", l)] if m]
        oldest = min(nums) if nums else None
        b.send(b"q"); b.pump(1)
    b.send(b"\x01j"); b.pump(0.5)
    b.send(b"Reply with just the word pong."); b.pump(0.5); b.send(b"\r")
    pong = False
    end = time.monotonic() + 60
    while time.monotonic() < end and not pong:
        b.pump(0.5)
        pong = bool(re.search(r"(?m)^\W*pong\W*$", b.text(), re.I))
    print(json.dumps({
        "mode": mode, "ring": int(ring), "gap": gap,
        "printed": printed, "lost": lost,
        "600_on_screen": 600 in on_screen, "highest_on_screen": max(on_screen) if on_screen else None,
        "modes_after_restore": modes[-1] if modes else None,
        "history_top": hist_top, "oldest_number_in_history": oldest,
        "pong": pong,
    }))
finally:
    for t in (a, b):
        if t is not None: kill_and_reap(t)
    for p, _ in jars():
        if p not in old: kill_tree(p)
