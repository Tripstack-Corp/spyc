#!/usr/bin/env bash
# `make vendor-ghostty` inside a Linux container, for a host that isn't Linux.
# The archives must be built on Linux (see scripts/vendor-ghostty.sh); this
# gives a Mac the same bytes CI builds. It writes into this checkout's
# crates/spyc-vt-sys/vendor/, exactly as the native recipe does.
set -euo pipefail

engine=$(command -v podman || command -v docker) || {
    echo "needs podman or docker"
    exit 1
}
zig_version=$(cat crates/spyc-vt-sys/REQUIRED_ZIG)

# Passed as an argument, not on stdin: apt-get reads stdin, and fed through
# `bash -s` it swallowed the rest of this script, which then exited 0 having
# built nothing.
in_container=$(cat <<'IN_CONTAINER'
set -euo pipefail
apt-get update -qq >/dev/null
apt-get install -y -qq git make perl ca-certificates curl xz-utils python3 >/dev/null
# ziglang.org's build of the pinned version, as CI's setup-zig installs, checked
# against the sha256 ziglang.org publishes for it.
curl -fsSL https://ziglang.org/download/index.json -o /tmp/index.json
read -r url sum < <(python3 -c '
import json, os, platform
d = json.load(open("/tmp/index.json"))[os.environ["ZIG_VERSION"]][platform.machine() + "-linux"]
print(d["tarball"], d["shasum"])')
curl -fsSL "$url" -o /tmp/zig.tar.xz
echo "$sum  /tmp/zig.tar.xz" | sha256sum -c -
mkdir -p /opt/zig && tar -xJf /tmp/zig.tar.xz -C /opt/zig --strip-components=1
export PATH="/opt/zig:$PATH"
make vendor-ghostty
IN_CONTAINER
)

"$engine" run --rm -v "$(pwd):/src" -w /src -e ZIG_VERSION="$zig_version" \
    docker.io/library/ubuntu:24.04 bash -c "$in_container"
