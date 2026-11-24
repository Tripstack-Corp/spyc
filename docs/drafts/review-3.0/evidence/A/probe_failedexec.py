
import sys; sys.path.insert(0, __import__('os').environ['UA_S'])
from probe_lib import *
SIGS = os.path.join(S, 'sigs')
p, d, sock = start_jar('failedexec')
try:
    o = Owner(sock)
    log('echo one', o.echo('one'))
    log('BEFORE:', sh('%s %d' % (SIGS, p.pid)).strip())
    w = wrapper(d, 'w-vanish', JAR, 'hold-handover) rm -f "$0";;')
    ans = o.upgrade(w); log('upgrade to a binary removed before the exec:', ans)
    log('AFTER failed exec:', sh('%s %d' % (SIGS, p.pid)).strip())
    log('echo two', o.echo('two'), 'lock', lock_state(sock))
    log(sh('lsof -n -P -a -p %d -d 0-4' % p.pid))
    ans = o.upgrade(wrapper(d, 'w-ok', JAR, '')); log('next upgrade:', ans)
    log('echo three', o.echo('three'))
    log('AFTER next upgrade:', sh('%s %d' % (SIGS, p.pid)).strip())
    # FIFO: an upgrade that names a FIFO
    fifo = os.path.join(d, 'fifo'); os.mkfifo(fifo)
    o.s.sendall(frame(12, fifo.encode())); t0 = time.time()
    time.sleep(2)
    with o.lock: ev = [e for e in o.events if e[3 if e[0]=='refused' else 2] > t0] if o.events else []
    log('after naming a FIFO, 2 s, answers:', ev)
    ans = o.upgrade(wrapper(d, 'w-ok2', JAR, ''), wait=15); log('a real upgrade meanwhile:', ans)
    log('echo four', o.echo('four'))
    time.sleep(40)
    ans = o.upgrade(wrapper(d, 'w-ok3', JAR, ''), wait=15); log('a real upgrade 40 s later:', ans)
    # release the fifo
    fd = os.open(fifo, os.O_WRONLY); os.write(fd, b'not a jar\n'); os.close(fd)
    time.sleep(2)
    with o.lock: log('events after the FIFO got a writer:', o.events[-3:])
    ans = o.upgrade(wrapper(d, 'w-ok4', JAR, ''), wait=30); log('a real upgrade once the FIFO is released:', ans)
    # V10: a reader with the token runs a file of its choice
    r = Owner(sock, role=2)
    mark = os.path.join(d, 'ran-by-reader')
    w = wrapper(d, 'w-reader', JAR, 'validate-handover) id -un > %s; head -c 400 | strings | grep -c unit-a-probe-token >> %s; exit 3;;' % (mark, mark))
    ans = r.upgrade(w); log('reader asks for an upgrade to a file of its choice:', ans)
    log('the file ran, and saw the token in its stdin:', open(mark).read() if os.path.exists(mark) else 'NOT RUN')
finally:
    end(p, d, sock)
