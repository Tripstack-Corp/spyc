# Runs probe_linux.py against the x86_64 glibc jars, as uid 1000, ROUNDS times per commit.
set -u
uname -m; ldd --version | head -1; python3 --version
ROUNDS=${ROUNDS:-3}
for n in pin main; do
  for r in $(seq 1 $ROUNDS); do
    S=/out/$n-r$r; rm -rf $S; mkdir -p $S; cp /p3/probe_lib.py $S/; chown -R 1000:1000 $S
    echo "===== $n round $r ($(cat /b/$n/main.rs | grep -o 'main_built("[0-9a-f]*' | cut -d'"' -f2))"
    /b/$n/target/x86_64-unknown-linux-gnu/debug/spyc-jar --build-info 2>&1 | head -2 || true
    setpriv --reuid=1000 --regid=1000 --clear-groups env -i PATH=/usr/bin:/bin UA_S=$S \
      UA_JAR=/b/$n/target/x86_64-unknown-linux-gnu/debug/spyc-jar \
      UA_DIFFER=/b/$n/differ/x86_64-unknown-linux-gnu/debug/spyc-jar \
      timeout 1500 python3 -I /p3/probe_linux.py > $S/probe_linux.log 2>&1
    echo "exit $?"
    grep -E 'SigIgn|failed exec|JAR DIED|jar alive|^try|upgrade [12]|echo' $S/probe_linux.log
  done
done
echo "== leftover processes"; ps -eo pid,ppid,user,stat,args | grep -v ' ps ' 
