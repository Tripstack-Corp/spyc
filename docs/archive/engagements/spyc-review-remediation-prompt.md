# spyc: code review remediation

You are working in the spyc repository (Tripstack-Corp/spyc). An external code
review was performed against commit `dbffd29` (v2.1.0-CURRENT). Your job is to
address the findings below. Read AGENTS.md first and follow every house
convention it establishes — comment style, the documentation contract,
SPYC-TRAP anchors, conventional commits, the 800-LoC guidance, and the
no-subprocess-git guard. Where a finding conflicts with an existing documented
decision in ROADMAP.md's decisions log or docs/archive/, surface the conflict
rather than silently overriding it.

## Ground rules

1. **Verify before fixing.** Each finding was produced by static review at a
   specific commit. Before changing anything, confirm the finding still holds
   on current `main`. If it doesn't, say so and skip it — do not fix things
   that aren't broken.
2. **One finding, one PR-shaped change.** Keep each finding's work in its own
   commit series with a conventional-commit message referencing the finding
   number below (e.g. `fix(mcp): validate root override against worktree
   registry (review F1)`). Do not bundle unrelated cleanups.
3. **Tests first-class.** Every behavioral change ships with a test that fails
   before the change and passes after. Follow the existing test placement
   conventions (module `#[cfg(test)]` blocks, `harness_tests/`, or `tests/` as
   appropriate to the layer).
4. **Run the full gate** (`make check` or the equivalent CI-parity target)
   before declaring any finding done. All work must pass with `--locked`.
5. **Update documentation in the same change.** The docs contract in
   ARCHITECTURE.md §"Documentation contract" applies: if behavior changes,
   the doc that describes it changes in the same commit.

## Findings

### F1 — MCP `root` override bypasses the path-traversal guard (highest priority)

**Where:** `src/mcp/readers.rs::effective_root`, consumed by
`get_file_content`, `search_paths`, `search_content`, `git_status`, `git_log`,
`git_diff` dispatch in `src/mcp/protocol.rs`.

**Problem:** `get_file_content` canonicalizes and checks
`starts_with(canonical_root)` — but `effective_root` accepts any
caller-supplied directory with only an `is_dir()` check. An agent passing
`root: "/"` (or `~/.ssh`, etc.) reads or greps anything the invoking user can
read. The check is decorative whenever `root` is supplied. Tool descriptions
say "a sibling worktree you're working in," implying a scoping that does not
exist.

**Decide, then implement.** Two acceptable resolutions; pick one and record
the decision in ROADMAP.md's decisions log:

- **(a) Enforce (recommended):** validate the `root` override against the set
  of legitimate roots spyc already knows about — the worktrees returned by
  `list_worktrees` for the current repo, plus the context file's
  `search_root`/`project_home`/`cwd` chain. Reject anything else with a clear
  tool error naming the allowed set. Canonicalize both sides before
  comparison (symlinked worktree paths must not produce false rejections).
- **(b) Document:** if enforcement is rejected (e.g. because the agent has
  pane shell access anyway and the boundary is judged theater), state plainly
  in SECURITY.md and in every affected tool description that the `root`
  parameter is unscoped and MCP tools are **not** a privilege boundary against
  the connected agent.

If you choose (a), also audit whether the same unvalidated-`root` pattern
reaches `git_diff`/`git_log`/`git_status` and the worktree tools, and cover
those in the same enforcement.

**Acceptance:** a test proving `root: "/tmp/definitely-not-a-worktree"` (and
`root: "/"`) is rejected (option a) or updated docs + tool descriptions that
no longer imply scoping (option b). Either way, SECURITY.md's threat model
section explicitly states what the MCP socket does and does not protect
against with respect to the *connected agent*, not just other local processes.

### F2 — SECURITY.md is stale and no longer describes the distribution model

**Where:** `SECURITY.md`.

**Problem:** The doc states "There is no prebuilt binary distribution" while
README.md documents Homebrew, a signed apt repo, release tarballs, and
crates.io. The license allow-list rationale is pinned "as of v1.37.1" against
a current version of 2.1.0-CURRENT. Binary distribution materially changes
the threat model: the release pipeline (`.github/workflows/release.yml`,
`apt.yml`, `homebrew.yml`) is now attack surface and is uncovered.

**Do:** rewrite SECURITY.md to match reality. It must now cover: how release
binaries are built and by what runner; how the apt repo signing key is
generated, stored, and scoped; what a consumer can verify (checksums,
signatures) and how; what happens on key compromise (rotation story); which
GitHub Actions permissions the release workflows hold and why; and the
`cargo-deny` license allow-list refreshed against the current dep graph with
the version reference updated. Fold in the F1 decision. Keep the existing
register — "what we do, what we don't, and why" — and do not pad it with
boilerplate.

**Acceptance:** no claim in SECURITY.md contradicts README.md, INSTALL.md, or
the workflow files. A reviewer reading only SECURITY.md learns the actual
release-signing chain.

### F3 — Non-atomic state writes

**Where:** confirmed in `src/state/frecency.rs::save` (`std::fs::write`
directly); audit the whole `src/state/` family (harpoon, marks, inventory
metadata, pager_positions, graveyard JSON sidecars) and
`src/app/session.rs` snapshot writes for the same pattern.

**Problem:** a crash or power loss mid-write leaves truncated JSON/TOML. The
startup health check mitigates by detecting and discarding corrupt files, but
the session autosave's entire purpose is crash survival — a crash *during*
the save is exactly when the snapshot matters, and today it can destroy the
previous good snapshot.

**Do:** introduce one small write-atomic helper (temp file in the same
directory + `rename`, with the temp name unlinkable on failure) in the
appropriate shared module, and convert every persistent-state write site to
use it. Do not add fsync-on-every-write unless a site genuinely needs
durability over performance — atomicity (no torn files) is the requirement;
note the distinction in the helper's doc comment. Sessions get priority.

**Acceptance:** a test that simulates interruption (write the temp, don't
rename, re-run load) proving the prior snapshot survives; grep shows no
remaining direct `fs::write` to files under the XDG state root from
production code paths.

### F4 — The 800-LoC file rule no longer describes the tree

**Where:** roughly ten production files between ~850 and ~1,400 lines:
`app/mod.rs` (1,381), `app/agent_status.rs` (1,263), `app/render/mod.rs`
(1,227), `app/effect.rs` (1,178), `app/state/mod.rs`, `app/pane_tabs.rs`,
`mcp/protocol.rs`, `mcp/config.rs`, `mcp/hooks.rs`, others near the line.

**Problem:** ROADMAP.md presents the 800-LoC rule as enforced by ceiling
guards. Either the guards use per-file baselines (in which case the rule as
documented is misleading) or the rule has eroded.

**Do:** first, determine what the guard actually enforces (read the Makefile
/ scripts). Then either (a) split the worst offenders where a natural seam
exists — do NOT force artificial splits that hurt navigation; the existing
`too_many_lines` clippy-allow rationale about dispatch functions applies to
files too — or (b) amend the documented rule to what is actually enforced
(e.g. "800 LoC for new files; legacy files ratcheted via baseline"). A mix is
fine: split the two or three files with obvious seams, ratchet the rest.

**Acceptance:** the documented rule and the enforced rule are the same rule.

### F5 — Fuzz targets exist but never run in CI

**Where:** `fuzz/fuzz_targets/` (dsl_parse, expand_path, expand_percent,
highlight, render_markdown, word_wrap); `.github/workflows/`.

**Do:** add a scheduled (weekly is fine) workflow that runs each fuzz target
for a bounded time (e.g. 5–10 min each) with the corpus cached between runs,
failing loudly on a crash and uploading the reproducer as an artifact. Keep
it off the PR path — this is background insurance, not a merge gate. Pin the
nightly toolchain it needs explicitly rather than floating.

**Acceptance:** workflow exists, is schedule-triggered, caches corpora, and a
deliberately-introduced panic in one target (verified locally, not
committed) demonstrates the failure path works.

### F6 — `state_dir()` falls back to bare `/tmp` when `$HOME` is unset

**Where:** `src/mcp/mod.rs::state_dir` (and check for the same fallback in
`src/state/`'s XDG resolution).

**Problem:** sockets and trusted-root marker sidecars land directly in
world-readable `/tmp`. The 0600 socket permission holds, but the `.root`
sidecar's trust argument ("owner-private, attacker can't forge it") weakens
in a shared directory, and predictable names invite squatting.

**Do:** fall back to a per-user directory (`/tmp/spyc-<uid>`) created with
0700, verifying ownership and mode on reuse (refuse a pre-existing dir owned
by someone else — classic /tmp squat defense). Honor `$XDG_STATE_HOME` before
`$HOME` if it isn't already consulted.

**Acceptance:** test covering the no-`$HOME` path; the squat-refusal branch
has a test.

### F7 — AGENTS.md is 43KB of always-loaded agent context (lowest priority)

**Do:** trim AGENTS.md toward a pointer document: keep the invariants, guards,
and conventions an agent must never violate; move depth (worktree lifecycle
narratives, extended rationale) into the installable skill and the docs it
already references. Target is meaningful reduction without losing any
load-bearing rule — if in doubt about whether a section is load-bearing,
keep it and flag it. This is judgment work; do it last, and propose the cut
list before executing it.

## Order of work

F1 → F2 (F2 folds in F1's decision) → F3 → F6 → F5 → F4 → F7.

## What NOT to do

- Do not touch the MVU structure, the sync-only concurrency model, the
  pager_stream seam, or the gix facade — these were reviewed favorably.
- Do not add dependencies for F3 or F6; std is sufficient. (tempfile is
  already a dev-dependency; keep it out of the production graph unless it
  already is one.)
- Do not reformat, rename, or "improve" code adjacent to your changes.
- Do not regenerate CHANGELOG.md by hand; the git-cliff flow owns it.
- If any finding turns out to be wrong on current main, document why in your
  summary instead of manufacturing a fix.

## Deliverable

For each finding: the commits, the test evidence, and a two-line summary of
what changed and why. For F1 and F4, the decision recorded in ROADMAP.md's
decisions log. Finish with anything you found along the way that the review
missed — same severity framing, no fixes without asking.
