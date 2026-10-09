#!/usr/bin/env zsh
set -u
HERE=${0:A:h}
STAGE=${SPYC_DEMO_STAGE:-/private/tmp/spyc-demo}
R="$STAGE/aurora-docs"
CAST="$STAGE/out/take.cast"
# Disposable copy of the fixture that ships beside this script: a beat edits
# RELEASE-NOTES.md, and the repo copy stays pristine.
mkdir -p "$STAGE/out"
rm -rf "$R" && mkdir -p "$R" && cp -R "$HERE/aurora-docs/." "$R"
export TUI_TEST_SESSION=spycdemo
TT=tui-test

say()  { print -r -- "  ‣ $*" }
k()    { $TT key press "$1" >/dev/null; sleep "${2:-0.5}" }
chord(){ $TT key press Ctrl+a >/dev/null; $TT type "$1" >/dev/null; sleep "${2:-1.1}" }
slowtype(){ local s=$1 d=${2:-0.05} i; for (( i=1; i<=${#s}; i++ )); do $TT type "${s[i]}" >/dev/null; sleep $d; done }
xp()   { $TT expect text "$1" --no-strict --timeout "${2:-15000}" >/dev/null || { print -u2 "  ✗ FAIL expect: $1"; return 1; } }
# assert text in the RIGHT column only (cols 101-200) -- proves the PREVIEW re-rendered,
# not vim's copy of the same string
xp_right(){ local pat=$1 i
  for i in {1..40}; do
    $TT text 2>/dev/null | cut -c101-200 | grep -q -- "$pat" && { print -r -- "  ✓ right column has: $pat"; return 0 }
    sleep 0.5
  done
  print -u2 "  ✗ FAIL right column: $pat"; return 1 }

cp "$HERE/aurora-docs/RELEASE-NOTES.md" "$R/RELEASE-NOTES.md"
rm -f "$CAST"
$TT close --all >/dev/null 2>&1

say "open ghostty session, truecolor"
$TT open --backend ghostty --cols 200 --rows 50 --cwd "$R" --shell zsh \
   --env "SPYC_PANE_CMD=vim RELEASE-NOTES.md" \
   --env "COLORTERM=truecolor" >/dev/null
$TT record start "$CAST" >/dev/null

say "beat 1 — launch"
$TT submit "spyc" >/dev/null
xp "CONTRIBUTING.md" || exit 1
sleep 2.2

say "beat 2 — browse"
for i in 1 2 3; do k j 0.38; done
sleep 0.6; k G 1.5

say "beat 3 — full-height rendered markdown"
k Ctrl+s 0.15; $TT type "|" >/dev/null
xp "parked_producer" || exit 1
sleep 2.8

say "beat 4 — scroll the render"
chord b 0.9
k g 0.1; k g 1.0
sleep 1.2
for i in 1 2 3 4; do k Ctrl+d 1.0; done
sleep 1.0
k g 0.1; k g 1.3

say "beat 5 — vim in the bottom pane"
chord a 0.9
chord c 1.5
k Enter 1.0
k Enter 3.2
xp "1485B" || exit 1
sleep 1.2

say "beat 6 — edit + save"
k G 0.8
$TT type "o" >/dev/null; sleep 0.6
k Enter 0.3                    # leave a blank line: an alert must be its OWN block
slowtype '## Rollback'; k Enter 0.3
k Enter 0.3
slowtype '> [!CAUTION]'; k Enter 0.3
slowtype '> Roll back with `aurora rollback --to 2.9`.'
sleep 0.5
k Escape 0.9
slowtype ':w'; k Enter 0.3

say "beat 7 — preview re-renders; scroll it to the new block"
xp "written" || exit 1          # vim confirms the write
sleep 1.6
chord b 0.9                    # focus the preview
k G 1.6                        # jump to the end of the RENDERED doc
xp_right "CAUTION" || exit 1   # the payoff, proven in the right column
sleep 4

$TT record stop >/dev/null
say "cast -> $CAST"
$TT close --all >/dev/null 2>&1
cp "$HERE/aurora-docs/RELEASE-NOTES.md" "$R/RELEASE-NOTES.md"
