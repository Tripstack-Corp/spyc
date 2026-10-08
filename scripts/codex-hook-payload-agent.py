#!/usr/bin/env python3
"""Controlled Codex-profile PTY child for reporter/host replay (not native CLI)."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import termios
import tty


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--scenario", required=True, choices=("question", "permission"))
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    fixtures = json.loads((Path(__file__).resolve().parent.parent /
                           "tests/fixtures/codex-hook-metadata.json").read_text())
    source = fixtures["question_start" if args.scenario == "question" else "permission"]
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
            show("PAYLOAD_QUESTION_READY: press Enter to begin replay")
            advance()
            report("codex-question-start", fixtures["question_start"], "tool_input")
            show("PAYLOAD_QUESTION_WAITING: question answer needed")
            advance()
            show("PAYLOAD_ANSWER_RECEIVED: completion deliberately withheld")
            advance()
            wrong = {**fixtures["question_end"], "tool_use_id": "call_wrong_completion"}
            report("codex-question-end", wrong, "tool_response")
            show("PAYLOAD_WRONG_COMPLETION: matching question still pending")
            advance()
            report("codex-question-end", fixtures["question_end"], "tool_response")
            show("PAYLOAD_MATCHED_COMPLETION: working quietly")
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
