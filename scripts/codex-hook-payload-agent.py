#!/usr/bin/env python3
"""Controlled Codex-profile PTY child for reporter/host replay (not native CLI)."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import select
import termios
import tty
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--scenario", required=True, choices=("question", "permission", "preblocked_question", "blocked_during_question"))
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    fixtures = json.loads((Path(__file__).resolve().parent.parent /
                           "tests/fixtures/codex-hook-metadata.json").read_text())
    source = fixtures["permission" if args.scenario == "permission" else "question_start"]
    common = {key: source[key] for key in ("session_id", "turn_id")}

    def report(status, metadata, body=None):
        # Content precedes every correlation field, as in a large hook body.
        payload = ({body: {"private-content": "private-content-" * 18000}} if body else {})
        payload.update(metadata)
        encoded = json.dumps(payload).encode()
        subprocess.run([str(args.binary), "--report-status", status, "--status-trace"],
                       input=encoded, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                       timeout=6, check=True)
        with (args.output / "reports.jsonl").open("a") as log:
            log.write(json.dumps({"status": status, "metadata": metadata,
                                  "input_bytes": len(encoded)}) + "\n")

    def agent_report(status):
        # Exercise an ordinary agent report over the real pane-bound MCP proxy.
        process = subprocess.Popen([str(args.binary), "--mcp"], stdin=subprocess.PIPE,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        def request(ident, method, params):
            process.stdin.write(json.dumps({"jsonrpc": "2.0", "id": ident,
                                          "method": method, "params": params}) + "\n")
            process.stdin.flush()
            assert select.select([process.stdout], [], [], 8)[0], "MCP timed out"
            response = json.loads(process.stdout.readline())
            assert "error" not in response, response
            result = response["result"]
            assert not result.get("isError"), result
            return result
        try:
            request(1, "initialize", {"protocolVersion": "2024-11-05", "capabilities": {},
                                     "clientInfo": {"name": "question-report-replay", "version": "1"}})
            context = request(2, "tools/call", {"name": "get_spyc_context", "arguments": {}})
            pane = json.loads(context["content"][0]["text"])["pane"]
            assert pane["id"] == os.environ["SPYC_PANE_ID"], pane
            result = request(3, "tools/call", {"name": "report_status", "arguments": {"status": status}})
            assert "status '" + status + "' set" in result["content"][0]["text"], result
            with (args.output / "reports.jsonl").open("a") as log:
                log.write(json.dumps({"status": status, "source": "agent MCP", "metadata": {},
                                      "input_bytes": 0, "pane_id": pane["id"]}) + "\n")
        finally:
            process.stdin.close()
            try:
                process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                process.terminate()
                process.wait(timeout=3)

    def show(text):
        print("\r" + text + "\r", flush=True)

    def advance():
        while True:
            value = os.read(0, 1)
            if not value:
                raise EOFError("replay input closed")
            if value in (b"\r", b"\n"):
                return

    before = termios.tcgetattr(0)
    tty.setraw(0)
    try:
        report("working", {**common, "hook_event_name": "UserPromptSubmit"})
        if args.scenario == "permission":
            report("blocked", fixtures["permission"], "tool_input")
            show("PAYLOAD_PERMISSION_READY: automatic review; no human dialogue")
            advance()
        else:
            show("PAYLOAD_" + args.scenario.upper() + "_READY: press Enter to begin replay")
            advance()
            if args.scenario == "preblocked_question":
                agent_report("blocked")
                show("PAYLOAD_GENERIC_BLOCKED: agent is about to ask the question")
                deadline = time.monotonic() + 20
                while not (args.output / "start-question").exists():
                    assert time.monotonic() < deadline, "question start trigger timed out"
                    time.sleep(0.05)
            report("codex-question-start", fixtures["question_start"], "tool_input")
            show("PAYLOAD_QUESTION_WAITING: question answer needed")
            advance()
            if args.scenario == "blocked_during_question":
                agent_report("blocked")
            show("PAYLOAD_ANSWER_RECEIVED: completion deliberately withheld")
            advance()
            wrong = {**fixtures["question_end"], "tool_use_id": "call_wrong_completion"}
            report("codex-question-end", wrong, "tool_response")
            show("PAYLOAD_WRONG_COMPLETION: matching question still pending")
            advance()
            report("codex-question-end", fixtures["question_end"], "tool_response")
            show("PAYLOAD_MATCHED_COMPLETION: correlated completion reported")
            advance()
            if args.scenario == "blocked_during_question":
                agent_report("working")
                show("PAYLOAD_NEWER_WORKING: explicit agent report retires its block")
                advance()
        report("done", {**common, "hook_event_name": "Stop"})
        show("PAYLOAD_REPLAY_DONE")
        # Keep the pane alive so lifecycle assertions concern status, not exit.
        while True:
            advance()
    finally:
        termios.tcsetattr(0, termios.TCSANOW, before)


if __name__ == "__main__":
    main()
