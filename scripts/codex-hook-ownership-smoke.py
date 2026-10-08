#!/usr/bin/env python3
"""tui-test regression for borrowed reporters and per-agent teardown ownership.

Runs controlled PTY children, never native agents or hooks. Every host quits
through :q and wait exit before any assertions about teardown are made.
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

spec = importlib.util.spec_from_file_location("codex_smoke", Path(__file__).with_name("codex-tui-smoke.py"))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class OwnershipSmoke(module.Smoke):
    def setup(self):
        self.project = self.output / "project"
        self.project.mkdir()
        self.env = os.environ.copy()
        for name in ("GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_COMMON_DIR", "GIT_NAMESPACE",
                     "GIT_OBJECT_DIRECTORY", "GIT_ALTERNATE_OBJECT_DIRECTORIES", "GIT_PREFIX"):
            self.env.pop(name, None)
        self.env["GIT_CONFIG_GLOBAL"] = "/dev/null"
        self.env["GIT_CONFIG_SYSTEM"] = "/dev/null"
        self.git("init", "--quiet")
        self.git("config", "user.name", "spyc fixture")
        self.git("config", "user.email", "spyc-fixture@example.invalid")
        self.git("config", "core.hooksPath", str(self.output / "no-git-hooks"))
        self.hosts = [self]
        self.home = self.output / "home"
        self.home.mkdir()
        # Isolate host config; status hooks still use the live spyc socket.
        (self.home / ".spycrc.toml").write_text("[pane]\ncodex_mcp = false\n")
        self.config = self.project / ".codex/config.toml"
        self.legacy = self.project / ".codex/hooks.json"
        self.claude = self.project / ".claude/settings.json"
        self.before = b'{"hooks":{"Stop":[{"hooks":[{"command":"spyc --report-status done"}]}]}}\n'
        self.snapshot = None
        if self.args.scenario in ("tracked", "malformed", "claude_only", "mcp_only"):
            self.legacy.parent.mkdir()
            self.legacy.write_bytes(self.before)
        if self.args.scenario in ("tracked", "malformed"):
            self.snapshot = b"model = 'preserve'\n" if self.args.scenario == "tracked" else b"{broken\n"
            self.config.write_bytes(self.snapshot)
        if self.args.scenario == "tracked":
            self.git("add", ".codex/config.toml")
            self.git("commit", "--quiet", "-m", "fixture: tracked config")
        if self.args.scenario in ("symlink", "dangling_symlink"):
            self.legacy.parent.mkdir()
            self.link_target = self.output / "shared-hooks.json"
            if self.args.scenario == "symlink":
                self.link_target.write_bytes(self.before)
            self.legacy.symlink_to(self.link_target)
        if self.args.scenario in ("mode_640", "mode_644"):
            self.legacy.parent.mkdir()
            self.user_json = {"user-setting": "keep", "hooks": {"Stop": [{"hooks": [{"command": "printf user-hook"}]}]}}
            existing = json.loads(json.dumps(self.user_json))
            existing["hooks"]["Stop"].append({"hooks": [{"command": "spyc --report-status done"}]})
            self.legacy.write_text(json.dumps(existing) + "\n")
            self.expected_mode = int(self.args.scenario.removeprefix("mode_"), 8)
            self.legacy.chmod(self.expected_mode)
        binary = self.args.binary.resolve()
        manifest = {"binary": str(binary), "sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                    "version": subprocess.check_output([str(binary), "--version"], text=True).strip(),
                    "scenario": self.args.scenario, "session": self.args.session,
                    "coverage": "controlled PTY, actual spyc installation and graceful teardown",
                    "native_agent_execution": False, "hook_execution": False, "hook_trust_approved": False}
        (self.output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")

    def git(self, *args):
        subprocess.run(["git", "-C", str(self.project), *args], env=self.env, check=True,
                       stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=15)

    def start_host(self, host, kind, consent=True):
        launcher = host.output / kind
        child = "import os; print('SPYC_OWNERSHIP_READY', flush=True); exec('while os.read(0, 1):\\n pass')"
        launcher.write_text("#!/bin/sh\nexec " + shlex.join([sys.executable, "-c", child]) + "\n")
        launcher.chmod(0o700)
        binary = self.args.binary.resolve()
        sessions = json.loads(subprocess.check_output(["tui-test", "--json", "sessions"], text=True))["sessions"]
        assert host.args.session not in sessions, "refusing to reuse a session"
        host.cli("run", "--backend", "ghostty", "--cols", "200", "--rows", "50",
                 "--cwd", str(self.project), "--env", "SHELL=/bin/sh",
                 "--env", "PATH=" + os.pathsep.join([str(binary.parent), os.environ["PATH"]]),
                 "--env", "SPYC_PANE_CMD=" + str(launcher), "--env", "COLORTERM=truecolor",
                 "--env", "SPYC_MCP_SOCK=", "--env", "SPYC_PANE_ID=",
                 "--env", "HOME=" + str(self.home),
                 "--env", "XDG_STATE_HOME=" + str(self.output / "state"),
                 "--env", "XDG_CONFIG_HOME=" + str(self.output / "config"),
                 str(binary), "--status-trace", "--no-lua")
        host.started = True
        host.cli("record", "start", str(host.output / "session.cast"), "--format", "cast")
        host.wait("🌶️", 15000)
        host.cli("type", ":hooks " + ("on" if consent else "off"))
        host.key("Enter")
        host.key("Ctrl+a", "c")
        host.wait("pane command:", 5000)
        host.key("Enter")
        host.wait("pane cwd:", 5000)
        host.key("Enter")
        host.wait("SPYC_OWNERSHIP_READY", 15000)
        time.sleep(0.3)
        host.dump("installed")

    def sibling(self, kind):
        args = argparse.Namespace(binary=self.args.binary, output=self.output / "sibling",
                                  session=self.args.session + "-sibling")
        sibling = module.Smoke(args)
        self.hosts.append(sibling)
        self.start_host(sibling, kind)
        return sibling

    @staticmethod
    def quit_host(host):
        host.key("Ctrl+a", "k")
        host.cli("type", ":q")
        host.key("Enter")
        exited = json.loads(host.cli("--json", "wait", "exit", "--timeout", "1000", check=False))
        if not exited.get("ok"):
            host.wait("press again to quit", 5000)
            host.capture("quit-confirmation")
            host.cli("type", ":q")
            host.key("Enter")
            exited = json.loads(host.cli("--json", "wait", "exit", "--timeout", "15000"))
        (host.output / "host-exit.json").write_text(json.dumps(exited, indent=2) + "\n")
        host.capture("exited")
        state = json.loads(host.cli("--json", "state"))
        assert state["data"]["exited"] == 0 and state["data"]["exit_signal"] is None, state
        (host.output / "host-state.json").write_text(json.dumps(state, indent=2) + "\n")
        host.cli("record", "stop", check=False)
        host.cli("close")
        host.started = False

    def unchanged(self):
        if self.args.scenario in ("symlink", "dangling_symlink"):
            assert self.legacy.is_symlink() and self.legacy.readlink() == self.link_target, "legacy symlink was replaced or unlinked"
            if self.args.scenario == "symlink":
                assert self.link_target.read_bytes() == self.before, "shared target changed"
            else:
                assert not self.link_target.exists(), "dangling target was created"
            assert not self.config.exists(), "refused legacy migration wrote canonical hooks"
            return
        assert self.legacy.exists() and self.legacy.read_bytes() == self.before, "teardown deleted or changed borrowed Codex reporters"
        if self.snapshot is not None:
            assert self.config.read_bytes() == self.snapshot, "refused configuration changed"

    def dump(self, name):
        if self.args.scenario not in ("symlink", "dangling_symlink"):
            return super().dump(name)
        self.key("Ctrl+a", "k")
        self.cli("type", ":activity dump")
        self.key("Enter")
        self.wait("activity dump", 5000)
        # Plain pagers wrap by default. The shared numbered-line extractor
        # omits continuation rows; assert against the actual wrapped screen.
        self.wait("link and target preserved", 5000)
        text = self.capture(name)
        self.key("q")
        self.key("Ctrl+a", "j")
        return text

    def check_mode(self, name):
        assert self.legacy.is_file() and not self.legacy.is_symlink(), "legacy source disappeared or changed type"
        mode = self.legacy.stat().st_mode & 0o777
        assert mode == self.expected_mode, f"legacy mode changed: {mode:o}, expected {self.expected_mode:o}"
        actual = json.loads(self.legacy.read_text())
        assert actual == self.user_json, actual
        (self.output / (name + "-legacy.json")).write_text(json.dumps({"mode": mode, "content": actual}, indent=2) + "\n")

    def run(self):
        self.setup()
        kind = "claude" if self.args.scenario == "claude_only" else (
            "agy" if self.args.scenario == "mcp_only" else "codex")
        self.start_host(self, kind, consent=self.args.scenario != "mcp_only")
        print(f"Started {self.args.scenario}: {self.args.session}", flush=True)
        if self.args.scenario in ("tracked", "malformed", "claude_only", "mcp_only", "symlink", "dangling_symlink"):
            self.unchanged()
            if self.args.scenario in ("symlink", "dangling_symlink"):
                diagnostic = self.dump("symlink-diagnostic")
                assert "is a symlink" in diagnostic and "link and target preserved" in diagnostic, diagnostic
            self.quit_host(self)
            self.unchanged()
            if kind == "claude":
                assert not self.claude.exists(), "managed Claude hooks were stranded"
        elif self.args.scenario in ("mode_640", "mode_644"):
            assert self.config.exists() and b"--report-status" in self.config.read_bytes()
            self.check_mode("installed")
            self.quit_host(self)
            self.check_mode("exited")
            assert not self.config.exists(), "managed canonical hooks were stranded"
        else:
            assert self.config.exists() and b"--report-status" in self.config.read_bytes()
            if self.args.scenario == "borrowed_last":
                self.legacy.write_bytes(self.before)
                self.snapshot = self.config.read_bytes()
                self.git("add", ".codex/config.toml")
                self.git("commit", "--quiet", "-m", "fixture: tracked config")
            sibling = self.sibling("claude" if self.args.scenario == "mixed" else "codex")
            self.quit_host(self)
            if self.args.scenario == "mixed":
                assert not self.config.exists() or b"--report-status" not in self.config.read_bytes(), "Codex cleanup was deferred to an unrelated agent"
                assert self.claude.exists(), "live sibling's Claude hooks disappeared"
            else:
                assert b"--report-status" in self.config.read_bytes(), "live Codex sibling lost its reporters"
            self.quit_host(sibling)
            if self.args.scenario == "borrowed_last":
                self.unchanged()
            else:
                assert not self.config.exists() or b"--report-status" not in self.config.read_bytes(), "last managed owner failed to clean Codex hooks"
                assert not self.claude.exists(), "last managed owner failed to clean Claude hooks"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--scenario", choices=("tracked", "malformed", "claude_only", "mcp_only", "shared", "mixed", "borrowed_last", "symlink", "dangling_symlink", "mode_640", "mode_644"), default="tracked")
    parser.add_argument("--session", default="spyc-hook-ownership-" + uuid.uuid4().hex[:10])
    smoke = OwnershipSmoke(parser.parse_args())
    result = {"passed": False}
    try:
        smoke.run()
        result["passed"] = True
    except Exception as error:
        result["error"] = str(error)
        print(str(error), file=sys.stderr, flush=True)
        for host in getattr(smoke, "hosts", [smoke]):
            if host.started:
                host.capture("failure")
    finally:
        for host in getattr(smoke, "hosts", [smoke]):
            if host.started:
                host.cli("record", "stop", check=False)
                host.cli("close", check=False)
        (smoke.output / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    print(f"{'PASS' if result['passed'] else 'FAIL'}: {smoke.output}", flush=True)
    return 0 if result["passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
