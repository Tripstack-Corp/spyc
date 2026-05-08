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
