#!/usr/bin/env bash
set -euo pipefail
B=${BENCH:?}
W=${SPYC_B:?}
O=${OUT:?}
mkdir -p $B/run-macos/A $B/run-macos/B
for v in A B; do cp $B/bin/$v/spyc-macos $B/run-macos/$v/spyc; cp $B/bin/$v/bench-macos $B/run-macos/$v/bench; done
{
echo "--- $(date)  $(uname -a)"
echo "--- pgrep -fl 'zig|cargo|rustc|clang|cc1plus|ld64|podman|vfkit' before timing:"
pgrep -fl 'zig|cargo|rustc|clang|cc1plus|ld64|podman|vfkit' | cut -c1-160 || echo "(none)"
echo "--- podman ps:"; podman ps 2>&1 || true
echo "--- binaries:"; shasum -a 256 $B/run-macos/*/*
} > $O/precheck-macos-arm64.txt 2>&1
cat $O/precheck-macos-arm64.txt
rm -f $O/raw-macos-arm64.tsv
bash $B/session.sh macos-arm64 $B/run-macos $B/data $W/spikes/vt-engine $O/raw-macos-arm64.tsv "${1:-12}" "${2:-800}" | tee $O/summary-macos-arm64.txt
echo "--- pgrep after:"; pgrep -fl 'zig|cargo|rustc|clang|cc1plus|ld64' | cut -c1-160 || echo "(none)"
