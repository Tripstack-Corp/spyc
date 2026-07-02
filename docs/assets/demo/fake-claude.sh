#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────
# Scripted stand-in for `claude` used ONLY to record the VHS demo GIF.
#
#   SPYC_PANE_CMD="$(pwd)/docs/assets/demo/fake-claude.sh" spyc
#
# Why a stand-in: a real agent's timing and wording vary run-to-run, which makes
# the .tape non-reproducible. This depicts a real, supported interaction — the
# agent reads the user's picks over the spyc MCP socket and answers with a path
# — but with fixed timing so `vhs docs/assets/demo/spyc.tape` is deterministic.
#
# Kept deliberately SHORT so the whole exchange fits the split pane and the
# closing path (the `gf` target) stays on screen. Every path is real.
# ─────────────────────────────────────────────────────────────────────────────

dim()  { printf '\033[2m%s\033[0m' "$1"; }
bold() { printf '\033[1m%s\033[0m' "$1"; }
mag()  { printf '\033[38;5;176m%s\033[0m' "$1"; }   # claude-ish mauve
gold() { printf '\033[38;5;179m%s\033[0m' "$1"; }   # tool-call gold

clear
printf '\n '; mag '✻'; printf ' '; bold 'Claude Code'; printf '   '; dim 'spyc MCP connected'; printf '\n\n'

# Read the user's question (typed by VHS).
printf ' '; mag '>'; printf ' '
read -r _question
printf '\n'

# MCP tool call: read the user's picks (the three docs they selected).
sleep 0.6
printf ' '; gold '⏺'; printf ' '; bold 'get_spyc_context'; printf ' '; dim '(spyc)'; printf '\n'
sleep 0.5
printf '   '; dim '⎿  picks: ARCHITECTURE.md, AGENTS.md, CLAUDE.md  ·  branch: main'; printf '\n\n'
sleep 0.7

# Short answer; the LAST path is the gf target.
printf ' '; mag '⏺'; printf ' '
cat <<'RESPONSE'
Those three describe spyc's MVU design — a Model/Runtime/ViewState split,
one update entry, and effects returned as data. The loop that ties them
together is the run loop in src/app/run.rs.
RESPONSE
printf '\n'

printf ' '; mag '>'; printf ' '
# Stay alive; spyc intercepts the meta chords sent from here so nothing
# further reaches this process.
while IFS= read -r _; do :; done
