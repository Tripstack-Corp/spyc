"""Score the candidate rules over the observation files and print the
report's tables as Markdown: `python3 -I analyze.py > analysis.md`.

Each row is one program, time point, observer position (and on Linux, Yama
scope). A rule is scored against the row's `expect_blocked` label."""
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
# Cases whose command is the shell itself, so a foreground of the shell is
# what they look like while working as intended.
SHELL_CASES = {"read-builtin", "ttin-job", "shell-prompt"}


def load(name):
    p = os.path.join(HERE, name)
    if not os.path.exists(p):
        return None
    with open(p) as f:
        return json.load(f)


def threads(row):
    for m in row["obs"].get("members", []):
        t = m.get("threads")
        if isinstance(t, list):
            for th in t:
                yield m, th
        elif isinstance(t, dict):
            yield m, {"run_state": t.get("error"), "syscall": {"error": t.get("error")}}


def s1(row):
    s = row["obs"].get("s1") or {}
    return None if "error" in s else s


def echo_off_canon(row):
    s = s1(row)
    return bool(s) and not s["echo"] and s["icanon"]


def s2_read(row):
    """A thread blocked in a read on the terminal."""
    for _, th in threads(row):
        sc = th.get("syscall") or {}
        if th.get("state") in ("S", "D") and sc.get("fd_is_tty"):
            return True
    return False


def s2_wait(row):
    """S2-read, or a wait whose visible descriptor set is the terminal's: an
    epoll set holding it, or a select whose descriptors below nfds are all it."""
    if s2_read(row):
        return True
    for _, th in threads(row):
        sc = th.get("syscall") or {}
        if th.get("state") not in ("S", "D"):
            continue
        if any(t.get("tty") for t in sc.get("epoll_targets") or [] if isinstance(t, dict)):
            return True
        below = sc.get("fds_below_nfds") or []
        if below and all(b["tty"] for b in below):
            return True
    return False


def s2_wchan(row):
    """A thread asleep in `wait_woken`, the kernel function a terminal read
    sleeps in, from `/proc/<pid>/task/<tid>/wchan`, which Yama doesn't gate."""
    return any(th.get("state") == "S" and th.get("wchan") == "wait_woken" for _, th in threads(row))


def s2_readable(row):
    errs = [(th.get("syscall") or {}).get("error") for _, th in threads(row)]
    return bool(errs) and not all(errs)


# CPU over the 100 ms pair that counts as working: 2 ms, 2 % of a core. An
# idle event loop's timer wakeups cost tens to hundreds of microseconds per
# window (`wakeups` below lists them), which macOS's nanosecond counters see.
MOVING_NS = 2_000_000


def moving(row):
    d = row["obs"].get("s3_pair_delta") or {}
    return max(d.get("cpu_ns", 0), d.get("run_ns", 0)) >= MOVING_NS or d.get("disk_bytes", 0) > 0


def wakeups(doc):
    """Rows whose CPU moved, but by less than MOVING_NS."""
    out = {}
    for r in doc["rows"]:
        d = r["obs"].get("s3_pair_delta") or {}
        c = max(d.get("cpu_ns", 0), d.get("run_ns", 0))
        if 0 < c < MOVING_NS:
            out.setdefault(r["case"], []).append(c // 1000)
    return out


def s4(row):
    return row["s4"]["unterminated"]


def valid(row):
    """A terminal case whose command had already gone (the prompt never came)
    can't be scored."""
    if row["cat"] == "terminal" and row["case"] not in SHELL_CASES and not row["cmd_running"]:
        return False
    return True


def canon(row):
    s = s1(row)
    return bool(s) and s["icanon"]


def job(row):
    """A job, not the shell, holds the foreground (`tcgetpgrp`, R4's other half)."""
    return row["cmd_running"]


def rules(platform):
    r = {
        "R1 echo off, canonical": echo_off_canon,
        "R4 S4 alone": s4,
        "R4g S4, a job in the foreground, not vetoed": lambda x: s4(x) and job(x) and not moving(x),
        "R6 R4g and canonical": lambda x: s4(x) and job(x) and canon(x) and not moving(x),
    }
    if platform == "linux":
        r["R2 member blocked reading the terminal"] = s2_read
        r["R2b R2, or a wait set that is the terminal"] = s2_wait
        r["R3 R2b vetoed by S3"] = lambda x: s2_wait(x) and not moving(x)
        r["R5 S4 and (R1 or R2b), not vetoed"] = lambda x: s4(x) and (echo_off_canon(x) or s2_wait(x)) and not moving(x)
        r["R7 R2b and canonical, not vetoed"] = lambda x: s2_wait(x) and canon(x) and not moving(x)
        r["R2w a thread asleep in wait_woken"] = s2_wchan
        r["R7w R2w and canonical, not vetoed"] = lambda x: s2_wchan(x) and canon(x) and not moving(x)
        r["R8 R7w or R1"] = lambda x: (s2_wchan(x) and canon(x) and not moving(x)) or echo_off_canon(x)
        r["R9 R7 or R1"] = lambda x: (s2_wait(x) and canon(x) and not moving(x)) or echo_off_canon(x)
    else:
        r["R5m S4 and R1, not vetoed"] = lambda x: s4(x) and echo_off_canon(x) and not moving(x)
        r["R8m R6 or R1"] = lambda x: (s4(x) and job(x) and canon(x) and not moving(x)) or echo_off_canon(x)
    return r


def score(rows, fn):
    pos = [r for r in rows if r["expect_blocked"]]
    neg = [r for r in rows if not r["expect_blocked"]]
    fn_rows = [r for r in pos if not fn(r)]
    fp_rows = [r for r in neg if fn(r)]
    return fp_rows, len(neg), fn_rows, len(pos)


def cases_of(rs):
    seen = []
    for r in rs:
        if r["case"] not in seen:
            seen.append(r["case"])
    return ", ".join(seen)


def rule_tables(doc, platform):
    rows = [r for r in doc["rows"] if valid(r)]
    dropped = sorted({r["case"] for r in doc["rows"] if not valid(r)})
    groups = {}
    for r in rows:
        groups.setdefault((r["scope"], r["position"]), []).append(r)
    print(f"### Rules on {platform}\n")
    if dropped:
        print(f"Not scored (the prompt never came): {', '.join(dropped)}.\n")
    print("| Rule | Scope | Position | False positives | False negatives | FP cases | FN cases |")
    print("|---|---|---|---|---|---|---|")
    for name, fn in rules(platform).items():
        for (scope, pos), rs in sorted(groups.items()):
            fp, nneg, fnr, npos = score(rs, fn)
            print(f"| {name} | {scope} | {pos} | {len(fp)} of {nneg} | {len(fnr)} of {npos} | {cases_of(fp)} | {cases_of(fnr)} |")
    print()


def summary(row, platform):
    s = s1(row)
    s1txt = "err " + str((row["obs"].get("s1") or {}).get("error")) if s is None else \
        f"{'E' if s['echo'] else 'e'}{'C' if s['icanon'] else 'c'}{'I' if s['isig'] else 'i'}"
    parts = []
    for m, th in threads(row):
        if platform == "linux":
            sc = th.get("syscall") or {}
            if "error" in sc:
                parts.append(f"{m.get('comm')}:{sc['error']}")
                continue
            name = sc.get("name")
            extra = ""
            if sc.get("fd_target"):
                extra = f"({sc['arg0']}={'tty' if sc.get('fd_is_tty') else sc['fd_target']})"
            elif "epoll_targets" in sc:
                tg = sc["epoll_targets"]
                extra = "(tty)" if any(isinstance(t, dict) and t.get("tty") for t in tg) else f"({len(tg) if isinstance(tg, list) else tg} fds)"
            elif "nfds" in sc:
                extra = f"(nfds={sc['nfds']})"
            parts.append(f"{m.get('comm')}/{th.get('state')}:{name}{extra}")
        else:
            parts.append(f"{m.get('comm')}:{th.get('run_state')}")
    # Collapse repeats.
    seen = {}
    for p in parts:
        seen[p] = seen.get(p, 0) + 1
    s2txt = " ".join(f"{k}×{v}" if v > 1 else k for k, v in seen.items()) or "(no members)"
    d = row["obs"].get("s3_pair_delta") or {}
    s3txt = f"{d.get('cpu_ns', 0) / 1e6:.0f}/{d.get('run_ns', 0) / 1e6:.0f} ms, {d.get('disk_bytes', 0) >> 10} KiB"
    return s1txt, s2txt, s3txt, ("yes" if s4(row) else "no")


def signal_table(doc, platform, scope, label="2s"):
    print(f"### Signals on {platform}, {label}, ancestor{', scope ' + scope if platform == 'linux' else ''}\n")
    print("S1: E/e echo on/off, C/c canonical on/off, I/i `ISIG`. S3: CPU (clock ticks) / scheduler run time over 100 ms, disk bytes.\n")
    print("| Case | Expect | S1 | S2 | S3 | S4 | Row |")
    print("|---|---|---|---|---|---|---|")
    for r in doc["rows"]:
        if r["label"] == label and r["position"] == "ancestor" and r["scope"] == scope:
            a, b, c, d = summary(r, platform)
            mark = "" if valid(r) else " (no prompt)"
            print(f"| {r['case']}{mark} | {'blocked' if r['expect_blocked'] else '—'} | {a} | {b} | {c} | {d} | `{r['id']}` |")
    print()


def access_table(doc, platform):
    print(f"### Access on {platform}\n")
    rows = doc["rows"]
    keys = []
    for r in rows:
        for k in (r.get("cells") or {}):
            if k not in keys:
                keys.append(k)
    print("| Scope | Target | Position | Target uid/ppid | " + " | ".join(keys) + " |")
    print("|---|---|---|---|" + "---|" * len(keys))
    for r in rows:
        if "cells" not in r:
            print(f"| {r['scope']} | {r['target']} | — | — | {r.get('error')} |")
            continue
        ti = r["target_info"]
        cells = " | ".join(str(r["cells"].get(k, "")) for k in keys)
        print(f"| {r['scope']} | {r['target']} | {r['position']} | {ti.get('uid')}/{ti.get('ppid')} | {cells} |")
    print()


def cost_table(doc, platform):
    print(f"### Cost on {platform}\n")
    print("| Group | Members | /proc entries | Full read µs (median, p90) | Instrumented calls | strace calls | Members only µs (median, p90) | Calls |")
    print("|---|---|---|---|---|---|---|---|")
    for r in doc["rows"]:
        st = r.get("strace_calls_unrelated_first_read")
        st = st["total"] if isinstance(st, dict) else "—"
        print(f"| {r['n']} | {r['members']} | {r['proc_entries_scanned']} | {r['full_us_median']}, {r['full_us_p90']} | {r['full_calls']} | {st} | "
              f"{r['members_only_us_median']}, {r['members_only_us_p90']} | {r['members_only_calls']} |")
    print()


def summary_table():
    """One line per rule: its score from the positions that matter."""
    cols = [("linux", "1", "ancestor"), ("linux", "1", "unrelated"), ("linux", "2", "unrelated"), ("macos", "n/a", "ancestor")]
    docs = {p: load(f"observation-{p}.json") for p in ("linux", "macos")}
    names = []
    for p in ("linux", "macos"):
        for n in rules(p):
            if n not in names:
                names.append(n)
    print("| Rule | " + " | ".join(f"{p} {'scope ' + s + ', ' if p == 'linux' else ''}{pos}" for p, s, pos in cols) + " |")
    print("|---|" + "---|" * len(cols))
    for n in names:
        cells = []
        for p, s, pos in cols:
            doc = docs[p]
            fn = rules(p).get(n)
            if doc is None or fn is None:
                cells.append("n/a")
                continue
            rs = [r for r in doc["rows"] if valid(r) and r["scope"] == s and r["position"] == pos]
            fp, nneg, fnr, npos = score(rs, fn)
            cells.append(f"FP {len(fp)}/{nneg}, FN {len(fnr)}/{npos}")
        print(f"| {n} | " + " | ".join(cells) + " |")
    print()


def main():
    if "--summary" in sys.argv:
        summary_table()
        return
    for platform in ("linux", "macos"):
        doc = load(f"observation-{platform}.json")
        if doc is None:
            print(f"_observation-{platform}.json is missing._\n")
            continue
        print(f"## {platform}\n")
        if doc.get("not_covered"):
            print("Not covered: " + "; ".join(f"{n['case']} ({n['why']})" for n in doc["not_covered"]) + ".\n")
        scopes = sorted({r["scope"] for r in doc["rows"]})
        signal_table(doc, platform, "1" if "1" in scopes else scopes[0])
        w = wakeups(doc)
        print("Below the S3 threshold (CPU moved by under 2 ms per 100 ms, µs per row): " +
              "; ".join(f"{k} {sorted(v)}" for k, v in sorted(w.items())) + ".\n")
        rule_tables(doc, platform)
        acc = load(f"access-{platform}.json")
        if acc:
            access_table(acc, platform)
        cost = load(f"cost-{platform}.json")
        if cost:
            cost_table(cost, platform)


if __name__ == "__main__":
    main()
