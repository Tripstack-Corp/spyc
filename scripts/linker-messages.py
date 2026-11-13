#!/usr/bin/env python3
"""Summarize the linker messages in a cargo build log, as Markdown. Never fails.

The build is run with `--force-warn linker_messages --force-warn linker_info`
in `RUSTFLAGS`, so every link reports, dependencies' build scripts and
proc-macros included. rustc files a linker's output under one of two lints:
`linker_messages` (warn by default) and `linker_info` (allow by default, for
output rustc considers informational). Both are listed.

A message is grouped with others of its kind once paths, hashes and archive
member indices are stripped, and counted. One that matches a name in
`FLAG_PATTERNS` is flagged: printed first, and as a GitHub `::warning`
annotation. Flagging is a report, not a gate; the script exits 0 whatever it
finds, including an empty or unreadable log.

usage: linker-messages.py BUILD_LOG
"""

import collections
import re
import sys

# Kinds of linker output that have meant a real defect in spyc's link.
# `duplicate symbol`: two archive members defined `_memset` on macOS (#1129).
FLAG_PATTERNS = ("duplicate symbol", "undefined symbol")


def normalize(line):
    line = re.sub(r"^warning: ", "", line.strip())
    line = re.sub(r"/[^\s()'\"]*/(lib[a-z0-9_]+)-[0-9a-f]+\.rlib\[\d+\]", r"\1.rlib", line)
    line = re.sub(r"\(/[^)]*/([^/)]+)\)", r"(\1)", line)
    line = re.sub(r"\b[0-9a-f]{16}-", "", line)
    # One row per archive, not per member, for messages about an object file.
    return re.sub(r"object file \((lib[a-z0-9_]+\.rlib)\([^)]*\)\)", r"object file (\1(*.o))", line)


def messages(text):
    """[(lint, message)] for every line of every linker warning in a cargo log."""
    out, block = [], []
    for line in text.splitlines():
        if re.match(r"^warning: (linker std(err|out): |ld: |.*rust-lld)", line):
            block = [normalize(line)]
        elif block and line.startswith(" ") and not line.lstrip().startswith(("|", "=")):
            if line.strip():
                block.append(normalize(line))
        elif block:
            m = re.search(r"`--force-warn (linker-\w+)`|#\[warn\((linker_\w+)\)\]", line)
            if m or not line.strip().startswith(("|", "=")):
                lint = (m.group(1) or m.group(2)).replace("_", "-") if m else "unknown"
                out += [(lint, msg) for msg in block]
                block = []
    return out


def report(text):
    found = collections.Counter(messages(text))
    lines = ["## Linker messages", ""]
    if not found:
        return "\n".join(lines + ["No linker output in this build.", ""]), []
    flagged = sorted(m for (_, m) in found if any(p in m for p in FLAG_PATTERNS))
    if flagged:
        lines += [f"**Flagged** ({', '.join(FLAG_PATTERNS)}):", ""]
        lines += [f"- `{m}`" for m in dict.fromkeys(flagged)] + [""]
    lines += ["| count | lint | message |", "|---:|---|---|"]
    for (lint, msg), n in sorted(found.items(), key=lambda kv: (-kv[1], kv[0])):
        lines.append(f"| {n} | `{lint}` | `{msg}` |")
    return "\n".join(lines + [""]), list(dict.fromkeys(flagged))


def main(argv):
    try:
        with open(argv[1], encoding="utf-8", errors="replace") as f:
            text = f.read()
    except (IndexError, OSError) as e:
        print(f"## Linker messages\n\nNo build log to read ({e}).")
        return 0
    summary, flagged = report(text)
    print(summary)
    for msg in flagged:
        print(f"::warning title=linker message::{msg}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
