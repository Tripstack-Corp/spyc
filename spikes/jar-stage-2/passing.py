# Two of the spike's passing checks not covered by a stage:
#   passing.py mcp   get_spyc_context from a Claude tab after a `kill -9` of its
#                    spyc and a `spyc -r`, the agent never restarted (R30a).
#   passing.py jar   `kill -9` of a pane's jar: the program gets SIGHUP, and
#                    spyc shows the tab as ended rather than hanging.
import os, re, shutil, signal, subprocess, sys, time
from common import *
which = sys.argv[1]
T = open(f"{D}/target_dir.txt").read().strip()
J = f"{T}/debug/spyc-jar"
REAL_HOME = os.environ.get("REAL_HOME", os.path.expanduser("~"))
home = f"{D}/home_pass"
shutil.rmtree(home, ignore_errors=True); os.makedirs(home)
open(f"{home}/.spycrc.toml", "w").write('[pane]\ntabs = ["zsh"]\n'); open(f"{home}/.zshrc", "w").close()
state = os.path.join(os.environ.get("TMPDIR", "/tmp"), "spass")
shutil.rmtree(state, ignore_errors=True); os.makedirs(state)
env = env_for(home); env.update(SPYC_JARS="1", SPYC_JAR_BIN=J, XDG_STATE_HOME=state)
tool = "mcp__spyc__get_spyc_context"

def jars():
    out = subprocess.run(["ps", "-axo", "pid=,command="], capture_output=True, text=True).stdout
    return [(int(l.split()[0]), l) for l in out.splitlines() if len(l.split()) > 2 and l.split()[1] == J and l.split()[2] in ("--socket", "resume-handover")]

def tree(pid):
    kids = subprocess.run(["pgrep", "-P", str(pid)], capture_output=True, text=True).stdout.split()
    return [pid] + [d for k in kids for d in tree(int(k))]

def kill_tree(pid):
    for p in reversed(tree(pid)):
        try: os.kill(p, 9)
        except ProcessLookupError: pass

def ask_pid(t, again=False):
    fresh = " The answer may have changed since you last asked, so call the tool again now; don't reuse an earlier result." if again else ""
    t.send(f"Call the {tool} tool and reply with only pid= followed by its pid field, as in pid=NNN.{fresh}".encode())
    seen = len(re.findall(r"pid=(\d{3,7})\b", t.text()))
    t.pump(0.5); t.send(b"\r")
    end = time.monotonic() + 120
    while time.monotonic() < end:
        t.pump(0.5)
        m = re.findall(r"pid=(\d{3,7})\b", t.text())
        if len(m) > seen: return int(m[-1])
    return None

old = {p for p, _ in jars()}
a = b = None
try:
    a = Term([f"{T}/debug/spyc"], env, f"{D}/work", rows=45, cols=130)
    a.pump(8); a.send(b"\x01j"); a.pump(0.8)
    if which == "mcp":
        a.send(f"PATH={T}/debug:$PATH HOME={REAL_HOME} claude --settings '{{\"tui\":\"default\"}}' --allowedTools {tool}\r".encode())
        end = time.monotonic() + 40
        while time.monotonic() < end:
            a.pump(0.5)
            if "I trust this folder" in a.text():
                a.send(b"\x1b[B"); a.pump(0.5); a.send(b"\r"); a.pump(3)
            if "for shortcuts" in a.text() or "? for" in a.text(): break
        a.pump(2)
        print("before: MCP answers from", ask_pid(a), "| spyc A is", a.pid)
        a.pump(3)  # an autosave of the session to restore
        kill_and_reap(a); a = None
        b = Term([f"{T}/debug/spyc", "-r"], env, f"{D}/work", rows=45, cols=130)
        wait_for(b, "sessions —"); b.send(b"\r"); b.pump(8)
        b.send(b"\x01j"); b.pump(0.5)
        print("after kill -9 and spyc -r: MCP answers from", ask_pid(b, again=True), "| spyc B is", b.pid)
    else:
        a.send(b"sleep 1000\r"); a.pump(1.5)
        [(jar, _)] = [j for j in jars() if j[0] not in old]
        program = [p for p in tree(jar) if p != jar]
        os.kill(jar, signal.SIGKILL)
        t0 = time.monotonic()
        while time.monotonic() - t0 < 5 and any(subprocess.run(["kill", "-0", str(p)], capture_output=True).returncode == 0 for p in program):
            a.pump(0.1)
        gone = time.monotonic() - t0
        a.pump(2)
        tail = [l.strip()[:100] for l in a.text().splitlines() if l.strip()]
        print(f"jar killed: its program gone in {gone:.1f} s:", gone < 5)
        print("spyc still answers keys:", a.reap() is None, "| the tab bar:", [l for l in tail if "─[" in l][-1:])
finally:
    for t in (a, b):
        if t is not None: kill_and_reap(t)
    for p, _ in jars():
        if p not in old: kill_tree(p)
