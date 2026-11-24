set -e
apt-get update -qq >/dev/null && apt-get install -y -qq gcc-x86-64-linux-gnu libc6-dev-amd64-cross >/dev/null
rustup target add x86_64-unknown-linux-gnu
export CARGO_BUILD_JOBS=4 CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-linux-gnu-gcc
for n in pin main; do
  cd /b/$n
  cp Cargo.lock Cargo.lock.repo
  cargo build --target x86_64-unknown-linux-gnu --bin spyc-jar --target-dir /b/$n/target
  cargo build --target x86_64-unknown-linux-gnu --bin spyc-jar --features differ --target-dir /b/$n/differ
  echo "== $n lock entries used:"; grep -A1 -E '^name = "(rustix|anyhow|libc|linux-raw-sys|bitflags|errno)"' Cargo.lock | grep version | tr '\n' ' '; echo
  echo "== $n same in repo lock:"; for p in rustix anyhow linux-raw-sys bitflags; do grep -A1 "^name = \"$p\"" Cargo.lock.repo | grep version | tr '\n' ' '; done; echo
  file target/x86_64-unknown-linux-gnu/debug/spyc-jar differ/x86_64-unknown-linux-gnu/debug/spyc-jar 2>/dev/null || ls -la target/x86_64-unknown-linux-gnu/debug/spyc-jar differ/x86_64-unknown-linux-gnu/debug/spyc-jar
done
