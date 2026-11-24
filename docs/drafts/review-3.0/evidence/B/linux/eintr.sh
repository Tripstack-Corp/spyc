#!/bin/sh
cd /tmp; export HOME=/tmp TMPDIR=/tmp
P=/home/rev/target-b/debug/build/spyc/5582ca34f1aeb829/out/review_b_probes-5582ca34f1aeb829
$P --ignored --nocapture --test-threads=1 eintr_ 2>&1 | grep -E "EINTR|panicked|result"
