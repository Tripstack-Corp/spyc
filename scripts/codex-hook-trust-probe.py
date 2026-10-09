#!/usr/bin/env python3
"""Read native hook identities in a disposable project; never execute hooks."""
import json
import os
import selectors
import subprocess
import time


def discover(binary, project, home, output, phase):
    environment = dict(os.environ, CODEX_HOME=str(home))
    for key in ("SPYC_MCP_SOCK", "SPYC_PANE_ID"):
        environment.pop(key, None)
    with (output / f"{phase}.stderr.log").open("w") as errors:
        process = subprocess.Popen([str(binary), "-c", "features.hooks=true", "app-server"],
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
                assert line, "native app server exited"
                value = json.loads(line)
                if value.get("id") == request_id:
                    assert "error" not in value, value
                    return value
            raise AssertionError("native app-server response timeout")

        try:
            send({"id": 1, "method": "initialize", "params": {
                "clientInfo": {"name": "spyc_hook_identity_probe", "version": "0.1"},
                "capabilities": {"experimentalApi": True}}})
            response(1)
            send({"method": "initialized", "params": {}})
            send({"id": 2, "method": "hooks/list", "params": {"cwds": [str(project)]}})
            result = response(2)
            (output / f"{phase}.hooks.json").write_text(json.dumps(result, indent=2) + "\n")
            entries = result["result"].get("data", [])
            assert len(entries) == 1 and entries[0]["cwd"] == str(project), result
            assert not entries[0].get("errors"), result
            hooks = entries[0]["hooks"]
            # The fixture permits loading a temporary project, but has no hook
            # trust records. Listing is read-only; no turn/approval RPC is sent.
            assert all(hook["trustStatus"] == "untrusted" for hook in hooks), hooks
            users = [hook for hook in hooks if hook.get("command") == "printf SPYC_USER_HOOK"]
            assert len(users) == 1, result
            return {key: users[0][key] for key in ("key", "currentHash", "trustStatus", "enabled", "sourcePath")}
        finally:
            selector.close()
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
