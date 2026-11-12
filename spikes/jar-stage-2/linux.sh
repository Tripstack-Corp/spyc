#!/bin/sh
# Runs inside the container (see run-linux.sh): the jar tests, then spyc on
# jars through the terminal harness, against Linux builds.
set -e
apk add --no-cache musl-dev python3 py3-pip bash zsh procps less >/dev/null
pip install -q --break-system-packages pyte 2>/dev/null
cd /src
echo "== spyc-jar tests"
cargo test -q --locked -p spyc-jar 2>&1 | grep -E 'test result|panicked|FAILED' || true
cargo build -q --locked --workspace
H=/tmp/h; rm -rf $H; mkdir -p $H/work/src $H/work/docs
cp /linux/*.py /linux/probe.sh $H/; chmod +x $H/probe.sh
echo "$CARGO_TARGET_DIR" > $H/target_dir.txt
seq 1 400 | sed 's/^/line number /' > $H/work/long.txt
echo hello > $H/work/README.md
for h in home2 home3 home4 home6 home9; do mkdir -p $H/$h; printf '[pane]\ntabs = ["zsh"]\n' > $H/$h/.spycrc.toml; : > $H/$h/.zshrc; done
cd $H
echo "== crash and restore (jar3)"; python3 jar3.py 2>&1 | tail -5
pkill -f 'spyc-jar --socket' || true; rm -f /tmp/spyc/jars/*.sock
echo "== R15a (probe)"; python3 r15a.py 2>&1 | tail -2
pkill -f 'spyc-jar --socket' || true; rm -f /tmp/spyc/jars/*.sock; rm -rf $H/home3/.local/state
for mode in jars nojars; do
  rm -rf $H/home4/.local/state
  echo "== fidelity ($mode)"; python3 fidelity.py $mode 2>&1 | tail -3
  pkill -f 'spyc-jar --socket' || true; pkill -f 'less long.txt' || true; rm -f /tmp/spyc/jars/*.sock
done
for mode in snap portable refused nosnap; do
  rm -rf $H/home6/.local/state
  echo "== snapshots ($mode)"; python3 snap.py $mode 2>&1 | tail -2
  pkill -f 'spyc-jar --socket' || true; rm -f /tmp/spyc/jars/*.sock
done
echo "== shape 2: spyc in an outer jar"; python3 shape2.py 2>&1 | tail -10
pkill -f 'spyc-jar --socket' || true
# Before the freeze: decision G, a lost jar's label, R18, and scope claims
# moving with a project.
for mode in free held; do
  echo "== decision G ($mode)"; python3 g.py $mode 2>&1 | tail -6
  pkill -f 'spyc-jar --socket' || true; pkill -f resume-handover || true
done
echo "== kill -9 of a jar"; python3 passing.py jar 2>&1 | tail -2
pkill -f 'spyc-jar --socket' || true
echo "== R18 and the orphan notice"; python3 r18.py 2>&1 | grep -E "notice|names the"
pkill -f 'spyc-jar --socket' || true
mkdir -p $H/q11_other $H/q11_b
echo "== scope claims move"; python3 scopes.py 2>&1 | tail -4
pkill -f 'spyc-jar --socket' || true
for mode in direct shell; do
  echo "== attention after a move ($mode)"; python3 attn.py $mode 2>&1 | tail -3
  pkill -f 'spyc-jar --socket' || true
done
