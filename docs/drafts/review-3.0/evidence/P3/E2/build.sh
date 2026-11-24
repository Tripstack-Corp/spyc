set -eu
cd /home/rev
[ -d spyc-p3 ] || cp -a spyc spyc-p3
[ -d target-p3 ] || cp -a .cache/spyc/target/spyc-76690fe5 target-p3
cd spyc-p3
git fetch -q origin '+refs/remotes/origin/main:refs/remotes/host-origin/main'
git checkout -q --detach 52ce15b75f343ba73f2d7aa94f5bca3c7f0d79fe
git log -1 --oneline
export CARGO_TARGET_DIR=/home/rev/target-p3 CARGO_BUILD_JOBS=4
cargo build --locked --bin spyc --bin spyc-jar 2>&1 | tail -5
ls -la /home/rev/target-p3/debug/spyc /home/rev/target-p3/debug/spyc-jar
/home/rev/target-p3/debug/spyc --version
