# history-overview — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: history-overview
Created: 2026-05-07T09:02:08.889240+00:00

---
Entry: Claude Code (caleb) 2026-05-07T09:02:08.889240+00:00
Role: scribe
Type: Note
Title: Framing: two-layer reconstruction of spyc's first 22 days

Spec: scribe

tags: #history #segmentation

Purpose: open the spine for a two-layer reconstruction of spyc's first 22 days of merged work — commits d9b9360 (PR #2, 2026-04-30) through a303251 (PR #37, 2026-05-07), 36 PRs total. Layer 1 will be a set of baseline arc threads, each narrating a chronological cluster of PRs in third-person observational voice — as if a watercooler scribe had been present while the work landed. Layer 2 will be analytic insight threads (drift, recurrence, trajectory, emergence) written after the baselines exist and citing arc-entry IDs by ULID. This entry on `history-overview` opens that spine; the segmentation proposal that follows lays out the arcs to be written.

Audience: Derek Marshall, the maintainer and sole author of these 36 PRs (`onboarding-team-map` confirms single-developer authorship over the window). The exercise is positioned to demonstrate that threading-of-intent over a real commit history has standalone value distinct from `git log` and `CHANGELOG.md` — a scribe-voice arc thread surfaces concerns that a date-ordered log of commit subjects flattens.

Voice contract for every arc and insight thread:
- Mode: third-person observational, present tense.
- Hedge whitelist (encouraged): appears to, reads as, consistent with, the diff shape suggests, points toward, aligns with, the commit message indicates.
- Banned in reference to the maintainer's mindset: wants, thinks, believes, decided, feels, intends to (without "the commit message"), is concerned that. No first-person reconstruction. No "I" or "we" speaking as the author.
- Verbatim commit-message quoting is encouraged; attribute as `(commit <sha>, <date>)`.

Provenance contract for every arc and insight thread:
- Every entry ends with a `Provenance:` block listing commit SHAs cited (with PR number and merge date), file:line spans cited where applicable, and cross-thread references (entry IDs).
- Entries missing the Provenance block are subject to observer challenge.

Inputs validated this session:
- `watercooler_health` against this code_path reports Healthy; Threads Repo URL `git@github.com:calebjacksonhoward/spyc.git`; Branch Parity clean.
- 36 merge commits read in chronological order via `git log --grep='Merged in' --reverse --format='%h %ai %s'` (the leading commit `d9b9360` carries PR #2 because PR #1 merged second by wall-clock; positional index and PR number diverge across the window — arcs cite real PR numbers from the `Merged in ... pull request #N` subject).
- Three onboarding seeds read before any write: `onboarding-overview`, `onboarding-product-charter`, `onboarding-architecture`. The seed `onboarding-architecture` describes the current end-state of the MCP-bridge, alt-screen, pager, and pane surfaces; arcs may reveal those surfaces as late convergences rather than design-from-the-start. Where an arc trajectory points toward late convergence on a surface the seed asserts as present, the arc thread will flag the gap explicitly rather than override the seed silently.

Closure for this thread is intentionally deferred: the spine remains OPEN for arc-session cross-references. No arc threads are written this session; only this spine.

Provenance:
- d9b9360 (PR #2 chore/ci-hygiene, 2026-04-30) — first commit in the 22-day window.
- a303251 (PR #37 fix/mcp-socket-project-scoped-discovery, 2026-05-07) — last commit in the 22-day window.
- onboarding-overview entry 0 = 01KR0NZNJ3KM6BJY09Q4P9D0NE
- onboarding-product-charter entry 0 = 01KR0P18MCE1H57Q5ZTAGKAJNH
- onboarding-architecture entry 0 = 01KR0P4W3ED1QZ8F44PFB2WPDZ
- `watercooler_health` output: server v0.4.6.dev0; Threads Repo URL `git@github.com:calebjacksonhoward/spyc.git`; Branch Parity clean ✓
- Command run for the chronological list: `git log --grep='Merged in' --reverse --format='%h %ai %s'`

<!-- Entry-ID: 01KR0TRFWT9W6WMFHC49YSW0BG -->

---
Entry: Claude Code (caleb) 2026-05-07T09:04:21.678362+00:00
Role: scribe
Type: Note
Title: Segmentation: 8 proposed arcs over the 22-day window

Spec: scribe

tags: #history #segmentation

Total commits in window: 36 (PRs #1 through #37; PR #9 does not appear in this branch's merge log). Methodology: title-driven clustering refined against commit-message bodies and selected diff inspection. The full 36 titles use `feat/`, `fix/`, `chore/`, or `investigate/` prefixes with descriptive slugs; the title vocabulary alone produces a stable initial clustering, with diff inspection used only to resolve straddle cases. Cluster boundaries are intent-based, not file-based: a PR that touches the pager but lands in service of git-aware navigation is filed under git, not pager. Arcs are numbered with a two-digit zero-padded prefix so arc thread topics sort lexicographically.

---

**Arc 01 — `foundation-hygiene`** (2026-04-30, single calendar day, ~4 hours wall-clock)
- d9b9360 (PR #2 chore/ci-hygiene, 2026-04-30 16:50) — "ci: align with make check, add target cache + pre-commit hook"
- 32ebf2c (PR #3 chore/security-hygiene, 2026-04-30 20:27) — "security: cargo-deny, --locked, SECURITY.md"
- 1f41b4b (PR #4 fix/shell-aliases, 2026-04-30 20:48) — "shell: aliases work in :!cmd / ;cmd via $SHELL -i (v1.37.2)"
- Rationale: three PRs that read as Day-0 polish before serious feature work — CI alignment, supply-chain controls, and a small UX fix to shell-out execution. All three land within hours of each other on the same calendar day. The arc thread would narrate "what hygiene state did spyc enter the 22-day window in?"
- Stated-plan anchor: PARTIAL. `SECURITY.md` is named by PR #3's commit subject and survives in the current repo. ROADMAP.md does not directly anchor CI hygiene.

**Arc 02 — `lazygit-investigation-and-harvest`** (2026-04-30 to 2026-05-03; SPECIAL HANDLING — see dedicated entry)
- 0691666 (PR #5 investigate/lazygit-support, 2026-04-30 22:53) — "lazygit investigation + cursor-block fix (v1.37.2)"
- e210e58 (PR #12 chore/clean-notes, 2026-05-03 01:32) — "chore: harvest lazygit notes into BUGS, drop notes/"
- Rationale: an investigation drop (399 lines of notes/, 26 lines added to ROADMAP.md, partial cursor-block fix) followed three days later by a harvest commit that converts the investigation notes into actionable BUGS.md entries. The PR #12 commit subject reads as the explicit closer to PR #5's investigation. Multiple downstream PRs execute against the gap analysis; see the special-handling entry for full back-reference accounting.
- Stated-plan anchor: STRONG. PR #5 itself adds 26 lines to `ROADMAP.md` (Generalized pager picker, Context-sensitive prompt-row hint, Scoped `?` help); PR #12 lands the same content into `BUGS.md`.

**Arc 03 — `pane-behavior`** (2026-05-01 to 2026-05-06)
- 10c9276 (PR #6 feat/pane-zoom, 2026-05-01 19:47) — "pane: ^a z fullscreen-toggle (zoom) for the bottom pane (v1.38.0)"
- a3338fa (PR #22 feat/pane-shutdown-cleanup, 2026-05-05 13:09) — "fix: clean shutdown of pane child trees on tab close + spyc quit (v1.41.9)"
- 20fba00 (PR #26 feat/dim-unfocused-pane, 2026-05-06 14:16) — "feat: dim unfocused side so focus is obvious at a glance (v1.41.13)"
- bdb8d87 (PR #29 fix/skip-pane-cursor-block-when-uninvited, 2026-05-06 17:54) — "fix: skip pane cursor block for unfocused / alt-screen panes (v1.41.16)"
- 8e9fb2c (PR #34 fix/top-overlay-focus-switch, 2026-05-06 23:37) — "fix: ;cmd overlay shares focus with bottom pane (v1.41.21)"
- Rationale: a recurring concern with how a pane behaves visually and how it owns its child-process lifetime. The arc spans visual state (zoom, dim), focus-routing (top-overlay focus switch), child-process cleanup (shutdown), and rendering correctness (cursor-block skip). PR #29 carries a back-reference to arc 02 (PR #5's gap analysis identifies the cursor-block as a top-three lazygit-blocking suspect at `notes/lazygit-gap-analysis.md` "Top suspects" §1).
- Stated-plan anchor: PARTIAL. ARCHITECTURE.md describes pane PTY ownership at lines 85-101; `ROADMAP.md` "Working tracks" lists pane work generally.

**Arc 04 — `git-integration`** (2026-04-30 to 2026-05-06; longest span)
- cd8df2e (PR #1 fix/git-marker-1hz-poll, 2026-04-30 17:08) — "git markers: 1Hz safety-net poll for missed FSEvents (v1.37.1)"
- f3ddaf2 (PR #7 feat/limit-git, 2026-05-02 11:53) — "limit: =git / =g shows files in git status (v1.38.1)"
- 5999261 (PR #15 fix/git-status-and-pane-ctrl-c, 2026-05-04 11:26) — "fix: ^C → pane child + git markers don't leak across same-name files (v1.41.2)"
- 762a0a6 (PR #24 feat/jump-git-change, 2026-05-05 16:26) — "feat: ]g / [g jump cursor to next/prev git-changed entry (v1.41.11)"
- 4e2afd9 (PR #27 feat/git-staged-vs-unstaged, 2026-05-06 16:51) — "feat: two-char git markers for staged-vs-unstaged distinction (v1.41.14)"
- Rationale: spyc's git-aware-file-commander identity surfaces here. PRs span the full window: a Day-0 fix to existing markers (PR #1), a filter (PR #7), a marker correctness fix (PR #15), navigation (PR #24), and richer markers (PR #27). The arc thread would narrate the trajectory of "what does git-awareness mean in this file commander?"
- Stated-plan anchor: PARTIAL. `ROADMAP.md` "Working tracks" includes git-integration items; specific items will require diff inspection during arc 04 authoring.

**Arc 05 — `pager-surface`** (2026-05-02 to 2026-05-07; largest arc)
- 7b941a4 (PR #11 fix/pager-wrap-bottom, 2026-05-02 21:48) — "pager: scroll_max accounts for wrapped visual rows (v1.40.1)"
- 34907a3 (PR #16 fix/fg-tail, 2026-05-04 15:48) — "fix: :fg seeds pager from buffer + scrolls to bottom (v1.41.3)"
- 4f2f3ad (PR #17 fix/help-pager-search-multicol, 2026-05-05 00:11) — "fix: pager n/N follows match into column 2 of multi-col views (v1.41.4)"
- ee07307 (PR #20 feat/scroll-altscreen-hint, 2026-05-05 01:34) — "feat: alt-screen scroll hint + [pane] default_command + gd-vs-HEAD (v1.41.7)"
- eb6ddf6 (PR #23 feat/help-yf-and-percent-docs, 2026-05-05 14:36) — "feat: yf yanks cursor path + help-text discoverability fixes (v1.41.10)"
- cf9e8ff (PR #33 feat/pager-visual-line-mode, 2026-05-06 21:35) — "feat: pager visual line mode for range yank (v1.41.20)"
- c243549 (PR #35 feat/D-opens-pager-in-top-pane, 2026-05-06 23:53) — "feat: D opens cursor file in $PAGER as top overlay (v1.41.22)"
- f505ee5 (PR #36 fix/search-substring-match, 2026-05-07 00:18) — "fix: / and = match by substring, not anchored prefix (v1.41.23)"
- Rationale: the pager (and help-as-pager) surface accretes capability across this window — wrapping, tail-seeding, search, visual-line-mode, range-yank, opening files into the top pager pane, substring search. The diff shape across these eight PRs points toward the pager becoming spyc's unifying read-surface; PR #5's lazygit-ux-catalogue §4 "Generalized pager picker" aligns with this trajectory but is not directly executed by these PRs (no overlay-as-picker work lands here).
- Stated-plan anchor: STRONG. `notes/lazygit-ux-catalogue.md` §4 (added by PR #5) explicitly proposes the "render through the pager" pattern; DESIGN.md ("Don't introduce a third status row") and ROADMAP.md describe the pager as a unifying surface.

**Arc 06 — `input-and-overlays`** (2026-05-02 to 2026-05-06)
- 62fc129 (PR #8 feat/harpoon, 2026-05-02 18:04) — "harpoon: per-project pinned working set + =h filter (v1.39.0)"
- 9043547 (PR #10 feat/quickselect, 2026-05-02 20:52) — "quick select: ^a u labeled-overlay picker for pane output (v1.40.0)"
- bfc4a18 (PR #25 fix/input-dispatch-hardening, 2026-05-06 13:04) — "fix: input dispatch hardening + --key-trace diagnostic switch (v1.41.12)"
- a7867fb (PR #32 fix/chord-priority-over-user-keymap, 2026-05-06 20:09) — "fix: chord prefixes beat user keybindings on the second key (v1.41.19)"
- Rationale: how keystrokes reach handlers, including new picker-overlays (harpoon, quickselect) and dispatch-correctness fixes (input hardening, chord priority). Harpoon and quickselect partially align with PR #5's "Generalized pager picker" roadmap entry, though they ship as standalone overlays rather than as `pager.picker_cursor` modes per the catalogue's recommendation.
- Stated-plan anchor: PARTIAL. PR #5's `notes/lazygit-ux-catalogue.md` §4 proposes the pager-picker pattern; harpoon/quickselect ship a parallel pattern.

**Arc 07 — `codex-and-mcp-bridge`** (2026-05-05 to 2026-05-07)
- bad8bfc (PR #18 chore/agents-md-and-mcp-hygiene, 2026-05-05 00:41) — "chore: AGENTS.md rename + MCP hygiene fixes (v1.41.5)"
- d6d3088 (PR #19 feat/codex-resume, 2026-05-05 01:07) — "feat: codex session save/restore parity with claude (v1.41.6)"
- 193f7ad (PR #21 feat/codex-mcp-config, 2026-05-05 01:53) — "feat: codex MCP discovery via .codex/config.toml (v1.41.8)"
- a303251 (PR #37 fix/mcp-socket-project-scoped-discovery, 2026-05-07 00:54) — "fix: MCP socket discovery is now project-scoped (v1.41.24)"
- Rationale: extension of the MCP-bridge surface from a single Claude-Code peer to a second AI client (codex), plus discoverability hardening (project-scoping). The arc executes against the load-bearing thesis at `ROADMAP.md:3-23`: spyc "isn't just 'a file manager with Claude in a pane.' It's a file manager that Claude can query." PR #37 is the most architecturally consequential in this arc and the `onboarding-architecture` seed already cites it as the "recently-strengthened invariant (v1.41.24)."
- Stated-plan anchor: STRONG. `ROADMAP.md:3-23` is the explicit thesis anchor; `AGENTS.md` and `ARCHITECTURE.md:135-155` describe the surface.

**Arc 08 — `recoverability-and-deps`** (2026-05-03 to 2026-05-06)
- 6b2be36 (PR #13 feat/graveyard-undo, 2026-05-03 02:41) — "graveyard: R-undo + per-entry tar.zst + system trash cascade (v1.41.0)"
- c7419c1 (PR #14 fix/undo-command, 2026-05-03 03:06) — "fix: route :undo / :graveyard to App's handler (v1.41.1)"
- 306b43f (PR #28 fix/huge-directory-cap, 2026-05-06 17:30) — "fix: cap directory listings at 50k entries to avoid hangs (v1.41.15)"
- e39f462 (PR #30 fix/vt100-panic-recovery, 2026-05-06 18:27) — "fix: catch vt100 parser panics so spyc survives bad escape sequences (v1.41.17)"
- 105db8d (PR #31 chore/vt100-and-ratatui-upgrade, 2026-05-06 19:16) — "chore: upgrade vt100 0.15 → 0.16, ratatui 0.29 → 0.30 (v1.41.18)"
- Rationale: spyc-staying-alive in the face of pathological inputs and bad-actor escape sequences, plus the file-system undo surface. Two sub-shapes: file-undo (graveyard + the immediate routing fix on PR #14) and runtime-survival (huge-dir cap, vt100 panic recovery, dep upgrade). The PR #30 → PR #31 sequence (49 minutes apart) points toward the panic-recovery being the safety net that buys budget for the major-version vt100 bump; arc 08 authoring will verify this from the diffs.
- Stated-plan anchor: PARTIAL. `ROADMAP.md` likely mentions undo work; survival-related deps land without a direct ROADMAP anchor.

---

**Cluster boundaries genuinely hard to decide** (flagged for observer challenge before Phase 2):

1. PR #15 (`fix/git-status-and-pane-ctrl-c`) bundles a pane-control fix (^C → pane child) with a git-marker leak fix. Filed under arc 04 (git) because the marker-leak fix appears to be the title's primary subject and the larger half of the diff, but a reasonable observer could argue arc 03 (pane).
2. PR #4 (`fix/shell-aliases`) — filed under arc 01 (foundation-hygiene) on the basis of timing (Day-0) and a polish-before-feature-work framing; a reasonable observer could argue this is shell-execution infrastructure that belongs alone or with the `!`-capture surface.
3. PR #8 (harpoon) and PR #10 (quickselect) — filed under arc 06 (input-and-overlays) because both add picker-shaped overlays; a reasonable observer could argue these are feature additions belonging in their own "discoverability" arc, especially given PR #5's roadmap entry on Generalized pager picker.
4. PR #20 (`feat/scroll-altscreen-hint`) bundles three unrelated concerns: alt-screen scroll hint, `[pane] default_command`, and `gd`-vs-HEAD. Filed under arc 05 (pager) on the basis of the alt-screen hint as the user-visible headline; the bundling itself is drift fuel for the insight layer.
5. PR #14 (`fix/undo-command`) follows PR #13 (`feat/graveyard-undo`) by 25 minutes wall-clock and patches a routing bug in the feature it follows. Filed in arc 08 with PR #13; a reasonable observer could argue PR #14 belongs as a one-PR follow-up note adjacent to PR #13 specifically.

**Drift findings captured for the insight layer** (commit-subject vs. diff content; observed without resolution):

- PR #5's commit subject reads "lazygit investigation + cursor-block fix (v1.37.2)" — a one-PR bundle of a multi-day investigation deliverable plus a partial fix. The cursor-block work continues in PR #29 six calendar days later; the title prefix `investigate/` understates the diff content. The diff shape suggests the cursor-block fix in PR #5 only covers the screenshot-reproducing case; PR #29 (`fix/skip-pane-cursor-block-when-uninvited`) generalizes it.
- PR #29 (`fix/skip-pane-cursor-block-when-uninvited`) makes no mention of lazygit in title or commit subject; the link to PR #5's gap analysis is implicit and discoverable only through `notes/lazygit-gap-analysis.md` "Top suspects" §1 (subsequently relocated to `BUGS.md` by PR #12).
- PR #20 packages three unrelated concerns under a single `feat/` slug; only one of the three appears as the title headline.
- PR #36 (`fix/search-substring-match`) reads as a behavior change ("/ and = match by substring, not anchored prefix") framed as a fix. Whether this is regression repair or behavior-change-as-fix is determinable only from a prior-state inspection.
- PR #30 → PR #31 sequence: panic-recovery for vt100 0.15 lands 49 minutes before the upgrade to vt100 0.16. The diff shape across the 49-minute gap points toward the panic-recovery as the safety net that buys budget for the major-version dep bump; the chore/feat split keeps each PR's diff narrowly scoped.
- PR #14 follows PR #13 by 25 minutes — common feature-plus-immediate-hotfix shape, recurring elsewhere in the window.

Provenance:
- All 36 SHAs and PR numbers from `git log --grep='Merged in' --reverse --format='%h %ai %s'` (run 2026-05-07).
- PR #5 ROADMAP additions verified by `git diff 1f41b4b..3949983 -- ROADMAP.md`: 26 lines added at the ROADMAP tail, three lazygit-inspired entries (Generalized pager picker, Context-sensitive prompt-row hint, Scoped `?` help).
- PR #5 gap analysis source: `notes/lazygit-gap-analysis.md` (relocated by PR #12). Top-suspects section is referenced by arcs 02 and 03.
- PR #5 UX catalogue source: `notes/lazygit-ux-catalogue.md` §2 (Context-sensitive footer, "options map") and §4 (Generalized pager picker). Referenced by arcs 02, 05, 06.
- onboarding-architecture entry 0 = 01KR0P4W3ED1QZ8F44PFB2WPDZ (current-state surface descriptions).
- onboarding-product-charter entry 0 = 01KR0P18MCE1H57Q5ZTAGKAJNH (load-bearing thesis at `ROADMAP.md:3-23`).
- Prior thread entry: framing entry 0 (`history-overview` index 0) = 01KR0TRFWT9W6WMFHC49YSW0BG.

<!-- Entry-ID: 01KR0TWHTC1MPK4KJ08Y9SPE6P -->

---
Entry: Claude Code (caleb) 2026-05-07T09:05:24.606731+00:00
Role: scribe
Type: Note
Title: PR #5 special handling: own arc with mandatory back-references

Spec: scribe

tags: #history #segmentation

PR #5 (`investigate/lazygit-support`, commit 0691666, 2026-04-30) is the only PR in the window prefixed `investigate/`. Its commit subject reads "lazygit investigation + cursor-block fix (v1.37.2)" (commit 0691666, 2026-04-30). The diff is unusual: 444 insertions across 7 files, of which 399 lines are documentation (`notes/lazygit-gap-analysis.md`, 111 lines; `notes/lazygit-ux-catalogue.md`, 288 lines), 26 lines are roadmap additions (`ROADMAP.md`), 14 are CHANGELOG, and 7 are the cursor-block fix in `src/pane/widget.rs`. The investigation deliverable dominates the diff; the code fix is the smaller half.

**Disposition: PR #5 receives its own short arc (arc 02 — `lazygit-investigation-and-harvest`), paired with PR #12 (`chore/clean-notes`, commit e210e58, 2026-05-03) — "harvest lazygit notes into BUGS, drop notes/" (commit e210e58, 2026-05-03). Downstream PRs that execute against PR #5's gap analysis or UX catalogue carry mandatory back-references to arc 02 entries.**

Rationale for own arc rather than fold-with-back-refs:

1. PR #12's commit subject — "harvest lazygit notes into BUGS, drop notes/" — makes the link to PR #5 explicit. Folding PR #5 into another arc would orphan PR #12 or force PR #12 into the same target arc by adoption rather than coherence. PR #12 has no other thematic home.
2. PR #5's stated-plan output (the three ROADMAP additions, the gap-analysis suspects list) is consumed across multiple downstream arcs (arc 03 cursor-block, arc 05 pager direction, arc 06 picker pattern, arc 08 vt100 follow-on). If PR #5 lived inside any one of those arcs, the other arcs' back-references would point at a non-spine entry, and the network would lose its hub.
3. The investigation deliverable itself is the load-bearing artifact. The two notes files (399 lines combined) are not standalone code — they are an analysis artifact whose narrative belongs in a thread of its own. The gap analysis identifies three top suspects ("Spurious cursor block from `widget.rs`," "No mouse, anywhere," "Synchronized-output (mode 2026) tearing") and the UX catalogue catalogues five borrow/adapt/skip recommendations against `lazygit-upstream/`. A single-arc-thread narration of that body matches its diff shape.

Verified back-reference targets (from gap-analysis content compared against later commit subjects and titles):

- **PR #29 `fix/skip-pane-cursor-block-when-uninvited`** (commit bdb8d87, 2026-05-06) executes against PR #5's gap-analysis "Top suspects" §1: "Spurious cursor block from `widget.rs`. spyc unconditionally reverse-videoes the cell at `screen.cursor_position()`, even when the child has set DEC ?25l (cursor hidden)." PR #29's title makes no mention of lazygit; the link is detectable only via the gap-analysis text. Confidence: HIGH-CONFIDENCE EXECUTION.
- **PR #20 `feat/scroll-altscreen-hint`** (commit ee07307, 2026-05-05) — the "alt-screen scroll hint" component aligns with the catalogue's observation that pane-resident TUIs lose discoverable scroll affordance in alt-screen mode. Confidence: PARTIAL EXECUTION; PR #20's other two bundled concerns (`[pane] default_command`, `gd`-vs-HEAD) are unrelated.
- **PR #8 `feat/harpoon`** (commit 62fc129, 2026-05-02) and **PR #10 `feat/quickselect`** (commit 9043547, 2026-05-02) — parallel to but not direct execution of `notes/lazygit-ux-catalogue.md` §4 "Generalized pager picker." Both ship picker-shaped overlays; neither rewires `pager.picker_cursor` per the catalogue's recommendation ("Adapt lazygit's `Menu` popup pattern into spyc's existing `pager.picker_cursor` machinery so any list-of-options surface is a pager mode rather than a fifth overlay"). Confidence: PARALLEL PATTERN, not direct execution.
- **PR #33 `feat/pager-visual-line-mode`** (commit cf9e8ff, 2026-05-06) and **PR #35 `feat/D-opens-pager-in-top-pane`** (commit c243549, 2026-05-06) — additional pager-surface accretion that extends the read-through-pager direction the catalogue proposes. Confidence: DIRECTION ALIGNMENT, not direct execution of any specific catalogue item.

PR #5 gap-analysis suspects that do **not** appear executed in the 22-day window (negative space; insight-layer fuel):

- "No mouse, anywhere" (catalogue §3, gap-analysis suspect §2) — no mouse-capture or mouse-encoder PR appears in the 36. Aligns with `onboarding-product-charter` non-goal "mouse support beyond what already exists" (`ROADMAP.md:426-447`); the gap-analysis suspect remains as a known accepted gap rather than executable item.
- "Synchronized-output (mode 2026) tearing" (gap-analysis suspect §3) — PR #31 (`chore/vt100-and-ratatui-upgrade`, commit 105db8d, 2026-05-06) bumps vt100 from 0.15 to 0.16. Whether 0.16 brings 2026 parsing is determinable only from inspection of vendored or upstream vt100 0.16 changelog; the upgrade commit subject does not say so. Flag for arc 08 authoring and the insight layer.

Back-reference contract for downstream arc threads:

When an arc thread (history-arc-NN-…) cites a PR that executes against PR #5, it must reference arc 02's entry IDs (the framing/segmentation/closure entries on `history-overview` plus the eventual arc 02 thread entries). Specifically:
- arc 03 entry on PR #29 must back-reference arc 02 (cursor-block top-suspect link).
- arc 05 entry on PR #20 must back-reference arc 02 (alt-screen hint catalogue alignment).
- arc 06 entry on PR #8 / PR #10 must back-reference arc 02 (picker pattern parallelism).
- arc 05 entries on PR #33 / PR #35 may back-reference arc 02 (read-through-pager direction); not mandatory.

Provenance:
- 0691666 (PR #5 investigate/lazygit-support, 2026-04-30) — `git show --stat 0691666` shows 7 files changed, 444 insertions, 6 deletions; `notes/lazygit-gap-analysis.md` 111 lines added, `notes/lazygit-ux-catalogue.md` 288 lines added, `ROADMAP.md` 26 lines added, `src/pane/widget.rs` 7 lines changed.
- e210e58 (PR #12 chore/clean-notes, 2026-05-03).
- bdb8d87 (PR #29 fix/skip-pane-cursor-block-when-uninvited, 2026-05-06).
- ee07307 (PR #20 feat/scroll-altscreen-hint, 2026-05-05).
- 62fc129 (PR #8 feat/harpoon, 2026-05-02).
- 9043547 (PR #10 feat/quickselect, 2026-05-02).
- cf9e8ff (PR #33 feat/pager-visual-line-mode, 2026-05-06).
- c243549 (PR #35 feat/D-opens-pager-in-top-pane, 2026-05-06).
- 105db8d (PR #31 chore/vt100-and-ratatui-upgrade, 2026-05-06).
- `notes/lazygit-gap-analysis.md` "Top suspects" §1, §2, §3 (in PR #5's diff at `git show 0691666:notes/lazygit-gap-analysis.md`).
- `notes/lazygit-ux-catalogue.md` §2 "Context-sensitive footer" and §4 "Numbered panels & direct-jump" / "Generalized pager picker" (in PR #5's diff at `git show 0691666:notes/lazygit-ux-catalogue.md`).
- `onboarding-product-charter` entry 0 = 01KR0P18MCE1H57Q5ZTAGKAJNH (mouse non-goal at `ROADMAP.md:426-447`).
- Prior thread entries: history-overview index 0 = 01KR0TRFWT9W6WMFHC49YSW0BG (framing); index 1 = 01KR0TWHTC1MPK4KJ08Y9SPE6P (segmentation).

<!-- Entry-ID: 01KR0TYF5F11DA8P5HNPA20DBK -->

---
Entry: Claude Code (caleb) 2026-05-07T09:06:16.551210+00:00
Role: scribe
Type: Note
Title: Closure: segmentation published; awaiting review before Phase 2

Spec: scribe

tags: #history #segmentation

Segmentation proposal published, awaiting operator + observer review before Phase 2 (arc 01).

This thread (`history-overview`) remains OPEN to receive arc-session cross-references. No arc threads have been written this session; only the spine.

**Arc threads that will exist after Phase 2 completes** (observer checklist):

- `history-arc-01-foundation-hygiene` — PRs #2, #3, #4 (3 commits; 2026-04-30)
- `history-arc-02-lazygit-investigation-and-harvest` — PRs #5, #12 (2 commits; 2026-04-30 to 2026-05-03; SPECIAL HANDLING)
- `history-arc-03-pane-behavior` — PRs #6, #22, #26, #29, #34 (5 commits; 2026-05-01 to 2026-05-06)
- `history-arc-04-git-integration` — PRs #1, #7, #15, #24, #27 (5 commits; 2026-04-30 to 2026-05-06)
- `history-arc-05-pager-surface` — PRs #11, #16, #17, #20, #23, #33, #35, #36 (8 commits; 2026-05-02 to 2026-05-07)
- `history-arc-06-input-and-overlays` — PRs #8, #10, #25, #32 (4 commits; 2026-05-02 to 2026-05-06)
- `history-arc-07-codex-and-mcp-bridge` — PRs #18, #19, #21, #37 (4 commits; 2026-05-05 to 2026-05-07)
- `history-arc-08-recoverability-and-deps` — PRs #13, #14, #28, #30, #31 (5 commits; 2026-05-03 to 2026-05-06)

Total: 8 arc threads, 36 PRs accounted for (3 + 2 + 5 + 5 + 8 + 4 + 4 + 5 = 36).

**Pre-Phase-2 review surface for the observer:**

1. Cluster boundary calls flagged at the segmentation entry (5 of them; PR #15, PR #4, PR #8/#10, PR #20, PR #14). Each is filed under one arc but defensibly belongs to another.
2. PR #5 disposition (own arc + back-reference contract, see special-handling entry). Alternative: fold PR #5 into arc 03 with cursor-block link, lose hub.
3. Drift findings (6 captured at the segmentation entry). Each is fuel for the eventual insight layer; observer review can confirm or contest before any arc thread freezes them in.
4. The "negative space" reading captured at the special-handling entry: gap-analysis suspects #2 (mouse) and #3 (synchronized-output) have no executing PR in the window. PR #31's vt100 0.15 → 0.16 upgrade may incidentally close suspect #3; verification is deferred to arc 08 authoring.

**Phase 2 expected sequencing:**

- Arc 01 first (smallest arc, lowest risk; validates the voice and provenance contracts on a 3-PR set).
- Arc 02 next (anchors the back-reference network for arcs 03, 05, 06).
- Arcs 03 / 04 / 05 / 06 / 07 / 08 in any order; 03 and 05 carry mandatory back-references to arc 02.
- Phase 3 (insight layer) blocks on all eight arc threads existing.

**This session's done-definition (self-check):**
- ☑ `watercooler_health` reported Healthy against the spyc code_path.
- ☑ Three onboarding seeds read before write (overview, product-charter, architecture).
- ☑ Thread `history-overview` exists with framing, segmentation, PR #5 special-handling, and closure entries (4 entries).
- ☑ Every entry carries a `Provenance:` block with real SHAs.
- ☑ Every entry follows the voice contract — banned words audit: no occurrence of "wants," "thinks," "believes," "decided" (about the maintainer's mindset; the word appears here only in self-referential framing of segmentation choices), "feels," "intends to" (without "the commit message"), "is concerned that" in reference to the maintainer's mindset.
- ☑ Closure entry lists every proposed arc thread name.
- ☑ No arc threads written this session.
- ☑ Tags `#history` and `#segmentation` carried on every entry.

Provenance:
- d9b9360 (PR #2 chore/ci-hygiene, 2026-04-30) — start of window.
- a303251 (PR #37 fix/mcp-socket-project-scoped-discovery, 2026-05-07) — end of window.
- All 36 SHAs and PR numbers from `git log --grep='Merged in' --reverse --format='%h %ai %s'`.
- Prior thread entries: index 0 = 01KR0TRFWT9W6WMFHC49YSW0BG (framing); index 1 = 01KR0TWHTC1MPK4KJ08Y9SPE6P (segmentation); index 2 = 01KR0TYF5F11DA8P5HNPA20DBK (PR #5 special handling).

<!-- Entry-ID: 01KR0V01TAJVSZFE5ZNMCZHQSF -->

---
Entry: Claude Code (caleb) 2026-05-07T09:52:57.040268+00:00
Role: scribe
Type: Note
Title: Tail: looking back at the segmentation as a deliverable

Spec: scribe

tags: #history #segmentation

Looking back at the four head entries above, the segmentation entry (index 1) is the load-bearing artifact in this thread. Everything else is scaffolding around it — the framing entry sets the voice and provenance contracts, the PR #5 special-handling entry ratifies the one disposition that doesn't fit the natural clusters, and the closure entry just hangs a Phase-2 checklist on the door. Strip those three and you'd still have the eight arcs. Strip the segmentation, and there's no Phase 2 to ship.

The choices that mattered, now that arc 01 has been written and we can read them from one arc's distance:

The eight-arc count was a decision, not a discovery. Thirty-six PRs across twenty-two days could have been segmented as four broad arcs (foundation, surfaces, integrations, survival) or as twelve narrow ones (one per file-system area). Eight is a midpoint that keeps each arc small enough to narrate in one session — arc 01 took one session for three PRs — while keeping the count low enough that the spine doesn't fragment. Whether eight scales for the larger arcs (arc 05's eight PRs in particular) is something only arc 05's session will tell us.

Giving PR #5 its own arc reads as the right call from where we sit. Arc 01's closure entry already had to forward-reference arc 02 to flag the v1.37.2 commit-subject overlap. If PR #5 had been folded into arc 03 or arc 06, the cross-arc back-references from arcs 03/05/06 would point at a non-spine entry, and the network would lose its hub. Looking back, the alternative would have produced a denser local arc and a thinner global reading — the wrong trade for an exercise whose value is the cross-cutting reading.

PR #4's disposition (kept in arc 01 on structural-binding grounds, not just timing) is the segmentation choice that will get tested most by Phase 3. The framing entry argued PR #4 belongs with PR #2 + PR #3 because it's the version-cutter; the per-PR PR #4 entry then made the structural-binding case explicit. If the eventual recurrence-or-emergence insight thread reads release-cuts as a recurring shape — and the v1.41.x cadence over arcs 03/05/08 suggests it might — then PR #4-as-version-cutter is one data point in a pattern, not just an arc-01 quirk.

What's been left to later: per-arc narration (in the arc threads), the negative-space readings (gap-analysis suspects #2 mouse and #3 synchronized-output) carried into arc 02 and arc 08, and the entire insight layer (Phase 3). The spine doesn't claim to do that work. It claims only to make the work writable.

Provenance:
- No new commit references; this entry reflects on the head entries which carry full SHA provenance.
- `history-overview` entry 0 = 01KR0TRFWT9W6WMFHC49YSW0BG (framing).
- `history-overview` entry 1 = 01KR0TWHTC1MPK4KJ08Y9SPE6P (segmentation; the load-bearing artifact).
- `history-overview` entry 2 = 01KR0TYF5F11DA8P5HNPA20DBK (PR #5 special handling).
- `history-overview` entry 3 = 01KR0V01TAJVSZFE5ZNMCZHQSF (closure).
- `history-arc-01-foundation-hygiene` framing entry = 01KR0W6FR7T01ZJR84MRKWA13A (the first arc whose existence lets us look back at the segmentation from one-arc distance).
- `history-arc-01-foundation-hygiene` closure entry = 01KR0WD8428XFNTJV11MXX59NF (forward-references arc 02 with the v1.37.2 commit-subject overlap question).

<!-- Entry-ID: 01KR0XNGQ0GS2QYN855X25NPZ7 -->

---
Entry: Claude Code (caleb) 2026-05-07T09:53:37.547684+00:00
Role: scribe
Type: Note
Title: Tail: note to future readers — how to use this thread

Spec: scribe

tags: #history #segmentation

If you're reading this on the dashboard and trying to figure out what `history-overview` is for: this is the spine of an eight-arc reconstruction of spyc's first 22 days of merged work. The reconstruction lives across nine threads — this spine plus eight arc threads — and the spine does one job: it lays out which arcs exist, why they're shaped that way, and which boundary calls were genuinely hard.

What's in this thread, head entries (indices 0–3):
- Index 0 — the framing entry. Voice contract, provenance contract, audience.
- Index 1 — the segmentation. Eight arcs, member PRs, rationale per arc, five flagged boundary calls, six drift findings reserved for the eventual insight layer.
- Index 2 — PR #5's special handling. The only PR in the window that gets its own arc with a back-reference contract.
- Index 3 — closure. Phase-2 sequencing checklist.

What's *not* here: per-arc narration. None of the head entries above narrate a single PR's diff. That work lives in `history-arc-NN-…` threads — start with `history-arc-01-foundation-hygiene` and walk forward. The arc threads are the bulk of the work; the spine is the index.

These two tail entries (indices 4 and 5) read differently from the head on purpose. The four head entries above are clinical and segmented because the segmentation deliverable needed that voice. The tails are looser — first-person plural where it helps, direct address (you're reading one right now), retrospective. They exist to tell you *what to do with* the thread that the head can't tell you because the head is busy *being* the deliverable.

How to navigate from here:
- For arc-by-arc narration: open the arc threads by topic name. The names are listed in the closure entry's checklist (`history-arc-01-foundation-hygiene` through `history-arc-08-recoverability-and-deps`).
- For chronology in PR-number order: arc threads cite PRs in their natural arc order, which isn't always wall-clock order. If you want strictly chronological, the segmentation entry's commit lists give you the SHA and date for every PR.
- For the back-reference network: arcs 03, 05, and 06 carry mandatory back-references to arc 02 (the lazygit-investigation-and-harvest hub). Following those links is how you reconstruct the cross-cutting reading from the spine view.

What to expect from the eventual insight threads (Phase 3): drift, recurrence, trajectory, and emergence threads written *after* all eight arcs exist. They'll cite arc-entry IDs by ULID and pull from the drift findings the segmentation entry already flagged. If those threads exist when you read this, follow their entry IDs back to the per-PR entries they reference. If they don't exist yet, the segmentation's six drift findings are the seed list.

If something here is wrong, the head entries are sealed by provenance contract — what you'd amend lives in a follow-up arc thread or in a new insight thread, not in a retroactive head edit. We're betting that letting the record stand and writing forward is more honest than rewriting.

Provenance:
- No new commit references; tails reflect on the head entries which carry full SHA provenance.
- `history-overview` entries 0–3 = 01KR0TRFWT9W6WMFHC49YSW0BG, 01KR0TWHTC1MPK4KJ08Y9SPE6P, 01KR0TYF5F11DA8P5HNPA20DBK, 01KR0V01TAJVSZFE5ZNMCZHQSF.
- `history-overview` tail entry 4 = 01KR0XNGQ0GS2QYN855X25NPZ7 (the reflection tail this note-to-future-readers entry sits beside).
- `history-arc-01-foundation-hygiene` thread (the only arc thread written at the time of this tail; topic-name template for the others).

<!-- Entry-ID: 01KR0XPRBJVH80FNZ9XN7DG01E -->
