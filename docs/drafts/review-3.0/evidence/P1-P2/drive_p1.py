#!/usr/bin/env python3
# P1 probe: a scratch spyc in a pty with NTABS jarred tabs; each round SIGSTOPs spyc
# for 1-3 s, SIGCONTs it, and counts the tabs whose jar no longer has spyc as owner.
# Technique from evidence/C/drive_v7.py (pty.fork, TIOCSWINSZ, DA1/DSR answers).
# Usage: drive_p1.py SPYC_BIN BASE ROUNDS NTABS LABEL [SMIN SMAX] [restartR]
import os, sys, pty, select, time, signal, fcntl, termios, struct, re, json, subprocess, tempfile, random, glob

BIN = sys.argv[1]; BASE = sys.argv[2]; ROUNDS = int(sys.argv[3]); NTABS = int(sys.argv[4]); LABEL = sys.argv[5]
SMIN = float(sys.argv[6]) if len(sys.argv) > 6 else 1.0
SMAX = float(sys.argv[7]) if len(sys.argv) > 7 else 3.0
TRY_R = len(sys.argv) > 8 and sys.argv[8] == 'restartR'
JARBIN = os.path.join(os.path.dirname(BIN), 'spyc-jar')
os.makedirs(BASE, exist_ok=True)
LOG = open(os.path.join(BASE, 'events.log'), 'w', buffering=1)
RES = open(os.path.join(BASE, 'rounds.jsonl'), 'w', buffering=1)
T0 = time.monotonic()
random.seed(int(os.environ.get('P1_SEED', '1')))
def ev(*a): LOG.write('%9.3f %s\n' % (time.monotonic() - T0, ' '.join(str(x) for x in a)))

PROG = "sh -c 'echo READY_$$; while IFS= read -r l; do echo got_$l; done'"
all_pids = set()   # every jar and program pid seen, for the cleanup

class S: pass
sess = None

def ps_table():
    out = subprocess.run(['ps', '-A', '-o', 'pid=,ppid=,args='], capture_output=True, text=True).stdout
    rows = []
    for l in out.splitlines():
        p = l.split(None, 2)
        if len(p) >= 2 and p[0].isdigit() and p[1].isdigit():
            rows.append((int(p[0]), int(p[1]), p[2] if len(p) > 2 else ''))
    return rows

def descendants(roots):
    rows = ps_table(); out = set(); frontier = set(roots)
    while frontier:
        nxt = {p for p, pp, _ in rows if pp in frontier and p not in out}
        out |= nxt; frontier = nxt
    return out

def start_session(idx):
    s = S(); s.idx = idx
    s.dir = os.path.join(BASE, 'sess%03d' % idx); os.makedirs(s.dir, exist_ok=True)
    home = os.path.join(s.dir, 'home'); os.makedirs(home, exist_ok=True)
    dirs = {k: os.path.join(s.dir, k.lower()) for k in ['XDG_STATE_HOME','XDG_CONFIG_HOME','XDG_DATA_HOME','XDG_CACHE_HOME']}
    for d in dirs.values(): os.makedirs(d, exist_ok=True)
    if os.environ.get('P1_RT_IN'):
        # a short link under /tmp to a directory in the scratch (macOS caps socket paths at 104 bytes)
        real = os.path.join(os.environ['P1_RT_IN'], 'rt%03d' % idx); os.makedirs(real, exist_ok=True); os.chmod(real, 0o700)
        s.rt = tempfile.mktemp(prefix='p1', dir='/tmp'); os.symlink(real, s.rt)
    else:
        s.rt = tempfile.mkdtemp(prefix='p1', dir='/tmp'); os.chmod(s.rt, 0o700)
    open(os.path.join(s.dir, 'runtime_dir'), 'w').write(s.rt)
    env = {'PATH': '/usr/bin:/bin:/usr/sbin:/sbin', 'TERM': 'xterm-256color', 'LANG': 'en_CA.UTF-8',
           'HOME': home, 'XDG_RUNTIME_DIR': s.rt, 'TMPDIR': s.rt + '/', 'SPYC_PANE_CMD': PROG,
           'SHELL': '/bin/sh', 'SPYC_DEBUG': '1'}
    env.update(dirs); s.state = dirs['XDG_STATE_HOME']
    wrap = os.environ.get('P1_WRAP', '').split()
    pid, fd = pty.fork()
    if pid == 0:
        os.chdir(home)
        if wrap: os.execve(wrap[0], wrap + [BIN], env)
        os.execve(BIN, [BIN], env)
    s.lead, s.fd, s.buf = pid, fd, b''
    s.pid = pid
    if wrap:
        # P1_WRAP (strace with an injected delay): spyc is the wrapper's child
        for _ in range(100):
            kids = [p for p, pp, a in ps_table() if pp == pid and BIN in a]
            if kids: s.pid = kids[0]; break
            time.sleep(0.1)
    s.raw = open(os.path.join(s.dir, 'raw.out'), 'wb', buffering=0)
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack('HHHH', 40, 160, 0, 0))
    ev('session', idx, 'spyc pid', pid, 'rt', s.rt)
    return s

def pump(s, t):
    end = time.monotonic() + t
    while True:
        left = end - time.monotonic()
        if left <= 0: return
        r, _, _ = select.select([s.fd], [], [], min(left, 0.05))
        if s.fd in r:
            try: d = os.read(s.fd, 65536)
            except OSError: ev('pty closed'); time.sleep(left); return
            s.raw.write(d); s.buf += d
            for q, a in [(b'\x1b[c', b'\x1b[?62c'), (b'\x1b[5n', b'\x1b[0n'), (b'\x1b[6n', b'\x1b[1;1R'), (b'\x1b[?u', b'\x1b[?0u')]:
                for _ in range(d.count(q)): os.write(s.fd, a)

def readies(s, since=0): return [int(x) for x in re.findall(rb'READY_(\d+)', s.buf[since:])]

def open_tabs(s):
    pump(s, 4)
    for i in range(NTABS):
        have = len(set(readies(s)))
        os.write(s.fd, b'\x01'); pump(s, 0.3); os.write(s.fd, b'c'); pump(s, 1.0); os.write(s.fd, b'\r')
        end = time.monotonic() + 40
        while time.monotonic() < end and len(set(readies(s))) <= have: pump(s, 0.2)
        if len(set(readies(s))) <= have:
            ev('tab', i, 'never ready'); return False
    s.progs = sorted(set(readies(s)))
    pump(s, 2)
    s.sockets = sorted(glob.glob(os.path.join(s.rt, 'spyc', 'jars', '*.sock')))
    s.jars = [p for p, pp, a in ps_table() if pp == s.pid and 'spyc-jar' in a]
    all_pids.update(s.jars); all_pids.update(s.progs)
    ev('tabs ready', 'programs', s.progs, 'jars', s.jars, 'sockets', len(s.sockets))
    return len(s.sockets) == NTABS and len(s.jars) == NTABS

def describe(sock):
    try:
        out = subprocess.run([JARBIN, 'describe', sock], capture_output=True, text=True, timeout=8).stdout
    except Exception as e:
        return {'error': str(e)}
    d = {}
    for l in out.splitlines():
        m = re.match(r'\s*([A-Za-z_]+)\s*[:=]\s*(.*)$', l)
        if m: d[m.group(1)] = m.group(2).strip()
    return d

def owners(s):
    return {os.path.basename(k): describe(k).get('owner_pid', '?') for k in s.sockets}

def tail_from(path, off):
    try:
        with open(path, 'rb') as f: f.seek(off); return f.read().decode('utf-8', 'replace')
    except FileNotFoundError: return ''
def size(path):
    try: return os.path.getsize(path)
    except FileNotFoundError: return 0
def logs(s):
    dbg = sorted(glob.glob(os.path.join(s.state, 'spyc', 'spyc-debug-*.log')))
    err = sorted(glob.glob(os.path.join(s.state, 'spyc', 'stderr', '*.log')))
    evl = sorted(glob.glob(os.path.join(s.state, 'spyc', 'events', '*.log')))
    return [x[-1] if x else '' for x in (dbg, err, evl)]

def end_session(s):
    if s is None: return
    try: os.killpg(s.lead, signal.SIGKILL)
    except Exception as e: ev('killpg', e)
    try: os.kill(s.pid, signal.SIGKILL)
    except Exception: pass
    try: os.waitpid(s.lead, 0)
    except Exception: pass
    jars = set(getattr(s, 'jars', [])) | {p for p, pp, a in ps_table() if 'spyc-jar' in a and p in all_pids}
    victims = set(jars) | descendants(jars) | set(getattr(s, 'progs', []))
    try:
        held = subprocess.run(['lsof', '-t', '+D', s.rt], capture_output=True, text=True).stdout.split()
        victims |= {int(p) for p in held if p.isdigit() and int(p) != os.getpid()}
    except Exception as e: ev('lsof', e)
    for p in victims:
        try: os.kill(p, signal.SIGCONT); os.kill(p, signal.SIGKILL)
        except ProcessLookupError: pass
        except Exception as e: ev('kill', p, e)
    ev('ended session', s.idx, 'killed', sorted(victims))
    if os.path.islink(s.rt): os.unlink(s.rt)
    else: subprocess.run(['rm', '-rf', s.rt])

import atexit
atexit.register(lambda: end_session(sess))

totals = {'rounds': 0, 'tabs_at_risk': 0, 'readers_ended': 0, 'retaken': 0, 'detached': 0, 'rounds_with_loss': 0, 'sessions': 0}
nsess = 0
r = 0
while r < ROUNDS:
    if sess is None:
        sess = start_session(nsess); nsess += 1; totals['sessions'] += 1
        if not open_tabs(sess):
            ev('session failed to start; tail', sess.buf[-2000:]); end_session(sess); sess = None
            if nsess > ROUNDS + 5: break
            continue
        o = owners(sess)
        ev('owners at start', o)
        if any(v != str(sess.pid) for v in o.values()):
            ev('not every jar owned at start'); end_session(sess); sess = None; continue
    s = sess
    dbg, err, evl = logs(s)
    marks = (size(dbg), size(err), size(evl), len(s.buf))
    stop = round(random.uniform(SMIN, SMAX), 2)
    pump(s, 0.5)
    ext = os.environ.get('P1_EXTERNAL')
    if ext:
        # the stop comes from outside (podman pause / unpause of this container): say ready, wait for done
        open(os.path.join(ext, 'ready_%d' % r), 'w').write(str(stop)); t_stop = time.monotonic()
        while not os.path.exists(os.path.join(ext, 'done_%d' % r)): time.sleep(0.05)
        t_cont = time.monotonic()
    elif os.environ.get('P1_CONT_ONLY'):
        # no stop: a bare SIGCONT to a running spyc, now a caught signal since #1317
        t_stop = time.monotonic(); os.kill(s.pid, signal.SIGCONT); t_cont = time.monotonic(); stop = 0
    else:
        os.kill(s.pid, signal.SIGSTOP); t_stop = time.monotonic()
        time.sleep(stop)
        os.kill(s.pid, signal.SIGCONT); t_cont = time.monotonic()
    # wait for every jar to be owned by spyc again, or the retakes to have failed
    deadline = t_cont + 30; o = {}
    while time.monotonic() < deadline:
        pump(s, 1.0)
        o = owners(s)
        dbg, err, evl = logs(s)
        settled_bad = len(re.findall(r'the tab is detached', tail_from(err, marks[1]))) + len(re.findall(r"didn't start", tail_from(evl, marks[2])))
        good = sum(1 for v in o.values() if v == str(s.pid))
        if good == NTABS and time.monotonic() - t_cont > 3: break
        if good + settled_bad >= NTABS and time.monotonic() - t_cont > 3: break
    pump(s, 1.0)
    dbg, err, evl = logs(s)
    dtail, etail, vtail = tail_from(dbg, marks[0]), tail_from(err, marks[1]), tail_from(evl, marks[2])
    ended = re.findall(r'jar \S+: (detached|jar lost) after (.*)', dtail)
    retaken = len(re.findall(r'taken back from its jar', vtail))
    det_lines = re.findall(r"spyc: a restored tab's jar runs on without it \((.*)\); the tab is detached", etail)
    unowned = sorted(k for k, v in o.items() if v != str(s.pid))
    screen_detached = s.buf[marks[3]:].count(b'detached')
    rec = {'label': LABEL, 'round': r, 'session': s.idx, 'stop_s': stop, 'readers_ended': len(ended),
           'reader_errors': sorted(set(e for _, e in ended)), 'retaken': retaken, 'detached_reasons': det_lines,
           'unowned_after': len(unowned), 'owners': o, 'screen_detached_mentions': screen_detached,
           'events_tail': vtail.strip().splitlines()[-8:], 'settle_s': round(time.monotonic() - t_cont, 1)}
    RES.write(json.dumps(rec) + '\n')
    ev('round', r, json.dumps({k: rec[k] for k in ('stop_s','readers_ended','reader_errors','retaken','detached_reasons','unowned_after','settle_s')}))
    totals['rounds'] += 1; totals['tabs_at_risk'] += NTABS; totals['readers_ended'] += len(ended)
    totals['retaken'] += retaken; totals['detached'] += len(unowned)
    r += 1
    if unowned:
        totals['rounds_with_loss'] += 1
        if TRY_R and not totals.get('restartR_done'):
            totals['restartR_done'] = True
            # ^a N to a detached tab (tabs open in jar-pid order), then ^a R: does it ask, does a second program start?
            jpid = describe(os.path.join(s.rt, 'spyc', 'jars', unowned[0])).get('pid', '')
            order = sorted(s.jars)
            n = order.index(int(jpid)) + 1 if jpid.isdigit() and int(jpid) in order else NTABS
            ev('^a', n, 'to the detached tab of jar', jpid)
            os.write(s.fd, b'\x01'); pump(s, 0.3); os.write(s.fd, str(n).encode()); pump(s, 1.5)
            m3 = len(s.buf); before = set(readies(s))
            os.write(s.fd, b'\x01'); pump(s, 0.3); os.write(s.fd, b'R'); pump(s, 2)
            asked = re.findall(rb'restart[^\x1b]{0,60}', s.buf[m3:])[:3]
            if re.search(rb'\[y/N\]', s.buf[m3:]): os.write(s.fd, b'y')
            pump(s, 10)
            new = sorted(set(readies(s, m3)) - before)
            alive = {p: subprocess.run(['ps', '-o', 'pid=,args=', '-p', str(p)], capture_output=True, text=True).stdout.strip() for p in s.progs}
            jars_now = [(p, a) for p, pp, a in ps_table() if 'spyc-jar' in a and (pp == s.pid or p in all_pids)]
            all_pids.update(new); all_pids.update(p for p, a in jars_now)
            ev('^a R on the active tab', json.dumps({'asked': [x.decode('utf-8','replace') for x in asked], 'new_programs': new, 'old_programs_alive': alive, 'jars': jars_now}))
            totals['restartR'] = {'asked': [x.decode('utf-8','replace') for x in asked], 'new_programs': new, 'old_alive': [p for p, v in alive.items() if v]}
        end_session(s); sess = None
ev('totals', json.dumps(totals))
open(os.path.join(BASE, 'totals.json'), 'w').write(json.dumps(totals, indent=1))
print(json.dumps(totals))
