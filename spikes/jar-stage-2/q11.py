# Q11: a project moves between spycs (U13) with a streaming Claude tab.
# spyc A opens the project with a Claude tab and a second project, asks
# Claude for a long answer, and detaches the project mid-turn; spyc B attaches
# it. The same jar and Claude should run on into B, the turn finish, and
# spyc's MCP tools answer from B: Claude's spike proxy asks the pane's jar
# where its owner went (R30a). `q11.py partway` instead kills B as it attaches.
import json, os, re, shutil, subprocess, sys, time
from common import *
partway = len(sys.argv) > 1 and sys.argv[1] == "partway"
kill_after = float(sys.argv[2]) if len(sys.argv) > 2 else 0.15
T = open(f"{D}/target_dir.txt").read().strip()
J = f"{T}/debug/spyc-jar"
REAL_HOME = os.environ.get("REAL_HOME", os.path.expanduser("~"))
home = f"{D}/home_q11"
shutil.rmtree(home, ignore_errors=True); os.makedirs(home)
open(f"{home}/.spycrc.toml", "w").write('[pane]\ntabs = ["zsh"]\n'); open(f"{home}/.zshrc", "w").close()
other = f"{D}/q11_other"; bdir = f"{D}/q11_b"
for d in (other, bdir): os.makedirs(d, exist_ok=True)
env = env_for(home); env.update(SPYC_JARS="1", SPYC_JAR_BIN=J)
# A short state directory: spyc's MCP socket lives there, and macOS caps a
# socket's path at 104 bytes.
state = os.path.join(os.environ.get("TMPDIR", "/tmp"), "sq11")
shutil.rmtree(state, ignore_errors=True); os.makedirs(state)
env["XDG_STATE_HOME"] = state
moves_dir = f"{state}/spyc/moves"
env["SPYC_JAR_TRACE"] = f"{D}/q11trace.log"
if os.path.exists(env["SPYC_JAR_TRACE"]): os.remove(env["SPYC_JAR_TRACE"])
# The managed MCP config runs `spyc --mcp` by name, so the spike's build first
# on PATH serves Claude's spyc tools through the spike's proxy.
tool = "mcp__spyc__get_spyc_context"
claude = (f"PATH={T}/debug:$PATH HOME={REAL_HOME} claude --settings '{{\"tui\":\"default\"}}' "
          f"--allowedTools {tool}")

def numbers(t):
    return [int(x) for x in re.findall(r"(?m)^\s*[│⏺]?\s*(\d{1,3})\s*│?\s*$", t.text())]

def jars():
    out = subprocess.run(["ps", "-axo", "pid=,command="], capture_output=True, text=True).stdout
    return [(int(l.split()[0]), l) for l in out.splitlines() if len(l.split()) > 2 and l.split()[1] == J and l.split()[2] in ("--socket", "resume-handover")]

def kill_tree(pid):
    for c in subprocess.run(["pgrep", "-P", str(pid)], capture_output=True, text=True).stdout.split():
        kill_tree(int(c))
    try: os.kill(pid, 9)
    except ProcessLookupError: pass

def describe(sock):
    out = subprocess.run([J, "describe", sock], capture_output=True, text=True).stdout
    return dict(l.split(": ", 1) for l in out.splitlines() if ": " in l)

def ask_pid(t, label):
    t.send(f"Call the {tool} tool and reply with only pid= followed by its pid field, as in pid=NNN.".encode())
    seen = len(re.findall(r"pid=(\d{3,7})\b", t.text()))
    t.pump(0.5); t.send(b"\r")
    end = time.monotonic() + 90
    while time.monotonic() < end:
        t.pump(0.5)
        m = re.findall(r"pid=(\d{3,7})\b", t.text())
        if len(m) > seen:
            return int(m[-1])
    print(f"{label}: no pid answer; screen tail:", [l for l in t.text().splitlines() if l.strip()][-6:])
    return None

def tree(pid):
    kids = subprocess.run(["pgrep", "-P", str(pid)], capture_output=True, text=True).stdout.split()
    return [pid] + [d for k in kids for d in tree(int(k))]

def claude_in(jar_pid):
    comm = lambda p: subprocess.run(["ps", "-o", "comm=", "-p", str(p)], capture_output=True, text=True).stdout.strip()
    return {p for p in tree(jar_pid) if comm(p).endswith("claude")}

old = {p for p, _ in jars()}
a = b = c = None
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
    a.pump(2)
    [(jar, line)] = [j for j in jars() if j[0] not in old]
    ours = claude_in(jar)
    sock = line.split(" --socket ")[1].split()[0]
    print("A: claude", ours, "in jar", jar, "| A's pid", a.pid, "| MCP answers from pid", ask_pid(a, "A"))
    a.send(b"Print the integers from 1 to 400, one per line, with no other text.")
    a.pump(0.5); a.send(b"\r")
    end = time.monotonic() + 60
    while time.monotonic() < end and not any(n >= 30 for n in numbers(a)):
        a.pump(0.3)
    # A second project, so the first can go; then back to the first.
    a.send(b"\x01k"); a.pump(0.5); a.send(b" n"); a.pump(0.5)
    a.send(b"\x15" + f"{other}\r".encode()); a.pump(4)
    a.send(b"  "); a.pump(2)
    a.send(b":detach project\r"); a.pump(1.5)
    print("A after :detach:", [l.strip() for l in a.text().splitlines() if "detach" in l][-1:])
    print("jar after the release:", {k: describe(sock).get(k) for k in ("owner", "held", "owner_mcp")})
    moves = os.listdir(moves_dir)
    print("move files:", len(moves))

    b = Term([f"{T}/debug/spyc"], env, bdir, rows=45, cols=130)
    b.pump(6)
    b.send(b":attach project\r")
    if partway:
        b.pump(kill_after); b_pid = b.pid; kill_and_reap(b); b = None
        time.sleep(1)
        print("B killed while attaching. move files left:", len(os.listdir(moves_dir)))
        print(f"B killed {kill_after} s into :attach. the jar says:", {k: describe(sock).get(k) for k in ("owner", "held", "owner_mcp", "program_pid")})
        r = subprocess.run([f"{T}/debug/spyc", "--jars"], env=env, capture_output=True, text=True).stdout
        mine = [l for l in r.splitlines() if os.path.basename(sock).split(".")[0] in l]
        print("spyc --jars lists the moved jar:", bool(mine), "| owner part:", mine[0].split(" owner ")[1].split("  ")[0] if mine else None)
        c = Term([f"{T}/debug/spyc"], env, bdir, rows=45, cols=130)
        c.pump(6); c.send(b":attach project\r"); c.pump(6)
        print("C after :attach:", [l.strip()[:100] for l in c.text().splitlines() if "attach" in l or "project" in l][-2:])
        print("the jar says:", {k: describe(sock).get(k) for k in ("owner", "owner_mcp")}, "| C's pid", c.pid)
        if describe(sock).get("owner") != "connected":
            # No move file left: B had attached and saved. Its session holds the
            # project: restore that one.
            kill_and_reap(c); c = None
            sessions = f"{state}/spyc/sessions"
            names = [json.load(open(f"{sessions}/{f}")) for f in os.listdir(sessions)]
            name = next((m["name"] for m in names if m.get("pid") == b_pid and any(p.get("label") == "work" for p in m["projects"])), None)
            print("B's saved session holds the moved project:", name)
            if name:
                c = Term([f"{T}/debug/spyc", "-r"], env, bdir, rows=45, cols=130)
                wait_for(c, "sessions —")
                # Each session has a number in the picker; rows wrap.
                n = re.search(rf"\[(\d)\]\s+{name}\b", c.text()).group(1)
                c.send(n.encode()); c.pump(0.5)
                if "sessions —" in c.text():
                    c.send(b"\r")
                c.pump(6)
                print("the restoring spyc's screen:", [l.strip()[:90] for l in c.text().splitlines() if l.strip()][-4:])
                print("after spyc -r of B's session:", {k: describe(sock).get(k) for k in ("owner", "owner_mcp")}, "| its pid", c.pid,
                      "| same claude:", bool(ours) and ours == claude_in(jar))
    else:
        t0 = time.monotonic()
        while time.monotonic() - t0 < 15 and describe(sock).get("owner") != "connected":
            b.pump(0.1)
        print(f"B owned the jar {time.monotonic() - t0:.1f} s after :attach project")
        b.pump(3)
        print("B after :attach:", [l.strip()[:100] for l in b.text().splitlines() if "attach" in l][-1:])
        d = describe(sock)
        print("the jar's owner now:", d.get("owner"), d.get("owner_mcp"), "| B's pid", b.pid)
        print("same jar:", [p for p, _ in jars() if p == jar] == [jar], "| same claude:", bool(ours) and ours == claude_in(jar))
        b.send(b"\x01j"); b.pump(0.5)
        end = time.monotonic() + 120
        while time.monotonic() < end and 400 not in numbers(b):
            b.pump(0.5)
        print("B: the turn finished (400 on screen):", 400 in numbers(b))
        print("B: MCP answers from pid", ask_pid(b, "B"))
        a.pump(1)
        print("A still runs, its other project in view:", a.reap() is None)
finally:
    for t in (a, b, c):
        if t is not None: kill_and_reap(t)
    for p, _ in jars():
        if p not in old: kill_tree(p)
