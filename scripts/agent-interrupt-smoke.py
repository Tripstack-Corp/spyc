#!/usr/bin/env python3
"""Replay interruption, child exit and bounded report TTL through real spyc/MCP.

Controlled raw-mode children stand in for agent profiles. Native agents, hooks
and hook trust are never executed or approved.
"""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import re
import select
import shlex
import subprocess
import sys
import time
import tty
import uuid


def rpc(binary, status, ttl=None, env=None):
    process = subprocess.Popen([str(binary), "--mcp"], stdin=subprocess.PIPE,
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=env)
    def request(ident, method, params):
        process.stdin.write(json.dumps({"jsonrpc": "2.0", "id": ident, "method": method, "params": params}) + "\n")
        process.stdin.flush()
        assert select.select([process.stdout], [], [], 8)[0], "MCP timeout"
        response = json.loads(process.stdout.readline())
        assert "error" not in response, response
        return response["result"]
    try:
        request(1, "initialize", {"protocolVersion": "2024-11-05", "capabilities": {},
                                  "clientInfo": {"name": "interrupt-replay", "version": "1"}})
        arguments = {"status": status}
        if ttl is not None:
            arguments["ttl_ms"] = ttl
        if env is not None:
            arguments["pane_id"] = env["SPYC_PANE_ID"]
        return request(2, "tools/call", {"name": "report_status", "arguments": arguments})
    finally:
        process.stdin.close()
        try:
            process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            process.terminate()
            process.wait(timeout=3)


def child(args):
    tty.setraw(0)
    routing = {key: os.environ[key] for key in ("SPYC_MCP_SOCK", "SPYC_PANE_ID")}
    (args.output / "routing.json").write_text(json.dumps(routing) + "\n")
    status = "blocked" if args.scenario == "exit_blocked" else "working"
    result = rpc(args.binary, status, 500_000 if args.scenario == "ttl" else None)
    assert not result.get("isError"), result
    print("SPYC_INTERRUPT_READY", flush=True)
    resumed = False
    while True:
        if (args.output / "exit-child").exists():
            return 7
        if (args.output / "resume-child").exists() and not resumed:
            result = rpc(args.binary, "working")
            assert not result.get("isError"), result
            print("SPYC_INTERRUPT_RESUMED", flush=True)
            resumed = True
        readable, _, _ = select.select([0], [], [], 0.05)
        if readable:
            data = os.read(0, 1)
            if not data:
                return 0
            if data in (b"\x1b", b"\x03"):
                print("SPYC_INTERRUPT_ACCEPTED", flush=True)
            elif data in (b"\r", b"\n"):
                print("SPYC_ORDINARY_INPUT_ACCEPTED", flush=True)


def load_ownership():
    spec = importlib.util.spec_from_file_location("hook_ownership", Path(__file__).with_name("codex-hook-ownership-smoke.py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def host(args):
    ownership = load_ownership()
    class InterruptSmoke(ownership.OwnershipSmoke):
        def run(self):
            self.setup()
            launcher = self.output / args.agent
            command = [sys.executable, str(Path(__file__).resolve()), "--child", "--binary", str(args.binary.resolve()),
                       "--output", str(self.output), "--scenario", args.scenario, "--agent", args.agent]
            launcher.write_text("#!/bin/sh\nexec " + shlex.join(command) + "\n")
            launcher.chmod(0o700)
            binary = args.binary.resolve()
            sessions = json.loads(subprocess.check_output(["tui-test", "--json", "sessions"], text=True))["sessions"]
            assert args.session not in sessions, "refusing to reuse a session"
            self.cli("run", "--backend", "ghostty", "--cols", "200", "--rows", "50", "--cwd", str(self.project),
                     "--env", "SHELL=/bin/sh", "--env", "PATH=" + str(binary.parent) + os.pathsep + os.environ["PATH"],
                     "--env", "SPYC_PANE_CMD=" + str(launcher), "--env", "HOME=" + str(self.home),
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
            self.wait("SPYC_INTERRUPT_READY", 15000)
            ready = self.dump("reported")
            self.status(ready, "blocked" if args.scenario == "exit_blocked" else "working")
            if args.scenario == "ttl":
                match = re.search(r"expires in (\d+)s", ready)
                assert match and int(match[1]) <= 300, ready
            elif args.scenario.startswith("exit"):
                (self.output / "exit-child").touch()
                self.wait("[exited 7]", 10000)
                gone = self.wait_dump("exited-pane", "idle")
                assert "no longer authoritative" in gone and "source: process-exit" in gone, gone
                routing = json.loads((self.output / "routing.json").read_text())
                result = rpc(binary, "working", env={**os.environ, **routing})
                (self.output / "late-report.json").write_text(json.dumps(result, indent=2) + "\n")
                assert result.get("isError"), "late report was accepted: " + json.dumps(result)
                self.status(self.dump("late-report-rejected"), "idle", semantic=False)
            else:
                self.key("Enter")
                self.wait("SPYC_ORDINARY_INPUT_ACCEPTED", 5000)
                time.sleep(3)
                self.status(self.dump("ordinary-input-working"), "working")
                self.key("Escape" if args.scenario == "escape" else "Ctrl+c")
                self.wait("SPYC_INTERRUPT_ACCEPTED", 5000)
                time.sleep(3)
                cancelled = self.dump("cancelled-idle")
                self.status(cancelled, "idle", semantic=False)
                assert "no longer authoritative" in cancelled and "source: SELF-REPORT" not in cancelled, cancelled
                time.sleep(3)
                self.status(self.dump("quiet-idle"), "idle", semantic=False)
                (self.output / "resume-child").touch()
                self.wait("SPYC_INTERRUPT_RESUMED", 5000)
                self.status(self.dump("new-report-working"), "working")
            self.quit_host(self)
    smoke = InterruptSmoke(args)
    result = {"passed": False, "coverage": "controlled PTY; actual host, input and pane-bound MCP"}
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
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--scenario", choices=("escape", "ctrl_c", "exit_working", "exit_blocked", "ttl"), default="escape")
    parser.add_argument("--agent", choices=("claude", "codex"), default="claude")
    parser.add_argument("--session", default="spyc-interrupt-" + uuid.uuid4().hex[:10])
    args = parser.parse_args()
    return child(args) if args.child else host(args)


if __name__ == "__main__":
    sys.exit(main())
