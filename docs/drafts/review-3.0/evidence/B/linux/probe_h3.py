import sys, time, os, signal, subprocess
sys.path.insert(0, "/scripts-b")
from jarprobe import *
B = sys.argv[1]
SCRIPT = "grep -E 'Sig(Ign|Blk|Cgt)|Umask' /proc/self/status; echo fds: $(ls /proc/self/fd | tr '\\n' ' '); for f in /proc/self/fd/*; do echo \"  $f -> $(readlink $f)\"; done; echo cwd: $(pwd); echo sid: $(cut -d' ' -f6 /proc/self/stat) pgrp: $(cut -d' ' -f5 /proc/self/stat) tpgid: $(cut -d' ' -f8 /proc/self/stat) pid: $$; echo env-spyc: $(env | grep SPYC | tr '\\n' ' '); echo END"
def like_spyc():
    signal.signal(signal.SIGTTOU, signal.SIG_IGN)
    os.umask(0o022)
print("== plain child of a clean parent (pipe, not a terminal) ==", flush=True)
print(subprocess.run(["/bin/sh", "-c", SCRIPT], capture_output=True, text=True, cwd="/tmp").stdout, flush=True)
print("== program in a jar started by a parent that ignores SIGTTOU, as spyc does ==", flush=True)
os.chdir("/tmp")
p, s = start_jar(B, ["/bin/sh", "-c", SCRIPT + "; exec sleep 100"], preexec=like_spyc, cwd="/home")
o = Conn(s, "owner"); o.replay(0); out = b""
t = time.time()
while b"END" not in out and time.time() - t < 5:
    m = o.recv(0.2)
    if m and m[0] == "output": out += m[2]; o.ack(m[1] + len(m[2]))
print(out.decode(errors="replace").replace("\r", ""), flush=True)
jp = p.pid
print("jar itself:", open(f"/proc/{jp}/status").read().split("SigQ")[1].split("Cpus")[0].strip().replace("\n", " | "), flush=True)
print("jar fds:", " ".join(f"{f}->{os.readlink(f'/proc/{jp}/fd/{f}')}" for f in os.listdir(f"/proc/{jp}/fd")), flush=True)
print("jar cwd:", os.readlink(f"/proc/{jp}/cwd"), flush=True)
end(p, s)
