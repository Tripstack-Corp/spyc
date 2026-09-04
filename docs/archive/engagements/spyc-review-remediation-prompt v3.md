# spyc: code review remediation (v3)

You are working in the spyc repository (Tripstack-Corp/spyc). An external code
review was performed against commit `dbffd29`; its findings were verified and
corrected against `208d3ba`, then hardened against runtime knowledge from an
agent working inside the repo. The commits between the review baseline and
main are entirely mouse work — treat `src/app/mouse.rs` and neighbors as the
one area where "verify the finding still holds" is more than a formality, and
do not touch `src/app/mouse.rs` in this engagement (mouse work is in flight).

Read AGENTS.md first and follow every house convention it establishes —
comment style, the documentation contract, SPYC-TRAP anchors, conventional
commits, and the no-subprocess-git guard. Where a finding conflicts with an
existing documented decision in ROADMAP.md's decisions log or docs/archive/,
surface the conflict rather than silently overriding it.

## Setup (do this before anything else)

1. `git pull --ff-only` in the main checkout. `create_worktree` branches off
   local `main`; a stale local main silently omits recently-merged PRs.
2. Do NOT edit in the main checkout — a PreToolUse hook blocks edits there
   and you will burn turns on denied writes before diagnosing why. Create a
   worktree via the spyc MCP `create_worktree` tool and do all work in it.
3. Never `git commit --no-verify`. The pre-commit hook runs git-touching
   tests; if a hook run flakes, that flake is a bug to fix (see the F1 test
   note), never a reason to bypass the hook or re-run until green.

## Ground rules

1. **Verify before fixing.** Confirm each finding on current `main` before
   changing anything. If it no longer holds, say so and skip it.
2. **One finding, one PR-shaped change**, conventional-commit messages
   referencing the finding number (e.g. `fix(mcp): validate root override
   against session roots (review F1)`). No bundled cleanups.
3. **Tests first-class.** Every behavioral change ships with a test that
   fails before and passes after, placed per existing conventions.
4. **Run the full gate** (`make check` / CI-parity target) before declaring
   any finding done. All work passes with `--locked`.
5. **Docs in the same change.** ARCHITECTURE.md's "Documentation contract"
   applies: behavior change and its doc change land together.

## Order of work

F1 decision (no code — record it first) → F2 → F1 implementation → F3 → F6 →
F5 → F4 → F7.

F2 jumps the queue because it is the only finding with an external-trust
cost that compounds daily: SECURITY.md's distribution claims are visibly
contradicted by the repo's own release artifacts, and it is the first
document a security researcher reads.

## Findings

### F1 — MCP `root` override bypasses the path-traversal guard

**Where:** `src/mcp/readers.rs::effective_root`, consumed by
`get_file_content` (the canonicalize + `starts_with(canonical_root)` check
around `src/mcp/protocol.rs:615`), and by `search_paths`, `search_content`,
`git_status`, `git_log`, `git_diff`, and the worktree tools.

**Defect:** the traversal guard is anchored to a value the caller picks.
`effective_root` accepts any directory passing `is_dir()`; supply
`root: "/"` and the `starts_with` check is decorative.

**Why enforcement (not documentation) — get the rationale right.** The lazy
threat ("the agent could read `~/.ssh`") argues for documenting instead: the
agent typically has Bash and the socket is same-user 0600. The case that
matters is the **harness permission asymmetry**: agent harnesses commonly
auto-approve MCP tool calls while gating shell execution behind per-command
permission prompts. In that configuration,
`search_content(root: "/", regex: "BEGIN OPENSSH PRIVATE KEY")` silently
bypasses a boundary the user believes exists. That is why the resolution is
enforce.

**Design constraint — the allowed set must be cursor-independent.** Do NOT
anchor validation to the focused column. `search_root`, `project_home`, and
`list_worktrees` all track the column the *user* is browsing, which moves
independently of where the *agent* is working. Anchor to those alone and
this daily workflow breaks: agent works in `spyc.worktrees/feat/foo`, user
navigates the focused column to an unrelated project, and the agent's
`root: <its own worktree>` is rejected mid-task. The failure mode is worse
than the defect: a rejected MCP call doesn't stop the agent, it makes the
agent fall back to unscoped, unlogged `Bash rg` — over-tight scoping
produces bypass, not safety. The invariant: **the allow-list must never
reject the agent's own working root during a session.**

Cursor-independent anchors to build the allowed set from (implementer picks
the combination; all three are legitimate):

- all worktrees of the repo spyc was launched in — stable for the session,
  not merely the currently-focused one;
- any worktree created via `create_worktree` during this session;
- the focused column's chain (`search_root`/`project_home`/`cwd`) may be
  *included* — it just can't be the whole set.

**Known-ideal end state, out of scope here:** validating against the calling
pane's own cwd. `SPYC_PANE_ID` is already injected into every agent pane's
environment (`src/app/pane_tabs.rs:~126`) and the `spyc --mcp` proxy runs
inside that pane — but read-tool dispatch resolves through the context file
(`src/context.rs`), which carries no pane identity; `report_status` receives
pane_id as a tool *argument*, not from the transport. Per-pane root
attribution therefore needs transport plumbing that doesn't exist. Record it
in the decisions log as the target design; do not build it in this
engagement.

**Do:** validate the `root` override against the cursor-independent allowed
set. Reject anything else with a clear tool error naming the allowed set.
Canonicalize both sides (symlinked worktree paths must not false-reject).
Apply the validation everywhere `effective_root`'s override reaches — the
git_* tools and worktree tools included.

**Record the decision** in ROADMAP.md's decisions log with the
harness-asymmetry rationale and the pane-identity end state — the two things
a future maintainer needs when someone proposes relaxing or tightening the
check.

**Test note:** these tests exercise worktree creation/removal, which has
documented CI-flake history (gix index-lock contention under parallel
`cargo test`). Reuse the existing infrastructure —
`serialize_worktree_mutation` (`src/git/worktree.rs:~129`),
`write_index_with_lock_retry`, and the `src/git/test_support.rs` retry
helpers — rather than rolling new worktree scaffolding.

**Acceptance:** tests proving `root: "/"` and `root: "/tmp/not-a-worktree"`
are rejected; a legitimate sibling worktree is accepted, including via a
symlinked path; and the cursor-independence case specifically — the agent's
worktree root remains accepted after the context file's `search_root` moves
to an unrelated directory. For documentation: F2 will already have written
the agent-facing threat-model section from the recorded decision — verify
that section matches what F1 actually shipped and amend only if the
implementation diverged; do not write it twice.

### F2 — SECURITY.md contradicts the shipped distribution model

**Where:** `SECURITY.md` — line 60 ("There is no prebuilt binary
distribution"), lines 82 and 132 ("if/when we start publishing prebuilt
binaries"), and the license allow-list rationale pinned to v1.37.1.

**Problem:** binary distribution shipped at 2.0.0 (signed apt repo, Homebrew
tap, release tarballs, crates.io — see README.md and
`.github/workflows/{release,apt,homebrew}.yml`). The release pipeline is now
attack surface the security document doesn't know exists.

**Do:** rewrite SECURITY.md to match reality: how release binaries are built
and on what runner; how the apt signing key is generated, stored, and
scoped; what a consumer can verify (checksums, signatures) and how; the
key-compromise/rotation story; which GitHub Actions permissions the release
workflows hold and why; the cargo-deny license allow-list refreshed against
the current dep graph with the version reference updated; and the F1
decision folded into the threat model (state what the MCP tool surface does
and does not protect against with respect to the *connected agent*, not just
other local processes). Keep the existing register — "what we do, what we
don't, and why" — no boilerplate.

**Acceptance:** no claim in SECURITY.md contradicts README.md, INSTALL.md,
or the workflow files; a reader learns the actual release-signing chain from
SECURITY.md alone.

### F3 — Five state writes still bypass the existing atomic helper

**Correction to the original finding:** `src/fs/atomic.rs::write_atomic`
already exists and is used by `state/sessions/mod.rs`, `state/marks.rs`,
`mcp/config.rs`, and `mcp/hooks.rs`. Do **not** build a new helper; sessions
are already covered. What remains is mechanical — convert the five straggler
call sites that still `std::fs::write` persistent state directly:

- `src/state/frecency.rs:117`
- `src/state/pager_positions.rs:112`
- `src/state/history.rs:147`
- `src/state/hook_consent.rs:47`
- `src/state/graveyard.rs:202`

(Line numbers as of `208d3ba`; re-locate if drifted. Ignore `fs::write`
inside `#[cfg(test)]` blocks — fixture setup, not state persistence.)

**Acceptance:** not a one-time grep — add a source-scan **guard test** in
the house idiom (`no_subprocess_git_in_production`,
`traps_resolve_against_architecture_anchors`, etc.): a
`state_writes_are_atomic` guard asserting no production code path under
`src/state/` (and any other module writing under the XDG state root) calls
`std::fs::write` directly, with an allow-list mechanism for any deliberate
exception. The guard is what stops the sixth straggler appearing next month.
If `write_atomic`'s existing tests don't already cover
interrupted-write-preserves-prior-file, add that; otherwise no new write
machinery.

### F6 — Two divergent state-root resolvers; MCP's ignores XDG

**Correction to the original finding:** the bare-`/tmp` fallback is a
symptom. The defect is two resolvers:

- `state::state_root()` — honors `$XDG_STATE_HOME`, returns `Option`
  (no HOME → `None`, callers skip persistence safely)
- `mcp::state_dir()` — ignores `$XDG_STATE_HOME`, falls back to bare `/tmp`

With `XDG_STATE_HOME` set — common, unlike an unset HOME — the MCP socket
and trusted-root sidecar land in `~/.local/state/spyc` while all other state
goes to `$XDG_STATE_HOME/spyc`: state splits across two directories, and the
sidecar's "owner-private, attacker can't forge it" trust argument is being
made about a path the rest of the program isn't using.

**Do:** unify on one resolver — `mcp` consumes `state::state_root()` (or
both consume one shared function). For the `None` case on the MCP side,
prefer refusing to start the socket server (with a logged reason) over
inventing a world-readable fallback; document the choice. The `/tmp` branch
and any `/tmp`-squat hardening disappear with it.

**Acceptance:** one resolver; a test that sets `XDG_STATE_HOME` and verifies
the socket path and (e.g.) the frecency path share a root; the no-HOME
behavior on the MCP side is explicit and tested.

### F5 — Fuzz targets exist but never run in CI

**Where:** `fuzz/fuzz_targets/` (dsl_parse, expand_path, expand_percent,
highlight, render_markdown, word_wrap); `.github/workflows/`.

**Do:** add a workflow triggered on both `schedule` (weekly) and
`workflow_dispatch`, running each target for a bounded time (5–10 min each)
with corpora cached between runs, failing loudly on a crash and uploading
the reproducer as an artifact. Off the PR path — background insurance, not a
merge gate. Pin the nightly toolchain explicitly.

**Acceptance:** a manual `workflow_dispatch` run completes with corpora
cached and artifacts uploaded — the dispatch run is the evidence the wiring
works, since a scheduled run can't be demonstrated on demand.

### F4 — ROADMAP.md misstates what the ceiling guard enforces

**Correction to the original finding:** there is no 800-LoC guard. The only
ceiling guard is `mod_rs_stays_decomposed` (`src/app/mod_tests.rs:25`),
capping `src/app/mod.rs` alone at 1,500 lines. AGENTS.md states the 800-LoC
rule correctly as a convention with an escape hatch; the misleading text is
ROADMAP.md:33, which jams "ceiling-guard-enforced" and "the 800-LoC file
rule" into one clause. Twenty-plus production files exceed 800 lines; the
largest, `src/app/mouse.rs` (~2,035), is excluded from this engagement — it
is actively being worked.

**Do:** fix ROADMAP.md:33 so the documented claim matches the enforced
reality (a one-line change). Then, separately and only where natural seams
exist, **propose — do not execute without approval —** splits for the two or
three worst offenders outside the mouse area. Do NOT force artificial
splits; the `too_many_lines` rationale about dispatch functions applies to
files too.

**Acceptance:** documented rule and enforced rule are the same rule; any
splits are presented as a plan, not landed code.

### F7 — AGENTS.md is 43KB of always-loaded agent context (lowest priority)

**Do:** propose — do not execute without approval — a trim of AGENTS.md
toward a pointer document: keep invariants, guards, and conventions an agent
must never violate; move depth (worktree lifecycle narratives, extended
rationale) into the installable skill and the docs it references.

**Hard constraint:** the proposed trim may not remove any sentence naming a
guard, a SPYC-TRAP slug, or an invariant. AGENTS.md is a large part of why
agents don't wreck this repo; when in doubt, keep the sentence and flag it.

**Acceptance:** a cut list with per-section justification, awaiting
approval. No edits landed.

## What NOT to do

- Do not bump the version. `main` is the `N.M.0-CURRENT` stream; across a
  seven-PR engagement a version bump is the most common cross-PR mistake and
  stays invisible until release time.
- Do not `git commit --no-verify`, ever (see Setup).
- Do not touch `src/app/mouse.rs` or the in-flight mouse work.
- Do not touch the MVU structure, the sync-only concurrency model, the
  pager_stream seam, or the gix facade — reviewed favorably.
- Do not add dependencies. std plus the existing tree covers every finding.
- Do not build a new atomic-write helper (F3 — it exists).
- Do not add `/tmp`-squat hardening (F6 — the path it defends should not
  exist after unification).
- Do not roll new worktree test scaffolding (F1 — reuse
  `serialize_worktree_mutation` / `test_support` retry helpers).
- Do not reformat, rename, or "improve" code adjacent to your changes.
- Do not regenerate CHANGELOG.md by hand; git-cliff owns it.
- If any finding turns out to be wrong on current main, document why in your
  summary instead of manufacturing a fix.

## Deliverable

For each finding: the commits, the test evidence, and a two-line summary of
what changed and why. For F1, the decisions-log entry containing the
harness-asymmetry rationale, the cursor-independence invariant, and the
pane-identity end state. For F4 and F7, proposals awaiting approval, not
landed changes. Finish with anything you found along the way that all three
review passes missed — same severity framing, no fixes without asking.
