#!/usr/bin/env python3
# P3-9: run as the OTHER user (uid 1001). Binds stand-in session sockets in
# /tmp/spyc-1000/sessions, which that user made first (mode 0777), each with a
# held lock file so spyc reads it as Live. Adapted from Unit E's
# probe_attach_trust.py: answers a lookup's describe with owner=connected and
# owner_pid=DECOY (a process of uid 1000), takes an owner hello, logs its
# token, paints a fake prompt and logs every input frame.
import os, sys, socket, struct, fcntl, threading, time
SESS, DECOY, LOG = sys.argv[1], int(sys.argv[2]), sys.argv[3]
NAMES = sys.argv[4:]
out = open(LOG, 'a', buffering=1)
def log(*a): print('%.3f' % time.time(), *a, file=out, flush=True)
def frame(kind, payload): return struct.pack('>IBB', len(payload) + 2, kind, 0x80) + payload
def s16(s): b = s.encode(); return struct.pack('>I', len(b)) + b
def alive(pid):
    # Another user's process: /proc/<pid>/stat is readable, a signal isn't.
    try: return open('/proc/%d/stat' % pid).read().rsplit(')', 1)[1].split()[0] not in ('Z', 'X')
    except OSError: return False
def welcome(): return frame(0x40, struct.pack('>HIQQHH', 1, 4242, 0, 0, 24, 80))
def description(name):
    if alive(DECOY): pairs = [('id', name), ('owner', 'connected'), ('owner_pid', str(DECOY)), ('pid', '4241')]
    else: pairs = [('id', name), ('owner', 'none'), ('pid', '4241')]
    return frame(0x4b, b''.join(s16(k) + s16(v) for k, v in pairs))
def read_frame(c):
    h = b''
    while len(h) < 4:
        x = c.recv(4 - len(h))
        if not x: return None
        h += x
    n = struct.unpack('>I', h)[0]; b = b''
    while len(b) < n:
        x = c.recv(n - len(b))
        if not x: return None
        b += x
    return b[0], b[2:]
def serve(c, name):
    try:
        f = read_frame(c)
        if not f: return
        kind, p = f
        version, role, tlen = struct.unpack('>HBH', p[:5]); token = p[5:5 + tlen]
        cred = c.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, struct.calcsize('3i'))
        pid, uid, gid = struct.unpack('3i', cred)
        if role != 3: log(name, 'hello: role', role, 'token', token.decode('utf-8', 'replace'), 'from peer uid', uid, 'pid', pid)
        c.sendall(welcome())
        if role == 3:
            f = read_frame(c)
            if f and f[0] == 0x0d: c.sendall(description(name))
            c.close(); return
        c.sendall(frame(0x41, struct.pack('>Q', 0) + b'\r\n[a stand-in jar of uid 1001 paints this terminal] password: '))
        while True:
            f = read_frame(c)
            if not f: break
            if f[0] == 2: log(name, 'input frame:', repr(bytes(f[1])))
            else: log(name, 'frame kind', f[0], repr(bytes(f[1][:60])))
    except Exception as e:
        log(name, 'serve error', repr(e))
def listen(name):
    path = os.path.join(SESS, name + '.sock')
    lk = open(path + '.lock', 'w'); os.chmod(path + '.lock', 0o644)
    fcntl.lockf(lk, fcntl.LOCK_EX | fcntl.LOCK_NB)
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM); s.bind(path); os.chmod(path, 0o777); s.listen(16)
    log('planted', path, 'by uid', os.getuid())
    def loop():
        while True:
            c, _ = s.accept()
            threading.Thread(target=serve, args=(c, name), daemon=True).start()
    threading.Thread(target=loop, daemon=True).start()
    return lk, s
keep = [listen(n) for n in NAMES]
open(os.path.join(os.path.dirname(LOG), 'stand_in.ready'), 'w').write('')
while True: time.sleep(3600)
