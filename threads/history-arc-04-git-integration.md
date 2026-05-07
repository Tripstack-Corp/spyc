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
