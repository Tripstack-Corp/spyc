# Evidence for the pre-3.0 review

What the unit reports and refuter files cite: probe scripts, their logs, the load runs and the mutation runs, copied from the reviewers' scratch directories on 2026-11-24 so they outlive them. They are the regression checks for the remediation. Paths inside them were rewritten: a scratch path now reads `docs/drafts/review-3.0/evidence/<unit>/…` or `<scratchpad>`, and the home directory reads `~`.

| Directory | From |
|---|---|
| `A/` | Unit A's probes of the upgrade and the exec boundary (`probe_*.py` with `probe_lib.py`), their logs and each scratch jar's standard error, `sigs.c` (reads macOS signal dispositions), the fuzz run logs |
| `B/` | Unit B's load runs (`mac_load.sh`, `linux/load.sh`, `mac/` and `linux/out*/` with their `summary` files), `linux/probe_*.py`, `jarprobe.py`, and `review_b_probes.patch`: the uncommitted probe tests from the `review/3.0-b` worktree |
| `C/` | Unit C's `drive_v7.py` (drives a scratch spyc in a PTY) and each run's `events.log`, `raw.out` and pids, the red-test logs, and `red-tests.patch`: the uncommitted red tests from the `review/3.0-c` worktree |
| `D/` | Unit D's probes (`ud.py` helper, `p_attach.py`, `p_detach.py`, `p_len.py`, `p_live.py`) |
| `E/` | Unit E's probes of the MCP control methods and of `spyc attach`'s trust, with logs |
| `F/` | Unit F's mutation runs (`mutation-results.json`, `mut-*.log`), `package-check.log`, `iso.sh` (the isolation wrapper) |
| `G/` | `fullfsync.py` |
| `refute-B/`, `refute-C/`, `refute-D/` | What the refuters ran |
| `gate-*-summary.txt` | The verdict and per-suite lines of `make check-ci` at the pin, on macOS and on Linux aarch64 musl |

## Left out

- From `P3/build/{pin,main}/`: the copies of `crates/spyc-jar` and of `Cargo.lock` that `build.sh` takes from the commit it builds. They are the repo's own files at `444857a2` and `52ce15b7`; `build.sh` says how they were copied.

- The fuzz corpus Unit A's runs grew (in the `review/3.0-a` worktree).
- Both full gate logs (about 450 kB each); their summaries are here.
- Unit F's `mac-tests.txt` and `linux-tests.txt` (about 400 kB each, the test-name lists) and `pkgx/` (the unpacked crate from `make package-check`).
- Every scratch runtime directory: `HOME`, `XDG_*` and `TMPDIR` trees, jar tokens, `jar-bin` stores (copies of `spyc-jar`), saved sessions, context files.
- Unit C's `redtest-env/` (a cargo home, about 2 GB) and its paste payloads over 300 kB.
- `A/sigs`, a compiled binary (its source `sigs.c` is here), and Python bytecode caches.
