#!/bin/sh
pids=""; b=0; while [ $b -lt 14 ]; do yes > /dev/null & pids="$pids $!"; b=$((b+1)); done
DIAG_ROUNDS=4 DIAG_PAR=16 DIAG_OUT=docs/drafts/review-3.0/evidence/B/diag-mac.txt ~/.cache/spyc/target/3.0-b-98dd1d4f/debug/build/spyc/95728e2278fd4d50/out/review_b_probes-95728e2278fd4d50 --ignored --nocapture --test-threads=1 upgrade_count_diag 2>&1 | grep -E "DIAG|panicked|result"
kill $pids; wait $pids 2>/dev/null
