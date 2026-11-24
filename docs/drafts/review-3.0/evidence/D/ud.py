
import os, sys, pty, time, signal, subprocess, select, fcntl, termios, struct, json
BIN = sys.argv[1] if False else os.environ['UD_BIN']
D = os.environ['UD_DIR']
RT = '/tmp/udD'
ENV = {
  'HOME': D+'/home', 'XDG_STATE_HOME': D+'/state', 'XDG_CONFIG_HOME': D+'/config',
  'XDG_DATA_HOME': D+'/data', 'XDG_CACHE_HOME': D+'/cache', 'XDG_RUNTIME_DIR': RT,
  'TMPDIR': D+'/tmp', 'PATH': '/usr/bin:/bin:/usr/sbin:/sbin', 'SHELL': '/bin/sh',
  'TERM': 'xterm-256color', 'LANG': 'en_US.UTF-8',
}
SESS = RT+'/spyc/sessions'
TOK = D+'/state/spyc/jar-tokens'
PIDS = []   # every pid/pgid we started, for cleanup
def log(*a):
    print(time.strftime('%H:%M:%S'), *a, flush=True)
def prep():
    for d in [RT+'/spyc', SESS, RT+'/spyc/jars']:
        os.makedirs(d, exist_ok=True); os.chmod(d, 0o700)
    os.makedirs(TOK, exist_ok=True); os.chmod(TOK, 0o700)
def start_jar(socket, argv, token=b'unit-d-probe-token-0123456789ab', cwd=None, hold=b''):
    fields = [token, socket.encode(), (cwd or D+'/proj').encode(), b'24', b'80', hold, b'', str(len(argv)).encode()] + [a.encode() for a in argv]
    data = b''.join(f+b'\0' for f in fields)
    p = subprocess.Popen([BIN+'/spyc-jar','--wire','1'], stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=open(D+'/jar-stderr.log','ab'), env=ENV)
    p.stdin.write(data); p.stdin.close()
    PIDS.append(('jar', p.pid))
    for _ in range(200):
        if os.path.exists(socket) and describe(socket): break
        time.sleep(0.05)
    return p
def describe(socket):
    r = subprocess.run([BIN+'/spyc-jar','describe',socket], capture_output=True, env=ENV, timeout=20)
    out = {}
    for line in r.stdout.decode(errors='replace').splitlines():
        if ': ' in line:
            k, v = line.split(': ',1); out[k]=v
    return out
def keep_token(name, token=b'unit-d-probe-token-0123456789ab'):
    with open(TOK+'/'+name,'wb') as f: f.write(token)
    os.chmod(TOK+'/'+name, 0o600)
class Pty:
    def __init__(self, argv, env=None, rows=30, cols=100):
        pid, fd = pty.fork()
        if pid == 0:
            os.chdir(D+'/proj')
            os.execve(argv[0], argv, env or ENV)
        self.pid, self.fd = pid, fd
        fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack('HHHH', rows, cols, 0, 0))
        self.buf = b''
        self.status = None
        PIDS.append(('pty', pid))
    def pump(self, t=0.1):
        end = time.time()+t
        while True:
            left = end-time.time()
            if left <= 0: break
            r,_,_ = select.select([self.fd],[],[],left)
            if not r: break
            try:
                b = os.read(self.fd, 65536)
            except OSError:
                break
            if not b: break
            self.buf += b
            # answer DA1 / DSR / kitty query as a terminal would
            for q,a in [(b'\x1b[c', b'\x1b[?62;22c'), (b'\x1b[5n', b'\x1b[0n'), (b'\x1b[6n', b'\x1b[1;1R'), (b'\x1b[16t', b'\x1b[6;16;8t')]:
                n = b.count(q)
                for _ in range(n): os.write(self.fd, a)
    def wait_for(self, needle, t=30):
        end = time.time()+t
        while time.time() < end:
            if needle in self.buf: return True
            self.pump(0.1)
            if self.alive() is False: self.pump(0.2); return needle in self.buf
        return needle in self.buf
    def write(self, b):
        os.write(self.fd, b)
    def alive(self):
        if self.status is not None: return False
        try:
            p, st = os.waitpid(self.pid, os.WNOHANG)
        except ChildProcessError:
            return False
        if p == 0: return True
        self.status = st; return False
    def text(self):
        return self.buf.decode(errors='replace')
def cleanup(extra_sockets=()):
    for s in extra_sockets:
        try:
            d = describe(s)
            pp = int(d.get('program_pid','0') or 0)
            if pp > 0: os.kill(pp, signal.SIGKILL)
        except Exception as e: pass
    for kind, pid in PIDS:
        try:
            if kind == 'pty': os.killpg(pid, signal.SIGKILL)
            else: os.kill(pid, signal.SIGKILL)
        except Exception: pass
        try: os.waitpid(pid, os.WNOHANG)
        except Exception: pass
