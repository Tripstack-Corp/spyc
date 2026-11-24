
import sys, re; sys.path.insert(0, __import__('os').environ['UD_DIR'])
from ud import *
import glob
prep()
TABENV = D+'/tabenv'
def squeezed(b): return re.sub(r'\s+','', re.sub(r'\x1b\[[0-9;?<>=]*[a-zA-Z~@]|\x1b\][^\x07\x1b]*(\x07|\x1b\\)|\x1b[()][0-9A-Za-z]|\x1b[>=78]', '', b.decode(errors='replace')))
def wait_sq(p, needle, since, t=20):
    end = time.time()+t
    while time.time() < end:
        p.pump(0.2)
        if needle in squeezed(p.buf[since:]): return True
    return False
try:
    if os.path.exists(TABENV): os.remove(TABENV)
    s = Pty([BIN+'/spyc'], rows=30, cols=120)
    s.wait_for(b'\x1b[?1049h', 30); s.pump(2)
    s.write(b':name SHORTNM\r'); s.pump(1)
    s.write(b'\x01c'); s.pump(1)
    s.write(('\x15sh -c \'env > '+TABENV+'; exec cat\'\r').encode())
    for _ in range(100):
        s.pump(0.2)
        if os.path.exists(TABENV) and os.path.getsize(TABENV) > 0: break
    s.pump(1)
    tabenv = dict(l.split('=',1) for l in open(TABENV).read().splitlines() if '=' in l)
    agent = {k: tabenv[k] for k in ('SPYC_MCP_SOCK','SPYC_PANE_ID','SPYC_JAR_SOCKET') if k in tabenv}
    log('tab env', agent)
    r = subprocess.run([BIN+'/spyc','--report-status','blocked'], env={**ENV, **agent}, capture_output=True, timeout=20, cwd=D+'/home')
    log('report-status', r.returncode, r.stderr[-200:])
    s.pump(1)
    s.write(b'\x01k'); s.pump(0.5)
    m = len(s.buf); s.write(b':activity dump\r')
    log('before ^d: dot=blocked shown', wait_sq(s, 'dot=blocked', m))
    log('  dump says', re.findall(r'dot=\w+', squeezed(s.buf[m:]))[:5])
    s.write(b'q'); s.pump(0.5)
    jar = agent['SPYC_JAR_SOCKET']
    log('jar before', {k: describe(jar).get(k) for k in ('owner_pid','program_pid')})
    m = len(s.buf)
    s.write(b'\x01 d')
    ok = wait_sq(s, 'SHORTNM', m, 40)
    s.pump(3)
    sess = SESS+'/SHORTNM.sock'
    d = describe(sess)
    log('after ^d: session jar', {k: d.get(k) for k in ('owner_pid','program','program_pid')}, 'pty pid', s.pid, 'alive', s.alive())
    for _ in range(50):
        if describe(jar).get('owner_pid') == d.get('program_pid'): break
        time.sleep(0.2)
    log('tab jar after', {k: describe(jar).get(k) for k in ('owner_pid','program_pid')})
    s.write(b'\x01k'); s.pump(0.5)
    m = len(s.buf); s.write(b':activity dump\r')
    got = wait_sq(s, 'dot=', m, 15)
    log('after ^d: dump says', re.findall(r'dot=\w+', squeezed(s.buf[m:]))[:5])
    s.write(b'q'); s.pump(0.5)
    # the event log of the new spyc
    for f in glob.glob(D+'/state/spyc/events/*.log'):
        log('events', f[-20:], open(f).read()[-900:])
finally:
    socks = glob.glob(RT+'/spyc/jars/*.sock') + glob.glob(SESS+'/*.sock')
    for j in socks:
        dd = describe(j)
        for k in ('program_pid','pid'):
            try:
                v = int(dd.get(k,'0') or 0)
                if v > 0: os.kill(v, signal.SIGKILL)
            except Exception: pass
    cleanup()
