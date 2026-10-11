#!/usr/bin/env python3
"""Film spyc's agent dots: four panes in four states, and the one that needs you.

Drives spyc in a dedicated tui-test session, records the take as an asciicast
and renders it to a GIF with agg. Three tabs run agent.py, a stand-in that
reports over spyc's real MCP server; the fourth runs rmatrix and is suspended.

    python3 demo.py --binary /abs/path/to/spyc
    python3 demo.py --binary /abs/path/to/spyc --gif ../../demo-agents.gif
"""

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time
import uuid

HERE = Path(__file__).resolve().parent
FIXTURE = HERE.parent / "markdown" / "aurora-docs"
COLS, ROWS = 120, 32
BLOCKED_FG = "#f7768e"
SUSPENDED = "\U0001f4a4"
# Catppuccin Macchiato, the palette the vhs-recorded README GIFs use.
THEME = ("24273a,cad3f5,494d64,ed8796,a6da95,eed49f,8aadf4,f5bde6,8bd5ca,b8c0e0,"
         "5b6078,ed8796,a6da95,eed49f,8aadf4,f5bde6,8bd5ca,a5adcb")
FONTS = "MesloLGL Nerd Font Mono,JetBrains Mono,SF Mono,Menlo,DejaVu Sans Mono"


class Demo:
    def __init__(self, args):
        self.args = args
        self.base = ["tui-test", "--session", args.session]
        self.output = args.output.resolve()
        self.output.mkdir(parents=True, exist_ok=False)
        self.log = self.output / "steps.jsonl"
        self.started = False

    def cli(self, *args, check=True):
        result = subprocess.run(self.base + list(args), capture_output=True, text=True, timeout=65)
        with self.log.open("a") as out:
            out.write(json.dumps({"time": time.time(), "args": args,
                                  "exit": result.returncode, "stderr": result.stderr}) + "\n")
        if check and result.returncode:
            raise AssertionError(f"tui-test {args}: {result.stderr.strip()}")
        return result.stdout

    def wait(self, text, timeout=10000, *extra):
        self.cli("expect", "text", text, "--match", "any", "--timeout", str(timeout), *extra)

    def key(self, *keys):
        self.cli("key", "press", *keys)

    def chord(self, key):
        self.key("Ctrl+a", key)
        time.sleep(0.4)

    def capture(self, name):
        text = self.cli("text")
        (self.output / f"{name}.txt").write_text(text)
        return text

    # ---------------------------------------------------------------- reading
    def dot(self, tab):
        """The glyph after `[tab]` in the divider, and its colour.

        Blocked and done are both `■`; only the colour separates "needs you"
        from "finished", so a glyph match alone cannot assert either.
        """
        found = json.loads(self.cli("--json", "find", "text", f"[{tab}]"))["data"]["matches"]
        for match in found:
            row, column = match["start"]["row"], match["end"]["column"]
            lead = json.loads(self.cli("--json", "cells", "0", str(row), "1", "1"))["data"]["cells"]
            if lead and lead[0]["char"] == "─":
                cell = json.loads(self.cli("--json", "cells", str(column), str(row), "1", "1"))
                cell = cell["data"]["cells"][0]
                return cell["char"], cell["fg"].lower()
        return None, None

    def wait_dot(self, tab, want, timeout=10):
        deadline = time.monotonic() + timeout
        while True:
            glyph, fg = self.dot(tab)
            if want == "working" and glyph == "●":
                return
            if want == "blocked" and glyph == "■" and fg == BLOCKED_FG:
                return
            if want == "done" and glyph == "■" and fg != BLOCKED_FG:
                return
            if want == "suspended" and glyph is not None and glyph.startswith(SUSPENDED):
                return
            if want == "running" and glyph is not None and not glyph.startswith(SUSPENDED):
                return
            if time.monotonic() > deadline:
                self.capture(f"no-{want}-tab{tab}")
                raise AssertionError(f"tab {tab} never showed {want}: got {glyph!r} {fg}")
            time.sleep(0.2)

    def dump(self, name):
        """`:activity dump` names every pane's state in words. Never on camera."""
        self.chord("k")
        self.cli("type", ":activity dump")
        self.key("Enter")
        self.wait("activity dump", 5000)
        # Four panes overflow one screen, so read it a page at a time.
        pages = []
        for page in range(12):
            pages.append(self.capture(f"{name}-{page}"))
            frame_end = [line for line in pages[-1].splitlines() if "┘" in line]
            if not frame_end or " Bot " in frame_end[-1] or " All " in frame_end[-1]:
                break
            self.key("PageDown")
            time.sleep(0.2)
        self.key("q")
        time.sleep(0.4)
        return "\n".join(pages)

    # ---------------------------------------------------------------- setup
    def stage(self):
        stage = self.args.stage
        if stage.exists():
            shutil.rmtree(stage)
        home = stage / "home"
        (home / ".config").mkdir(parents=True)
        (stage / "bin").mkdir()
        shutil.copytree(FIXTURE, home / "aurora-docs")
        # desktop = false: a Done would otherwise ping the recorder's own desktop.
        (home / ".spycrc.toml").write_text("[notify]\ndesktop = false\nvisual = true\n")
        shim = stage / "bin" / "claude"
        shim.write_text(f'#!/bin/sh\nexec "{sys.executable}" "{HERE / "agent.py"}" "$@"\n')
        shim.chmod(0o755)
        self.stage_dir = stage.resolve()
        self.home = self.stage_dir / "home"
        self.trigger = self.stage_dir / "block-now"

    def launch(self):
        binary = self.args.binary.resolve()
        sessions = json.loads(subprocess.check_output(["tui-test", "--json", "sessions"], text=True))
        assert self.args.session not in sessions["sessions"], "refusing to reuse a session"
        path = os.pathsep.join([str(self.stage_dir / "bin"), str(binary.parent), os.environ["PATH"]])
        home = str(self.home)
        # SPYC_MCP_SOCK / SPYC_PANE_ID are blanked so that, run from inside a
        # spyc pane, the demo can never report to the spyc you are using.
        self.cli("run", "--backend", "ghostty", "--cols", str(COLS), "--rows", str(ROWS),
                 "--cwd", str(self.home / "aurora-docs"),
                 "--env", "HOME=" + home,
                 "--env", "XDG_CONFIG_HOME=" + home + "/.config",
                 "--env", "XDG_STATE_HOME=" + home + "/.local/state",
                 "--env", "SHELL=/bin/sh", "--env", "PATH=" + path,
                 "--env", "COLORTERM=truecolor", "--env", "SPYC_PANE_CMD=claude",
                 "--env", "SPYC_MCP_SOCK=", "--env", "SPYC_PANE_ID=",
                 "--env", "SPYC_DEMO_BIN=" + str(binary),
                 "--env", "SPYC_DEMO_TRIGGER=" + str(self.trigger),
                 str(binary), "--no-lua")
        self.started = True
        self.wait("CONTRIBUTING.md", 15000)

    def new_tab(self, command):
        self.chord("c")
        self.wait("pane command:", 5000)
        # `^a c` prefills the default command; typing on top of it would
        # spawn `claudeclaude`.
        self.key("Ctrl+u")
        self.cli("type", command)
        self.key("Enter")
        self.wait("pane cwd:", 5000)
        self.key("Enter")

    def answer_consent(self):
        # Modal, and only y/n close it: anything else typed is swallowed while
        # it stays up. A fresh state dir asks on the first agent pane.
        try:
            self.wait("spyc — agent status", 6000)
        except AssertionError:
            return
        self.cli("type", "y")
        self.wait("spyc — agent status", 5000, "--not")

    def arrange(self):
        self.new_tab("claude worker")
        self.answer_consent()
        self.wait_dot(1, "working")
        self.new_tab("claude blocker")
        self.wait_dot(2, "working")
        self.new_tab("claude done")
        self.wait_dot(3, "done")
        # rmatrix draws at 30fps by default; the GIF keeps at most 12.
        self.new_tab("rmatrix --fps 15")
        self.wait_dot(4, "running")
        dump = self.dump("arranged")
        for line in ('[1] "claude"  dot=working', '[2] "claude"  dot=working',
                     '[3] "claude"  dot=done'):
            assert line in dump, f"arrangement is missing {line!r}"

    # ---------------------------------------------------------------- beats
    def film(self):
        self.chord("4")
        self.cli("record", "start", str(self.output / "take.cast"), "--format", "cast")
        time.sleep(0.6)

        # Cold open on the rain, zoomed. Suspending while zoomed freezes it on
        # camera, so the unzoom reveals the divider with the 💤 already set.
        self.chord("z")
        time.sleep(1.6)
        self.key("Ctrl+z")
        time.sleep(0.8)
        self.chord("z")
        self.wait_dot(4, "suspended")
        time.sleep(1.0)

        # The glance: back in the list with the worker on screen. Every tab's
        # state reads from the divider without opening any of them.
        self.chord("1")
        self.chord("k")
        for key in ("j", "j", "k"):
            time.sleep(0.6)
            self.key(key)
        time.sleep(1.4)

        # The one that needs you: red square, and the border pulses.
        self.trigger.touch()
        self.wait_dot(2, "blocked")
        time.sleep(2.8)

        # Answer it. The agent finishes and moves the file list over MCP.
        self.chord("2")
        self.wait("[y/N]", 5000)
        time.sleep(1.2)
        self.cli("type", "y")
        time.sleep(0.3)
        self.key("Enter")
        self.wait("aurora-docs/guides", 10000)
        self.wait_dot(2, "done")
        time.sleep(2.2)

        # Wake the rain to close; the loop restarts on it.
        self.chord("4")
        self.key("Ctrl+z")
        self.wait_dot(4, "running")
        time.sleep(1.5)
        self.cli("record", "stop")

        dump = self.dump("filmed")
        for line in ('[1] "claude"  dot=working', '[2] "claude"  dot=done',
                     '[3] "claude"  dot=done'):
            assert line in dump, f"final state is missing {line!r}"

    def render(self):
        gif = self.output / "take.gif"
        subprocess.run(["agg", "--text-font-family", FONTS, "--font-size", "16",
                        "--theme", THEME, "--idle-time-limit", "2", "--fps-cap", "12",
                        str(self.output / "take.cast"), str(gif)],
                       check=True, capture_output=True)
        if self.args.gif:
            shutil.copyfile(gif, self.args.gif)
        return gif

    def run(self):
        self.stage()
        self.launch()
        self.arrange()
        if self.args.setup:
            print(f"arranged; attach with: tui-test --session {self.args.session} monitor")
            return
        self.film()
        gif = self.render()
        print(f"{gif} ({gif.stat().st_size // 1024} KiB)")


def preflight():
    missing = [tool for tool in ("tui-test", "agg", "rmatrix") if shutil.which(tool) is None]
    hints = {"tui-test": "brew tap microsoft/tui-test && brew install tui-test",
             "agg": "brew install agg", "rmatrix": "cargo install rmatrix-reloaded"}
    if missing:
        sys.exit("missing: " + "; ".join(f"{t} ({hints[t]})" for t in missing))


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--binary", required=True, type=Path, help="the spyc to film")
    parser.add_argument("--gif", type=Path, help="also copy the rendered GIF here")
    parser.add_argument("--output", type=Path,
                        default=Path("/tmp/spyc-demo") / time.strftime("agents-%Y%m%d-%H%M%S"),
                        help="a new directory for the take, the cast and the step log")
    parser.add_argument("--stage", type=Path, default=Path("/tmp/spyc-demo-agents"),
                        help="scratch HOME and fixture, recreated every run")
    parser.add_argument("--session", default="spyc-demo-agents-" + uuid.uuid4().hex[:8])
    parser.add_argument("--setup", action="store_true",
                        help="arrange the four panes and leave the session running, unfilmed")
    args = parser.parse_args()
    preflight()
    demo = Demo(args)
    ok = False
    try:
        demo.run()
        ok = True
    except Exception as error:
        if demo.started:
            demo.capture("failure")
        print(f"FAIL: {error}", file=sys.stderr)
    finally:
        # Close only this session: `close --all` would take other agents' runs.
        if demo.started and not (ok and args.setup):
            demo.cli("record", "stop", check=False)
            demo.cli("close", check=False)
    print(f"{'PASS' if ok else 'FAIL'}: {demo.output}")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
