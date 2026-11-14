#!/usr/bin/env python3
"""Interleaved A/B timing: W warm-ups each, then N rounds of (A, B) in
alternating order. Prints mean, sd, min per variant and writes every run.

usage: timeit.py --name NAME --runs N --warmup W --out RAW.tsv --mode MODE \
         A_BIN B_BIN [args...]
MODE: wall      — wall time of the whole process (prep run untimed)
      bench     — parse the spike bench's own "in X.XXXs" lines (vt100, ghostty)
      spycstart — drive.py startup-and-quit in a pty, wall time
"""
import argparse, os, re, statistics, subprocess, sys, time

HERE = os.path.dirname(os.path.abspath(__file__))

def run_once(mode, binary, args, prep):
    if prep:
        subprocess.run(prep, shell=True, check=True)
    if mode == "bench":
        out = subprocess.run([binary] + args, capture_output=True, text=True, check=True).stdout
        res = {}
        for eng, secs in re.findall(r"^\s+(vt100|ghostty)\s+[\d.]+ MiB/s\s+\(\d+ MiB in ([\d.]+)s", out, re.M):
            res[eng] = float(secs)
        if set(res) != {"vt100", "ghostty"}:
            sys.exit("bench output unparsed:\n" + out)
        return res
    if mode == "spycstart":
        out = subprocess.run([sys.executable, os.path.join(HERE, "drive.py"), binary] + args,
                             capture_output=True, text=True)
        if out.returncode != 0:
            sys.exit("drive.py failed:\n" + out.stdout + out.stderr)
        return {"wall": float(out.stdout.split()[-1])}
    t0 = time.perf_counter()
    p = subprocess.run([binary] + args, capture_output=True)
    dt = time.perf_counter() - t0
    if p.returncode not in (0, 1):
        sys.exit(f"exit {p.returncode}: {p.stderr[-2000:]!r}")
    return {"wall": dt}

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--name", required=True)
    ap.add_argument("--runs", type=int, default=10)
    ap.add_argument("--warmup", type=int, default=2)
    ap.add_argument("--out", required=True)
    ap.add_argument("--mode", default="wall")
    ap.add_argument("--prep", default="")
    ap.add_argument("a")
    ap.add_argument("b")
    ap.add_argument("args", nargs=argparse.REMAINDER)
    o = ap.parse_args()
    bins = {"A": o.a, "B": o.b}
    for _ in range(o.warmup):
        for v in "AB":
            run_once(o.mode, bins[v], o.args, o.prep)
    data = {"A": [], "B": []}
    with open(o.out, "a") as raw:
        raw.write(f"# {o.name}: mode={o.mode} runs={o.runs} warmup={o.warmup} args={o.args} prep={o.prep!r}\n")
        raw.write(f"# A={o.a}\n# B={o.b}\n")
        for i in range(o.runs):
            order = "AB" if i % 2 == 0 else "BA"
            for v in order:
                r = run_once(o.mode, bins[v], o.args, o.prep)
                data[v].append(r)
                raw.write(f"{o.name}\t{i}\t{v}\t" + "\t".join(f"{k}={x:.6f}" for k, x in sorted(r.items())) + "\n")
    for key in sorted(data["A"][0]):
        for v in "AB":
            xs = [r[key] for r in data[v]]
            print(f"{o.name}\t{key}\t{v}\tn={len(xs)}\tmean={statistics.mean(xs):.4f}\tsd={statistics.stdev(xs):.4f}\tmin={min(xs):.4f}")

main()
