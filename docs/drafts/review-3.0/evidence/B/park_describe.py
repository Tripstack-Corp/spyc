import sys, time, os, threading, socket
sys.path.insert(0, os.path.dirname(__file__))
from jarprobe import *
B = sys.argv[1]
p, s = start_jar(B, ["/bin/sh", "-c", "while :; do echo tick; sleep 0.05; done"])
o = Conn(s, "owner"); o.replay(0)
d = os.path.dirname(s)
calls = os.path.join(d, "calls")
w = os.path.join(d, "jar-slow-parked")
open(w, "w").write(f"""#!/bin/sh
REAL={B}
case "$1" in
validate-handover) c=$(cat {calls} 2>/dev/null || echo 0); c=$((c+1)); echo $c > {calls}; [ $c -ne 2 ] || sleep 4;;
esac
exec "$REAL" "$@"
""")
os.chmod(w, 0o755)
stop = False
def acker():
    while not stop:
        m = o.recv(0.2)
        if m and m[0] == "output": o.ack(m[1] + len(m[2]))
        if m and m[0] in ("refused", "kind74"): print("owner heard:", m[0], m[1] if m[0]=="refused" else m[1][:40], f"at {time.time()-t0:.2f}s", flush=True)
t0 = time.time()
threading.Thread(target=acker, daemon=True).start()
o.send(12, w.encode())
# A lookup as client::describe makes it: connect, hello, describe, 2 s for the answer.
results = []
while time.time() - t0 < 12:
    t = time.time()
    try:
        c = socket.socket(socket.AF_UNIX); c.settimeout(2); c.connect(s)
        c.sendall(frame(1, struct.pack(">HBH", 1, 3, 0))); c.sendall(frame(13))
        got = c.recv(65536); ok = bool(got)
        c.close()
    except Exception as e:
        ok = False
    results.append((round(t - t0, 2), ok, round(time.time() - t, 2)))
    time.sleep(0.3)
stop = True
print("lookup at s, answered, took s:", results, flush=True)
end(p, s)
