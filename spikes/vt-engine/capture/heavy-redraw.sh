#!/usr/bin/env bash
# Deterministic stand-in for an agent CLI's redraw pattern, which is what #34
# describes: a DECSTBM scroll region, spinner frames rewritten in place with
# CR, a progress bar repainted by absolute cursor positioning, DECSC/DECRC
# around status writes, and enough content to push lines off the top.
#
# Synthetic ON PURPOSE. A captured agent session is not reproducible (it depends
# on model output, terminal size, and whatever prompt state the day had), and
# the question here is about the mechanism, not one session's bytes.
set -u
printf '\033[2J\033[H'
printf '\033[3;22r'          # scroll region: rows 3..22, header/footer fixed
printf '\033[1;1Hheader pinned\n'
SPIN='|/-\'
for i in $(seq 1 120); do
  # content line inside the region (scrolls off the region top)
  printf '\033[%d;1Hstep %03d: doing work with some text on the line\n' $(( (i % 20) + 3 )) "$i"
  # spinner rewritten in place with CR, no newline
  printf '\033[23;1H%s working\r' "${SPIN:$((i % 4)):1}"
  # progress bar repainted by absolute positioning inside DECSC/DECRC
  printf '\0337\033[24;1H['
  filled=$(( i / 4 ))
  for _ in $(seq 1 "$filled"); do printf '#'; done
  for _ in $(seq 1 $(( 30 - filled ))); do printf ' '; done
  printf ']\0338'
  # occasional SGR churn
  if (( i % 7 == 0 )); then printf '\033[1;33mwarn\033[0m '; fi
  if (( i % 11 == 0 )); then printf '\033[38;2;120;200;255mrgb\033[0m '; fi
done
printf '\033[r'              # release the scroll region
printf '\033[24;1Hdone\n'
