
import sys; sys.path.insert(0, __import__('os').environ['UA_S'])
from probe_lib import *
for name, case in [('nodesc', 'resume-handover) exec "$REAL" "$@" 0</dev/null;;'),
                   ('fault-blob', 'resume-handover) SPYC_JAR_RESUME_FAULT=blob exec "$REAL" "$@";;'),
                   ('fault-panic', 'resume-handover) SPYC_JAR_RESUME_FAULT=panic exec "$REAL" "$@";;')]:
    p, d, sock = start_jar(name)
    try:
        o = Owner(sock); log(name, 'echo one', o.echo('one'))
        pp = int(describe(sock)['program_pid'])
        ans = o.upgrade(wrapper(d, 'w-' + name, JAR, case), wait=20); log(name, 'answer:', ans)
        time.sleep(1.5)
        alive = subprocess.run(['kill', '-0', str(pp)]).returncode == 0
        log(name, 'jar process alive:', p.poll() is None, 'program alive:', alive)
        try: log(name, 'lookup:', {k: v for k, v in describe(sock).items() if k in ('held', 'pid', 'drained')})
        except Exception as e: log(name, 'lookup failed:', repr(e))
    finally:
        if p.poll() is None: end(p, d, sock)
        else:
            try: os.killpg(pp, signal.SIGKILL)
            except Exception: pass
            shutil.rmtree(d, ignore_errors=True)
