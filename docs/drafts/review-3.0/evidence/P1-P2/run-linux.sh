# The container runs behind the P1 results (run from the Mac; each in its own container).
# Build at 52ce15b7 (container p12-build), into /home/rev/target-p12 in the review volume:
set -e
cd /home/rev
if [ ! -d spyc-p12 ]; then git clone -q --local /home/rev/spyc spyc-p12; fi
cd spyc-p12
git fetch -q /host-repo refs/remotes/origin/main:refs/remotes/host/main
git checkout -q --detach 52ce15b75f343ba73f2d7aa94f5bca3c7f0d79fe
git log --oneline -1
if [ ! -d /home/rev/target-p12 ]; then cp -a /home/rev/.cache/spyc/target/spyc-76690fe5 /home/rev/target-p12; fi
export CARGO_TARGET_DIR=/home/rev/target-p12 CARGO_BUILD_JOBS=4
time cargo build --bins 2>&1 | tail -5
ls -la /home/rev/target-p12/debug/spyc /home/rev/target-p12/debug/spyc-jar
/home/rev/target-p12/debug/spyc --version

# 50 idle rounds, then 50 under stress-ng --cpu 14 --io 4 (container p12-run):
cd /tmp
B=/home/rev/target-p12/debug/spyc
D=/p12/linux
echo "== idle start $(date -u +%FT%TZ) load: $(cat /proc/loadavg)"
env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin P1_SEED=11 python3 -I /p12/drive_p1.py $B $D/idle50 50 4 idle 1 3
echo "== idle exit $? $(date -u +%FT%TZ) load: $(cat /proc/loadavg)"
stress-ng --cpu 14 --io 4 --timeout 1800s > $D/stress-ng.log 2>&1 &
SN=$!
sleep 10
echo "== stress start $(date -u +%FT%TZ) load: $(cat /proc/loadavg)"
env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin P1_SEED=22 python3 -I /p12/drive_p1.py $B $D/stress50 50 4 stress 1 3 restartR
echo "== stress exit $? $(date -u +%FT%TZ) load: $(cat /proc/loadavg)"
kill $SN; wait $SN
ps -A -o pid,ppid,args

# Controls (container p12-run2): 30 rounds of a bare SIGCONT, then 8 rounds of 12-13 s stops under stress:
cd /tmp
B=/home/rev/target-p12/debug/spyc
D=/p12/linux
echo "== cont-only start $(date -u +%FT%TZ) load: $(cat /proc/loadavg)"
env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin P1_SEED=44 P1_CONT_ONLY=1 python3 -I /p12/drive_p1.py $B $D/contonly30 30 4 cont-only
echo "== cont-only exit $? $(date -u +%FT%TZ)"
stress-ng --cpu 14 --io 4 --timeout 900s > $D/stress-ng-long.log 2>&1 &
SN=$!
sleep 10
echo "== long-stop start $(date -u +%FT%TZ) load: $(cat /proc/loadavg)"
env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin P1_SEED=55 python3 -I /p12/drive_p1.py $B $D/longstop8 8 4 long-stop-stress 12 13
echo "== long-stop exit $? $(date -u +%FT%TZ) load: $(cat /proc/loadavg)"
kill $SN; wait $SN
ps -A -o pid,ppid,args

# cgroup freeze (container p12-pause): the driver waits on ready_N/done_N while the Mac runs
#   podman pause p12-pause; sleep 2; podman unpause p12-pause   (host log: linux/pause10/host-pause.log)
cd /tmp; env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin P1_SEED=66 P1_EXTERNAL=/p12/linux/pause10/ctl python3 -I /p12/drive_p1.py /home/rev/target-p12/debug/spyc /p12/linux/pause10/run 10 4 podman-pause 2 2; echo "== pause exit $?"; ps -A -o pid,ppid,args
