
import sys, re; sys.path.insert(0, __import__('os').environ['UD_DIR'])
import ud
from ud import *
import glob
RTF = os.environ['UD_RT']
ud.RT = RTF; ud.SESS = RTF+'/spyc/sessions'; ud.ENV['XDG_RUNTIME_DIR'] = RTF
NAME = os.environ['UD_NAME']
def strip(b): return re.sub(r'\x1b\[[0-9;?<>=]*[a-zA-Z~@]|\x1b\][^\x07\x1b]*(\x07|\x1b\\)|\x1b[()][0-9A-Za-z]|\x1b[>=78]', '', b.decode(errors='replace'))
try:
    os.makedirs(RTF, exist_ok=True); os.chmod(RTF, 0o700)
    tabenv = D+'/tabenv'
    if os.path.exists(tabenv): os.remove(tabenv)
    s = Pty([BIN+'/spyc'], rows=30, cols=120)
    s.wait_for(b'\x1b[?1049h', 30); s.pump(2)
    s.write((':name '+NAME+'\r').encode()); s.pump(1)
    s.write(b'\x01c'); s.pump(1)
    s.write(('\x15sh -c \'env > '+tabenv+'; exec cat\'\r').encode())
    for _ in range(100):
        s.pump(0.2)
        if os.path.exists(tabenv) and os.path.getsize(tabenv) > 0: break
    s.pump(1)
    jar = [l.split('=',1)[1] for l in open(tabenv).read().splitlines() if l.startswith('SPYC_JAR_SOCKET=')][0]
    given = ud.SESS+'/'+NAME+'.sock'
    log('name len', len(NAME), 'given path len', len(given), 'canonical len', len(os.path.realpath(ud.SESS)+'/'+NAME+'.sock'))
    m = len(s.buf)
    s.write(b'\x01 d')
    t0 = time.time()
    newpid = None
    while time.time()-t0 < 60:
        s.pump(0.5)
        d = describe(given) if os.path.exists(given) else {}
        if d.get('program_pid') and describe(jar).get('owner_pid') not in ('', None, str(s.pid)):
            break
    s.pump(5)
    d = describe(given) if os.path.exists(given) else {}
    log('session jar', {k: d.get(k) for k in ('owner_pid','program_pid')}, 'tab jar', {k: describe(jar).get(k) for k in ('owner_pid','program_pid')})
    shown = strip(s.buf[m:])
    log('screen mentions own name:', NAME[:20] in shown, '| orphan offer:', [x for x in ['no spyc owned','restore','discard','Restore','Discard'] if x in shown])
    for f in glob.glob(D+'/state/spyc/events/*.log'):
        log('events', os.path.basename(f)[:30], open(f).read()[-500:])
finally:
    socks = glob.glob(RTF+'/spyc/jars/*.sock') + glob.glob(RTF+'/spyc/sessions/*.sock')
    for j in socks:
        dd = describe(j)
        for k in ('program_pid','pid'):
            try:
                v = int(dd.get(k,'0') or 0)
                if v > 0: os.kill(v, signal.SIGKILL)
            except Exception: pass
    cleanup()
