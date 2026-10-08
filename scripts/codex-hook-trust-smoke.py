#!/usr/bin/env python3
"""tui-test installation/cleanup plus native Codex hook-identity discovery.

Uses a controlled PTY child. Native Codex only lists hooks; none are approved or
executed. Project-load permission exists only in a disposable CODEX_HOME fixture.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import time
import uuid


def import_file(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


source = Path(__file__).resolve()
smoke_module = import_file("codex_smoke", source.with_name("codex-tui-smoke.py"))
probe = import_file("codex_hook_identity_probe", source.with_name("codex-hook-trust-probe.py"))


class IdentitySmoke(smoke_module.Smoke):
    def run(self):
        binary = self.args.binary.resolve()
        native = Path(shutil.which("codex")).resolve()
        self.project = self.output / "project"
        self.home = self.output / "codex-home"
        config_dir = self.project / ".codex"
        config_dir.mkdir(parents=True)
        self.home.mkdir()
        subprocess.run(["git", "init", "--quiet", str(self.project)], check=True)
        # Allows discovery of this fixture only, without approving any hook.
        # The real user's project and hook trust records are never read/edited.
        self.home_config = self.home / "config.toml"
        self.home_config.write_text("[projects." + json.dumps(str(self.project)) + "]\ntrust_level = 'trusted'\n")
        self.trust_before = self.home_config.read_bytes()
        own = {"type": "command", "command": "spyc --report-status done"}
        user = {"type": "command", "command": "printf SPYC_USER_HOOK", "timeout": 7}
        if self.args.scenario.startswith("json"):
            if self.args.scenario == "json_groups":
                groups = [{"hooks": [own]}, {"hooks": [user]}]
            else:
                groups = [{"hooks": [own, user]}]
            (config_dir / "hooks.json").write_text(json.dumps({"hooks": {"Stop": groups}}, indent=2) + "\n")
        elif self.args.scenario == "safe":
            (config_dir / "hooks.json").write_text(json.dumps({"hooks": {"Stop": [{"hooks": [user, own]}]}}, indent=2) + "\n")
        else:
            separate = "[[hooks.Stop]]\n" if self.args.scenario == "toml_groups" else ""
            (config_dir / "config.toml").write_text(
                "[[hooks.Stop]]\n[[hooks.Stop.hooks]]\ntype = 'command'\ncommand = 'spyc --report-status done'\n"
                + separate + "[[hooks.Stop.hooks]]\ntype = 'command'\ncommand = 'printf SPYC_USER_HOOK'\ntimeout = 7\n")
        self.before = probe.discover(native, self.project, self.home, self.output, "before")
        self.native = native
        launcher = self.output / "codex"
        child = "import os; print('SPYC_HOOK_IDENTITY_READY', flush=True); " \
                "exec('while os.read(0, 1):\\n pass')"
        launcher.write_text("#!/bin/sh\nexec " + shlex.join([sys.executable, "-c", child]) + "\n")
        launcher.chmod(0o700)
        manifest = {"binary": str(binary), "sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                    "version": subprocess.check_output([str(binary), "--version"], text=True).strip(),
                    "native_codex": str(native), "native_version": subprocess.check_output([str(native), "--version"], text=True).strip(),
                    "scenario": self.args.scenario, "session": self.args.session,
                    "coverage": "controlled PTY installer/cleanup; native read-only hooks/list",
                    "fixture_project_load_allowed": True, "hook_trust_approved": False}
        (self.output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
        sessions = json.loads(subprocess.check_output(["tui-test", "--json", "sessions"], text=True))["sessions"]
        assert self.args.session not in sessions, "refusing to reuse a session"
        path = os.pathsep.join([str(binary.parent), os.environ["PATH"]])
        self.cli("run", "--backend", "ghostty", "--cols", "600", "--rows", "60",
                 "--cwd", str(self.project), "--env", "SHELL=/bin/sh", "--env", "PATH=" + path,
                 "--env", "SPYC_PANE_CMD=" + str(launcher), "--env", "COLORTERM=truecolor",
                 "--env", "SPYC_MCP_SOCK=", "--env", "SPYC_PANE_ID=",
                 "--env", "XDG_STATE_HOME=" + str(self.output / "state"),
                 str(binary), "--status-trace", "--no-lua")
        self.started = True
        self.cli("record", "start", str(self.output / "session.cast"), "--format", "cast")
        self.wait("🌶️", 15000)
        # Consent only to spyc editing this disposable project's declarations.
        self.cli("type", ":hooks on")
        self.key("Enter")
        self.key("Ctrl+a", "c")
        self.wait("pane command:", 5000)
        self.key("Enter")
        self.wait("pane cwd:", 5000)
        self.key("Enter")
        self.wait("SPYC_HOOK_IDENTITY_READY", 15000)
        time.sleep(0.5)
        installed = probe.discover(native, self.project, self.home, self.output, "after-install")
        assert installed == self.before, f"installation changed user identity: {self.before} -> {installed}"
        dump = self.dump("installation")
        if self.args.scenario != "safe":
            assert "would move user hook trust identities" in dump, dump
        self.key("Ctrl+a", "k")
        self.cli("type", ":hooks off")
        self.key("Enter")
        self.key("Ctrl+a", "j")
        disabled = probe.discover(native, self.project, self.home, self.output, "after-disable")
        assert disabled == self.before, f"cleanup changed user identity: {self.before} -> {disabled}"
        self.capture("disabled")
        assert self.home_config.read_bytes() == self.trust_before, "fixture trust configuration was edited"

    def after_close(self):
        closed = probe.discover(self.native, self.project, self.home, self.output, "after-close")
        assert closed == self.before, f"teardown changed user identity: {self.before} -> {closed}"
        assert self.home_config.read_bytes() == self.trust_before


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--scenario", choices=("json_shared", "json_groups", "toml_shared", "toml_groups", "safe"), default="json_shared")
    parser.add_argument("--session", default="spyc-hook-trust-" + uuid.uuid4().hex[:10])
    args = parser.parse_args()
    smoke = IdentitySmoke(args)
    result = {"passed": False, "coverage": "installer/cleanup and native hook identity; hook execution not tested"}
    try:
        smoke.run()
        smoke.cli("record", "stop", check=False)
        smoke.cli("close")
        smoke.started = False
        smoke.after_close()
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
