
import sys; sys.path.insert(0, __import__('os').environ['UA_S'])
from probe_lib import *
p, d, sock = start_jar('linux')
try:
    o = Owner(sock)
    log('echo one', o.echo('one'))
    linux_view(p.pid, 'BEFORE')
    vlog = os.path.join(d, 'child')
    cases = 'validate-handover) ls -l /proc/$$/fd > %s.validate.$$ 2>&1; grep Sig /proc/$$/status >> %s.validate.$$;;\nhold-handover) ls -l /proc/$$/fd > %s.hold.$$ 2>&1;;' % (vlog, vlog, vlog)
    ans = o.upgrade(wrapper(d, 'w1', DIFFER, cases)); log('upgrade 1 -> differ:', ans)
    log('echo two', o.echo('two')); linux_view(p.pid, 'AFTER UPGRADE 1')
    ans = o.upgrade(wrapper(d, 'w2', DIFFER, cases)); log('upgrade 2 -> differ again:', ans)
    log('echo three', o.echo('three')); linux_view(p.pid, 'AFTER UPGRADE 2')
    for f in sorted(os.listdir(d)):
        if f.startswith('child.'): log('=== inherited by', f); log(open(os.path.join(d, f)).read())
finally:
    end(p, d, sock)
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
        ans = o.upgrade(wrapper(d, 'w-quit%d' % i, JAR, 'hold-handover) exec 0<&-; exit 0;;'), wait=30)
        n += 1
        if p.poll() is not None:
            log('JAR DIED after', n, 'tries; status', p.returncode, 'last answer', ans); break
        if i % 10 == 0: log('try', i, ans)
    else:
        log('jar alive after', n, 'tries; last', ans)
finally:
    if p.poll() is None: end(p, d, sock)
    else: shutil.rmtree(d, ignore_errors=True)
