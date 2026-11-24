
import sys; sys.path.insert(0, __import__('os').environ['UA_S'])
from probe_lib import *
p, d, sock = start_jar('lateproof')
try:
    o = Owner(sock)
    log('echo one', o.echo('one'))
    # The real validator's proof, delivered on standard output after the validator has exited 0 and after the deadline.
    w = wrapper(d, 'w-late', JAR, 'validate-handover) out=$("$REAL" validate-handover); ( sleep 40; echo "$out" ) & exit 0;;')
    ans = o.upgrade(w, wait=80); log('validator exits 0 at once; its proof line arrives after the 30 s deadline:', ans)
    log('echo two', o.echo('two'))
    # macOS: after a failed exec, holders that close their end at once
    ans = o.upgrade(wrapper(d, 'w-vanish', JAR, 'hold-handover) rm -f "$0";;')); log('failed exec:', ans)
    for i in range(20):
        ans = o.upgrade(wrapper(d, 'w-quit%d' % i, JAR, 'hold-handover) exec 0<&-; exit 0;;'), wait=30)
        if p.poll() is not None: log('JAR DIED', p.returncode, ans); break
    else: log('macOS: jar alive after 20 early-quitting holders; last', ans)
    log('echo three', o.echo('three'))
finally:
    if p.poll() is None: end(p, d, sock)
