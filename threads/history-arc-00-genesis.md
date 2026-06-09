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

---
Entry: Claude Code (caleb) 2026-06-09T04:48:02.539648+00:00
Role: scribe
Type: Note
Title: M6 + M4 — pure-Rust file ops and the config/keymap DSL (feeds arc-06)

Spec: scribe

tags: #history #genesis

Moment: genesis — Reconstructed: file operations move into pure Rust (M6) and the config + keymap DSL is born (M4)   [kind: new-capability]
When: 2026-04-15 · commits 8f931cd4 (M6), 5b1e09a4 (M4), f30386f2 (vi marks)
Recorded rationale: "M6: file operations (c, m, R, M, ^W, ^X) in pure Rust; L and f via in-app pager" (8f931cd4); "M4: .cspyrc.toml config, keymap DSL, live reload, theme + masks from config" (5b1e09a4)
Inferred intent: two architectural seams open same-day — (a) file mutations done natively rather than shelling out, (b) configuration externalized into a TOML + a keymap DSL with live reload. Both are durable subsystems. confidence: high — evidence: src/config/{dsl,mod}.rs and the keymap user-binding path exist from M4 onward (visible in the 28c4d329 rename touching src/config/dsl.rs, src/keymap/user.rs).
Supersedes: partially supersedes the initial commit's shell-out chmod (^W/^X) by adding native c/m/R/M copy/move/rename ops.

M6 (8f931cd4) brings copy/move/rename/mkdir into Rust rather than spawning cp/mv; L (long listing) and f route through the in-app pager — the first use of the pager as an output surface, which becomes the arc-05 pager subsystem. M4 (5b1e09a4) externalizes configuration: a `.cspyrc.toml` (note the cspy-era name), a keymap DSL allowing user rebinds, live reload on save, and theme/ignore-masks sourced from config. This is the genesis of the input/config subsystem that arc-06 later builds on — the keymap resolver (present since the root commit) now gains a user-binding layer fed by the DSL.

f30386f2 (same day) adds vi-style marks (m{a-z} / '{a-z}) and rebinds move→M, mkdir→+. The marks + the J jump (from the prior moment) are the two roots of the vi marks + jump-history subsystem that feeds arc-06.

+1 folded: 9a3c59df (2026-04-15 14:52) "Auto-refresh listing dir on filesystem changes" — first filesystem-watcher wiring; the git-status watcher (later in this thread) is built on this same notify-based watch path.
+1 folded: ae07e336 (2026-04-15 15:16) "Add info commands: D date, V version, I session, C color toggle, s setenv" — the colon-less info-command family (V later moves to gV/:version in commit 3e73b300).

Provenance:
- 8f931cd4 (2026-04-15 14:11) — M6 native file ops; L/f via pager.
- 5b1e09a4 (2026-04-15 14:42) — M4 .cspyrc.toml + keymap DSL + live reload (src/config/, src/keymap/user.rs).
- f30386f2 (2026-04-15 15:03) — vi marks; move→M, mkdir→+.
- 9a3c59df, ae07e336 (2026-04-15) — fs auto-refresh; info commands.

<!-- Entry-ID: 01KTNBAXG9MHQ0G014X7K127QK -->

---
Entry: Claude Code (caleb) 2026-06-09T04:48:27.615806+00:00
Role: scribe
Type: Note
Title: M8 — the embedded pty pane (split-under-listing); birth of the pane subsystem (feeds arc-03)

Spec: scribe

tags: #history #genesis

Moment: genesis — Reconstructed: an embedded pty pane is spiked as a horizontal split under the file listing   [kind: new-capability]
When: 2026-04-15 · commit bbdcebb9 (M8 spike), followed by polish 31a36e01
Recorded rationale: "M8 v1 spike: embedded pty pane (horizontal split under the listing)" (bbdcebb9). Body: "Deps added: portable-pty 0.8, vt100 0.15. ... Main loop polls at 16ms while the pane is open (snappy for interactive TUIs like claude's visual mode); 250ms otherwise. ... Pane command defaults to `$CSPY_PANE_CMD` if set, else `$SHELL`, else /bin/sh."
Inferred intent: this is the load-bearing differentiator — the embedded Claude pane the project was named for ("pair with Claude Code (embedded pty pane)", root commit). The whole pane/pty subsystem (arc-03) begins here. confidence: high — evidence: src/pane/{mod,input,widget}.rs created in this commit (mod.rs 160, input.rs 118, widget.rs 79 lines).
Supersedes: (none — new subsystem)

The M8 spike (bbdcebb9) introduces the architecture that defines spyc: a portable-pty + vt100 terminal embedded as a horizontal split beneath the listing. The adaptive poll cadence (16ms open / 250ms idle) and the toggle-chord-forwards-everything-else input model are set here and persist. Note the env var is `$CSPY_PANE_CMD` — pre-rename; it becomes SPYC_PANE_CMD at 28c4d329. Tests step 59→64.

31a36e01 (same day, "Pane polish") adds the ^W prefix, send-selection, the focus model, the divider, and the default 30/70 split — the pane's interaction surface. The ^W prefix established here is the namespace under which context-piping (M10), restart, and yank-scrollback all later hang.

This pane subsystem is the single largest downstream-arc feeder: pane recovery, startup tabs, scroll mode, cwd-in-divider, and the MCP-pane bridge all trace to this commit. See history-seg threads for the pane line (arc-03).

Provenance:
- bbdcebb9 (2026-04-15 16:46) — M8 spike; src/pane/{mod,input,widget}.rs; portable-pty 0.8 + vt100 0.15 added; 16ms/250ms poll.
- 31a36e01 (2026-04-15 21:35) — pane polish: ^W prefix, focus model, divider, 30/70 split.

<!-- Entry-ID: 01KTNBBP6KWVSW542TEBQ5R58S -->
