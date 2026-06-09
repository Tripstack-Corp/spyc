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

---
Entry: Claude Code (caleb) 2026-06-09T04:48:55.736086+00:00
Role: scribe
Type: Note
Title: The pager subsystem — search, ANSI, hex-dump, shell-mode capture (feeds arc-05)

Spec: scribe

tags: #history #genesis

Moment: genesis — Reconstructed: the in-app pager grows from a delegate into a full viewer; shell modes (! capture / ; overlay) defined   [kind: new-capability]
When: 2026-04-15 → 04-16 · commits 3bb95e9e, c48ef7fe, 15702012, 8368b6f9, 0ed22736
Recorded rationale: "Enhanced in-app pager: search, ANSI colors, text-file viewing, whitespace markers" (3bb95e9e); "Hex-dump view for binary files" (c48ef7fe); "Shell modes: ! captures to pager, ; runs interactive in top overlay" (15702012)
Inferred intent: the pager stops being a thin $PAGER delegate and becomes a native viewer with its own feature surface (search/ANSI/hex/line-numbers/save) — the arc-05 pager. The !/; shell-mode dichotomy (capture-to-pager vs interactive-overlay) is fixed here and never changes. confidence: high — evidence: these are sequential single-concern commits each adding one pager capability.
Supersedes: supersedes the root commit's "d/Enter on text → $PAGER → less" delegation by replacing it with a native pager for in-app viewing.

The native pager subsystem assembles over two days. 3bb95e9e adds in-pager search, ANSI color interpretation, text viewing, and whitespace markers. c48ef7fe adds hex-dump for binary files (the root commit had binary files as no-ops). 15702012 fixes the two shell-execution modes that persist for the whole project: `!` captures stdout to the pager, `;` runs interactively in a top overlay. 8368b6f9 adds pager line-numbers (l), save-output (s), page-back (b), and the [V] mode tag. 0ed22736 adds pane scroll mode, one-shot repaint, ^L redraw, and creates ROADMAP.md.

This is the arc-05 pager's genesis: search infrastructure (/) reused later by diff-view and help; the capture path (!) that becomes the streaming pager (v0.10), the PTY-backed capture (v1.12), and the large-file cap (v1.27).

+1 folded: 14989f60 (2026-04-15 23:18) "UX polish: double-press quit, any-key dismiss, partial capture, F9 resume" — the double-press-quit guard (later expanded to warn on running pane processes, v1.7) and F9 capture-resume.
+1 folded: c48ef7fe whitespace/marker color tuning.

Provenance:
- 3bb95e9e (2026-04-15 21:59) — pager search/ANSI/text/whitespace.
- c48ef7fe (2026-04-15 22:09) — hex-dump for binaries.
- 15702012 (2026-04-15 22:54) — shell modes ! (capture→pager) / ; (interactive overlay).
- 8368b6f9 (2026-04-15 23:51) — pager line numbers / save / page-back / [V] tag.
- 0ed22736 (2026-04-16 00:26) — pane scroll mode + ROADMAP.md created.

<!-- Entry-ID: 01KTNBCHN5019NVA29V97ZH26D -->

---
Entry: Claude Code (caleb) 2026-06-09T04:49:21.022427+00:00
Role: scribe
Type: Note
Title: vi marks + jump-history navigation; shared shell prompt with vi editing

Spec: scribe

tags: #history #genesis

Moment: genesis — Reconstructed: navigation history ('' jump-back, backtick start-dir) and a vi-editable shell prompt with shared history land   [kind: new-capability]
When: 2026-04-15 · commits 07937d5d, 83ea8137
Recorded rationale: "Navigation: '' jump-back (cd -), backtick jump to start dir" (07937d5d); "Vi-editable shell prompt with shared persistent history" (83ea8137)
Inferred intent: the input subsystem (arc-06) gains two pillars same-day — directory jump-history (the '' / backtick pair, complementing the m{a-z} marks from f30386f2) and a vi-modal prompt editor with persistent shared history. confidence: high — evidence: these are the navigation/input primitives the v1.28–v1.34 jump-history popup cluster later reworks.
Supersedes: extends the vi marks (f30386f2) and J jump (833e3fa6) into a coherent jump/back navigation model.

07937d5d adds '' (jump-back, cd - semantics) and backtick (jump to the start dir) — directory-level history navigation that sits alongside the per-file m{a-z} marks. 83ea8137 makes the shell prompt vi-editable with shared persistent history across invocations — the line-editor surface that later acquires dw/cw (f5e34dce), word-boundary tuning (v1.35.1), and the J-prompt promotion (v1.33).

This is the input/marks/jump-history root that feeds arc-06. The jump-history data structure introduced here (and the J prompt) is the exact thing reworked across the dense v1.28–v1.34 run folded later in this thread — pickaxe `git log -S 'jump_history'` confirms continuity from this era into that cluster.

+1 folded: a3f7831b (2026-04-15 23:59) "Makefile: build, release, cross-compile, install, deploy" — the Makefile build surface (later split so only install needs sudo, 7e04132e).

Provenance:
- 07937d5d (2026-04-15 23:24) — '' jump-back, backtick start-dir.
- 83ea8137 (2026-04-15 23:45) — vi-editable shell prompt + shared persistent history.
- a3f7831b (2026-04-15 23:59) — Makefile build/release/cross-compile/install/deploy.

<!-- Entry-ID: 01KTNBDAPBCB80SNR6K3N42608 -->

---
Entry: Claude Code (caleb) 2026-06-09T04:49:48.738279+00:00
Role: scribe
Type: Note
Title: M9 — multi-tab pane + powerline status bar with git branch (feeds arc-03, arc-04)

Spec: scribe

tags: #history #genesis

Moment: genesis — Reconstructed: the pane becomes multi-tab and the status bar becomes a powerline showing git branch + activity   [kind: new-capability]
When: 2026-04-16 · commits 9fae4eba (M9 tabs), 06bc1ed4 (M9 status bar), then a3935d0f (focus indicators)
Recorded rationale: "M9: Multi-tab pane, pager full-width/yank, remove mouse capture" (9fae4eba); "M9: Tab rename, powerline status bar, git branch, activity indicators, ESC cancel" (06bc1ed4)
Inferred intent: two distinct births here — (a) the multi-tab pane (src/pane/tabs.rs, 154 new lines) which is the "multi-pane = tabs" model that recurs through the whole project, and (b) the powerline status bar carrying git branch + activity, which seeds the arc-04 git-in-status line. confidence: high — evidence: src/pane/tabs.rs created in 9fae4eba; status bar git-branch added in 06bc1ed4.
Supersedes: supersedes the single-pane M8 model (bbdcebb9) by making the pane a tab container.

M9 lands across two commits. 9fae4eba creates src/pane/tabs.rs (154 lines) — the multi-tab pane. This is the canonical "tabs, not splits" pane model. It also removes mouse capture (restoring native terminal text selection — a recurring tension; mouse capture is re-enabled then re-removed several times later, e.g. fd131b9c then 72921b4d). 06bc1ed4 adds tab rename, the powerline status bar, the git-branch display, and activity indicators. The git-branch-in-status is the seed of the arc-04 git-status line, complemented two days later by git file-status colors in the listing (ec8689d6, 2026-04-16: "Git file status colors in listing: modified, added, untracked, deleted").

a3935d0f (2026-04-16) adds focus indicators (dim cursor when unfocused, blinking pane cursor when focused) — the focus-model polish on top of M9.

ROADMAP context (FEATURES.md added at 14ee484, 2026-04-16, "update ROADMAP.md to reflect M9 completion"): the genesis ROADMAP enumerates M10 context-piping, M11 worktrees, M12 diff-view, plus session-forking and demo-mode as the forward plan — confirming the milestone numbering was a deliberate, documented sequence.

+1 folded: a20664ca (2026-04-16) "Update FEATURES.md ... add INSTALL.md".
+1 folded: f5e34dce (2026-04-16) "Fix Alt+Enter newline ... add dw/cw to vi line editor" — line-editor operators.
+1 folded: ec8689d6 (2026-04-16) git file-status colors in listing (arc-04 seed).

Provenance:
- 9fae4eba (2026-04-16 07:46) — M9 multi-tab pane; src/pane/tabs.rs 154 lines.
- 06bc1ed4 (2026-04-16 09:00) — M9 powerline status bar + git branch + activity.
- 14ee484 (2026-04-16 11:25) — FEATURES.md added; ROADMAP marks M9 done.
- ec8689d6 (2026-04-16 19:13) — git file-status colors in listing.

<!-- Entry-ID: 01KTNBE56ZYMGFZ592XT4A1BF5 -->

---
Entry: Claude Code (caleb) 2026-06-09T04:50:25.404102+00:00
Role: scribe
Type: Note
Title: M10–M12 — context piping, git worktrees, git diff-view in pager

Spec: scribe

tags: #history #genesis

Moment: genesis — Reconstructed: the planned ROADMAP milestones M10/M11/M12 ship in sequence — context piping, worktrees, diff-view   [kind: new-capability]
When: 2026-04-16 · commits a74d3a66 (M10), 816ac2b2 (M11), 54d7691c (M12)
Recorded rationale: "M10: Context piping — send file contents to pane with ^W p / ^W i" (a74d3a66); "M11: Git worktree integration, pending key indicator in prompt line" (816ac2b2); "M12: Git diff view in pager with gd (unstaged) and gD (staged)" (54d7691c)
Inferred intent: these execute, in order, the exact M10/M11/M12 the genesis ROADMAP enumerated — the milestone plan is being followed as written. M10 is the file-manager→pane bridge that justifies the embedded-pane design; M12 reuses the pager's existing ANSI/search infra for diffs. confidence: high — evidence: each commit subject matches its ROADMAP milestone entry verbatim (ROADMAP.md at 14ee484 quotes M10 "^W p sends picks, ^W i sends inventory", M12 "d on a modified file ... opens a diff view").
Supersedes: M10 builds on the ^W prefix (31a36e01); M12 builds on the pager ANSI/search (3bb95e9e).

M10 (a74d3a66) wires context-piping: ^W p sends picks, ^W i sends inventory items, as paths or contents, into the active pane tab — the browse→prompt bridge the ROADMAP framed as "we're already a file manager with multi-select." M11 (816ac2b2) adds git-worktree integration plus a pending-key indicator in the prompt line (the multi-key chord feedback). M12 (54d7691c) adds git diff-view in the pager (gd unstaged / gD staged), reusing the pager's line-numbering and search — an arc-05/arc-04 crossover. gd later grows to include new files (v1.15, baa31de2) and gb blame.

+1 folded: 7dea52bf (2026-04-16) "Help overlay now uses the pager" — the help text becomes a pager buffer (scrollable/searchable), unifying two surfaces.
+1 folded: 6bcbb95d (2026-04-16) "Pager: v opens buffer in $EDITOR, returns to pager on quit".
+1 folded: a cluster of 6 startup-robustness commits (4719ae1e, 30902126, 34310dac, c2c67fb6, plus 157ebd50/be627741 resize fixes) hardening "never crash on permission denied / unreadable cwd — flash error and stay put" (2026-04-16).
+1 folded: e48f7c02 (2026-04-16) "Code review cleanup: hot-path fixes".

Provenance:
- a74d3a66 (2026-04-16 13:11) — M10 context piping ^W p / ^W i.
- 816ac2b2 (2026-04-16 16:04) — M11 git worktree integration.
- 54d7691c (2026-04-16 16:24) — M12 git diff-view gd/gD in pager.
- ROADMAP.md @14ee484 — M10/M11/M12 enumerated as the forward plan.

<!-- Entry-ID: 01KTNBF97DDYPGFFS9SJ9MHB9M -->

---
Entry: Claude Code (caleb) 2026-06-09T04:50:53.979296+00:00
Role: scribe
Type: Note
Title: v0.9–v0.10 — license/edition bump, session management, streaming pager, syntax highlighting

Spec: scribe

tags: #history #genesis

Moment: genesis — Reconstructed: the project versions to v0.9/v0.10, gains session save/restore, a streaming pager, and syntect syntax highlighting   [kind: new-capability]
When: 2026-04-17 · commits 5586b68d, 5daa3f6e, b362bc40 (sessions), b2ce7393 (streaming), bd11a3ef (syntect)
Recorded rationale: "Bump to v0.9.0, BSD-3-Clause license, add CONTRIBUTING.md" (5586b68d); "Upgrade to Rust edition 2024, bump MSRV to 1.85" (5daa3f6e); "Session management: save on quit, restore with --resume, picker UI" (b362bc40); "Streaming pager: ! commands show output in real-time" (b2ce7393); "Syntax highlighting in pager via syntect (base16-eighties.dark theme)" (bd11a3ef)
Inferred intent: pre-release hygiene (license, edition 2024, MSRV 1.85, CONTRIBUTING) plus three feature additions. Session save/restore is the genesis of the session subsystem that the current branch (fix/q-command-session-save) still touches. The streaming pager reworks the ! capture path from blocking to real-time. confidence: high.
Supersedes: streaming pager (b2ce7393) supersedes the blocking ! capture from 15702012 — output now renders incrementally; subsequent fixes 19891930/36fb2520 tune the 16ms poll and stderr merge.

Session management (b362bc40) introduces save-on-quit + --resume + a picker UI — durable infrastructure; conversation-aware restore with Claude session ID/name follows at v1.5 (2bd2747e). The streaming pager (b2ce7393, then 19891930 "16ms poll during capture, suppress EOF while running" and 36fb2520 "merge stderr, hourglass timer") converts ! from capture-then-show to show-as-it-runs. syntect (bd11a3ef) adds syntax highlighting (base16-eighties.dark) to the pager. By this point the codebase is self-reported at "14k lines, 64 commits" (6074b0fd).

+3 folded pager-clear artifact iterations: b5b2f237 → 73d761f7 (revert to inner_area) → 39a7fbf9 (no border in full-width) — the recurring "clear the right region" pager bug, fixed/reverted/refixed (2026-04-17).
+1 folded: 1040cc88 (2026-04-17) "Fix grid pagination oscillation; add --debug flag and debug_log module" — the debug_log subsystem.
+1 folded: 9b49016c (2026-04-17) "v0.10.0: ... add version-bump convention" — the version-bump-per-feature convention that explains this thread's dense cadence is formalized here.

Provenance:
- 5586b68d, 5daa3f6e (2026-04-17) — v0.9.0, BSD-3-Clause, edition 2024, MSRV 1.85.
- b362bc40 (2026-04-17 06:44) — session save/restore + --resume + picker.
- b2ce7393 (2026-04-17 06:48) — streaming pager (real-time !).
- bd11a3ef (2026-04-17 09:58) — syntect syntax highlighting.
- 9b49016c — version-bump convention formalized.

<!-- Entry-ID: 01KTNBG5C73G8HPRTA74KQNJEK -->

---
Entry: Claude Code (caleb) 2026-06-09T04:51:26.141518+00:00
Role: scribe
Type: Decision
Title: : command line + the cspy→spyc rename at v1.0.0 (segment-topology)

Spec: scribe

tags: #history #genesis

Moment: genesis — Reconstructed: the colon command-line is added, then the project renames cspy→spyc at v1.0.0   [kind: segment-topology]
When: 2026-04-17 · commits 4fb0ad1a (!? editor), 4fb0ad1a→4fb0ad1a, 489c800a (v0.13), then 28c4d329 (rename → spyc v1.0.0)
Recorded rationale: ": command line, = limit filter, numeric prefix display" (4fb0ad1a, commit subject); rename — "The 1.0 release. cspy is now spyc (spy + claude = spicy 🌶️). Rename package, binary, config (.spycrc.toml), state dirs, env vars, debug macro, all source comments, docs, build files, and CI config. Add 🌶️ pepper emoji ... New logo: Twemoji pepper (CC-BY 4.0) ... Version 0.13.0 → 1.0.0" (28c4d329 body)
Inferred intent: the `:` command line (4fb0ad1a) opens the colon-command dispatch surface (:cd, :sort, :marks, :set, :version, :grep, :task, :pause/:resume all hang off it later). The rename (28c4d329) is the single topology pivot of the whole segment. confidence: high — evidence: pickaxe `git log -S 'spyc'` returns 28c4d329 as the first commit introducing the token; rename touches src/app.rs, src/config/dsl.rs, src/keymap/{action,resolver,user}.rs, build files, CI.
Supersedes: the rename supersedes every cspy-era identifier — .cspyrc.toml→.spycrc.toml, $CSPY_PANE_CMD→$SPYC_PANE_CMD, the cspy! debug macro, docs/logo.svg→docs/spyc-logo.svg.

This is THE topology moment named in the framing note. Before 28c4d329 the binary, package, config file (.cspyrc.toml), env vars, and the logo are all `cspy` — the etymology "c(laude) + spy" (clone of SideFX's `spy`). At v1.0.0 the letters reverse to `spyc`, re-glossed as "spy + claude = spicy 🌶️", and the pepper branding enters (status-bar prefix, Twemoji pepper logo CC-BY 4.0). 0535ae6f immediately follows to update the repo URL to bitbucket.org/tripstack/spyc, and 3be7989a adds the pepper to version strings.

The `:` command-line (4fb0ad1a, immediately before the rename) is the other lasting addition here — the dispatch table for colon commands. The genesis run between v0.11 and v1.0 also delivered the !? history picker/editor (462f7a4a v0.11, 3759e63c "vi-editable lines, /search, :N jump, dedup").

+1 folded: 489c800a (2026-04-17) "Bump version to 0.13.0 (catch up missed bumps)" — explicitly reconciling the version cadence.
+1 folded: 03113fad (2026-04-17) doc sync (CLAUDE/FEATURES/ROADMAP).

Provenance:
- 4fb0ad1a (2026-04-17 17:00) — : command line, = limit filter, numeric prefix.
- 28c4d329 (2026-04-17 18:13) — RENAME cspy→spyc, v0.13.0→v1.0.0, pepper branding (verified via `git show --stat`: package/binary/config/env/source/docs/CI all renamed).
- 0535ae6f (2026-04-17 18:17) — repo URL → bitbucket.org/tripstack/spyc.
- 3be7989a (2026-04-17 18:20) — 🌶️ pepper in version strings.

<!-- Entry-ID: 01KTNBH4F01YB3YNV9JT6MM8QH -->

---
Entry: Claude Code (caleb) 2026-06-09T04:51:58.092925+00:00
Role: scribe
Type: Note
Title: v1.1–v1.3 — git-status gutter markers, live .git/index watch, :cd/:sort/:marks/:set

Spec: scribe

tags: #history #genesis

Moment: genesis — Reconstructed: git status moves from color-override to gutter markers with a live .git/index watcher; colon commands proliferate   [kind: refactor]
When: 2026-04-17 · commits f60f2bea (v1.1), 285aa960 (v1.2 gutter markers), d5ff47f2 (v1.3 colon cmds), 14e2ab1c (v1.3.1 index watch)
Recorded rationale: "Fix file type colors overridden by git status; bump to 1.1.0" (f60f2bea); "Git status markers in gutter instead of color override; bump to 1.2.0" (285aa960); ":cd, :sort, :marks, :set, pager buffer history; bump to 1.3.0" (d5ff47f2); "Watch .git/index for live git status marker updates; bump to 1.3.1" (14e2ab1c)
Inferred intent: a small supersession in the arc-04 git-status line — git state stops hijacking filetype colors (which conflicted) and moves to a dedicated gutter column, then becomes live via a .git/index watcher built on the M4-era fs watch path. confidence: high — evidence: 285aa960 subject explicitly states "instead of color override"; pickaxe on the git-status rendering confirms the gutter move.
Supersedes: 285aa960 supersedes ec8689d6 (git file-status COLORS in listing) — markers replace the color override because filetype colors and git colors collided (the bug f60f2bea fixed).

The git-status-in-listing feature (born ec8689d6) is reworked twice here: f60f2bea fixes filetype colors being clobbered by git status, then 285aa960 abandons color-override entirely for gutter markers. 14e2ab1c makes those markers live by watching .git/index — the first git-specific use of the watcher, later broadened to watch .git/ as a directory (v1.18.1, cd43cd97) after commits weren't triggering refresh. d5ff47f2 adds the :cd/:sort/:marks/:set colon commands plus pager buffer history.

+1 folded: pager buffer-history (multiple pager buffers) introduced in d5ff47f2, roadmapped at 484af6e4.

Provenance:
- f60f2bea (2026-04-17 18:31) — v1.1.0, filetype-vs-git color fix.
- 285aa960 (2026-04-17 19:24) — v1.2.0, gutter markers replace color override.
- d5ff47f2 (2026-04-17 20:18) — v1.3.0, :cd/:sort/:marks/:set + pager buffer history.
- 14e2ab1c (2026-04-17 20:27) — v1.3.1, .git/index live watch.

<!-- Entry-ID: 01KTNBJ345J5SFWXZA2YN5M6K5 -->

---
Entry: Claude Code (caleb) 2026-06-09T04:52:28.167655+00:00
Role: scribe
Type: Note
Title: Test suite + AppState extraction — app.rs becomes app/mod.rs (first decomposition, prefigures REFACTOR_PLAN)

Spec: scribe

tags: #history #genesis

Moment: genesis — Reconstructed: a comprehensive test suite lands and App is split into a testable AppState; src/app.rs becomes the src/app/ directory   [kind: refactor]
When: 2026-04-17 → 04-18 · commits 130b4664 (tests), 0961a82b (CI gating), 488ae17b (AppState), b787715e (Phase 3), 37279fc1 (Phase 4)
Recorded rationale: "Add comprehensive test suite (74→224 tests), fix all 71 clippy errors, fix CI MSRV" (130b4664); "Extract AppState from App: 23 domain methods now testable without a terminal" (488ae17b); "Phase 3: Extract dispatch_command/dispatch_prompt to AppState (25 new tests)" (b787715e); "Phase 4: Extract apply() action dispatcher to AppState (28 new tests)" (37279fc1)
Inferred intent: this is the first real decomposition pressure — domain logic is pulled out of the terminal-bound App into an AppState so it can be unit-tested without a TTY. The "Phase 3 / Phase 4" naming here directly anticipates the phased REFACTOR_PLAN authored at segment-end (265d2816). confidence: high — evidence: src/app.rs (single file) becomes src/app/mod.rs (directory) — `git show 488ae17:src/app/mod.rs | wc -l` = 3896, while the pre-split file was src/app.rs.
Supersedes: restructures the monolithic src/app.rs (843→~3900 lines by this point) into src/app/{mod,state}.rs with extracted dispatch.

This is the seam the REFACTOR_PLAN later formalizes. 130b4664 takes tests 74→224 and clears 71 clippy errors; 0961a82b adds a panic hook, cargo-audit, and coverage gating to CI. Then 488ae17b extracts AppState (23 domain methods testable without a terminal), b787715e extracts dispatch_command/dispatch_prompt (+25 tests), 37279fc1 extracts the apply() action dispatcher (+28 tests). The "Phase N" vocabulary and the testability-without-a-PTY goal are exactly the rationale REFACTOR_PLAN.md (265d2816) restates two weeks later: "Side effects modeled as data ... so handlers are unit-testable without a real PTY." Note app/mod.rs is 3896 lines here and grows to 7421 by segment-end — the monolith re-accretes faster than it's split, which is the tension the final REFACTOR_PLAN names.

+1 folded: 3118be41 (2026-04-17) BUGS.md update; the BUGS.md triage file becomes the running issue log for the rest of genesis.

Provenance:
- 130b4664 (2026-04-17 21:49) — 74→224 tests, 71 clippy fixes, CI MSRV.
- 0961a82b (2026-04-17 22:07) — panic hook, cargo-audit, coverage gating.
- 488ae17b (2026-04-17 22:32) — AppState extraction; src/app.rs → src/app/mod.rs (3896 lines).
- b787715e (2026-04-17 22:41) — Phase 3 dispatch extraction.
- 37279fc1 (2026-04-17 22:50) — Phase 4 apply() extraction.

<!-- Entry-ID: 01KTNBK0V3M9HGE654CBDR4276 -->

---
Entry: Claude Code (caleb) 2026-06-09T04:53:01.604411+00:00
Role: scribe
Type: Note
Title: M13–M14 + v1.5–v1.6 — gf/gF path-jump, MCP context handoff, inventory rewrite

Spec: scribe

tags: #history #genesis

Moment: genesis — Reconstructed: gf/gF path-jumping from pane output, the first MCP bridge (context handoff), and the inventory becomes a file-backed cache   [kind: new-capability]
When: 2026-04-18 → 04-19 · commits c39c4180 (M13 gf/gF), a31c3b84 (M14 MCP), 2bd2747e (session restore), b7ad6ee8 (inventory rewrite)
Recorded rationale: "Add gf/gF: jump to file paths in pane output (M13); bump to 1.4.0" (c39c4180); "Add MCP context handoff and exited-tab UX fix (M14); bump to v1.5.0" (a31c3b84); "Add conversation-aware session restore with Claude session ID/name" (2bd2747e); "Rewrite inventory as file-backed cache with graveyard, tags, and put" (b7ad6ee8)
Inferred intent: M13 closes the loop from the pane back to the file manager (Claude prints a path, gf jumps to it). M14 is the genesis of the MCP subsystem — first version is a one-way context handoff, expanded to writable actions at v1.8 and a full transport rewrite at v1.10. The inventory rewrite moves from in-memory to a file-backed cache with a graveyard + tags. confidence: high.
Supersedes: 2bd2747e extends session restore (b362bc40) with conversation-aware Claude session ID/name; b7ad6ee8 supersedes the de9caa59 inventory-UX-persistence approach with a full file-backed cache rewrite.

M13 (c39c4180, then 016bdfcc/88873045 fixing Claude-CLI output patterns and scroll-mode) makes pane output addressable: gf/gF jump to file paths printed in the pane — the inverse of M10's context-piping. M14 (a31c3b84) is the first MCP bridge: context handoff so an external Claude can read spyc's workspace state. This MCP line then runs: writable actions (v1.8, c473e673 "Claude can mutate the TUI workspace"), the proactive-use CLAUDE.md instruction (d218e882), the HTTP→Unix-socket transport rewrite (v1.10, f81e7ade), and the search MCP exposure (v1.24, covered later). The inventory rewrite (b7ad6ee8) introduces the file-backed cache with graveyard/tags/put.

+1 folded: 0ce26578 (2026-04-19) "Add unicode-width for correct CJK/emoji column alignment" — column-width correctness.
+1 folded: 8ec589e2 (2026-04-19) "Add CHANGELOG.md and --version --verbose" — CHANGELOG.md begins here, maintained per-version for the rest of genesis.
+5 folded README/docs/status-bar polish (2026-04-19): 5a42f192, 727ee55c, 0af2c4cc, c80406f1, 7a5c7f22.

Provenance:
- c39c4180 (2026-04-18 18:03) — M13 gf/gF path-jump, v1.4.0.
- a31c3b84 (2026-04-18 21:10) — M14 MCP context handoff, v1.5.0.
- 2bd2747e (2026-04-18 21:55) — conversation-aware session restore.
- b7ad6ee8 (2026-04-19 06:31) — inventory as file-backed cache.
- 8ec589e2 (2026-04-19) — CHANGELOG.md introduced.

<!-- Entry-ID: 01KTNBM19MB1PT31ZZMZHE3T59 -->

---
Entry: Claude Code (caleb) 2026-06-09T04:53:30.213434+00:00
Role: scribe
Type: Note
Title: v1.7–v1.8 — performance refactor, writable MCP actions (Claude mutates the workspace)

Spec: scribe

tags: #history #genesis

Moment: genesis — Reconstructed: a performance refactor with ^a pane prefix and activity monitor; then writable MCP actions let Claude mutate the TUI   [kind: new-capability]
When: 2026-04-19 · commits 4f3e98e5 (v1.7.0), c473e673 (v1.8.0), d218e882 (CLAUDE.md MCP guidance)
Recorded rationale: "v1.7.0: Performance refactor, ^a pane prefix, yank commands, activity monitor" (4f3e98e5); "v1.8.0: Writable MCP actions — Claude can mutate the TUI workspace" (c473e673); "CLAUDE.md: instruct Claude to use spyc MCP tools proactively" (d218e882)
Inferred intent: v1.7 is a perf pass plus the ^a pane prefix (a second chord namespace alongside ^W) and an activity monitor. v1.8 is the MCP inflection: the bridge goes from read-only context handoff (M14) to writable — Claude can drive navigation/picks/filters in the live TUI. The CLAUDE.md edit (d218e882) makes proactive MCP use a project convention. confidence: high.
Supersedes: c473e673 (writable MCP) supersedes the read-only M14 handoff (a31c3b84) — the MCP surface gains mutation, not just observation.

The MCP subsystem crosses from observe to act here. v1.8 (c473e673) exposes write actions (navigate_to, pick_files, set_filter and kin — the tool surface visible today as the spyc MCP server) so an external Claude mutates the workspace. d218e882 codifies "use spyc MCP tools proactively" in CLAUDE.md. v1.7 (4f3e98e5) introduces ^a as a pane prefix and an activity monitor (self-measurement oscillation fixed later at 538dc942).

+1 folded: 0662dce5 (2026-04-19) "Add startup health check; rewrite README with MCP-first positioning" — the README repositions around the MCP/Claude story; 73579d7e softens the first-mover claim.
+3 folded README hero/screenshot edits (c156564b, 73579d7e) and ROADMAP cleanup (d14203a4, 33ba0e6b "add writable MCP actions, context enrichment, Elm refactor") — note the ROADMAP entry naming the "Elm refactor" target that REFACTOR_PLAN later stages.
+1 folded: 440c0273/1c69e315/72dfa732 (2026-04-19) — pane exit-status display (three iterative fixes).

Provenance:
- 4f3e98e5 (2026-04-19 18:55) — v1.7.0 perf refactor, ^a prefix, activity monitor.
- c473e673 (2026-04-19 21:50) — v1.8.0 writable MCP actions.
- d218e882 (2026-04-19 21:55) — CLAUDE.md proactive-MCP convention.
- 33ba0e6b (2026-04-19) — ROADMAP names the Elm/MVU refactor target.

<!-- Entry-ID: 01KTNBMXVMNF3CWFR2D0DVCCEG -->
