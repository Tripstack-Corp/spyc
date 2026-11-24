#!/bin/sh
set -u
cd /home/rev
if [ ! -d spyc-b ]; then cp -a spyc spyc-b; fi
if [ ! -d target-b ]; then cp -a .cache/spyc/target/spyc-76690fe5 target-b; fi
cp /scripts-b/review_b_probes.rs spyc-b/tests/review_b_probes.rs
cd spyc-b && git rev-parse HEAD
export CARGO_TARGET_DIR=/home/rev/target-b CARGO_BUILD_JOBS=4
cargo test --locked --no-run --test review_b_probes --test jar_process 2>&1 | tail -5
echo build-exit=$?
