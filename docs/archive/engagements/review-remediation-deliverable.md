# Code-review remediation — deliverable

Engagement brief: `docs/drafts/spyc-review-remediation-prompt v4.md`.
Baseline `208d3ba` → head `edc27b8`. Twelve PRs, all merged.

## Summary

| # | Finding | PR | Outcome |
|---|---|---|---|
| F1 | MCP `root` override bypasses the traversal guard | #235 (decision), #237 (impl) | fixed |
| F2 | SECURITY.md contradicts the shipped distribution | #236 | fixed |
| F3 | State writes bypass the atomic helper | #238 | fixed + guard |
| F6 | Two divergent state-root resolvers | #239 | fixed |
| F5 | Fuzz targets never run | #240, #242 | fixed, dispatch verified |
| F4 | ROADMAP misstates the 800-LoC rule | #241 (docs), #244–#246 (mouse split) | fixed + executed |
| F7 | AGENTS.md is 43KB of always-loaded context | #243 | executed |

Every change gated with `make check`; no CI failure reached `main`.

## Per finding

**F1 — `d86dcb4`, `f851fcd`.** `effective_root` accepted any directory passing
`is_dir()`, and `get_file_content` anchored its canonicalize + `starts_with`
check on that same caller-supplied value, so `root: "/"` made the guard
decorative across all six read tools. Now validated against a
cursor-independent allowed set.
*Tests:* `/` and an unrelated directory rejected; the agent's root survives
`search_root`/`project_home`/`cwd` all moving elsewhere; a symlinked path is
accepted.

**F2 — `6abc6d7`.** SECURITY.md claimed "no prebuilt binary distribution" and
that signing "would be theater", while the repo ships signed tarballs, a brew
tap, a signed apt repo, and crates.io. Rewritten around release-pipeline
compromise, the keyless chain, and the apt key as the one stored secret.
*Tests:* n/a (docs); cross-checked against README, INSTALL, and the workflows.

**F3 — `1265900`.** Eight persistent-state writes used `std::fs::write`, so a
crash mid-write truncated the file and the health check's only recourse was to
discard it. All now use the existing `write_atomic`.
*Tests:* `state_writes_are_atomic` source-scan guard, **verified to fail** on an
injected canary before being trusted.

**F6 — `72d3ae7`.** `mcp::state_dir()` read `$HOME` directly and ignored
`$XDG_STATE_HOME`, splitting state across two directories for anyone who sets
it. Unified on `state::state_root()`; callers decline when there's no
trustworthy location rather than falling back to a world-readable `/tmp`.
*Tests:* socket and sidecar share the overridden root; the absent-dir branch
asserted via an injected parameter (no `env::set_var`, which is unsafe in
edition 2024).

**F5 — `328477b`, `d19ab5a`.** Six libFuzzer targets had no scheduled runner.
Weekly `schedule` + `workflow_dispatch`, 6-target matrix, accumulating corpus
cache, pinned nightly.
*Tests:* **a real `workflow_dispatch` run: all six targets green.** The first
attempt failed on all six — `taiki-e` installs the musl build of cargo-fuzz,
which defaults `--target` to its own host triple, and ASAN cannot work with
static libc. Fixed in `d19ab5a`.

**F4 — `7c9b042`, `6643a56`, `84c3ba8`, `edc27b8`.** ROADMAP listed the 800-LoC
rule among finished foundations, reading as compliance. Corrected; and
`src/app/mouse.rs` (1,529 production lines, the tree's largest) decomposed.
*Tests:* 1,788 tests pass unchanged at each step — pure relocation.

    mod.rs 307 · route.rs 526 · selection.rs 399 · forward.rs 193 · scroll.rs 182

**F7 — `4b99400`.** 43,256 → 33,877 bytes (−22%), no rule removed. Chosen by
measurement: `## Conventions` is 17% of the bytes and holds 17 of the 20
load-bearing markers; `## What it does` was 38% and held none.
*Tests:* every sentence naming a guard / SPYC-TRAP / invariant extracted from
the old file and asserted against the new one.

## The F1 decisions-log entry

Recorded in ROADMAP.md, carrying the three things the brief required:

1. **Harness asymmetry** — enforcement is justified not by "the agent could read
   `~/.ssh`" (it has Bash) but because harnesses auto-approve MCP calls while
   gating shell execution behind per-command prompts. There,
   `search_content(root: "/")` bypasses a boundary the *user* believes exists.
2. **Cursor-independence invariant** — the allowed set must never reject the
   agent's own working root. Anchoring on `search_root` alone would reject it
   the moment the user browses elsewhere, and a rejected call sends the agent to
   unscoped `Bash rg`. Over-tight scoping produces bypass, not safety.
3. **Pane-identity end state** — validating against the calling pane's cwd is the
   target design, blocked on transport: `SPYC_PANE_ID` reaches the pane env, but
   read-tool dispatch resolves through the context file, which carries no pane
   identity.

## What all four review passes missed

Severity-framed. **Nothing below was fixed without asking.**

### Fixed in passing (surfaced during the work)

- **SECURITY.md *understated* the posture** — medium. The review caught the
  stale distribution claim but not that the doc said signing "would be theater"
  while `release.yml` has produced SLSA attestations and cosign signatures since
  2.0.0. A reader was talked out of verification that already worked.
- **The untrusted-input claim was false** — medium. "No untrusted-input parser
  beyond TOML config files we already control" sat next to six fuzz targets.
  The pager renders arbitrary files, vt100 parses arbitrary child output, gix
  parses git objects. That sentence was the justification for not fuzzing.
- **F3 was eight sites, not five** — low/medium. `inventory`, `harpoon`, and
  `skill_prompt` appeared in neither the review nor the hand-audit after it.
- **F6 was a resolver divergence, not a `/tmp` fallback** — medium. The `/tmp`
  case needs an unset `$HOME`; the divergence bites anyone who sets
  `$XDG_STATE_HOME`.
- **F4's "twenty-plus oversized files" was a measurement artifact** — low. Raw
  `wc -l` counts inline tests; the real figure is 16.
- **No apt key-rotation procedure existed** — medium. Now written down,
  including that every client must re-import because `signed-by` pins the old
  key, and that it has never been rehearsed.
- **Unsigned commits are the weakest link** — informational. Signing proves an
  artifact came from the workflow, not that the source was authored by who you
  think.

### Open — needs your decision, not fixed

**1. The guard-test idiom has a latent blind spot — medium.**
`no_subprocess_git_in_production` and the new `state_writes_are_atomic` both
split production from tests at the first `#[cfg(test)]` **substring**. A comment
merely *mentioning* the attribute truncates the scan: under that heuristic
`src/app/render/mod.rs` reads as 22 production lines when it has 827. No
`src/state` file triggers it today, so neither guard is currently blinded — but
a guard with a silent false negative is worse than no guard, because it reads as
assurance. Fix is small (match at line start, or brace-match). Not done here
because it changes a shared house idiom that predates this engagement.

**2. Issue #230 is half-fixed, and its two proposed fixes don't compose —
medium.** The pin-window timing bug is fixed. But `assign_codex_sessions`
filters on `r.started_secs + START_SKEW_SECS >= *spawn`, and a resumed codex
session appends to its original rollout with a frozen timestamp — so a resumed
tab is *structurally unpinnable* and falls through to the mtime ranking. Fixing
the ranking alone doesn't reach it. Commented on the issue and re-scoped; left
open deliberately.

**3. Agent-pane input deafness after a focus round-trip — high, unreproduced.**
`docs/drafts/mutli-question-bug-investigation.md`. Two candidate mechanisms with
opposite predictions (a latched `resolver_pending` vs. Ink dropping its handler
on a SIGWINCH re-render), a one-second discriminating test (press `Esc` before
`^c`), and a decisive one (`SPYC_KEY_TRACE=1`, whose RX line already logs
`pane_focused=` and `pending=`).

**4. mouse.rs tests did not move with their subjects — low.** `mod.rs` is 1,815
total against 307 production. Relocating each test cluster is a natural
follow-up; doing it inside the split would have turned a reviewable relocation
into a rewrite.

## Process notes

- **A piped gate reports the pager's exit code.** `make check 2>&1 | tail -20`
  returns *tail's* status, so a failed gate reads as green. F1 was pushed with a
  failing `fmt-check` that CI then caught. Fixed by `set -o pipefail`; now in the
  brief's don't-do list. Worth a `.PHONY` wrapper or a documented invocation.
- **`gh pr merge` immediately after `gh pr update-branch` fails** with "2 of 2
  required status checks are expected" — the update re-triggers checks and the
  merge races their registration. Needs a wait-then-retry loop; bit every PR in
  the sequence.
