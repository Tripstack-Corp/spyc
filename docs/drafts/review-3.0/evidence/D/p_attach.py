
import sys; sys.path.insert(0, __import__('os').environ['UD_DIR'])
from ud import *
prep()
A_S, B_S = SESS+'/ALPHA.sock', SESS+'/BETA.sock'
out = D+'/alpha.out'
try:
    open(out,'wb').close()
    ja = start_jar(A_S, ['/bin/sh','-c','stty raw -echo; exec cat > '+out])
    jb = start_jar(B_S, ['/bin/cat'])
    keep_token('ALPHA'); keep_token('BETA')
    log('jars', ja.pid, jb.pid, describe(A_S).get('program_pid'))
    c1 = Pty([BIN+'/spyc-jar','attach',A_S,TOK+'/ALPHA'])
    for _ in range(100):
        c1.pump(0.1)
        if describe(A_S).get('owner_pid') == str(c1.pid): break
    log('C1', c1.pid, 'owner_pid', describe(A_S).get('owner_pid'))
    a = Pty([BIN+'/spyc','attach'])
    log('prompt seen', a.wait_for(b'Attach to which?', 30))
    log('A shows:', repr(a.text()))
    # the client listed as attached goes away, and another attaches
    os.kill(c1.pid, signal.SIGTERM)
    for _ in range(100):
        c1.pump(0.05)
        if describe(A_S).get('owner') != 'connected': break
    log('after C1 left: owner', describe(A_S).get('owner'), 'C1 alive', c1.alive())
    c2 = Pty([BIN+'/spyc-jar','attach',A_S,TOK+'/ALPHA'])
    for _ in range(100):
        c2.pump(0.1)
        if describe(A_S).get('owner_pid') == str(c2.pid): break
    log('C2', c2.pid, 'owner_pid', describe(A_S).get('owner_pid'))
    a.write(b'1\r')
    t0 = time.time()
    while a.alive() and time.time()-t0 < 30: a.pump(0.2)
    a.pump(0.3)
    log('A exited after %.1fs status' % (time.time()-t0), a.status, 'C2 alive', c2.alive())
    log('A said:', repr(a.text()[-600:]))
    log('owner_pid now', describe(A_S).get('owner_pid'))
    # H6: one read holding ^\ in the middle
    c2.pump(0.5)
    c2.write(b'hello\x1cworld')
    t0 = time.time()
    while c2.alive() and time.time()-t0 < 10: c2.pump(0.1)
    c2.pump(0.3)
    log('C2 after hello^\\world: alive', c2.alive(), 'tail', repr(c2.text()[-80:]))
    time.sleep(0.5)
    log('program received', repr(open(out,'rb').read()))
    # bracketed paste holding the byte
    c3 = Pty([BIN+'/spyc-jar','attach',A_S,TOK+'/ALPHA'])
    for _ in range(100):
        c3.pump(0.1)
        if describe(A_S).get('owner_pid') == str(c3.pid): break
    c3.pump(0.5)
    c3.write(b'\x1b[200~paste-a\x1cpaste-b\x1b[201~')
    t0 = time.time()
    while c3.alive() and time.time()-t0 < 10: c3.pump(0.1)
    c3.pump(0.3); time.sleep(0.5)
    log('C3 after a paste holding 0x1c: alive', c3.alive(), 'tail', repr(c3.text()[-60:]))
    log('program received', repr(open(out,'rb').read()))
finally:
    cleanup([A_S, B_S])
