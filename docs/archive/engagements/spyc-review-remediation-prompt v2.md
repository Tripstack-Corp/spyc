# spyc: code review remediation (v2)

You are working in the spyc repository (Tripstack-Corp/spyc). An external code
review was performed against commit `dbffd29`; its findings were then verified
and corrected against `208d3ba` (current main at time of writing). The 15
commits between the two are entirely mouse work — so treat `src/app/mouse.rs`
and its neighbours as the one area where "verify the finding still holds" is
more than a formality, and be aware mouse work may still be in flight (PR
#234) — do not touch `src/app/mouse.rs` in this engagement.

Read AGENTS.md first and follow every house convention it establishes —
comment style, the documentation contract, SPYC-TRAP anchors, conventional
commits, and the no-subprocess-git guard. Where a finding conflicts with an
existing documented decision in ROADMAP.md's decisions log or docs/archive/,
surface the conflict rather than silently overriding it.

## Ground rules

1. **Verify before fixing.** Confirm each finding on current `main` before
   changing anything. If it no longer holds, say so and skip it — do not fix
   things that aren't broken.
2. **One finding, one PR-shaped change.** Keep each finding's work in its own
   commit series with a conventional-commit message referencing the finding
   number (e.g. `fix(mcp): validate root override against known roots (review
   F1)`). Do not bundle unrelated cleanups.
3. **Tests first-class.** Every behavioural change ships with a test that fails
   before the change and passes after, placed per existing conventions.
4. **Run the full gate** (`make check` or the CI-parity target) before
   declaring any finding done. All work must pass with `--locked`.
5. **Update documentation in the same change.** ARCHITECTURE.md's
   "Documentation contract" applies: behaviour change and its doc change land
   in the same commit.

## Order of work

F1 decision (no code — record it first) → F2 → F1 implementation → F3 → F6 →
F5 → F4 → F7.

F2 jumps the queue because it is the only finding with an external-trust cost
that compounds daily: SECURITY.md makes claims about the distribution model
that a public repo's release artifacts visibly contradict, and it is the
first document a security researcher reads.

## Findings

### F1 — MCP `root` override bypasses the path-traversal guard

**Where:** `src/mcp/readers.rs::effective_root`, consumed by
`get_file_content` (the canonicalize + `starts_with(canonical_root)` check
around `src/mcp/protocol.rs:615`), and by `search_paths`, `search_content`,
`git_status`, `git_log`, `git_diff`, and the worktree tools.

**Defect:** the traversal guard is anchored to a value the caller picks.
`effective_root` accepts any directory passing `is_dir()`; supply
`root: "/"` and the `starts_with` check is decorative.

**Why this justifies enforcement — get the rationale right.** The lazy
threat ("the agent could read `~/.ssh`") argues for documentation instead:
the connected agent typically has Bash, the socket is same-user 0600, and
anything that can reach the socket can already `cat` the file. The case that
actually matters is the **harness permission asymmetry**: agent harnesses
commonly auto-approve MCP tool calls while gating shell execution behind
per-command permission prompts. In that configuration,
`search_content(root: "/", regex: "BEGIN OPENSSH PRIVATE KEY")` silently
bypasses a boundary the user believes exists. That is not theatre, and it is
why the resolution is **(a) enforce**, not (b) document.

**Do:** validate the `root` override against the set of legitimate roots
spyc already knows — the worktrees `list_worktrees` reports for the current
repo, plus the context file's `search_root`/`project_home`/`cwd` chain.
Reject anything else with a clear tool error naming the allowed set.
Canonicalize both sides before comparison (symlinked worktree paths must not
false-reject). Apply the same validation everywhere `effective_root`'s
override reaches — the git_* tools and worktree tools included, not just
`get_file_content`.

**Record the decision** in ROADMAP.md's decisions log, and include the
harness-asymmetry sentence above as the stated rationale — it is the part a
future maintainer will need when someone proposes relaxing the check.

**Acceptance:** tests proving `root: "/"` and
`root: "/tmp/not-a-worktree"` are rejected while a legitimate sibling
worktree from `list_worktrees` is accepted (including via a symlinked path);
SECURITY.md's threat model explicitly states what the MCP tool surface does
and does not protect against with respect to the *connected agent*, not just
other local processes.

### F2 — SECURITY.md contradicts the shipped distribution model

**Where:** `SECURITY.md` — line 60 ("There is no prebuilt binary
distribution"), lines 82 and 132 ("if/when we start publishing prebuilt
binaries"), and the license allow-list rationale pinned to v1.37.1.

**Problem:** binary distribution shipped at 2.0.0 (signed apt repo, Homebrew
tap, release tarballs, crates.io — see README.md and
`.github/workflows/{release,apt,homebrew}.yml`). The release pipeline is now
attack surface and the security document does not know it exists.

**Do:** rewrite SECURITY.md to match reality. Cover: how release binaries
are built and on what runner; how the apt signing key is generated, stored,
and scoped; what a consumer can verify (checksums, signatures) and how; the
key-compromise/rotation story; which GitHub Actions permissions the release
workflows hold and why; the cargo-deny license allow-list refreshed against
the current dep graph with the version reference updated; and the F1
decision folded into the threat model. Keep the existing register — "what we
do, what we don't, and why" — no boilerplate padding.

**Acceptance:** no claim in SECURITY.md contradicts README.md, INSTALL.md,
or the workflow files; a reader learns the actual release-signing chain from
SECURITY.md alone.

### F3 — Five state writes still bypass the existing atomic helper

**Correction to the original finding:** `src/fs/atomic.rs::write_atomic`
already exists and is already used by `state/sessions/mod.rs`,
`state/marks.rs`, `mcp/config.rs`, and `mcp/hooks.rs`. Do **not** build a
new helper, and sessions are already covered. What remains is mechanical:
convert the five straggler call sites that still `std::fs::write` persistent
state directly:

- `src/state/frecency.rs:117`
- `src/state/pager_positions.rs:112`
- `src/state/history.rs:147`
- `src/state/hook_consent.rs:47`
- `src/state/graveyard.rs:202`

(Line numbers as of `208d3ba`; re-locate if drifted. Ignore `fs::write`
inside `#[cfg(test)]` blocks — those are fixture setup, not state
persistence.)

**Acceptance:** after conversion, a grep shows no remaining direct
`fs::write` to files under the XDG state root from production code paths.
If `write_atomic`'s existing tests don't already cover the
interrupted-write-preserves-prior-file property, add one; otherwise no new
test machinery is needed.

### F6 — Two divergent state-root resolvers; MCP's ignores XDG

**Correction to the original finding:** the bare-`/tmp` fallback is a
symptom, not the defect. The defect is that spyc has two resolvers:

- `state::state_root()` — honours `$XDG_STATE_HOME`, returns `Option`
  (no HOME → `None`, callers skip persistence safely)
- `mcp::state_dir()` — ignores `$XDG_STATE_HOME` entirely, falls back to
  bare `/tmp`

With `XDG_STATE_HOME` set — a common configuration, unlike an unset HOME —
the MCP socket and trusted-root sidecar land in `~/.local/state/spyc` while
all other state goes to `$XDG_STATE_HOME/spyc`: state splits across two
directories, and the sidecar's "owner-private, attacker can't forge it"
trust argument is being made about a path the rest of the program isn't
using.

**Do:** unify on one resolver — `mcp` consumes `state::state_root()` (or
both consume one shared function). Decide and document the policy for the
`None` case on the MCP side: refusing to start the socket server (with a
logged reason) is preferable to inventing a world-readable fallback
location. The `/tmp` branch and any need for `/tmp`-squat hardening
disappear with it.

**Acceptance:** one resolver; a test that sets `XDG_STATE_HOME` and
verifies the socket path and, e.g., the frecency path share a root; the
no-HOME behaviour on the MCP side is explicit and tested.

### F5 — Fuzz targets exist but never run in CI

**Where:** `fuzz/fuzz_targets/` (dsl_parse, expand_path, expand_percent,
highlight, render_markdown, word_wrap); `.github/workflows/`.

**Do:** add a workflow triggered on **both** `schedule` (weekly) and
`workflow_dispatch`, running each target for a bounded time (5–10 min each)
with the corpus cached between runs, failing loudly on a crash and uploading
the reproducer as an artifact. Keep it off the PR path — background
insurance, not a merge gate. Pin the nightly toolchain explicitly rather
than floating.

**Acceptance:** a manual `workflow_dispatch` run completes with corpora
cached and artifacts uploaded — that dispatch run is the evidence the
failure path is wired, since a scheduled run can't be demonstrated on
demand.

### F4 — ROADMAP.md misstates what the ceiling guard enforces

**Correction to the original finding:** there is no 800-LoC guard. The only
ceiling guard in the tree is `mod_rs_stays_decomposed`
(`src/app/mod_tests.rs:25`), which caps `src/app/mod.rs` alone at 1,500
lines. AGENTS.md states the 800-LoC rule correctly as a convention with an
escape hatch; the misleading text is ROADMAP.md:33, which jams
"ceiling-guard-enforced" and "the 800-LoC file rule" into one clause.
Meanwhile twenty-plus production files exceed 800 lines, the largest being
`src/app/mouse.rs` at ~2,035 (excluded from this engagement per the header —
it is actively being worked).

**Do:** fix the ROADMAP.md:33 wording so the documented claim matches the
enforced reality (a one-line change). Then, separately and only if natural
seams exist, propose — do not execute without approval — splits for the two
or three worst offenders outside the mouse area. Do NOT force artificial
splits; the existing `too_many_lines` rationale about dispatch functions
applies to files too.

**Acceptance:** the documented rule and the enforced rule are the same
rule; any proposed splits are presented as a plan, not landed code.

### F7 — AGENTS.md is 43KB of always-loaded agent context (lowest priority)

**Do:** propose — do not execute without approval — a trim of AGENTS.md
toward a pointer document: keep invariants, guards, and conventions an agent
must never violate; move depth (worktree lifecycle narratives, extended
rationale) into the installable skill and the docs it already references.

**Hard constraint:** the proposed trim may not remove any sentence naming a
guard, a SPYC-TRAP slug, or an invariant. AGENTS.md is a large part of why
agents don't wreck this repo; when in doubt, keep the sentence and flag it.

**Acceptance:** a cut list with per-section justification, awaiting
approval. No edits landed in this engagement.

## What NOT to do

- Do not touch `src/app/mouse.rs` or the in-flight mouse work.
- Do not touch the MVU structure, the sync-only concurrency model, the
  pager_stream seam, or the gix facade — these were reviewed favourably.
- Do not add dependencies. std plus the existing tree is sufficient for
  every finding here.
- Do not build a new atomic-write helper (see F3 — it exists).
- Do not add `/tmp`-squat hardening (see F6 — the path it defends should
  not exist after unification).
- Do not reformat, rename, or "improve" code adjacent to your changes.
- Do not regenerate CHANGELOG.md by hand; the git-cliff flow owns it.
- If any finding turns out to be wrong on current main, document why in
  your summary instead of manufacturing a fix.

## Deliverable

For each finding: the commits, the test evidence, and a two-line summary of
what changed and why. For F1, the decisions-log entry containing the
harness-asymmetry rationale. For F4 and F7, proposals awaiting approval, not
landed changes. Finish with anything you found along the way that both
reviews missed — same severity framing, no fixes without asking.
