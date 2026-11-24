import sys, time, os, subprocess
sys.path.insert(0, os.path.dirname(__file__))
from jarprobe import *
B = sys.argv[1]; ring = int(sys.argv[2])
def rss(pid): return int(subprocess.run(["ps","-o","rss=","-p",str(pid)],capture_output=True,text=True).stdout.strip() or 0)
for name, prog in [("8 KiB writes", "yes | head -c %d; exec sleep 1000" % (ring*2)),
                   ("1-byte writes", "perl -e '$|=1; for(1..%d){print q(x); select(undef,undef,undef,0.00002)} sleep 1000'" % (ring*2))]:
    p, s = start_jar(B, ["/bin/sh", "-c", prog], extra=("--ring", str(ring), "--window", str(ring//4)))
    base = rss(p.pid); t0 = time.time()
    while time.time() - t0 < 300:
        d = describe(s); log = d.get("log", "")
        printed = int(log.split(", ")[1].split()[0]) if ", " in log else 0
        if printed >= ring*2 - 10: break
        time.sleep(1)
    print(f"{name}: ring {ring} B, {d.get('log')}, after {time.time()-t0:.0f}s; jar RSS {base} KiB at start, {rss(p.pid)} KiB now", flush=True)
    end(p, s)
