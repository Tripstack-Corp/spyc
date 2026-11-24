#!/usr/bin/env python3
# Unit C probe driver: run a scratch spyc in a pty, open a jarred tab, SIGSTOP its jar, time what the tab shows.
import os, sys, pty, select, time, signal, fcntl, termios, struct, re, json, subprocess, tempfile, shutil

BIN = sys.argv[1]           # absolute path to spyc
BASE = sys.argv[2]          # scratch base dir (inside the unit's scratchpad)
MODE = sys.argv[3]          # 'typing' or 'idle' or 'paste'
LIMIT = float(sys.argv[4])  # seconds to watch after SIGSTOP
LOG = open(os.path.join(BASE, 'events.log'), 'w', buffering=1)
RAW = open(os.path.join(BASE, 'raw.out'), 'wb', buffering=0)
T0 = time.monotonic()
def ev(*a):
    LOG.write('%8.3f %s\n' % (time.monotonic() - T0, ' '.join(str(x) for x in a)))

home = os.path.join(BASE, 'home'); os.makedirs(home, exist_ok=True)
dirs = {k: os.path.join(BASE, k.lower()) for k in ['XDG_STATE_HOME','XDG_CONFIG_HOME','XDG_DATA_HOME','XDG_CACHE_HOME']}
for d in dirs.values(): os.makedirs(d, exist_ok=True)
rt = tempfile.mkdtemp(prefix='sc7', dir='/tmp')   # short: socket paths must fit 104 bytes
os.chmod(rt, 0o700)
open(os.path.join(BASE, 'runtime_dir'), 'w').write(rt)
prog = "sh -c 'echo READY_$$; while IFS= read -r l; do echo got_$l; done'"
env = {
  'PATH': '/usr/bin:/bin:/usr/sbin:/sbin', 'TERM': 'xterm-256color', 'LANG': 'en_CA.UTF-8',
  'HOME': home, 'XDG_RUNTIME_DIR': rt, 'TMPDIR': rt + '/', 'SPYC_PANE_CMD': prog, 'SHELL': '/bin/sh',
}
env.update(dirs)
ev('env', json.dumps(env))
pid, fd = pty.fork()
if pid == 0:
    os.chdir(home)
    os.execve(BIN, [BIN], env)
open(os.path.join(BASE, 'spyc_pid'), 'w').write(str(pid))
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
            except OSError: ev('pty closed'); return
            RAW.write(d)
            buf += d
            for q, a in [(b'\x1b[c', b'\x1b[?62c'), (b'\x1b[5n', b'\x1b[0n'), (b'\x1b[6n', b'\x1b[1;1R'), (b'\x1b[>q', b''), (b'\x1b[?u', b'\x1b[?0u')]:
                n = d.count(q)
                for _ in range(n):
                    if a: os.write(fd, a)
def seen(pat, since=0):
    return re.search(pat, buf[since:]) is not None
def wait_for(pat, t, since=0):
    end = time.monotonic() + t
    while time.monotonic() < end:
        pump(0.1)
        if seen(pat, since): return True
    return False

jar_pid = None; prog_pid = None
def cleanup():
    try: os.killpg(pid, signal.SIGKILL)
    except Exception as e: ev('killpg', e)
    for p in [prog_pid, jar_pid]:
        if p:
            try:
                os.kill(p, signal.SIGCONT)
                if p == prog_pid: os.killpg(p, signal.SIGKILL)
                os.kill(p, signal.SIGKILL)
            except Exception as e: ev('kill', p, e)
    # any jar my spyc started that outlived it: its ppid is 1 now, find by my runtime dir's sockets via lsof
    out = subprocess.run(['lsof', '-t', '+D', rt], capture_output=True, text=True).stdout.split()
    for p in out:
        if int(p) != os.getpid():
            ev('ending leftover in my runtime dir', p, subprocess.run(['ps','-o','command=','-p',p],capture_output=True,text=True).stdout.strip())
            try: os.kill(int(p), signal.SIGCONT); os.kill(int(p), signal.SIGKILL)
            except Exception as e: ev('kill', p, e)
import atexit; atexit.register(cleanup)
pump(4)
ev('started; screen bytes', len(buf))
os.write(fd, b'\x01'); pump(0.3); os.write(fd, b'c'); pump(1.0); os.write(fd, b'\r')
ok = wait_for(rb'READY_\d+', 40)
ev('program ready', ok)
if not ok:
    ev('tail', buf[-3000:]); sys.exit(2)
m = re.search(rb'READY_(\d+)', buf); prog_pid = int(m.group(1)); ev('program pid', prog_pid)
mark = len(buf)
os.write(fd, b'hello\r'); ok = wait_for(rb'got_hello', 10, mark); ev('typing works', ok)
# the jar: spyc's child whose command is spyc-jar
out = subprocess.run(['ps', '-o', 'pid=,ppid=,command=', '-A'], capture_output=True, text=True).stdout
jars = [l.split(None, 2) for l in out.splitlines()]
jars = [j for j in jars if len(j) == 3 and j[1] == str(pid) and j[2].startswith('spyc-jar')]
ev('jars of my spyc', jars)
if len(jars) != 1: sys.exit(3)
jar_pid = int(jars[0][0])
open(os.path.join(BASE, 'jar_pid'), 'w').write(str(jar_pid))
open(os.path.join(BASE, 'prog_pid'), 'w').write(str(prog_pid))
pump(2)
mark = len(buf)
os.kill(jar_pid, signal.SIGSTOP); ts = time.monotonic(); ev('SIGSTOP jar', jar_pid)
firsts = {}
pats = {'detached': rb'detached', 'jar lost': rb'jar lost', 'taking back': rb'Taking this tab', 'not started': rb'not started', 'got_': rb'got_'}
i = 0
while time.monotonic() - ts < LIMIT:
    if MODE == 'typing':
        os.write(fd, b'x' if i % 10 else b'\r'); i += 1
    elif MODE in ('paste', 'pasteR') and i == 0:
        os.write(fd, b'\x1b[200~' + b'p' * 200000 + b'\x1b[201~'); i += 1
    pump(0.2)
    for k, p in pats.items():
        if k not in firsts and seen(p, mark):
            firsts[k] = time.monotonic() - ts; ev('first', k, '%.1f s after SIGSTOP' % firsts[k])
    if 'detached' in firsts and time.monotonic() - ts > firsts['detached'] + 30: break
ev('watch over; firsts', json.dumps(firsts), 'keys sent', i)
# continue the jar: does the tab come back?
os.kill(jar_pid, signal.SIGCONT); ev('SIGCONT jar'); mark2 = len(buf)
pump(3); mark2 = len(buf); os.write(fd, b'\rzzz9\r'); pump(8); back = buf[mark2:].count(b'zzz9') >= 2; ev('typing after SIGCONT reaches the program', back)
# where is the jar now?
st = subprocess.run(['ps', '-o', 'pid=,ppid=,stat=,command=', '-p', str(jar_pid)], capture_output=True, text=True).stdout.strip()
ev('jar after', st)
ev('screen tail', buf[-1500:])
ev('labels seen after SIGCONT', {k: seen(p, mark2) for k, p in pats.items()})
ev('jar sockets', subprocess.run(['ls','-la', rt + '/spyc/jars'], capture_output=True, text=True).stdout)
ev('my processes', subprocess.run(['ps','-A','-o','pid=,ppid=,stat=,command='], capture_output=True, text=True).stdout.count('spyc-jar sh'))
if MODE == 'pasteR':
    mark3 = len(buf)
    os.write(fd, b'\x01'); pump(0.3); os.write(fd, b'R'); pump(2)
    ev('after ^a R, screen asks', re.findall(rb'restart[^\x1b]{0,60}', buf[mark3:])[:3])
    if seen(rb'\[y/N\]', mark3): os.write(fd, b'y')
    pump(10)
    readies = re.findall(rb'READY_(\d+)', buf[mark3:])
    ev('new programs started after ^a R', readies)
    js = subprocess.run(['lsof', '-t', '+D', rt], capture_output=True, text=True).stdout.split()
    ev('processes holding files in my runtime dir', [subprocess.run(['ps','-o','pid=,ppid=,command=','-p',p],capture_output=True,text=True).stdout.strip() for p in js])
    ev('old program alive', subprocess.run(['ps','-o','pid=,command=','-p',str(prog_pid)],capture_output=True,text=True).stdout.strip())
ev('done')
