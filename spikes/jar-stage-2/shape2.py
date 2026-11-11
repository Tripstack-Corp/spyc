import os, re, signal, statistics, subprocess, time
from common import *
T = open(f"{D}/target_dir.txt").read().strip()
J = f"{T}/debug/spyc-jar"
SPYC = f"{T}/debug/spyc"
env = env_for(f"{D}/home9")
env.update(SPYC_JARS="1", SPYC_JAR_BIN=J)
if os.environ.get("SHAPE2_DEBUG"):
    env.update(SPYC_KEY_TRACE="1", SPYC_JAR_TRACE=f"{D}/jartrace.log")
S = os.path.join(os.environ.get("TMPDIR", "/tmp"), "spyc", "outer.sock")
if os.path.exists(S):
    os.remove(S)
outer = subprocess.Popen([J, "--socket", S, "--attach-signal", "--cwd", f"{D}/work", "--rows", "40", "--cols", "120", "--", SPYC],
                         env=env, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
time.sleep(4)  # spyc starts with nothing attached

def attach(rows, cols, kitty=True):
    t = Term([J, "attach", S], env, f"{D}/work", rows=rows, cols=cols, kitty=kitty)
    t.pump(4)
    return t

def last_number(t):
    n = [int(x) for x in re.findall(r"(?m)^\s*(17\d{8})\s*$", t.text())]
    return max(n) if n else None

a = attach(40, 120)
print("A attached:", "menu" in a.text(), "| modes set on A:", a.set_modes())
def dbg(t, what):
    if os.environ.get("SHAPE2_DEBUG"):
        print(f"--- {what}"); print("\n".join(l for l in t.text().splitlines() if l.strip()))
dbg(a, "A before ^a j")
a.send(b"\x01j"); a.pump(0.8)
dbg(a, "A after ^a j")
a.send(b"echo shape2-$((6*7))\r"); a.pump(2)
print("A: typed through two relays:", "shape2-42" in a.text())
dbg(a, "A after typing")
a.send(b"\x1c"); a.pump(1.5)
print("A detached; modes left on A's terminal:", a.set_modes(), "| client exited:", a.reap() is not None)

b = attach(30, 100, kitty=False)
print("B attached at 30x100:", "menu" in b.text(), "| modes set on B:", b.set_modes(), "| pane kept shape2-42:", "shape2-42" in b.text())
b.send(b"\x01j"); b.pump(0.5)
b.send(b"while true; do date +%s; sleep 0.2; done\r"); b.pump(2.5)
n1 = last_number(b)
kill_and_reap(b)
print("B's client killed -9 (an SSH drop) at", n1)
time.sleep(3)
c = attach(40, 120)
c.pump(1)
n2 = last_number(c)
print("C attached; the loop is now at", n2, "| kept running while nobody was attached:", bool(n1 and n2 and n2 >= n1 + 2))
c.send(b"\x03"); c.pump(1)

def echo_latency(t, n=30):
    out = []
    for i in range(n):
        ch = "abcdefghijklmnopqrstuvwxyz"[i % 26]
        before = t.text().count(ch)
        t0 = time.monotonic()
        t.send(ch.encode())
        while time.monotonic() - t0 < 2:
            t.pump(0.002)
            if t.text().count(ch) > before:
                out.append((time.monotonic() - t0) * 1000)
                break
    t.send(b"\x15"); t.pump(0.3)
    return statistics.median(out), max(out)

c.send(b"\x01j"); c.pump(0.5)
m2 = echo_latency(c)
print(f"echo through spyc in an outer jar: median {m2[0]:.1f} ms, max {m2[1]:.1f} ms")
c.send(b"\x1c"); c.pump(1)
outer.kill(); outer.wait()
direct = Term([SPYC], env, f"{D}/work", rows=40, cols=120)
direct.pump(6); direct.send(b"\x01j"); direct.pump(0.8)
m1 = echo_latency(direct)
print(f"echo through spyc run directly:    median {m1[0]:.1f} ms, max {m1[1]:.1f} ms")
kill_and_reap(direct)
