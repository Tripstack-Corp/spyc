
import sys; sys.path.insert(0, __import__('os').environ['UA_S'])
from probe_lib import *
p, d, sock = start_jar('boundary')
try:
    o = Owner(sock)
    log('welcome', o.welcome, 'echo one', o.echo('one'))
    log('describe', json.dumps(describe(sock)))
    log('lock', lock_state(sock))
    proc_view(p.pid, 'BEFORE (stock image)')
    vlog = os.path.join(d, 'child')
    cases = ('validate-handover) lsof -n -P -p $$ > %s.validate.$$ 2>&1; ps -o pid,sigignore,sigmask -p $$ >> %s.validate.$$;;\n'
             'hold-handover) lsof -n -P -p $$ > %s.hold.$$ 2>&1; ps -o pid,sigignore,sigmask -p $$ >> %s.hold.$$;;') % (vlog, vlog, vlog, vlog)
    w1 = wrapper(d, 'w1-differ', DIFFER, cases)
    ans = o.upgrade(w1); log('upgrade 1 (stock -> differ, by wrapper):', ans)
    log('echo two', o.echo('two')); log('lock', lock_state(sock))
    log('describe', json.dumps(describe(sock)))
    proc_view(p.pid, 'AFTER UPGRADE 1 (differ image)')
    w2 = wrapper(d, 'w2-differ', DIFFER, cases)
    ans = o.upgrade(w2); log('upgrade 2 (differ -> differ, from an image that arrived by upgrade):', ans)
    log('echo three', o.echo('three')); log('lock', lock_state(sock))
    log('describe', json.dumps(describe(sock)))
    proc_view(p.pid, 'AFTER UPGRADE 2')
    w3 = wrapper(d, 'w3-stock', JAR, cases)
    ans = o.upgrade(w3); log('upgrade 3 (differ -> stock, a downgrade):', ans)
    log('echo four', o.echo('four'))
    for f in sorted(os.listdir(d)):
        if f.startswith('child.'):
            log('=== inherited by', f); log(open(os.path.join(d, f)).read())
    log('jar-bin contents:', sh('ls -laR %s/jar-bin | head -40' % d))
finally:
    end(p, d, sock)
    log('left in /tmp:', sh('ls -d /tmp/uaA-* 2>/dev/null'))
