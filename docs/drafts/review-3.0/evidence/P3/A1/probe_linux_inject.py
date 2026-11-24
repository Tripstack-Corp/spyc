
# P3 variant of probe_linux.py's second half for x86_64 under qemu-user, where
# a spawn returns before the holder has run (qemu turns glibc's vfork-style
# clone into a fork), so the jar's sendmsg always beats the holder's exit. A
# tracer in a sidecar container (inject_sidecar.sh) delays each sendmsg of the
# jar by 300 ms once this script asks, which puts the holder's exit before the
# send, as the native aarch64 run found by chance after 5 tries.
# UA_SKIPFAIL=1 is the control: no failed exec first, so SIGPIPE stays ignored.
import sys; sys.path.insert(0, __import__('os').environ['UA_S'])
from probe_lib import *
SKIP = os.environ.get('UA_SKIPFAIL') == '1'
p, d, sock = start_jar('linuxfail')
try:
    o = Owner(sock)
    log('echo one', o.echo('one'))
    linux_view(p.pid, 'BEFORE failed exec')
    if not SKIP:
        ans = o.upgrade(wrapper(d, 'w-vanish', JAR, 'hold-handover) rm -f "$0";;')); log('failed exec:', ans)
    else:
        log('control: no failed exec')
    linux_view(p.pid, 'AFTER failed exec' if not SKIP else 'CONTROL')
    log('echo two', o.echo('two'))
    open(os.path.join(S, 'jarpid'), 'w').write(str(p.pid))
    open(os.path.join(S, 'attach-now'), 'w').write('')
    t = time.time()
    while not os.path.exists(os.path.join(S, 'attached')):
        assert time.time() - t < 60, 'no tracer'
        time.sleep(0.1)
    log('tracer attached')
    n = 0
    for i in range(int(os.environ.get('UA_TRIES', '10'))):
        ans = o.upgrade(wrapper(d, 'w-quit%d' % i, JAR, 'hold-handover) exec 0<&-; exit 0;;'), wait=30)
        n += 1
        if p.poll() is not None:
            log('JAR DIED after', n, 'tries; status', p.returncode, 'last answer', ans); break
        log('try', i, ans)
    else:
        log('jar alive after', n, 'tries; last', ans)
finally:
    if p.poll() is None: end(p, d, sock)
    else: shutil.rmtree(d, ignore_errors=True)
