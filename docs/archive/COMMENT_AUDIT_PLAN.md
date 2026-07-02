# Comment Audit — recheck every comment block against the AGENTS.md standard

**Status:** COMPLETE 2026-06-26. Phase 0 calibration (10 files / 323 blocks)
measured ~7% violations with the core pristine; B was pocketed, not
codebase-wide — so Phase 1 was **targeted, not a 12-PR campaign**:
- `#564` — state/inventory + history field/method-doc trim (7 deletions).
- the `comment-audit-phase1-finish` PR — resolver-test assert-restating trim
  (6 deletions) + production future-work temporal rot (pane/pty_host/dsl/markdown).
Per-owner decision the test-comment pass was scoped to the resolver tests
only (the rest of the test tree is low-visibility; left as-is). The slop
guard (#561) + the sharpened AGENTS.md standard prevent regrowth.

## Why

A spot-check found committed reasoning-leakage — a resolver test debating
itself ("`// …actually let's check / Actually looking at the code / Wait,
actually …`") above a one-line assert (deleted in #561). That class of slop
reads as careless and **taints the credibility of the genuine load-bearing
`why` comments around it**, which is the actual cost — external reviewers see
one bad comment and assume the whole ~22% comment density is slop.

This charter is the one-time correction: run **every** comment block past the
AGENTS.md comment standard and remove the failures, keeping the `why`.

## Already shipped (don't re-litigate)

- **The standard** (AGENTS.md → "Comments state what IS"): a comment earns its
  place only by explaining a non-obvious decision / invariant / gotcha the code
  can't show; never narrate what the code says; never commit reasoning-in-progress.
- **The slop guard** (#561): `app::mod_tests::guard_tests::comments_carry_no_reasoning_leakage`
  fails `make check` on a curated set of deliberation phrases. Reasoning-leakage
  (category C) is now structurally prevented; this audit is about the rest.

## The rubric (what each comment is checked against)

A comment is **KEEP** only if it explains a non-obvious **decision / invariant /
gotcha / cross-module interaction / regression** the code itself can't show.
It is **DELETE** (or FIX) if it:

- **B — narrates what the code already says** — `// increment counter` over `i += 1`.
- **C — reasoning-in-progress** — `// wait, actually…` (now guarded; sweep mops up any miss).
- **temporal rot** — "for now" / "until X lands" / "with the Y PR" / "stays on Z until…".
- **restates a name / type / signature** — `/// Returns the count` on `fn count()`.
- **bakes a version literal as an example** (existing rule; use `<x.y.z>`).

**Default to KEEP.** The dense `why` is the asset that lets agents work in this
codebase; over-deleting it is the real risk, not leaving a borderline comment.

## Grounded finding (this sets the scale)

Three high-density files sampled during scoping — `resolver/mod.rs` (hot path),
`app/effect.rs` (hot path, 33% comments), `app/util.rs` (leaf helpers, prime
narrate-the-obvious territory) — are **almost entirely category A**. The 22%
density is mostly legitimate `why`, not narration. So the working hypothesis is
**B is modest and pocketed** (tests, older/less-touched corners), not a
codebase-wide flood. A blind "rewrite all ~16k comment lines" pass would be
high-cost, high-risk, low-yield. **Scale the effort to Phase 0's measured rate.**

## Method (semantic → classify, adversarially keep, then delete)

Per file / module group:

1. **Classify** — an agent labels each comment KEEP / DELETE / FIX with a
   one-line reason. **No edits.**
2. **Adversarial keep-check** — a second (skeptic) agent tries to *justify
   keeping* each DELETE candidate. Only comments that survive both (genuine
   noise) are removed.
3. **Apply** — delete the consensus set; **deletions-only diff** (no code
   changes). `make check` stays green; SPYC-TRAP anchors and the slop guard
   stay intact.
4. **Owner review** — a focused, per-module-group diff (not one mega-diff).

## Phased plan

### Phase 0 — Calibrate (cheap, do first; no code change)

Classify ~8–10 files spanning every module group; report the real
KEEP / DELETE / rot rate and **where B concentrates**. Output: a go/no-go and a
sized Phase 1. This is the decision gate — is this a 2-PR touch-up or a 12-PR
campaign?

### Phase 1 — sized by Phase 0

- **B pocketed** → a few **targeted PRs** on the offender files only.
- **B widespread** → **per-module-group audit PRs**, one per group below.

Module groups (chunking for Phase 1 / one PR each):
`src/app/state` · `src/app/render` · `src/app/*` (handlers) · `src/git` ·
`src/ui` · `src/fs` · `src/pane` · `src/mcp` · `src/keymap` · `src/state` ·
`src/config`+`src/shell` · top-level (`src/*.rs`).

## Per-PR conventions

- Set up via spyc's `create_worktree` / tear down with `remove_worktree`.
- Deletions-only, behavior-preserving; `make check` green.
- Bump the version per PR (patch); squash-merge `--close-source=false` after CI.

## Definition of done

Every module group audited; category B / temporal rot removed; category A intact;
density lands where the noise was (estimated ~14–16% if B turns out widespread —
but Phase 0 decides). The standard + slop guard keep it from regrowing.
