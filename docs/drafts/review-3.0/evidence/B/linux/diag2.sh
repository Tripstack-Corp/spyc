#!/bin/sh
cd /tmp; export HOME=/tmp TMPDIR=/tmp
P=/home/rev/target-b/debug/build/spyc/5582ca34f1aeb829/out/review_b_probes-5582ca34f1aeb829
JP=/home/rev/target-b/debug/build/spyc/8f2105297c64a06c/out/jar_process-8f2105297c64a06c
stress-ng --cpu 7 --io 2 --vm 2 --vm-bytes 512M --temp-path /tmp --timeout 3600 > /dev/null 2>&1 &
SP=$!
( i=0; while [ $i -lt 6 ]; do $JP --test-threads=48 > /out/diag2-suite-$i.log 2>&1; echo "suite $i exit $?" >> /out/diag2-suite.txt; i=$((i+1)); done ) &
SU=$!
sleep 3
k=0
while kill -0 $SU 2>/dev/null; do
  DIAG_ROUNDS=4 DIAG_PAR=16 DIAG_OUT=/out/diag-linux2.txt $P --ignored --nocapture --test-threads=1 upgrade_count_diag 2>&1 | grep -E "DIAG|panicked"
  k=$((k+1))
done
cat /proc/loadavg
kill $SP; wait
