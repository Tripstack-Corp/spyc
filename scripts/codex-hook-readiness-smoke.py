#!/usr/bin/env python3
"""Inspect fresh Codex hook discovery without approving project or hook trust."""

import argparse
import json
import os
from pathlib import Path
import selectors
import subprocess
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    project = output / "project"
    home = output / "codex-home"
    config_dir = project / ".codex"
    config_dir.mkdir(parents=True)
    home.mkdir()
    subprocess.run(["git", "init", "--quiet", str(project)], check=True)
    signals = {
        "UserPromptSubmit": "working", "PermissionRequest": "blocked",
        "Stop": "done", "Interrupt": "idle",
        "PreToolUse": "codex-question-start", "PostToolUse": "codex-question-end",
    }
    lines = []
    for event, signal in signals.items():
        lines.append(f"[[hooks.{event}]]")
        if event in ("PreToolUse", "PostToolUse"):
            lines.append('matcher = "^request_user_input$"')
        lines.extend([f"[[hooks.{event}.hooks]]", 'type = "command"',
                      "command = " + json.dumps(f"spyc --report-status {signal} 2>/dev/null || true")])
    config = config_dir / "config.toml"
    config.write_text("\n".join(lines) + "\n")
    original = config.read_bytes()
    environment = dict(os.environ, CODEX_HOME=str(home))
    for key in ("SPYC_MCP_SOCK", "SPYC_PANE_ID"):
        environment.pop(key, None)
    manifest = {"project": str(project), "codex_home": str(home),
                "codex_version": subprocess.check_output(["codex", "--version"], text=True).strip(),
                "project_hook_events": list(signals), "project_trust_approved": False,
                "hook_trust_approved": False}
    (output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    with (output / "stderr.log").open("w") as errors:
        process = subprocess.Popen(["codex", "-c", "features.hooks=true", "app-server"],
                                   cwd=project, env=environment, stdin=subprocess.PIPE,
                                   stdout=subprocess.PIPE, stderr=errors, text=True, bufsize=1)
        selector = selectors.DefaultSelector()
        selector.register(process.stdout, selectors.EVENT_READ)

        def send(value):
            process.stdin.write(json.dumps(value) + "\n")
            process.stdin.flush()

        def response(request_id):
            deadline = time.monotonic() + 15
            while time.monotonic() < deadline:
                if not selector.select(timeout=1):
                    continue
                line = process.stdout.readline()
                assert line, "app server exited"
                value = json.loads(line)
                if value.get("id") == request_id:
                    assert "error" not in value, value
                    return value
            raise AssertionError("app-server response timeout")

        try:
            send({"id": 1, "method": "initialize", "params": {
                "clientInfo": {"name": "spyc_hook_readiness_probe", "version": "0.1"},
                "capabilities": {"experimentalApi": True}}})
            response(1)
            send({"method": "initialized", "params": {}})
            send({"id": 2, "method": "hooks/list", "params": {"cwds": [str(project)]}})
            result = response(2)
            (output / "hooks-list.json").write_text(json.dumps(result, indent=2) + "\n")
            for entry in result["result"].get("data", []):
                print(json.dumps({"hook_count": len(entry.get("hooks", [])),
                                  "warnings": entry.get("warnings"), "errors": entry.get("errors"),
                                  "hooks": [{key: hook.get(key) for key in
                                      ("eventName", "enabled", "trustStatus", "sourcePath")}
                                      for hook in entry.get("hooks", [])]}))
            entries = result["result"].get("data", [])
            assert len(entries) == 1 and entries[0]["cwd"] == str(project), result
            assert entries[0].get("hooks") == [], "untrusted project hooks became available"
            assert "Project-local config, hooks, and exec policies are disabled" in (output / "stderr.log").read_text()
            assert config.read_bytes() == original, "project declarations changed"
            assert not (home / "config.toml").exists(), "probe wrote trust configuration"
            (output / "result.json").write_text(json.dumps({"passed": True,
                "case": "untrusted project declarations ignored; execution not tested"}, indent=2) + "\n")
        finally:
            selector.close()
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
    # This probe lists discovery and trust metadata; it does not start a turn
    # or establish hook execution. No project/command trust mutation is sent.
    print(f"Captured fresh readiness metadata: {output}")


if __name__ == "__main__":
    main()
