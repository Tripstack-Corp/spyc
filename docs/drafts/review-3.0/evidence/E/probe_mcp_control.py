#!/usr/bin/env python3
# Unit E probe: drive a scratch spyc in a pty, open a jarred tab, then send a
# control-plane JSON-RPC method to its MCP socket as an unrelated process would.
# Everything lives under BASE (a short /tmp path: macOS caps socket paths at 104).
import os, pty, sys, time, select, json, socket, struct, fcntl, termios, signal, subprocess, glob, shutil

SPYC = '~/.cache/spyc/target/3.0-9f8124c5/debug/spyc'
BASE = sys.argv[1]
METHOD = sys.argv[2]            # e.g. spyc/shutdown
LOG = sys.argv[3]
out = open(LOG, 'w')
def log(*a):
    print(*a, file=out, flush=True)

if os.path.exists(BASE):
    shutil.rmtree(BASE)
dirs = {k: os.path.join(BASE, k) for k in ['home','state','config','data','cache','run','tmp','proj']}
for d in dirs.values():
    os.makedirs(d, mode=0o700)
    os.chmod(d, 0o700)
env = {
    'HOME': dirs['home'], 'XDG_STATE_HOME': dirs['state'], 'XDG_CONFIG_HOME': dirs['config'],
    'XDG_DATA_HOME': dirs['data'], 'XDG_CACHE_HOME': dirs['cache'], 'XDG_RUNTIME_DIR': dirs['run'],
    'TMPDIR': dirs['tmp'], 'TERM': 'xterm-256color', 'SHELL': '/bin/sh', 'LANG': 'en_CA.UTF-8',
    'PATH': '/usr/bin:/bin:/usr/sbin:/sbin',
}
log('env', json.dumps(env))
pid, fd = pty.fork()
if pid == 0:
    os.chdir(dirs['proj'])
    os.execve(SPYC, [SPYC], env)
fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack('HHHH', 40, 120, 0, 0))
log('spyc pid', pid)
screen = bytearray()
def pump(secs):
    end = time.time() + secs
    while time.time() < end:
        r, _, _ = select.select([fd], [], [], 0.05)
        if fd in r:
            try:
                b = os.read(fd, 65536)
            except OSError:
                return False
            if not b:
                return False
            screen.extend(b)
            # answer terminal queries so spyc never waits on one
            if b'\x1b[c' in b or b'\x1b[0c' in b: os.write(fd, b'\x1b[?62;22c')
            if b'\x1b[6n' in b: os.write(fd, b'\x1b[1;1R')
            if b'\x1b[?u' in b: os.write(fd, b'\x1b[?0u')
            if b'\x1b]11;?' in b: os.write(fd, b'\x1b]11;rgb:0000/0000/0000\x1b\\')
            if b'\x1b]10;?' in b: os.write(fd, b'\x1b]10;rgb:ffff/ffff/ffff\x1b\\')
            if b'\x1b[14t' in b: os.write(fd, b'\x1b[4;800;1200t')
            if b'\x1b[16t' in b: os.write(fd, b'\x1b[6;20;10t')
    return True

def alive(p):
    try:
        os.kill(p, 0); return True
    except ProcessLookupError:
        return False
    except PermissionError:
        return True

def framed(obj):
    body = json.dumps(obj).encode()
    return b'Content-Length: %d\r\n\r\n' % len(body) + body

def read_framed(s, timeout=10):
    s.settimeout(timeout)
    buf = b''
    while b'\r\n\r\n' not in buf:
        c = s.recv(4096)
        if not c: return None
        buf += c
    head, rest = buf.split(b'\r\n\r\n', 1)
    n = int([l for l in head.split(b'\r\n') if l.lower().startswith(b'content-length')][0].split(b':')[1])
    while len(rest) < n:
        c = s.recv(4096)
        if not c: break
        rest += c
    return rest[:n].decode()

def describe(sock):
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.settimeout(3)
    s.connect(sock)
    s.sendall(bytes.fromhex('0000000701800001030000') + bytes.fromhex('000000020d80'))
    buf = b''
    fields = {}
    try:
        while True:
            c = s.recv(65536)
            if not c: break
            buf += c
    except Exception:
        pass
    i = 0
    while i + 6 <= len(buf):
        n = struct.unpack('>I', buf[i:i+4])[0]
        kind = buf[i+4]
        payload = buf[i+6:i+4+n]
        if kind == 0x4b:
            j = 0; items = []
            while j + 4 <= len(payload):
                m = struct.unpack('>I', payload[j:j+4])[0]
                items.append(payload[j+4:j+4+m].decode('utf-8', 'replace')); j += 4 + m
            fields = dict(zip(items[0::2], items[1::2]))
        i += 4 + n
    return fields

def jar_fields(sock):
    r = subprocess.run([SPYC, '--jars'], env=env, capture_output=True, text=True, timeout=30)
    return r.stdout + r.stderr

jar_pids = []
try:
    pump(4)
    mcp = os.path.join(dirs['state'], 'spyc', 'mcp-%d.sock' % pid)
    for _ in range(100):
        if os.path.exists(mcp): break
        pump(0.1)
    log('mcp socket exists', os.path.exists(mcp), mcp)
    # ^a c, then the tab's command
    os.write(fd, b'\x01'); pump(0.3); os.write(fd, b'c'); pump(1.0)
    os.write(fd, b'\x15'); pump(0.3); os.write(fd, b'sleep 9999\r'); pump(8)
    socks = glob.glob(os.path.join(dirs['run'], 'spyc', 'jars', '*.sock'))
    log('jar sockets', socks)
    listing = jar_fields(None)
    log('spyc --jars:\n' + listing)
    prog = None
    for line in listing.splitlines():
        if ' pid ' in line:
            try:
                prog = int(line.split(' pid ')[1].split()[0])
            except Exception:
                pass
    # the jar process pids, for cleanup (jars leave our session by setsid)
    ps = subprocess.run(['ps', '-axo', 'pid=,command='], capture_output=True, text=True).stdout
    for l in ps.splitlines():
        if BASE in l or ('spyc-jar' in l and 'sleep' in l):
            pass
    for sk in socks:
        log('describe', sk, json.dumps(describe(sk)))
    log('program pid (sleep)', prog, 'alive', prog and alive(prog))
    # An unrelated process of the same user: connect straight to the socket.
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.connect(mcp)
    req = {'jsonrpc': '2.0', 'id': 7, 'method': METHOD}
    if METHOD == 'spyc/statusline':
        req['params'] = {'pane_id': 'not-the-callers-pane', 'statusline': {'five_hour': {'used_percent': 99}}}
    s.sendall(framed(req))
    reply = read_framed(s)
    log('reply to', METHOD, ':', reply)
    t0 = time.time()
    exited = None
    while time.time() - t0 < 15:
        if not pump(0.2):
            pass
        wp, st = os.waitpid(pid, os.WNOHANG)
        if wp == pid:
            exited = st; break
    log('spyc exited after %.1fs' % (time.time() - t0) if exited is not None else 'spyc still running after 15s', exited)
    time.sleep(3)
    log('program (sleep) alive after:', prog and alive(prog))
    log('jar sockets after:', glob.glob(os.path.join(dirs['run'], 'spyc', 'jars', '*.sock')))
    log('spyc --jars after:\n' + jar_fields(None))
    tail = screen[-3000:].decode('utf-8', 'replace')
    log('last screen bytes (escaped):', repr(tail[-1500:]))
finally:
    # end what this probe started: spyc's session, and every jar in BASE
    try: os.kill(pid, signal.SIGKILL)
    except Exception: pass
    try: os.waitpid(pid, 0)
    except Exception: pass
    ps = subprocess.run(['ps', '-axo', 'pid=,pgid=,command='], capture_output=True, text=True).stdout
    for sock in glob.glob(os.path.join(BASE, '**', '*.sock'), recursive=True):
        pass
    # describe each jar we made for its pids
    for sock in glob.glob(os.path.join(dirs['run'], 'spyc', '**', '*.sock'), recursive=True):
        try:
            f = describe(sock)
        except Exception as e:
            log('cleanup: no answer at', sock, e); continue
        log('cleanup socket', sock, 'pid', f.get('pid'), 'program_pid', f.get('program_pid'))
        for key, group in (('program_pid', True), ('pid', False)):
            p = int(f.get(key) or 0)
            if p > 1:
                try: os.kill(p, signal.SIGCONT)
                except Exception: pass
                try:
                    (os.killpg if group else os.kill)(p, signal.SIGKILL)
                except Exception: pass
    log('remaining processes mentioning BASE or our jars:')
    ps = subprocess.run(['ps', '-axo', 'pid=,ppid=,pgid=,command='], capture_output=True, text=True).stdout
    for l in ps.splitlines():
        if 'sleep 9999' in l or BASE in l:
            log('  ', l)
    out.flush()
