#!/usr/bin/env python3
"""Start spyc in a pty in DIR with `-c quit`, answer its terminal probes, and
print the wall time from fork to exit. usage: drive.py ABS_BINARY DIR [HOME]"""
import fcntl, os, pty, select, struct, sys, tempfile, termios, time

binary, workdir = sys.argv[1], sys.argv[2]
home = sys.argv[3] if len(sys.argv) > 3 else tempfile.mkdtemp(prefix="spychome-")
env = {k: v for k, v in os.environ.items()
       if not k.startswith(("SPYC_", "CLAUDE", "XDG_", "CODEX", "TMUX"))}
env.update(HOME=home, TERM="xterm-256color", SHELL="/bin/sh")
t0 = time.perf_counter()
pid, fd = pty.fork()
if pid == 0:
    fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 160, 0, 0))
    os.chdir(workdir)
    os.execve(binary, [binary, "--no-lua", "-c", "quit", "-c", "quit"], env)
fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 160, 0, 0))
buf = b""
deadline = time.time() + 60
while True:
    if time.time() > deadline:
        os.kill(pid, 9)
        sys.stderr.write("timeout; last output: %r\n" % buf[-3000:])
        sys.exit(2)
    r, _, _ = select.select([fd], [], [], 0.5)
    if fd in r:
        try:
            chunk = os.read(fd, 65536)
        except OSError:
            break
        if not chunk:
            break
        buf += chunk
        tail = buf[-4096:]
        if b"\x1b[c" in chunk or b"\x1b[0c" in chunk:
            os.write(fd, b"\x1b[?62c")
        if b"\x1b[5n" in chunk:
            os.write(fd, b"\x1b[0n")
        if b"\x1b[6n" in chunk:
            os.write(fd, b"\x1b[1;1R")
    else:
        done, _ = os.waitpid(pid, os.WNOHANG)
        if done:
            break
_, status = os.waitpid(pid, 0)
dt = time.perf_counter() - t0
if os.environ.get("DRIVE_DEBUG"):
    sys.stderr.write("%r\n" % buf[-3000:])
if status != 0:
    sys.stderr.write("exit status %d; last output: %r\n" % (status, buf[-3000:]))
    sys.exit(3)
print(f"{dt:.6f}")
