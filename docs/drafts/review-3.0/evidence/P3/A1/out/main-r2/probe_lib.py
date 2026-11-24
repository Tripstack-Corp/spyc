
import os, sys, socket, struct, subprocess, threading, time, json, fcntl, tempfile, shutil, signal

S = os.environ['UA_S']
JAR = os.environ.get('UA_JAR', '~/.cache/spyc/target/3.0-9f8124c5/debug/spyc-jar')
DIFFER = os.environ.get('UA_DIFFER', '~/.cache/spyc/target/3.0-9f8124c5/differ/debug/spyc-jar')
TOKEN = b'unit-a-probe-token-0123456789'
LOG = []
def log(*a):
    s = ' '.join(str(x) for x in a); LOG.append(s); print(s, flush=True)

def frame(kind, payload, flags=0x80):
    return struct.pack('>IBB', len(payload)+2, kind, flags) + payload

def hello(role, token=TOKEN, version=1):
    return frame(1, struct.pack('>HBH', version, role, len(token)) + token)

def recv_exact(s, n):
    b = b''
    while len(b) < n:
        c = s.recv(n-len(b))
        if not c: raise EOFError('eof')
        b += c
    return b

def read_frame(s):
    n, = struct.unpack('>I', recv_exact(s, 4))
    body = recv_exact(s, n)
    return body[0], body[1], body[2:]

def describe(sock):
    s = socket.socket(socket.AF_UNIX); s.settimeout(10); s.connect(sock)
    s.sendall(hello(3, b'')); k,_,p = read_frame(s)
    if k != 64: return {'_refused': p}
    s.sendall(frame(13, b'')); k,_,p = read_frame(s); s.close()
    out = {}; i = 0; f = []
    while i < len(p):
        n, = struct.unpack('>I', p[i:i+4]); f.append(p[i+4:i+4+n].decode('utf8','replace')); i += 4+n
    return dict(zip(f[0::2], f[1::2]))

def lock_state(sock):
    try: fd = os.open(sock + '.lock', os.O_RDONLY)
    except OSError: return 'unknown'
    try:
        fcntl.lockf(fd, fcntl.LOCK_SH | fcntl.LOCK_NB); return 'FREE'
    except OSError: return 'held'
    finally: os.close(fd)

class Owner:
    def __init__(self, sock, role=1):
        self.s = socket.socket(socket.AF_UNIX); self.s.connect(sock)
        self.s.sendall(hello(role)); k,_,p = read_frame(self.s)
        assert k == 64, (k, p)
        self.welcome = struct.unpack('>HIQQHH', p[:26])
        self.text = b''; self.events = []; self.lock = threading.Lock(); self.alive = True
        self.s.sendall(frame(5, struct.pack('>Q', 0)))
        threading.Thread(target=self.rd, daemon=True).start()
        threading.Thread(target=self.pinger, daemon=True).start()
    def rd(self):
        try:
            while True:
                k,f,p = read_frame(self.s)
                with self.lock:
                    if k == 65:
                        off, = struct.unpack('>Q', p[:8]); self.text += p[8:]
                        self.s.sendall(frame(4, struct.pack('>Q', off+len(p)-8)))
                    elif k == 71: self.events.append(('refused', struct.unpack('>H', p[:2])[0], p[2:].decode('utf8','replace'), time.time()))
                    elif k == 74: self.events.append(('upgraded', p.decode('utf8','replace'), time.time()))
                    elif k == 69: pass
                    else: self.events.append(('kind', k, time.time()))
        except Exception as e:
            self.alive = False; self.events.append(('closed', repr(e), time.time()))
    def pinger(self):
        n = 0
        while self.alive:
            time.sleep(1); n += 1
            try: self.s.sendall(frame(9, struct.pack('>Q', n)))
            except Exception: return
    def echo(self, word, wait=20):
        self.s.sendall(frame(2, (word + '\n').encode()))
        t = time.time()
        while time.time() - t < wait:
            if ('got=' + word).encode() in self.text: return True
            time.sleep(0.05)
        return False
    def upgrade(self, binary, wait=70):
        with self.lock: start = len(self.events)
        t0 = time.time(); self.s.sendall(frame(12, binary.encode()))
        while time.time() - t0 < wait:
            with self.lock:
                for e in self.events[start:]:
                    if e[0] in ('refused','upgraded','closed'): return e, round(time.time()-t0, 2)
            time.sleep(0.05)
        return None, round(time.time()-t0, 2)

def start_jar(name, binary=JAR, program=None):
    d = tempfile.mkdtemp(prefix='uaA-'+name+'-', dir='/tmp'); os.chmod(d, 0o700)
    sock = os.path.join(d, 'j.sock')
    program = program or ['/bin/sh', '-c', 'while read l; do echo got=$l; done']
    fields = [TOKEN, sock.encode(), b'', b'24', b'80', b'', b'', str(len(program)).encode()] + [a.encode() for a in program]
    start = b''.join(f + b'\0' for f in fields)
    home = os.path.join(S, 'home'); os.makedirs(home, exist_ok=True)
    env = {'PATH': '/usr/bin:/bin:/usr/sbin:/sbin', 'HOME': home, 'TMPDIR': os.path.join(S,'tmp')+'/',
           'XDG_RUNTIME_DIR': os.path.join(S,'run'), 'XDG_STATE_HOME': os.path.join(S,'state'),
           'XDG_CONFIG_HOME': os.path.join(S,'config'), 'XDG_DATA_HOME': os.path.join(S,'data'), 'XDG_CACHE_HOME': os.path.join(S,'cache')}
    for k in ('tmp','run','state','config','data','cache'): os.makedirs(os.path.join(S,k), mode=0o700, exist_ok=True)
    p = subprocess.Popen([binary, '--wire', '1'], stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=open(os.path.join(S, name+'.jar.stderr'),'ab'), env=env)
    p.stdin.write(start); p.stdin.close()
    t = time.time()
    while not os.path.exists(sock):
        assert time.time()-t < 20, 'no socket'; time.sleep(0.05)
    log('jar', name, 'pid', p.pid, 'socket', sock)
    with open(os.path.join(S, 'pids.txt'), 'a') as f: f.write('%d %s\n' % (p.pid, sock))
    return p, d, sock

def wrapper(d, name, real, cases):
    path = os.path.join(d, name)
    with open(path, 'w') as f:
        f.write('#!/bin/sh\nREAL=%s\ncase "$1" in\n%s\nesac\nexec "$REAL" "$@"\n' % (real, cases))
    os.chmod(path, 0o755); return path

def sh(cmd):
    return subprocess.run(cmd, shell=True, capture_output=True, text=True).stdout

def proc_view(pid, tag):
    log('--- ps', tag); log(sh('ps -o pid,pgid,sess,stat,sigignore,sigmask,sigcatch,command -p %d' % pid))
    log('--- lsof', tag); log(sh('lsof -n -P -p %d 2>/dev/null' % pid))

def end(p, d, sock):
    try:
        info = describe(sock); pp = int(info.get('program_pid') or 0)
        if pp: os.killpg(pp, signal.SIGKILL)
    except Exception as e: log('describe at end failed', e)
    try: os.killpg(p.pid, signal.SIGKILL)
    except Exception: pass
    p.wait(); shutil.rmtree(d, ignore_errors=True)

def linux_view(pid, tag):
    out = ['--- /proc %d %s' % (pid, tag)]
    for l in open('/proc/%d/status' % pid):
        if l.startswith(('SigIgn','SigBlk','SigCgt')): out.append(l.strip() + '   SIGPIPE ignored=%d' % ((int(l.split()[1],16) >> 12) & 1) if l.startswith('SigIgn') else l.strip())
    for fd in sorted(os.listdir('/proc/%d/fd' % pid), key=int):
        try:
            tgt = os.readlink('/proc/%d/fd/%s' % (pid, fd))
            flags = [x for x in open('/proc/%d/fdinfo/%s' % (pid, fd)) if x.startswith('flags')][0].split()[1]
            out.append('fd %s -> %s flags=%s cloexec=%d' % (fd, tgt, flags, (int(flags, 8) >> 19) & 1))
        except Exception as e: out.append('fd %s: %s' % (fd, e))
    log('\n'.join(out))
