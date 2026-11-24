set -x
B=/home/rev/.cache/spyc/target/spyc-76690fe5/debug/spyc
for m in pasteR:60 typing:90; do
  mode=${m%%:*}; lim=${m##*:}
  mkdir -p /out/$mode
  env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin python3 -I /unitc/drive_v7.py $B /out/$mode $mode $lim
  echo "== $mode exit $?"
done
ps -A -o pid,ppid,command
