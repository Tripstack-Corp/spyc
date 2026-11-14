#!/usr/bin/env bash
# One timing session. usage: session.sh TARGET BINDIR DATAROOT SPIKEDIR RAW RUNS REPS
#   BINDIR holds A/{spyc,bench} and B/{spyc,bench}; DATAROOT from mkdata.py.
set -euo pipefail
T=$1 BIN=$2 D=$3 SPIKE=$4 RAW=$5 RUNS=$6 REPS=$7
H=$(cd "$(dirname "$0")" && pwd)
echo "=== session $T $(date)  uname: $(uname -sm)"
( cd "$SPIKE" && python3 "$H/timeit.py" --name engine --mode bench --runs "$RUNS" --warmup 2 --out "$RAW" \
    "$BIN/A/bench" "$BIN/B/bench" "$REPS" )
python3 "$H/timeit.py" --name bigdir --mode spycstart --runs "$RUNS" --warmup 2 --out "$RAW" \
    "$BIN/A/spyc" "$BIN/B/spyc" "$D/bigdir"
M=$D/merge
python3 "$H/timeit.py" --name merge --mode wall --runs "$RUNS" --warmup 2 --out "$RAW" \
    --prep "cp $M/ours $M/cur" "$BIN/A/spyc" "$BIN/B/spyc" --merge-driver "$M/base" "$M/cur" "$M/theirs"
echo "=== end $(date)"
