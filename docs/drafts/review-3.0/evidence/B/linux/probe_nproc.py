import sys, time, os, resource, subprocess, signal
sys.path.insert(0, "/scripts-b")
from jarprobe import *
B = sys.argv[1]
INF = resource.getrlimit(resource.RLIMIT_NPROC)[1]
print("hard NPROC", INF, flush=True)
def threads_of_user():
    n = 0
    for pid in os.listdir("/proc"):
        if pid.isdigit():
            try:
                if os.stat(f"/proc/{pid}").st_uid == os.getuid(): n += len(os.listdir(f"/proc/{pid}/task"))
            except Exception: pass
    return n
def alive(pid):
    try: os.kill(pid, 0); return True
    except Exception: return False
# (a) the acceptor can't start a connection's thread
p, s = start_jar(B, ["/bin/sleep", "100000"])
o = Conn(s, "owner"); o.replay(0)
time.sleep(0.3)
prog = int(describe(s)["program_pid"])
time.sleep(0.3)
n = threads_of_user()
resource.prlimit(p.pid, resource.RLIMIT_NPROC, (n - 1, INF))
print(f"V9a: user tasks {n}; jar {p.pid} soft RLIMIT_NPROC {n-1}; jar threads {len(os.listdir(f'/proc/{p.pid}/task'))}", flush=True)
try:
    c = Conn(s, "lookup"); print("   lookup welcome:", c.welcome, flush=True); c.send(13); print("   lookup answer:", (c.recv(3) or ("none",))[0], flush=True)
except Exception as e:
    print("   lookup:", repr(e), flush=True)
time.sleep(1)
print(f"   after one lookup: jar alive={p.poll() is None} exit status={p.returncode} program state={open(f"/proc/{prog}/stat").read().split()[2] if alive(prog) else "gone"}", flush=True)
print(f"   owner's connection: {o.recv(1)}", flush=True)
if p.poll() is None: resource.prlimit(p.pid, resource.RLIMIT_NPROC, (INF, INF))
end(p, s)
try: os.kill(prog, signal.SIGKILL)
except Exception: pass
# (b) the owner's reader starts but its writer can't
p, s = start_jar(B, ["/bin/sleep", "100000"])
time.sleep(0.5)
n = threads_of_user()
K = int(sys.argv[2]) if len(sys.argv) > 2 else 1
resource.prlimit(p.pid, resource.RLIMIT_NPROC, (n + K, INF))
print(f"V9b: user tasks {n}; jar {p.pid} soft RLIMIT_NPROC {n+K}; jar threads {len(os.listdir(f'/proc/{p.pid}/task'))}", flush=True)
o = None
try:
    o = Conn(s, "owner"); print("   owner welcome:", o.welcome, flush=True)
except Exception as e:
    print("   owner connect:", repr(e), flush=True)
time.sleep(1)
resource.prlimit(p.pid, resource.RLIMIT_NPROC, (INF, INF))
if p.poll() is not None:
    print("   jar died:", p.returncode); sys.exit(0)
print("   limit lifted; jar alive:", p.poll() is None, "; jar threads", len(os.listdir(f'/proc/{p.pid}/task')), "; describe owner:", describe(s).get("owner"), flush=True)
if o: o.close()
time.sleep(13)
d = describe(s)
print("   13 s after that owner's socket closed (deadline 10 s): owner =", d.get("owner"), flush=True)
try:
    o2 = Conn(s, "owner"); print("   a new owner gets:", o2.welcome, flush=True)
except Exception as e:
    print("   new owner:", repr(e), flush=True)
end(p, s)
