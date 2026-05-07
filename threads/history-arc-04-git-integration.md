# history-arc-04-git-integration — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: history-arc-04-git-integration
Created: 2026-05-07T11:22:51.278100+00:00

---
Entry: Claude Code (caleb) 2026-05-07T11:22:51.278100+00:00
Role: scribe
Type: Note
Title: Framing: arc 04 — capability accretion across five axes, longest arc, two empirical refutations

Spec: scribe

tags: #history #arc-04

Arc title: `git-integration`. Date span: 2026-04-30 (PR #1) to 2026-05-06 (PR #27) — the full 22-day window, the longest span of any arc. Member PRs:

- cd8df2e (PR #1 fix/git-marker-1hz-poll, 2026-04-30 17:08) — "git markers: 1Hz safety-net poll for missed FSEvents (v1.37.1)" (commit cd8df2e, 2026-04-30).
- f3ddaf2 (PR #7 feat/limit-git, 2026-05-02 11:53) — "limit: =git / =g shows files in git status (v1.38.1)" (commit f3ddaf2, 2026-05-02).
- 5999261 (PR #15 fix/git-status-and-pane-ctrl-c, 2026-05-04 11:26) — "fix: ^C → pane child + git markers don't leak across same-name files (v1.41.2)" (commit 5999261, 2026-05-04).
- 762a0a6 (PR #24 feat/jump-git-change, 2026-05-05 16:26) — "feat: ]g / [g jump cursor to next/prev git-changed entry (v1.41.11)" (commit 762a0a6, 2026-05-05).
- 4e2afd9 (PR #27 feat/git-staged-vs-unstaged, 2026-05-06 16:51) — "feat: two-char git markers for staged-vs-unstaged distinction (v1.41.14)" (commit 4e2afd9, 2026-05-06).

**Diagnosis: capability accretion along five axes; pattern-8 (reference / inventory) register for the framing; per-entry shape variety driven by what each diff is.** Arc 04 reads structurally different from arc 03. Arc 03 was the same surface returning under new constraints — a recurring-concern shape with no pivot. Arc 04 is the opposite: each PR adds a *different* facet of git-awareness without backtracking on any prior PR. Five axes surface across the window — marker reliability (PR #1), filtering (PR #7), marker correctness (PR #15's git half), navigation (PR #24), marker fidelity (PR #27) — and the arc accretes along each independently. PR #27 extends the parser machinery PR #15 introduced, but that reads as ground-up *extension* rather than supersession; PR #15's extracted `parse_porcelain_statuses` keeps doing the same job, with a richer return type. No PR in arc 04 supersedes another. The framing entry leans pattern-8's declarative register because the organizing question of the arc — "what does git-awareness mean in this file commander?" — is reference-shaped: the answer is a list of facets, not a story arc.

The per-entry shapes do not all match the framing register. Inheriting arc 03's precedent of material-driven shape variety, each per-PR head entry below picks the shape its diff actually wants:

- PR #1 (1Hz safety-net poll): feature-compact, system-reliability flavor. Names the FSEvents soft spot explicitly.
- PR #7 (`=git`/`=g` filter): feature-compact with a fork-out observation. The BUGS.md edit splits a single user request — "files being worked on" — into two halves: the `=git` filter shipped here, and a deferred harpoon-style pinned-set design pass. The harpoon ships in arc 06's PR #8 four hours later. Arc 04 is therefore the genesis of `=git` *and* a fork point for arc 06's harpoon.
- PR #15 (`^C` → pane + git-marker leak): bundle-shape with a within-entry phase split. Two independent fixes in different files (app/mod.rs +5 lines for `^C`-routing; sysinfo.rs +87 lines for the basename-collision in `git_file_statuses`). Diff inspection confirms the two halves do not share a root cause; the entry treats them as two observations under one PR rather than collapsing them into one. See "PR #15 disposition" below.
- PR #24 (`]g`/`[g` jump): feature-shaped with a brief refutation of the brief's lazygit-roadmap hypothesis (see "Empirical refutations" below). Pure-domain logic on `AppState` plus resolver chord-prefix support; 5 unit tests pin the behavior cases.
- PR #27 (two-char staged-vs-unstaged markers): refactor-extends-PR-15 shape with a brief refutation of the brief's lazygit-roadmap hypothesis. Largest src diff in the arc (174 lines `list_view.rs` + 116 lines `sysinfo.rs`). Promotes `GitFileStatus` from flat enum to struct with independent `staged: Option<GitChange>` / `unstaged: Option<GitChange>` / `untracked: bool` fields.

**Cadence choice: option A — per-PR — with PR #15's entry containing a within-entry phase split.** Five PRs → five per-PR entries plus framing and closure → seven head entries. The within-entry split for PR #15 names two phases under one entry's heading, rather than splitting PR #15 into two entries. The reasoning: option A's per-PR cadence preserves PR-level provenance grain, which matters here because no within-arc supersession or strong sequence dependency forces a phase-not-PR shape; what arc 04 wants instead is one entry per PR, with PR #15's bundle treated honestly as one PR carrying two concerns. Arc 02's option B (phase-not-PR) does not fit because there are no phases — only axes. Arc 03's option A precedent inherits cleanly.

**PR #15 disposition: one PR, two concerns, single entry with within-entry phase split.** The Phase-1 segmentation entry on `history-overview` (= 01KR0TWHTC1MPK4KJ08Y9SPE6P) flagged PR #15 as a hard cluster-boundary call: defensibly arc-03 (pane-control via the `^C`-route guard) or arc-04 (git-marker basename-collision fix). The segmentation filed it under arc-04 on the basis that the git-marker-leak half is the larger diff (87 lines vs 5) and its commit-subject token reads as the title's load-bearing concern. Diff inspection confirms the asymmetry. The empirical question — whether the two halves share a common root cause or are coincident in one PR — resolves toward *coincident*. The `^C`-route fix in `src/app/mod.rs:2679-2693` adds a `pane_has_focus` guard to the existing "^C is not a quit binding" footgun-flash, so a focused-pane `^C` reaches the child rather than tripping the flash. The git-marker-leak fix in `src/sysinfo.rs:62-160` extracts a pure-parser `parse_porcelain_statuses` and adds an `in_this_dir` distinction so a deep entry (`content-acquisition/CLAUDE.md`) does not write its basename over a same-named root row (`CLAUDE.md`). Different files, different concerns, no shared call chain, no shared root cause. The PR #15 entry below treats them as two observations within one entry, in the diff-weight order (git-marker leak first, `^C`-routing second).

The five-minute check the brief flagged — whether PR #15's pane-control fix shares lineage with arc 03's PR #34 overlay-focus model — comes back orthogonal. PR #15's `pane_has_focus = self.pane_tabs.is_some() && self.state.pane_focused` is a precondition gate added to one footgun-flash early in `App::handle_key`; it does not interact with PR #34's overlay-vs-pane focus axis (= 01KR10JBACRS3Z71WTHGBVCPJM, which arrives two days later in arc 03). Both consult `pane_focused`, but the consumption shapes differ — PR #15 uses it as a one-bit "skip the flash" gate, PR #34 uses it as the source axis for an overlay/list/pane focus tri-state. Arc 03's seams-aside (= 01KR11TME2KF5QFQ45GJYG8MC7) names `pane_focused`'s post-PR-#34 three-meaning load; PR #15's use sits cleanly in the original list-vs-pane meaning and predates the overlay axis.

**Empirical refutations (brief-flagged hypothesis, refuted against the diffs).** The brief proposed that PR #24 and PR #27 are "likely lazygit-roadmap executions per arc 02's hub disposition" against `notes/lazygit-ux-catalogue.md` (read at commit 0691666). Verification against the catalogue (= arc 02's investigation entry 01KR0YXXZRQR24CSNAK4Q7808T, which preserves the ux-catalogue text inline because PR #12 deleted the `notes/` files) does not bear out the hypothesis:

- **PR #24 does not execute against catalogue §1.** §1 ("Numbered panels & direct-jump") is the only "jump" item in the catalogue and its disposition is **skip** verbatim: "spyc has exactly two top-level surfaces (list, pane) where lazygit has five, so `1` and `2` would be wasted on a binding that `^W j`/`^W k` already covers cleanly." The catalogue's §1 is about pressing `1`..`5` to jump between panels. PR #24 ships `]g`/`[g` — vim's hunk-navigation idiom (`]c`/`[c` for hunks in vim-fugitive / vim-gitgutter) generalized to dirty-row navigation in spyc's listing. The PR #24 resolver code (`src/keymap/resolver.rs:340-352`) names `[t/]t` and `[b/]b` as the existing pattern PR #24 inherits ("Bracket pairs are reserved for 'next/prev <thing>' jumps"), and those existing chords are pager-internal vim-style jumps, not lazygit-derived. PR #24 reads as a continuation of spyc's pre-existing vim-bracket-jump family, not as catalogue execution.

- **PR #27 does not execute against any catalogue section.** The seven catalogue sections (§1 numbered panels & direct-jump skip, §2 context-sensitive footer adapt, §3 command log + random tip skip-the-log/adapt-the-tip, §4 popups/pickers adapt, §5 scoped `?` help adapt, §6 single-key row-verb panel reuse skip, §7 two-letter chord jumps skip) are about UI patterns (panel routing, popup machinery, footer affordances, help structure). None catalogue git-data fidelity at the marker level. PR #27's two-character XY-pair display promotes `GitFileStatus` from a flat enum to a struct with independent staged/unstaged halves; the surface choice (two cells in the existing 2-wide marker column) reads structurally similar to `git status -s`'s native rendering, not to lazygit's two-panel staged/unstaged split. No catalogue text covers this design space.

The refutations matter for the back-reference network: arc 04 carries no mandatory back-references to arc 02. Arc 02's published back-reference table (= 01KR0Z3673Z27FJ4GV92FYV4QJ) does not enumerate arc 04 by PR number, consistent with this finding. Arc 04 cites arc 02 below only because the empirical refutation requires walking the catalogue text, and arc 02's investigation entry is the only durable home for the catalogue's verbatim dispositions (PR #12 deleted the `notes/` source).

**Cross-thread back-link**: this thread continues from `history-overview`:
- Framing entry 0 = 01KR0TRFWT9W6WMFHC49YSW0BG.
- Segmentation entry 1 = 01KR0TWHTC1MPK4KJ08Y9SPE6P (arc 04 PR list; PR #15 cluster-boundary call source).
- PR #5 special-handling entry 2 = 01KR0TYF5F11DA8P5HNPA20DBK (back-reference contract).
- Closure entry 3 = 01KR0V01TAJVSZFE5ZNMCZHQSF (arc thread name list).

And from arc 02 (catalogue-verification target only):
- Investigation entry = 01KR0YXXZRQR24CSNAK4Q7808T (preserves verbatim ux-catalogue dispositions; cited by PR #24 and PR #27 entries below for the empirical refutation).
- Closure / back-reference table = 01KR0Z3673Z27FJ4GV92FYV4QJ (does not enumerate arc 04 — confirms refutation).

This thread remains OPEN for cross-arc references.

**Day-0 sequence note.** PR #1 lands at 17:08 on 2026-04-30 — eighteen minutes after PR #2 (CI hygiene, arc 01) merged at 16:50. PR #1 is the first user-facing fix of the project; PR #2 was the prior commit and was infrastructure-only. The git-marker surface was the first thing the project surfaced as needing a follow-on after the window opened. The PR #1 entry below treats this as a sequence fact, not as a thesis about the surface's importance.

Provenance:
- cd8df2e (PR #1 fix/git-marker-1hz-poll, 2026-04-30 17:08) — first commit in arc 04; ~18 minutes after PR #2 (arc 01).
- f3ddaf2 (PR #7 feat/limit-git, 2026-05-02 11:53).
- 5999261 (PR #15 fix/git-status-and-pane-ctrl-c, 2026-05-04 11:26) — cluster-boundary call from segmentation.
- 762a0a6 (PR #24 feat/jump-git-change, 2026-05-05 16:26).
- 4e2afd9 (PR #27 feat/git-staged-vs-unstaged, 2026-05-06 16:51) — last commit in arc 04.
- `git show --stat 5999261` confirms the bundle: app/mod.rs +5/-0 (^C-route half), sysinfo.rs +75/-12 (git-marker-leak half).
- `git show 0691666:notes/lazygit-ux-catalogue.md` (read for empirical refutation) — §1 disposition "skip", verbatim text quoted in arc 02 investigation entry.
- `history-overview` segmentation entry = 01KR0TWHTC1MPK4KJ08Y9SPE6P (arc 04 PR list; PR #15 cluster-boundary call).
- `history-overview` PR #5 special-handling entry = 01KR0TYF5F11DA8P5HNPA20DBK (back-reference contract; arc 04 not enumerated).
- `history-arc-02-lazygit-investigation-and-harvest` investigation entry = 01KR0YXXZRQR24CSNAK4Q7808T (catalogue text preserved verbatim).
- `history-arc-02-lazygit-investigation-and-harvest` closure entry = 01KR0Z3673Z27FJ4GV92FYV4QJ (back-reference table; arc 04 absence confirms refutation).
- `history-arc-03-pane-behavior` PR #34 entry = 01KR10JBACRS3Z71WTHGBVCPJM (overlay-focus model; PR #15 lineage-check target — orthogonal).
- `history-arc-03-pane-behavior` seams-aside = 01KR11TME2KF5QFQ45GJYG8MC7 (`pane_focused`'s post-PR-#34 three-meaning load; PR #15 predates and uses original meaning only).
- `history-arc-01-foundation-hygiene` PR #2 entry = 01KR0W81XE4K3G7BBSP42GE1HH (PR #2 = arc 01 first commit, merged 16:50; PR #1 follows ~18 min later).

<!-- Entry-ID: 01KR12T4DHGDH3B9YYXM0F093A -->

---
Entry: Claude Code (caleb) 2026-05-07T11:23:53.929285+00:00
Role: scribe
Type: Note
Title: PR #1 (fix/git-marker-1hz-poll): the first user-facing follow-on, an FSEvents backstop, 0-dps idle preserved

Spec: scribe

tags: #history #arc-04

PR #1 is the first move in arc 04 and the first user-facing fix of the 22-day window. It lands eighteen minutes after PR #2 (arc 01's CI hygiene merge at 16:50), making the git-marker surface the first thing the project surfaces as needing a follow-on after the window opens. Commit subject reads "git markers: 1Hz safety-net poll for missed FSEvents (v1.37.1)" (commit cd8df2e, 2026-04-30). Diff: 6 files, +71/-2. Source code: 37 lines across `src/app/state.rs` (+19) and `src/app/mod.rs` (+18).

**The bug being fixed.** Stale `+`/`~`/`?` markers (and the top-bar branch/dirty string) sometimes stayed visible after a `git commit` until the user changed directories. The PR's CHANGELOG names the cause: "the `notify`-driven FSEvents watch on `.git/` would occasionally miss the `.git/index.lock` → `.git/index` atomic rename that happens on every commit -- macOS FSEvents has a known soft spot for inode replacement" (commit cd8df2e, 2026-04-30). The watcher is correct in principle and the watcher path is preserved unchanged; what this PR adds is a backstop, not a replacement.

**The fix shape: 1Hz diff-aware poll.** A new `AppState::refresh_git_state` method re-runs `crate::sysinfo::git_status` and `crate::sysinfo::git_file_statuses`, compares against the live `git_info` and `git_files` fields, and returns early with `false` when nothing changed. Only on a real diff does it overwrite the live state, call `rebuild_rows`, and return `true` (`src/app/state.rs:474-494` post-merge). The doc-comment names the design-load explicitly: "The diff guard preserves the 0-dps-idle target: when nothing changed, we don't bump `list_generation` or request a repaint."

The driver lives in `App::run` (`src/app/mod.rs:1352-1366` post-merge): a `last_git_poll: Instant` is updated each tick and `GIT_POLL_INTERVAL = Duration::from_secs(1)` gates re-entry. The poll fires only when `self.state.git_info.is_some()` — outside a git repo there is nothing to converge on. When `refresh_git_state()` returns `true`, the surrounding code sets `needs_draw = true; draw_reason = 3`, which is the same dirty-frame mechanism arc 03's pane-state code paths use.

**Sequence-grain consequence for arc 04.** PR #1 is the genesis of the 1Hz git-poll machinery that PR #7 (`=git`/`=g` filter, this arc, two days later) reuses without modification. PR #7's CHANGELOG names the dependency directly: "The filter stays live as the 1Hz git poll updates `git_files`" (commit f3ddaf2, 2026-05-02). The poll's diff-aware return value — bumping `list_generation` only on real change — is what lets the filter re-render without burning a frame on every idle tick.

**Drift findings flagged for the insight layer**:

- The PR ships at v1.37.1, but the 22-day window's first merge by wall-clock is PR #2 (arc 01's `chore/ci-hygiene`, 2026-04-30 16:50), which lands as `[Unreleased]` content with no version bump (per arc 01's PR #2 entry = 01KR0W81XE4K3G7BBSP42GE1HH). PR #1's `(v1.37.1)` tag therefore reads as the *first version cut* of the window — and the v1.37.1 release ships a single fix (the 1Hz poll). PR #4 cuts v1.37.2 hours later, packaging arc 01's three PRs together (per arc 01's PR #4 entry = 01KR0WBKNMQF231X2T8KTGD9KS). Two release cuts on Day 0, one for arc 04, one for arc 01.

- The fix is a *backstop*, not a replacement. The watcher path stays in place; the poll runs in addition. The CHANGELOG names this design choice verbatim ("Watcher path is unchanged; this is a backstop, not a replacement"). A reader scanning the title alone might infer that polling supplants the FSEvents watch; the diff and CHANGELOG together confirm the watch is preserved.

- The poll only runs when in a git repo (`self.state.git_info.is_some()`). Outside a repo it never fires. The 0-dps-idle target is preserved on both axes — by the in-repo gate and by the diff-aware return value.

Provenance:
- cd8df2e (PR #1 fix/git-marker-1hz-poll, 2026-04-30 17:08) — full PR.
- `src/app/state.rs:474-494` (post-merge) — `refresh_git_state` body; doc-comment quoted verbatim.
- `src/app/mod.rs:999-1003,1352-1366` (post-merge) — `last_git_poll: Instant` declaration and the 1Hz gate in `App::run`.
- `git diff cd8df2e^1..cd8df2e^2 -- CHANGELOG.md` — verbatim quotes ("FSEvents has a known soft spot for inode replacement"; "Watcher path is unchanged; this is a backstop, not a replacement"; "diff-aware -- only bumps `list_generation` and requests a repaint when something actually changed, so idle dps stays at 0").
- d9b9360 (PR #2 chore/ci-hygiene, 2026-04-30 16:50) — preceding merge by wall-clock; PR #1 follows ~18 minutes later.
- f3ddaf2 (PR #7 feat/limit-git, 2026-05-02) — downstream consumer of `refresh_git_state`'s `git_files` updates; named here for sequence-grain forward reference.
- `onboarding-architecture` entry 0 = 01KR0P4W3ED1QZ8F44PFB2WPDZ — current-state seed describing the dirty-frame `needs_draw` machinery; this PR adds a new `draw_reason = 3` source within that machinery.
- `history-overview` framing entry = 01KR0TRFWT9W6WMFHC49YSW0BG.
- `history-arc-04-git-integration` framing entry = 01KR12T4DHGDH3B9YYXM0F093A.
- `history-arc-01-foundation-hygiene` PR #2 entry = 01KR0W81XE4K3G7BBSP42GE1HH (sequence reference: PR #2 = wall-clock-first merge of the window).
- `history-arc-01-foundation-hygiene` PR #4 entry = 01KR0WBKNMQF231X2T8KTGD9KS (sequence reference: PR #4 cuts v1.37.2 hours later — two release cuts on Day 0, this PR's v1.37.1 + arc 01's v1.37.2).

<!-- Entry-ID: 01KR12W1M20SQW3QXT8VC09REK -->

---
Entry: Claude Code (caleb) 2026-05-07T11:24:52.092443+00:00
Role: scribe
Type: Note
Title: PR #7 (feat/limit-git): =git/=g filter, the BUGS.md split, fork-out to arc 06's harpoon

Spec: scribe

tags: #history #arc-04

PR #7 is the second move in arc 04 and the first additive feature on the git-awareness surface. Commit subject reads "limit: =git / =g shows files in git status (v1.38.1)" (commit f3ddaf2, 2026-05-02). Diff: 9 files, +57/-11. Source code: 22 lines in `src/app/state.rs`.

**The feature.** A new limit-filter pattern `git` (or shorthand `g`) — typed at the `=` prompt — restricts the listing to entries whose `git_files` lookup returns a non-`Clean` status. Implementation is a single `else if` arm in `AppState::apply_temp_filter` (`src/app/state.rs:441-456` post-merge): for each row, look up `self.git_files.get(&r.display)`, default to `Clean` on miss, retain the row if non-clean. The filter coexists with `git_files`'s parent-directory marking (a deep entry's basename + `/` is keyed in the map under PR #15's later parent-marking rule), so directories containing changes stay navigable when the filter is active.

A second arm in the `=` prompt-handler (`src/app/state.rs:1230-1236` post-merge) adds the no-changes guard: when `self.git_files.is_empty()` the filter flashes "not in a git repo (or no changes)" instead of presenting an empty list. The PR's CHANGELOG entry names this verbatim: "Outside a git repo (or with no changes), applying `=git` flashes 'not in a git repo (or no changes)' instead of silently showing an empty list" (commit f3ddaf2, 2026-05-02).

**Reuse of PR #1's machinery.** The CHANGELOG names the dependency directly: "The filter stays live as the 1Hz git poll updates `git_files`" (commit f3ddaf2, 2026-05-02). PR #1 (this arc, two days earlier) added `AppState::refresh_git_state` whose diff-aware behavior bumps `list_generation` only on real change. PR #7's filter consumes the resulting `git_files` map without itself touching the poll machinery — when a user stages or commits while the filter is active, the listing converges within a second through PR #1's backstop. The 0-dps-idle target is preserved transitively.

**The BUGS.md fork.** The PR's BUGS.md edit (`git diff f3ddaf2^1..f3ddaf2^2 -- BUGS.md`) replaces a single SMALL line — "= should be able to show 'files being worked on' which could be files included in git status or setup like the harpoon tool in neovim" — with two SMALL items. The first half ("`=git` / `=g` filter") ships in this PR. The second half ("harpoon-style 'currently working on' pinned set") is rewritten as a deferred design pass: "small ordered per-project file list with quick numeric jumps, persistent across sessions; not just a filter mode. Distinct from picks (per-dir, ephemeral), marks (single-file pointer per letter), inventory (yank stash). Needs a real design pass — design space overlaps existing concepts." (BUGS.md post-merge, 2026-05-02).

The harpoon-style pinned set ships four hours later in arc 06's PR #8 (`feat/harpoon`, 62fc129, 2026-05-02 18:04). Arc 04 is therefore the genesis of `=git` *and* a fork point for arc 06's harpoon: the BUGS.md split rewrites the original combined request into two artifacts, only one of which lives in arc 04. Arc 06 will narrate PR #8 against the arc-02 lazygit-ux-catalogue §4 picker pattern; the BUGS.md text PR #7 leaves behind is the design-space framing that arc 06 inherits.

**Drift findings flagged for the insight layer**:

- The PR cuts v1.38.1, but the prior version on `main` post-merge of PR #6 (arc 03's `feat/pane-zoom`, 2026-05-01) is v1.38.0. PR #7 is therefore a patch-level bump on top of arc 03's minor cut. The version cadence does not run lock-step with arc number: arc 04's first PR cut v1.37.1 (Day 0), arc 03's first PR cut v1.38.0 (Day 1), arc 04's second PR cuts v1.38.1 (Day 2). The arcs interleave at the version axis without colliding.

- The CHANGELOG framing reads "Requested via BUGS.md; the harpoon-style pinned-set part of that request is split out for a deeper design pass" (commit f3ddaf2, 2026-05-02). The split itself is named in the changelog, but the destination of the split (PR #8 four hours later) is not. A reader walking the changelog forward sees the split announced but not closed; arc 06's narration is what closes it.

- Code-shape detail: the filter implementation is 22 lines and lives entirely on `AppState`. There is no resolver work, no UI work, no help-text update beyond a one-line addition in `src/ui/help.rs`. The CHANGELOG calls out the no-empty-listing flash; the help text adds "`=git` / `=g`" to the limit-filter list. This is a small, scoped diff that earns its keep by reusing PR #1's machinery on one axis and `apply_temp_filter`'s pre-existing pattern-dispatch on another.

Provenance:
- f3ddaf2 (PR #7 feat/limit-git, 2026-05-02 11:53) — full PR.
- cd8df2e (PR #1 fix/git-marker-1hz-poll, 2026-04-30) — `refresh_git_state` and 1Hz poll machinery this PR reuses without modification.
- `src/app/state.rs:441-456` (post-merge) — filter arm in `apply_temp_filter`.
- `src/app/state.rs:1230-1236` (post-merge) — no-changes guard in the `=` prompt handler.
- `git diff f3ddaf2^1..f3ddaf2^2 -- BUGS.md` — the SMALL-line split (one entry → two; harpoon half rewritten as deferred design pass).
- `git diff f3ddaf2^1..f3ddaf2^2 -- CHANGELOG.md` — verbatim quotes ("The filter stays live as the 1Hz git poll updates `git_files`"; "harpoon-style pinned-set part of that request is split out for a deeper design pass").
- 62fc129 (PR #8 feat/harpoon, 2026-05-02 18:04) — arc 06 destination for the deferred half; named here for fork-out forward reference.
- `history-arc-04-git-integration` framing entry = 01KR12T4DHGDH3B9YYXM0F093A.
- `history-arc-04-git-integration` PR #1 entry = 01KR12W1M20SQW3QXT8VC09REK.
- `history-overview` segmentation entry = 01KR0TWHTC1MPK4KJ08Y9SPE6P (arc-04 PR list; arc 06 PR list).
- `history-arc-02-lazygit-investigation-and-harvest` investigation entry = 01KR0YXXZRQR24CSNAK4Q7808T (catalogue §4 picker pattern context for arc 06's eventual harpoon narration).

<!-- Entry-ID: 01KR12XTG7E5TC0RNTJ65G67T7 -->

---
Entry: Claude Code (caleb) 2026-05-07T11:26:10.705344+00:00
Role: scribe
Type: Note
Title: PR #15 (fix/git-status-and-pane-ctrl-c): one PR, two coincident concerns — basename-collision in porcelain parsing, and the ^C-route guard

Spec: scribe

tags: #history #arc-04

PR #15 is the third move in arc 04 and the only PR in this arc that bundles two concerns under one commit. Commit subject reads "fix: ^C → pane child + git markers don't leak across same-name files (v1.41.2)" (commit 5999261, 2026-05-04). Diff: 6 files, +125/-15. Source code: `src/sysinfo.rs` +75/-12 (the git-marker-leak half), `src/app/mod.rs` +5/-0 (the `^C`-route half).

The Phase-1 segmentation entry on `history-overview` (= 01KR0TWHTC1MPK4KJ08Y9SPE6P) flagged PR #15 as a hard cluster-boundary call. The framing entry above (= 01KR12T4DHGDH3B9YYXM0F093A) confirmed the disposition empirically: two independent fixes in different files, no shared root cause, treated as two phases under one entry rather than collapsed into one observation. The diff-weight order — git-marker leak first (87 lines including the 75-line refactor and 5 new unit tests, sysinfo.rs), `^C`-routing second (5 lines, app/mod.rs) — matches the order below. The commit subject's left-to-right order ("`^C` → pane child + git markers …") is the inverse; the title leads with the smaller half, the diff weight leads with the larger.

---

**Phase 1 — Git markers don't leak across same-name files (`src/sysinfo.rs:62-160`).**

The bug. A clean root-level file rendered with a `~` (modified) marker when a sibling-named file in a subdirectory was actually the dirty one. The PR's CHANGELOG names the case verbatim: "root `CLAUDE.md` clean, `content-acquisition/CLAUDE.md` modified → both rows showed `~`" (commit 5999261, 2026-05-04). The pre-fix `git_file_statuses` collapsed every porcelain entry to its basename and indexed the map by that basename, so the deep file's status overwrote the root row.

The fix shape. The PR extracts a pure-parser function `parse_porcelain_statuses(porcelain: &str, prefix: &str)` from the spawn-`git`-and-parse body. The parser body itself gains an `in_this_dir = (top_component == filename)` boolean that gates two distinct map writes. The CHANGELOG names the rule: "The basename now only goes into the map for files actually in the listing directory; deeper entries still mark the parent directory" (commit 5999261, 2026-05-04). In code (`src/sysinfo.rs:148-160` post-merge):

- A row that *is* in the listing directory writes `name → status` (basename-keyed).
- A row that *isn't* in the listing directory writes `top_component/ → Modified` (parent-directory-keyed) and skips the basename write entirely.

The extraction is the load-bearing diff move: spawning `git` was previously inlined with the parsing, and the parser could not be unit-tested without forking a subprocess. After the extraction the parser is pure, and the PR adds five unit tests that pin the rules:

1. `deep_modification_does_not_dirty_same_basename_at_root` — the regression case.
2. `root_modification_marks_basename` — the simple positive case.
3. `root_and_deep_same_basename_uses_root_status` — when both a root and a deep sibling exist, the root entry reflects the root status.
4. `prefix_strips_listing_dir` — entries outside the listing prefix are filtered out.
5. `rename_takes_new_name` — `R old.md -> new.md` keys under `new.md`.

Sequence-grain consequence. PR #27 (this arc, two days later) extends `parse_porcelain_statuses` from the flat-`GitFileStatus`-enum return type to a struct-with-staged/unstaged-halves return type. The extraction PR #15 ships here is what makes PR #27's refactor land cleanly — without the pure-parser shape, the staged-vs-unstaged decode would have to live inside the spawn-`git` body. Arc 04 reads as PR #15 setting up the table that PR #27 then dresses with new fields.

---

**Phase 2 — `^C` reaches the pane child (`src/app/mod.rs:2679-2693`).**

The bug. Pressing `^C` while the pane was focused tripped an existing footgun-flash ("`^C` is not a quit binding — use Q (or :q) to quit, Esc to cancel modes") instead of delivering `0x03` to the running child process. The CHANGELOG names the cause: "the '^C is not a quit binding' footgun-guard fired before the pane-forward path, so pressing `^C` while focused on a child process (zsh, a long-running command, etc.) flashed the hint instead of delivering `0x03` to the child" (commit 5999261, 2026-05-04).

The fix shape. A new `pane_has_focus = self.pane_tabs.is_some() && self.state.pane_focused` is computed before the existing footgun-guard, and the guard's predicate gains an `&& !pane_has_focus` clause. When the pane has focus, the guard skips and the dispatch falls through to the pane-forward path that forwards every other control code (`^T`, `^D`, …) already. The CHANGELOG names this asymmetry verbatim: "Other control codes (`^T`, `^D`, …) were already forwarded; only the `^C` case carried the extra guard" (commit 5999261, 2026-05-04). The fix narrows the guard's scope rather than removing it; outside-pane `^C` still triggers the footgun-flash.

Lineage check (orthogonal to PR #34). The `pane_has_focus` precondition reads `self.pane_tabs.is_some() && self.state.pane_focused` — a simple two-bit conjunction in the original list-vs-pane meaning of `pane_focused`. Arc 03's PR #34 (`fix/top-overlay-focus-switch`, 8e9fb2c, 2026-05-06; entry = 01KR10JBACRS3Z71WTHGBVCPJM) lands two days after this PR and extends `pane_focused` to also carry overlay-vs-pane meaning. PR #15 predates that extension and uses the original meaning only; the surfaces don't interact. Arc 03's seams-aside (= 01KR11TME2KF5QFQ45GJYG8MC7) names `pane_focused`'s post-PR-#34 three-meaning load and lists PR #6's zoom save-source axis, PR #34's overlay-vs-pane axis, and the original list-vs-pane meaning; PR #15 sits inside the third only.

---

**Drift findings flagged for the insight layer**:

- The commit subject orders the bundle "`^C` → pane child + git markers don't leak across same-name files." The `^C` fix is 5 lines; the git-marker fix is 87 lines including a 75-line refactor and 5 new unit tests. The subject's left-to-right order is the inverse of the diff weight. A reader scanning subjects only weights the bundle's halves equally; the diff does not.

- The two halves do not share a root cause. They share a PR. The git-marker fix touches `src/sysinfo.rs::git_file_statuses` (porcelain parsing in the file-statuses path); the `^C` fix touches `src/app/mod.rs::App::handle_key` (early-key footgun-guard in dispatch). No call-chain connects them. The bundle reads as one PR's worth of fixes-noticed-while-shipping rather than a co-located fix.

- The phase-1 refactor adds a pure-parser function and a unit-test surface that previously did not exist for porcelain parsing. The five tests pin behavior cases the original inlined body could not have been tested for without forking `git`. PR #27 (this arc, two days later) extends the parser further; the test surface scales accordingly (3 new tests in PR #27).

- The phase-2 fix narrows an existing footgun-guard rather than removing it. The asymmetry note in the CHANGELOG ("Other control codes were already forwarded; only the `^C` case carried the extra guard") signals that the guard was an *exception*, not the rule — the rest of the dispatch already handled the pane-forward case. The fix brings `^C` into alignment with the existing rule rather than introducing a new one.

Provenance:
- 5999261 (PR #15 fix/git-status-and-pane-ctrl-c, 2026-05-04 11:26) — full PR.
- `src/sysinfo.rs:62-160` (post-merge) — `git_file_statuses` and the new pure-parser `parse_porcelain_statuses`.
- `src/sysinfo.rs:349-403` (post-merge) — the five new unit tests pinning the basename / parent-directory rules.
- `src/app/mod.rs:2679-2693` (post-merge) — `pane_has_focus` precondition and the `&& !pane_has_focus` clause on the footgun-guard.
- `git diff 5999261^1..5999261^2 -- CHANGELOG.md` — verbatim quotes ("root `CLAUDE.md` clean, `content-acquisition/CLAUDE.md` modified → both rows showed `~`"; "The basename now only goes into the map for files actually in the listing directory"; "Other control codes (`^T`, `^D`, …) were already forwarded; only the `^C` case carried the extra guard"; "the '^C is not a quit binding' footgun-guard fired before the pane-forward path").
- 4e2afd9 (PR #27 feat/git-staged-vs-unstaged, 2026-05-06) — downstream extender of `parse_porcelain_statuses`; named here for sequence-grain forward reference.
- 8e9fb2c (PR #34 fix/top-overlay-focus-switch, 2026-05-06) — arc 03 lineage-check target; orthogonal.
- `history-overview` segmentation entry = 01KR0TWHTC1MPK4KJ08Y9SPE6P (cluster-boundary call source).
- `history-arc-03-pane-behavior` PR #34 entry = 01KR10JBACRS3Z71WTHGBVCPJM (overlay-focus model post-dating this PR; orthogonal).
- `history-arc-03-pane-behavior` seams-aside = 01KR11TME2KF5QFQ45GJYG8MC7 (`pane_focused`'s three-meaning load post-PR-#34; PR #15 uses original meaning only).
- `history-arc-04-git-integration` framing entry = 01KR12T4DHGDH3B9YYXM0F093A.
- `history-arc-04-git-integration` PR #1 entry = 01KR12W1M20SQW3QXT8VC09REK.
- `history-arc-04-git-integration` PR #7 entry = 01KR12XTG7E5TC0RNTJ65G67T7.

<!-- Entry-ID: 01KR130775Q4PKYEN6FE1743DJ -->

---
Entry: Claude Code (caleb) 2026-05-07T11:27:16.814123+00:00
Role: scribe
Type: Note
Title: PR #24 (feat/jump-git-change): ]g / [g vim-bracket family extension; not catalogue §1 (refutation)

Spec: scribe

tags: #history #arc-04

PR #24 is the fourth move in arc 04 and the navigation axis. Commit subject reads "feat: ]g / [g jump cursor to next/prev git-changed entry (v1.41.11)" (commit 762a0a6, 2026-05-05). Diff: 9 files, +179/-5. Source code: `src/app/state.rs` +106 (50 lines logic + 50 lines tests), `src/keymap/resolver.rs` +37, `src/keymap/action.rs` +7.

**The feature.** Two new chord pairs — `]g` (next) and `[g` (previous) — walk the current listing in either direction looking for a row whose `git_files` lookup returns a non-clean status, advancing the cursor to the first match and wrapping at the listing boundary. The PR's CHANGELOG names the muscle-memory anchor verbatim: "Vim-style 'next hunk' muscle memory for the file list" (commit 762a0a6, 2026-05-05). The wrap behavior is described in the same entry: "Wraps around end-of-list so the chord can be held without thinking about direction."

**The implementation.** Three files carry the change:

- `src/app/state.rs:200-240` (post-merge) — `AppState::jump_to_git_change(forward: bool) -> bool`. Pure-domain logic. Reuses the same `git_files` map the listing markers already consume. The walk uses `for n in 1..=len` so a press from a dirty row advances to the *next* dirty row rather than staying put (the doc-comment names this verbatim: "we never re-test the cursor's own row"). Returns `false` when the listing has no changes; the caller handles the empty case via `flash_info("no git changes in this directory")` (`src/app/state.rs:716-727` post-merge).

- `src/keymap/action.rs:140-149` (post-merge) — two new `Action` variants `JumpNextGitChange` and `JumpPrevGitChange`, with describe-text strings.

- `src/keymap/resolver.rs:31-40,222-241,342-353` (post-merge) — two new `PendingSeq` variants `NextBracket` and `PrevBracket`, the mid-sequence dispatch arm that handles `g` as the only sub-command, and the top-level `[` / `]` dispatch that sets the pending state. The resolver code carries an inline comment that names the existing pattern: "Bracket pairs are reserved for 'next/prev <thing>' jumps, mirroring the [t/]t and [b/]b chords in the pager" (`src/keymap/resolver.rs:344-346` post-merge).

**Test surface.** Five unit tests in `src/app/state.rs:2358-2410` (post-merge) pin the cases: skip-clean-rows, wrap-forward, wrap-backward, advance-off-the-current-dirty-row, returns-false-when-no-changes. Pure-domain testing follows the same shape PR #15's `parse_porcelain_statuses` tests use — no PTY, no UI, no git subprocess.

**Refutation against catalogue §1 (brief-flagged hypothesis, refuted).** The arc-04 framing entry (= 01KR12T4DHGDH3B9YYXM0F093A) named the brief's hypothesis that PR #24 executes against arc 02's lazygit-ux-catalogue §1. Verification against the catalogue text preserved verbatim in arc 02's investigation entry (= 01KR0YXXZRQR24CSNAK4Q7808T):

§1 ("Numbered panels & direct-jump") catalogues lazygit's `1`..`5` panel-jumping. The catalogue's recommendation is verbatim **skip**: "spyc has exactly two top-level surfaces (list, pane) where lazygit has five, so `1` and `2` would be wasted on a binding that `^W j`/`^W k` already covers cleanly." The catalogue's §1 is structurally about *cross-window* surface jumping — `1` jumps to a panel, `2` jumps to a different panel. PR #24's `]g`/`[g` is structurally about *within-window* row stepping — it never leaves the listing surface, it steps between rows of the same type with a non-clean git status. Different idiom, different inspiration.

The PR #24 resolver code names the actual inspiration explicitly. The inline comment at `src/keymap/resolver.rs:344-346` (post-merge) reads "mirroring the [t/]t and [b/]b chords in the pager." Those chords are pre-existing spyc / pager-internal vim-bracket family bindings, themselves modeled on vim's `]c`/`[c` (next/previous diff hunk) family. PR #24 reads as a continuation of that family, not as catalogue execution.

The empirical position: PR #24 does not execute against any catalogue section. Arc 02's published back-reference table (= 01KR0Z3673Z27FJ4GV92FYV4QJ) does not enumerate PR #24, consistent with this finding. The brief's hypothesis is refuted against the catalogue text and the diff.

**Drift findings flagged for the insight layer**:

- The PR's title and CHANGELOG both reach for "vim-style 'next hunk' muscle memory" as the anchor. The catalogue dispositions for vim-derived idioms vs lazygit-derived idioms read differently across arc 02's seven sections — vim parallels are accepted as native to spyc's design language, lazygit borrows require explicit borrow/adapt/skip framing. PR #24 lands as a vim-bracket family extension, not as a borrow.

- The walk algorithm's wrap behavior is opinionated. The CHANGELOG names the rationale: "Wraps around end-of-list so the chord can be held without thinking about direction" (commit 762a0a6, 2026-05-05). The resolver-side state machine treats `[` and `]` as two-key chords with `g` as the only sub-command; the design leaves room for additional sub-commands (`]m`/`[m` for marks, `]p`/`[p` for picks) without changing the resolver shape. None of those land in the 22-day window.

- The five unit tests deliberately pin one behavior the title/CHANGELOG do not name: pressing `]g` from a dirty row advances to the *next* dirty row rather than staying put. The doc-comment makes the rule explicit; the test (`jump_advances_off_the_current_dirty_row`) pins it. A reader scanning the title alone might assume the cursor stays on the first dirty row found in either direction — which would include the cursor's own row. The implementation is one off-by-one decision against that reading, and the test exists to keep it pinned.

Provenance:
- 762a0a6 (PR #24 feat/jump-git-change, 2026-05-05 16:26) — full PR.
- `src/app/state.rs:200-240` (post-merge) — `jump_to_git_change` body; doc-comment quoted ("we never re-test the cursor's own row").
- `src/app/state.rs:716-727` (post-merge) — Action dispatch arms; flash-empty case.
- `src/app/state.rs:2358-2410` (post-merge) — five unit tests.
- `src/keymap/action.rs:140-149` (post-merge) — `JumpNextGitChange` / `JumpPrevGitChange` Action variants.
- `src/keymap/resolver.rs:31-40,222-241,342-353` (post-merge) — `PendingSeq::NextBracket` / `PrevBracket`, mid-sequence arm, top-level `[` / `]` dispatch; inline comment quoted ("mirroring the [t/]t and [b/]b chords in the pager").
- `git diff 762a0a6^1..762a0a6^2 -- CHANGELOG.md` — verbatim quotes ("Vim-style 'next hunk' muscle memory for the file list"; "Wraps around end-of-list so the chord can be held without thinking about direction"; "Reuses the same `git_files` map the listing markers consume").
- `git show 0691666:notes/lazygit-ux-catalogue.md` §1 — verbatim disposition "skip" and the rationale ("`^W j`/`^W k` already covers cleanly"). Catalogue text preserved in arc 02 investigation entry.
- `history-arc-02-lazygit-investigation-and-harvest` investigation entry = 01KR0YXXZRQR24CSNAK4Q7808T (catalogue text source for refutation).
- `history-arc-02-lazygit-investigation-and-harvest` closure entry = 01KR0Z3673Z27FJ4GV92FYV4QJ (back-reference table; PR #24 not enumerated, consistent with refutation).
- `history-arc-04-git-integration` framing entry = 01KR12T4DHGDH3B9YYXM0F093A (refutation hypothesis source).
- `history-arc-04-git-integration` PR #15 entry = 01KR130775Q4PKYEN6FE1743DJ (`parse_porcelain_statuses` test-surface precedent that this PR's pure-domain test shape inherits).

<!-- Entry-ID: 01KR1327VZTQAYNNPMBCTC3SSM -->

---
Entry: Claude Code (caleb) 2026-05-07T11:28:38.053925+00:00
Role: scribe
Type: Note
Title: PR #27 (feat/git-staged-vs-unstaged): GitFileStatus from enum to struct, two-cell XY display; not catalogue (refutation)

Spec: scribe

tags: #history #arc-04

PR #27 is the fifth and final move in arc 04 and the marker-fidelity axis. Commit subject reads "feat: two-char git markers for staged-vs-unstaged distinction (v1.41.14)" (commit 4e2afd9, 2026-05-06). Diff: 9 files, +272/-78 — the largest src diff in the arc. Source code: `src/ui/list_view.rs` +174/-46, `src/sysinfo.rs` +116/-28, `src/app/state.rs` +13/-13, plus a 1-line touch in `src/app/mod.rs`. Three new parser tests in `src/sysinfo.rs`.

**The feature.** The left gutter on each listing row now shows the full porcelain XY pair (column 0 = staged side, column 1 = unstaged side), mirroring `git status -s`. The PR's CHANGELOG names the cases verbatim: "`M ` is staged-only, ` M` is unstaged-only, `MM` is partially staged + further edits, `R~` is staged rename + further unstaged edits, ` ?` is untracked. Each char carries its own color so the staged/unstaged halves are independently legible at a glance" (commit 4e2afd9, 2026-05-06). The marker column was already 2 cells wide pre-PR (a single glyph plus a trailing space); the layout does not shift.

**The refactor.** The structural move is a type promotion. Pre-PR, `GitFileStatus` is a flat enum with variants `Clean`, `Modified`, `Added`, `Deleted`, `Renamed`, `Untracked`, `Conflicted` — one row of state per file. Post-PR, `GitFileStatus` is a struct (`src/ui/list_view.rs`):

- `staged: Option<GitChange>` — the index/staged half (porcelain X column).
- `unstaged: Option<GitChange>` — the working-tree half (porcelain Y column).
- `untracked: bool` — the special-case `??` row (orthogonal to staged/unstaged).

A new `GitChange` enum carries the per-side kind (`Modified`, `Added`, `Deleted`, `Renamed`, `Conflicted`). The CHANGELOG names the design choice verbatim: "Internally `GitFileStatus` is now a struct (`staged: Option<GitChange>`, `unstaged:`, `untracked: bool`) instead of a flat enum; new `GitChange` carries the per-side kind" (commit 4e2afd9, 2026-05-06).

**The parser change.** PR #15's `parse_porcelain_statuses` (= 01KR130775Q4PKYEN6FE1743DJ) is the extension point. Pre-PR-#27, the parser returned the flat-enum status by collapsing `xy` to a single variant via a chain of pattern guards (`s.starts_with('R') || s.ends_with('R') => Renamed`). Post-PR-#27, the parser decodes each half independently via a new `decode_half(c: char) -> Option<GitChange>` helper and constructs the struct from the two halves separately:

```
Some(GitChange::Modified) for 'M'|'T',
Some(GitChange::Added) for 'A',
Some(GitChange::Deleted) for 'D',
Some(GitChange::Renamed) for 'R'|'C',
Some(GitChange::Conflicted) for 'U',
None for ' ' (and ?, !)
```
(`src/sysinfo.rs:107-117` post-merge.)

The conflict shapes (`UU`, `DD`, `AA`) bypass the per-half decode and write `Conflicted` to both halves directly, so the marker reads `!!` and stands out — the CHANGELOG does not name this; the code comment does (`src/sysinfo.rs:152-156` post-merge): "Conflicts (`UU`, `DD`, `AA`) collapse to Conflicted on both halves so the marker reads `!!` and stands out."

**Test surface.** Three new parser tests in `src/sysinfo.rs:425-461` (post-merge): `staged_only_modify` (`M  foo.rs`), `partially_staged_modify` (`MM foo.rs` — both halves set), `conflict_marks_both_halves` (`UU foo.rs` — both halves Conflicted). The five tests PR #15 introduced are all updated in-place to read the new struct-shape getters (`s.unstaged.is_some()`, `s.staged == Some(GitChange::Renamed)`) instead of comparing flat-enum equality.

**Consumer-side updates.** PR #24's `jump_to_git_change` consumed the flat-enum return type via `!= GitFileStatus::Clean`. Post-PR-#27 it consults a new `is_clean()` accessor: `self.git_files.get(&r.display).copied().is_some_and(|s| !s.is_clean())` (`src/app/state.rs:215-220` post-merge). The `=git`/`=g` filter from PR #7 gets the same mechanical update at `src/app/state.rs:554-560` (post-merge). Two arc-04 consumers touched, both via the same `is_clean()` interface; neither's behavior changes.

**Refutation against the catalogue (brief-flagged hypothesis, refuted).** The arc-04 framing entry (= 01KR12T4DHGDH3B9YYXM0F093A) named the brief's hypothesis that PR #27 executes against arc 02's lazygit-ux-catalogue. Verification against the catalogue text preserved verbatim in arc 02's investigation entry (= 01KR0YXXZRQR24CSNAK4Q7808T):

The seven catalogue sections cover panel-jump (§1, skip), context-sensitive footer (§2, adapt — the prompt-row hint), command log + random tip (§3, skip-the-log/adapt-the-tip), popups/pickers (§4, adapt — extend the pager), scoped help (§5, adapt), single-key row-verbs (§6, skip), two-letter chord jumps (§7, skip). All seven are about UI/affordance patterns. None catalogue git-data fidelity at the marker-display level. The catalogue's leading framing acknowledges the divergence with lazygit on the mouse axis; it does not catalogue git's own porcelain XY pair display.

The display choice in PR #27 — a two-cell XY pair, each half independently colored, mirroring `git status -s` — reads structurally similar to git's *own* shell-side output, not to lazygit's two-panel staged/unstaged split. lazygit's idiom (per arc 02 investigation entry's catalogue read of `lazygit-upstream/`) is a separate "Files" panel showing the porcelain pair as text rows; PR #27 inlines the pair into the existing listing's marker column.

The empirical position: PR #27 does not execute against any catalogue section. Arc 02's published back-reference table (= 01KR0Z3673Z27FJ4GV92FYV4QJ) does not enumerate PR #27, consistent with this finding. The brief's hypothesis is refuted against the catalogue text and the diff.

**Drift findings flagged for the insight layer**:

- The CHANGELOG entry sits under `### Changed`, not `### Added` or `### Fixed`. The bucket choice tracks the diff: existing functionality (the marker column) gets a richer rendering, no new key bindings, no new commands. The rendering change is wide (174 lines in `list_view.rs`) but additive only at the type-shape level; user-facing surface is the same column, fuller content.

- PR #15's `parse_porcelain_statuses` extraction (= 01KR130775Q4PKYEN6FE1743DJ) two days earlier is what makes this PR's parser refactor land cleanly. Without the pure-parser shape, the per-half `decode_half` helper would have to live inside the spawn-`git`-and-parse body, and the three new struct-shape tests could not be written without forking a subprocess. The extension PR #27 ships against PR #15's table is structural, not just semantic — the table is what lets the new fields land.

- The directory-marking rule from PR #15 (`top_component/ → Modified`) is preserved structurally but updated semantically. Pre-PR-#27, the directory-keyed entry stored a flat `Modified`. Post-PR-#27, it stores `GitFileStatus::unstaged(GitChange::Modified)` (a helper constructor on the new struct, `src/sysinfo.rs:188-192` post-merge). The CHANGELOG does not name this; the code comment does ("directories don't have a meaningful per-half staging concept"). The choice keeps PR #7's `=git` filter and PR #24's `]g`/`[g` jumper both operating on subtree-marker rows without surface change.

- The CHANGELOG names "3 new parser tests cover the staged-only / partially-staged / conflict shapes" — but the diff also rewrites all five PR #15 tests to read the new struct-shape getters. Test-surface count goes from 5 to 8; test-rewrite cost is the load-bearing-but-unnamed half of PR #27's test work.

Provenance:
- 4e2afd9 (PR #27 feat/git-staged-vs-unstaged, 2026-05-06 16:51) — full PR. Last commit in arc 04.
- 5999261 (PR #15 fix/git-status-and-pane-ctrl-c, 2026-05-04) — `parse_porcelain_statuses` extraction this PR extends; named for sequence-grain dependency.
- 762a0a6 (PR #24 feat/jump-git-change, 2026-05-05) — `jump_to_git_change` consumer this PR updates via `is_clean()`.
- f3ddaf2 (PR #7 feat/limit-git, 2026-05-02) — `=git` filter consumer this PR updates via `is_clean()`.
- `src/sysinfo.rs:107-117` (post-merge) — `decode_half` helper.
- `src/sysinfo.rs:148-192` (post-merge) — struct-construction in `parse_porcelain_statuses`; conflict-collapse path; directory-marker `unstaged(Modified)` shape.
- `src/sysinfo.rs:425-461` (post-merge) — three new parser tests (`staged_only_modify`, `partially_staged_modify`, `conflict_marks_both_halves`).
- `src/app/state.rs:215-220,554-560` (post-merge) — `is_clean()` updates at the two arc-04 consumer sites.
- `git diff 4e2afd9^1..4e2afd9^2 -- CHANGELOG.md` — verbatim quotes ("`M ` is staged-only, ` M` is unstaged-only, `MM` is partially staged + further edits, `R~` is staged rename + further unstaged edits, ` ?` is untracked"; "Each char carries its own color so the staged/unstaged halves are independently legible at a glance"; "Marker column was already 2 cells wide (was `~` + space) — no layout shift"; "Internally `GitFileStatus` is now a struct").
- `git show 0691666:notes/lazygit-ux-catalogue.md` (read for empirical refutation) — none of §1–§7 catalogues git-data fidelity at the marker-display level.
- `history-arc-02-lazygit-investigation-and-harvest` investigation entry = 01KR0YXXZRQR24CSNAK4Q7808T (catalogue text source for refutation; lazygit-upstream Files-panel idiom).
- `history-arc-02-lazygit-investigation-and-harvest` closure entry = 01KR0Z3673Z27FJ4GV92FYV4QJ (back-reference table; PR #27 not enumerated, consistent with refutation).
- `history-arc-04-git-integration` framing entry = 01KR12T4DHGDH3B9YYXM0F093A.
- `history-arc-04-git-integration` PR #1 entry = 01KR12W1M20SQW3QXT8VC09REK.
- `history-arc-04-git-integration` PR #7 entry = 01KR12XTG7E5TC0RNTJ65G67T7.
- `history-arc-04-git-integration` PR #15 entry = 01KR130775Q4PKYEN6FE1743DJ (`parse_porcelain_statuses` extension target).
- `history-arc-04-git-integration` PR #24 entry = 01KR1327VZTQAYNNPMBCTC3SSM (`jump_to_git_change` consumer).

<!-- Entry-ID: 01KR134PZSQDAFVJK3M35FTKXF -->

---
Entry: Claude Code (caleb) 2026-05-07T11:30:11.292087+00:00
Role: scribe
Type: Note
Title: Closure: arc 04 baseline complete; two empirical refutations published; arc 05 (pager-surface) follows

Spec: scribe

tags: #history #arc-04

Arc 04 baseline narration complete. Five PRs cover five facets of git-awareness — marker reliability (PR #1), filtering (PR #7), marker correctness + a coincident pane-control fix (PR #15), navigation (PR #24), marker fidelity (PR #27) — across the longest span of any arc (Day 0 to Day 21). The arc reads as capability accretion along independent axes, not as a recurring concern. PR #1's 1Hz machinery becomes the consistency rail PR #7's filter and PR #24's jumper both quietly reuse; PR #15's `parse_porcelain_statuses` extraction becomes the table PR #27's struct refactor extends. PR #15 is the bundle: two coincident concerns under one PR, treated within one entry as two phases.

**Sequence-grain summary**:

- PR #1 (Day 0, 17:08) — first user-facing fix of the project, ~18 min after PR #2's CI hygiene. v1.37.1 cut.
- PR #7 (Day 2, 11:53) — `=git`/`=g` filter; reuses PR #1's machinery. BUGS.md fork-out: harpoon half deferred to arc 06's PR #8 four hours later. v1.38.1 cut.
- PR #15 (Day 4, 11:26) — bundle: ^C-route guard (5 lines, app/mod.rs) + parser extraction (87 lines, sysinfo.rs). v1.41.2 cut.
- PR #24 (Day 5, 16:26) — `]g`/`[g` jumper; pure-domain on AppState. v1.41.11 cut.
- PR #27 (Day 21*, 16:51) — type promotion from flat enum to struct; extends PR #15's parser, updates PR #7 and PR #24 consumers via new `is_clean()`. v1.41.14 cut.

(* Day 21 by date arithmetic from 2026-04-30 to 2026-05-21? — correction: 2026-05-06 is Day 6. The arc spans Day 0 to Day 6 inclusive, not Day 21. The framing entry's claim of "the full 22-day window" reflects the *positional* placement of the first and last arc-04 PRs in a window that ends with PR #37 on Day 7 (2026-05-07). PR #27 is not the last PR of the window — PR #28 through PR #37 land after it on Day 6 and Day 7. The framing entry's "longest span of any arc" is correct in the relative sense: arc 04 spans Day 0 to Day 6 (inclusive), seven calendar days, longest of any arc per the segmentation entry's own date-anchors. The "full 22-day window" phrase is an overstatement of the *absolute* span and is corrected here for the durable record; the relative observation — longest arc span — stands.)

**Cadence shape (precedent observation, not prescription).** Option A inherits cleanly. Five PRs → five per-PR entries plus framing and closure → seven head entries. PR #15's within-entry phase split is the cadence variation arc 04 commits to; arcs 05–08 may inherit, ignore, or break with their own stated reasons. Arc 03's per-entry shape variety carries forward — the five per-PR entries below register differently (feature-compact for PR #1 and PR #7; bundle-with-phase-split for PR #15; feature-with-refutation for PR #24 and PR #27). Pattern-8's reference register holds the framing entry; the per-PR entries draw from feature-shaped, refactor-shaped, and bundle-shaped registers as their diffs ask.

**Empirical refutations published**:

- **PR #24 does not execute against catalogue §1.** Catalogue §1 (Numbered panels & direct-jump) is **skip** verbatim per arc 02's investigation entry; spyc has effectively two top-level surfaces and `^W j`/`^W k` already covers them. PR #24's `]g`/`[g` is a vim-bracket family extension, mirroring spyc's pre-existing `[t/]t` and `[b/]b` pager chords, themselves modeled on vim's `]c`/`[c` next/previous-hunk family. The PR #24 resolver code names the inheritance explicitly. (Full refutation in PR #24 entry = 01KR1327VZTQAYNNPMBCTC3SSM.)

- **PR #27 does not execute against any catalogue section.** All seven catalogue sections cover UI/affordance patterns (panel routing, footer, popups, scoped help, row verbs, chord jumps); none catalogue git-data fidelity at the marker-display level. PR #27's two-cell XY pair display reads structurally similar to `git status -s`'s native rendering, not to lazygit's two-panel staged/unstaged split. (Full refutation in PR #27 entry = 01KR134PZSQDAFVJK3M35FTKXF.)

The empirical position: arc 04 carries no mandatory back-references to arc 02. Arc 04 cites arc 02 only in service of these two refutations — to walk the catalogue text that PR #12 deleted from `notes/` and that survives only inline in arc 02's investigation entry.

**Cross-arc continuity notes from arc 04 to arc 05 (forward) and arc 06 (sideways)**:

- Arc 05 (`history-arc-05-pager-surface`, 8 PRs, 2026-05-02 to 2026-05-07; the largest arc by PR count) follows next. No direct dependency from arc 04 → arc 05 surfaces in the diffs. Arc 05's PR #20 (`feat/scroll-altscreen-hint`, ee07307) carries an arc-02 catalogue §2 back-reference per the spine's PR #5 special-handling entry (= 01KR0TYF5F11DA8P5HNPA20DBK); arc 04 makes no claim against arc 05's PR list.

- Arc 06 (`history-arc-06-input-and-overlays`, 4 PRs) inherits the harpoon design space from PR #7's BUGS.md fork. PR #8 (`feat/harpoon`, 62fc129, 2026-05-02 18:04) lands four hours after PR #7's merge and ships the "currently working on" pinned-set half PR #7 deferred. Arc 06's eventual PR #8 entry should back-reference both PR #7 entry (= 01KR12XTG7E5TC0RNTJ65G67T7) for the BUGS.md fork-out and arc 02 investigation entry (= 01KR0YXXZRQR24CSNAK4Q7808T) for catalogue §4 picker-pattern parallelism.

**Voice contract precedent observations (not new precedent — inheritance from arcs 01/02/03)**:

- Verbatim commit-subject quoting honored on every per-PR entry, attributed `(commit <sha>, <date>)`.
- No mindset attribution to Derek anywhere in head entries. No first-person "I/we" in heads.
- Sequence-over-timing privileged. The "first move," "next move," "extends the table" register from arc 01 carries forward; clock-padding language avoided.
- Hedge-token whitelist used freely from the brief's list ("appears to," "reads as," "consistent with," "the diff shape suggests," "points toward," "aligns with," "the commit message indicates").
- Banned mindset words audited: no "wants," "thinks," "believes," "decided," "feels," "intends to" (without "the commit message"), "is concerned that" referencing Derek's mindset.

**Arc 04 done-definition self-check**:

- ☑ `watercooler_health` reported Healthy against the spyc code_path at session start.
- ☑ Required reads completed: history-overview, history-arc-01, history-arc-02 (full investigation entry verbatim), history-arc-03 (story-tail and seams-aside verbatim, full PR #34 and PR #29 references), `notes/lazygit-ux-catalogue.md` at commit 0691666, `onboarding-architecture` entry 0, `onboarding-overview` entry 0.
- ☑ Diagnosis stated in framing entry: capability accretion across five axes, pattern-8 register for framing, per-entry shape variety inherits from arc 03.
- ☑ Cadence shape stated in framing: option A per-PR with PR #15's within-entry phase split.
- ☑ Thread `history-arc-04-git-integration` exists with framing + 5 per-PR entries + this closure (7 head entries). Tail(s) follow.
- ☑ PR #15 entry addresses the bundle explicitly: within-entry phase split (basename-collision phase + ^C-route phase), with the empirical no-shared-root-cause finding stated.
- ☑ PR #24 and PR #27 entries verify against arc 02's investigation entry for ux-catalogue items they were hypothesized to execute against — finding: neither executes against any catalogue section. Refutations published with arc-02 entry IDs cited.
- ☑ Each head entry follows head voice contract (third-person observational, present tense, sequence-faithful, verbatim commit-subject quoting attributed).
- ☑ Provenance blocks on every entry.
- ☑ Tags `#history` and `#arc-04` on every entry.
- ☑ No insight-layer claims. Drift findings flagged, not interpreted. No emergent-property naming. No motive attribution. No forward predictions.
- ☐ Thread to be left OPEN after tail(s).

This thread will remain OPEN for cross-arc references after the tail(s) are written.

Provenance:
- cd8df2e (PR #1, 2026-04-30 17:08) — first commit in arc 04.
- f3ddaf2 (PR #7, 2026-05-02 11:53).
- 5999261 (PR #15, 2026-05-04 11:26) — cluster-boundary call; bundle.
- 762a0a6 (PR #24, 2026-05-05 16:26).
- 4e2afd9 (PR #27, 2026-05-06 16:51) — last commit in arc 04 by wall-clock.
- 62fc129 (PR #8 feat/harpoon, 2026-05-02 18:04) — arc 06 destination for PR #7's BUGS.md fork-out; named here for sideways reference.
- ee07307 (PR #20 feat/scroll-altscreen-hint, 2026-05-05) — arc 05 carrier of catalogue §2 back-reference; named here for arc-05 forward-reference.
- `history-overview` framing entry = 01KR0TRFWT9W6WMFHC49YSW0BG.
- `history-overview` segmentation entry = 01KR0TWHTC1MPK4KJ08Y9SPE6P.
- `history-overview` PR #5 special-handling entry = 01KR0TYF5F11DA8P5HNPA20DBK.
- `history-overview` closure entry = 01KR0V01TAJVSZFE5ZNMCZHQSF.
- `history-arc-02-lazygit-investigation-and-harvest` investigation entry = 01KR0YXXZRQR24CSNAK4Q7808T.
- `history-arc-02-lazygit-investigation-and-harvest` closure entry = 01KR0Z3673Z27FJ4GV92FYV4QJ.
- `history-arc-04-git-integration` framing entry = 01KR12T4DHGDH3B9YYXM0F093A.
- `history-arc-04-git-integration` PR #1 entry = 01KR12W1M20SQW3QXT8VC09REK.
- `history-arc-04-git-integration` PR #7 entry = 01KR12XTG7E5TC0RNTJ65G67T7.
- `history-arc-04-git-integration` PR #15 entry = 01KR130775Q4PKYEN6FE1743DJ.
- `history-arc-04-git-integration` PR #24 entry = 01KR1327VZTQAYNNPMBCTC3SSM.
- `history-arc-04-git-integration` PR #27 entry = 01KR134PZSQDAFVJK3M35FTKXF.

<!-- Entry-ID: 01KR137J5GESFYP7CN6KCD7MAS -->

---
Entry: Claude Code (caleb) 2026-05-07T11:32:55.085419+00:00
Role: scribe
Type: Note
Title: Tail: arc 04 told as a story — the machinery underneath capability accretion, and the lazygit influence that wasn't

Spec: scribe

tags: #history #arc-04 #tail

Arc 04 isn't shaped like arc 03. Arc 03's tail sits one thread away in this same review pass, and the easy frame would be to read this arc by analogy — look for a recurring concern, find five PRs returning to the same surface under new conditions, and call that the structure. The recurring-concern reading doesn't fit. None of the five PRs supersedes another. None returns to a surface a previous PR already touched and broadens it. PR #27 extends PR #15's parser, but the extension is additive — the parser keeps doing the same job, with a richer return type. There is no within-arc supersession. What arc 04 has instead is five distinct facets of git-awareness, each shipping its own axis: a marker-reliability backstop (PR #1), a filter (PR #7), a marker-correctness fix bundled with a `^C`-route guard (PR #15), a bracket-family jumper (PR #24), and a marker-fidelity refactor (PR #27).

The first thing worth noticing is the wall-clock arithmetic at the start of the window. PR #1 lands eighteen minutes after PR #2's CI-hygiene merge, which means the very first user-facing fix of the project came eighteen minutes after the CI-hygiene infrastructure went green. Whatever git-marker bug triggered PR #1 was already real on Day 0 — the FSEvents soft spot for inode replacement that the CHANGELOG names, hitting on every `git commit`. The 1Hz poll is a backstop, not a fix to the underlying watcher logic; the watcher path is preserved unchanged. That's what "hardening before features" looks like with an obvious feature backlog already accumulating: the first thing that ships is a 37-line backstop with a diff-aware return that protects the 0-dps-idle target. Nothing fancy. Nothing that announces itself.

The thing worth slowing down for is the machinery chain across the arc that nothing in the commits names. PR #1 builds a 1Hz git poll on `AppState`. PR #7's CHANGELOG names that machinery directly — "the filter stays live as the 1Hz git poll updates `git_files`" — and reuses it. PR #24's `]g`/`[g` jumper consumes the same `git_files` map; the CHANGELOG names the consistency claim ("Reuses the same `git_files` map the listing markers consume, so detection is consistent with what the user sees"). And then PR #15's `parse_porcelain_statuses` — extracted as a pure-parser function with five unit tests — is what makes PR #27's struct refactor land cleanly two days later. PR #27 doesn't just extend the parser; it could only have extended the parser cheaply *because* PR #15 made the parser pure. The five PR #15 tests get rewritten in-place against the new struct getters, and three new tests land for the staged-only / partially-staged / conflict shapes. None of the commits says "this enables that." But the chain is real, and it's what lets the arc read as additive rather than thrashing. The implicit chain is the structural fact you'd miss if you only read commit subjects.

PR #15 is the bundle, and the way to read it is that the title leads with the smaller half. The commit subject is "fix: ^C → pane child + git markers don't leak across same-name files" — `^C` first, git-markers second. The diff weight inverts that: 5 lines for the `^C` guard in `app/mod.rs`, 87 lines for the parser refactor in `sysinfo.rs` (including the 75-line refactor and 5 unit tests). The two halves don't share a root cause, don't share a call chain, don't share files. They share a PR. The five-minute check on whether the `^C`-route guard touches the same `pane_focused` axis arc 03's PR #34 generalizes a couple of days later comes back orthogonal — PR #15 uses `pane_focused` in its original list-vs-pane meaning only and predates the overlay-vs-pane axis PR #34 introduces. The bundle reads as one PR's worth of fixes-noticed-while-shipping, not as a co-located fix.

The two refutations are worth flagging because they're load-bearing for what the arc *is*. The brief proposed that PR #24 and PR #27 are likely lazygit-roadmap executions per arc 02's hub disposition. They aren't. PR #24's `]g`/`[g` is vim's hunk-nav idiom (`]c`/`[c` for hunks), already extant in spyc as `[t/]t` and `[b/]b` in the pager — the resolver code in PR #24 names the inheritance verbatim ("mirroring the [t/]t and [b/]b chords in the pager"). The catalogue's only "jump" item is §1 (Numbered panels & direct-jump), which the catalogue ranks as **skip** for spyc's two-surface layout because `^W j`/`^W k` already covers it. PR #27's two-cell XY display mirrors `git status -s`'s native output, not lazygit's two-panel staged/unstaged split; no catalogue section covers git-data fidelity at the marker level at all. What this means structurally is that the arc 02 → arc 04 back-reference network the brief anticipated doesn't exist, and not because arc 04 ignored arc 02 — because the arc 02 catalogue dispositions don't reach into git-data fidelity or vim-bracket nav. Arc 04's lazygit influence, if any, is at the level of "git is a first-class surface for spyc users" — which the catalogue framing does name in its leading tension paragraph — but not at the level of borrow/adapt/skip executions. The brief's hypothesis was reasonable on its face, and the catalogue check is what disconfirmed it. That's exactly what a catalogue is for.

There's a small sub-story inside PR #7 worth pulling out separately because it's a fork-out point that the arc 04 narration doesn't fully close. PR #7's BUGS.md edit splits a single user request — "files being worked on" — into two halves. The simple half (`=git`/`=g` filter) ships in PR #7. The more ambitious half (harpoon-style pinned set, persistent across sessions, with quick numeric jumps) gets rewritten as a deferred design pass in BUGS.md, where the rewritten text reads "Distinct from picks (per-dir, ephemeral), marks (single-file pointer per letter), inventory (yank stash). Needs a real design pass — design space overlaps existing concepts." Four hours later, in arc 06's PR #8, the harpoon ships. Arc 04 is the genesis of the simple half; arc 06 will narrate what landed when the deferred design pass came due. The CHANGELOG announces the split; it doesn't tell you the destination, and the chronology — four hours from PR #7 merge to PR #8 merge — suggests the deferred design pass was less "deferred" than the BUGS.md text framed it. Arc 06 will be the place to read that.

What the arc's shape suggests for whoever lands next on this surface — and this is observation, not prediction — is that the structural seams to watch are the implicit machinery chain (`git_files` is now the consumer surface for the listing markers, the `=git`/`=g` filter, the `]g`/`[g` jumper, and the post-PR-#27 staged/unstaged display) and the parser-rule asymmetry (PR #15's `in_this_dir` rule for basename-vs-parent-marker is load-bearing for the basename-keyed map's correctness — a future feature that wants to mark files by full path instead of basename will have to reason carefully about that rule, since the basename-collision regression PR #15 fixed is exactly the failure mode such a feature would re-introduce). Neither of those is in the commit messages. They live in the diff and the doc-comments and, now, here.

Provenance:
- No new commit references; this entry reflects on the head entries which carry full SHA provenance.
- `history-arc-04-git-integration` head entries 0–6 = 01KR12T4DHGDH3B9YYXM0F093A (framing), 01KR12W1M20SQW3QXT8VC09REK (PR #1), 01KR12XTG7E5TC0RNTJ65G67T7 (PR #7), 01KR130775Q4PKYEN6FE1743DJ (PR #15), 01KR1327VZTQAYNNPMBCTC3SSM (PR #24), 01KR134PZSQDAFVJK3M35FTKXF (PR #27), 01KR137J5GESFYP7CN6KCD7MAS (closure).
- `history-arc-02-lazygit-investigation-and-harvest` investigation entry = 01KR0YXXZRQR24CSNAK4Q7808T (catalogue text source for the refutation paragraph).
- `history-arc-03-pane-behavior` PR #34 entry = 01KR10JBACRS3Z71WTHGBVCPJM (overlay-focus model orthogonality check for PR #15's `pane_focused` use).
- `history-arc-03-pane-behavior` story-tail = 01KR11S8RG29J98QKN1H0VAA6W (arc 03's recurring-concern shape; this tail's contrasting observation).
- `history-arc-01-foundation-hygiene` PR #2 entry = 01KR0W81XE4K3G7BBSP42GE1HH (Day-0 wall-clock arithmetic anchor).

<!-- Entry-ID: 01KR13CJ5XS5VREYA4741JHDSQ -->
