
import sys; sys.path.insert(0, __import__('os').environ['UA_S'])
from probe_lib import *
p, d, sock = start_jar('h5')
try:
    o = Owner(sock)
    log('echo one', o.echo('one'))
    pidf = os.path.join(d, 'holder.pid')
    w = wrapper(d, 'w-stoppable', JAR, 'hold-handover) echo $$ > %s; trap "kill -STOP $" TERM; "$REAL" hold-handover <&0 & wait; wait; exit 0;;' % pidf)
    ans = o.upgrade(w, wait=10); log('upgrade whose holder stops itself when the new image ends it:', ans)
    hp = int(open(pidf).read())
    log('holder', hp, sh('ps -o pid,stat,command -p %d' % hp).strip())
    t0 = time.time(); ok = o.echo('two', wait=20); log('echo two within 20 s:', ok, 'after', round(time.time()-t0,1))
    try: log('lookup:', describe(sock))
    except Exception as e: log('lookup failed:', repr(e))
    log('jar threads/state:', sh('ps -M -p %d | head -5' % p.pid))
    os.kill(hp, signal.SIGKILL); time.sleep(1)
    log('after killing the stopped holder: echo three', o.echo('three', wait=10))
    ok = False
    for i in range(20):
        try: o2 = Owner(sock, role=2); ok = o2.echo('x', 3) or True; break
        except Exception as e: time.sleep(0.5)
    log('a new reader connects after the holder is gone:', ok)
finally:
    end(p, d, sock)
