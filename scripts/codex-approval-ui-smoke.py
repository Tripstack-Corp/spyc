#!/usr/bin/env python3
"""Native Codex approval UI checks; skips hook review without granting trust.

These checks establish visible-wait detection and modal recovery. They do not
establish hook delivery or semantic question recovery; use codex-tui-smoke.py
with hooks reviewed by the user for those acceptance cases.
"""
import argparse
import importlib.util
import json
from pathlib import Path
import shlex
import sys
import time
import uuid

source = Path(__file__).with_name("codex-tui-smoke.py")
spec = importlib.util.spec_from_file_location("codex_smoke", source)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


def compact(text):
    return "".join(text.split())


class ApprovalUI(module.Smoke):
    def cli(self, *args, **kwargs):
        args = list(args)
        if args and args[0] == "run":
            args[args.index("--cols") + 1] = str(self.args.cols)
            command = ['codex', '-a', 'on-request', '-c', 'approvals_reviewer="user"', '-s', 'read-only']
            if self.args.scenario.startswith("mcp"):
                probe = str(Path(__file__).with_name("codex-approval-probe.py").resolve())
                command += ['-c', 'mcp_servers.attention_probe.command="python3"',
                            '-c', 'mcp_servers.attention_probe.args=' + json.dumps([probe, str(self.output / "executed.txt")]),
                            '-c', 'mcp_servers.attention_probe.default_tools_approval_mode="prompt"']
            for i, arg in enumerate(args):
                if arg.startswith("SPYC_PANE_CMD="):
                    args[i] = "SPYC_PANE_CMD=" + shlex.join(command)
        return super().cli(*args, **kwargs)

    def open_pane(self):
        self.key("Ctrl+a", "c")
        self.wait("pane command:", 5000)
        self.key("Enter")
        self.wait("pane cwd:", 5000)
        self.key("Enter")
        deadline = time.monotonic() + 45
        ready_since = None
        while time.monotonic() < deadline:
            screen = self.cli("text")
            if "Hooks need review" in screen:
                self.capture("hook-review-skipped")
                self.key("Esc")
                ready_since = None
            elif "Update available" in screen and "esc skip" in screen:
                self.key("Esc")
                ready_since = None
            elif "Ask Codex to do anything" in screen:
                ready_since = ready_since or time.monotonic()
                if time.monotonic() - ready_since >= 2:
                    return
            else:
                ready_since = None
            time.sleep(0.25)
        raise AssertionError("Codex startup did not settle; inspect the saved viewport")

    def prompt(self, text):
        self.cli("type", text)
        deadline = time.monotonic() + 5
        while compact(text) not in compact(self.cli("text")):
            assert time.monotonic() < deadline, "complete prompt not visible in composer"
            time.sleep(0.1)
        time.sleep(0.5)
        self.key("Enter")

    def wait_form(self, phrases):
        deadline = time.monotonic() + 90
        while True:
            screen = compact(self.cli("text"))
            if all(compact(phrase) in screen for phrase in phrases):
                return
            if self.args.scenario == "edit" and compact("Would you like to run the following command?") in screen:
                raise AssertionError("model chose command approval; native file-edit case not exercised")
            assert time.monotonic() < deadline, "native approval form did not appear"
            time.sleep(0.5)

    def held_wait(self, reason):
        self.capture("approval-open")
        time.sleep(6)
        dump = self.dump("approval-held")
        assert "dot=blocked" in dump, dump
        assert "source: SCRAPE-FALLBACK" in dump, dump
        if self.args.cols >= 150:
            assert reason in dump, dump

    def recovered(self):
        deadline = time.monotonic() + 15
        while True:
            dump = self.dump("approval-recovered")
            if "dot=blocked" not in dump and "source: SCRAPE-FALLBACK" not in dump:
                break
            assert time.monotonic() < deadline, dump
            time.sleep(0.5)
        time.sleep(3)
        dump = self.dump("approval-still-recovered")
        assert "dot=blocked" not in dump and "source: SCRAPE-FALLBACK" not in dump, dump
        self.capture("recovered")

    def command(self):
        self.prompt("No report_status. Use exec_command: cmd=sleep 5, sandbox_permissions=require_escalated, justification=Sleep test. No persistent approval.")
        self.wait_form(["Would you like to run the following command?", "Yes, proceed",
                        "No, and tell Codex what to do differently", "Press enter to confirm or esc to cancel"])
        self.held_wait("awaiting command approval")
        self.key("Enter")
        self.recovered()

    def edit(self):
        destination = self.output / "approved.txt"
        self.prompt("No report_status. Use apply_patch directly with this exact patch:\n"
                    "*** Begin Patch\n*** Add File: " + str(destination)
                    + "\n+SPYC_EDIT_APPROVAL\n*** End Patch\n"
                    "Do not run discovery commands or mkdir; the output directory exists. "
                    "Use no other tool for this edit. If cancelled, do not retry. Then finish.")
        self.wait_form(["Would you like to make the following edits?", "Yes, proceed",
                        "Yes, and don't ask again for these files",
                        "No, and tell Codex what to do differently", "Press enter to confirm or esc to cancel"])
        assert not destination.exists(), "edit happened before approval"
        self.held_wait("awaiting file-edit approval")
        assert not destination.exists(), "edit happened while awaiting approval"
        self.key("Enter")
        deadline = time.monotonic() + 60
        while not destination.exists():
            assert time.monotonic() < deadline, "approved edit did not run"
            time.sleep(0.5)
        assert destination.read_text() == "SPYC_EDIT_APPROVAL\n"
        self.recovered()

    def mcp(self):
        self.mcp_wait(False)

    def mcp_decline(self):
        self.mcp_wait(True)

    def mcp_wait(self, decline):
        marker = self.output / "executed.txt"
        self.prompt("Call attention_probe once. No report_status. If cancelled, do not retry. Then finish.")
        self.wait_form(["Field 1/1", "1. Allow", "Run the tool and continue",
                        "Cancel this tool call", "enter to submit | esc to cancel"])
        assert not marker.exists(), "tool ran before approval"
        self.held_wait("awaiting MCP tool approval")
        assert not marker.exists(), "tool ran while awaiting approval"
        if decline:
            self.key("Down")
            self.wait("› 2. Cancel", 5000)
            self.capture("cancel-selected")
        self.key("Enter")
        if not decline:
            deadline = time.monotonic() + 60
            while not marker.exists():
                assert time.monotonic() < deadline, "approved tool did not run"
                time.sleep(0.5)
            assert marker.read_text() == "SPYC_MCP_APPROVAL_EXECUTED\n"
        self.recovered()
        assert marker.exists() != decline, "cancelled tool ran"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--root", type=Path, default=source.parent.parent)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--scenario", choices=("command", "edit", "mcp", "mcp_decline"), default="mcp")
    parser.add_argument("--cols", type=int, default=200)
    parser.add_argument("--session", default="spyc-approval-ui-" + uuid.uuid4().hex[:10])
    args = parser.parse_args()
    assert args.cols >= 40, "this driver requires at least 40 columns"
    smoke = ApprovalUI(args)
    result = {"passed": False, "coverage": "UI only; no hook trust approval", "cols": args.cols}
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
