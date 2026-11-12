# Stage 6 gap: a moved project's scope claims go with it. A shell tab
# registers a claim through `spyc --mcp`, the project moves from A to B, and
# the claim lists in B, not in A.
import json, os, re, shutil, subprocess, time
from common import *
T = open(f"{D}/target_dir.txt").read().strip()
J = f"{T}/debug/spyc-jar"
home = f"{D}/home_scopes"
shutil.rmtree(home, ignore_errors=True); os.makedirs(home)
open(f"{home}/.spycrc.toml", "w").write('[pane]\ntabs = ["zsh"]\n'); open(f"{home}/.zshrc", "w").close()
other = f"{D}/q11_other"; bdir = f"{D}/q11_b"
for d in (other, bdir): os.makedirs(d, exist_ok=True)
state = os.path.join(os.environ.get("TMPDIR", "/tmp"), "ssc")
shutil.rmtree(state, ignore_errors=True); os.makedirs(state)
env = env_for(home); env.update(SPYC_JARS="1", SPYC_JAR_BIN=J, XDG_STATE_HOME=state)

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
try:
    a = Term([f"{T}/debug/spyc"], env, f"{D}/work", rows=45, cols=150)
    a.pump(8); a.send(b"\x01j"); a.pump(0.8)
    print("A registers:", call(a, "register_scope", {"paths": ["src/moved/*.rs"], "intent": "editing", "note": "moves-with-me"}, "claim_id"))
    print("A lists before:", call(a, "list_scopes", {}, "moves-with-me") or "no moved claim")
    a.send(b"\x01k"); a.pump(0.5); a.send(b" n"); a.pump(0.5)
    a.send(b"\x15" + f"{other}\r".encode()); a.pump(4)
    a.send(b"  "); a.pump(2)
    a.send(b":detach project\r"); a.pump(1.5)
    mv = os.path.join(state, "spyc", "moves"); print("move file claims:", [json.load(open(os.path.join(mv, f))).get("claims") for f in os.listdir(mv)])
    a.send(b"\x01j"); a.pump(0.8)
    print("A's other project lists:", call(a, "list_scopes", {}, "moves-with-me") or "no moved claim")
    b = Term([f"{T}/debug/spyc"], env, bdir, rows=45, cols=150)
    b.pump(6); b.send(b":attach project\r"); b.pump(6)
    b.send(b"\x01j"); b.pump(0.8)
    print("B's moved tab lists:", call(b, "list_scopes", {}, "moves-with-me") or "no moved claim")
finally:
    for t in (a, b):
        if t is not None: kill_and_reap(t)
    for p in set(jars()) - old:
        for k in subprocess.run(["pgrep", "-P", str(p)], capture_output=True, text=True).stdout.split():
            try: os.kill(int(k), 9)
            except ProcessLookupError: pass
        try: os.kill(p, 9)
        except ProcessLookupError: pass
