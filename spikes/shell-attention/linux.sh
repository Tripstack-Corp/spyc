#!/bin/sh
# Inside the container, as `probe`: `linux.sh build`, `linux.sh run SCOPE`,
# `linux.sh cost`. run.sh calls it; outputs land in /work, merged by run.sh.
set -eu
export CARGO_HOME=/work/cargo-home CARGO_TARGET_DIR=/work/target
export PATH=/usr/local/cargo/bin:$PATH
BIN=/work/target/release/shell-attention-probe
case "$1" in
  build)
    rm -rf /work/fixtures && cp -R /spike/fixtures /work/fixtures && rm -rf /work/fixtures/bin
    cargo build -q --release --manifest-path /spike/probe/Cargo.toml
    (cd /work/fixtures/goread && GOFLAGS=-buildvcs=false CGO_ENABLED=0 GOCACHE=/work/go-cache go build -o /work/fixtures/bin/goread .)
    ;;
  run)
    rm -rf /work/home && mkdir -p /work/home
    $BIN run --fix /work/fixtures --home /work/home --work /work --scope "$2" --out "/work/obs-$2.json"
    $BIN yama --home /work/home --scope "$2" --out "/work/access-$2.json"
    ;;
  cost)
    $BIN cost --home /work/home --out /work/cost.json
    ;;
esac
