
import sys; sys.path.insert(0, __import__('os').environ['UD_DIR'])
from ud import *
import glob
prep()
NAME = os.environ.get('UD_NAME')
s = None
try:
    s = Pty([BIN+'/spyc'], rows=30, cols=100)
    ok = s.wait_for(b'\x1b[?1049h', 30); s.pump(2)
    log('spyc up', ok, s.pid)
    if b'[y]' in s.buf: s.write(b'n'); s.pump(0.5)
    s.write((':name '+NAME+'\r').encode()); s.pump(1)
    s.write(b'\x01c'); s.pump(1)
    s.write(b'\x15cat\r')
    for _ in range(100):
        s.pump(0.2)
        if glob.glob(RT+'/spyc/jars/*.sock'): break
    s.pump(2)
    if b'[y]yes' in s.buf: s.write(b'n'); s.pump(0.5)
    jars = glob.glob(RT+'/spyc/jars/*.sock')
    log('pane jars', jars)
    for j in jars: log(' ', j, {k: describe(j).get(k) for k in ('owner','owner_pid','program','program_pid')})
    mark = len(s.buf)
    s.write(b'\x01 d')
    t0 = time.time()
    while s.alive() and time.time()-t0 < 40: s.pump(0.2)
    s.pump(1)
    log('spyc alive after detach:', s.alive(), 'status', s.status, 'after %.1fs' % (time.time()-t0))
    tail = s.buf[mark:].decode(errors='replace')
    import re
    log('terminal after the detach (escapes stripped):', repr(re.sub(r'\x1b\[[0-9;?<>=]*[a-zA-Z~]|\x1b[()][0-9A-Za-z]|\x1b[>=]', '', tail)[-900:]))
    for j in jars: log('pane jar now', j, {k: describe(j).get(k) for k in ('owner','owner_pid','program_pid')})
    log('sessions dir', os.listdir(SESS), 'jars dir', os.listdir(RT+'/spyc/jars'))
    log('events', open(glob.glob(D+'/state/spyc/events/*.log')[0]).read()[-700:])
    sd = D+'/state/spyc/sessions'
    for f in (os.listdir(sd) if os.path.isdir(sd) else []):
        m = json.load(open(sd+'/'+f)); log('manifest', f, 'name', m.get('name')[:40], 'pid', m.get('pid'), 'tabs', [t.get('jar_socket') for p in m['projects'] for t in p.get('tabs',[])])
finally:
    socks = glob.glob(RT+'/spyc/jars/*.sock') + glob.glob(SESS+'/*.sock')
    for j in socks:
        d = describe(j)
        for k in ('program_pid','pid'):
            try:
                v = int(d.get(k,'0') or 0)
                if v > 0: os.kill(v, signal.SIGKILL)
            except Exception: pass
    cleanup()
