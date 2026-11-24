#!/bin/sh
# Unit B Linux load runner. ITER THREADS TAG
set -u
T=/home/rev/.cache/spyc/target/spyc-76690fe5/debug/build
JP=$T/spyc/8f2105297c64a06c/out/jar_process-8f2105297c64a06c
UT=$T/spyc-jar/cb7448dbbfb02dfe/out/spyc_jar-cb7448dbbfb02dfe
ITER=$1; THR=$2; TAG=$3
O=/out
cd /tmp
uname -a > $O/$TAG-env.txt; nproc >> $O/$TAG-env.txt; free -m >> $O/$TAG-env.txt
stress-ng --cpu 7 --io 2 --vm 2 --vm-bytes 512M --temp-path /tmp --timeout 7200 > $O/$TAG-stress.txt 2>&1 &
SP=$!
sleep 5
ps -o pid,args | grep -c stress-ng >> $O/$TAG-env.txt
i=1
while [ $i -le $ITER ]; do
  s=$(date +%s)
  RUST_BACKTRACE=1 $JP --test-threads=$THR > $O/$TAG-jp-$i.log 2>&1; jp=$?
  RUST_BACKTRACE=1 $UT --test-threads=$THR > $O/$TAG-ut-$i.log 2>&1; ut=$?
  echo "$TAG iter $i threads $THR jar_process=$jp unit=$ut secs=$(( $(date +%s) - s )) load=$(cut -d' ' -f1-3 /proc/loadavg) stressng=$(pgrep -c stress-ng) sleep300=$(pgrep -c -f 'sleep 300')" >> $O/summary
  i=$((i+1))
done
kill $SP; wait
echo done >> $O/summary
