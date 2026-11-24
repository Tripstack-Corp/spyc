#!/bin/sh
cd /tmp; export HOME=/tmp TMPDIR=/tmp
P=/home/rev/target-b/debug/build/spyc/5582ca34f1aeb829/out/review_b_probes-5582ca34f1aeb829
stress-ng --cpu 7 --io 2 --vm 2 --vm-bytes 512M --temp-path /tmp --timeout 3600 > /dev/null 2>&1 &
SP=$!
sleep 3
for k in 1 2 3; do
DIAG_ROUNDS=6 DIAG_PAR=16 DIAG_OUT=/out/diag-linux.txt $P --ignored --nocapture --test-threads=1 upgrade_count_diag 2>&1 | grep -E "DIAG|panicked|result"
done
cat /proc/loadavg
kill $SP; wait
