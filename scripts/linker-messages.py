#!/usr/bin/env python3
"""Summarize the linker messages in a cargo build log, as Markdown; with
`--deny EXCEPTIONS`, fail on one that is neither informational nor excepted.

rustc files a linker's output under one of two lints: `linker_messages` (warn
by default) and `linker_info` (allow by default, for output it considers
informational, such as which macOS version an object was built for). The macOS
jobs of `.github/workflows/shipped-targets.yml` force both on for spyc's link
with `--force-warn`, which no `allow` in Cargo.toml can lower.

The summary groups each line of linker output with others of its kind once
paths, hashes and archive member indices are stripped, and counts them. A line
matching `FLAG_PATTERNS` is flagged: printed first, and as a GitHub `::warning`
annotation. Without `--deny` it exits 0 whatever it finds.

With `--deny EXCEPTIONS` it exits 1, after the summary, when:
- a message rustc did not file under `linker_info` (one under
  `linker_messages`, or under no lint this can name) matches no exception;
- it parses no linker output at all, which on a macOS link means the log
  format may have changed;
- there is no log, or the exceptions file can't be read or has a bad entry.

An exceptions entry is a line holding a spyc issue number and a Python regex,
which is searched in a message's first line as the summary prints it and
covers the lines that continue it (the objects an `ld: duplicate symbol`
names). An entry without an issue number is an error; `# ` starts a comment.

usage: linker-messages.py [--deny EXCEPTIONS] BUILD_LOG
"""

import argparse
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


def warnings(text):
    """[(lint, [line, ...])] for each linker warning in a cargo log, lines normalized."""
    out, block = [], []
    for line in text.splitlines():
        if re.match(r"^warning: (linker std(err|out): |ld: |.*rust-lld)", line):
            if block:
                out.append(("unknown", block))
            block = [normalize(line)]
        elif block and line.startswith(" ") and not line.lstrip().startswith(("|", "=")):
            if line.strip():
                block.append(normalize(line))
        elif block:
            m = re.search(r"`--force-warn (linker-\w+)`|#\[warn\((linker_\w+)\)\]", line)
            if m or not line.strip().startswith(("|", "=")):
                lint = (m.group(1) or m.group(2)).replace("_", "-") if m else "unknown"
                out.append((lint, block))
                block = []
    return out + [("unknown", block)] if block else out


def messages(text):
    """[(lint, line)] for every line of every linker warning in a cargo log."""
    return [(lint, line) for lint, block in warnings(text) for line in block]


def split_messages(block):
    """A warning's lines as [(first line, continuation lines)]: Apple's `ld`
    starts each message with `ld: ` and puts what it names on the lines after."""
    out = []
    for line in block:
        if not out or re.match(r"(linker std(err|out): |ld: |.*rust-lld)", line):
            out.append((line, []))
        else:
            out[-1][1].append(line)
    return out


def load_exceptions(path):
    """[(issue, regex)] from an exceptions file; ValueError names a bad line."""
    with open(path, encoding="utf-8") as f:
        lines = f.read().splitlines()
    entries = []
    for n, raw in enumerate(lines, 1):
        line = raw.strip()
        if not line or re.match(r"#(\s|$)", line):
            continue
        m = re.fullmatch(r"(\d+)\s+(\S.*)", line)
        if not m:
            raise ValueError(f"{path}:{n}: an entry needs a spyc issue number, then a pattern: {raw!r}")
        try:
            entries.append((int(m.group(1)), re.compile(m.group(2))))
        except re.error as e:
            raise ValueError(f"{path}:{n}: the pattern is not a regex ({e}): {raw!r}") from e
    return entries


def gate(text, exceptions):
    """(denied, excepted): the messages the gate fails on, as `lint`: line,
    and the excepted ones with their issue."""
    denied, excepted = [], []
    for lint, block in warnings(text):
        if lint == "linker-info":
            continue
        for first, _ in split_messages(block):
            issue = next((i for i, rx in exceptions if rx.search(first)), None)
            if issue is None:
                denied.append(f"`{lint}`: {first}")
            else:
                excepted.append((first, issue))
    return list(dict.fromkeys(denied)), list(dict.fromkeys(excepted))


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
    args = argparse.ArgumentParser(prog=argv[0])
    args.add_argument("--deny", metavar="EXCEPTIONS")
    args.add_argument("log", nargs="?")
    args = args.parse_args(argv[1:])
    deny = args.deny is not None
    try:
        with open(args.log, encoding="utf-8", errors="replace") as f:
            text = f.read()
    except (TypeError, OSError) as e:
        print(f"## Linker messages\n\nNo build log to read ({e}).")
        return 1 if deny else 0
    summary, flagged = report(text)
    print(summary)
    for msg in flagged:
        print(f"::warning title=linker message::{msg}", file=sys.stderr)
    if not deny:
        return 0
    try:
        exceptions = load_exceptions(args.deny)
    except (OSError, ValueError) as e:
        print(f"::error title=linker exceptions::{e}", file=sys.stderr)
        print(f"Exceptions file unusable: {e}")
        return 1
    if not warnings(text):
        msg = "No linker output parsed from this macOS link: the log format may have changed."
        print(f"::error title=linker messages::{msg}", file=sys.stderr)
        print(f"**{msg}**")
        return 1
    denied, excepted = gate(text, exceptions)
    if excepted:
        print("Excepted:\n")
        print("\n".join(f"- `{msg}` (#{issue})" for msg, issue in excepted) + "\n")
    for msg in denied:
        print(f"::error title=linker message::{msg}", file=sys.stderr)
    if denied:
        print("**Denied** (add an exception with an issue in the exceptions file, or fix the link):\n")
        print("\n".join(f"- {msg}" for msg in denied))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
