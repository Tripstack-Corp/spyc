#!/usr/bin/env python3
"""Unit B review: a minimal spyc-jar driver (wire v1) for probes. Scratch only."""
import os, socket, struct, subprocess, sys, tempfile, time, signal

TOKEN = b"review-b-token-0123456789"
ROLE = {"owner": 1, "reader": 2, "lookup": 3, "program": 4}

def start_jar(binary, argv, extra=(), exit_linger_ms=None, hold_ms=None, cwd=None, env=None, preexec=None):
    d = tempfile.mkdtemp(prefix="revb", dir="/tmp")
    os.chmod(d, 0o700)
    sock = os.path.join(d, "j.sock")
    fields = [TOKEN, sock.encode(), (cwd or "").encode(), b"24", b"80",
              (str(hold_ms) if hold_ms is not None else "").encode(),
              (str(exit_linger_ms) if exit_linger_ms is not None else "").encode(),
              str(len(argv)).encode()] + [a.encode() for a in argv]
    blob = b"".join(f + b"\0" for f in fields)
    p = subprocess.Popen([binary, "--wire", "1", *extra], stdin=subprocess.PIPE,
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                         env=env, restore_signals=True, preexec_fn=preexec)
    p.stdin.write(blob); p.stdin.close()
    t = time.time()
    while not os.path.exists(sock):
        if time.time() - t > 10: raise SystemExit("no socket")
        time.sleep(0.01)
    return p, sock

def frame(kind, payload=b""):
    return struct.pack(">IBB", len(payload) + 2, kind, 0x80) + payload

class Conn:
    def __init__(self, sock, role="owner", token=TOKEN):
        self.s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.s.connect(sock)
        tok = b"" if role == "lookup" else token
        self.s.sendall(frame(1, struct.pack(">HBH", 1, ROLE[role], len(tok)) + tok))
        self.buf = b""
        self.welcome = self.recv(10)
    def send(self, kind, payload=b""):
        self.s.sendall(frame(kind, payload))
    def replay(self, frm=0): self.send(5, struct.pack(">Q", frm))
    def ack(self, off): self.send(4, struct.pack(">Q", off))
    def ping(self, n): self.send(9, struct.pack(">Q", n))
    def signal(self, s): self.send(6, struct.pack(">i", s))
    def recv(self, wait):
        self.s.settimeout(wait)
        try:
            while True:
                if len(self.buf) >= 4:
                    n = struct.unpack(">I", self.buf[:4])[0]
                    if len(self.buf) >= 4 + n:
                        f = self.buf[4:4 + n]; self.buf = self.buf[4 + n:]
                        return decode(f[0], f[2:])
                c = self.s.recv(65536)
                if not c: return ("eof",)
                self.buf += c
        except socket.timeout:
            return None
    def close(self): self.s.close()

def decode(kind, p):
    if kind == 64: return ("welcome",) + struct.unpack(">HIQQHH", p[:26])
    if kind == 65: return ("output", struct.unpack(">Q", p[:8])[0], p[8:])
    if kind == 70: return ("exited",) + struct.unpack(">iB", p[:5])
    if kind == 71: return ("refused", struct.unpack(">H", p[:2])[0], p[2:].decode(errors="replace"))
    if kind == 69: return ("pong", struct.unpack(">Q", p[:8])[0])
    if kind == 73: return ("foreground", struct.unpack(">I", p[:4])[0])
    if kind == 75:
        out, i = {}, 0
        while i < len(p):
            n = struct.unpack(">I", p[i:i+4])[0]; k = p[i+4:i+4+n].decode(); i += 4 + n
            n = struct.unpack(">I", p[i:i+4])[0]; v = p[i+4:i+4+n].decode(); i += 4 + n
            out[k] = v
        return ("description", out)
    return ("kind%d" % kind, p)

def describe(sock):
    c = Conn(sock, "lookup")
    c.send(13)
    m = c.recv(5)
    c.close()
    return m[1] if m and m[0] == "description" else {}

def end(p, sock):
    try:
        pid = int(describe(sock).get("program_pid", "0"))
        if pid: os.killpg(pid, signal.SIGKILL)
    except Exception: pass
    p.kill(); p.wait()
