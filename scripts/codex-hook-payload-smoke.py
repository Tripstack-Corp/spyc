#!/usr/bin/env python3
"""tui-test replay of large hook input through the actual reporter and host.

Uses a controlled PTY child, not native Codex. Does not grant hook trust.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import time
import uuid

source = Path(__file__).with_name("codex-tui-smoke.py")
spec = importlib.util.spec_from_file_location("codex_smoke", source)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class PayloadReplay(module.Smoke):
    @staticmethod
    def reported(dump, state):
        assert f"dot={state}  agent=true" in dump, dump
        assert f"source: SELF-REPORT status={state}" in dump, dump

    def run(self):
        binary = self.args.binary.resolve()
        sandbox = self.output / "project"
        sandbox.mkdir()
        fakebin = self.output / "bin"
        fakebin.mkdir()
        command = [sys.executable, str(Path(__file__).with_name("codex-hook-payload-agent.py").resolve()),
                   "--binary", str(binary), "--scenario", self.args.scenario,
                   "--output", str(self.output)]
        launcher = fakebin / "codex"
        launcher.write_text("#!/bin/sh\nexec " + shlex.join(command) + "\n")
        launcher.chmod(0o700)
        manifest = {"binary": str(binary), "sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                    "version": subprocess.check_output([str(binary), "--version"], text=True).strip(),
                    "session": self.args.session, "scenario": self.args.scenario,
                    "coverage": "controlled PTY replay; actual reporter/host; no native CLI or hook trust",
                    "fixture_sha256": hashlib.sha256((source.parent.parent /
                        "tests/fixtures/codex-hook-metadata.json").read_bytes()).hexdigest()}
        (self.output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
        sessions = json.loads(subprocess.check_output(["tui-test", "--json", "sessions"], text=True))["sessions"]
        assert self.args.session not in sessions, "refusing to reuse an existing session"
        path = os.pathsep.join([str(fakebin), str(binary.parent), os.environ["PATH"]])
        self.cli("run", "--backend", "ghostty", "--cols", "220", "--rows", "60",
                 "--cwd", str(sandbox), "--env", "SHELL=/bin/sh", "--env", "PATH=" + path,
                 "--env", "SPYC_PANE_CMD=" + str(launcher), "--env", "COLORTERM=truecolor",
                 "--env", "SPYC_MCP_SOCK=", "--env", "SPYC_PANE_ID=",
                 "--env", "XDG_STATE_HOME=" + str(self.output / "state"),
                 str(binary), "--status-trace", "--no-lua")
        self.started = True
        self.cli("record", "start", str(self.output / "session.cast"), "--format", "cast")
        self.wait("🌶️", 15000)
        # Own disposable project only. Reporters are invoked by the replay,
        # without installing hooks or relying on native hook trust.
        self.cli("type", ":hooks off")
        self.key("Enter")
        self.key("Ctrl+a", "c")
        self.wait("pane command:", 5000)
        self.key("Enter")
        self.wait("pane cwd:", 5000)
        self.key("Enter")
        self.wait("PAYLOAD_" + self.args.scenario.upper() + "_READY", 15000)
        print(f"Started {self.args.scenario} replay: {self.args.session}", flush=True)
        getattr(self, self.args.scenario)()
        self.key("Enter")
        self.wait("PAYLOAD_REPLAY_DONE", 10000)
        self.reported(self.wait_dump("done", "done"), "done")
        reports = [json.loads(line) for line in (self.output / "reports.jsonl").read_text().splitlines()]
        assert any(report["input_bytes"] > 8192 for report in reports)
        if self.args.scenario in ("preblocked_question", "blocked_during_question"):
            agent_reports = [report for report in reports if report.get("source") == "agent MCP"]
            expected = ["blocked", "working"] if self.args.scenario == "blocked_during_question" else ["blocked"]
            assert [report["status"] for report in agent_reports] == expected, reports
        assert all("private-content" not in json.dumps(report) for report in reports)
        trace = (self.output / "state/spyc/mcp.log").read_text()
        assert "report-status: hook metadata:" in trace, "reporter trace was not captured"
        assert "private-content" not in trace, "raw hook content leaked into trace"
        for capture in self.output.glob("*.dump.txt"):
            assert "private-content" not in capture.read_text(), "raw hook content leaked into host dump"

    def permission(self):
        self.reported(self.dump("permission-working"), "working")
        time.sleep(6)
        dump = self.dump("permission-held-working")
        self.reported(dump, "working")
        assert len(self.events(dump, "PermissionRequest")) == 1, dump
        assert "not applied: PermissionRequest precedes review" in dump, dump
        assert "SCRAPE-FALLBACK" not in dump, dump

    def preblocked_question(self):
        self.question(preblocked=True)

    def blocked_during_question(self):
        self.question(independent=True)

    def question(self, preblocked=False, independent=False):
        self.reported(self.dump("initial-working"), "working")
        self.key("Enter")
        if preblocked:
            self.wait("PAYLOAD_GENERIC_BLOCKED", 10000)
            self.reported(self.dump("agent-pre-question-block"), "blocked")
            time.sleep(6)
            self.reported(self.dump("agent-block-held"), "blocked")
            (self.output / "start-question").touch()
        self.wait("PAYLOAD_QUESTION_WAITING", 10000)
        start = self.wait_dump("question-blocked", "blocked")
        self.reported(start, "blocked")
        event = self.events(start, "PreToolUse")
        assert len(event) == 1 and all(value != "not-reported" for value in event[0][2:]), start
        time.sleep(6)
        self.reported(self.dump("question-held"), "blocked")
        self.key("Enter")
        self.wait("PAYLOAD_ANSWER_RECEIVED", 10000)
        self.reported(self.dump("answer-without-completion"), "blocked")
        self.key("Enter")
        self.wait("PAYLOAD_WRONG_COMPLETION", 10000)
        wrong = self.dump("wrong-completion")
        self.reported(wrong, "blocked")
        assert "not applied:" in wrong and "call_wrong_completion" in wrong, wrong
        self.key("Enter")
        self.wait("PAYLOAD_MATCHED_COMPLETION", 10000)
        expected = "blocked" if independent else "working"
        working = self.wait_dump("matching-completion", expected)
        self.reported(working, expected)
        if independent:
            assert "another question or uncorrelated blocked report" in working, working
        completed = self.events(working, "PostToolUse")
        assert len(completed) == 2 and completed[-1][2:] == event[0][2:], working
        time.sleep(8)
        self.reported(self.dump("quiet-after-completion"), expected)
        if independent:
            self.key("Enter")
            self.wait("PAYLOAD_NEWER_WORKING", 10000)
            self.reported(self.wait_dump("newer-agent-working", "working"), "working")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--scenario", choices=("question", "permission", "preblocked_question", "blocked_during_question"), default="question")
    parser.add_argument("--session", default="spyc-hook-payload-" + uuid.uuid4().hex[:10])
    args = parser.parse_args()
    smoke = PayloadReplay(args)
    result = {"passed": False, "coverage": "reporter/host replay; native CLI and trust not exercised"}
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
