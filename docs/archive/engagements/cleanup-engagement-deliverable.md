# Post-remediation cleanup — deliverable

> **Archived (2026-08-19).** It was held out of this directory because its
> "Found along the way" list still carried open items. Two have since closed:
> **item 2** — SECURITY.md now says pane attribution is not authorization
> (#429) — and **item 3** — spyc honours the child's DECCKM (#259). Item 1 is
> documented in AGENTS.md (#256). What stays open is observation, not
> unfinished engagement work: **item 4** (no low-disk warning on
> `create_worktree`; the shared-target-dir guidance in AGENTS.md is the only
> mitigation), **item 6** (the cargo-deny segfault, deliberately undecided on
> one data point — and `deny` has since left `make check` for `audit.yml`),
> and **item 7** (`pick_best_rollout`'s two-unclaimed-panes residual, still
> noted on #230). C7 became
> [`docs/drafts/pane-identity-transport-proposal.md`](../../drafts/pane-identity-transport-proposal.md),
> accepted for 2.2.

Brief: the C1–C7 cleanup engagement, plus C8 and C9 added mid-flight.
Baseline `edc27b8`. Eleven PRs: #247–#250, #252, #253, #255, #256.

## Summary

All merged.

| Item | PR | Commit | Outcome |
|---|---|---|---|
| C1 | #247 | `0e30e5b` | **parked** — did not reproduce; instrumented so the next occurrence is decisive |
| C2 | #249 | `6e087b3` | fixed — one shared splitter + a spawn-hygiene guard |
| C3 | #250 | `c8a3c0c` | **fixed** (not the dead-end the brief allowed for) |
| C4 | #252 | `1103cfe` | done — 61 tests relocated, count preserved exactly |
| C5 | #253 | `bddb28f` | delivered the achievable part; **acceptance criterion was impossible** |
| C6 | #253 | `bddb28f` | documented |
| C7 | — | draft doc | proposal only, **recommendation differs from the brief's framing** |
| C8 | #255 | `b258afb` | fixed — but **the stated mechanism was wrong** |
| C9 | #248 | `9aefbb4` | fixed — a repo-corrupting bug, added mid-engagement |
| — | #256 | `7530145` | the `FETCH_HEAD` warning, documented on request |

## Per item

**C1 — agent-pane input deafness.** Did not reproduce under a deliberate attempt
(instrumented build, `SPYC_KEY_TRACE=1`, Claude pane + shell pane, unanswered
multi-question, repeated pane switches). No speculative fix. Landed the one trace
field that makes the next occurrence decisive: `app_cursor=` on the send line.
*Evidence:* reproduction matrix and untried variations recorded in
`docs/drafts/mutli-question-bug-investigation.md`.

What it did establish: **candidate A (a latched `resolver_pending`) is ruled
out.** Every pending state resets on an unmatched key; `route_input` with
`pending` set still reaches `feed`, which resets; the only pre-`feed` swallow is
60 ms, same-key, and requires `!resolver_pending`. The live trace confirmed it —
`g` armed the chord, the next key cleared it. And **a new candidate C is
confirmed to exist**: spyc never reads the child's DECCKM state, so a child in
application-cursor mode receives `ESC [ A` where it waits for `ESC O A`. It is
the only candidate that predicts the reported asymmetry — arrows dying while a
bare `^c` survives.

**C2 — guard splitter.** Three guards each carried
`src.split("#[cfg(test)]").next()`, which fails open twice: a comment
*mentioning* the attribute truncated `render/mod.rs` to 22 production lines of
827, and a mid-file `#[cfg(test)] mod x;` hid 90% of `git/worktree.rs`. Replaced
with one shared `guard_support::production_half` that removes guarded items
rather than truncating.
*Evidence:* four canaries; verified three go red against the old heuristic. All
four existing guards still pass under the wider scan — the blind spot was real
but wasn't hiding violations. Plus a new guard asserting every test-side git
spawn resists an inherited `GIT_DIR`.

**C3 — resumed codex sessions.** Fixed, and narrower than stated: Signal 1
already covers `codex resume <UUID>`, including spyc's own restore. Only
`resume --last`, an id-less restore, and codex's in-app picker are unpinnable.
For those, added `mtime_secs` to `RolloutMeta` and a fallback taking the most
recently written matching rollout that has grown since the pane spawned.
*Evidence:* four new tests (fix, gate, claimed-set on the fallback, staleness);
seven pre-existing pinning tests unchanged; the original #230 regression test
`pick_prefers_resumed_session_over_a_fresh_unrelated_one` stays green. mtime
remains a liveness *filter* and never becomes a ranking key.

**C4 — mouse test relocation.** `mod.rs` 1,815 → 303 lines, zero tests left.
61 `#[test]` before, 61 after.
*Evidence:* count asserted both sides; `make check` green. Five tests filed under
the *selection* headings turned out to assert `route_mouse`'s decision, not the
selection machinery — the headings had grouped by feature rather than by function
under test, visible only once the two lived in different files.

**C5 — the gate.** The acceptance criterion — `make check-ci 2>&1 | tail -5`
exiting nonzero — **is not achievable**. A pipeline's status is its last
command's in `sh`, `bash` and `zsh`, and the pipe is built in the caller's shell.
Verified, not assumed. Delivered what a target can do: `check-ci` ends in
`=== GATE: PASS ===` / `=== GATE: FAIL ===`, so the verdict survives a tail even
though the exit code cannot.
*Evidence:* demonstrated with a deliberate type-error canary piped through
`tail -3`; canary reverted, only the passing state committed.

**C6 — the merge race.** Documented in AGENTS.md's merges section with the
wait-then-retry loop actually used. It bit all twelve PRs of the previous
engagement and every one of this one.

**C7 — pane identity.** `docs/drafts/pane-identity-transport-proposal.md`.
Recommends the `initialize` handshake over a per-pane socket, and reaches a
conclusion the brief's framing did not anticipate — see findings below.

**C8 — status cache key.** Git config decides what `status` reports and moves
neither `index` nor `HEAD`, so the poll short-circuits on a stale answer
indefinitely. Folded the shared config's mtime into the key, resolved once per
chdir beside the gitdir (following `commondir` for linked worktrees, which have
no config of their own). One extra `stat` per poll.
*Evidence:* `a_config_change_moves_the_status_cache_key`, verified red-before /
green-after. Also fixes `core.excludesFile` and `status.showUntrackedFiles`
changes, which previously needed an unrelated index write to take effect.

**C9 — `GIT_DIR` leak.** `GIT_DIR` overrides `-C`, and git exports it into hook
processes, so a hook-launched `cargo test` retargets every scratch-repo command
at the developer's real repository. One shared `git_command()` builder strips the
nine redirect variables; the nine ad-hoc spawn sites drop the three that bite.
*Evidence:* `git_dir_really_does_beat_dash_c` characterizes the hazard;
`git_command_strips_the_ambient_repo_redirect` asserts the fix. `make check`
green with `core.bare` confirmed unchanged after the run.

## Corrections to the brief

Four items were premised on something that turned out not to hold. Recording
them because in each case the wrong premise would have produced the wrong work.

- **C5's acceptance criterion was impossible** — medium. No callee can control a
  caller-side pipeline's exit status. Building to it would have meant shipping
  something that looked like a fix and wasn't.
- **C8's stated mechanism was wrong** — medium. "A failed walk is cached
  indistinguishably from a clean repo" is false: `apply_git_worker_result`
  already discards `entries: None`, and the sync path nulls before refilling.
  The real defect is that a *structurally* clean answer is a legitimate success
  the mtime key cannot invalidate.
- **C3's scope was too broad** — low. Resumed sessions are not generally
  unpinnable; Signal 1 covers the common path.
- **C7's trust framing was incomplete** — see below.

## Found along the way — not fixed, no unrequested changes

### 1. `git reset FETCH_HEAD` starves the alphabetically-later branch — high

A bare `git fetch` writes **every** fetched branch into `FETCH_HEAD`;
`git reset FETCH_HEAD` consumes only the **first line**, which is alphabetical.
Every line is marked `not-for-merge`, and `reset` ignores that.

Observed live: another session's worktree on `feat/about-action` was reset three
times to `2fd45e6` — the tip of this engagement's `docs/gate-and-merge-race-…`
branch — because `docs/` sorts before `feat/`. It cost that session four
commits. `FETCH_HEAD` is per-worktree, so the file was its own; the *content*
was wrong.

spyc is a multi-worktree, multi-agent tool that encourages exactly this layout,
so the idiom deserves a warning even though it isn't spyc's. **Documented in
AGENTS.md at the owner's request (#256)**, beside the `update-branch` merge-race
note — which is where someone looks when trying to get a branch current, exactly
when they reach for `reset FETCH_HEAD`. The safe forms are named there:
`git fetch origin <branch> && git reset origin/<branch>`, or
`gh pr update-branch`, which is server-side and races nothing local.

The diagnosis was slow *because* `FETCH_HEAD` is per-worktree: the file was that
session's own, so it read as another session writing into its worktree rather
than a local idiom resolving the wrong ref. Worth remembering as a shape — "the
file is mine, the content is wrong" is not where suspicion goes first.

### 2. No pane-attribution mechanism gives authorization — medium

C7's proposal covers this, but it belongs here too because it bears on F1's
shipped decision. Env-supplied pane ids are forgeable by the agent, and so is
the per-pane socket: the sockets are listable and 0600-owned by the same user the
agent runs as. Per-pane roots would stop an accident, not an attempt. F1's threat
model is prompt injection through an auto-approved MCP surface — precisely the
actor who would forge. SECURITY.md should say so in the same paragraph that
describes what attribution enables.

### 3. spyc's DECCKM gap — medium

`encode_key` emits the CSI form unconditionally; `vt100::application_cursor()`
exists and is never read. Any child that sets DECCKM and parses strictly loses
its arrow keys. Confirmed empirically. Usually benign because most parsers accept
both forms. **Not fixed** — it is C1's leading candidate and fixing it without a
trace would be the speculative fix the brief forbids.

### 4. Worktree build caches fill the disk — medium

Each worktree carries its own ~3 GB `target/`. Thirteen of them took this
machine to 153 MB free and failed a build mid-engagement. `create_worktree` is
deliberately frictionless and nothing prunes or warns. Freed 32 GB by removing
eleven merged worktrees. Worth considering a low-disk warning on
`create_worktree`, or a `:worktree prune` for merged ones. **Not built.**

### 5. The 2026-07-02 retry widening treated a symptom — informational

`RUN_GIT_MAX_ATTEMPTS`' comment records a hook-time `index.lock: Not a
directory` failure attributed to "genuine transient contention" and fixed by
widening the budget 3 → 6. That was the C9 leak. The budget is now belt to
C9's braces; nothing needs changing, but the comment overstates what was
understood at the time.

### 6. `cargo deny` segfaulted in CI — informational, undecided

`deny` crashed with `Segmentation fault (core dumped)` on #255 after `fmt` and
`clippy` passed. cargo-deny is pinned and sha256-verified (0.19.4), so the binary
is deterministic — but `cargo deny check` **refetches the RUSTSEC advisory
database every run**, so the non-deterministic input is the database, not our
config, and #255 touches no dependencies. A re-run was therefore the correct
response rather than a stopgap: there is nothing in our control to fix. It passed
on the re-run (51s), so a single crash on freshly-fetched external input.

If it recurs, the resolution is pinning the advisory-DB snapshot — but that
trades away the fresh-advisory detection `audit.yml` exists for, so it should not
be changed on one data point. **Undecided, deliberately.**

### 7. `pick_best_rollout` residual — low

Two concurrent codex panes in one cwd where *neither* is resuming can still
resolve to the same rollout. Needs a claimed-rollout exclusion set threaded
through `TranscriptQuery`. Unchanged; still noted on #230.

## Process notes on my own conduct

- **I filled the disk** (item 4) and only noticed when a build failed. Cleaning
  worktrees as they merge, not at the end, would have avoided it.
- **A scripted regex edit across four files mangled brace/paren delimiters**,
  caught by `cargo check`, reverted, redone as an anchored insert. Second time
  this session a clever script was worse than a careful one.
- **I bundled C6's AGENTS.md rule into C5's commit** and had to lift it out and
  amend. The split is visible only in the history, not the diff.
- **My push cadence is what surfaced item 1.** The other session's idiom was
  already unsafe; frequent pushes to an alphabetically-earlier branch made it
  fire. Pausing pushes on request was the right call and worth offering sooner.
- **I followed the brief's Setup rule 3** (`never --no-verify`) against my own
  flagged objection, and it corrupted the repo within one commit. Flagging a
  concern and then complying is correct; the value was in having flagged it, so
  the cause was identified in minutes rather than hours.
