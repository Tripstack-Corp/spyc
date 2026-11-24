
# P3 variant of probe_linux.py's second half: the same failed exec, then
# upgrades to a holder that exits at once (holder/quick_holder, native), in
# place of the shell wrapper, which under x86_64 emulation is too slow to be
# gone before the jar's sendmsg.
import sys; sys.path.insert(0, __import__('os').environ['UA_S'])
from probe_lib import *
QUICK = os.environ['UA_QUICK']
p, d, sock = start_jar('linuxfail')
try:
    o = Owner(sock)
    log('echo one', o.echo('one'))
    linux_view(p.pid, 'BEFORE failed exec')
    ans = o.upgrade(wrapper(d, 'w-vanish', JAR, 'hold-handover) rm -f "$0";;')); log('failed exec:', ans)
    linux_view(p.pid, 'AFTER failed exec')
    log('echo two', o.echo('two'))
    n = 0
    for i in range(int(os.environ.get('UA_TRIES', '40'))):
        q = os.path.join(d, 'w-quick%d' % i); shutil.copy(QUICK, q); os.chmod(q, 0o755)
        ans = o.upgrade(q, wait=30)
        n += 1
        if p.poll() is not None:
            log('JAR DIED after', n, 'tries; status', p.returncode, 'last answer', ans); break
        if i % 10 == 0: log('try', i, ans)
    else:
        log('jar alive after', n, 'tries; last', ans)
finally:
    if p.poll() is None: end(p, d, sock)
    else: shutil.rmtree(d, ignore_errors=True)
