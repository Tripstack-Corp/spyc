# After a move, the receiving spyc's attention: an agent tab (a fake
# `claude` first on PATH) reports `blocked` through `spyc --mcp`, the project
# moves from A to B, and B's `Space !` should list it, though the agent says
# nothing more.
import json, os, re, shutil, subprocess, sys, time
# `attn.py shell` starts the agent in the shell tab; by default it is a tab of
# its own, as `^a c` starts one.
in_shell = len(sys.argv) > 1 and sys.argv[1] == "shell"
from common import *
T = open(f"{D}/target_dir.txt").read().strip()
J = f"{T}/debug/spyc-jar"
home = f"{D}/home_attn"
shutil.rmtree(home, ignore_errors=True); os.makedirs(home)
open(f"{home}/.spycrc.toml", "w").write('[pane]\ntabs = ["zsh"]\n'); open(f"{home}/.zshrc", "w").close()
other = f"{D}/q11_other"; bdir = f"{D}/q11_b"
for d in (other, bdir): os.makedirs(d, exist_ok=True)
state = os.path.join(os.environ.get("TMPDIR", "/tmp"), "sat")
shutil.rmtree(state, ignore_errors=True); os.makedirs(state)
env = env_for(home); env.update(SPYC_JARS="1", SPYC_JAR_BIN=J, XDG_STATE_HOME=state, SPYC_JAR_TRACE=os.environ.get("GTRACE", ""))
fake = f"{home}/bin"; os.makedirs(fake)
init = json.dumps({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "0"}}})
note = json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"})
req = json.dumps({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "report_status", "arguments": {"status": "blocked", "note": "waits-on-you"}}})
open(f"{fake}/claude", "w").write(f"""#!/bin/sh
(printf '%s\\n' '{init}' '{note}' '{req}'; sleep 2) | {T}/debug/spyc --mcp > {home}/report.log 2>&1
echo fake-claude-waiting
exec {home}/sleeper/claude 1000
""")
os.makedirs(f"{home}/sleeper")
if sys.platform == "darwin":
    os.symlink(shutil.which("sleep"), f"{home}/sleeper/claude")
else:
    # busybox and uutils pick their command by argv[0]: a symlink named
    # `claude` to them isn't `sleep`. A script keeps the name on Linux.
    open(f"{home}/sleeper/claude", "w").write("#!/bin/sh\nwhile :; do sleep 1; done\n")
    os.chmod(f"{home}/sleeper/claude", 0o755)
os.chmod(f"{fake}/claude", 0o755)
env["PATH"] = fake + ":" + env["PATH"]

def call(t, tool, args, tag):
    init = {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "0"}}}
    note = {"jsonrpc": "2.0", "method": "notifications/initialized"}
    req = {"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": tool, "arguments": args}}
    lines = " ".join("'" + json.dumps(m) + "'" for m in (init, note, req))
    t.send(f"clear; (printf '%s\\n' {lines}; sleep 2) | {T}/debug/spyc --mcp | grep -o '{tag}[^\\\\]*' | head -3; echo done-{tag}\r".encode())
    end = time.monotonic() + 15
    while f"done-{tag}" not in t.text().split("echo done-")[-1] and time.monotonic() < end:
        t.pump(0.3)
    t.pump(0.5)
    return [l.strip() for l in t.text().splitlines() if tag in l and "printf" not in l and "done-" not in l]

def jars():
    out = subprocess.run(["ps", "-axo", "pid=,command="], capture_output=True, text=True).stdout
    return [int(l.split()[0]) for l in out.splitlines() if len(l.split()) > 2 and l.split()[1] == J and l.split()[2] in ("--socket", "resume-handover")]

old = set(jars())
a = b = None
def attention(t):
    t.send(b"\x01k"); t.pump(0.5); t.send(b" !"); t.pump(1.5)
    nothing = "nothing needs you" in t.text()
    # `Space !` goes to the agent when one needs you: back to the files.
    t.send(b"\x01k"); t.pump(0.5)
    return "nothing needs you" if nothing else "an agent needs you"
try:
    a = Term([f"{T}/debug/spyc"], env, f"{D}/work", rows=45, cols=150)
    a.pump(8); a.send(b"\x01j"); a.pump(0.8)
    if in_shell:
        a.send(b"claude\r"); a.pump(6)
    else:
        a.send(b"\x01c"); a.pump(0.8); a.send(b"\x15claude\r"); a.pump(0.8); a.send(b"\r"); a.pump(6)
    print("the fake claude reported:", open(f"{home}/report.log").read()[-160:])
    print("A's attention before:", attention(a))
    a.send(b" n"); a.pump(0.5)
    a.send(b"\x15" + f"{other}\r".encode()); a.pump(4)
    a.send(b"  "); a.pump(2)
    a.send(b":detach project\r"); a.pump(1.5)
    mv = os.path.join(state, "spyc", "moves")
    print("the move carries:", [json.load(open(os.path.join(mv, f))).get("statuses") for f in os.listdir(mv)])
    b = Term([f"{T}/debug/spyc"], env, bdir, rows=45, cols=150)
    b.pump(6); b.send(b":attach project\r"); b.pump(6)
    b.send(b"  "); b.pump(1)
    print("B, back on its own project; tab line:", [l.strip()[:80] for l in b.text().splitlines() if l.startswith("─[")][:1])
    print("B's attention after the move:", attention(b))
finally:
    for t in (a, b):
        if t is not None: kill_and_reap(t)
    for p in set(jars()) - old:
        for k in subprocess.run(["pgrep", "-P", str(p)], capture_output=True, text=True).stdout.split():
            try: os.kill(int(k), 9)
            except ProcessLookupError: pass
        try: os.kill(p, 9)
        except ProcessLookupError: pass
