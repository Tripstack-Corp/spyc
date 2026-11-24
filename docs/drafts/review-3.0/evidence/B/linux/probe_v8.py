import sys, time, os, threading, subprocess
sys.path.insert(0, "/scripts-b")
from jarprobe import *
B = sys.argv[1]; SECS = int(sys.argv[2]) if len(sys.argv) > 2 else 10
jars = []; stop = False
def pinger(c):
    n = 0
    while not stop:
        n += 1; c.ping(n)
        t = time.time()
        while time.time() - t < 2:
            m = c.recv(0.5)
for i in range(10):
    p, s = start_jar(B, ["/bin/sleep", "100000"])
    c = Conn(s, "owner"); c.replay(0)
    if i < 5:
        threading.Thread(target=pinger, args=(c,), daemon=True).start(); kind = "owned"
    else:
        c.close(); kind = "ownerless"
    jars.append((p, s, kind))
time.sleep(3)
procs = []
for p, s, kind in jars:
    out = f"/tmp/strace-{p.pid}.txt"
    procs.append((subprocess.Popen(["strace", "-f", "-c", "-o", out, "-p", str(p.pid)], stderr=subprocess.DEVNULL), out, p, kind))
time.sleep(SECS)
for sp, out, p, kind in procs:
    sp.terminate(); sp.wait()
for sp, out, p, kind in procs:
    txt = open(out).read()
    total = [l for l in txt.splitlines() if l.strip().endswith("total")]
    calls = {}
    for l in txt.splitlines():
        f = l.split()
        if len(f) >= 5 and f[-1].isidentifier() and f[3].isdigit():
            calls[f[-1]] = int(f[3])
    thr = len(os.listdir(f"/proc/{p.pid}/task"))
    tot = sum(calls.values())
    print(f"V8 linux jar {p.pid} {kind}: threads={thr} syscalls in {SECS}s={tot} ({tot/SECS:.1f}/s) " + " ".join(f"{k}={v}" for k, v in sorted(calls.items(), key=lambda kv: -kv[1])[:8]), flush=True)
stop = True
for p, s, kind in jars: end(p, s)
