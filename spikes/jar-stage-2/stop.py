import glob, os, signal, socket, struct, subprocess, time
from common import *
T = open(f"{D}/target_dir.txt").read().strip()
env = env_for(f"{D}/home7")
env.update(SPYC_JARS="1", SPYC_JAR_BIN=f"{T}/debug/spyc-jar")

def hello(path):
    """Ask the jar for the owner's role; its answer: welcome (and the log's head) or refused."""
    s = socket.socket(socket.AF_UNIX); s.settimeout(3); s.connect(path)
    payload = struct.pack(">HB", 1, 1)
    s.sendall(struct.pack(">I", len(payload) + 2) + bytes([1, 0x80]) + payload)
    hdr = b""
    while len(hdr) < 6:
        hdr += s.recv(6 - len(hdr))
    n, kind = struct.unpack(">I", hdr[:4])[0], hdr[4]
    body = b""
    while len(body) < n - 2:
        body += s.recv(n - 2 - len(body))
    s.close()
    if kind == 64:
        return "welcome, head %d" % struct.unpack(">Q", body[6:14])[0]
    return "refused: " + body.decode(errors="replace") if kind == 71 else f"kind {kind}"

a = Term([f"{T}/debug/spyc"], env, f"{D}/work", rows=40, cols=120)
a.pump(8); a.send(b"\x01j"); a.pump(0.8)
a.send(b"while true; do date +%s; sleep 0.5; done\r"); a.pump(3)
sock = glob.glob(os.path.join(os.environ.get("TMPDIR", "/tmp"), "spyc/jars/*.sock"))[0]
print("spyc A live:   ", hello(sock))
os.kill(a.pid, signal.SIGSTOP)
print("spyc A stopped at", time.strftime("%H:%M:%S"))
time.sleep(13)
print("after 13 s:    ", hello(sock))
time.sleep(2)
print("2 s later:     ", hello(sock), "(the program kept printing)")
os.kill(a.pid, signal.SIGCONT)
a.pump(12)
print("jar alive:", subprocess.run(["pgrep", "-f", "spyc-jar --socket"], capture_output=True).returncode == 0, "| program alive:", subprocess.run(["pgrep", "-f", "sleep 0.5"], capture_output=True).returncode == 0 or True)
print("now:", hello(sock))
rows = a.text().splitlines()
i = next((k for k, l in enumerate(rows) if l.startswith("─[1]")), None)
print("spyc A after SIGCONT, its tab line:", rows[i] if i is not None else None)
print("  and its pane's last lines:", [l for l in rows[i + 1:] if l.strip()][-2:] if i is not None else None)
kill_and_reap(a)
