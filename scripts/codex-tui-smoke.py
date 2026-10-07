#!/usr/bin/env python3
"""Exercise real Codex attention states in a dedicated tui-test session."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time
import uuid


class Smoke:
    def __init__(self, args):
        self.args = args
        self.base = ["tui-test", "--session", args.session]
        self.output = args.output.resolve()
        self.output.mkdir(parents=True, exist_ok=False)
        self.log = self.output / "steps.jsonl"
        self.started = False

    def cli(self, *args, check=True):
        result = subprocess.run(self.base + list(args), capture_output=True, text=True,
                                timeout=65)
        with self.log.open("a") as out:
            out.write(json.dumps({"time": time.time(), "args": args,
                                  "exit": result.returncode, "stderr": result.stderr}) + "\n")
        if check and result.returncode:
            raise AssertionError(f"tui-test {args}: {result.stderr.strip()}")
        return result.stdout

    def wait(self, text, timeout=45000):
        self.cli("expect", "text", text, "--match", "any", "--timeout", str(timeout))

    def key(self, *keys):
        self.cli("key", "press", *keys)

    def capture(self, name):
        text = self.cli("text")
        (self.output / f"{name}.txt").write_text(text)
        return text

    def prompt(self, text):
        self.cli("type", text)
        # Codex treats Enter arriving in the same paste burst as pasted text.
        # Wait for the composer to render before sending its submission key.
        time.sleep(0.5)
        self.key("Enter")

    def dump(self, name):
        self.key("Ctrl+a", "k")
        self.cli("type", ":activity dump")
        self.key("Enter")
        self.wait("activity dump", 5000)
        text = self.capture(name)
        lines = []
        for line in text.splitlines():
            match = re.search(r"│\s*\d+\s(.*?)\s*│", line)
            if match:
                lines.append(match[1])
        dump = "\n".join(lines)
        (self.output / f"{name}.dump.txt").write_text(dump)
        self.key("q")
        self.key("Ctrl+a", "j")
        return dump

    def wait_dump(self, name, expected, timeout=15):
        deadline = time.monotonic() + timeout
        while True:
            dump = self.dump(name)
            if f'dot={expected}  agent=true' in dump:
                return dump
            assert time.monotonic() < deadline, f"no {expected} report:\n{dump}"
            time.sleep(0.5)

    @staticmethod
    def status(dump, expected, semantic=True):
        assert f'dot={expected}  agent=true' in dump, f"expected {expected}:\n{dump}"
        if semantic:
            assert f"source: SELF-REPORT status={expected}" in dump, dump
            assert "authoritative)" in dump and "not applied" not in dump, dump

    @staticmethod
    def events(dump, event):
        return re.findall(rf"event={event} status=(\S+) tool=(\S+) turn=(\S+) call=(\S+)", dump)

    def question(self):
        self.prompt("/plan")
        self.wait("Vim: Insert | Plan mode", 15000)
        self.prompt("For this diagnostic, do not call spyc report_status or edit files. "
                    "Use native request_user_input to ask one question with Proceed and Cancel choices. "
                    "After I answer, use exec_command to run sleep 30, then reply exactly "
                    "SPYC_QUESTION_DIAGNOSTIC_COMPLETE. Do not use request_user_input_async.")
        self.wait("Question 1/1")
        self.capture("question-open")
        blocked = self.dump("question-blocked")
        self.status(blocked, "blocked")
        start = self.events(blocked, "PreToolUse")
        assert len(start) == 1 and start[0][:2] == ("blocked", "request_user_input"), blocked
        assert all(x != "not-reported" for x in start[0][2:]), blocked
        assert len(self.events(blocked, "UserPromptSubmit")) == 1, blocked
        # Hold the open question through multiple idle thresholds.
        time.sleep(6)
        self.status(self.dump("question-still-blocked"), "blocked")
        self.key("Enter")
        answered_at = time.monotonic()
        deadline = time.monotonic() + 15
        while True:
            working = self.dump("question-after-answer")
            end = self.events(working, "PostToolUse")
            if end:
                break
            assert time.monotonic() < deadline, "no question completion report"
            time.sleep(0.5)
        self.status(working, "working")
        assert len(end) == 1 and end[0][:2] == ("working", "request_user_input"), working
        assert end[0][2:] == start[0][2:], "completion changed turn/call"
        assert re.findall(r"pane_id: (\S+)", blocked) == re.findall(r"pane_id: (\S+)", working)
        assert re.findall(r"codex_session_id: (\S+)", blocked) == re.findall(r"codex_session_id: (\S+)", working)
        time.sleep(8)
        self.status(self.dump("question-quiet-working"), "working")
        # Match the final response beneath a bullet, not the marker in the prompt.
        self.wait("• SPYC_QUESTION_DIAGNOSTIC_COMPLETE", 45000)
        assert "• Ran sleep 30" in self.capture("question-finished")
        assert time.monotonic() - answered_at >= 29, "quiet sleep did not last 30 seconds"
        # Final text can render before the asynchronous Stop reporter arrives.
        done = self.wait_dump("question-done", "done")
        self.status(done, "done")
        assert len(self.events(done, "Stop")) == 1, done
        assert len(self.events(done, "PreToolUse")) == len(self.events(done, "PostToolUse")) == 1, done

    def approval(self):
        self.prompt("For this diagnostic, do not call spyc report_status or edit files. "
                    "Use exec_command with sandbox_permissions=require_escalated and justification "
                    "'May I run the read-only spyc approval diagnostic?' to run sleep 30. "
                    "After it completes reply exactly SPYC_APPROVAL_DIAGNOSTIC_COMPLETE. "
                    "Do not request a persistent command prefix.")
        self.wait("Would you like to run the following command?")
        opened = self.capture("approval-open")
        modal = opened.split("Would you like to run the following command?", 1)[1]
        assert re.findall(r"(?m)^\s*\$\s+(.+)$", modal) == ["sleep 30"], modal
        assert "› 1. Yes, proceed (y)" in modal, "one-time approval is not selected"
        blocked = self.dump("approval-blocked")
        self.status(blocked, "blocked")
        assert len(self.events(blocked, "PermissionRequest")) == 1, blocked
        # Approve only this diagnostic's sleep, never a general prefix or hook trust.
        self.key("Enter")
        time.sleep(6)
        self.status(self.dump("approval-after-answer"), "working", semantic=False)
        time.sleep(8)
        self.status(self.dump("approval-quiet-working"), "working", semantic=False)
        self.wait("• SPYC_APPROVAL_DIAGNOSTIC_COMPLETE", 45000)
        done = self.wait_dump("approval-done", "done")
        self.status(done, "done")
        assert len(self.events(done, "Stop")) == 1, done

    def run(self):
        binary = self.args.binary.resolve()
        manifest = {"binary": str(binary), "sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                    "root": str(self.args.root.resolve()), "session": self.args.session,
                    "scenario": self.args.scenario,
                    "version": subprocess.check_output([str(binary), "--version"], text=True).strip()}
        (self.output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
        # This script stays alive while the daemon runs; tool runners can reap
        # detached descendants when the launching command ends.
        sessions = subprocess.check_output(["tui-test", "--json", "sessions"], text=True)
        assert self.args.session not in sessions, "refusing to reuse an existing session"
        command = "codex" if self.args.scenario == "question" else (
            'codex -a on-request -c approvals_reviewer="user" -s read-only')
        path = str(binary.parent) + os.pathsep + os.environ["PATH"]
        self.cli("run", "--backend", "ghostty", "--cols", "200", "--rows", "60",
                 "--cwd", str(self.args.root.resolve()), "--env", "SHELL=/bin/sh",
                 "--env", "PATH=" + path, "--env", "SPYC_PANE_CMD=" + command,
                 "--env", "COLORTERM=truecolor", "--env", "SPYC_MCP_SOCK=",
                 "--env", "SPYC_PANE_ID=", str(binary), "--status-trace", "--no-lua")
        self.started = True
        self.cli("record", "start", str(self.output / "session.cast"), "--format", "cast")
        self.key("Ctrl+a", "c")
        self.wait("pane command:", 5000)
        self.key("Enter")
        self.wait("pane cwd:", 5000)
        self.key("Enter")
        self.wait("Vim: Insert", 30000)
        self.capture("ready")
        print(f"Started {self.args.scenario}: {self.args.session}", flush=True)
        getattr(self, self.args.scenario)()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parent.parent)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--scenario", choices=("question", "approval"), default="question")
    parser.add_argument("--session", default="spyc-codex-" + uuid.uuid4().hex[:10])
    args = parser.parse_args()
    smoke = Smoke(args)
    result = {"passed": False}
    try:
        smoke.run()
        result["passed"] = True
    except Exception as error:
        result["error"] = str(error)
        if smoke.started:
            smoke.capture("failure")
        print(str(error), file=sys.stderr, flush=True)
    finally:
        (smoke.output / "result.json").write_text(json.dumps(result, indent=2) + "\n")
        if smoke.started:
            smoke.cli("record", "stop", check=False)
            smoke.cli("close", check=False)
    print(f"{'PASS' if result['passed'] else 'FAIL'}: {smoke.output}", flush=True)
    return 0 if result["passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
