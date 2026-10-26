#!/usr/bin/env python3
"""Which right-clicks does this terminal pass to the application?

Turns on the mouse modes spyc uses (?1000h ?1002h ?1006h, plus bracketed
paste), then walks through plain / Shift / Option(Alt) / Control / Command
right-clicks and records what reached the application for each: a right
press (with the modifier bits the terminal encoded), a paste, or nothing.
docs/drafts/UX_2_0_PLAN.md section 0 holds the results so far.

Run it directly in the terminal under test, not inside a spyc pane or tmux,
which would intercept the clicks.

    python3 scripts/rightclick-probe.py [OUTDIR]

Prints a Markdown table on exit; with OUTDIR, also writes it there as
rightclick-<terminal>-<time>.md. Stdlib only, macOS and Linux (WSL).
"""

import datetime
import os
import platform
import re
import select
import sys
import termios
import tty

ESC = "\x1b"
ON = ESC + "[?1049h" + ESC + "[?1000h" + ESC + "[?1002h" + ESC + "[?1006h" + ESC + "[?2004h"
OFF = ESC + "[?2004l" + ESC + "[?1006l" + ESC + "[?1002l" + ESC + "[?1000l" + ESC + "[?1049l"

SGR_MOUSE = re.compile(rb"\x1b\[<(\d+);(\d+);(\d+)([Mm])")
X10_MOUSE = re.compile(rb"\x1b\[M(.)(.)(.)", re.S)
PASTE = re.compile(rb"\x1b\[200~(.*?)\x1b\[201~", re.S)
XTVERSION = re.compile(rb"\x1bP>\|(.*?)\x1b\\", re.S)


def is_mac():
    return platform.system() == "Darwin"


def is_wsl():
    return "microsoft" in platform.release().lower()


def steps():
    alt = "Option (⌥)" if is_mac() else "Alt"
    out = [
        ("plain right-click", "Right-click with NO modifier held."),
        ("⇧ right-click", "Hold Shift (⇧) and right-click."),
        (f"{alt} right-click", f"Hold {alt} and right-click."),
        ("⌃ right-click", "Hold Control (⌃) and right-click."),
    ]
    if is_mac():
        out.append(("⌘ right-click", "Hold Command (⌘) and right-click."))
    return out


def decode_button(b):
    mods = [name for bit, name in ((4, "shift"), (8, "alt/meta"), (16, "ctrl")) if b & bit]
    if b & 64:
        return "wheel", mods
    motion = bool(b & 32)
    button = {0: "left", 1: "middle", 2: "right", 3: "none"}[b & 3]
    return ("drag-" if motion else "") + button, mods


def terminal_identity(xtversion):
    env = os.environ
    parts = []
    for key in ("TERM_PROGRAM", "TERM_PROGRAM_VERSION", "LC_TERMINAL", "LC_TERMINAL_VERSION", "TERM"):
        if env.get(key):
            parts.append(f"{key}={env[key]}")
    if env.get("WT_SESSION"):
        parts.append("Windows Terminal (WT_SESSION set)")
    if env.get("KITTY_WINDOW_ID"):
        parts.append("kitty (KITTY_WINDOW_ID set)")
    if xtversion:
        parts.append(f"XTVERSION={xtversion}")
    if is_mac():
        osname = f"macOS {platform.mac_ver()[0]}"
    elif is_wsl():
        osname = f"WSL ({platform.release()})"
    else:
        osname = f"{platform.system()} {platform.release()}"
    return osname, parts


def slug(xtversion):
    env = os.environ
    name = (
        env.get("LC_TERMINAL")
        or env.get("TERM_PROGRAM")
        or ("WindowsTerminal" if env.get("WT_SESSION") else "")
        or (xtversion.split()[0] if xtversion else "")
        or env.get("TERM", "unknown")
    )
    return re.sub(r"[^A-Za-z0-9_.-]+", "_", name)


class Probe:
    def __init__(self):
        self.fd = sys.stdin.fileno()
        self.buf = b""
        self.steps = steps()
        self.results = [None] * len(self.steps)
        self.noise = [[] for _ in self.steps]
        self.index = 0
        self.xtversion = ""

    def write(self, s):
        os.write(sys.stdout.fileno(), s.encode())

    def read(self, timeout):
        ready, _, _ = select.select([self.fd], [], [], timeout)
        if ready:
            self.buf += os.read(self.fd, 4096)
            return True
        return False

    def query_xtversion(self):
        self.write(ESC + "[>0q")
        deadline = datetime.datetime.now() + datetime.timedelta(seconds=0.4)
        while datetime.datetime.now() < deadline:
            if not self.read(0.05):
                continue
            m = XTVERSION.search(self.buf)
            if m:
                self.xtversion = m.group(1).decode(errors="replace")
                self.buf = self.buf[: m.start()] + self.buf[m.end():]
                return

    def draw(self, flash=""):
        out = [ESC + "[H" + ESC + "[2J"]
        out.append("spyc right-click probe  (n = nothing reached me / the terminal showed its own menu,\r\n")
        out.append("                         b = redo previous step, q = quit)\r\n\r\n")
        for i, (label, _) in enumerate(self.steps):
            mark = "→" if i == self.index else " "
            res = self.results[i] or ""
            noise = f"   (also saw: {', '.join(self.noise[i])})" if self.noise[i] else ""
            out.append(f" {mark} {label:<22} {res}{noise}\r\n")
        out.append("\r\n")
        if self.index < len(self.steps):
            out.append(ESC + "[1m" + self.steps[self.index][1] + ESC + "[0m\r\n")
            out.append("(If a menu or a paste appears instead, dismiss it with Esc, then press n.)\r\n")
        if flash:
            out.append("\r\n" + flash + "\r\n")
        self.write("".join(out))

    def record(self, result):
        self.results[self.index] = result
        self.index += 1

    def handle_events(self):
        """Consume complete sequences from the buffer. Returns False to quit."""
        while self.buf:
            m = PASTE.match(self.buf)
            if m:
                self.buf = self.buf[m.end():]
                if self.index < len(self.steps):
                    self.record(f"PASTE ({len(m.group(1))} bytes; content not shown)")
                continue
            if self.buf.startswith(b"\x1b[200~"):
                return True  # wait for the paste's end
            m = SGR_MOUSE.match(self.buf)
            if m:
                self.buf = self.buf[m.end():]
                self.mouse(int(m.group(1)), m.group(4) == b"M")
                continue
            m = X10_MOUSE.match(self.buf)
            if m:
                self.buf = self.buf[m.end():]
                b = m.group(1)[0] - 32
                self.mouse(b, (b & 3) != 3, legacy=True)
                continue
            if self.buf.startswith(b"\x1b"):
                if len(self.buf) < 3 or re.match(rb"\x1b\[<[\d;]*$", self.buf):
                    return True  # incomplete sequence, wait for more
                # An unrecognized escape sequence: drop it, note it.
                end = re.match(rb"\x1b(\[[\x30-\x3f]*[\x20-\x2f]*[\x40-\x7e]|.)", self.buf, re.S)
                cut = end.end() if end else 1
                seq = self.buf[:cut]
                self.buf = self.buf[cut:]
                if self.index < len(self.steps) and seq != b"\x1b":
                    self.noise[self.index].append(f"esc seq {seq[1:]!r}")
                continue
            chunk, self.buf = self.buf, b""
            if len(chunk) > 1:
                # Several bytes in one read without bracketing: a raw paste.
                if self.index < len(self.steps):
                    self.record(f"TEXT ({len(chunk)} bytes, unbracketed paste?)")
                continue
            key = chunk.decode(errors="replace")
            if key in ("q", "\x03"):
                return False
            if self.index >= len(self.steps):
                continue
            if key == "n":
                self.record("nothing reached the app")
            elif key == "b" and self.index > 0:
                self.index -= 1
                self.results[self.index] = None
                self.noise[self.index] = []
        return True

    def mouse(self, b, press, legacy=False):
        if self.index >= len(self.steps):
            return
        button, mods = decode_button(b)
        tag = "legacy X10 " if legacy else ""
        desc = f"{tag}{button} press" + (f" + {'+'.join(mods)}" if mods else ", no modifier bits")
        if not press:
            return
        if button == "right":
            self.record(desc)
        else:
            self.noise[self.index].append(desc)

    def run(self):
        old = termios.tcgetattr(self.fd)
        try:
            tty.setraw(self.fd)
            self.write(ON)
            self.query_xtversion()
            self.draw()
            while self.index < len(self.steps):
                self.read(None)
                if not self.handle_events():
                    break
                self.draw()
            if self.index >= len(self.steps):
                self.draw("Done. Press any key to leave.")
                self.read(None)
        finally:
            self.write(OFF)
            termios.tcsetattr(self.fd, termios.TCSADRAIN, old)

    def report(self):
        osname, ident = terminal_identity(self.xtversion)
        lines = [
            f"# Right-click probe: {slug(self.xtversion)}",
            "",
            f"- when: {datetime.datetime.now().isoformat(timespec='seconds')}",
            f"- os: {osname}",
            f"- terminal: {'; '.join(ident) if ident else 'unidentified'}",
            "",
            "| gesture | reached the app as |",
            "| --- | --- |",
        ]
        for (label, _), res, noise in zip(self.steps, self.results, self.noise):
            extra = f" (also saw: {', '.join(noise)})" if noise else ""
            lines.append(f"| {label} | {res or 'not tested'}{extra} |")
        return "\n".join(lines) + "\n"


def main():
    if os.environ.get("SPYC_PANE_ID"):
        sys.exit("You are inside a spyc pane, which intercepts right-click. Run this in a bare terminal window.")
    if os.environ.get("TMUX"):
        sys.exit("You are inside tmux, which intercepts mouse reports. Run this in a bare terminal window.")
    if not sys.stdin.isatty():
        sys.exit("stdin is not a terminal.")
    outdir = sys.argv[1] if len(sys.argv) > 1 else None
    probe = Probe()
    probe.run()
    text = probe.report()
    print(text)
    if outdir is None:
        return
    name = f"rightclick-{slug(probe.xtversion)}-{datetime.datetime.now():%Y%m%d-%H%M%S}.md"
    path = os.path.join(outdir, name)
    try:
        with open(path, "w", encoding="utf-8") as f:
            f.write(text)
        print(f"Saved to {path}")
    except OSError as e:
        print(f"Could not save to {path}: {e}; copy the table above instead.")


if __name__ == "__main__":
    main()
