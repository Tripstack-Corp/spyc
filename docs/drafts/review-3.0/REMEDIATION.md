# Remediation ledger

One row per finding the remediation brief's section 3 closes, plus what its section 4 adds. A row is closed when its red test was seen failing and then passing on both platforms, a refuter who didn't write the fix failed to break it, and its PR is merged. Each PR that changes a row updates it here.

Findings, sites and evidence are in [`SYNTHESIS.md`](SYNTHESIS.md), [`ADDENDUM.md`](ADDENDUM.md) and [`H-cross-version.md`](H-cross-version.md). What section 5 files instead of fixing is listed at the end.

| Finding | Slice | Red test | PR | Refuter's verdict | Status |
|---|---|---|---|---|---|
| B3 (jar tests red under load) | A | | | | open |
| F5 (fixed waits that assume a state) | A | | | | open |
| B13's remainder (the counter's echo) | A | | | | open |
| B10 (tests leak processes) | A | | | | open |
| F3 (macOS in CI), after the loaded runs are green | A | | | | waits on the four above |
| C1 = G2 = H2 (restore with jars off runs a program twice) | B | | | | open; red test exists (`evidence/C/red-tests.patch`) |
| C2, both halves, with P1 (`^a R` and the retake) | B | | | | open |
| B12 (`EINTR` ends a link on Linux) | B | | | | open |
| C12 (`[detached]` says why) | B | | | | open |
| B1 (unchecked `thread::spawn`, upgrade path first) | C | | | | open |
| A1 (`SIGPIPE` after a failed `exec`) | C | | | | open |
| A2 (reason 6 where 5 is promised) | C | | | | open |
| A3 (the copy step opens any path) | C | | | | open |
| A9 (a validator's process group) | C | | | | open |
| G9 (three jar-or-nothing sentences) | C | | | | open |
| A13 (the specification's prose) | C | | | | open |
| D7 (token rotation no code performs) | C | | | | open |
| E6's sentence under "Who may connect" | C | | | | open |
| F6 (no test of the peer-user check) | C | | | | open |
| F7 (no test of `SO_NOSIGPIPE`) | C | | | | open |
| S6: `honoured()` removed, with F1 and G1 | D | | | | open |
| F2 (`spyc-jar` in lockstep with spyc) | D | | | | open |
| A8 = F4 (`jar_compat` and the first fixture) | D | | | | open |
| F8 (a `.deb` without `spyc-jar`) | D | | | | open |
| F9 (the release document) | D | | | | open |
| D1 (connect at the given path; refuse a `:name` that can't fit) | E | | | | open |
| D2, D6, and P3-9's low (start and save before release) | E | | | | open |
| The orphan prompt (confirm a discard; no type-ahead) | E | | | | open |
| D5 (`save_released` keeps the last report) | E | | | | open |
| D3 (`take_over` re-reads before it signals) | E | | | | open |
| E1 (the control methods need the session's token) | F | | | | open |
| E2, high since P3-9 (one ownership check on every path) | F | | | | open |
| E3 = G5 (tasks and captures signal through the jar) | F | | | | open |
| E4 (`SECURITY.md` describes 3.0) | F | | | | open |
| E7 (a session's token is forgotten with its jar) | F | | | | open |
| H1 (2.4.0 restores a 3.0 session and runs every program twice) | B | | | | open; added to slice B at the owner's word, 2026-11-24 |
| Handover fault hooks (Unit D's crash ordering, by test) | §4 | | | | open |

## Filed, not fixed (section 5)

Issues on the `3.0.x` milestone, one per remaining medium, one per unit for its lows and informationals, and B4 with its design. Listed here as they are filed.
