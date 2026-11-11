#!/bin/bash
printf 'asking\033[c\n'
IFS= read -r -s -t 3 -d c first
printf "first-ok=[%q]\n" "$first"
IFS= read -r -s -t 60 -d X second
printf 'second=[%q]\n' "$second"
sleep 60
