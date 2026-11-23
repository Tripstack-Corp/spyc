#!/usr/bin/env bash
# Hold each shipped binary to its size budget (#933).
#
#   scripts/binary-size.sh NAME=PATH...
#
# NAME is a line in the budgets file (scripts/binary-size-budgets, or
# $SPYC_SIZE_BUDGETS), PATH the binary. Prints each one's size, its budget and
# the room left, and fails when one is over its budget, has none, or isn't
# there. On CI the same table goes to the job's summary.
set -euo pipefail

budgets="${SPYC_SIZE_BUDGETS:-$(dirname "$0")/binary-size-budgets}"
[ -f "$budgets" ] || { echo "no budgets file at $budgets"; exit 1; }
[ "$#" -gt 0 ] || { echo "usage: $0 NAME=PATH..."; exit 2; }

mib() { awk -v b="$1" 'BEGIN { printf "%.2f MiB", b / 1048576 }'; }

failed=0
table="| Binary | Size | Budget | Room |
|---|---:|---:|---:|"
for pair in "$@"; do
    name="${pair%%=*}"
    path="${pair#*=}"
    budget=$(awk -v n="$name" '$1 == n { print $2 }' "$budgets")
    if [ -z "$budget" ]; then
        echo "$name: no budget in $budgets"
        failed=1
        continue
    fi
    if [ ! -f "$path" ]; then
        echo "$name: no binary at $path"
        failed=1
        continue
    fi
    size=$(wc -c < "$path" | tr -d ' ')
    room=$((budget - size))
    if [ "$room" -lt 0 ]; then
        verdict="OVER by $(mib $((-room)))"
        failed=1
    else
        verdict="$(mib "$room") left"
    fi
    printf '%-26s %10s of %10s  %s\n' "$name" "$(mib "$size")" "$(mib "$budget")" "$verdict"
    table="$table
| \`$name\` | $(mib "$size") | $(mib "$budget") | $verdict |"
done

if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
    printf '### Release binary sizes\n\n%s\n' "$table" >> "$GITHUB_STEP_SUMMARY"
fi
if [ "$failed" != 0 ]; then
    echo "over a budget, or unchecked: see BUILD.md, \"Release binary size\", to raise one"
    exit 1
fi
