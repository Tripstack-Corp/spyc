
import os, pty, sys, time, threading, re, random
mode = sys.argv[1]          # "echo" or "noecho"
secs = float(sys.argv[2])
prog = "(i=0; while :; do i=$((i+1)); echo n=$i; sleep 0.005; done) & while read l; do echo got=$l; done"
if mode == "noecho":
    prog = "stty -echo; " + prog
pid, fd = pty.fork()
if pid == 0:
    os.execv("/bin/sh", ["/bin/sh", "-c", prog])
buf = bytearray()
stop = False
def rd():
    while not stop:
        try:
            b = os.read(fd, 65536)
        except OSError:
            break
        if not b: break
        buf.extend(b)
t = threading.Thread(target=rd, daemon=True); t.start()
time.sleep(0.3)
end = time.time() + secs
typed = 0
while time.time() < end:
    os.write(fd, b"after\n"); typed += 1
    time.sleep(random.uniform(0.001, 0.01))
time.sleep(0.5)
stop = True
os.kill(pid, 9)
text = buf.decode("utf-8", "replace")
counts = []
glued = 0
for line in text.splitlines():
    s = line.strip()
    m = re.match(r"n=(\d+)$", s)
    if m: counts.append(int(m.group(1)))
    elif re.match(r"n=\d+\D", s): glued += 1
breaks = sum(1 for a, b in zip(counts, counts[1:]) if b != a + 1)
ex = [l for l in text.splitlines() if re.match(r"n=\d+\D", l.strip())][:3]
print(f"mode={mode} typed={typed} counts={len(counts)} breaks={breaks} glued_lines={glued} examples={ex!r}")
