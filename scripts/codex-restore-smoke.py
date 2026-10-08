#!/usr/bin/env python3
"""Restore mixed saved tabs twice through the actual spyc -r picker.

Disposable HOME/state and controlled PTY children; no native Codex or hook trust.
Assertions cover per-tab refusal, selected-tab mapping and saved-record survival.
"""
import argparse
import datetime
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import select
import shlex
import sys
import time
import tty
import uuid


def child():
    args = sys.argv[2:]
    with open(os.environ["SPYC_RESTORE_LOG"], "a") as out:
        out.write(json.dumps(args) + "\n")
    model = args[args.index("--model") + 1] if "--model" in args else "UNEXPECTED"
    tty.setraw(0)
    print("SPYC_RESTORE_READY_" + model, flush=True)
    while True:
        readable, _, _ = select.select([0], [], [], 0.1)
        if readable and not os.read(0, 1024):
            return 0


def host(args):
    source = Path(__file__).resolve()
    spec = importlib.util.spec_from_file_location("hook_ownership", source.with_name("codex-hook-ownership-smoke.py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)

    class RestoreSmoke(module.OwnershipSmoke):
        def launch(self, host, round_name):
            binary = args.binary.resolve()
            host.cli("run", "--backend", "ghostty", "--cols", "200", "--rows", "50", "--cwd", str(self.project),
                     "--env", "SHELL=/bin/sh", "--env", "PATH=" + str(binary.parent) + os.pathsep + os.environ["PATH"],
                     "--env", "HOME=" + str(self.home), "--env", "CODEX_HOME=" + str(self.home / ".codex"), "--env", "XDG_STATE_HOME=" + str(self.output / "state"),
                     "--env", "XDG_CONFIG_HOME=" + str(self.output / "config"), "--env", "COLORTERM=truecolor",
                     "--env", "SPYC_MCP_SOCK=", "--env", "SPYC_PANE_ID=", "--env", "SPYC_PANE_CMD=",
                     "--env", "SPYC_RESTORE_LOG=" + str(self.output / "launches.jsonl"), str(binary), "--no-lua", "-r")
            host.started = True
            host.cli("record", "start", str(host.output / "session.cast"), "--format", "cast")
            host.wait("RESTORE_ISOLATION", 15000)
            host.wait("Enter restore", 5000)
            host.capture(round_name + "-picker")
            host.key("Enter")

        def read_launches(self):
            path = self.output / "launches.jsonl"
            return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []

        def check_saved(self, name):
            saved = json.loads(self.saved_path.read_text())
            assert saved["id"] == self.saved["id"], saved
            assert len(saved["tabs"]) == 3 and saved["active_tab"] == self.saved_active, saved
            assert saved["tabs"][self.refused_index] == self.refused, "refused record was changed or lost"
            assert saved["tabs"][self.saved_active]["label"] == self.expected_model, saved
            (self.output / (name + "-saved.json")).write_text(json.dumps(saved, indent=2) + "\n")

        def check_restored(self, host, name, expected_count):
            host.wait("SPYC_RESTORE_READY_" + self.expected_model, 12000)
            host.capture(name + "-selected")
            dump = host.dump(name + "-activity")
            assert dump.count("pane_id:") == 2, dump
            active = next(line for line in dump.splitlines() if line.startswith("*"))
            assert '"' + self.expected_model + '"' in active, dump
            assert "--future-option" not in dump and "initial prompt" not in dump and "-i img.png" not in dump, dump
            until = time.monotonic() + 5
            while len(self.read_launches()) < expected_count and time.monotonic() < until:
                time.sleep(0.1)
            launches = self.read_launches()
            assert len(launches) == expected_count, launches
            assert all("--model" in argv and argv[argv.index("--model") + 1] in ("alpha", "beta") for argv in launches), launches
            assert all("resume" in argv and "--no-daemon" in argv for argv in launches), launches
            host.key("Ctrl+a", "k")
            host.key("Space", "s")
            host.wait("unopened tabs (kept saved)", 5000)
            host.wait("saved tab " + str(self.refused_index + 1), 5000)
            host.wait(self.reason, 5000)
            host.capture(name + "-reasons")
            host.key("Escape")
            # Wait for the debounced crash-sufficient save before the quit save.
            until = time.monotonic() + 8
            while True:
                saved = json.loads(self.saved_path.read_text())
                if saved["saved_at"] != self.saved["saved_at"]:
                    break
                assert time.monotonic() < until, "autosave did not update the source session"
                time.sleep(0.1)
            self.check_saved(name + "-autosave")

        def run(self):
            self.setup()
            launcher = self.output / "codex"
            launcher.write_text("#!/bin/sh\nexec " + shlex.join([sys.executable, str(source), "--child"]) + ' "$@"\n')
            launcher.chmod(0o700)
            base = shlex.quote(str(launcher))
            alpha = {"command": base + " --model alpha", "label": "alpha", "agent_session_id": str(uuid.uuid4())}
            beta = {"command": base + " --model beta", "label": "beta", "agent_session_id": str(uuid.uuid4())}
            unsupported = {"command": base + " --future-option", "label": "unopened", "agent_session_id": str(uuid.uuid4())}
            if args.scenario == "leading_unknown":
                tabs, active, self.refused_index, self.expected_model, self.saved_active, self.reason = [unsupported, alpha, beta], 1, 0, "alpha", 1, "unsupported Codex option"
            elif args.scenario == "middle_prompt":
                unsupported["command"] = base + " 'initial prompt'"
                tabs, active, self.refused_index, self.expected_model, self.saved_active, self.reason = [alpha, unsupported, beta], 1, 1, "beta", 2, "initial prompt"
            elif args.scenario == "trailing_image":
                unsupported["command"] = base + " -i img.png"
                tabs, active, self.refused_index, self.expected_model, self.saved_active, self.reason = [alpha, beta, unsupported], 2, 2, "beta", 1, "image"
            else:
                tabs, active = [unsupported], 0
            for index, tab in enumerate(tabs):
                tab.update(cwd=str(self.project), agent_kind="codex", agent_session_name="saved name " + str(index), claim_owner="saved-owner-" + str(index))
            self.refused = dict(unsupported)
            now = datetime.datetime.now(datetime.timezone.utc)
            self.saved = {"id": 991, "saved_at": now.isoformat(), "epoch_secs": int(now.timestamp()), "cwd": str(self.project),
                          "tabs": tabs, "active_tab": active, "pane_height_pct": 70, "pane_focused": True,
                          "name": "RESTORE_ISOLATION", "project_home": str(self.project), "scope_claims": []}
            directory = self.output / "state/spyc"
            directory.mkdir(parents=True)
            (directory / "hook_consent.json").write_text(json.dumps({str(self.project): False}))
            self.saved_path = directory / "sessions/991.json"
            self.saved_path.parent.mkdir()
            self.saved_path.write_text(json.dumps(self.saved) + "\n")
            manifest = json.loads((self.output / "manifest.json").read_text())
            manifest.update(coverage="actual restore picker, controlled children, autosave, quit and second restore", saved_session=self.saved)
            (self.output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
            self.launch(self, "first")
            if args.scenario == "all_refused":
                self.wait("session restore refused for all 1 tabs", 10000)
                self.capture("all-refused")
                self.quit_host(self)
                assert not self.read_launches(), "a refused command executed"
                assert json.loads(self.saved_path.read_text()) == self.saved, "wholly refused session changed"
                return
            self.check_restored(self, "first", 2)
            self.quit_host(self)
            self.check_saved("first-quit")
            sibling = module.module.Smoke(argparse.Namespace(output=self.output / "second", session=args.session + "-second"))
            self.hosts.append(sibling)
            self.launch(sibling, "second")
            self.check_restored(sibling, "second", 4)
            self.quit_host(sibling)
            self.check_saved("second-quit")

    smoke = RestoreSmoke(args)
    result = {"passed": False, "coverage": "actual restore UI; controlled children; no native trust"}
    try:
        smoke.run()
        result["passed"] = True
        print("PASS " + args.scenario + ": " + str(smoke.output), flush=True)
        return 0
    except Exception as error:
        result["error"] = str(error)
        print("FAIL " + args.scenario + ": " + str(smoke.output) + " — " + str(error), file=sys.stderr, flush=True)
        return 1
    finally:
        (smoke.output / "result.json").write_text(json.dumps(result, indent=2) + "\n")
        for test_host in getattr(smoke, "hosts", [smoke]):
            if test_host.started:
                test_host.capture("failure")
                test_host.cli("record", "stop", check=False)
                test_host.cli("close", check=False)


def main():
    if len(sys.argv) > 1 and sys.argv[1] == "--child":
        return child()
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--session", default="spyc-codex-restore-" + uuid.uuid4().hex[:8])
    parser.add_argument("--scenario", choices=("leading_unknown", "middle_prompt", "trailing_image", "all_refused"), default="leading_unknown")
    return host(parser.parse_args())


if __name__ == "__main__":
    raise SystemExit(main())
