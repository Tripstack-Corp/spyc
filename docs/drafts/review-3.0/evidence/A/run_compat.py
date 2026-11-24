import subprocess, os, sys
env = {k: v for k, v in os.environ.items() if not k.startswith('SPYC') and k != 'SSH_AUTH_SOCK'}
env['SPYC_JAR_OLD_BIN'] = '~/.cache/spyc/target/3.0-9f8124c5/debug/spyc-jar'
p = subprocess.Popen([sys.argv[1], '--ignored', '--nocapture'], env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, start_new_session=True)
try:
    out, _ = p.communicate(timeout=30); print(out); print('EXIT', p.returncode)
except subprocess.TimeoutExpired:
    os.killpg(p.pid, 9); out, _ = p.communicate(); print(out); print('TIMED OUT after 30 s: killed')
