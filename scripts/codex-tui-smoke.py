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
        self.cli("expect", "text", text, "--whitespace", "normalize",
                 "--match", "any", "--timeout", "5000")
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

    def wait_dump(self, name, expected, timeout=15, pane=None):
        deadline = time.monotonic() + timeout
        while True:
            dump = self.dump(name)
            view = self.pane(dump, pane) if pane else dump
            if f'dot={expected}  agent=true' in view:
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
        self.finish_question("SPYC_QUESTION_DIAGNOSTIC_COMPLETE")

    def finish_question(self, marker):
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
        self.wait("• " + marker, 45000)
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
        self.answer_approval("sleep 30")
        time.sleep(6)
        working = self.dump("approval-after-answer")
        self.status(working, "working", semantic=False)
        assert "source: SELF-REPORT status=working" in working, working
        time.sleep(8)
        quiet = self.dump("approval-quiet-working")
        self.status(quiet, "working", semantic=False)
        assert "source: SELF-REPORT status=working" in quiet, quiet
        self.wait("• SPYC_APPROVAL_DIAGNOSTIC_COMPLETE", 45000)
        done = self.wait_dump("approval-done", "done")
        self.status(done, "done")
        assert len(self.events(done, "Stop")) == 1, done

    def auto(self):
        self.prompt("For this diagnostic, do not call spyc report_status or edit files. "
                    "Use exec_command with sandbox_permissions=require_escalated and justification "
                    "'May I run the read-only spyc automatic review diagnostic?' to run sleep 30. "
                    "After it completes reply exactly SPYC_AUTO_REVIEW_COMPLETE. "
                    "Do not request a persistent command prefix.")
        deadline = time.monotonic() + 45
        while True:
            review = self.dump("auto-review")
            if self.events(review, "PermissionRequest"):
                break
            assert time.monotonic() < deadline, "no permission hook during automatic review"
            time.sleep(0.5)
        assert len(self.events(review, "PermissionRequest")) == 1, review
        time.sleep(6)
        screen = self.capture("auto-running")
        assert "Would you like to run the following command?" not in screen, screen
        quiet = self.dump("auto-quiet-working")
        self.status(quiet, "working", semantic=False)
        assert "source: SELF-REPORT status=working" in quiet, quiet
        assert "not applied: PermissionRequest precedes review" in quiet, quiet
        self.wait("• SPYC_AUTO_REVIEW_COMPLETE", 45000)
        assert "• Ran sleep 30" in self.capture("auto-finished")
        done = self.wait_dump("auto-done", "done")
        self.status(done, "done")
        assert len(self.events(done, "Stop")) == 1, done

    def answer_approval(self, command):
        self.wait("Would you like to run the following command?")
        opened = self.capture("approval-open")
        modal = opened.split("Would you like to run the following command?", 1)[1]
        assert re.findall(r"(?m)^\s*\$\s+(.+)$", modal) == [command], modal
        assert "› 1. Yes, proceed (y)" in modal, "one-time approval is not selected"
        # The UI fallback is deliberately debounced; seeing the first modal
        # draw does not mean its quiet-window scan has completed.
        blocked = self.wait_dump("approval-blocked", "blocked")
        self.status(blocked, "blocked", semantic=False)
        assert "source: SCRAPE-FALLBACK status=blocked (awaiting command approval)" in blocked, blocked
        assert "not applied: PermissionRequest precedes review" in blocked, blocked
        assert len(self.events(blocked, "PermissionRequest")) == 1, blocked
        # Approve only this diagnostic's sleep, never a general prefix or hook trust.
        self.key("Enter")
        return blocked

    def mixed(self):
        self.prompt("/plan")
        self.wait("Vim: Insert | Plan mode", 15000)
        self.prompt("For this diagnostic, do not call spyc report_status or edit files. "
                    "First run sleep 1 via exec_command with sandbox_permissions=require_escalated, "
                    "justification 'May I run the read-only spyc mixed prompt diagnostic?' and no persistent prefix. "
                    "Then ask one Proceed/Cancel question via native request_user_input. "
                    "After I answer, run sleep 30 without escalation and reply exactly SPYC_MIXED_DIAGNOSTIC_COMPLETE. "
                    "Do not use request_user_input_async.")
        approval = self.answer_approval("sleep 1")
        self.wait("Question 1/1")
        question = self.dump("mixed-question")
        permission = self.events(approval, "PermissionRequest")
        start = self.events(question, "PreToolUse")
        assert len(permission) == len(start) == 1, question
        assert permission[0][2] == start[0][2], "approval and question must share a turn"
        self.finish_question("SPYC_MIXED_DIAGNOSTIC_COMPLETE")

    @staticmethod
    def pane(dump, number):
        match = re.search(rf"(?ms)^[ *]\[{number}\] .*?(?=^[ *]\[\d+\] |\Z)", dump)
        assert match, f"pane {number} missing:\n{dump}"
        return match[0]

    @classmethod
    def parallel_state(cls, dump, expected, identities=None):
        found = []
        for number, status in enumerate(expected, 1):
            part = cls.pane(dump, number)
            cls.status(part, status)
            pane = re.findall(r"pane_id: (\S+)", part)
            session = re.findall(r"codex_session_id: (\S+)", part)
            start = cls.events(part, "PreToolUse")
            end = cls.events(part, "PostToolUse")
            assert len(pane) == len(session) == len(start) == 1, part
            assert start[0][:2] == ("blocked", "request_user_input"), part
            assert all(x != "not-reported" for x in start[0][2:]), part
            if end:
                assert len(end) == 1 and end[0][:2] == ("working", "request_user_input"), part
                assert end[0][2:] == start[0][2:], "completion changed turn/call"
            else:
                assert status == "blocked", "answered pane has no completion"
            assert len(cls.events(part, "UserPromptSubmit")) == 1, part
            assert len(cls.events(part, "Stop")) == (1 if status == "done" else 0), part
            found.append((pane[0], session[0], *start[0][2:]))
        assert all(found[0][i] != found[1][i] for i in range(4)), "panes share identity"
        if identities:
            assert found == identities, "pane/session/question binding changed"
        return found

    def begin_parallel_question(self, marker):
        self.prompt("/plan")
        self.wait("Vim: Insert | Plan mode", 15000)
        self.prompt("For this diagnostic, do not call spyc report_status or edit files. "
                    "Use native request_user_input to ask one question with Proceed and Cancel choices. "
                    "After I answer, use exec_command to run sleep 30, then reply exactly "
                    + marker + ". Do not use request_user_input_async.")
        self.wait("Question 1/1")

    def open_pane(self):
        self.key("Ctrl+a", "c")
        self.wait("pane command:", 5000)
        self.key("Enter")
        self.wait("pane cwd:", 5000)
        self.key("Enter")
        self.wait("Vim: Insert", 30000)

    def parallel(self):
        self.begin_parallel_question("SPYC_PARALLEL_ONE_COMPLETE")
        first = self.pane(self.dump("parallel-first-blocked"), 1)
        self.status(first, "blocked")
        self.open_pane()
        self.begin_parallel_question("SPYC_PARALLEL_TWO_COMPLETE")
        both = self.dump("parallel-both-blocked")
        identities = self.parallel_state(both, ("blocked", "blocked"))
        assert re.findall(r"pane_id: (\S+)", first) == [identities[0][0]]
        assert "mcp connections: conn:2 bound:2" in both, both
        self.key("Enter")
        answered_at = time.monotonic()
        working = self.wait_dump("parallel-second-working", "working", pane=2)
        self.parallel_state(working, ("blocked", "working"), identities)
        time.sleep(8)
        self.parallel_state(self.dump("parallel-second-quiet"), ("blocked", "working"), identities)
        self.wait("• SPYC_PARALLEL_TWO_COMPLETE", 45000)
        assert "• Ran sleep 30" in self.capture("parallel-second-finished")
        assert time.monotonic() - answered_at >= 29
        second_done = self.wait_dump("parallel-second-done", "done", pane=2)
        self.parallel_state(second_done, ("blocked", "done"), identities)
        self.key("Ctrl+a", "1")
        self.wait("Question 1/1", 5000)
        self.capture("parallel-first-still-open")
        self.key("Enter")
        answered_at = time.monotonic()
        working = self.wait_dump("parallel-first-working", "working", pane=1)
        self.parallel_state(working, ("working", "done"), identities)
        time.sleep(8)
        self.parallel_state(self.dump("parallel-first-quiet"), ("working", "done"), identities)
        self.wait("• SPYC_PARALLEL_ONE_COMPLETE", 45000)
        assert "• Ran sleep 30" in self.capture("parallel-first-finished")
        assert time.monotonic() - answered_at >= 29
        done = self.wait_dump("parallel-both-done", "done", pane=1)
        self.parallel_state(done, ("done", "done"), identities)
        assert re.findall(r"report_status:(\d+)", done) == ["8"], done

    def run(self):
        binary = self.args.binary.resolve()
        manifest = {"binary": str(binary), "sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                    "root": str(self.args.root.resolve()), "session": self.args.session,
                    "scenario": self.args.scenario,
                    "version": subprocess.check_output([str(binary), "--version"], text=True).strip(),
                    "codex_version": subprocess.check_output(["codex", "--version"], text=True).strip()}
        (self.output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
        # This script stays alive while the daemon runs; tool runners can reap
        # detached descendants when the launching command ends.
        sessions = subprocess.check_output(["tui-test", "--json", "sessions"], text=True)
        assert self.args.session not in sessions, "refusing to reuse an existing session"
        command = "codex" if self.args.scenario in ("question", "parallel") else (
            'codex -a on-request -c approvals_reviewer="user" -s read-only')
        if self.args.scenario == "auto":
            command = 'codex -a on-request -c approvals_reviewer="auto_review" -s read-only'
        path = str(binary.parent) + os.pathsep + os.environ["PATH"]
        self.cli("run", "--backend", "ghostty", "--cols", "200", "--rows", "60",
                 "--cwd", str(self.args.root.resolve()), "--env", "SHELL=/bin/sh",
                 "--env", "PATH=" + path, "--env", "SPYC_PANE_CMD=" + command,
                 "--env", "COLORTERM=truecolor", "--env", "SPYC_MCP_SOCK=",
                 "--env", "SPYC_PANE_ID=", str(binary), "--status-trace", "--no-lua")
        self.started = True
        self.cli("record", "start", str(self.output / "session.cast"), "--format", "cast")
        self.open_pane()
        self.capture("ready")
        print(f"Started {self.args.scenario}: {self.args.session}", flush=True)
        getattr(self, self.args.scenario)()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parent.parent)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--scenario", choices=("question", "approval", "mixed", "parallel", "auto"), default="question")
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
