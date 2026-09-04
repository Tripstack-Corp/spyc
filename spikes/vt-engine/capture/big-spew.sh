#!/usr/bin/env bash
# Throughput fixture: a large, mostly-plain build-log-shaped stream with the
# colour and CR-progress churn a real `cargo build` emits.
set -u
for i in $(seq 1 4000); do
  printf '\033[32m   Compiling\033[0m crate-%04d v0.1.%d (/some/reasonably/long/path/crate-%04d)\n' "$i" $((i%20)) "$i"
  if (( i % 25 == 0 )); then printf '    Building [=====>       ] %d/4000\r' "$i"; fi
  if (( i % 400 == 0 )); then printf '\033[1;33mwarning\033[0m: unused variable `x` at src/lib.rs:%d:%d\n' "$i" $((i%80)); fi
done
printf '\033[1;32m    Finished\033[0m release target(s)\n'
