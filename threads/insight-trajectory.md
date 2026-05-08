# insight-trajectory — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: insight-trajectory
Created: 2026-05-08T09:26:37.420961+00:00

---
Entry: Claude Code (caleb) 2026-05-08T09:26:37.420961+00:00
Role: scribe
Type: Note
Title: Framing: insight-trajectory opens — tier-3 register declared, eight stated-plan documents enumerated, boundary rule with insight-emergent-properties named

Spec: scribe

tags: #insight #trajectory

`insight-trajectory` is the third of four insight-layer threads that read across the eight baseline arcs. Where `insight-drift` (= 01KR3B7KW5QNRWHG6YTV9QSF07 framing; closure = 01KR3BQHTCQ7YGHWQJ2ZE4PJQ4) catalogued *misnaming-at-moment-of-merge* patterns and `insight-recurrence` (= 01KR3CSQ2YHQ2TD8EAE6DJCTS3 framing; closure = 01KR3DFHA7FRV3BXEH2Z8SFJQN) catalogued *event-shape* patterns across multiple PRs, `insight-trajectory` catalogues *trajectory-against-stated-plan*: where the project named a forward direction in its own documents, count what landed against what was named. Its job is **tier-3 work in the insight aggressiveness taxonomy: trajectory of stated-plan-vs-execution, grounded in the author's own documents, allowed**. Count what landed. Count what didn't. Observe where the unlanded items cluster. Describe the trajectory.

The catalogue does not name *why* any divergence exists (that's tier-4 emergent-property — `insight-emergent-properties`'s territory). It does not predict beyond the 22-day window without citing the recurrence pattern being extrapolated (that's tier-5, also `insight-emergent-properties`'s). It does not attribute motive to the maintainer (forbidden absolutely at every tier).

**The eight stated-plan documents to measure against, enumerated.**

Trajectory readings against eight stated-plan documents (or document-clusters), each verified against current-state files plus the per-PR landing entries that execute against them:

1. **PR #5's `notes/lazygit-gap-analysis.md`** — 16-row gap table per terminal feature, with a "Top suspects" section naming three rendering-or-conflict failure modes worth investigating: §1 (cursor-block), §2 (mouse), §3 (mode-2026 / synchronized output). Source verified at `git show 0691666:notes/lazygit-gap-analysis.md` (commit 0691666, PR #5, 2026-04-30); relocated to `BUGS.md` by PR #12 (2026-05-03). Three suspects; trajectory disposition for each is verifiable.

2. **PR #5's `notes/lazygit-ux-catalogue.md`** — 7 sections, each with a borrow/adapt/skip recommendation, plus a "Top 3 to consider first" closing section ranking §4 (Generalized pager picker), §2 (Context-sensitive prompt-row hint), §5 (Scoped `?` help). Source verified at `git show 0691666:notes/lazygit-ux-catalogue.md`. Seven sections; the catalogue's positive recommendations (adapt) and negative recommendations (skip) read against execution differently.

3. **PR #5's three additions to `ROADMAP.md`** — 26 lines added at the feature-tracks tail (per arc 02 investigation entry = 01KR0YXXZRQR24CSNAK4Q7808T's diff inspection). Three new entries verbatim-imported from the catalogue's "Top 3 to consider first" ranking: Generalized pager picker (catalogue §4), Context-sensitive prompt-row hint (catalogue §2), Scoped `?` help (catalogue §5). Survive in the current ROADMAP at lines 559-581. Three entries; trajectory disposition reads against the three executions individually.

4. **The product charter at `ROADMAP.md:3-23`** — *"It's a file manager that Claude can query — current directory, cursor, picks, inventory, filter, git branch — via a standard protocol. That bidirectional awareness is the positioning that differentiates spyc from `tmux` + `claude`."* Verified at `ROADMAP.md:11-15` (current state). Claude singular; arc 07 introduces a second AI peer (codex). Trajectory reads at the charter's load-bearing word "Claude."

5. **The product charter's non-goals at `ROADMAP.md:426-447`** — six items: native Windows; plugin system; localization; telemetry; full SLSA L3; mouse support beyond what already exists. Verified at current state (`ROADMAP.md:426-447`). Six non-goals; window-trajectory reads as honored or violated per item.

6. **The v2.0 framing at `ROADMAP.md:472-476`** — *"v2.0 version bump is a signaling choice as much as a semver one... target mid-to-late May 2026."* Tied to MCP positioning shift + public distribution. Verified at current state. Within-window observation only (beyond-window trajectory is tier-5).

7. **The v1.41.x patch cadence (insight-recurrence Pattern 5 = 01KR3D8RH5DNYC37WSGFVETXT3)** — 24 consecutive v1.41.x patches across the second half of the window after four minor cuts cluster early. Insight-recurrence's closure flagged this as "correlated with an unstated SemVer policy." There is no spyc-internal SemVer policy document. The cadence is observable; whether to claim trajectory is the live question this entry addresses below in document #7's per-document entry.

8. **The `onboarding-risk-register` seed (entry 0 = 01KR0P9JC8Z3DF6FQ1GJPF3VKA)** — catalogues five long-lived `cargo-deny` advisory ignores (RUSTSEC-2026-0009, RUSTSEC-2024-0320, RUSTSEC-2025-0141, RUSTSEC-2024-0436, RUSTSEC-2017-0008) with reasons, plus drift findings from arcs. The seed's catalogue is itself a stated-state-of-acceptance against which the window's largest dep-change (PR #31's vt100/ratatui/ansi-to-tui trio bump) can be measured.

A ninth surface — PR #5's "What we'd need to actually run to confirm" methodology — is folded into the gap-analysis entry as a corollary observation rather than carrying its own per-document entry, since the methodology shares PR #5's authorship and the same trajectory question (suspect §1's empirical-verification path).

A boundary case the brief flagged — the eight-arc record's cumulative four-grain framing as itself a stated artifact — is declined per the brief's guidance: that is emergent-property territory and belongs to `insight-emergent-properties`.

**Voice contract — the analyst register, carried unchanged from `insight-drift` and `insight-recurrence`.**

Permitted: more confident analytic register where observations span multiple PRs and stated-plan documents; synthesis across arcs; first-person plural sparingly when meaningful (*the catalogue*, *the trajectory reading*); structured headers when the material is per-document-shaped.

Banned: motive attribution to the maintainer (no *Derek wanted X / decided Y / thought Z*); invented technical details (provenance still required at every entry, including specific arc-entry ULIDs); clock-padding language (sequence facts where load-bearing only); fabricated trajectories (where there is no stated plan to track, *do not claim trajectory* — name the absence); trajectory claims dressing up motive ("the maintainer chose to defer X" is forbidden; "X was specified in document Y; X partially landed at PR Z" is the permitted shape).

The honesty contracts hold without modification. Three sites are particularly load-bearing for tier-3 discipline:

- *Where a stated plan is partially executed* (catalogue §2 by PR #20; the charter's "Claude" widening at arc 07's substrate-level), the entry names the partial execution exactly.
- *Where a stated plan is not executed* (catalogue §4 in its specific shape across four PRs; catalogue §5 deferred-on-§4), the entry names the non-execution with its load-bearing properties (DIRECTION ALIGNMENT counts; the specific shape doesn't land).
- *Where there is no stated plan but the project shows a recurrent shape* (Pattern 5's v1.41.x cadence; possibly the methodology's empirical-run absence), the entry names the absence-of-stated-plan rather than fabricating a trajectory. The cadence is what shipped, not what was promised.

**Methodology.**

For each stated-plan document (or per-cluster), the per-document entry below: states the stated-plan content, quoting verbatim where load-bearing; enumerates the landing instances with arc-entry citations and dispositions (executed-as-specified / partial / direction-aligned / deferred / non-executed); cross-references prior insight-thread observations without re-litigating them; flags the boundary with `insight-emergent-properties` where a tier-4 temptation arises; names where execution diverged from specification at the load-bearing grain.

**Cadence — eight stated-plan documents handled across six per-document entries.**

The lazygit corpus (#1, #2, #3) reads as three separate entries because each carries a distinct load-bearing observation: the gap-analysis suspects ship the *longest single trajectory* in the network (three for three on suspect-resolution-or-deferral across two arcs over eight calendar days); the UX catalogue ships the *most load-bearing reading* (skip honored exactly; adapt all modified-not-executed); the ROADMAP additions ship the *verbatim-imported reading* (three entries; zero exactly-as-specified executions). The methodology (#9 in the brief) folds into the gap-analysis entry. The charter (#4) and non-goals (#5) read as one entry because the non-goals trajectory is the charter trajectory's complement (the charter's positive framing of what spyc is; the non-goals' negative framing of what spyc isn't). The v2.0 framing (#6), the v1.41.x cadence (#7), and the advisory-ignore catalogue (#8) each read as their own entry.

**Cross-thread reading: trajectory builds on recurrence and drift but does not duplicate.**

Where insight-recurrence's closure (= 01KR3DFHA7FRV3BXEH2Z8SFJQN) flagged six patterns and named which carry tier-3 weight (Patterns 3 SMALL/MAYBE-to-FIXED lift; 4 named-then-fixed bracket; 5 v1.41.x cadence — partially), this thread carries those observations forward against the stated-plan documents that anchor them, without re-enumerating instance lists. Where insight-drift's Pattern F (= 01KR3BN3N6YF60414FFVHAM50Y) named the "22-day window" framing's ~3× compression of the merge-window (the spine's project-age framing applied to a merge-window measurement), this thread treats the trajectory of v2.0 (#6) and the v1.41.x cadence (#7) against the corrected merge-window denominator (~7-8 calendar days) where intensity-per-day reads are load-bearing.

**What `insight-trajectory` is NOT for.**

NOT motive attribution. The most acute tier-4 temptation in this thread is the catalogue's adapt-recommendations-all-modified pattern: every catalogue §-positive recommendation landed as partial / parallel / deferred, never as exactly-specified. Why this happened is forbidden here; that the trajectory has this shape is tier-3 and named factually.

NOT emergent-property naming. *The trajectory has shape X across N stated-plan documents* is tier-3 when X is a counting observation. *X reflects an emergent property of the working register / surface complexity / planning style* is tier-4.

NOT forward predictions beyond the 22-day window. The v2.0 framing's mid-to-late-May-2026 target sits at the window's edge; whether v2.0 lands in the next ~7-21 days is *outside the window* and tier-5. The within-window observation is the load-bearing one: at v1.41.24 on 2026-05-07, v2.0 has not landed.

NOT re-litigating drift or recurrence instance enumerations. Cross-references; not duplications.

NOT claiming trajectory where no stated plan exists. The v1.41.x cadence is observable; the SemVer policy is unstated. The trajectory thread's job at #7 is to name the absence, not to invent the plan.

Provenance:
- `insight-drift` framing entry = 01KR3B7KW5QNRWHG6YTV9QSF07 (analyst register declared; tier taxonomy named).
- `insight-drift` Pattern F entry = 01KR3BN3N6YF60414FFVHAM50Y (span-phrasing inconsistency; ~3× compression of the merge-window denominator; informs v2.0 and cadence trajectory readings here).
- `insight-drift` closure entry = 01KR3BQHTCQ7YGHWQJ2ZE4PJQ4 (boundary rule for placement of cross-cutting observables).
- `insight-recurrence` framing entry = 01KR3CSQ2YHQ2TD8EAE6DJCTS3 (tier-2 register; six patterns).
- `insight-recurrence` Pattern 5 entry = 01KR3D8RH5DNYC37WSGFVETXT3 (v1.41.x cadence; the unstated-policy question this thread answers at document #7).
- `insight-recurrence` closure entry = 01KR3DFHA7FRV3BXEH2Z8SFJQN (cross-thread observations directly handed forward, including which patterns carry tier-3 weight).
- `insight-recurrence` story-tail entry = 01KR3DM9DBRV6MBA4D516KRR12 (story-tails-as-upstream observation; informs this thread's per-cluster entry shape).
- `history-overview` framing entry = 01KR0TRFWT9W6WMFHC49YSW0BG (voice contract source; carried unchanged into the analyst register).
- `history-overview` segmentation entry = 01KR0TWHTC1MPK4KJ08Y9SPE6P (eight-arc segmentation; arc-affiliation table for stated-plan trajectory cross-arc reads).
- `notes/lazygit-gap-analysis.md` and `notes/lazygit-ux-catalogue.md` content verified at `git show 0691666:notes/...` (commit 0691666, PR #5, 2026-04-30). Top-suspects §1/§2/§3 and catalogue §1-§7 dispositions sourced from arc 02 investigation entry = 01KR0YXXZRQR24CSNAK4Q7808T.
- `ROADMAP.md` current state — charter at lines 3-23 (verified at `ROADMAP.md:1-50`); non-goals at 426-447 (verified at `ROADMAP.md:420-447`); v2.0 framing at 472-476 (verified at `ROADMAP.md:449-476`); three lazygit-inspired entries at 559-581 (verified at `ROADMAP.md:555-582`).
- `BUGS.md` post-PR-#37 state — SMALL/MAYBE/FIXED bucket distribution verified at `BUGS.md:1-110` head plus FIXED block at 107-260; the cursor-block-reverse-video text added by PR #12 still present in SMALL post-PR-#29 (verified by arc 03 PR #29 entry = 01KR10G02J2234D0WBMWMYC35M).
- `onboarding-risk-register` entry 0 = 01KR0P9JC8Z3DF6FQ1GJPF3VKA — five `cargo-deny` advisory ignores plus drift findings.
- `onboarding-product-charter` entry 0 = 01KR0P18MCE1H57Q5ZTAGKAJNH — charter source (`ROADMAP.md:3-23` quoted verbatim there).
- Per-PR landing entries verified for trajectory: arc 02 investigation = 01KR0YXXZRQR24CSNAK4Q7808T; arc 03 PR #29 = 01KR10G02J2234D0WBMWMYC35M; arc 05 PR #20 = 01KR2A6TT516XA5FEGVBXYPWD7; arc 05 PR #33 = 01KR2AAX12XSNRNZPTXJT2TXJA; arc 05 PR #35 = 01KR2AD5PV989H58E49E5D18NM; arc 06 PR #8 = 01KR2GCH3Q8DR9DATBBC802Q8W; arc 06 PR #10 = 01KR2GH1D9QCGDPZEMWW09R898; arc 07 PR #37 = 01KR2JCF7QEJHEG30TVMWY79CQ; arc 08 PR #31 = 01KR397RTYNS34SAGM46YJJRBY.
- `watercooler_health` against the spyc code_path reports Healthy at session start (server v0.4.6.dev0; threads-repo URL `git@github.com:calebjacksonhoward/spyc.git`; branch parity clean).

<!-- Entry-ID: 01KR3EJ0RWZXEBMYHY9EEZQX4A -->

---
Entry: Claude Code (caleb) 2026-05-08T09:28:42.420940+00:00
Role: scribe
Type: Note
Title: Document #1: PR #5's gap-analysis Top Suspects — three for three on suspect-resolution-or-deferral, the longest single trajectory in the network

Spec: scribe

tags: #insight #trajectory

**Stated-plan content.** PR #5 (commit 0691666, 2026-04-30) added `notes/lazygit-gap-analysis.md` (111 lines, since relocated to `BUGS.md` by PR #12 on 2026-05-03). The body comprises a 16-row gap table per terminal feature, an initialization-fingerprint section, and a "Top suspects" section with three numbered candidates for the user-reported "rendering / conflict issues" — verbatim, in descending probability:

> **§1 — Spurious cursor block from `widget.rs`.** "spyc unconditionally reverse-videoes the cell at `screen.cursor_position()`, even when the child has set DEC ?25l (cursor hidden). vt100 already exposes `screen.hide_cursor()`, but `src/pane/widget.rs:43–55` never reads it. lazygit hides the cursor and draws its own selection highlight, so a stray reverse-video square sits on some panel — visually reads exactly as 'rendering glitch'."

> **§2 — No mouse, anywhere.** "Mouse capture is not enabled on the host terminal (`src/main.rs::setup_terminal` has no `EnableMouseCapture`), and `src/pane/input.rs` has no encoder for `Event::Mouse`. lazygit defaults `MouseEvents: true` and binds click/scroll on every panel — to a daily user this manifests as 'clicks and scroll-wheel don't work in lazygit', easily called a 'conflict issue'."

> **§3 — Synchronized-output (mode 2026) tearing.** "tcell wraps every redraw in `\x1b[?2026h … \x1b[?2026l`. vt100 0.15 has no parse arm for 2026 — bytes are dropped, but more importantly, spyc never gets the 'buffer until end-of-frame' hint, so during a fast diff scroll or commit-list page-down the renderer reads a half-finished frame and paints it. Looks like flicker / a sliver of stale text under the new content for one frame."

The document also closes with a "What we'd need to actually run to confirm" methodology section, naming three concrete empirical-verification steps for suspect §1 and §2: *"run lazygit in the lower pane against this worktree, with `SPYC_PTY_DEBUG=1`; click a panel and verify nothing reaches the pty (`SPYC_PTY_DEBUG` writer-side); compare bare-terminal lazygit (truecolor diff palette) against in-pane lazygit screenshot to confirm truecolor downgrade or rule it out."*

**Per-suspect trajectory.**

**§1 — RESOLVED. Closed by arc 03's PR #29 (= 01KR10G02J2234D0WBMWMYC35M).** PR #5 itself shipped the narrow `if !self.screen.hide_cursor()` guard (a 7-line `src/pane/widget.rs` fix); arc 03's PR #29 (commit bdb8d87, 2026-05-06) generalized to a three-condition guard `focused && !alternate_screen() && !hide_cursor()` and dropped the focused/unfocused dim branch entirely. The PR #29 entry quotes the policy comment verbatim, naming the broader class — "(nvim, vim, less, htop, lazygit, claude in TUI mode) paint their own cursor in their own shape." PR #29's commit subject reads "fix: skip pane cursor block for unfocused / alt-screen panes (v1.41.16)"; the back-reference to PR #5's gap-analysis suspect §1 is implicit at the commit subject level (no `lazygit` mention) but explicit at the policy-comment level (the alt-screen TUI list names lazygit).

The trajectory observation: PR #5 specified the bug class via the suspect §1 text and shipped a narrow fix that addressed exactly the lazygit-named case (hide_cursor); PR #29 generalized to the broader bug class six calendar days later. The specification covered the lazygit case; the execution covered both the lazygit case (PR #5) and the broader class (PR #29). The gap-analysis suspect §1 is fully resolved at the eight-arc terminus.

A *durable-record* incompleteness flagged by arc 03's PR #29 entry: PR #29's diff does not remove the cursor-block-reverse-video text PR #12's harvest had lifted from suspect §1 into BUGS.md `### SMALL ###`. Verification against current-state `BUGS.md:4-13` confirms the entry persists post-window. The behavior is fixed; the catalogued risk text survives in the durable record. The trajectory disposition is RESOLVED at code/behavior; INCOMPLETE at durable-record cleanup. Arc 03's PR #29 entry already named this; the trajectory thread carries it forward as a partial-trajectory note rather than re-litigating.

**§2 — DEFERRED-AS-NON-GOAL. Non-executed across the 22-day window; aligns with charter non-goal at `ROADMAP.md:445-447`.** Verified at current-state `ROADMAP.md:445-447`: *"Mouse support beyond what already exists. Old roadmap mentions it; deprioritize indefinitely. The tool is keyboard-first by thesis."* No PR in the 22-day window enables mouse capture in `src/main.rs::setup_terminal`, and no PR in the 22-day window adds a `KeyCode::Mouse(_)` arm to `src/pane/input.rs`. Verified by grep across the eight per-PR entries: arc 06 PR #10 (= 01KR2GH1D9QCGDPZEMWW09R898) explicitly refutes any mouse alignment for quickselect (the labeled-overlay picker is keyboard-only with alphabetic labels; case-as-intent dispatch).

PR #12's harvest (= arc 02 harvest entry, cited by arc 02 investigation entry = 01KR0YXXZRQR24CSNAK4Q7808T) lifted suspect §2 into `BUGS.md ### BIGGER ###` rather than `### SMALL ###`, with the framing "Worth designing carefully because spyc itself doesn't want mouse events outside the pane — the right shape is 'forward to pane only when pane is focused.'" Verified at current-state `BUGS.md:54-69`: the entry persists in BIGGER post-window. Two separate stated documents converge on the same disposition: the gap analysis names the gap; the charter names the deferral; the harvest catalogues both.

The trajectory observation: suspect §2 is the cleanest case of stated-plan-trajectory honoring stated-plan-deferral. Two surfaces of the maintainer-authored plan agreed (the gap analysis flagged a class; the charter named the class as non-goal); execution honored the deferral. The trajectory is non-execution-as-honored, not non-execution-as-omitted.

**§3 — RESOLVED. Closed by arc 08's PR #31 (= 01KR397RTYNS34SAGM46YJJRBY).** PR #31 (commit fc1789d / merge 105db8d, 2026-05-06) bumped vt100 0.15 → 0.16 (forced by ratatui 0.29 → 0.30, forced by ansi-to-tui 7 → 8 — the dep-graph trio whose forcing function is `unicode-width ≥0.2.1` from vt100 0.16 vs `=0.2.0` from ratatui 0.29). The PR #31 entry quotes three stated-plan resolutions on the §3 question:

- *Commit body verbatim*: "Also retires the two MAYBE entries from BUGS.md about mode 2026 (synchronized output) and OSC 8 (terminal hyperlinks) — both should now parse correctly under 0.16."
- *BUGS.md diff*: removes both MAYBE entries (the upgrade-motivation entry PR #30 added; the mode-2026 entry PR #12 lifted from `notes/lazygit-gap-analysis.md`); adds a `(fixed, v1.41.18)` block whose closing line repeats the assertion verbatim.
- *CHANGELOG entry under `### Changed`*: names the trio bump and the panic fix; does not name mode 2026 by name.

The trajectory observation: arc 02's investigation entry deferred §3's resolution explicitly to arc 08 — *"Whether arc 08's PR #31 (`chore/vt100-and-ratatui-upgrade`, vt100 0.15 → 0.16) incidentally addresses suspect §3 is determinable only from inspection of vt100 0.16's release notes; the arc-02 author defers to arc 08 for that empirical check."* PR #31's diff supplies that inspection at the BUGS.md durable-record level (the maintainer's commit body and BUGS.md text both assert resolution); the upstream vt100 0.16 source is not vendored in the diff, so the authoritative resolution is the maintainer's claim plus the upgrade actually shipping. The arc 08 → arc 02 cross-thread closes at PR #31; this is the trajectory's terminus for §3.

A *test-coverage* note flagged by arc 08's PR #31 entry: no test in PR #31 exercises the specific mode-2026 escape sequence the upgrade is claimed to fix. The test-coverage and the trajectory-resolution do not align at the unit-test grain. Captured factually; the trajectory disposition is RESOLVED at the durable-record-and-dep-graph level; verification at the test grain is not narratable from the diff.

**The methodology, folded in (#9 in the framing's enumeration).**

PR #5's "What we'd need to actually run to confirm" section named a specific empirical-verification methodology for suspect §1 — *run lazygit in the lower pane with `SPYC_PTY_DEBUG=1`; click a panel and verify nothing reaches the pty*. The trajectory: the methodology was named in the gap analysis; the empirical run is not narratable from any commit-message or per-PR entry in the 22-day window. Arc 03's PR #29 entry confirms PR #29 generalized to the three-condition class-shape guard *without* citing the methodology — PR #29's policy comment names the alt-screen TUI list (the broader class) as the empirical answer to the gap analysis's question, not the `SPYC_PTY_DEBUG=1` run as the verification.

Trajectory disposition for the methodology: **NAMED-NOT-CITED**. The specific empirical methodology PR #5 named is not visibly executed in the per-PR entries; the underlying class-shape question the methodology was meant to verify is answered via a different path (the alt-screen TUI enumeration in PR #29's policy comment). The naming-without-execution is the kind of stated-plan-vs-trajectory observation that's cleanest to flag without interpretation: the gap-analysis methodology was specified; the path actually taken to suspect §1's broader class was different.

**Cross-thread cross-reference: insight-recurrence Pattern 4 (named-then-fixed bracket) reads at this trajectory.**

`insight-recurrence` Pattern 4 (closure entry = 01KR3DFHA7FRV3BXEH2Z8SFJQN) named three named-then-fixed bracket instances at three time grains. The gap-analysis-suspects bracket is *not* one of those three (Pattern 4 treated PR #18 → PR #37 at the two-day grain; PR #28 reading PR #12's harvest at the one-PR cross-arc grain; the 49-minute pair). The gap-analysis-suspects bracket is a *cross-arc* trajectory at *eight calendar days* across *two arcs* (arc 02 → arc 03 for §1; arc 02 → arc 08 for §3), substantially longer than Pattern 4's largest grain. Where Pattern 4 named the *recurrence* of the bracket shape (three instances at three grains), this trajectory entry names the *trajectory longevity* of the gap-analysis-suspects shape. The two readings do not duplicate; Pattern 4 counted recurrence-of-shape; this entry counts trajectory-of-stated-suspect.

**The cumulative reading.**

Three suspects; three dispositions:
- §1: RESOLVED (with durable-record incompleteness) by arc 03's PR #29 (Day 6 of the window).
- §2: DEFERRED-AS-NON-GOAL across the window; aligns with charter non-goal.
- §3: RESOLVED (with test-coverage gap) by arc 08's PR #31 (Day 6).

Three for three on suspect-resolution-or-deferral. The trajectory longevity from PR #5 (Day 0) to PR #29 (Day 6) plus PR #31 (Day 6) is six and seven calendar days respectively; from PR #5's gap-analysis catalogue to the eight-arc record's terminus, the suspects all close. The methodology PR #5 named for verification is named-not-cited; the underlying questions are answered via different paths.

**The longest single trajectory in the network.** Three stated-plan items in one document; eight calendar days from specification to terminal disposition; two arcs touched for resolution (arc 03 for §1; arc 08 for §3); one arc for non-execution-as-honored (the entire 22-day window, against the charter non-goal at #5). The framing entry named this as the longest-single-trajectory hypothesis; this entry verifies it factually.

The catalogue's seven-section trajectory (the next per-document entry, against the UX catalogue) carries a different load-bearing observation — *not* longevity, but the asymmetry between skip-honored-exactly and adapt-all-modified.

Provenance:
- 0691666 (PR #5 investigate/lazygit-support, 2026-04-30) — full PR.
- `notes/lazygit-gap-analysis.md` "Top suspects" §1, §2, §3 verbatim, verified at `git show 0691666:notes/lazygit-gap-analysis.md`. Methodology section verbatim, verified at the same source.
- bdb8d87 (PR #29 fix/skip-pane-cursor-block-when-uninvited, 2026-05-06) — §1 resolution; three-condition guard at `src/pane/widget.rs`; policy-comment alt-screen TUI list.
- fc1789d / 105db8d (PR #31 chore/vt100-and-ratatui-upgrade, 2026-05-06) — §3 resolution; trio-bump dep-graph chain; BUGS.md MAYBE retirement; commit-body assertion verbatim.
- `ROADMAP.md:445-447` current state — mouse non-goal verbatim, source for §2's deferral disposition.
- `BUGS.md:4-13` current state — cursor-block-reverse-video text from PR #12's harvest, surviving post-PR-#29 (durable-record incompleteness).
- `BUGS.md:54-69` current state — mouse §2 entry in BIGGER bucket, surviving post-window.
- `history-arc-02-lazygit-investigation-and-harvest` investigation entry = 01KR0YXXZRQR24CSNAK4Q7808T (gap-analysis source; deferral language for §3 to arc 08).
- `history-arc-03-pane-behavior` PR #29 entry = 01KR10G02J2234D0WBMWMYC35M (§1 resolution; alt-screen TUI list; durable-record-incompleteness flag).
- `history-arc-08-recoverability-and-deps` PR #31 entry = 01KR397RTYNS34SAGM46YJJRBY (§3 resolution; trio-bump structure; arc 02 → arc 08 cross-thread terminus).
- `insight-recurrence` Pattern 4 entry = 01KR3D5B59F5DX6BZZPB1VTQB3 (named-then-fixed bracket recurrence reading; non-overlap with this trajectory entry's longevity reading).
- `insight-trajectory` framing entry = 01KR3EJ0RWZXEBMYHY9EEZQX4A.

<!-- Entry-ID: 01KR3ENV1WP6R9SFRE1QME291S -->

---
Entry: Claude Code (caleb) 2026-05-08T09:30:44.211984+00:00
Role: scribe
Type: Note
Title: Document #2: PR #5's UX catalogue — skip honored exactly, adapt all modified-not-executed (the load-bearing observation)

Spec: scribe

tags: #insight #trajectory

**Stated-plan content.** PR #5 added `notes/lazygit-ux-catalogue.md` (288 lines, since relocated to `BUGS.md` by PR #12). Seven sections, each with a borrow / adapt / skip recommendation; closing "Top 3 to consider first" section ranks §4 (Generalized pager picker), §2 (Context-sensitive prompt-row hint), §5 (Scoped `?` help). The catalogue opens with a tension-acknowledgement: *"lazygit is mouse-first with keyboard parity... spyc's DESIGN.md is explicit that 'Keys are the API; mouse is a courtesy.' That difference shows up most strongly in surfaces 4 (popups) and 6 (row verbs) below: the *affordance* lazygit shows on screen often only earns its keep because clicking it is a primary input. Where I recommend adapting, I'm recommending the keyboard half, not the click target."*

The catalogue's seven-section recommendation table:

- §1 Numbered panels & direct-jump — **skip** ("spyc has exactly two top-level surfaces (list, pane) where lazygit has five, so `1` and `2` would be wasted on a binding that `^W j`/`^W k` already covers cleanly").
- §2 Context-sensitive footer — **adapt** ("into the prompt row, not the status bar... a `context_hints()` accessor on each overlay returning a `Vec<(key, label)>`; paint via `Style::DIM` when the prompt is otherwise idle").
- §3 Command log + "Random tip" — **skip** the log ("Spyc doesn't *run* git or other commands behind the user's back the way lazygit does"); **adapt** the tip ("Adapt as a one-shot flash on first launch of the session, not a panel").
- §4 Popups / pickers (Menu, Confirm, Alert, Prompt, Toast) — **adapt** ("extend the pager into a generalized pick-from-list mode... A `PagerView::picker_items: Vec<(Label, Action)>` with Enter-to-fire gives spyc lazygit's Menu without adding a fifth overlay").
- §5 Sub-menu drill-down — scoped help — **adapt** ("scope `?` to current overlay first, then `?` again for global"; effort note: *"becomes nearly free once §4 lands"*).
- §6 Single-key action vocabulary on rows — **skip** ("spyc has effectively one (the list), so the same letter shouldn't be taught two meanings").
- §7 Two-letter chord jumps — **skip** ("spyc's chord-family discipline... is the right call for a file commander").

**Per-section trajectory.**

**§1 — SKIP HONORED. Refuted explicitly by arc 06's PR #10 (= 01KR2GH1D9QCGDPZEMWW09R898).** PR #10 (commit 9043547, 2026-05-02) ships the labeled-overlay quickselect with alphabetic labels (`abcdefghilmnoprstuvwxyz`, 23 letters with `q`/`Q`/`j`/`k` deliberately omitted), case-as-intent dispatch, ephemeral 1- or 2-letter labels assigned per match per scan. The PR #10 entry explicitly reads PR #10 against the §1 reading and refutes it: *"the labels are alphabetic (`a`..`z` minus `q`/`j`/`k`), not numeric. The catalogue's §1 pattern is specifically lazygit's `[N]-Status`, `[N]-Files` etc. with `1`..`5` as direct-jump targets to top-level panels... PR #10 doesn't ship numbered direct-jumps to anything. The labeled-overlay picker is structurally a different idiom... The §1 SKIP recommendation is not invalidated by PR #10's existence; PR #10 is not a §1 instance."* §1 stays skip-honored across the window.

**§2 — PARTIAL EXECUTION. Half-landed by arc 05's PR #20 (= 01KR2A6TT516XA5FEGVBXYPWD7).** PR #20 (commit ee07307, 2026-05-05) ships an alt-screen scroll-mode hint: `^a v` against a full-screen TUI (codex, claude post-startup, vim, htop, lazygit) flashes a context-aware message instead of the generic one. The PR #20 entry verifies the disposition: *"PR #20 honors the *spirit* — a context-sensitive hint surfaced when context shifts (the scroll-mode flash gains a context-aware variant) — without executing the catalogue's proposed shape (a per-overlay `context_hints()` accessor). The flash-info path is hardcoded in one `Action` arm; no `context_hints()` accessor is introduced."* PR #20 narrows to alt-screen detection; the broader options-map idea — a per-overlay `context_hints()` accessor on each overlay returning `Vec<(key, label)>`, painted in DIM at the prompt row when otherwise idle — does not land. §2 trajectory disposition: PARTIAL EXECUTION, narrowed to alt-screen detection.

**§3 — NON-EXECUTED.** No PR in the 22-day window ships a command log or a one-shot startup tip surface. The catalogue's recommendation has two halves (skip the log; adapt the tip); only the skip-half is observable as honored (no command-log surface lands). The adapt-half (one-shot flash via `Action::describe`-keyed tip table) is non-executed. §3 trajectory disposition: SKIP-HALF HONORED, ADAPT-HALF NON-EXECUTED.

**§4 — DIRECTION ALIGNMENT BY FOUR PRs ACROSS TWO ARCS; SPECIFIC SHAPE NON-EXECUTED.** This is the catalogue's load-bearing observation, ranked #1 in the "Top 3 to consider first" closing section as "Highest-leverage of the lazygit borrows." The catalogue's specific recommendation — extend the pager via `PagerView::picker_items: Vec<(Label, Action)>` with Enter-to-fire dispatch — is not directly executed by any PR in the 22-day window. Four PRs hold DIRECTION ALIGNMENT, in two arcs:

- *arc 05 PR #33* (= 01KR2AAX12XSNRNZPTXJT2TXJA, `feat/pager-visual-line-mode`, cf9e8ff, 2026-05-06) — ships `VisualSelection { anchor, cursor }` field on `PagerView`, structurally analogous to `picker_items` (both are `Option`-shaped state on `PagerView` that gates a mode), but selects a *line range* for one terminal action (yank), not pick-from-many discrete options. PR #33's entry: "DIRECTION ALIGNMENT, not direct execution."
- *arc 05 PR #35* (= 01KR2AD5PV989H58E49E5D18NM, `feat/D-opens-pager-in-top-pane`, c243549, 2026-05-06) — `display_in_pane` launches an external `$PAGER` (less, most, etc.) into a pty overlay; does not populate spyc's internal `PagerView` with picker items. PR #35's entry: "DIRECTION ALIGNMENT, no direct catalogue-item execution."
- *arc 06 PR #8* (= 01KR2GCH3Q8DR9DATBBC802Q8W, `feat/harpoon`, 62fc129, 2026-05-02) — ships `HarpoonMenu` as a standalone modal `Block` rendered by `App::render_harpoon_menu`, with its own cursor and `delete_armed` state, dispatched through `App::handle_harpoon_menu_key`; the pager is not involved. PR #8's entry: "PARALLEL PATTERN, not direct execution of §4."
- *arc 06 PR #10* (= 01KR2GH1D9QCGDPZEMWW09R898, `feat/quickselect`, 9043547, 2026-05-02) — ships `QuickSelect` co-located with `Pane`-adjacent code, with case-as-intent dispatch; `PagerView` is not involved. PR #10's entry: "PARALLEL PATTERN, not direct execution of §4."

The four PRs' joint-disposition is named at arc 05's closure (= 01KR2AJVZA1E85YSKHF4FNRQQ3): *"four PRs across two arcs hold §4 alignment; zero execute the `PagerView::picker_items` shape."* Arc 05's story-tail (= 01KR2ANRAEFWWR5W9FQP11A0DB) deferred the question to the insight layer factually. §4 trajectory disposition: DIRECTION ALIGNMENT BY FOUR PRs, SPECIFIC SHAPE NON-EXECUTED.

A property of the four-PR alignment worth naming at trajectory-grain: the four PRs split 2-2 between pager-as-mode-side (arc 05) and standalone-overlay-side (arc 06). Two different families of "the picker shape, but not as a pager mode." The catalogue's §4 thesis ("render *into* the pager") is honored by the arc 05 half (pager hosts the mode) and refused by the arc 06 half (the picker stands alone). Both halves count as DIRECTION ALIGNMENT against the catalogue's general "render through pager" framing; neither half lands the specific `PagerView::picker_items: Vec<(Label, Action)>` field.

**§5 — DEFERRED-ON-§4. Non-executed; consistent with the catalogue's own effort note.** The catalogue's effort note for §5 reads verbatim: *"becomes nearly free once §4 lands."* Inversely, §5 stays expensive while §4 stays unlanded. No PR in the 22-day window restructures `src/ui/help.rs` to scope `?` first to current overlay then to global. Verified by absence: arc 05's PR #23 (`feat/help-yf-and-percent-docs`) adds a help-yf binding and discoverability fixes without restructuring the help dump's section ordering. §5 trajectory disposition: NON-EXECUTED, consistent with the catalogue's own conditional-on-§4 framing.

**§6 — SKIP HONORED.** The catalogue's skip rationale was structural: spyc's globally-consistent verb vocabulary is the right call for a one-list-pane file commander. No PR in the 22-day window introduces panel-scoped key reuse where the same letter does different things in different panels. Verified by absence: arc 06's chord-family fixes (PR #25 input-dispatch hardening; PR #32 chord-priority) reinforce the existing chord-prefix discipline rather than introducing panel-scoped overload. §6 trajectory disposition: SKIP HONORED.

**§7 — SKIP HONORED.** The catalogue's skip rationale named existing chord-family discipline (`g <x>` / `^a <x>` / `^W <x>` / `H <x>` / `W <x>` / `m <x>` / `' <x>`). No PR in the 22-day window introduces flat 2-letter mnemonics. PR #8's harpoon family adds `H1`..`H9` / `Ha` / `Hx` / `Hh` (under the new `H` chord prefix) — that's chord-prefixed, not flat 2-letter. §7 trajectory disposition: SKIP HONORED.

**The load-bearing observation, summarized.**

Negative recommendations (skip): four sections ranked skip — §1, §3-log-half, §6, §7. *All four were honored exactly across the 22-day window.* No PR introduced numbered panels-jump, command log surface, panel-scoped key reuse, or flat 2-letter mnemonics.

Positive recommendations (adapt): four sections ranked adapt — §2, §3-tip-half, §4, §5. *All four landed in modified shape — partial / parallel / deferred — none executed exactly as the catalogue specified.*
- §2: PARTIAL (alt-screen-hint half only; broader options-map idea non-executed).
- §3-tip-half: NON-EXECUTED.
- §4: DIRECTION ALIGNMENT BY FOUR PRs; specific shape non-executed.
- §5: NON-EXECUTED (consistent with the catalogue's conditional-on-§4 framing).

**The asymmetry is the trajectory observation.** The catalogue's skip recommendations were honored as specified; the catalogue's adapt recommendations all landed in modified shape, none as specified. Counting the dispositions: 4-of-4 skips honored; 0-of-4 adapts executed-as-specified, 1-of-4 partial, 1-of-4 non-executed-against-conditional, 2-of-4 non-executed (§3-tip-half; pieces of §4 specifically). The skip-vs-adapt asymmetry holds across the entire seven-section catalogue.

**Cross-thread cross-reference.** This entry refers to insight-recurrence Pattern 4 (named-then-fixed bracket) for the bracket-shape question: catalogue §4's specification has held open across the window with four DIRECTION ALIGNMENT PRs without a closing PR that lands `PagerView::picker_items`. Whether the §4 bracket is a *long-grain* named-then-fixed bracket awaiting closure (and Pattern 4's three-instance count understates the population) or a *non-bracket* (where the four PRs are the trajectory and there is no eventual close to the catalogue's specific shape) is determinable only beyond the window. Tier-3 trajectory observation: at the window's terminus, §4's bracket sits open with four DIRECTION ALIGNMENT PRs in lieu of one specific-shape execution.

**Boundary with `insight-emergent-properties`.** The asymmetry between skip-honored-exactly and adapt-all-modified is data; whether it reflects an emergent property of "the catalogue is more reliable as a refusal mechanism than as an execution mechanism" or "the maintainer's working-style produces parallel-not-substrate execution" or any other property naming is *tier-4 territory* and forbidden here. The trajectory thread states the asymmetry; the property name is `insight-emergent-properties`'s.

Provenance:
- 0691666 (PR #5 investigate/lazygit-support, 2026-04-30) — full PR.
- `notes/lazygit-ux-catalogue.md` §1-§7 verbatim, including the "Top 3 to consider first" closing section, verified at `git show 0691666:notes/lazygit-ux-catalogue.md`.
- ee07307 (PR #20 feat/scroll-altscreen-hint, 2026-05-05) — §2 PARTIAL EXECUTION; verified at arc 05 PR #20 entry = 01KR2A6TT516XA5FEGVBXYPWD7.
- cf9e8ff (PR #33 feat/pager-visual-line-mode, 2026-05-06) — §4 DIRECTION ALIGNMENT (pager-as-mode side); verified at arc 05 PR #33 entry = 01KR2AAX12XSNRNZPTXJT2TXJA.
- c243549 (PR #35 feat/D-opens-pager-in-top-pane, 2026-05-06) — §4 DIRECTION ALIGNMENT (pager-launchable-from-listing side); verified at arc 05 PR #35 entry = 01KR2AD5PV989H58E49E5D18NM.
- 62fc129 (PR #8 feat/harpoon, 2026-05-02) — §4 PARALLEL PATTERN (standalone-overlay side); verified at arc 06 PR #8 entry = 01KR2GCH3Q8DR9DATBBC802Q8W.
- 9043547 (PR #10 feat/quickselect, 2026-05-02) — §4 PARALLEL PATTERN (standalone-overlay side); §1 explicit refutation; verified at arc 06 PR #10 entry = 01KR2GH1D9QCGDPZEMWW09R898.
- `history-arc-05-pager-surface` closure entry = 01KR2AJVZA1E85YSKHF4FNRQQ3 (cumulative §4 reading after arc 05; "four PRs across two arcs hold §4 alignment; zero execute the `PagerView::picker_items` shape").
- `history-arc-05-pager-surface` story-tail entry = 01KR2ANRAEFWWR5W9FQP11A0DB (deferred §4 question to insight layer).
- `history-arc-02-lazygit-investigation-and-harvest` investigation entry = 01KR0YXXZRQR24CSNAK4Q7808T (catalogue source; per-PR §-disposition assignments).
- `insight-recurrence` Pattern 4 entry = 01KR3D5B59F5DX6BZZPB1VTQB3 (named-then-fixed bracket recurrence; cross-reference for the §4 bracket-open-at-window-terminus question).
- `insight-trajectory` framing entry = 01KR3EJ0RWZXEBMYHY9EEZQX4A.

<!-- Entry-ID: 01KR3ESJ42TT0ZGJHGHJ5CTNYC -->
