# R18: after a `kill -9` of spyc, `spyc --jars` says what each orphaned jar
# is from what spyc wrote into it: the tab's label, agent and project.
import os, shutil, subprocess
from common import *
T = open(f"{D}/target_dir.txt").read().strip()
J = f"{T}/debug/spyc-jar"
home = f"{D}/home_r18"
shutil.rmtree(home, ignore_errors=True); os.makedirs(home)
open(f"{home}/.spycrc.toml", "w").write('[pane]\ntabs = ["zsh"]\n'); open(f"{home}/.zshrc", "w").close()
state = os.path.join(os.environ.get("TMPDIR", "/tmp"), "sr18")
shutil.rmtree(state, ignore_errors=True); os.makedirs(state)
env = env_for(home); env.update(SPYC_JARS="1", SPYC_JAR_BIN=J, XDG_STATE_HOME=state)
a = Term([f"{T}/debug/spyc"], env, f"{D}/work", rows=40, cols=120)
a.pump(8)
kill_and_reap(a)
out = subprocess.run([f"{T}/debug/spyc", "--jars"], env=env, capture_output=True, text=True).stdout
mine = [l for l in out.splitlines() if "(other in work)" in l or "zsh (" in l]
print("spyc --jars names the orphaned tab:", mine[:1])
for l in out.splitlines():
    if "zsh (" in l:
        sock = os.path.join(os.environ.get("TMPDIR", "/tmp"), "spyc", "jars", l.split()[0] + ".sock")
        subprocess.run([J, "describe", sock])
        pid = int(l.split(" pid ")[1].split()[0])
        subprocess.run(["kill", "-9", str(pid)])
