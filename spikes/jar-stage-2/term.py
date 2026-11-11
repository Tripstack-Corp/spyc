"""A scripted terminal for the dtach experiments: runs a command on a pty,
answers the common terminal queries (or not), tracks the modes the program
sets, and renders the screen with pyte."""
import fcntl, os, pty, re, select, signal, struct, termios, time

import pyte

QUERIES = [
    ("DA1", re.compile(rb"\x1b\[0?c")),
    ("DA2", re.compile(rb"\x1b\[>0?c")),
    ("DSR", re.compile(rb"\x1b\[5n")),
    ("CPR", re.compile(rb"\x1b\[6n")),
    ("kitty?u", re.compile(rb"\x1b\[\?u")),
    ("OSC10?", re.compile(rb"\x1b\]10;\?(\x07|\x1b\\)")),
    ("OSC11?", re.compile(rb"\x1b\]11;\?(\x07|\x1b\\)")),
    ("XTVERSION", re.compile(rb"\x1b\[>0?q")),
    ("DECRQM", re.compile(rb"\x1b\[\?(\d+)\$p")),
    ("XTWINOPS", re.compile(rb"\x1b\[(14|16|18)t")),
]
PRIV = re.compile(rb"\x1b\[\?([\d;]+)([hl])")
KPUSH = re.compile(rb"\x1b\[>(\d*)u")
KPOP = re.compile(rb"\x1b\[<(\d*)u")


class Term:
    def __init__(self, argv, env, cwd, rows=40, cols=120, answer=True, kitty=True):
        self.rows, self.cols, self.answer, self.kitty = rows, cols, answer, kitty
        self.screen = pyte.Screen(cols, rows)
        self.stream = pyte.ByteStream(self.screen)
        self.raw = bytearray()
        self.modes = {}
        self.kitty_depth = 0
        self.queries = {}
        self.t0 = time.monotonic()
        pid, fd = pty.fork()
        if pid == 0:
            os.chdir(cwd)
            os.execve(argv[0], argv, env)
        self.pid, self.fd = pid, fd
        fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
        self.exit = None

    def _answer(self, chunk):
        for name, rx in QUERIES:
            for m in rx.finditer(chunk):
                self.queries[name] = self.queries.get(name, 0) + 1
                if not self.answer:
                    continue
                y, x = self.screen.cursor.y, self.screen.cursor.x
                reply = {
                    "DA1": b"\x1b[?62;22c",
                    "DA2": b"\x1b[>0;10;1c",
                    "DSR": b"\x1b[0n",
                    "CPR": f"\x1b[{y + 1};{x + 1}R".encode(),
                    "kitty?u": b"\x1b[?0u" if self.kitty else b"",
                    "OSC10?": b"\x1b]10;rgb:dddd/dddd/dddd\x1b\\",
                    "OSC11?": b"\x1b]11;rgb:1111/1111/1111\x1b\\",
                    "XTVERSION": b"\x1bP>|harness(1.0)\x1b\\",
                    "DECRQM": b"\x1b[?" + (m.group(1) if m.groups() else b"0") + b";2$y",
                    "XTWINOPS": b"\x1b[4;800;1200t",
                }[name]
                if reply:
                    os.write(self.fd, reply)

    def _track(self, chunk):
        for m in PRIV.finditer(chunk):
            for n in m.group(1).split(b";"):
                if n:
                    self.modes[int(n)] = m.group(2) == b"h"
        self.kitty_depth += len(KPUSH.findall(chunk))
        self.kitty_depth -= len(KPOP.findall(chunk))

    def pump(self, secs):
        end = time.monotonic() + secs
        while time.monotonic() < end:
            r, _, _ = select.select([self.fd], [], [], max(0, end - time.monotonic()))
            if not r:
                continue
            try:
                chunk = os.read(self.fd, 65536)
            except OSError:
                self.reap()
                return
            if not chunk:
                self.reap()
                return
            self.raw += chunk
            self._track(chunk)
            self._answer(chunk)
            self.stream.feed(chunk)

    def send(self, data):
        os.write(self.fd, data)

    def reap(self):
        if self.exit is None:
            try:
                p, st = os.waitpid(self.pid, os.WNOHANG)
                if p:
                    self.exit = st
            except ChildProcessError:
                self.exit = -1
        return self.exit

    def text(self):
        return "\n".join(line.rstrip() for line in self.screen.display)

    def nonblank(self):
        return sum(1 for line in self.screen.display if line.strip())

    def set_modes(self):
        return sorted(k for k, v in self.modes.items() if v)

    def close(self):
        try:
            os.kill(self.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        self.reap()
