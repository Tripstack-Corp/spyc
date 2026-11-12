# Decision G: a spyc stopped past its jar's deadline loses the lease. Resumed,
# it takes the jar back when no one else has (free), and marks the tab
# detached when another spyc took it meanwhile (held).
#   g.py free | held
import os, re, shutil, signal, subprocess, sys, time
from common import *
which = sys.argv[1]
T = open(f"{D}/target_dir.txt").read().strip()
J = f"{T}/debug/spyc-jar"
home = f"{D}/home_g"
shutil.rmtree(home, ignore_errors=True); os.makedirs(home)
open(f"{home}/.spycrc.toml", "w").write('[pane]\ntabs = ["zsh"]\n'); open(f"{home}/.zshrc", "w").close()
state = os.path.join(os.environ.get("TMPDIR", "/tmp"), "sg")
shutil.rmtree(state, ignore_errors=True); os.makedirs(state)
env = env_for(home); env.update(SPYC_JARS="1", SPYC_JAR_BIN=J, XDG_STATE_HOME=state, SPYC_JAR_TRACE=os.environ.get("GTRACE", ""))

def jars():
    out = subprocess.run(["ps", "-axo", "pid=,command="], capture_output=True, text=True).stdout
    return [(int(l.split()[0]), l) for l in out.splitlines() if len(l.split()) > 2 and l.split()[1] == J and l.split()[2] in ("--socket", "resume-handover")]

def owner(sock):
    out = subprocess.run([J, "describe", sock], capture_output=True, text=True).stdout
    return next((l.split(": ", 1)[1] for l in out.splitlines() if l.startswith("owner: ")), "?")

def tab_line(t):
    return next((l.strip()[:60] for l in t.text().splitlines() if l.startswith("─[1]")), None)

def last_count(t):
    n = [int(x) for x in re.findall(r"(?m)^\s*(17\d{8})\s*$", t.text())]
    return max(n) if n else None

old = {p for p, _ in jars()}
a = b = None
try:
    a = Term([f"{T}/debug/spyc"], env, f"{D}/work", rows=40, cols=120)
    a.pump(8); a.send(b"\x01j"); a.pump(0.8)
    a.send(b"while true; do date +%s; sleep 0.5; done\r"); a.pump(4)
    [(jar, line)] = [j for j in jars() if j[0] not in old]
    sock = line.split(" --socket ")[1].split()[0]
    print("A owns its jar:", owner(sock))
    os.kill(a.pid, signal.SIGSTOP)
    time.sleep(12)
    print("A stopped 12 s; the jar's owner:", owner(sock))
    if which == "held":
        # Another owner with the jar's token takes it meanwhile. `spyc -r`
        # won't: it restores a stopped spyc's session as a copy, with panes of
        # its own. `spyc-jar attach` stands in for a takeover (decision D
        # puts the real one after v1).
        b = Term([J, "attach", sock, token_file(home, sock, state)], env, f"{D}/work", rows=40, cols=120)
        b.pump(3)
        print("B took the jar; its owner:", owner(sock), "| B shows the loop:", last_count(b) is not None)
    n0 = last_count(a)
    os.kill(a.pid, signal.SIGCONT)
    a.pump(6)
    # spyc redraws after an outside SIGCONT only on its next input, jars or
    # not: a key it ignores brings the screen up to date.
    a.send(b"\x1b"); a.pump(1)
    print("A resumed; the jar's owner:", owner(sock), "| A's tab:", tab_line(a))
    if which == "free":
        a.send(b"\x03"); a.pump(0.5)
        n1 = last_count(a)
        print("A's pane holds what the program printed meanwhile:", n0, "->", n1, bool(n0 and n1 and n1 >= n0 + 10))
        a.send(b"echo g-$((6*7))\r"); a.pump(2)
        print("A types into its pane again:", "g-42" in a.text())
    else:
        print("B still owns it, A says detached:", "[detached]" in (tab_line(a) or ""))
        b.send(b"\x03"); b.pump(0.5); b.send(b"echo g-$((6*7))\r"); b.pump(2)
        print("B types into the pane:", "g-42" in b.text())
        a.send(b"\x01x"); a.pump(1)
        b.send(b"echo still-$((6*7))\r"); b.pump(2)
        print("A closing its detached tab left the program running:", "still-42" in b.text())
finally:
    for t in (a, b):
        if t is not None: kill_and_reap(t)
    for p, _ in jars():
        if p not in old:
            for k in subprocess.run(["pgrep", "-P", str(p)], capture_output=True, text=True).stdout.split():
                try: os.kill(int(k), 9)
                except ProcessLookupError: pass
            try: os.kill(p, 9)
            except ProcessLookupError: pass
