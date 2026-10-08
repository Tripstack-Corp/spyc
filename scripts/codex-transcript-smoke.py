#!/usr/bin/env python3
"""Exercise recorded Codex command argv through the actual Ctrl-A v pager.

Controlled child and disposable HOME; recorded commands are displayed, never run.
No native Codex or hook trust is exercised.
"""
import argparse
import datetime
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import select
import shlex
import sys
import time
import tty
import uuid


def child():
    tty.setraw(0)
    print("\x1b[?1049h\x1b[2J\x1b[HSPYC_TRANSCRIPT_CHILD_READY", flush=True)
    while True:
        readable, _, _ = select.select([0], [], [], 0.1)
        if readable and not os.read(0, 1024):
            return 0


def host(args):
    source = Path(__file__).resolve()
    spec = importlib.util.spec_from_file_location("hook_ownership", source.with_name("codex-hook-ownership-smoke.py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)

    class TranscriptSmoke(module.OwnershipSmoke):
        def run(self):
            self.setup()
            binary = args.binary.resolve()
            fixture = source.parent.parent / "tests/fixtures/codex-shell-rollout.jsonl"
            record = json.loads(fixture.read_text())
            records = [{"type": "event_msg", "payload": {"type": "user_message", "message": "SPYC_TRANSCRIPT_PROSE"}}, record]
            if args.scenario == "duplicate":
                records.append(record)
            if args.scenario == "mixed":
                legacy = json.loads(json.dumps(record))
                legacy["payload"]["item"].update(id="legacy-command", command="printf legacy-compatible", aggregated_output="LEGACY_COMMAND_RESULT")
                malformed = json.loads(json.dumps(record))
                malformed["payload"]["item"].update(id="malformed-command", command=["echo", 42], aggregated_output="SPYC_INVALID_COMMAND_OUTPUT")
                records += [legacy, malformed]
            session = str(uuid.uuid4())
            now = datetime.datetime.now(datetime.timezone.utc).isoformat().replace("+00:00", "Z")
            meta = {"timestamp": now, "type": "session_meta", "payload": {"id": session, "timestamp": now, "cwd": str(self.project)}}
            directory = self.home / ".codex/sessions/2026/10/08"
            directory.mkdir(parents=True)
            rollout = directory / ("rollout-2026-10-08T00-00-00-" + session + ".jsonl")
            rollout.write_text("".join(json.dumps(value) + "\n" for value in [meta] + records))
            launcher = self.output / "codex"
            launcher.write_text("#!/bin/sh\nexec " + shlex.join([sys.executable, str(source), "--child"]) + "\n")
            launcher.chmod(0o700)
            command = shlex.join([str(launcher), "resume", session])
            manifest = json.loads((self.output / "manifest.json").read_text())
            manifest.update(coverage="actual host, recorded rollout and Ctrl-A v; no command execution or native trust", fixture_sha256=hashlib.sha256(fixture.read_bytes()).hexdigest(), rollout=str(rollout))
            (self.output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
            self.cli("run", "--backend", "ghostty", "--cols", "200", "--rows", "50", "--cwd", str(self.project),
                     "--env", "SHELL=/bin/sh", "--env", "PATH=" + str(binary.parent) + os.pathsep + os.environ["PATH"],
                     "--env", "SPYC_PANE_CMD=" + command, "--env", "HOME=" + str(self.home),
                     "--env", "XDG_STATE_HOME=" + str(self.output / "state"), "--env", "COLORTERM=truecolor",
                     "--env", "SPYC_MCP_SOCK=", "--env", "SPYC_PANE_ID=", str(binary), "--no-lua")
            self.started = True
            self.cli("record", "start", str(self.output / "session.cast"), "--format", "cast")
            self.wait("🌶️", 15000)
            self.cli("type", ":hooks off")
            self.key("Enter")
            self.key("Ctrl+a", "c")
            self.wait("pane command:", 5000)
            self.key("Enter")
            self.wait("pane cwd:", 5000)
            self.key("Enter")
            self.wait("SPYC_TRANSCRIPT_CHILD_READY", 10000)
            self.key("Ctrl+a", "v")
            self.wait("SPYC_TRANSCRIPT_PROSE", 10000)
            self.wait("codex --version", 8000)
            self.wait("WARNING: proceeding", 5000)
            shown = self.capture("commands-shown")
            assert shown.count("codex --version") == 1, shown
            assert "SPYC_TRANSCRIPT_PROSE" in shown and "SPYC_INVALID_COMMAND_OUTPUT" not in shown, shown
            if args.scenario == "mixed":
                assert "legacy-compatible" in shown and "LEGACY_COMMAND_RESULT" in shown, shown
            self.key("t")
            until = time.monotonic() + 8
            while True:
                hidden = self.capture("commands-hidden")
                if "SPYC_TRANSCRIPT_PROSE" in hidden and "codex --version" not in hidden and "WARNING: proceeding" not in hidden:
                    break
                assert time.monotonic() < until, "tool lines remained visible: " + hidden
                time.sleep(0.1)
            assert "SPYC_TRANSCRIPT_PROSE" in hidden, hidden
            assert "codex --version" not in hidden and "WARNING: proceeding" not in hidden, hidden
            assert "legacy-compatible" not in hidden and "SPYC_INVALID_COMMAND_OUTPUT" not in hidden, hidden
            self.key("t")
            self.wait("codex --version", 8000)
            with rollout.open("a") as stream:
                stream.write(json.dumps({"type": "event_msg", "payload": {"type": "user_message", "message": "SPYC_TRANSCRIPT_RELOADED"}}) + "\n")
            self.key("r")
            self.wait("SPYC_TRANSCRIPT_RELOADED", 8000)
            self.wait("codex --version", 8000)
            restored = self.capture("commands-restored")
            assert restored.count("codex --version") == 1, restored
            self.key("Escape")
            self.wait("SPYC_TRANSCRIPT_CHILD_READY", 5000)
            self.quit_host(self)

    smoke = TranscriptSmoke(args)
    result = {"passed": False, "coverage": "controlled PTY and recorded rollout; actual transcript pager"}
    try:
        smoke.run()
        result["passed"] = True
    except Exception as error:
        result["error"] = str(error)
        if smoke.started:
            smoke.capture("failure")
        print(str(error), file=sys.stderr, flush=True)
    finally:
        if smoke.started:
            smoke.cli("record", "stop", check=False)
            smoke.cli("close", check=False)
        (smoke.output / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    print(f"{'PASS' if result['passed'] else 'FAIL'}: {smoke.output}", flush=True)
    return 0 if result["passed"] else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--child", action="store_true")
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--scenario", choices=("recorded", "duplicate", "mixed"), default="recorded")
    parser.add_argument("--session", default="spyc-transcript-" + uuid.uuid4().hex[:10])
    args = parser.parse_args()
    if args.child:
        return child()
    if args.binary is None or args.output is None:
        parser.error("--binary and --output are required")
    return host(args)


if __name__ == "__main__":
    sys.exit(main())
