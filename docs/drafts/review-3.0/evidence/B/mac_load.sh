#!/bin/sh
# Unit B macOS load runner: ITER BURNERS THREADS TAG
set -u
S=docs/drafts/review-3.0/evidence/B
JP=~/.cache/spyc/target/3.0-b-98dd1d4f/debug/build/spyc/d17db8b209b38b4e/out/jar_process-d17db8b209b38b4e
UT=~/.cache/spyc/target/3.0-b-98dd1d4f/debug/build/spyc-jar/79ec937233ebc073/out/spyc_jar-79ec937233ebc073
ITER=$1; BURN=$2; THR=$3; TAG=$4
mkdir -p $S/mac
for i in $(seq 1 $ITER); do
  pids=""
  b=0; while [ $b -lt $BURN ]; do yes > /dev/null & pids="$pids $!"; b=$((b+1)); done
  start=$(date +%s)
  env -u SPYC_JAR_SOCKET -u SPYC_MCP_SOCK -u SPYC_CONTEXT -u SPYC_PANE_ID -u SSH_AUTH_SOCK \
    XDG_RUNTIME_DIR=$S/xdg/run XDG_STATE_HOME=$S/xdg/state XDG_CONFIG_HOME=$S/xdg/config \
    XDG_DATA_HOME=$S/xdg/data XDG_CACHE_HOME=$S/xdg/cache TMPDIR=$S/tmp HOME=$S/home RUST_BACKTRACE=1 \
    $JP --test-threads=$THR > $S/mac/$TAG-jp-$i.log 2>&1
  jp=$?
  env -u SPYC_JAR_SOCKET -u SPYC_MCP_SOCK -u SPYC_CONTEXT -u SPYC_PANE_ID -u SSH_AUTH_SOCK \
    XDG_RUNTIME_DIR=$S/xdg/run XDG_STATE_HOME=$S/xdg/state XDG_CONFIG_HOME=$S/xdg/config \
    XDG_DATA_HOME=$S/xdg/data XDG_CACHE_HOME=$S/xdg/cache TMPDIR=$S/tmp HOME=$S/home RUST_BACKTRACE=1 \
    $UT --test-threads=$THR > $S/mac/$TAG-ut-$i.log 2>&1
  ut=$?
  kill $pids 2>/dev/null
  wait $pids 2>/dev/null
  echo "$TAG iter $i burners $BURN threads $THR jar_process=$jp unit=$ut secs=$(( $(date +%s) - start ))" >> $S/mac/summary
done
