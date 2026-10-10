#!/usr/bin/env python3
"""Stand-in agent for the agents demo: plays one role, reporting over spyc's MCP.

demo.py installs this as `claude` on the pane's PATH, so spyc treats each tab as
a Claude pane. Every dot change in the clip is a real `report_status` call on
the same `spyc --mcp` connection a real agent uses; only the work is scripted.
A real agent cannot be filmed here: under the recorder's HOME it shows that
account's status line and any organization notices.

Roles, chosen by the first known word in the arguments:
  worker   works for the whole clip and never needs you
  blocker  works, then blocks on a question once the driver creates the
           trigger file; answering lets it finish and move the file list
  done     finishes one turn straight away
"""

import json
import os
from pathlib import Path
import select
import subprocess
import sys
import time

DIM, BOLD, RESET = "\033[2m", "\033[1m", "\033[0m"
TASKS = {
    "worker": "audit every doc for broken links",
    "blocker": "update the guides for the 3.0 codec default",
    "done": "summarize RELEASE-NOTES.md",
}


class Spyc:
    """One MCP session over `spyc --mcp`, the stdio proxy the pane's env routes."""

    def __init__(self):
        self.process = subprocess.Popen([os.environ["SPYC_DEMO_BIN"], "--mcp"],
                                        stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                        stderr=subprocess.DEVNULL, text=True)
        self.next_id = 1
        self.request("initialize", {"protocolVersion": "2024-11-05", "capabilities": {},
                                    "clientInfo": {"name": "spyc-demo-agent", "version": "1"}})

    def request(self, method, params):
        self.process.stdin.write(json.dumps({"jsonrpc": "2.0", "id": self.next_id,
                                             "method": method, "params": params}) + "\n")
        self.process.stdin.flush()
        self.next_id += 1
        if not select.select([self.process.stdout], [], [], 8)[0]:
            raise RuntimeError(f"spyc MCP did not answer {method}")
        response = json.loads(self.process.stdout.readline())
        if "error" in response or response.get("result", {}).get("isError"):
            raise RuntimeError(f"spyc MCP refused {method}: {response}")
        return response["result"]

    def tool(self, name, **arguments):
        return self.request("tools/call", {"name": name, "arguments": arguments})

    def status(self, state):
        self.tool("report_status", status=state)


def say(text="", style=""):
    print(f"{style}{text}{RESET if style else ''}", flush=True)


def step(text, pause=1.1):
    say(f"  {text}", DIM)
    time.sleep(pause)


def work(spyc):
    # The report lives five minutes; renewing it every pass keeps the dot live.
    docs = sorted(str(p) for p in Path.cwd().rglob("*.md") if ".claude" not in p.parts)
    while True:
        spyc.status("working")
        for doc in docs:
            step(f"checking {os.path.relpath(doc)} ... ok")


def block(spyc):
    spyc.status("working")
    for line in ("reading guides/quickstart.md", "reading guides/tuning.md",
                 "reading guides/troubleshooting.md"):
        step(line, 0.6)
    trigger = Path(os.environ["SPYC_DEMO_TRIGGER"])
    while not trigger.exists():
        time.sleep(0.1)
    spyc.status("blocked")
    say()
    say("Three guides still show the 2.x codec default. Update their examples? [y/N] ", BOLD)
    sys.stdin.readline()
    spyc.status("working")
    for name in ("quickstart", "tuning", "troubleshooting"):
        step(f"updated guides/{name}.md", 0.5)
    spyc.tool("navigate_to", path=str(Path.cwd() / "guides"))
    step("opened guides/ in spyc", 0.4)
    spyc.status("done")
    say("Done: 3 guides updated.", BOLD)


def finish(spyc):
    spyc.status("working")
    step("reading RELEASE-NOTES.md", 0.8)
    spyc.status("done")
    say("Aurora 3.0 changes the default codec and parks producers under backpressure.", BOLD)


def main():
    role = next((a for a in sys.argv[1:] if a in TASKS), None)
    if role is None:
        sys.exit(f"usage: claude {{{'|'.join(TASKS)}}}")
    spyc = Spyc()
    say(f"› {TASKS[role]}", BOLD)
    say()
    {"worker": work, "blocker": block, "done": finish}[role](spyc)
    say()
    say("›", BOLD)
    # A real agent waits at its prompt; so does this one.
    for _ in sys.stdin:
        pass


if __name__ == "__main__":
    main()
