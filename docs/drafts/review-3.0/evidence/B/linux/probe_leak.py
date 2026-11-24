import sys, time, os
sys.path.insert(0, "/scripts-b")
from jarprobe import *
B = sys.argv[1]
p, s = start_jar(B, ["/bin/sh", "-c", "while :; do echo tick; sleep 0.2; done"])
def stat():
    fds = len(os.listdir(f"/proc/{p.pid}/fd")); thr = len(os.listdir(f"/proc/{p.pid}/task"))
    rss = [l for l in open(f"/proc/{p.pid}/status") if l.startswith("VmRSS")][0].split()[1]
    return f"fds={fds} threads={thr} rss={rss}kB"
o = Conn(s, "owner"); o.replay(0); time.sleep(0.5)
print("start:", stat(), flush=True)
for round in range(3):
    for i in range(300):
        describe(s)
    for i in range(100):
        r = Conn(s, "reader"); r.replay(0); r.recv(0.05); r.close()
    o.close()
    o = Conn(s, "owner")  # may need the old one's release first
    for k in range(50):
        if o.welcome and o.welcome[0] == "welcome": break
        time.sleep(0.05); o = Conn(s, "owner")
    o.replay(0)
    for i in range(50):
        c = Conn(s, "lookup"); c.close()  # a lookup that asks nothing
    time.sleep(6)
    print(f"after round {round+1} (300 describes, 100 readers, an owner replaced, 50 silent lookups): {stat()}", flush=True)
end(p, s)
