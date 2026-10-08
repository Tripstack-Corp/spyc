#!/usr/bin/env python3
"""Verify spyc stays responsive around non-reading and noisy PTY children."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--session", required=True)
    parser.add_argument("--scenario", choices=("pane", "capture", "firehose", "resume", "pipe"), required=True)
    args = parser.parse_args()
    binary = args.binary.resolve()
    root = args.output.resolve()
    root.mkdir(parents=True, exist_ok=False)
    base = ["tui-test", "--session", args.session]
    started = False
    child_pid = None

    def cli(*command, check=True):
        result = subprocess.run(base + list(command), text=True, capture_output=True, timeout=20)
        logged = command if command[0] != "write" else ("write", "<synthetic bracketed paste>")
        with (root / "steps.jsonl").open("a") as out:
            out.write(json.dumps({"args": logged, "exit": result.returncode, "stderr": result.stderr}) + "\n")
        if check and result.returncode:
            raise AssertionError(f"tui-test {logged}: {result.stderr}")
        return result.stdout, result.returncode

    def wait(text, timeout=5000):
        cli("expect", "text", text, "--match", "any", "--timeout", str(timeout))

    def key(*keys):
        cli("key", "press", *keys)

    def capture(name):
        text, _ = cli("text")
        (root / f"{name}.txt").write_text(text)
        return text

    def launch(resume=False):
        nonlocal started
        command = [str(binary), "--no-lua"]
        if resume:
            command.append("-r")
        cli("run", "--backend", "ghostty", "--cols", "120", "--rows", "35",
            "--cwd", str(root), "--env", "SHELL=/bin/sh",
            "--env", "XDG_STATE_HOME=" + str(root / "state"),
            "--env", "SPYC_MCP_SOCK=", "--env", "SPYC_PANE_ID=",
            "--env", "SPYC_PANE_CMD=python3 " + str(child), *command)
        started = True
        wait(root.name)

    def open_pane():
        key("Ctrl+a", "c")
        wait("pane command:")
        key("Enter")
        wait("pane cwd:")
        key("Enter")
        wait("PIPE_READER_READY" if args.scenario == "pipe" else
             "OUTPUT_FIREHOSE" if args.scenario == "firehose" else "NON_READING_CHILD_READY")

    def activity():
        key("Ctrl+a", "k")
        cli("type", ":activity dump")
        key("Enter")
        wait("activity dump", 3000)
        capture("responsive")
        key("q")
        key("Ctrl+a", "j")

    child = root / "child.py"
    code = ("import os,sys,time,tty,subprocess\nfrom pathlib import Path\n"
            "tty.setraw(sys.stdin.fileno())\n"
            f"Path({str(root / 'child-pid')!r}).write_text(str(os.getpid()))\n"
            "host=os.getppid()\n"
            "for _ in range(8):\n"
            " name=subprocess.check_output(['ps','-o','comm=','-p',str(host)],text=True).strip()\n"
            " if Path(name).name == 'spyc': break\n"
            " host=int(subprocess.check_output(['ps','-o','ppid=','-p',str(host)],text=True))\n"
            "else: raise RuntimeError('test spyc ancestor not found')\n"
            f"Path({str(root / 'host-pid')!r}).write_text(str(host))\n")
    expected_pipe = None
    if args.scenario == "pipe":
        source = root / "large.txt"
        source.write_bytes(b"x" * (9 * 1_048_576))
        expected_pipe = b"\x1b[200~" + f"[file: {source}]\n".encode() + source.read_bytes() + b"\x1b[201~"
        code += ("print('PIPE_READER_READY',flush=True)\n"
                 "received=bytearray()\n"
                 f"while len(received)<{len(expected_pipe)}:\n"
                 " received.extend(os.read(0,65536))\n"
                 f"Path({str(root / 'received.bin')!r}).write_bytes(received)\n"
                 "print('PIPE_RECEIVED',flush=True)\ntime.sleep(120)\n")
    elif args.scenario == "firehose":
        code += ("end=time.monotonic()+45\n"
                 "while time.monotonic()<end:\n"
                 " os.write(1,b'OUTPUT_FIREHOSE '+b'x'*120+b'\\r\\n')\n")
    else:
        code += "print('NON_READING_CHILD_READY',flush=True)\ntime.sleep(120)\n"
    child.write_text(code)
    manifest = {"binary": str(binary), "sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                "version": subprocess.check_output([str(binary), "--version"], text=True).strip(),
                "scenario": args.scenario, "session": args.session}
    (root / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    result = {"passed": False}
    try:
        sessions = subprocess.check_output(["tui-test", "--json", "sessions"], text=True)
        assert args.session not in sessions, "refusing to reuse an existing session"
        launch()
        if args.scenario == "capture":
            cli("type", "!python3 " + str(child))
            key("Enter")
            wait("NON_READING_CHILD_READY")
        else:
            open_pane()
        child_pid = int((root / "child-pid").read_text())
        if args.scenario == "resume":
            # Save a session through normal quit, then restore only this test's
            # isolated state directory. The replacement child has a new PID.
            key("Ctrl+a", "k")
            cli("type", ":q")
            key("Enter")
            wait("running process")
            cli("type", ":q")
            key("Enter")
            cli("wait", "exit", "--timeout", "5000")
            child_pid = None
            cli("close")
            started = False
            (root / "child-pid").unlink()
            launch(resume=True)
            wait("sessions — j/k navigate")
            key("1")
            wait("NON_READING_CHILD_READY")
            child_pid = int((root / "child-pid").read_text())
            key("Ctrl+a", "j")
        capture("ready")
        if args.scenario == "pipe":
            key("Ctrl+a", "k")
            cli("type", ":limit large.txt")
            key("Enter")
            key("Ctrl+a", "P")
            wait("Pipe 9.0 MiB")
            capture("confirmation")
            assert not (root / "received.bin").exists(), "content sent before confirmation"
            key("Enter")
            wait("pipe cancelled")
            assert not (root / "received.bin").exists(), "cancelled pipe sent content"
            key("Ctrl+a", "P")
            wait("Pipe 9.0 MiB")
            key("y")
            wait("PIPE_RECEIVED", 15000)
            assert (root / "received.bin").read_bytes() == expected_pipe, "pipe was truncated or reordered"
            capture("received")
            activity()
        elif args.scenario == "firehose":
            # Drive both input and scrollback while the parser consumes a
            # continuous byte stream. Each host command has a bounded wait.
            for _ in range(3):
                activity()
                key("Ctrl+a", "v")
                time.sleep(0.3)
                key("Esc")
            activity()
        else:
            if args.scenario != "capture":
                activity()
            cli("write", "\x1b[200~" + "x" * 65536 + "\x1b[201~")
            time.sleep(0.5)
            if args.scenario == "capture":
                # Ctrl-Z backgrounds the running capture without waiting for
                # its full input pipe, exposing the responsive file list.
                key("Ctrl+z")
                cli("type", ":activity dump")
                key("Enter")
                wait("activity dump", 3000)
                capture("responsive")
            else:
                activity()
                key("Ctrl+a", "z")
                key("Ctrl+a", "z")
                activity()
            assert os.kill(child_pid, 0) is None, "the host killed the non-reading child"
        result["passed"] = True
    except Exception as error:
        result["error"] = str(error)
        capture("failed")
        if (root / "host-pid").exists() and Path("/usr/bin/sample").exists():
            subprocess.run(["sample", (root / "host-pid").read_text(), "2", "10",
                            "-file", str(root / "blocked.sample.txt")], capture_output=True, timeout=15)
    finally:
        if child_pid:
            try:
                os.kill(child_pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        if started:
            cli("close", check=False)
        (root / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    print(f"{'PASS' if result['passed'] else 'FAIL'} {args.scenario}: {root}", flush=True)
    return 0 if result["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
