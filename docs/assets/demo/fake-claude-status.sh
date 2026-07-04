#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────
# Scripted stand-in that DRIVES THE AGENT-AWARENESS DOTS for the VHS demo
# (docs/assets/demo/agents.tape). It reports working → blocked → done via
# `spyc --report-status`, which frames to the SPYC_MCP_SOCK + SPYC_PANE_ID the
# pane already carries, so the tab's activity dot animates and the transition
# into `blocked` rings the visual bell (the spice-heat border pulse).
#
# TWO INSTANCES, ONE POINT. The demo runs two `claude` tabs so "which agent
# needs me" actually means something. A per-launch counter (/tmp/spyc-demo-
# agent-n, reset by the tape) splits behaviour:
#   • instance 1 — the busy worker: stays `working` for the whole clip (loops,
#     re-reporting so the dot keeps pulsing ●). It never needs you.
#   • instance 2 — the one that needs you: works briefly, then `blocked` (red ■
#     + visual bell). It waits on stdin; the tape focuses this tab and presses
#     Enter, which clears spyc's latch AND resumes the script → `done` ■.
#
# Fixed sleeps keep `vhs` reproducible. Every reported state is a real report.
# ─────────────────────────────────────────────────────────────────────────────

report() { command spyc --report-status "$1" >/dev/null 2>&1; }
mag()  { printf '\033[38;5;176m%s\033[0m' "$1"; }   # claude-ish mauve
bold() { printf '\033[1m%s\033[0m' "$1"; }
dim()  { printf '\033[2m%s\033[0m' "$1"; }

# Per-launch counter → instance 1 (worker) vs instance 2+ (blocker).
CNT=/tmp/spyc-demo-agent-n
n=$(( $(cat "$CNT" 2>/dev/null || echo 0) + 1 ))
echo "$n" > "$CNT"

clear
printf '\n '; mag '✻'; printf ' '; bold 'Claude Code'; printf '   '; dim 'spyc MCP connected'; printf '\n\n'

# Read the user's prompt (typed by VHS).
printf ' '; mag '>'; printf ' '
read -r _q
printf '\n'

if [ "$n" = "1" ]; then
  # ── The busy worker: works for the whole clip, never blocks. Keeps the dot
  #    pulsing ● by emitting progress and re-reporting `working`. ─────────────
  report working
  i=1
  while [ "$i" -le 24 ]; do
    sleep 1.4
    printf '   '; dim "scanning $i/24 — src/…"; printf '\n'
    report working
    i=$(( i + 1 ))
  done
else
  # ── The one that needs you: works briefly, then BLOCKS on a decision. The
  #    tab dot settles to a red ■ and the border rings the visual bell. The tape
  #    focuses this tab and presses Enter — clearing the latch AND resuming us.
  report working
  sleep 0.7
  printf ' '; mag '⏺'; printf ' '; dim 'reading the call sites, planning the rename…'; printf '\n'
  sleep 1.6
  report blocked
  printf '\n '; bold 'Apply this rename across all 4 call sites?'; printf '  [y/N] '
  read -r _ack          # the Enter sent by agents.tape (also clears spyc's latch)
  printf 'y\n'
  report working
  sleep 0.5
  printf '   '; dim 'applying across 4 files…'; printf '\n'
  sleep 1.4
  report done
  printf ' '; mag '⏺'; printf ' '; bold 'Done'; printf ' — 4 call sites updated, tests green.\n'
fi

printf '\n '; mag '>'; printf ' '
# Stay alive; spyc intercepts the meta chords sent from the tape.
while IFS= read -r _; do :; done
