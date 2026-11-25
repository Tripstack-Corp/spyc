#!/bin/sh
# Rerun the spike and rewrite its observation files: `./run.sh` (both),
# `./run.sh macos` or `./run.sh linux`, then `python3 -I analyze.py`.
#
# Linux runs in a container under the podman machine. Its `ptrace_scope` is
# the VM's, not the container's, so the run sets it to 0, 1 and 2 in turn and
# puts back what it found, on any exit. Every `podman machine` VM is
# disposable; nothing else in it should rely on `ptrace` while this runs.
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
what=${1:-all}

macos() {
  T=${TMPDIR:-/tmp}/shell-attention
  rm -rf "$T/home" && mkdir -p "$T/home"
  CARGO_TARGET_DIR="$T/target" cargo build -q --release --manifest-path "$HERE/probe/Cargo.toml"
  if command -v go >/dev/null; then
    (cd "$HERE/fixtures/goread" && CGO_ENABLED=0 go build -o "$HERE/fixtures/bin/goread" .)
  fi
  BIN="$T/target/release/shell-attention-probe"
  "$BIN" run --fix "$HERE/fixtures" --home "$T/home" --work "$T" --out "$HERE/observation-macos.json"
  "$BIN" yama --home "$T/home" --out "$HERE/access-macos.json"
  "$BIN" cost --home "$T/home" --out "$HERE/cost-macos.json"
}

linux() {
  podman build -q -t localhost/spyc-shell-attention -f "$HERE/Containerfile" "$HERE" >/dev/null
  podman rm -f sa-linux >/dev/null 2>&1 || true
  podman volume create --ignore sa-work >/dev/null
  podman run -d --name sa-linux --init -v "$HERE:/spike" -v sa-work:/work \
    localhost/spyc-shell-attention sleep infinity >/dev/null
  podman exec -u root sa-linux sh -c 'mkdir -p /run/sshd && chown probe /work'
  podman exec -u probe sa-linux sh /spike/linux.sh build
  orig=$(podman machine ssh cat /proc/sys/kernel/yama/ptrace_scope | tr -d '\r')
  trap 'podman machine ssh sudo sysctl -qw kernel.yama.ptrace_scope=$orig' EXIT
  for s in 0 1 2; do
    podman machine ssh sudo sysctl -qw kernel.yama.ptrace_scope=$s
    podman exec -u probe sa-linux sh /spike/linux.sh run $s
  done
  podman machine ssh sudo sysctl -qw kernel.yama.ptrace_scope="$orig"
  trap - EXIT
  podman exec -u probe sa-linux sh /spike/linux.sh cost
  for f in obs-0 obs-1 obs-2 access-0 access-1 access-2 cost; do
    podman cp "sa-linux:/work/$f.json" "$HERE/.$f.json"
  done
  python3 -I "$HERE/merge.py" "$HERE"
  rm -f "$HERE"/.obs-?.json "$HERE"/.access-?.json "$HERE/.cost.json"
  podman rm -f sa-linux >/dev/null
  podman volume rm sa-work >/dev/null
}

case "$what" in
  macos) macos ;;
  linux) linux ;;
  all) linux; macos ;;
esac
