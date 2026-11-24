# Runs probe_linux_quick.py against the x86_64 glibc jars, as uid 1000, ROUNDS times per commit.
set -u
uname -m; ldd --version | head -1
ROUNDS=${ROUNDS:-5}
for n in pin main; do
  ln -sfn /b/$n/target/x86_64-unknown-linux-gnu/debug/spyc-jar /tmp/p3real
  for r in $(seq 1 $ROUNDS); do
    S=/out/quick-$n-r$r; rm -rf $S; mkdir -p $S; cp /p3/probe_lib.py $S/; chown -R 1000:1000 $S
    echo "===== quick holder, $n round $r"
    setpriv --reuid=1000 --regid=1000 --clear-groups env -i PATH=/usr/bin:/bin UA_S=$S \
      UA_JAR=/b/$n/target/x86_64-unknown-linux-gnu/debug/spyc-jar \
      UA_DIFFER=/b/$n/differ/x86_64-unknown-linux-gnu/debug/spyc-jar \
      UA_QUICK=/p3/holder/quick_holder \
      timeout 1500 python3 -I /p3/probe_linux_quick.py > $S/probe_linux_quick.log 2>&1
    echo "exit $?"
    grep -E 'SIGPIPE ignored|failed exec|JAR DIED|jar alive|^try' $S/probe_linux_quick.log | cut -c1-260
  done
done
echo "== leftover processes"; ps -eo pid,ppid,user,stat,args | grep -v ' ps '
