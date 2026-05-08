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

---
Entry: Claude Code (caleb) 2026-05-08T09:32:07.112635+00:00
Role: scribe
Type: Note
Title: Document #3: PR #5's three ROADMAP additions — three entries verbatim-imported, zero exactly-as-specified executions, one partial, two deferred

Spec: scribe

tags: #insight #trajectory

**Stated-plan content.** PR #5 (commit 0691666, 2026-04-30) added 26 lines to `ROADMAP.md` per arc 02 investigation entry's diff inspection (`git diff 1f41b4b..3949983 -- ROADMAP.md`). Three new entries land verbatim from the catalogue's "Top 3 to consider first" ranking. Verified at current-state `ROADMAP.md:559-581` (post-PR-#37, the eight-arc record's terminus):

> 1. **"Generalized pager picker"** (`ROADMAP.md:559-566`): "Adapt lazygit's `Menu` popup pattern into spyc's existing `pager.picker_cursor` machinery so any list-of-options surface (project chooser, `W l` worktree picker, branch checkout) is a pager mode rather than a fifth overlay. Stays inside DESIGN.md's 'render *into* the pager' rule. Highest-leverage of the lazygit borrows because the scoped-help item below builds on it."

> 2. **"Context-sensitive prompt-row hint"** (`ROADMAP.md:567-575`): "Paint the most-relevant keys for the active overlay or mode into the prompt row using the DIM modifier — only when keys differ from list-mode (pager `?/n/s/:N`, finder, `!?` history editor, picker). DESIGN.md is explicit that a third status row is forbidden, so the prompt row is the only legal transient surface for this. Directly addresses the 'I know it exists but forgot the key' failure mode without a help-overlay context switch."

> 3. **"Scoped `?` help"** (`ROADMAP.md:576-581`): "Restructure the existing `src/ui/help.rs` dump to lead with the active surface's keys, then a collapsed 'global / other surfaces' tail. Content reorganization, not a new feature; cost is near-zero once the generalized pager picker lands and `?` can render its scoped section as a picker."

The three entries are tagged `(lazygit-inspired)` and survive intact post-PR-#37. PR #12's harvest (2026-05-03) deleted the `notes/...` cross-references that originally accompanied the entries (since the notes themselves were removed by the harvest), but the three entries' bodies remain at the same lines, with the same internal cross-reference structure (#1's "highest-leverage"; #1's reference forward to #3; #3's reference back to #1's machinery).

**Per-entry trajectory.**

**Entry #1 — Generalized pager picker.** Identical trajectory disposition to catalogue §4 (covered at this thread's document-#2 entry = 01KR3ESJ42TT0ZGJHGHJ5CTNYC). The ROADMAP entry is a verbatim re-statement of catalogue §4's recommendation, including the same load-bearing structural fragment (`pager.picker_cursor` machinery; the picker-as-pager-mode framing). Trajectory disposition: **DIRECTION ALIGNMENT BY FOUR PRs; SPECIFIC SHAPE NON-EXECUTED**. The four PRs (arc 05 PR #33 = 01KR2AAX12XSNRNZPTXJT2TXJA; arc 05 PR #35 = 01KR2AD5PV989H58E49E5D18NM; arc 06 PR #8 = 01KR2GCH3Q8DR9DATBBC802Q8W; arc 06 PR #10 = 01KR2GH1D9QCGDPZEMWW09R898) hold the same alignment against the ROADMAP entry as against the catalogue §4 source.

A note specific to the ROADMAP-entry-vs-catalogue framing: the ROADMAP entry includes verbatim the parenthetical *"(project chooser, `W l` worktree picker, branch checkout)"* — three concrete uses the picker would enable. None of those three uses landed in the 22-day window as a `pager.picker_cursor` mode. Project chooser is not narratable in the per-PR entries; `W l` worktree picker exists but predates the window; branch checkout is not landed at all. The ROADMAP entry's enumerated use-cases all sit at NON-EXECUTED post-window.

**Entry #2 — Context-sensitive prompt-row hint.** Identical trajectory disposition to catalogue §2. Trajectory disposition: **PARTIAL EXECUTION** by arc 05's PR #20 (= 01KR2A6TT516XA5FEGVBXYPWD7), narrowed to alt-screen detection. The ROADMAP entry's enumerated overlays-or-modes worth context-hinting — pager `?/n/s/:N`, finder, `!?` history editor, picker — none of these surfaces gain a per-overlay context-hint accessor. The alt-screen scroll-mode flash that PR #20 ships is a single hardcoded variant in one `Action` arm; the architectural shape the ROADMAP entry specifies (per-overlay `context_hints()` accessor; DIM at prompt row when otherwise idle) does not land.

The trajectory observation: the ROADMAP entry's specified shape is wider than what landed. The ROADMAP entry frames the hint as a per-overlay capability; PR #20 shipped a context-aware variant of one specific flash. The mechanism PR #20 ships is one-off and not extensible to the other overlays the ROADMAP entry names. The catalogue's `context_hints()` accessor would be that mechanism; PR #20 did not introduce it.

**Entry #3 — Scoped `?` help.** Identical trajectory disposition to catalogue §5. Trajectory disposition: **NON-EXECUTED**, consistent with the entry's own conditional-on-#1 framing (*"cost is near-zero once the generalized pager picker lands and `?` can render its scoped section as a picker"*). Since #1 has not landed in its specific shape, #3's near-zero cost has not materialized. The conditional clause is what makes the trajectory disposition consistent rather than divergent: the ROADMAP entry itself names the dependency, and the dependency is unresolved at window's terminus.

**The cumulative reading.**

Three ROADMAP entries; one partial execution; two deferred (one direction-aligned-not-executed; one non-executed-against-conditional). Zero entries that the maintainer authored as committed forward work landed in their specified shape across the 22-day window.

The ROADMAP-trajectory observation is structurally adjacent to the catalogue-trajectory observation but differs in framing. The catalogue (this thread's document #2 entry = 01KR3ESJ42TT0ZGJHGHJ5CTNYC) has seven sections of which four are skip and four are adapt; the skip-vs-adapt asymmetry was the load-bearing observation. The ROADMAP entries are a pure-positive-recommendation set: all three are adapt-imports from the catalogue's positive recommendations. There is no skip-half to honor exactly. The asymmetry-vs-symmetry difference is structural: the catalogue admits skip recommendations as one half of its trajectory disposition (and the skip-half all honored exactly); the ROADMAP entries are positive-only, and the trajectory has no negative-recommendation honor-disposition to balance against. The ROADMAP-trajectory's observation is purely *"three positive-recommendation entries; zero exactly-as-specified executions"*.

**Cross-thread reading: the trajectory plus insight-recurrence Pattern 4.**

`insight-recurrence` Pattern 4 (= 01KR3D5B59F5DX6BZZPB1VTQB3) named one named-then-fixed bracket instance with weighted design options at the open-side: PR #18 → PR #37 (two-day grain). PR #18's BUGS.md SMALL note authored three weighted design options; PR #37 implemented exactly the marked option. That bracket *closed* in the window.

The three ROADMAP entries here read as bracket-open-at-window-terminus instances at the multi-week grain — the entries' open-sides (the catalogue PR #5 imports) are stated; the close-sides (the specific-shape executions) have not landed. The brackets are open. Pattern 4's recurrence reading observed three closed brackets at three time grains; this trajectory entry observes three open brackets at the same multi-week grain at window's terminus.

The cross-thread cross-reference: the named-then-fixed bracket *recurrence* that closes is observable in the per-arc record; the named-then-fixed bracket *trajectory* that stays open across the window is observable here. Both readings are tier-2/tier-3 respectively; neither is tier-4 (whether the open brackets eventually close beyond-window is tier-5 and forbidden here). The ROADMAP entries' bracket-states are *open* at window's terminus; the trajectory thread states this factually.

**Boundary with `insight-emergent-properties`.**

The strongest tier-4 temptation in this entry is the question *why* three positive-recommendation ROADMAP entries import verbatim from the catalogue's "Top 3" but none execute exactly. The trajectory thread states the count; the *why* (planning-vs-execution dynamic, working-style under capacity, surface-specificity question, etc.) is forbidden here and belongs to `insight-emergent-properties`. This entry sits at the most acute tier-3-discipline test moment of the trajectory thread: the temptation to interpret the three-zero ratio as evidence of a property is high. Held to tier-3.

Provenance:
- 0691666 (PR #5 investigate/lazygit-support, 2026-04-30) — full PR.
- `git diff 1f41b4b..3949983 -- ROADMAP.md` (per arc 02 investigation entry's verification): 26 lines added at the feature-tracks tail; three entries each tagged `(lazygit-inspired)` with `notes/lazygit-ux-catalogue.md §N` cross-references at original-author state.
- `ROADMAP.md:559-581` current state — three entries verbatim, surviving post-PR-#37; cross-reference structure intact (#1's "highest-leverage" / forward to #3; #3's "near-zero once... #1 lands" / back to #1).
- ee07307 (PR #20 feat/scroll-altscreen-hint, 2026-05-05) — Entry #2 PARTIAL EXECUTION; verified at arc 05 PR #20 entry = 01KR2A6TT516XA5FEGVBXYPWD7.
- cf9e8ff (PR #33), c243549 (PR #35), 62fc129 (PR #8), 9043547 (PR #10) — Entry #1 DIRECTION ALIGNMENT four-PR set (cross-arc); verified at arc 05 PR #33 entry = 01KR2AAX12XSNRNZPTXJT2TXJA, arc 05 PR #35 entry = 01KR2AD5PV989H58E49E5D18NM, arc 06 PR #8 entry = 01KR2GCH3Q8DR9DATBBC802Q8W, arc 06 PR #10 entry = 01KR2GH1D9QCGDPZEMWW09R898.
- `history-arc-02-lazygit-investigation-and-harvest` investigation entry = 01KR0YXXZRQR24CSNAK4Q7808T (ROADMAP additions verbatim quoted; verbatim-import provenance).
- `history-arc-05-pager-surface` closure entry = 01KR2AJVZA1E85YSKHF4FNRQQ3 (cumulative §4-and-#1 reading).
- `insight-recurrence` Pattern 4 entry = 01KR3D5B59F5DX6BZZPB1VTQB3 (named-then-fixed bracket; cross-reference for bracket-open-at-window-terminus reading).
- `insight-trajectory` document #2 entry (UX catalogue) = 01KR3ESJ42TT0ZGJHGHJ5CTNYC (catalogue §-disposition shared with ROADMAP entries).
- `insight-trajectory` framing entry = 01KR3EJ0RWZXEBMYHY9EEZQX4A.

<!-- Entry-ID: 01KR3EW3166JZ59TDR8PYMGN4T -->

---
Entry: Claude Code (caleb) 2026-05-08T09:33:56.723338+00:00
Role: scribe
Type: Note
Title: Documents #4 and #5: the product charter and its non-goals — substrate-level widening with registration-level peer-specificity; five non-goals all honored across the window

Spec: scribe

tags: #insight #trajectory

The product charter and its non-goals read as one entry because the non-goals trajectory is the charter trajectory's complement. The charter names what spyc *is* in positive form; the non-goals name what spyc explicitly *isn't*. Both surfaces are maintainer-authored at `ROADMAP.md`, both are sourced at the `onboarding-product-charter` seed (entry 0 = 01KR0P18MCE1H57Q5ZTAGKAJNH), and both reads against the eight-arc record carry their own load-bearing observation.

---

**Document #4 — the product charter at `ROADMAP.md:3-23`.**

Stated-plan content, verified at current-state `ROADMAP.md:3-23`:

> *"spyc is a vi-keyboard-driven file commander that exposes itself to an AI coding agent as a queryable context source. The target user is a developer who already thinks in vi motions and wants Claude Code living in the same workspace -- not one window over, not in a browser tab, in the same session, sharing context.*
>
> *The MCP server (M14) shifted the tool's nature: spyc isn't just 'a file manager with Claude in a pane.' It's a file manager that Claude can query -- current directory, cursor, picks, inventory, filter, git branch -- via a standard protocol. That bidirectional awareness is the positioning that differentiates spyc from `tmux` + `claude`.*
>
> *Every other feature -- picks, inventory, pager, status bar, sessions -- is supporting infrastructure that makes the split-pane workflow fast and comfortable. The roadmap is organized accordingly: the pane-and-agent integration is the defining work track, not the trailing milestone."*

The charter's load-bearing word is "Claude" — singular. The thesis sentences ("an AI coding agent"; "Claude Code"; "Claude can query"; "differentiates spyc from `tmux` + `claude`") frame spyc as a tool whose *defining* differentiating bet is bidirectional awareness with one specific AI peer.

**Trajectory observation: the charter's "Claude" widens at substrate-level; stays peer-specific at registration-level.** Arc 07 (= 01KR2HYMMHAH316CA9KTWKWT6W framing; PR #21 = 01KR2J81DHNG4K8NHFVN0XMD1M; PR #37 = 01KR2JCF7QEJHEG30TVMWY79CQ) executes the codex-as-second-peer addition across three PRs:

- *PR #18* (2026-05-05) — chore/AGENTS.md rename plus MCP hygiene. Threads `context_path` through `Pane::spawn`; introduces the `.spyc-context-<pid>.json` marker file convention.
- *PR #19* (2026-05-05) — codex session save/restore parity with claude.
- *PR #21* (2026-05-05) — codex MCP discovery via `.codex/config.toml`; the second `ensure_*` registration file lands.
- *PR #37* (2026-05-07) — MCP socket discovery is now project-scoped; the function `discover_live_socket(caller_cwd)` is *peer-agnostic* (walks `.spyc-context-<pid>.json` markers regardless of which peer is calling).

The arc 07 PR #37 entry is explicit on the substrate-vs-registration distinction (consistent with arc 07's story-tail = 01KR2JM67RTQHQYN0223GTKH1V): *"The function `read_context_pids_in_dir(dir)` reads `dir`, extracts entries whose names match `.spyc-context-<pid>.json`, parses the PID via `pid_str.parse::<u32>()`, and returns the PID list."* The discovery walk is peer-agnostic; one socket, peer-agnostic discovery.

But the registration files are peer-specific: `.mcp.json` for claude, `.codex/config.toml` for codex. Two `ensure_*` registration files side-by-side in `src/mcp.rs`; each peer-specific. The substrate (the marker-walk discovery) is generalized; the registration is not.

**The trajectory disposition for the charter's "Claude" word.** At substrate-level, the charter's word *widens to "the AI peer"* — discovery, the marker-file convention, and `Pane::spawn`'s `context_path` parameter all treat "the queryable context source" as peer-agnostic. At registration-level, the charter's word *stays peer-specific* — claude and codex have their own registration paths, neither a generalization. The charter's sentence "*It's a file manager that Claude can query*" is *slightly modifiable* to "*It's a file manager that the AI peer can query*" without breaking against the substrate-level execution; the registration-level reads as continuing peer-specificity by intent (codex shipped as a parallel peer, not as a parameterized peer).

The charter is not *fully generalized* in the parametric sense (no peer-generic registration; each new peer gets its own `ensure_*` file). The trajectory is partial-widening at substrate-level; non-widening at registration-level. The charter sentence's "Claude" is therefore not exactly-honored, not exactly-broken — it has shifted at one architectural layer and stayed at another. That asymmetry is the trajectory observation.

---

**Document #5 — the product charter's non-goals at `ROADMAP.md:426-447`.**

Stated-plan content, verified at current-state `ROADMAP.md:426-447`:

> *"Native Windows support. WSL is the supported story... Plugin system. A decade of maintenance debt for a feature 3% of users will touch... Localization. English only. The target audience reads English docs. Telemetry. Not even anonymized opt-in... SLSA L3 / supply-chain theatre. Minisign signatures + SBOM + a reproducible build job are proportionate. Full SLSA attestation infrastructure is not. Mouse support beyond what already exists. Old roadmap mentions it; deprioritize indefinitely. The tool is keyboard-first by thesis."*

Six non-goals enumerated. Trajectory disposition for each across the 22-day window:

- **Native Windows support.** HONORED. Verified by absence and by CI matrix: `bitbucket-pipelines.yml` matrix (per arc 01 entries / `onboarding-test-surface` references) targets macOS / Linux only; no `windows-latest` job. No PR in the 22-day window adds Windows-specific paths or compilation guards. `portable-pty` is in the dep graph (technically Windows-capable), but no commit narrates a Windows test pass or platform-specific arm.

- **Plugin system.** HONORED. No PR in the 22-day window adds a plugin-loading surface, dynamic-library hook, or extension-point registry. The customization surfaces that exist (`.spycrc.toml`, the keymap layer) stay as they were; none gain plugin-system shape.

- **Localization.** HONORED. No PR in the 22-day window adds an `i18n/` directory, gettext-style infrastructure, or per-language string table. Verified at `onboarding-risk-register` entry 0's drift-finding #4: *"No `i18n/` directory, no localized README. `ROADMAP.md:434-436` lists localization as an explicit non-goal."*

- **Telemetry.** HONORED. No PR in the 22-day window adds outbound network calls, anonymous-usage-data emission, or opt-in telemetry plumbing. The MCP socket and stdio-proxy paths are local-only; the `:fg` / `:task` viewers are intra-process; no per-PR entry narrates a telemetry surface.

- **Full SLSA L3 / supply-chain theatre.** HONORED. The proportionate-not-exhaustive supply-chain controls land per arc 01: `cargo-deny` ignores at `deny.toml:72-94` (five long-lived advisory ignores per `onboarding-risk-register`); SECURITY.md authored at PR #3; `cargo-deny` in CI per `bitbucket-pipelines.yml`. SLSA attestation infrastructure is not introduced. Five advisory ignores survive the trio bump (per this thread's document #8 entry below).

- **Mouse support beyond what already exists.** HONORED. Verified at this thread's document #1 entry (gap-analysis suspect §2 = NON-EXECUTED-AS-NON-GOAL). No PR in the 22-day window enables mouse capture in `src/main.rs::setup_terminal` or adds a `KeyCode::Mouse(_)` arm to `src/pane/input.rs`. PR #6 (zoom) does not invoke mouse; PR #10 (quickselect) is keyboard-only labeled-overlay with alphabetic labels. The non-goal-honoring is observable both negatively (no execution) and positively (charter's non-goal explicitly named at `ROADMAP.md:445-447`).

**Six non-goals; six honored across the window. The cleanest non-trajectory in the network.**

The non-trajectory framing is the load-bearing observation: where stated-plan documents anchor *positive* recommendations to track (gap-analysis suspects §1-§3; catalogue §1-§7; ROADMAP additions #1-#3; charter's "Claude"), the trajectory has shape (resolved / partial / deferred / non-executed; with various asymmetries). Where stated-plan documents anchor *negative* recommendations (the six non-goals), the trajectory has uniform honor across the window. The charter's non-goals are the cleanest stated-plan-vs-execution match in the network: zero divergences across six items.

A pattern observation worth flagging at trajectory-grain: the catalogue's *negative* recommendations (the four skip items at §1, §3-log-half, §6, §7) were *also* honored exactly across the window (per this thread's document #2 entry = 01KR3ESJ42TT0ZGJHGHJ5CTNYC). The 22-day window's stated-plan-vs-execution shape carries the *same* asymmetry across two surfaces:
- Negative recommendations (charter non-goals; catalogue skips): all honored exactly, all negative-trajectory cleanest.
- Positive recommendations (catalogue adapts; ROADMAP additions; charter's "Claude" word; gap-analysis suspect §1): trajectory has shape (partial / direction-aligned / deferred / resolved), none cleanly exactly-honored except non-execution-as-aligned (suspect §2).

The negative-vs-positive asymmetry is a tier-3 *count* observation; whether it reflects an emergent property is forbidden here. The trajectory thread states the count.

**The charter as a positive-and-negative pair, summarized.**

The charter's positive framing ("Claude... bidirectional awareness... split-pane workflow") sees substrate-level widening (peer-agnostic discovery; one marker-file convention) and registration-level stay-as-is (two peer-specific `ensure_*` files). Partial widening; the charter's sentence is shift-able without breaking but not fully generalized.

The charter's non-goals (six items) sees uniform honor across the window. Zero divergences.

The pair shape: positive-framing has shape; negative-framing has uniform honor. The trajectory observation is the asymmetry of these two halves of the same charter document.

**Cross-thread cross-reference.** insight-recurrence's closure (= 01KR3DFHA7FRV3BXEH2Z8SFJQN) flagged that *"Pattern 1 (bundle-as-shape) is uncorrelated with stated plans... no ROADMAP entry or charter section addresses bundling-discipline."* That observation is structurally adjacent to this entry's positive-vs-negative framing: the charter's positive framing speaks to the bidirectional-awareness bet; the non-goals speak to capabilities-not-pursued. Bundling-discipline (insight-recurrence's Pattern 1) is at neither register and therefore uncorrelated with stated plans, as insight-recurrence's closure named factually.

**Boundary with `insight-emergent-properties`.** The cleanness of the non-goals trajectory is the most acute tier-4 temptation in this entry: the temptation to name *what kind of project* produces uniform-honor on six negative recommendations and partial-shape on positive recommendations is high. The trajectory thread states the count (six honored; the positive-framing's substrate-vs-registration asymmetry); the property name is `insight-emergent-properties`'s.

Provenance:
- `ROADMAP.md:3-23` current state — charter sentences quoted verbatim above.
- `ROADMAP.md:426-447` current state — six non-goals quoted verbatim above.
- `bitbucket-pipelines.yml` — macOS / Linux matrix only (per `onboarding-test-surface` references); no `windows-latest` job.
- `deny.toml:72-94` — five long-lived advisory ignores; non-goal-aligned with proportionate-supply-chain framing.
- `onboarding-product-charter` entry 0 = 01KR0P18MCE1H57Q5ZTAGKAJNH (charter source; non-goals enumeration; SLSA-and-mouse non-goals named).
- `onboarding-risk-register` entry 0 = 01KR0P9JC8Z3DF6FQ1GJPF3VKA (drift-finding #4 confirms localization non-execution; advisory-ignore catalogue informs SLSA non-goal honoring).
- `history-arc-07-codex-and-mcp-bridge` framing entry = 01KR2HYMMHAH316CA9KTWKWT6W (groundwork → expansion → closure framing).
- `history-arc-07-codex-and-mcp-bridge` PR #18 entry = 01KR2J1R3HXNZPAHE9118BGBQJ (`Pane::spawn` `context_path` parameter; marker-file convention).
- `history-arc-07-codex-and-mcp-bridge` PR #21 entry = 01KR2J81DHNG4K8NHFVN0XMD1M (codex MCP discovery; second `ensure_*` registration file).
- `history-arc-07-codex-and-mcp-bridge` PR #37 entry = 01KR2JCF7QEJHEG30TVMWY79CQ (peer-agnostic discovery walk; substrate-level widening).
- `history-arc-07-codex-and-mcp-bridge` story-tail entry = 01KR2JM67RTQHQYN0223GTKH1V (substrate-vs-registration observation source).
- `insight-trajectory` document #1 entry = 01KR3ENV1WP6R9SFRE1QME291S (gap-analysis suspect §2 non-goal-honored).
- `insight-trajectory` document #2 entry = 01KR3ESJ42TT0ZGJHGHJ5CTNYC (catalogue skip-vs-adapt asymmetry; complementary observation).
- `insight-recurrence` closure entry = 01KR3DFHA7FRV3BXEH2Z8SFJQN (Pattern 1 uncorrelated-with-stated-plans observation; cross-reference).
- `insight-trajectory` framing entry = 01KR3EJ0RWZXEBMYHY9EEZQX4A.

<!-- Entry-ID: 01KR3EZDWSTW7TPWBY7KXB0KB3 -->

---
Entry: Claude Code (caleb) 2026-05-08T09:35:14.840281+00:00
Role: scribe
Type: Note
Title: Document #6: the v2.0 framing — within-window observation only; v2.0 not landed at v1.41.24, with pre-staging visible

Spec: scribe

tags: #insight #trajectory

**Stated-plan content.** The v2.0 framing at `ROADMAP.md:472-476`, verified at current state:

> *"The v2.0 version bump is a signaling choice as much as a semver one. The tool has been shipping 1.x for a while, but the MCP positioning shift + public distribution justifies a major bump to mark the transition."*

The release ladder around v2.0 at `ROADMAP.md:455-471`:

> *"v1.8 (current) -- Writable MCP actions... v1.7 -- Performance refactor... v1.9 -- Distribution track. Release automation, macOS notarization, Homebrew tap, asciinema demo. v2.0 -- Public distribution launch. Gated on: thesis-track items #1-#2 shipped (session forking, prompt templates), remaining Distribution track complete. External announcement: TripStack engineering blog post, optional Show HN. Target: mid-to-late May 2026."*

The v2.0 plan has three load-bearing properties: it is *signaling-as-much-as-semver*; it ties to MCP positioning shift plus public distribution; it has a target window of *mid-to-late May 2026*.

**The drift-vs-actual that this entry confronts before tracking trajectory.** The release-ladder narrative names "v1.8 (current)" and lists v1.7 as the prior step, with v1.9 as the next step. The actual `Cargo.toml:3` value at the eight-arc record's terminus is `1.41.24` (per arc 08 PR #31's diff inspection plus arc 07 PR #37's diff inspection). The release-ladder section's "v1.8 (current)" is stale relative to the post-window state by approximately 33 minor versions. The release-ladder's *narrative* and the *Cargo.toml truth* are decoupled. The trajectory observation here treats the *v2.0 framing paragraph* (the signaling-choice-and-target-window framing at `ROADMAP.md:472-476`) as the load-bearing stated-plan, since it is the maintainer-authored framing that survives unmodified at current-state and is internally coherent regardless of where the version-current-state sentence is.

**Within-window trajectory.**

The 22-day window closes at v1.41.24 on 2026-05-07. v2.0 has not landed. Trajectory disposition for the v2.0 framing: **NOT LANDED WITHIN WINDOW**.

Pre-staging is visible at multiple surfaces. Per arc 01's framing and the `onboarding-release-process` seed:

- *Arc 01's PR #2 (`chore/ci-hygiene`, 2026-04-30)* — `make check` and target cache scaffolding for CI pipelines that public distribution would consume.
- *Arc 01's PR #3 (`chore/security-hygiene`, 2026-04-30)* — `SECURITY.md` and `cargo-deny` plus `--locked`. Public-distribution disposition: SECURITY.md is the responsible-disclosure surface a public distribution would point at.
- *Distribution-related Make targets* — `make dist-checksums`, `make dist-sign` infrastructure named in the brief and consistent with the `ROADMAP.md:462-463` "v1.9 -- Distribution track. Release automation, macOS notarization, Homebrew tap, asciinema demo" framing. The release-automation surface is pre-staged within the window even as the version stays in v1.41.x.

The pre-staging is observable in arc 01; the v2.0 cut is not landed. Trajectory observation: pre-staging visible; the v2.0 cut itself sits at the window's edge.

**The window's last day (2026-05-07) is also the start of the v2.0 target window ("mid-to-late May 2026").** The eight-arc record terminates as the v2.0 target window opens. The within-window observation cannot extend past the trajectory's terminus; whether v2.0 lands in the next ~7-21 days beyond the window is *outside scope* for this thread (tier-5 forward prediction, reserved for `insight-emergent-properties` if it cites a specific recurrence pattern as the basis for extrapolation).

**The intensity-per-day denominator question (per insight-drift Pattern F).**

Insight-drift's Pattern F entry (= 01KR3BN3N6YF60414FFVHAM50Y) named the spine's *"22-day window"* phrasing as project-age rather than merge-window — the actual merge-window is ~7-8 calendar days. For the v2.0 trajectory, the merge-window denominator is the relevant one: 36 PRs over ~7-8 calendar days is ~4.5-5 PRs per calendar day, sustained merge cadence. Whether this cadence is sufficient to land v2.0's gate items ("thesis-track items #1-#2 shipped (session forking, prompt templates), remaining Distribution track complete") in the next 7-21 days is a tier-5 question and forbidden here.

The within-window observation is the load-bearing one: at the window's terminus, *(a)* the v2.0 framing has not landed; *(b)* the pre-staging is visible at multiple infrastructural surfaces; *(c)* the gate items the v2.0 framing names (session forking, prompt templates, remaining Distribution track) are not narratable as landed in any per-PR entry; *(d)* the target window (mid-to-late May 2026) starts at the eight-arc record's terminus.

**The signaling-as-semver framing (the second load-bearing fragment).**

The v2.0 paragraph names the version bump as *"a signaling choice as much as a semver one."* The paragraph anchors the signaling to *"MCP positioning shift + public distribution"*. The MCP positioning shift trajectory is treated at this thread's document #4 entry (= 01KR3EZDWSTW7TPWBY7KXB0KB3 — the charter's substrate-level widening; codex as second peer; PR #37's project-scoped discovery). The public-distribution side is not narratable as landed in the per-PR entries: no public-binary distribution PR, no Homebrew tap PR, no notarization PR lands within the window. The signaling-as-semver framing has *one* of its two anchor surfaces (MCP positioning shift) executing partially within the window; the *other* anchor surface (public distribution) does not execute within the window.

Trajectory observation for the signaling fragment: half-anchored within window; the second anchor's execution sits beyond the window. This is *not* a within-window divergence (the v2.0 framing names mid-to-late May 2026 as the target, which sits at the window's edge); it is a within-window-vs-anchored-future read. The trajectory thread states the half-anchoring; the *forward* prediction of when the second anchor lands is forbidden here.

**Cross-thread cross-reference.**

`insight-recurrence` Pattern 5 (v1.41.x patch cadence; this thread's document #7 entry) reads against the within-window v2.0-not-landed observation. The cadence shipped 24 consecutive v1.41.x patches; no minor cut and no major cut landed after PR #13's v1.41.0 on Day 4. Whether the patch-only cadence reflects a *holding pattern* before the v2.0 gate or a *steady-state* unrelated to v2.0 is *unstated* — there is no maintainer-authored document linking the patch cadence to the v2.0 gate. The trajectory thread observes the parallelism (the cadence shipped while v2.0 was framed-but-not-landed); the linkage is absent from any stated plan.

**Boundary with `insight-emergent-properties`.**

The strongest tier-5 temptation in this entry is to extrapolate from the pre-staging visibility plus the cadence pattern to a forward prediction of v2.0 landing within the v2.0 target window. That extrapolation is forbidden here. The within-window observation is the load-bearing observation: *v2.0 not landed at v1.41.24; pre-staging visible; signaling fragment's MCP-positioning anchor partially executed at substrate-level; signaling fragment's public-distribution anchor not executed within window; the v2.0 target window begins at the eight-arc record's terminus*. That is the trajectory at tier-3.

Provenance:
- `ROADMAP.md:472-476` current state — v2.0 paragraph quoted verbatim above; signaling-as-semver framing.
- `ROADMAP.md:455-471` current state — release ladder; v2.0 gate items quoted verbatim above ("session forking, prompt templates, remaining Distribution track complete"); target "mid-to-late May 2026."
- Arc 01 framing entry (foundation-hygiene; PR #2 / PR #3 / PR #4) — pre-staging observable at CI hygiene plus `SECURITY.md` plus `cargo-deny` plus `--locked` surfaces.
- `onboarding-release-process` thread — local-only release flow via SSH cloning plus `make install`; SemVer enforcement via PRs; pre-staging context.
- `Cargo.toml:3` post-window state — `version = "1.41.24"` (verified at arc 07 PR #37 entry = 01KR2JCF7QEJHEG30TVMWY79CQ and arc 08 PR #31 entry = 01KR397RTYNS34SAGM46YJJRBY).
- `history-arc-07-codex-and-mcp-bridge` PR #37 entry = 01KR2JCF7QEJHEG30TVMWY79CQ (MCP positioning shift; substrate-level widening; one anchor of signaling fragment executes partially within window).
- `insight-drift` Pattern F entry = 01KR3BN3N6YF60414FFVHAM50Y (merge-window denominator question; intensity-per-day reading).
- `insight-recurrence` Pattern 5 entry = 01KR3D8RH5DNYC37WSGFVETXT3 (v1.41.x cadence; cross-reference for parallelism with v2.0-not-landed observation).
- `insight-trajectory` document #4-and-#5 entry = 01KR3EZDWSTW7TPWBY7KXB0KB3 (charter trajectory; MCP positioning shift; cross-reference for signaling fragment's first anchor).
- `insight-trajectory` framing entry = 01KR3EJ0RWZXEBMYHY9EEZQX4A.

<!-- Entry-ID: 01KR3F1TDZQGAQNZYEYYMFAGCE -->

---
Entry: Claude Code (caleb) 2026-05-08T09:36:24.378331+00:00
Role: scribe
Type: Note
Title: Document #7: the v1.41.x patch cadence — no stated plan to track; the trajectory thread declines to claim trajectory and names the absence factually

Spec: scribe

tags: #insight #trajectory

**Stated-plan content.** The v1.41.x patch cadence is not anchored in any stated plan. There is no spyc-internal SemVer policy document. The closest stated reference at `ROADMAP.md:451-453`:

> *"Semver per `CONTRIBUTING.md`. Version bumps in `Cargo.toml` as part of the PR that ships the change. The `CHANGELOG.md` entry lands in the same commit."*

The CONTRIBUTING.md reference is a procedural rule (versions bump in the same PR that ships the change; CHANGELOG lands in the same commit) — not a policy on *when* a minor versus a patch is justified. Standard SemVer-conventions semantics apply by reference, but spyc does not author a project-level rule on minor-vs-patch granularity. The cadence is *unstated*.

**Why this entry exists despite the absence-of-plan.** insight-recurrence Pattern 5 (= 01KR3D8RH5DNYC37WSGFVETXT3) catalogued the cadence as a *recurrence* observation. Insight-recurrence's closure (= 01KR3DFHA7FRV3BXEH2Z8SFJQN) flagged the cadence as *"correlated with the SemVer policy that exists nowhere as a stated plan but is observed everywhere as a working pattern"* and explicitly handed forward to this trajectory thread the question: *"was the post-v1.41.0 patch-only cadence anticipated, or did it emerge from the work itself?"* The brief named the question and recommended this thread *not* claim trajectory where there is no plan to track, and *name the absence-of-plan-to-track* explicitly.

This entry executes that recommendation.

**The cadence as recurrence-pattern, factually.** Verified at insight-recurrence Pattern 5 entry: 4 minor cuts (v1.38.0, v1.39.0, v1.40.0, v1.41.0) cluster across PRs 6, 8, 10, 13 — the first six wall-clock days of the window. After v1.41.0 lands, no further minor cuts occur. 24 consecutive v1.41.x patches land across PRs 14 through 37, distributed across arcs 03, 04, 05, 06, 07, and 08. The closing version is v1.41.24 at PR #37.

These are *what shipped, observed factually*. The cadence's load-bearing properties:

- 24 consecutive patches under one minor (the largest contiguous patch-only stretch in the window).
- Four minor cuts cluster in the first 48 hours of the window.
- No major bump within the 22-day window (v2.0 sits at the window's edge per this thread's document #6 entry = 01KR3F1TDZQGAQNZYEYYMFAGCE).
- No spyc-internal SemVer policy document anchors any of the above as expected behavior.

**Trajectory disposition: NO STATED PLAN TO TRACK.**

The trajectory thread declines to claim trajectory against the cadence because no stated-plan document specifies the cadence as expected. The cadence is observable *in the post-PR Cargo.toml record*; it is not anchored in a stated *plan*. Reading the cadence as honoring SemVer's broad pre-1.0-or-post-1.0 conventions is plausible interpretation, but interpretation against an *implicit* convention is not the same kind of trajectory observation as interpretation against an explicit project-level policy.

The framing entry's recommendation: *"name the cadence factually, name its load-bearing properties (24 consecutive patches; minor cuts cluster early; no major bump within the 22-day window), and *not* make a trajectory-tracks-plan claim where there's no plan to track."* This entry executes the recommendation.

**The cadence-as-trajectory question (the boundary case).**

A reader inclined to claim trajectory could argue two paths:

- *Argument A: the cadence honors SemVer as imported policy.* `ROADMAP.md:451-453` invokes SemVer by reference; SemVer's conventions arguably anticipate the patch-cluster shape (capability additions are minor; refinements are patches; in a single-developer post-1.0 codebase, refinements tend to dominate). The cadence-as-trajectory reading would name *"SemVer-as-imported-policy was honored exactly: capability-introducing PRs cut minors (PR #6 zoom; PR #8 harpoon; PR #10 quickselect; PR #13 graveyard); refinement PRs cut patches"*. That reading is a real reading, and it is *interpretation*, not stated-trajectory.

- *Argument B: the cadence reveals a project-shape pattern with no stated origin.* The 24-consecutive-patch stretch is the load-bearing recurrence; the unstated nature of the SemVer policy distinguishes it from a tracked-stated-plan. The cadence-as-pattern reading is what insight-recurrence Pattern 5 already does at tier-2.

The trajectory thread *cannot adjudicate* between Arguments A and B without crossing into tier-4 territory (claiming the cadence reflects an emergent property of the working register, the SemVer-by-import discipline, etc.). This entry holds at the *named-the-absence-of-stated-policy* observation. The cadence is what shipped, not what was promised. The recurrence Pattern 5 entry counts the cadence; this entry confirms there is no stated plan to track at the trajectory grain; insight-emergent-properties (next thread) will name what kind of property the cadence reveals.

**The cross-arc cadence distribution.**

Per Pattern 5's verification: minor cuts land in three arcs (arc 03 once at PR #6; arc 06 twice at PR #8 and PR #10; arc 08 once at PR #13). Arcs 01, 02, 04, 05, 07 ship zero minors in the 22-day window. Arc 04 (git-integration) is notable for not getting a minor: the five arc-04 PRs ship at v1.37.1, v1.38.1, v1.41.2, v1.41.11, v1.41.14 — all patches. Arc 05 (pager-surface) is similarly all-patches across its eight PRs.

Whether arc-affiliation correlates with minor-vs-patch disposition is *observable* at the recurrence grain (Pattern 5 named the count); the *interpretation* of why some arcs get minors and others don't is tier-4 (or tier-3 only if a stated plan binds arc-affiliation to release-cadence; verification: no stated plan does so).

**Cross-thread cross-reference: insight-recurrence's closure observation.**

Insight-recurrence's closure entry (= 01KR3DFHA7FRV3BXEH2Z8SFJQN) named Pattern 5's tier-3 disposition as *"correlated with the SemVer policy that exists nowhere as a stated plan but is observed everywhere as a working pattern."* This entry confirms the named-correlation factually: *"observed everywhere as a working pattern; nowhere as a stated plan; trajectory thread declines to claim trajectory."* The closure's observation and this entry's confirmation form the boundary rule for cadence-trajectory questions: *if the policy is unstated, name the absence; do not claim trajectory*.

**Boundary with `insight-emergent-properties`.**

The cadence's emergent-property reading is reserved for `insight-emergent-properties`. Possible property namings flagged by insight-recurrence Pattern 5 itself: *"capability-introductions cluster early; later-window work is refinement-and-correction"*; *"the maintainer's SemVer policy treats minor as capability-additions only"*; *"the post-v1.41.0 work is by-shape patches even when the diff weight is feature-comparable to earlier minors."* All three are candidate property names; this trajectory entry cannot select among them, and the property naming is `insight-emergent-properties`'s.

The strongest tier-3-discipline test moment of this thread is at this entry: the temptation to interpret the cadence as honoring an implicit SemVer policy is high; the trajectory thread's discipline is to refuse the interpretation and *name the absence-of-stated-policy* instead.

Provenance:
- `ROADMAP.md:451-453` current state — Semver-by-reference framing quoted verbatim above; the closest stated SemVer-policy text in the project.
- `Cargo.toml:3` window-trajectory — version-tag map verified per insight-recurrence Pattern 5 entry plus arc 02 PR #5 entry (v1.37.3 verification) plus arc 07 PR #37 entry (v1.41.24 terminus) plus arc 08 PR #31 entry (v1.41.18 trio bump).
- `insight-recurrence` Pattern 5 entry = 01KR3D8RH5DNYC37WSGFVETXT3 (cadence-as-recurrence catalogue; minor-cut distribution across arcs; 24 consecutive patches).
- `insight-recurrence` closure entry = 01KR3DFHA7FRV3BXEH2Z8SFJQN (closure's "correlated with the SemVer policy that exists nowhere as a stated plan" observation; cross-thread observation directly handed forward).
- `history-overview` segmentation entry = 01KR0TWHTC1MPK4KJ08Y9SPE6P (full version-to-PR mapping; one source of truth for the version ladder).
- `insight-trajectory` document #6 entry = 01KR3F1TDZQGAQNZYEYYMFAGCE (v2.0 framing not landed within window; cross-reference for parallelism with cadence).
- `insight-trajectory` framing entry = 01KR3EJ0RWZXEBMYHY9EEZQX4A (recommendation to name the absence-of-stated-plan rather than claim trajectory).

<!-- Entry-ID: 01KR3F3Y55NW2GAC0GY05FCYAD -->

---
Entry: Claude Code (caleb) 2026-05-08T09:37:37.669339+00:00
Role: scribe
Type: Note
Title: Document #8: the cargo-deny advisory-ignore catalogue — five ignores survive the window's largest dep change; stated state holds

Spec: scribe

tags: #insight #trajectory

**Stated-plan content.** The `onboarding-risk-register` seed (entry 0 = 01KR0P9JC8Z3DF6FQ1GJPF3VKA) catalogues five long-lived `cargo-deny` advisory ignores at `deny.toml:72-94` with documented reasons. Verified at the seed entry verbatim:

> *"`cargo-deny` advisory ignores in `deny.toml:72-94` are five long-lived transitive issues (time/yaml-rust/bincode/paste/serial). Each carries a documented `reason`. New ignores must follow that pattern; don't silently add a bare ID."*

The five ignores, per the seed's catalogue plus verification through the per-PR entries' BUGS.md and Cargo.lock readings:

- *RUSTSEC-2026-0009* — `time` via `syntect → plist`. Transitive.
- *RUSTSEC-2024-0320* — `yaml-rust` via `syntect`. Transitive.
- *RUSTSEC-2025-0141* — `bincode` via `syntect`. Transitive.
- *RUSTSEC-2024-0436* — `paste` via `ratatui`. Transitive.
- *RUSTSEC-2017-0008* — `serial` via `portable-pty`. Transitive.

The seed entry frames the catalogue as a *stated state of acceptance* — each ignore carries a `reason` field that documents the transitive-issue context plus the maintainer-authored disposition (deferred until upstream fixes; not load-bearing for the project's own surface; etc.). The catalogue is itself the stated plan: the five ignores are accepted, and new ignores must follow the documented-reason pattern.

**Window-trajectory observation.**

The 22-day window's largest dep change is PR #31 (= 01KR397RTYNS34SAGM46YJJRBY): the trio bump of `vt100 0.15 → 0.16`, `ratatui 0.29 → 0.30`, `ansi-to-tui 7 → 8`, plus the dep-graph trio's transitive churn (839 lines of `Cargo.lock` change). The forcing function is `unicode-width ≥0.2.1` from `vt100 0.16` versus `=0.2.0` from `ratatui 0.29`. Two of the five advisory ignores reference crates in this dep family:

- *RUSTSEC-2024-0436* (`paste` via `ratatui`) — the seed entry flagged this as "transit-via-ratatui."
- *RUSTSEC-2025-0141* (`bincode` via `syntect`) — `syntect` was also affected by the trio's dep churn (the seed names `syntect` as the source of three ignores: `time`, `yaml-rust`, `bincode`).

PR #31's diff against `deny.toml`: empty. The arc 08 PR #31 entry (= 01KR397RTYNS34SAGM46YJJRBY) verifies: *"`git diff 105db8d^1..105db8d^2 -- deny.toml` is empty. The five long-lived `cargo-deny` advisory ignores from the `onboarding-risk-register` seed entry 0 catalogue all survive: RUSTSEC-2026-0009 (time via syntect→plist), RUSTSEC-2024-0320 (yaml-rust via syntect), RUSTSEC-2025-0141 (bincode via syntect), RUSTSEC-2024-0436 (paste via ratatui), RUSTSEC-2017-0008 (serial via portable-pty). The `paste` ignore is specifically transit-via-ratatui per the seed's `reason` field; the ratatui 0.29 → 0.30 bump did not eliminate the `paste` dependency."*

**Trajectory disposition: STATED STATE OF ACCEPTANCE HOLDS ACROSS THE WINDOW'S LARGEST DEP CHANGE.**

Five ignores; zero reduced. The major-version trio bump did not opportunistically resolve any of the five long-lived ignores, despite touching two of the named upstream crates (`ratatui` and the syntect-syndicated set). The seed's catalogue — as both the inventory of five ignores and the stated rule that new ignores follow the documented-reason pattern — survives intact post-PR-#31.

The trajectory observation is structurally cleaner than catalogue-§-trajectory or ROADMAP-additions-trajectory: there is no partial-execution to flag, no direction-alignment-without-execution to name, no deferral-against-conditional to note. The catalogue says "five accepted"; the post-window state shows "five accepted"; zero divergence.

**The asymmetry-of-stable-state observation.**

This entry's trajectory parallels the charter-non-goals trajectory at this thread's document #5 entry (= 01KR3EZDWSTW7TPWBY7KXB0KB3): both are stated *negative-or-stable-state* plans that the window honored uniformly. The charter's six non-goals all honored across the window; the advisory-ignore catalogue's five ignores all survive the trio bump. Both are uniform-honor trajectories.

The structural distinction worth flagging at trajectory-grain: the charter non-goals are *not-pursued* state (no PR adds telemetry, mouse, plugin system, etc.), while the advisory ignores are *not-resolved* state (the dep churn could have eliminated transitive ignores opportunistically; it did not). One is non-execution-against-not-doing; the other is non-execution-against-could-have-done. Both register as uniform-honor against their respective stated plans, but the *space of permissible execution* differs:

- For non-goals: any PR that *adds* the non-goal capability would violate the trajectory. Zero such PRs landed; trajectory honored.
- For advisory ignores: any PR could have eliminated zero, one, or all of the ignores opportunistically (trio bump touched the dep family); zero were eliminated. The trajectory holds at the stated stable-state.

The asymmetry is between *constraints that bind execution* (non-goals) and *constraints whose dissolution would have been possible* (advisory ignores). Both register as honored; the *nature* of the honoring is different. Captured for tier-4 reading at `insight-emergent-properties`.

**Cross-thread cross-reference.**

`insight-recurrence`'s closure (= 01KR3DFHA7FRV3BXEH2Z8SFJQN) flagged Pattern 3 (BUGS.md SMALL/MAYBE-to-FIXED lift) as *"highly correlated with stated plans"* — the lift recurrence is partially a property of the gap-analysis methodology PR #5 introduced. The advisory-ignore trajectory here is *uncorrelated* with that lift recurrence: the five ignores do not appear in BUGS.md as SMALL/MAYBE entries; they live at `deny.toml:72-94` only. The non-correlation is structural — `cargo-deny` advisory ignores live in the supply-chain control surface, not in the user-reportable bug catalogue. The two stated-plan registers (BUGS.md SMALL/MAYBE versus `deny.toml` advisory ignores) speak to different surfaces and have different recurrence-and-trajectory dispositions.

The arc 08 PR #28 (huge directory cap) and PR #31 (trio bump) entries are the ones most adjacent to the supply-chain surface; neither names the advisory ignores. The seed entry 0 is the only stated-plan document at the catalogue-level. Captured factually.

**Boundary with `insight-emergent-properties`.**

The cleanness of the advisory-ignore trajectory plus the uniformity of the charter non-goals trajectory plus the asymmetry between not-pursued versus could-have-done is the most acute tier-4 temptation in this entry. The temptation to name *what kind of stated-state-discipline* produces uniform-honor across two surfaces is high. The trajectory thread states the count (five ignores survive; zero reduced; trio bump touched the dep family without opportunistic resolution); the property name is `insight-emergent-properties`'s.

Provenance:
- `deny.toml:72-94` current state — five long-lived advisory ignores; per-ignore `reason` field documenting transitive-issue context.
- `onboarding-risk-register` entry 0 = 01KR0P9JC8Z3DF6FQ1GJPF3VKA — the catalogue source; "five long-lived transitive issues (time/yaml-rust/bincode/paste/serial)" verbatim.
- 105db8d (PR #31 chore/vt100-and-ratatui-upgrade, 2026-05-06) — the window's largest dep change.
- `git diff 105db8d^1..105db8d^2 -- deny.toml`: empty (per arc 08 PR #31 entry verification).
- `history-arc-08-recoverability-and-deps` PR #31 entry = 01KR397RTYNS34SAGM46YJJRBY (trio bump structure; `deny.toml` empty diff; advisory-ignore catalogue verification).
- `insight-recurrence` closure entry = 01KR3DFHA7FRV3BXEH2Z8SFJQN (Pattern 3 stated-plan correlation; non-correlation with this trajectory).
- `insight-trajectory` document #4-and-#5 entry = 01KR3EZDWSTW7TPWBY7KXB0KB3 (charter non-goals trajectory parallel; uniform-honor cross-reference).
- `insight-trajectory` framing entry = 01KR3EJ0RWZXEBMYHY9EEZQX4A.

<!-- Entry-ID: 01KR3F65XSQ4B74MDSQMYEM2R0 -->

---
Entry: Claude Code (caleb) 2026-05-08T09:39:24.688280+00:00
Role: scribe
Type: Closure
Title: Closure: insight-trajectory catalogue assembled — eight stated-plan documents handled across six per-document entries; observations flagged for insight-emergent-properties

Spec: scribe

tags: #insight #trajectory

The catalogue is assembled. Eight stated-plan documents across six per-document entries plus framing. Per-document trajectory dispositions:

- **Document #1 (gap-analysis Top Suspects)** — three for three on suspect-resolution-or-deferral (§1 RESOLVED with durable-record incompleteness; §2 DEFERRED-AS-NON-GOAL; §3 RESOLVED with test-coverage gap). Methodology #9 folded in: NAMED-NOT-CITED.
- **Document #2 (UX catalogue)** — 4-of-4 skips honored exactly; 0-of-4 adapts executed-as-specified, 1-of-4 partial (§2), 1-of-4 non-executed-against-conditional (§5), 2-of-4 non-executed (§3-tip-half; §4-specific-shape). Skip-vs-adapt asymmetry the load-bearing observation.
- **Document #3 (ROADMAP additions)** — three positive-recommendation entries; zero exactly-as-specified executions. One PARTIAL (#2); two deferred (#1 DIRECTION ALIGNMENT BY FOUR PRs; #3 NON-EXECUTED-AGAINST-CONDITIONAL).
- **Documents #4 + #5 (charter + non-goals)** — substrate-level widening with registration-level peer-specificity (charter's "Claude" word slightly modifiable but not generalized); six non-goals all honored across the window (the cleanest uniform-honor trajectory).
- **Document #6 (v2.0 framing)** — NOT LANDED WITHIN WINDOW; pre-staging visible; signaling fragment's MCP-positioning anchor partially executed at substrate-level; public-distribution anchor not executed within window.
- **Document #7 (v1.41.x cadence)** — NO STATED PLAN TO TRACK; cadence is what shipped, not what was promised; trajectory thread declines to claim trajectory and names the absence factually.
- **Document #8 (advisory-ignore catalogue)** — STATED STATE OF ACCEPTANCE HOLDS across the window's largest dep change; five ignores survive the trio bump.

**What this catalogue contributes to the network.**

- *A name for the catalogue's load-bearing asymmetry.* The 22-day window's stated-plan-vs-execution shape carries a *negative-vs-positive asymmetry* across multiple surfaces: charter non-goals (six honored) plus catalogue skip recommendations (four honored) all uniformly honored; charter positive framing (substrate-level partial widening) plus catalogue adapt recommendations (four with shape) plus ROADMAP additions (three with shape) all in modified shape, none exactly-as-specified. Skip-honored-exactly is consistent across two stated-plan documents; adapt-all-modified is consistent across three. The asymmetry is the catalogue's load-bearing observation at trajectory-grain.

- *A confirmation of the longest-single-trajectory hypothesis.* The framing entry posited the gap-analysis suspects three-for-three trajectory as the longest single trajectory in the network. Document #1's verification confirms the hypothesis: three suspects in one document; eight calendar days from specification (PR #5 on Day 0) to terminal disposition (§1 at PR #29 on Day 6; §2 deferred-as-non-goal across the window; §3 at PR #31 on Day 6). Two arcs touched for resolution; one arc of non-execution-as-honored (the entire window, against the charter non-goal). The catalogue's seven-section trajectory does not exceed this longevity — its dispositions cluster around DIRECTION ALIGNMENT (§4) rather than terminal-RESOLVED.

- *A factual placement decision for the v1.41.x cadence question.* insight-recurrence's closure handed the cadence forward as "correlated with the SemVer policy that exists nowhere as a stated plan." This thread's document #7 entry confirms the absence factually: there is no stated plan to track. The cadence is recurrence-grade observation (Pattern 5); it is not trajectory-grade observation. Both readings are honest; the boundary between them is the *stated-plan-existence* question.

- *A clean tier-3-vs-tier-4 boundary at six sites.* Each per-document entry flagged the strongest tier-4 temptation it faced. Document #2's adapt-all-modified ratio; Document #3's three-zero ratio; Document #5's six-honored uniformity asymmetry against positive framing; Document #6's signaling-fragment partial anchoring; Document #7's interpretation-against-implicit-SemVer-policy temptation; Document #8's stated-state-discipline temptation. All six held to tier-3 by stating the count and refusing the property name. The tier-3 discipline is most acute at document #3 (three positive-recommendation ROADMAP entries; zero exactly-as-specified executions) where the ratio's interpretive pull is highest.

**Cross-thread observation for `insight-emergent-properties`'s author (Phase 10D).**

`insight-emergent-properties` will read the catalogue's tier-3 trajectory observations and ask *what kind of property each trajectory is*. The catalogue does not pre-name the properties — that is the next thread's job — but flags which trajectory observations have tier-4 weight, which are tier-3-only, and which are candidates the catalogue gave the author explicit material on.

**Tier-4 candidates flagged with explicit data:**

- **The catalogue skip-vs-adapt asymmetry (document #2).** 4-of-4 skips honored exactly; 0-of-4 adapts executed-as-specified, 4-of-4 in modified shape. The data is rich enough that the property name could be *the catalogue is more reliable as a refusal mechanism than as an execution mechanism*; or *positive recommendations land in modified shape because adapt-pattern executes against the maintainer's working surfaces, while skip-recommendations land exactly because they require no execution at all*; or any other framing that names the asymmetry as a property. The trajectory thread states the count; the emergent-properties thread can name.

- **The charter non-goals plus advisory-ignore catalogue uniform-honor (documents #5 and #8).** Six non-goals all honored; five advisory ignores all survive the window's largest dep change. The trajectory parallels are real; the *kind* of honor differs (constraints-that-bind versus constraints-whose-dissolution-would-have-been-possible). The emergent-property reading would name *what kind of stated-state-discipline produces uniform-honor*; this trajectory thread sketched the asymmetry between *not-pursued* (non-goals) and *not-resolved* (advisory ignores).

- **The DIRECTION ALIGNMENT four-PR cluster against catalogue §4 (documents #2 and #3).** Four PRs across two arcs hold §4 alignment from two different families (pager-as-mode side at arc 05; standalone-overlay side at arc 06); zero execute the `PagerView::picker_items: Vec<(Label, Action)>` shape. The data is rich (four PRs, two families, one bracket-open-at-window-terminus). The emergent-property reading would name *what kind of execution dynamic produces parallel-not-substrate execution* or *the picker shape lives at multiple surfaces because surface-specific instantiation is the project's working pattern*. The trajectory thread states the count; the property name is the next thread's.

- **The substrate-vs-registration distinction in the charter trajectory (document #4).** The charter's "Claude" word widens at substrate-level (peer-agnostic discovery); stays peer-specific at registration-level (two `ensure_*` files side-by-side). The emergent-property reading would name *what kind of architectural discipline produces substrate-generalize-but-registration-keep-specific*; possibilities include "the registration surface is intentionally peer-specific because each peer's config-file convention is its own contract"; "substrate widens cheaply, registration widens at the cost of supporting infrastructure each peer's tooling expects"; etc. The trajectory thread sketched the asymmetry; the property name is reserved.

**Tier-3-only observations:**

- **The longest-single-trajectory observation (document #1).** Three suspects, three dispositions, eight calendar days, two arcs. The longevity is structural; the *property* of why the gap-analysis suspects produced a complete trajectory while the catalogue's adapt recommendations did not is tier-4 territory but the *count* is tier-3 only. Per-suspect dispositions are tier-3.

- **The verbatim-import observation (document #3).** Three ROADMAP entries verbatim-imported from the catalogue's "Top 3 to consider first." The verbatim-importation itself is fact at tier-3; whether it reflects an emergent property of the planning-vs-execution dynamic is tier-4 and reserved.

- **The methodology-named-not-cited observation (document #1).** PR #5's empirical-verification methodology was named in the gap analysis; the actual empirical run is not narratable from any per-PR entry. The named-not-cited fact is tier-3; whether it reflects an emergent property of *what kind of evidence the project treats as load-bearing* (commit-level diff comparison versus per-PR empirical trace) is tier-4 and reserved.

**Boundary cases — observations that should NOT be promoted to tier-4:**

- **The v1.41.x cadence as trajectory.** Document #7 declined to claim trajectory because no stated plan anchors the cadence. The emergent-properties thread can name what kind of property the cadence reveals (Pattern 5 already gave the recurrence-side framing; multiple candidate property names flagged at insight-recurrence's closure). But the trajectory thread *did not extend Pattern 5 into a claimed trajectory*; the emergent-properties thread should not retroactively promote it to a stated-plan trajectory either. The honest framing is *"observed everywhere as a working pattern; nowhere as a stated plan."*

- **The v2.0 forward prediction.** Document #6 stated within-window observations only. The v2.0 target window (mid-to-late May 2026) starts at the eight-arc record's terminus. Forward predictions of v2.0 landing within the target are tier-5 and require citing a specific recurrence as the basis for extrapolation. The emergent-properties thread can do this if it cites Pattern 5 (cadence) or the pre-staging-visibility observation (per arc 01) as the basis; it should not extrapolate without citation.

**Per-document trajectory disposition counts, final.**

- *Executed-as-specified*: 0 across all eight documents.
- *Partial / direction-aligned*: 1 (catalogue §2 + ROADMAP entry #2 = same observation); 4 PRs hold DIRECTION ALIGNMENT against catalogue §4 + ROADMAP entry #1; 1 substrate-level partial-widening (charter "Claude" word).
- *Non-executed-against-conditional*: 1 (catalogue §5 + ROADMAP entry #3 = same).
- *Non-executed*: 1 (catalogue §3-tip-half); 1 (v2.0 within-window); §4 specific shape across four PRs.
- *Resolved*: 2 (gap-analysis §1; gap-analysis §3).
- *Deferred-as-non-goal* (uniform honor): 1 (gap-analysis §2 + 6 charter non-goals — same trajectory family).
- *Skip-honored exactly*: 4 (catalogue §1, §3-log-half, §6, §7); 6 (charter non-goals); 5 (advisory ignores).

The catalogue's skip-or-non-goal-or-stated-state honor count: 15 across the window. The catalogue's exactly-as-specified positive-recommendation execution count: 0.

**Voice / register audit, brief.**

The analyst register held throughout. The most acute tier-3-discipline test moments were document #3 (the three-zero positive-recommendation ratio); document #5 (the six non-goals' uniform-honor cleanness); document #7 (the cadence-as-trajectory boundary case). All three held. No motive attribution to the maintainer anywhere. No tier-4 property naming anywhere. No tier-5 forward prediction anywhere.

**The thread is left OPEN.** Cross-thread references from `insight-emergent-properties` will cite this catalogue's per-document entry IDs.

Provenance:
- `insight-trajectory` framing entry = 01KR3EJ0RWZXEBMYHY9EEZQX4A.
- `insight-trajectory` document #1 entry = 01KR3ENV1WP6R9SFRE1QME291S (gap-analysis suspects + methodology).
- `insight-trajectory` document #2 entry = 01KR3ESJ42TT0ZGJHGHJ5CTNYC (UX catalogue).
- `insight-trajectory` document #3 entry = 01KR3EW3166JZ59TDR8PYMGN4T (ROADMAP additions).
- `insight-trajectory` document #4-and-#5 entry = 01KR3EZDWSTW7TPWBY7KXB0KB3 (charter + non-goals).
- `insight-trajectory` document #6 entry = 01KR3F1TDZQGAQNZYEYYMFAGCE (v2.0 framing).
- `insight-trajectory` document #7 entry = 01KR3F3Y55NW2GAC0GY05FCYAD (v1.41.x cadence; absence-of-plan named).
- `insight-trajectory` document #8 entry = 01KR3F65XSQ4B74MDSQMYEM2R0 (advisory-ignore catalogue).
- `insight-drift` closure entry = 01KR3BQHTCQ7YGHWQJ2ZE4PJQ4 (boundary rule for cross-cutting observables).
- `insight-recurrence` closure entry = 01KR3DFHA7FRV3BXEH2Z8SFJQN (cross-thread observations directly handed forward).
- `history-overview` framing entry = 01KR0TRFWT9W6WMFHC49YSW0BG.
- `history-overview` segmentation entry = 01KR0TWHTC1MPK4KJ08Y9SPE6P.
- All eight arc threads' per-PR entries (cited at the relevant per-document entries) are the load-bearing infrastructure this catalogue was built atop.

<!-- Entry-ID: 01KR3F9EF9WF9Q34FRAR2XPSZS -->

---
Entry: Claude Code (caleb) 2026-05-08T09:40:54.747571+00:00
Role: scribe
Type: Note
Title: Tail: trajectory's upstream is the maintainer-authored stated-plan documents themselves — and the per-PR entries' back-reference architecture made cross-document reading legible

Spec: scribe

tags: #insight #trajectory #tail

insight-recurrence's story-tail (= 01KR3DM9DBRV6MBA4D516KRR12) named that *the arc story-tails — the cumulative-grain entries at the arc layer — were that thread's upstream*, in contrast to insight-drift's per-PR drift-findings practice. The asymmetry was structural: drift is per-PR observation; recurrence requires cumulative grain.

This thread's upstream is different again, and the asymmetry is again structural.

The trajectory thread's upstream runs in two layers, and both layers are *outside the per-arc record* in different senses.

**Layer 1 — the maintainer-authored stated-plan documents themselves.** PR #5's two notes files plus the three ROADMAP additions plus the charter at `ROADMAP.md:3-23` plus the non-goals at `:426-447` plus the v2.0 framing at `:472-476` plus the cadence reference at `:451-453` plus the seed entry 0 of `onboarding-risk-register` (the advisory-ignore catalogue). Eight stated-plan documents, all maintainer-authored, all available *outside the network's narration of execution*. None of these documents are arc entries. They are the *inputs to the trajectory thread*, not derivatives of the per-PR entries.

That is what makes trajectory tier-3 work distinctively. The trajectory question is *what does the network of arcs look like against the maintainer's own forward-statements?* — and the maintainer's forward-statements are not at the arc layer. They live in `ROADMAP.md`, `notes/...` (relocated to `BUGS.md`), the charter sentences, the v2.0 paragraph, and the supply-chain config files. The trajectory thread reads *across* the per-PR record, holding the stated-plan documents as the constant register that the arcs execute against. This is structurally upstream of any single arc — like Pattern F (insight-drift's = 01KR3BN3N6YF60414FFVHAM50Y) was for the spine-level framing, the entire trajectory catalogue is upstream of the arcs.

But the per-PR entries are still what made the trajectory readable — that's Layer 2.

**Layer 2 — the per-PR entries' back-reference architecture.** The arc 02 investigation entry (= 01KR0YXXZRQR24CSNAK4Q7808T) is the most load-bearing single piece of infrastructure in the trajectory catalogue. It quotes the gap analysis verbatim, names the catalogue §-disposition assignments, names PRs #20 / #33 / #35 / #8 / #10 as the §2 / §4 alignment partners, and defers §3 explicitly to arc 08. The investigation entry is the *single authoritative reading* the arc record provided of PR #5's two notes files — and almost every per-document entry in this trajectory catalogue cited it.

The investigation entry was not the only critical upstream node. Arc 03's PR #29 entry verifies §1 resolution at code level. Arc 05's PR #20 entry verifies §2 partial. Arc 05's PR #33 entry plus PR #35 entry verify §4 DIRECTION ALIGNMENT (pager-as-mode side). Arc 06's PR #8 entry plus PR #10 entry verify §4 PARALLEL PATTERN (standalone-overlay side). Arc 07's PR #37 entry verifies the substrate-level widening of the charter's "Claude" word. Arc 08's PR #31 entry verifies §3 resolution plus advisory-ignore stability. The arc 05 closure (= 01KR2AJVZA1E85YSKHF4FNRQQ3) and arc 05 story-tail (= 01KR2ANRAEFWWR5W9FQP11A0DB) carry the cumulative §4 reading at arc-grain. The arc 07 framing entry plus story-tail carry the substrate-vs-registration distinction.

The structure was deliberate per the arc 02 framing entry's mandatory back-reference contract (cited from `history-overview` PR #5 special-handling entry): downstream PRs that execute against PR #5's stated-plan content *must* back-reference arc 02. PR #20, PR #33, PR #35, PR #8, PR #10, PR #29, PR #31 all do. The trajectory thread did not need to derive each per-PR's relationship to PR #5 from diff inspection; the arc record had already done so, and the per-PR entries cited the arc 02 investigation entry as the canonical source.

**The asymmetry between this thread's upstream and insight-recurrence's.** Insight-recurrence's tail named the upstream as the *story-tails* — the cumulative-grain entries at the arc layer. This thread's upstream is two-layered: the *stated-plan documents* (outside the per-arc record entirely) plus the *per-PR back-references* (at the arc layer but with their cumulative-grain implications resolved at the arc 02 investigation entry plus arc 05 / arc 07 closure entries). The story-tails were *secondarily* upstream — the arc 04 story-tail (= 01KR13CJ5XS5VREYA4741JHDSQ) at machinery-chains; the arc 07 story-tail at substrate-vs-registration; the arc 08 story-tail at the eight-arc cumulative reading — but the *primary* upstream was the per-PR entries' back-reference architecture against the stated-plan documents.

This is what makes trajectory work uneven across documents. Documents #1, #2, #3 (the lazygit corpus) had the densest per-PR back-reference architecture — six PRs across four arcs with mandatory back-references to PR #5's gap analysis or catalogue. Reading the corpus's trajectory was almost entirely a matter of assembling the back-references the per-PR entries had already laid down. Documents #4-#5 (the charter and non-goals) had a thinner back-reference architecture — the charter's "Claude" word is not directly back-referenced by arc 07's PRs; the substrate-vs-registration reading lives at arc 07's story-tail rather than at any PR-level diff comment. Reading the charter's trajectory required cross-source synthesis that the per-PR entries did not pre-stage. Document #6 (v2.0) had no back-reference architecture — no PR back-references the v2.0 framing at all. The trajectory had to be derived from the absence-of-execution against the v2.0 gate items plus the pre-staging-visibility plus the within-window observation. Document #7 (cadence) is *the* case where the absence-of-plan demanded the absence-of-trajectory-claim. Document #8 (advisory ignores) had a single per-PR back-reference (arc 08 PR #31's empty `deny.toml` diff verification).

The trajectory work distributes unevenly across the catalogue: the lazygit-corpus documents are roughly two-thirds-back-reference-assembly, one-third trajectory-grain refinement; documents #4-#8 are roughly one-third-back-reference-assembly, two-thirds cumulative-grain synthesis. The estimate is rough; the asymmetry is real.

**What this means for `insight-emergent-properties`.** That thread's upstream will run heavier still on cumulative-grain synthesis, because emergent properties are tier-4 and structurally upstream of any single arc. The trajectory observations this catalogue produces are themselves upstream material for the emergent-properties thread — the asymmetries (skip-honored-exactly versus adapt-all-modified; non-goals uniform-honor versus charter-positive partial-widening; gap-analysis-suspects three-for-three versus catalogue §4 four-PR DIRECTION ALIGNMENT) are *what insight-emergent-properties has data on*. Where this thread had the stated-plan documents to anchor against, that thread has *this thread's six per-document entries* as a primary anchor. The chain of upstream relationships across the four insight threads — drift relies on per-PR drift-findings; recurrence relies on arc story-tails; trajectory relies on per-PR back-references plus stated-plan documents; emergent-properties will rely on the trajectory catalogue plus the recurrence catalogue plus the drift catalogue — is not coincidence. The four insight threads are sequenced *because each thread's primary upstream is the prior thread's output*, structurally.

The infrastructure-tail forecast at insight-drift (= 01KR3BVVYN37WDWJ5A3D8A5XWH) named the per-PR drift-findings practice as the upstream that made the drift catalogue legible. The recurrence-tail forecast at insight-recurrence (= 01KR3DM9DBRV6MBA4D516KRR12) named the story-tails as the upstream. This trajectory-tail names the per-PR back-reference architecture *plus the stated-plan documents* as this thread's upstream. The emergent-properties tail will run heavier still on cumulative reading; the tail-prediction stack now reads consistently across three threads.

The thread is left OPEN.

Provenance:
- `insight-drift` infrastructure tail = 01KR3BVVYN37WDWJ5A3D8A5XWH (the upstream-naming pattern this tail re-uses with the trajectory upstream).
- `insight-recurrence` story-tail = 01KR3DM9DBRV6MBA4D516KRR12 (story-tails-as-upstream observation; cross-reference for the asymmetry-of-upstreams reading).
- arc 02 investigation entry = 01KR0YXXZRQR24CSNAK4Q7808T (the single most load-bearing piece of infrastructure in this trajectory catalogue).
- arc 03 PR #29 entry = 01KR10G02J2234D0WBMWMYC35M; arc 05 PR #20 entry = 01KR2A6TT516XA5FEGVBXYPWD7; arc 05 PR #33 entry = 01KR2AAX12XSNRNZPTXJT2TXJA; arc 05 PR #35 entry = 01KR2AD5PV989H58E49E5D18NM; arc 06 PR #8 entry = 01KR2GCH3Q8DR9DATBBC802Q8W; arc 06 PR #10 entry = 01KR2GH1D9QCGDPZEMWW09R898; arc 07 PR #37 entry = 01KR2JCF7QEJHEG30TVMWY79CQ; arc 08 PR #31 entry = 01KR397RTYNS34SAGM46YJJRBY (per-PR back-reference architecture).
- `history-arc-05-pager-surface` closure = 01KR2AJVZA1E85YSKHF4FNRQQ3; `history-arc-05-pager-surface` story-tail = 01KR2ANRAEFWWR5W9FQP11A0DB (cumulative §4 reading at arc-grain).
- `history-arc-07-codex-and-mcp-bridge` story-tail = 01KR2JM67RTQHQYN0223GTKH1V (substrate-vs-registration distinction's arc-grain source).
- `insight-trajectory` framing entry = 01KR3EJ0RWZXEBMYHY9EEZQX4A.
- `insight-trajectory` closure entry = 01KR3F9EF9WF9Q34FRAR2XPSZS.

<!-- Entry-ID: 01KR3FC6D4NH972M9MF4NYDYC6 -->
