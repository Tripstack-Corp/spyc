import sys, time, os, signal, subprocess
sys.path.insert(0, "/scripts-b")
from jarprobe import *
B = sys.argv[1]
cases = [
 ("hup-ignoring child, same group", "(trap '' HUP; exec sleep 30) & echo bg-started; exit 3"),
 ("job-control shell, child in own group", "set -m; sleep 30 & echo bg-started; exit 3"),
 ("nohup server", "nohup sleep 30 >/dev/null 2>&1 & echo bg-started; exit 3"),
 ("nohup server, output to terminal", "nohup sleep 30 & echo bg-started; exit 3"),
]
for name, script in cases:
    for linger in (None, 2**64-1):
        p, s = start_jar(B, ["/bin/sh", "-c", script], exit_linger_ms=(None if linger is None else str(linger)))
        o = Conn(s, "owner"); o.replay(0)
        t0 = time.time(); exited = None; desc = None
        while time.time() - t0 < 8:
            m = o.recv(0.1)
            if m and m[0] == "output": o.ack(m[1] + len(m[2]))
            if m and m[0] == "exited" and exited is None: exited = time.time() - t0
            if desc is None and describe(s).get("exit") == "3": desc = time.time() - t0
            if exited and desc: break
        o.signal(3)
        ans = None; t1 = time.time()
        while time.time() - t1 < 2 and ans is None:
            m = o.recv(0.1)
            if m and m[0] in ("refused",): ans = m
        print(f"H1 linux [{name}] exit_linger={'until-read' if linger else 'default'}: describe exit=3 after {desc}, owner told 'exited' after {exited} (watched 8 s), kill then: {ans}", flush=True)
        o.close()
        # the jar after its owner goes: does it end?
        t2 = time.time()
        while time.time() - t2 < 3 and p.poll() is None: time.sleep(0.1)
        print(f"   owner gone 3 s: jar running={p.poll() is None}", flush=True)
        end(p, s)
        subprocess.run(["pkill", "-f", "sleep 30"])
