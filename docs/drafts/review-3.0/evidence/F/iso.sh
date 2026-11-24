#!/bin/sh
# Run a command with every spyc-relevant variable pointed into the unit F scratch dir,
# in its own session/process group; the group is killed when the command ends.
S=docs/drafts/review-3.0/evidence/F/iso
mkdir -p "$S/run" "$S/state" "$S/config" "$S/data" "$S/cache" "$S/tmp" "$S/home"
chmod 700 "$S/run"
exec env -u SPYC_CONTEXT -u SPYC_JAR_SOCKET -u SPYC_MCP_SOCK -u SPYC_PANE_ID -u SSH_AUTH_SOCK \
  XDG_RUNTIME_DIR="$S/run" XDG_STATE_HOME="$S/state" XDG_CONFIG_HOME="$S/config" \
  XDG_DATA_HOME="$S/data" XDG_CACHE_HOME="$S/cache" TMPDIR="$S/tmp" HOME="$S/home" \
  CARGO_HOME=~/.cargo RUSTUP_HOME=~/.rustup \
  PATH="~/.cargo/bin:/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin" \
  python3 -c 'import os,sys,signal,subprocess
os.setsid()
p=subprocess.Popen(sys.argv[1:])
rc=p.wait()
signal.signal(signal.SIGTERM, signal.SIG_IGN)
try: os.killpg(0, signal.SIGTERM)
except Exception: pass
sys.exit(rc)' "$@"
