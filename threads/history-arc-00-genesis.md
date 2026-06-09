# history-arc-00-genesis — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: history-arc-00-genesis
Created: 2026-06-09T04:46:50.420671+00:00

---
Entry: Claude Code (caleb) 2026-06-09T04:46:50.420671+00:00
Role: scribe
Type: Note
Title: Framing — the pre-PR genesis era (2026-04-15 → 04-29, 244 direct commits)

Spec: scribe

tags: #history #genesis

Moment: genesis — Framing the pre-PR era   [kind: segment-topology]
When: 2026-04-15 (root commit 57ae2733) → 2026-04-29 (final genesis commit 265d2816)
Recorded rationale: this thread's substrate is commit messages + genesis-era committed docs (ROADMAP.md, FEATURES.md, DESIGN.md, ARCHITECTURE.md, CLAUDE.md, REFACTOR_PLAN.md), quoted verbatim where load-bearing.
Inferred intent: (none — this is a framing note)
Supersedes: (none — this is the earliest segment; `00` prefix sorts before history-arc-01-foundation-hygiene)

COVERAGE NATURE (honesty disclosure). This era PREDATES the Bitbucket PR workflow. There are NO PR numbers, NO PR bodies, NO review threads. The first merged PR (#2, merge d9b9360) lands AFTER this range. The 244 commits here went direct-to-main. So "recorded rationale" = the commit subjects/bodies (which are unusually descriptive and version-tagged in this era) plus the genesis versions of committed planning/design docs. All citations are by commit SHA + date, never PR#. Inferred reads are marked.

THE NAME. The project was born as `cspy` (root: commit 57ae2733, 2026-04-15, "Initial commit: cspy — vi-keyboard file commander with Claude Code pairing planned"). The recorded etymology: "A Rust clone of SideFX's in-house `spy` tool ... Designed to pair with Claude Code (embedded pty pane) — hence the name: c(laude) + spy." It was renamed to `spyc` exactly once, at v1.0.0 (commit 28c4d329, 2026-04-17): "cspy is now spyc (spy + claude = spicy 🌶️)". That single commit renamed package, binary, config (.cspyrc.toml → .spycrc.toml), state dirs, env vars, the debug macro, all source, docs, build files, and CI; it also added the 🌶️ pepper branding. The rename is the segment's central topology moment — everything before 28c4d329 is `cspy`, everything after is `spyc`. (Pickaxe `git log -S 'spyc'` confirms 28c4d329 is the first commit introducing the literal token "spyc".)

THE THROUGHLINE. The final genesis commit (265d2816, 2026-04-29) authors REFACTOR_PLAN.md: the staged path from `app/mod.rs` (~7400 lines) → MVU. That doc names the architecture's central tension on day ~14, BEFORE the PR workflow existed. It is the plan that the later windows' MVU refactor (see history-seg-refactor-mvu) and the module decomposition (see history-seg-module-decomposition) execute. The 7400-line monolith named here is the one later split. This thread quotes its genesis version where it lands.

CADENCE. v0.9 → v1.0 → ~v1.37 all land inside these 14 days — heavy minor-version cadence, often several bumps per day. This contrasts sharply with the later windows' patch-corridor cadence (cf. insight-recurrence Pattern 5 / window-2). Author throughout is Derek Marshall, co-authored with Claude.

Budget: ~16–20 moments follow, chronological, clustered by milestone (M4/M6/M8/M9/...) and subsystem birth (pager, pane/pty, input/marks, config DSL, git status, markdown, project-search). Dense version-bump runs are folded with explicit +N lines; every one of the 244 commits is either a moment or named in a fold.

Provenance:
- 57ae2733 (root, 2026-04-15 13:09) — cspy initial commit; M1–M3 + polish; src/app.rs 843 lines, column-major listing, vi motion, picks/inventory, shell-out, EDITOR/PAGER integration, incremental search, ui/theme.rs Tokyo-Night palette.
- 28c4d329 (2026-04-17 18:13) — the cspy→spyc rename; pepper branding; 0.13.0→1.0.0.
- 265d2816 (2026-04-29 21:14) — REFACTOR_PLAN.md authored; app/mod.rs measured at 7421 lines (verified `git show 265d2816:src/app/mod.rs | wc -l`).

<!-- Entry-ID: 01KTNB8Q0AH5GBS0V13PQRV1SR -->

---
Entry: Claude Code (caleb) 2026-06-09T04:47:22.843161+00:00
Role: scribe
Type: Note
Title: cspy is born — M1–M3 file commander (column listing, vi motion, picks, shell-out)

Spec: scribe

tags: #history #genesis

Moment: genesis — Reconstructed: the entire file-commander core lands in one initial commit as `cspy`   [kind: new-capability]
When: 2026-04-15 (commit 57ae2733; repo-root sibling 6f981bc is the empty "Initial commit")
Recorded rationale: "A Rust clone of SideFX's in-house `spy` tool, targeted at macOS and Linux with static musl/universal builds. Designed to pair with Claude Code (embedded pty pane) — hence the name: c(laude) + spy." — commit 57ae2733 body
Inferred intent: the foundational shape (file list + vi keys + shell-out + EDITOR/PAGER delegation) is fully present at t=0; the project starts as a working tool, not a skeleton. confidence: high — evidence: src/app.rs is 843 lines in the initial commit.
Supersedes: (none — earliest code)

The root commit declares milestones M1–M3 already done. Recorded feature set (commit 57ae2733): column-major file list ("`ls -C` style", per-page width packing so one long filename widens only its column); full vi motion (hjkl / arrows / Space / Enter, counts like 5j/10k, gg/G, ^B/^F pagination); navigation (e/v edit, d/Enter display, u/- climb, H/~ home); picks (t, T pattern, ^T all/clear) and cross-directory inventory (y take, p drop, i view, z empty); shell-out with % substitution (! / ; prompt, $ drops to $SHELL, ^W/^X chmod, TUI suspend/resume to avoid alt-screen flash); EDITOR/PAGER delegation (v/e → $EDITOR, d/Enter on text → $PAGER → less); incremental search with / (case-insensitive prefix, glob on * ? [, n/N wraparound); ignore masks; Tokyo-Night-ish palette with a terracotta cursor bar (src/ui/theme.rs).

Module topology at birth: src/app.rs (843 lines), src/fs/{entry,listing,mod}.rs, src/keymap/{action,mod,resolver}.rs, src/shell/{expand,mod}.rs, src/main.rs. The keymap resolver (251 lines) is already a separate concern — the input subsystem that later grows into arc-06 starts here as a resolver table. The fs listing and the shell-expansion split (%-substitution) are present from line zero.

+1 folded: 63b728db (2026-04-15 13:30) "Add ? help overlay; adopt clippy pedantic/nursery lints; add Bitbucket Pipelines CI" — establishes the lint stance and CI that persist through the whole history.
+1 folded: 833e3fa6 (2026-04-15 13:48) "Add J jump; column-aware cursor motion" — first appearance of J (jump-to-dir), a subsystem that gets heavily reworked in the v1.28–v1.34 jump-history cluster later in this thread.

Provenance:
- 6f981bc (2026-04-15 17:07) — repo-root empty "Initial commit".
- 57ae2733 (2026-04-15 13:09) — cspy M1–M3; src/app.rs 843 lines + fs/keymap/shell modules.
- 63b728db, 833e3fa6 (2026-04-15) — help overlay/CI/lints; J jump.

<!-- Entry-ID: 01KTNB9PK8J16N5H5QVDX0DER4 -->
