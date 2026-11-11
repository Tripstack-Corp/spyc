#!/bin/sh
# Every 5 s: can this process still read ~/Documents, the pasteboard and a
# Keychain item? Records pass or fail only, never contents.
log="$1"
i=0
while [ $i -lt 24 ]; do
  d=$(ls "$HOME/Documents" >/dev/null 2>&1 && echo ok || echo FAIL)
  p=$(pbpaste >/dev/null 2>&1 && echo ok || echo FAIL)
  k=$(security find-generic-password -s spyc-jar-probe -w >/dev/null 2>&1 && echo ok || echo FAIL)
  term=$(ps -axco comm | grep -qx Terminal && echo up || echo gone)
  echo "$(date +%H:%M:%S) terminal=$term documents=$d pasteboard=$p keychain=$k" >> "$log"
  if [ $i -eq 6 ]; then
    osascript -e 'display notification "spyc jar probe: notification after Terminal quit" with title "spyc jar spike"' >/dev/null 2>&1 && echo "$(date +%H:%M:%S) notification=ok" >> "$log" || echo "$(date +%H:%M:%S) notification=FAIL" >> "$log"
  fi
  i=$((i+1)); sleep 5
done
