#!/usr/bin/env zsh
set -u
HERE=${0:A:h}
STAGE=${SPYC_DEMO_STAGE:-/private/tmp/spyc-demo}
R="$STAGE/aurora-docs"
CAST="$STAGE/out/fold.cast"
mkdir -p "$STAGE/out"
rm -rf "$R" && mkdir -p "$R" && cp -R "$HERE/aurora-docs/." "$R"
export TUI_TEST_SESSION=spycfold
TT=tui-test
say(){ print -r -- "  ‣ $*" }
k(){ $TT key press "$1" >/dev/null; sleep "${2:-0.5}" }
z(){ $TT type "$1" >/dev/null; sleep 0.35; $TT type "$2" >/dev/null; sleep "${3:-1.8}" }   # chord, keyed slowly
xp(){ $TT expect text "$1" --no-strict --timeout "${2:-15000}" >/dev/null || { print -u2 "  ✗ FAIL: $1"; return 1; } }
# assert the NUMBER of fold markers -- an assertion that actually bites
folds(){ $TT text 2>/dev/null | grep -c "▸" }
want_folds(){ local want=$1 i n
  for i in {1..30}; do n=$(folds); [[ "$n" == "$want" ]] && { print -r -- "  ✓ $want folds"; return 0 }; sleep 0.4; done
  print -u2 "  ✗ FAIL: wanted $want folds, saw $(folds)"; return 1 }

rm -f "$CAST"; $TT close --all >/dev/null 2>&1
$TT open --backend ghostty --cols 170 --rows 44 --cwd "$R" --shell zsh --env "COLORTERM=truecolor" >/dev/null
$TT record start "$CAST" >/dev/null

say "open the handbook in the pager"
$TT submit "spyc" >/dev/null
xp "HANDBOOK.md" || exit 1
sleep 2
$TT type "/HAND" >/dev/null; sleep 0.5; k Enter 0.6; k Escape 0.5
k Enter 2.6
k g 0.1; k g 1.0
xp "audience: operators" || exit 1
sleep 2.5

say "]] then za — fold just the section you are reading"
$TT type "]]" >/dev/null; sleep 1.4
$TT type "za" >/dev/null; sleep 1.0
want_folds 1 || exit 1
sleep 3
$TT type "za" >/dev/null; sleep 1.0
want_folds 0 || exit 1
sleep 1.5
k g 0.1; k g 1.2

say "zM — the whole doc becomes a table of contents"
z z M 1.0
want_folds 10 || exit 1
sleep 4.5

say "]] — walk the table of contents"
for i in 1 2 3; do $TT type "]]" >/dev/null; sleep 1.0; done
sleep 1

say "zR — expand everything again"
z z R 1.0
want_folds 0 || exit 1
sleep 2
k g 0.1; k g 1.5

say "m — flip to raw markdown source and back"
$TT type "m" >/dev/null; sleep 3
$TT type "m" >/dev/null; sleep 2.5

$TT record stop >/dev/null
say "cast -> $CAST"
$TT close --all >/dev/null 2>&1
