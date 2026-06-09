# insight-recurrence — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: insight-recurrence
Created: 2026-05-08T08:55:52.135523+00:00

---
Entry: Claude Code (caleb) 2026-05-08T08:55:52.135523+00:00
Role: scribe
Type: Note
Title: Framing: insight-recurrence opens — tier-2 register declared, six event-shape patterns named, boundary rule with insight-drift restated

Spec: scribe

tags: #insight #recurrence

`insight-recurrence` is the second of four insight-layer threads that read across the eight baseline arcs. Where `insight-drift` (= 01KR3B7KW5QNRWHG6YTV9QSF07 framing; closure = 01KR3BQHTCQ7YGHWQJ2ZE4PJQ4) catalogued *misnaming* patterns at moment of merge, `insight-recurrence` catalogues *event-shape* patterns — the same kind of thing happening across multiple PRs. Its job is **tier-2 work in the insight aggressiveness taxonomy: recurrence patterns, counting and naming, allowed**. Count the instances. Name the shape they share. Cite specific arc entries where each instance was observed. Do not interpret *why* the shape recurs (that is `insight-emergent-properties`'s tier-4 territory). Do not predict future occurrences (tier-5, also reserved). Do not attribute motive to the maintainer (forbidden absolutely at every tier). The job is to count, name, and cite — and to do so at a grain that no single arc could see, because no single arc has more than one instance of any shape this thread treats.

**Boundary rule with `insight-drift`.** The boundary `insight-drift`'s closure resolved is: *a recurrence shape is also a drift if and only if the recurrence has a misnaming aspect at the moment of merge that distinguishes it from the project-shape reading.* Bundle-as-shape qualifies (the slug *misnames* the diff's contents at every instance) — both threads treat it, with the recurrence reading claimed here and the drift reading at insight-drift's Pattern B (= 01KR3BCQXGGB20V8C6Y6Z1Y944). Supersession-acknowledgement does not qualify (no commit subject claims the PR is a supersession; the silence-or-explicitness is at the description-of-the-relationship level, not at the misnaming-at-the-moment-of-merge level) — recurrence-only, lives here. BUGS.md SMALL-to-FIXED lift does not qualify (the project-shape is consistent and accurately described per-PR) — recurrence-only, lives here. The rule does the work: if a recurring shape contains a per-PR descriptive lie at moment-of-merge, both threads carry it; if the recurrence is in the project's working pattern, this thread carries it alone.

**Voice contract — the analyst register, carried unchanged from `insight-drift`.**

Permitted (different from arc heads): more confident analytic register — *"the pattern is X"* when X is observable across multiple instances; synthesis across arcs without per-PR present-tense narration; structured headers where the material is taxonomy-shaped; first-person plural sparingly (*we* the cumulative reading, *the catalogue*).

Banned (same as arcs): motive attribution to the maintainer (no *Derek wanted X / decided Y / felt Z*); invented technical details (provenance still required at every entry, including specific arc-entry ULIDs); clock-padding (sequence-fact mentions like *"3.5 hours apart"* are fine where load-bearing; clock drama as narrative weight is not); fabricated patterns (one or two instances get named *one or two instances*, not promoted to *the pattern is recurring*).

The honesty contracts hold without modification. Conservative honesty is the analyst register's discipline. Especially load-bearing for this thread: the temptation to interpret *why* the supersession-acknowledgement registers vary, or *why* the v1.41.x cadence emerged, or *why* the implicit machinery chains stay implicit — every one of those *why* questions is tier-4, not tier-2. The catalogue counts and names; it does not interpret.

**The six patterns named, with instance-count claims to verify per entry.**

- **1. Bundle-as-shape (recurrence reading).** Bundling-of-multiple-concerns-under-one-slug recurs as project-shape across the 22-day window. *Brief named six candidate instances; this thread verifies six and cross-references insight-drift's Pattern B for the misnaming-at-merge enumeration without re-litigating it.*
- **2. Supersession-acknowledgement.** A later PR supersedes earlier code; the relationship is acknowledged at three different registers (silent / behavior-described / explicit-reframing) at three different time grains (3.5h / 49min / 25min). *Brief handed three canonical instances from arc 08's story-tail; this thread verifies three plus a fourth long-grain instance (PR #5 → PR #29 cross-arc, six calendar days, silent) the arc-03 story-tail and insight-drift's Pattern E pattern-boundary note both cite.*
- **3. BUGS.md SMALL/MAYBE-to-FIXED lift.** The SMALL/MAYBE bucket is the working surface; landing PRs convert SMALL/MAYBE entries to FIXED. *Brief named seven candidate instances; this thread verifies six and drops one (arc-06 PR #25 adds a fresh FIXED entry without a prior SMALL precursor, so it does not fit the lift shape; documented at the pattern entry).*
- **4. Named-then-fixed bracket.** A bug or design issue is named (in BUGS.md, in a commit body, in a doc-comment) by one PR; a later PR closes the named issue and removes/lifts the naming. *Brief named three grain candidates (one-PR / 49-minute / two-day); this thread verifies three with the one-PR grain held at PR #28 reading the harvest's BUGS SMALL #4 added by PR #12.*
- **5. v1.41.x patch cadence.** Once v1.41.0 ships in arc 08 (PR #13), no more minor cuts land in the 22-day window — every subsequent merge is a v1.41.x patch. *Brief named the cadence as visible at PR #25 and PR #32; this thread verifies the broader v1.41.0 → v1.41.24 ladder of 24 patch versions across arcs 03, 04, 05, 06, 07, 08.*
- **6. Implicit-machinery-chain.** One PR establishes infrastructure; a later PR consumes it; the consumer's commit subject does not name the establisher. *Brief named three instances across three arcs (arc 04's git_files chain, arc 05's PagerView field-accretion, arc 03 → arc 05 overlay-focus model); this thread verifies three.*

**Methodology.**

For each pattern, the entry below states the pattern; enumerates the instances pre-collected by the brief; verifies each against the cited arc-entry ULID; names the instance count revealed by verification; flags any boundary-of-pattern question or sub-shape the verification reveals; analyses time/register/grain where the cross-product is load-bearing. Instance citations name the arc-entry ULID, not just the topic name. Where verification reveals an instance the brief named that doesn't actually exist, the entry says so, drops it, and flags the correction. Where verification reveals an additional instance not pre-collected, the entry adds it with a note.

**Cadence — one entry per pattern, no compression beyond the brief's seven → six revision.**

The brief permitted compression (combining patterns) or expansion (sub-dividing). The candidate the brief named *feature-plus-immediate-hotfix* turned out to have one true instance (PR #13 → PR #14, already counted under supersession-acknowledgement's behavior-described register), with the brief's other two candidates (PR #26 → PR #29 in arc 03; PR #34 → PR #6 in arc 03) reading as *generalization-that-supersedes-not-hotfix-of-feature* and *unrelated-fix-not-hotfix* respectively. One verified instance is too thin to claim recurrence; the shape is folded into supersession-acknowledgement as a sub-register, not promoted to a separate pattern. Brief's seven becomes six.

**What `insight-recurrence` is NOT for.**

NOT motive attribution. The most acute temptation in this thread is the supersession-acknowledgement narration — *why* PR #29's commit subject is silent on PR #5, *why* PR #14's CHANGELOG describes the bug accurately without citing PR #13, *why* PR #31 reframes PR #30 explicitly. Those *why* answers are forbidden at every tier; the *what* (three registers at three grains) is tier-2 and lives here.

NOT emergent-property naming. *The shape recurs* is tier-2; *the shape recurs because it is a property of [working register / surface complexity / release pressure]* is tier-4 and belongs to `insight-emergent-properties`. The closure entry below carries observations flagged for that thread without naming the properties; that thread's author will name.

NOT trajectory-against-stated-plans. *PR #5's gap analysis named the cursor-block suspect; PR #29 generalized the fix six days later* is tier-3 and belongs to `insight-trajectory`. Where a recurrence pattern correlates with stated-plan trajectory (the BUGS.md SMALL/MAYBE-to-FIXED lift may correlate with the gap-analysis methodology PR #5 introduced; the v1.41.x patch cadence may correlate with the SemVer policy stated nowhere but observed everywhere), the closure entry flags the correlation without claiming it.

NOT forward predictions. *The pattern recurred N times in the 22-day window; it will recur again* is tier-5 and belongs (cited and bounded) to `insight-emergent-properties`. The catalogue counts what happened.

NOT re-litigating drift. Bundle-as-shape's six instances live at insight-drift's Pattern B (= 01KR3BCQXGGB20V8C6Y6Z1Y944); this thread cross-references rather than re-enumerates.

A recurrence pattern may *also* be a drift pattern (Bundle-as-shape is the canonical case; the boundary rule above resolves placement). Both threads are part of the same network, and a reader following one observation across both should land on cross-references that close the loop, not on duplicated instance enumerations that diverge under maintenance.

Provenance:
- `insight-drift` framing entry = 01KR3B7KW5QNRWHG6YTV9QSF07 (analyst register declared; tier taxonomy named).
- `insight-drift` Pattern B entry = 01KR3BCQXGGB20V8C6Y6Z1Y944 (bundle-as-shape misnaming enumeration; cross-referenced from this thread's Pattern 1 entry rather than re-enumerated).
- `insight-drift` Pattern D entry = 01KR3BGMAKS4AZNZE2QFXH10W4 (Pattern D vs. Pattern E distinction; PR #13 → PR #14 25-min behavior-described supersession noted at pattern-boundary level).
- `insight-drift` Pattern E entry = 01KR3BK1VP3SZ5DM9VAQ01FFYX (within-PR self-correction strict reading; PR #31's between-PR reframing flagged as recurrence material for this thread).
- `insight-drift` closure entry = 01KR3BQHTCQ7YGHWQJ2ZE4PJQ4 (boundary rule the catalogue resolves to; cross-reference observation for this thread).
- `insight-drift` negative-space tail = 01KR3BT6MNZMWRMHX14QMYZ86Y (dominant-drift-register-is-description observation; informs this thread's pattern selection).
- `insight-drift` infrastructure tail = 01KR3BVVYN37WDWJ5A3D8A5XWH (the arc-side flagging practice that made cross-arc patterns legible).
- `history-overview` framing entry = 01KR0TRFWT9W6WMFHC49YSW0BG (voice contract for arcs; carried unchanged into insight register).
- `history-overview` segmentation entry = 01KR0TWHTC1MPK4KJ08Y9SPE6P (eight-arc segmentation; pre-collected drift-and-recurrence flags at arc grain).
- The eight arc threads (history-arc-01 through history-arc-08), all OPEN; per-PR entries' drift-findings sections plus story-tails are the source of the candidate instance lists this thread verifies.
- arc-08 story-tail entry = 01KR3A23E11K8F7VNVSM5XY6M2 (three-grain × three-register supersession-acknowledgement summary; the directly-handed-off observation).
- arc-03 story-tail entry = 01KR11S8RG29J98QKN1H0VAA6W (silent-supersession precedent at six-day cross-arc grain and 3.5-hour within-arc grain).
- arc-04 story-tail entry = 01KR13CJ5XS5VREYA4741JHDSQ (machinery-chain capability-accretion; arc-04 chain instance source).
- arc-05 story-tail entry = 01KR2ANRAEFWWR5W9FQP11A0DB (machinery-chain at arc-05 PagerView; the cross-arc PR #34 → PR #35 chain instance source).
- arc-07 story-tail entry = 01KR2JM67RTQHQYN0223GTKH1V (named-then-fixed bracket at two-day grain; substrate-vs-registration observation).
- `watercooler_health` against the spyc code_path reports Healthy at session start (server v0.4.6.dev0; threads-repo URL `git@github.com:calebjacksonhoward/spyc.git`; branch parity clean).
- Federated search to watercooler-cloud's `onboarding-spyc-rust-bitbucket` namespace not attempted at this entry; the brief permits writing the insight thread from spyc-side arc entries alone, and the catalogue is well-sourced from the eight arcs' drift-findings, story-tails, and `insight-drift`'s established cross-references without external sourcing.

<!-- Entry-ID: 01KR3CSQ2YHQ2TD8EAE6DJCTS3 -->

---
Entry: Claude Code (caleb) 2026-05-08T08:57:10.505470+00:00
Role: scribe
Type: Note
Title: Pattern 1: Bundle-as-shape (recurrence reading) — six instances across five arcs, the project-shape behind insight-drift's densest pattern

Spec: scribe

tags: #insight #recurrence

**Pattern statement (recurrence framing).** Bundling-of-multiple-concerns-under-one-PR is a recurring shape across the 22-day window. Six instances span five arcs. The recurrence reading asks: *across the project, how often does a single PR carry multiple thematically-distinct concerns, and what kinds of bundling shapes appear at the project grain?* The drift reading (insight-drift's Pattern B = 01KR3BCQXGGB20V8C6Y6Z1Y944) asks the same six instances a different question: *at moment of merge, does the slug accurately describe what the diff carries?* Both questions resolve to the same six PRs. This entry takes the project-grain reading and refuses to re-litigate the descriptive-accuracy reading; the cross-reference does the work.

**Instance enumeration — cross-referenced to insight-drift Pattern B for descriptive verification, claimed here for the recurrence shape.**

The six instances are catalogued at insight-drift's Pattern B entry (= 01KR3BCQXGGB20V8C6Y6Z1Y944) with full per-PR detail:

1. **PR #15 (arc 04)** — basename-collision parser-extraction (87L) + ^C-route guard (5L). *Arc-entry citation: 01KR130775Q4PKYEN6FE1743DJ.*
2. **PR #20 (arc 05)** — alt-screen scroll hint + `[pane] default_command` + `gd`-vs-HEAD. *Arc-entry citation: 01KR2A6TT516XA5FEGVBXYPWD7.*
3. **PR #10 (arc 06)** — quickselect feature + `gf`/`gF` scroll-mode `### Fixed` half. *Arc-entry citation: 01KR2GH1D9QCGDPZEMWW09R898.*
4. **PR #25 (arc 06)** — input-dispatch hardening (two enumerated cases) + `--key-trace` diagnostic infrastructure. *Arc-entry citation: 01KR2GMSNX29CWFN154QBK6TJ3.*
5. **PR #18 (arc 07)** — AGENTS.md rename + MCP hygiene fixes + a deferred-design BUGS.md note that brackets future work. *Arc-entry citation: 01KR2J1R3HXNZPAHE9118BGBQJ.*
6. **PR #14 (arc 08)** — routing fix (2L) + `.gitignore` (2L) + `CLAUDE.md` (1L). *Arc-entry citation: 01KR38XPJ07ZFQHH1TG6X461WN.*

**Instance count: six.** The count matches insight-drift's Pattern B verification. No revisions. The recurrence reading does not need to re-do the per-PR diff-weight arithmetic; insight-drift's Pattern B carries that work and this thread cites it.

**The recurrence reading the drift framing did not have an angle for: what kinds of bundling shapes recur?**

Insight-drift's Pattern B catalogued four sub-shapes within the six instances (bundle-of-noticed-while-shipping; bundle-of-shared-infrastructure; bundle-of-equal-weight-concerns; bundle-of-rename-plus-groundwork-plus-deferred-design-note). Those sub-shapes are the load-bearing observation for the recurrence reading too — the recurrence is not generic-bundling; it is a small set of legible bundling-shapes that recur with their own internal structure. The six instances distribute across the four sub-shapes:

- **Bundle-of-noticed-while-shipping** (two instances): PR #15 and PR #14. Two unrelated fixes ride one PR because both were spotted in proximity. No shared infrastructure, no shared call chain, no shared root cause. The smallest amplitude (PR #14: 2 + 2 + 1 = 5 lines bundled around the load-bearing 2-line routing fix). The recurrence reading: this sub-shape is the cheapest kind of bundle — the cost of opening a separate PR for a 2-line `.gitignore` addition exceeds the cost of bundling it with the routing fix.

- **Bundle-of-shared-infrastructure** (two instances): PR #10 and PR #25. A feature half and a fix-or-diagnostic half ride one PR because both consume a contract introduced for the feature. PR #10's `pickable_text` helper is consumed by quickselect (the feature) and by `gf`/`gF` (the fix); PR #25's `--key-trace` infrastructure is consumed by the defensive guards and by future bug reports. The recurrence reading: this sub-shape is the sub-shape where the bundle is *load-bearing for shipping the feature itself* — the new contract is the connection.

- **Bundle-of-equal-weight-concerns** (one instance): PR #20. Three concerns each independently shippable and roughly comparable in size, all under one `feat/` slug. The densest single instance. Recurrence reading: a one-instance sub-shape; the catalogue does not promote it.

- **Bundle-of-rename-plus-groundwork-plus-deferred-design-note** (one instance): PR #18. A rename half + a hygiene half + a BUGS.md design note that brackets future work; the PR is doing three different *kinds* of structural move at once. Recurrence reading: a one-instance sub-shape; the catalogue does not promote it. PR #18's BUGS.md note is the named-half that PR #37 closes two days later — the *named-then-fixed bracket* recurrence at Pattern 4's two-day grain originates here.

**Notes on bundling distribution and pattern boundary.**

- *Density across arcs.* The six instances spread across five arcs (arc 04, arc 05, arc 06 ×2, arc 07, arc 08). Arc 06 is the only arc with two instances in its four-PR span. Insight-drift's Pattern B made this observation factually and declined to interpret; this thread carries the same factual observation and the same decline. The recurrence reading adds a small refinement: arc 06's two instances are *both* bundle-of-shared-infrastructure shape, the only two such shape instances in the catalogue. Arc 06 contributes 2/2 of that sub-shape; whether that means the picker-overlay-introducing PRs structurally invite consumer-ride-along bundles is a question for `insight-emergent-properties`. Captured factually here.

- *Sub-shape distribution as recurrence evidence.* Two-of-six is the noticed-while-shipping sub-shape; two-of-six is the shared-infrastructure sub-shape; one-of-six each for the equal-weight and rename-plus-groundwork sub-shapes. The 2-2-1-1 distribution is small enough that *recurrence* is the right word for the noticed-while-shipping and shared-infrastructure sub-shapes; *single instance with a name* is the right word for the equal-weight and rename-plus-groundwork sub-shapes. The catalogue does not promote 1-instance sub-shapes to "the sub-shape recurs."

- *Cross-reference for the drift reading.* The drift reading's question — *does the slug accurately describe the diff?* — is what insight-drift's Pattern B answered. The six diffs answer *no, with varying amplitude*: PR #15's slug names the smaller half first; PR #20's slug names all three concerns explicitly; PR #14's slug names only the load-bearing concern; PR #18's slug names two of three halves; PR #10's slug is `feat/quickselect` and the CHANGELOG carries `### Fixed` content; PR #25's slug names both halves. The drift amplitude varies; the bundling itself recurs. *Recurrence and drift are different lenses on the same six PRs.*

- *Boundary with the named-then-fixed bracket pattern.* PR #18's BUGS.md design note is part of PR #18's bundle (this entry's instance 5 / sub-shape 4); the same note is the open-side of the named-then-fixed bracket at Pattern 4's two-day grain (PR #18 → PR #37). The catalogue does not double-count: PR #18 is a bundle instance here; the *PR #18 → PR #37 bracket* is a separate observation in Pattern 4. Same diff, two distinct shapes.

- *No additional instances.* Verification did not reveal a seventh bundle instance in the eight arcs. The brief's six is the instance count.

Provenance:
- `insight-drift` Pattern B entry = 01KR3BCQXGGB20V8C6Y6Z1Y944 (six-instance enumeration with per-PR diff weights, sub-shape taxonomy, drift framing).
- arc-04 PR #15 entry = 01KR130775Q4PKYEN6FE1743DJ (bundle instance 1).
- arc-05 PR #20 entry = 01KR2A6TT516XA5FEGVBXYPWD7 (bundle instance 2).
- arc-06 PR #10 entry = 01KR2GH1D9QCGDPZEMWW09R898 (bundle instance 3).
- arc-06 PR #25 entry = 01KR2GMSNX29CWFN154QBK6TJ3 (bundle instance 4).
- arc-07 PR #18 entry = 01KR2J1R3HXNZPAHE9118BGBQJ (bundle instance 5; also Pattern 4 named-then-fixed bracket open-side).
- arc-08 PR #14 entry = 01KR38XPJ07ZFQHH1TG6X461WN (bundle instance 6).
- `insight-recurrence` framing entry = 01KR3CSQ2YHQ2TD8EAE6DJCTS3.

<!-- Entry-ID: 01KR3CW3DBHPTB6K8R8047TBCP -->

---
Entry: Claude Code (caleb) 2026-05-08T08:59:00.130439+00:00
Role: scribe
Type: Note
Title: Pattern 2: Supersession-acknowledgement — three canonical instances at three time grains × three acknowledgement registers, plus a fourth long-grain instance at six calendar days

Spec: scribe

tags: #insight #recurrence

**Pattern statement.** A later PR supersedes earlier code. The supersession is real (the later PR's diff edits, generalizes, retracts, or makes-unreachable code the earlier PR shipped) and observable from the diff alone. The relationship between the two PRs is acknowledged at one of three different registers — silent (no commit-message reference; the supersession is visible only in the code's evolution), behavior-described (the later PR's commit body or CHANGELOG describes the bug being fixed accurately and does not cite the predecessor), or explicit-reframing (the later PR's commit body names the predecessor and reframes the prior framing). The pattern's identity is the *cross-product* of time grain × acknowledgement register: three instances of the same structural shape at three different time grains carrying three different registers.

**Instance enumeration with arc-entry citations.**

1. **Arc 03 PR #26 → PR #29 (3.5 hours, silent).** PR #26 (`feat/dim-unfocused-pane`, commit 20fba00, 2026-05-06 14:16 UTC) lands the per-cell `Modifier::DIM` modifier and leaves the cursor-block code's existing `if !self.focused { add_modifier(DIM) }` branch alone. PR #29 (`fix/skip-pane-cursor-block-when-uninvited`, commit bdb8d87, 2026-05-06 17:54 UTC) lands 3.5 hours later, same source file (`src/pane/widget.rs`), and drops that dim branch entirely. Under PR #29's three-condition guard (focused, not-alt-screen, not-hide-cursor), an unfocused pane never enters the cursor-block paint path at all, so the dim branch becomes unreachable. The acknowledgement register is silent: nothing in PR #29's commit subject, commit body, or CHANGELOG entry references PR #26. The supersession lives in the code's evolution alone. *Cite: arc-03 story-tail = 01KR11S8RG29J98QKN1H0VAA6W ("PR #29's diff edits code PR #26's diff added that morning, and again: nothing in PR #29's commit subject acknowledges PR #26"); arc-03 PR #29 entry = 01KR10G02J2234D0WBMWMYC35M.*

2. **Arc 08 PR #13 → PR #14 (25 minutes, behavior-described).** PR #13 (`feat/graveyard-undo`, commit 6b2be36, 2026-05-03 02:41 UTC) ships `:undo` under CHANGELOG's `### Added` block but does not wire the command name into `AppState::dispatch_command`'s punt list. PR #14 (`fix/undo-command`, commit c7419c1, 2026-05-03 03:06 UTC) lands 25 minutes later with two lines added to the punt list. PR #14's commit body describes the bug accurately and verbatim — *"Repro: type `:undo` → flash 'unknown command: undo'"* — and does not cite PR #13 as the predecessor that shipped the broken pairing. The acknowledgement register is behavior-described: the *bug* is named in PR #14's text; the *PR-relationship* is not. *Cite: arc-08 PR #14 entry = 01KR38XPJ07ZFQHH1TG6X461WN; arc-08 story-tail = 01KR3A23E11K8F7VNVSM5XY6M2 ("PR #14's CHANGELOG describes the bug accurately and does not cite PR #13"); insight-drift Pattern D entry = 01KR3BGMAKS4AZNZE2QFXH10W4 (Pattern D's instance is the same diff pair, scoped to the documented-vs-wired drift; this entry takes the recurrence-of-supersession-acknowledgement reading).*

3. **Arc 08 PR #30 → PR #31 (49 minutes, explicit reframing).** PR #30 (`fix/vt100-panic-recovery`, commit e39f462, 2026-05-06 18:27 UTC) ships catch_unwind defensive recovery and adds a BUGS.md MAYBE block arguing that the upgrade *"touches every place that holds a `vt100::Screen` reference"* and should *"defer until someone has a clear afternoon."* PR #31 (`chore/vt100-and-ratatui-upgrade`, commit 105db8d, 2026-05-06 19:16 UTC) lands 49 minutes later with the upgrade. PR #31's commit body opens: *"The vt100 bump is the proper fix for the `screen.rs:934.unwrap()` panic (caught defensively in v1.41.17). Smaller than I'd previously framed it"* — five words doing the explicit reframing. PR #31's diff also deletes PR #30's BUGS.md MAYBE block, the *"clear afternoon"* deferral having arrived as the same afternoon the deferral was authored. The acknowledgement register is explicit-reframing: the predecessor PR's framing is named and reframed in the successor PR's commit body. *Cite: arc-08 PR #31 entry = 01KR397RTYNS34SAGM46YJJRBY; arc-08 story-tail = 01KR3A23E11K8F7VNVSM5XY6M2 ("'Smaller than I'd previously framed it' — five words doing a lot of work in the commit message"); insight-drift Pattern E entry = 01KR3BK1VP3SZ5DM9VAQ01FFYX (the between-PR reframing flagged at pattern-boundary level for this thread).*

**Three-instance count for the canonical pattern.** The arc-08 story-tail's three-instance summary (= 01KR3A23E11K8F7VNVSM5XY6M2: *"three such instances at three time grains (3.5 hours, 49 minutes, 25 minutes) with three different acknowledgement registers (silent in arc 03; behavior-described in PR #14; explicit reframing in PR #31)"*) holds verbatim. Time grains: 3.5h / 49m / 25m. Registers: silent / explicit-reframing / behavior-described. Cross-product is full — each grain carries a distinct register, and each register lands at a distinct grain. The three instances are not three iterations of one shape; they are three shapes of one super-pattern, each contributing a different cell of the grain × register matrix.

**Plus one fourth instance at six-calendar-day grain (silent register, cross-arc).**

4. **Arc 02 PR #5 → arc 03 PR #29 (six calendar days, silent, cross-arc).** PR #5 (`investigate/lazygit-support`, commit 0691666, 2026-04-30 22:53 UTC) ships a narrow guard against a specific lazygit case: `if !screen.hide_cursor()`, single condition, motivated by exactly one app. PR #29 (commit bdb8d87, 2026-05-06 17:54 UTC) lands six calendar days later with a three-condition guard (focused, not-alt-screen, not-hide-cursor) generalizing PR #5's narrow case to the broader class. The acknowledgement register is silent: PR #29's commit subject, commit body, and CHANGELOG describe the nvim-beam-in-insert-mode user report and the new policy without naming PR #5 as the predecessor it generalizes from. The arc-03 story-tail makes the supersession explicit: *"What makes the supersession diagnostic isn't the guard-broadening per se — it's that nothing in either commit says 'this supersedes PR #5.'"* The link to PR #5's gap analysis is implicit and discoverable only through `notes/lazygit-gap-analysis.md` "Top suspects" §1 (subsequently relocated to `BUGS.md` by PR #12). *Cite: arc-03 story-tail = 01KR11S8RG29J98QKN1H0VAA6W; arc-03 PR #29 entry = 01KR10G02J2234D0WBMWMYC35M; arc-02 investigation entry = 01KR0YXXZRQR24CSNAK4Q7808T; insight-drift Pattern E entry = 01KR3BK1VP3SZ5DM9VAQ01FFYX (PR #29's policy comment listing alt-screen TUIs without naming PR #5).*

The fourth instance is held *separately* from the three-grain × three-register canonical cross-product because:
- The arc-08 story-tail's three-instance summary is what this thread's brief handed forward; the fourth instance is the long-grain background-shape that makes the canonical three legible as recurrence rather than coincidence.
- The fourth instance shares the silent register with instance 1 (Arc 03 PR #26 → PR #29). Two silent supersessions converge on the *same* successor PR (PR #29), one cross-arc at six-day grain, one within-arc at 3.5-hour grain. The convergence is observable; the catalogue does not interpret it.
- Promoting the four instances to a single grain ladder (six-day / 3.5-hour / 49-minute / 25-minute) would risk over-claiming: with four instances, the catalogue would still hold one instance per grain, no recurrence-within-grain. The three-by-three cross-product the arc-08 story-tail already named is the load-bearing matrix; the fourth is a long-grain corroborating instance.

**Instance count: three canonical (arc-08 story-tail's enumeration) + one fourth long-grain instance = four total.** The four-instance reading is what the eight-arc record carries.

**Notes on time-grain × acknowledgement-register and pattern boundary.**

- *Time-grain spread.* From 25 minutes (PR #13 → PR #14) to six calendar days (PR #5 → PR #29) is a factor of ~350 in elapsed time. Across that range the same structural shape recurs — *a later PR supersedes earlier code; the relationship to the earlier PR is acknowledged in one of three registers*. The recurrence is genuinely shape-recurrence, not time-clustered phenomenon.

- *Register spread.* Silent / behavior-described / explicit-reframing is not a continuum; it is three distinct registers. Silent: no acknowledgement at all, the supersession is in the code only. Behavior-described: the bug or change is named accurately in the successor PR's text, but the predecessor PR is not cited. Explicit-reframing: the predecessor PR's framing is named and reframed in the successor's text. Two of the four instances are silent (instances 1 and 4); one is behavior-described (instance 2); one is explicit-reframing (instance 3). The register distribution is 2-1-1, weighted toward silent.

- *Where two of the four converge.* PR #29 is the successor in both instance 1 (within-arc, 3.5h, silent) and instance 4 (cross-arc, six days, silent). The same PR carries both supersessions in the same diff. The arc-03 story-tail makes the convergence factual: *"PR #29's diff edits code PR #26's diff added that morning"* (instance 1) and *"What makes the supersession diagnostic isn't the guard-broadening per se — it's that nothing in either commit says 'this supersedes PR #5'"* (instance 4). One PR closes two distinct supersessions at two distinct grains, both silent. Whether that convergence makes PR #29's silence more diagnostic, less diagnostic, or a separate observation entirely is a question for `insight-emergent-properties`. Captured factually.

- *Why this is recurrence and not drift.* Insight-drift's closure entry (= 01KR3BQHTCQ7YGHWQJ2ZE4PJQ4) places this material here explicitly: *"no commit subject claims the PR is a supersession; the silence-or-explicitness is at the description-of-the-relationship level, not at the misnaming-at-the-moment-of-merge level."* Misnaming-at-merge would be a drift; *not-mentioning-the-relationship* is a recurrence shape that lives at description-grain. The boundary holds.

- *Why instance 2 is recurrence and Pattern D is drift, despite being the same PR pair.* Insight-drift's Pattern D (= 01KR3BGMAKS4AZNZE2QFXH10W4) treats PR #13 → PR #14 as a documented-vs-wired drift instance: the CHANGELOG promised `:undo` and the code didn't deliver it for 25 minutes. This thread's instance 2 treats the same diff pair as a supersession-acknowledgement instance: PR #14 supersedes PR #13's broken pairing, with the bug behavior described in commit text and the predecessor PR not cited. Same observable, two readings, two threads. The cross-reference closes the loop.

- *Why this is not Pattern E (within-PR self-correction).* Insight-drift's Pattern E (= 01KR3BK1VP3SZ5DM9VAQ01FFYX) catalogues a single intra-diff instance (PR #30's BUGS.md MAYBE block retracting its own commit body's "unmaintained" framing). PR #30 → PR #31 is between-PR. Pattern E is *intra-diff*; this Pattern is *cross-PR*. The 49-minute window holds both phenomena (PR #30's intra-diff Pattern E instance + PR #30 → PR #31's cross-PR Pattern 2 instance 3) without conflict.

- *Maintainer-experience axis declined.* The temptation to interpret *why* the registers vary — *why* PR #29 is silent, *why* PR #14 is behavior-described, *why* PR #31 is explicit-reframing — is acute and emergent-property-shaped. This thread declines. The three registers are observable. The variance is observable. The cause of the variance is `insight-emergent-properties`'s.

Provenance:
- arc-03 story-tail = 01KR11S8RG29J98QKN1H0VAA6W (instance 1 within-arc + instance 4 cross-arc; "nothing in either commit says 'this supersedes PR #5'" framing source).
- arc-03 PR #29 entry = 01KR10G02J2234D0WBMWMYC35M (the successor PR in instances 1 and 4).
- arc-08 story-tail = 01KR3A23E11K8F7VNVSM5XY6M2 (three-grain × three-register canonical summary; the directly-handed-off observation).
- arc-08 PR #14 entry = 01KR38XPJ07ZFQHH1TG6X461WN (instance 2 successor PR; "Repro: type `:undo` → flash 'unknown command: undo'" verbatim).
- arc-08 PR #31 entry = 01KR397RTYNS34SAGM46YJJRBY (instance 3 successor PR; "Smaller than I'd previously framed it" verbatim).
- arc-02 investigation entry = 01KR0YXXZRQR24CSNAK4Q7808T (instance 4 predecessor PR's gap analysis; the implicit lineage source).
- arc-02 harvest entry = 01KR0Z11CKNJRYEZ3T38EAFSC4 (BUGS.md cursor-block residual that bridges instance 4's six-day gap).
- `insight-drift` Pattern D entry = 01KR3BGMAKS4AZNZE2QFXH10W4 (the documented-vs-wired drift reading of instance 2; cross-referenced rather than re-litigated).
- `insight-drift` Pattern E entry = 01KR3BK1VP3SZ5DM9VAQ01FFYX (the within-PR self-correction reading of PR #30; the between-PR reframing of PR #31 flagged for this thread at pattern-boundary level).
- `insight-drift` closure entry = 01KR3BQHTCQ7YGHWQJ2ZE4PJQ4 (boundary-rule placement decision: supersession-acknowledgement is recurrence-only, not drift).
- `insight-recurrence` framing entry = 01KR3CSQ2YHQ2TD8EAE6DJCTS3.

<!-- Entry-ID: 01KR3CZEM22Y5BRT1F2VQZ6EKZ -->

---
Entry: Claude Code (caleb) 2026-05-08T09:00:39.877988+00:00
Role: scribe
Type: Note
Title: Pattern 3: BUGS.md SMALL/MAYBE-to-FIXED lift — six verified instances across five arcs (one brief candidate dropped after verification)

Spec: scribe

tags: #insight #recurrence

**Pattern statement.** BUGS.md's SMALL and MAYBE buckets are the working surface for tracked-but-not-yet-fixed concerns. A landing PR that closes one of those concerns lifts the corresponding entry from SMALL/MAYBE to FIXED — typically by deleting the SMALL/MAYBE line and adding a `(fixed, v1.41.X)`-tagged FIXED block, or by editing the entry's bucket inline. The shape is: *the bucket is the queue; landing PRs drain it.* The pattern is recurrence-only; no misnaming aspect at moment of merge distinguishes it from project-shape, so it does not appear in `insight-drift`.

**Instance enumeration with arc-entry citations.**

1. **Arc 02 PR #12 (genesis) — harvest creates the SMALL/BIGGER/MAYBE buckets from PR #5's gap analysis.** PR #12 (`chore/clean-notes`, commit e210e58, 2026-05-03) is the *genesis* instance: 399 lines deleted from `notes/lazygit-gap-analysis.md` and `notes/lazygit-ux-catalogue.md`; 48 lines added to BUGS.md across the SMALL bucket (cursor-block, COLORTERM, graveyard repositioning), the BIGGER bucket (mouse), and the MAYBE bucket (mode 2026, OSC 8). The harvest *creates* the entries that downstream PRs lift from. PR #12 is not itself a SMALL-to-FIXED lift; it is the upstream act that makes the recurrent lift shape possible. *Cite: arc-02 harvest entry = 01KR0Z11CKNJRYEZ3T38EAFSC4.*

2. **Arc 05 PR #35 — `D opens pager in top pane` lifts the BUGS.md SMALL `D`-user-request to FIXED.** PR #35 (`feat/D-opens-pager-in-top-pane`, commit c243549, 2026-05-06 23:53 UTC) implements the user's `D` request and lifts the corresponding SMALL entry to FIXED in the same diff. *Cite: arc-05 PR #35 entry = 01KR2AD5PV989H58E49E5D18NM; arc-05 story-tail = 01KR2ANRAEFWWR5W9FQP11A0DB ("PR #35 lifted a BUGS.md SMALL the same way the day before").*

3. **Arc 05 PR #36 — `/` and `=` substring-match lifts the BUGS.md SMALL substring-match request to FIXED.** PR #36 (`fix/search-substring-match`, commit f505ee5, 2026-05-07 00:18 UTC) shifts the matcher semantics and lifts the corresponding SMALL entry to FIXED. *Cite: arc-05 PR #36 entry = 01KR2AFHD42DHX6XQS7S6VK4M5; arc-05 story-tail = 01KR2ANRAEFWWR5W9FQP11A0DB ("BUGS.md lifts a SMALL entry to FIXED").*

4. **Arc 07 PR #18 → PR #37 — the named-then-fixed bracket: PR #18 *adds* the BUGS.md SMALL note; PR #37 lifts it to FIXED two days later.** PR #18 (`chore/agents-md-and-mcp-hygiene`, commit bad8bfc, 2026-05-05 00:41 UTC) adds a 13-line BUGS.md SMALL entry naming the cross-project MCP-attachment bug, weighting three design fixes and marking option (b) as *"most spyc-shaped."* PR #37 (`fix/mcp-socket-project-scoped-discovery`, commit a303251, 2026-05-07 00:54 UTC) implements exactly option (b), removes PR #18's 13-line SMALL entry, removes an older 2-line entry that predates the window (*"something funky is happening with our MCP support"*), and adds a `(fixed, v1.41.24)` block to FIXED whose closing line names the older entry: *"is also resolved by this change."* This is the only verified instance in the catalogue where the same arc *opens and closes the same SMALL entry* — the bracket recurs at Pattern 4 too. *Cite: arc-07 PR #18 entry = 01KR2J1R3HXNZPAHE9118BGBQJ; arc-07 PR #37 entry = 01KR2JCF7QEJHEG30TVMWY79CQ; arc-07 story-tail = 01KR2JM67RTQHQYN0223GTKH1V.*

5. **Arc 08 PR #28 — directory cap lifts BUGS SMALL #4 (huge directory) to FIXED.** PR #28 (`fix/huge-directory-cap`, commit 306b43f, 2026-05-06 17:30 UTC) caps directory listings at 50,000 entries. PR #28's commit body names BUGS SMALL #4 directly, names the failure mode (`stat()` syscalls × entry count = event-loop block on slow filesystems), and names the chosen-but-not-empirically-defended cap by listing what fits under it. The corresponding SMALL entry is lifted to FIXED. *Cite: arc-08 PR #28 entry = 01KR3903VA7DTNDJKQAFZ6DP8M; arc-08 story-tail = 01KR3A23E11K8F7VNVSM5XY6M2 ("PR #28's commit body names BUGS SMALL #4 directly").*

6. **Arc 08 PR #31 — vt100/ratatui upgrade lifts BUGS.md MAYBE entries (mode 2026, OSC 8) to FIXED.** PR #31 (`chore/vt100-and-ratatui-upgrade`, commit 105db8d, 2026-05-06 19:16 UTC) deletes PR #30's BUGS.md MAYBE block (the `vt100 0.15 unmaintained` claim PR #30 had partially retracted in its own diff) *and* lifts the older MAYBE entries from PR #12's harvest (mode 2026, OSC 8) to FIXED. Three MAYBE-to-FIXED lifts in a single diff, naming the upstream-fix at three independent surfaces (commit body, BUGS.md MAYBE-removal, BUGS.md FIXED-block). *Cite: arc-08 PR #31 entry = 01KR397RTYNS34SAGM46YJJRBY; arc-08 story-tail = 01KR3A23E11K8F7VNVSM5XY6M2 ("PR #31's diff names mode 2026 (synchronized output) at three independent surfaces").*

**Instance count: six.** Five lift instances (instances 2, 3, 4, 5, 6) plus the genesis instance (instance 1: PR #12 creates the bucket). All five lift instances verified against arc-entry citations.

**One brief candidate dropped after verification.**

The brief named *"Arc 06 PR #25 lifts BUGS.md SMALL items related to dispatch."* Verification at the arc-06 PR #25 entry (= 01KR2GMSNX29CWFN154QBK6TJ3) reveals that PR #25 *adds* a fresh `### FIXED ###` block tagged `(defensive, v1.41.12)` describing the user report and the response shape, *without removing a prior SMALL entry of the same content*. PR #25's BUGS.md edit is `+15 / -0`: a fresh FIXED entry, no SMALL precursor. The shape is *"add to FIXED to record what was just fixed"* — a related but distinct shape from *"lift SMALL/MAYBE to FIXED."* The lift requires a SMALL or MAYBE entry to already exist; PR #25 has no such precursor in the BUGS.md state PR #18's bundle had left the previous day. *Drop instance from the catalogue's SMALL/MAYBE-to-FIXED lift count; flag the related shape as observation. The brief's "items related to dispatch" wording is consistent with PR #25 directly responding to a user report rather than draining a queued entry. Cite: arc-06 PR #25 entry = 01KR2GMSNX29CWFN154QBK6TJ3 (BUGS.md FIXED block with `(defensive, v1.41.12)` tag, quoted verbatim there; no SMALL deletion).*

The catalogue holds six instances with one brief candidate dropped. Honest counting is what the analyst register requires.

**Notes on lift shape and pattern boundary.**

- *The lift is multi-channel.* Most lift instances (2, 3, 4, 5, 6) exercise the same multi-channel pattern: the predecessor SMALL/MAYBE entry is deleted, a FIXED entry is added (typically with a `(fixed, v1.41.X)` tag), and the CHANGELOG carries an `### Added`/`### Changed`/`### Fixed` block. Three text channels for one act. The recurrence reading does not need to count the channels per instance; the consistent multi-channel shape is itself the recurrence.

- *The genesis matters.* Without instance 1 (PR #12's harvest creating the SMALL/BIGGER/MAYBE buckets), the lift shape would have nothing to lift from. The genesis is structurally upstream of every other lift instance. This makes the BUGS.md SMALL/MAYBE-to-FIXED lift recurrence *partially* a property of the gap-analysis methodology PR #5 introduced and PR #12 harvested — a question for `insight-trajectory` to interpret (*does the lift recurrence reflect direct execution against the gap-analysis plan, or is it a project-shape pattern that would have emerged anyway?*). This thread does not interpret. Captured factually here for the trajectory thread's author.

- *Time-grain spread.* Instance 2 (arc-05 PR #35) lifts a SMALL entry; the entry's age in the SMALL bucket isn't necessarily traceable from the diff alone, since SMALL entries enter from multiple sources (PR #12 harvest; PR #18 inline addition; user reports the maintainer files at unknown times). Instance 6 (arc-08 PR #31) lifts MAYBE entries that traceably entered at PR #12's harvest three calendar days earlier. Instance 4 (arc-07 PR #18 → PR #37) opens *and* closes its own SMALL entry within the same arc, two calendar days. The lift-from-genesis time grains span from same-arc-bracket (PR #18 → PR #37, 2 days) to multi-arc lag (PR #12's MAYBE entries lifted by arc 08's PR #31, 3 days). The catalogue does not promote the time-grain variance to a sub-shape; the variance is observable and is captured here factually.

- *Asymmetry between SMALL lifts and MAYBE lifts.* Of the five lift instances, three lift SMALL entries (instances 2, 3, 5) and one lifts MAYBE entries (instance 6); instance 4 lifts a SMALL entry it had itself added two days earlier. The SMALL bucket's lifts are dominantly small-fix-shaped (a `D` request, a substring-match shift, a directory cap); the MAYBE bucket's single lift is a major-version dep upgrade. Whether the SMALL-vs-MAYBE distinction tracks fix-amplitude (*small concerns lift cheap; maybe concerns lift expensive*) or just naming-honesty (*MAYBE means the maintainer wasn't sure when authoring; the lift confirmed*) is for `insight-emergent-properties`. Captured factually.

- *Boundary with Pattern 4 (named-then-fixed bracket).* Instance 4 (PR #18 → PR #37) is *both* a SMALL-to-FIXED lift instance here and a named-then-fixed bracket instance at Pattern 4. The catalogue does not double-count the recurrence; the same observable resolves to two distinct shapes. The SMALL-to-FIXED lift is *the bucket-drain shape*; the named-then-fixed bracket is *the same-author-opens-then-closes-the-issue shape*. PR #18 → PR #37 is the only instance that satisfies both shapes simultaneously; the other lifts are bucket-drains where the SMALL entry was authored by an earlier process (PR #12's harvest, or a user report the maintainer filed without an associated PR), not by a self-bracketing PR.

- *No additional instances.* Verification did not reveal a seventh lift instance in the eight arcs. The brief's seven minus one dropped equals six, matching the pre-existing eight-arc ground.

Provenance:
- arc-02 harvest entry = 01KR0Z11CKNJRYEZ3T38EAFSC4 (instance 1 genesis; SMALL/BIGGER/MAYBE bucket creation).
- arc-05 PR #35 entry = 01KR2AD5PV989H58E49E5D18NM (instance 2).
- arc-05 PR #36 entry = 01KR2AFHD42DHX6XQS7S6VK4M5 (instance 3).
- arc-05 story-tail = 01KR2ANRAEFWWR5W9FQP11A0DB (instances 2 and 3 SMALL-to-FIXED lift framing).
- arc-06 PR #25 entry = 01KR2GMSNX29CWFN154QBK6TJ3 (the dropped brief candidate; fresh FIXED-without-SMALL-precursor shape).
- arc-07 PR #18 entry = 01KR2J1R3HXNZPAHE9118BGBQJ (instance 4 open-side; the BUGS.md SMALL entry's authorship).
- arc-07 PR #37 entry = 01KR2JCF7QEJHEG30TVMWY79CQ (instance 4 close-side; the BUGS.md SMALL → FIXED lift, plus the older 2-line entry resolved-by-this-change).
- arc-07 story-tail = 01KR2JM67RTQHQYN0223GTKH1V (instance 4 named-then-fixed bracket framing; cross-referenced from Pattern 4).
- arc-08 PR #28 entry = 01KR3903VA7DTNDJKQAFZ6DP8M (instance 5; BUGS SMALL #4 named in commit body).
- arc-08 PR #31 entry = 01KR397RTYNS34SAGM46YJJRBY (instance 6; MAYBE-to-FIXED for mode 2026 and OSC 8).
- arc-08 story-tail = 01KR3A23E11K8F7VNVSM5XY6M2 (instances 5 and 6 framing).
- `insight-drift` closure entry = 01KR3BQHTCQ7YGHWQJ2ZE4PJQ4 (boundary-rule placement: SMALL-to-FIXED lift is recurrence-only, not drift).
- `insight-recurrence` framing entry = 01KR3CSQ2YHQ2TD8EAE6DJCTS3.

<!-- Entry-ID: 01KR3D2G1S7DXYSPDZDRXQBPDX -->

---
Entry: Claude Code (caleb) 2026-05-08T09:02:13.052875+00:00
Role: scribe
Type: Note
Title: Pattern 4: Named-then-fixed bracket — three instances at three grains (one-PR / 49-minute / two-day) where the same author opens and closes a named issue

Spec: scribe

tags: #insight #recurrence

**Pattern statement.** A bug, design issue, or deferred concern is *named* in durable text by one PR (or one diff): a BUGS.md SMALL/MAYBE entry, a commit-body framing, a doc-comment, or a CHANGELOG hedge. A later PR (or the same PR's later half) *closes* the named issue and removes/lifts the naming. The bracket is the open-side text plus the close-side fix; the recurrence is that the bracket-shape appears at three different time grains in the eight-arc record.

This pattern overlaps with Pattern 3 (SMALL-to-FIXED lift) in the case where the named text is a BUGS.md entry. The distinction is at the framing level: Pattern 3 counts *bucket drains*; Pattern 4 counts *bracketed authorship* (same author opens and closes an issue, possibly within the same PR, possibly across PRs separated by hours or days). The two patterns share instance 4 of Pattern 3 (PR #18 → PR #37); the catalogue does not double-count the *observable* but treats the two pattern questions as distinct.

**Instance enumeration with arc-entry citations.**

1. **Arc 08 PR #28 (one-PR bracket).** PR #28 (`fix/huge-directory-cap`, commit 306b43f, 2026-05-06 17:30 UTC) closes BUGS SMALL #4 (huge directory). The open-side: the SMALL #4 entry traceably entered the BUGS.md record via PR #12's harvest (= 01KR0Z11CKNJRYEZ3T38EAFSC4 carries the harvest's bucket additions; the harvest PR's diff added BIGGER mouse and MAYBE mode-2026/OSC-8 verbatim, and the SMALL bucket gained cursor-block and COLORTERM at the harvest, with SMALL #4 traceable to the gap-analysis suspect record from PR #5's investigation). The close-side: PR #28's diff caps directory listings at 50,000 entries and lifts SMALL #4 to FIXED.

   The bracket grain is *one-PR* in the sense that PR #28 is the close. The open-side is upstream-author-of-record-different (PR #12 harvest from PR #5 investigation), making the strict reading: this is a *cross-arc same-codebase bracket* where the open-side and close-side are different PRs, and the close-side is one PR. The catalogue holds it as the *one-PR-grain bracket* because the close happens in a single landed PR with no in-between fix-attempts; the *open-side authorship* across PR #5 → PR #12 → PR #28 is the long-tail context that makes the bracket an issue-named-then-fixed shape rather than a casual bug fix. *Cite: arc-08 PR #28 entry = 01KR3903VA7DTNDJKQAFZ6DP8M; arc-02 harvest entry = 01KR0Z11CKNJRYEZ3T38EAFSC4 (open-side recordable-text source); arc-08 story-tail = 01KR3A23E11K8F7VNVSM5XY6M2 ("PR #28's commit body names BUGS SMALL #4 directly").*

2. **Arc 08 PR #30 → PR #31 (49-minute bracket).** PR #30 (`fix/vt100-panic-recovery`, commit e39f462, 2026-05-06 18:27 UTC) opens the bracket: a BUGS.md MAYBE block argues the upgrade *"touches every place that holds a `vt100::Screen` reference"* and recommends *"defer until someone has a clear afternoon."* The open-side names the cost and weights the fix as not-yet-tractable. PR #31 (`chore/vt100-and-ratatui-upgrade`, commit 105db8d, 2026-05-06 19:16 UTC) lands 49 minutes later, deletes PR #30's MAYBE block, and lifts the older PR-#12-authored MAYBE entries (mode 2026, OSC 8) to FIXED in the same diff. The bracket close also reframes the cost: *"Smaller than I'd previously framed it"* — the same five words that carry the explicit-reframing register at Pattern 2's instance 3.

   The bracket grain is *49 minutes*. The open-side and close-side are different PRs by the same author within the same hour. *Cite: arc-08 PR #30 entry = 01KR393P15VTJSZ1WGYGZ8ZS01; arc-08 PR #31 entry = 01KR397RTYNS34SAGM46YJJRBY; arc-08 story-tail = 01KR3A23E11K8F7VNVSM5XY6M2 ("the 'clear afternoon' had arrived as the same afternoon the deferral was authored").*

3. **Arc 07 PR #18 → PR #37 (two-day bracket).** PR #18 (`chore/agents-md-and-mcp-hygiene`, commit bad8bfc, 2026-05-05 00:41 UTC) opens the bracket: a 13-line BUGS.md SMALL note names the cross-project MCP-attachment bug, weights three design fixes, and marks option (b) — *"keeps the 'just works' ergonomics while ruling out cross-instance attachment"* — as *"most spyc-shaped."* The open-side is unusually detailed for a SMALL entry: it names the bug, names the threat ($HOME-unset widening to cross-user), weights three solutions, picks one. PR #37 (`fix/mcp-socket-project-scoped-discovery`, commit a303251, 2026-05-07 00:54 UTC) lands two calendar days later, implements exactly option (b) (project-scoped walk reading the canonical `.spyc-context-<pid>.json` marker file PR #18 made canonical in the same bundle), removes PR #18's 13-line SMALL entry, and adds a `(fixed, v1.41.24)` block.

   The bracket grain is *two calendar days*. The open-side and close-side are different PRs by the same author at the same arc. PR #18's open-side has a property the other two brackets do not: the open-side *also* canonicalizes the file (`.spyc-context-<pid>.json`) that the close-side will consume. The bracket is not just "name the bug, fix it later"; it is *"name the bug AND build the infrastructure the fix will need, then fix it later."* The arc-07 story-tail makes this observation factually: *"the BUGS.md note pre-existed the codex-parity expansion that made the note mandatory; the canonical marker file PR #37 needed was already in the codebase by the time PR #19 and PR #21 widened the codepath that fed it."* *Cite: arc-07 PR #18 entry = 01KR2J1R3HXNZPAHE9118BGBQJ; arc-07 PR #37 entry = 01KR2JCF7QEJHEG30TVMWY79CQ; arc-07 story-tail = 01KR2JM67RTQHQYN0223GTKH1V.*

**Instance count: three.** Three brackets at three time grains: one-PR (arc 08 PR #28, with cross-arc upstream open-side from PR #12 / PR #5); 49 minutes (arc 08 PR #30 → PR #31); two days (arc 07 PR #18 → PR #37). Cross-product against open-side authorship: instance 1's open-side is upstream-cross-arc; instances 2 and 3 are same-author-same-arc-self-bracketing. Cross-product against weighted-options-at-open-side: instance 3 is the only bracket whose open-side weights design options and marks one as preferred; instance 2's open-side weights cost-vs-benefit (defer-because-too-big) without ranked options; instance 1's open-side is descriptive (the gap-analysis suspect text PR #12 lifted) without ranked options.

**Notes on time-grain × bracket-shape and pattern boundary.**

- *Time-grain spread.* From one-PR (instance 1, where the close-side is single-PR with cross-arc upstream open-side) to 49 minutes (instance 2, same-day same-author) to two calendar days (instance 3, same-arc cross-PR). The grain spread is wide; what holds across all three grains is the *bracket shape* — text names the issue, code closes it, durable record updates to FIXED.

- *The brief's third grain candidate revisited.* The brief named *"Arc 08 PR #28 is potentially a one-PR bracket (named in BUGS.md SMALL by some prior PR; closed by PR #28 itself; verify which PR opened the SMALL entry, if any)."* Verification: SMALL #4 traces to PR #12's harvest, which traces to PR #5's gap analysis. So the *open-side authorship* of instance 1 is two PRs upstream of the close-side, across two arcs. The *close-side* is single-PR. The catalogue holds instance 1 as the *one-PR-close-grain* bracket because the close-action is a single PR; the cross-arc open-side authorship is the long-tail context that makes the bracket recur as bracket rather than as casual bug fix. The brief's "verify which PR opened the SMALL entry" question is answered: PR #12 carries the harvest text; PR #5 carries the gap-analysis source.

- *The PR #18 weighted-options open-side as a sub-shape.* PR #18's BUGS.md note is the only open-side in the catalogue with explicitly-weighted design options (three options, one marked "most spyc-shaped"). PR #30's MAYBE block weights cost-vs-benefit but does not enumerate options. PR #12's harvest entries describe gap-analysis suspects with one fix path implied per entry. The arc-07 story-tail observed this factually: *"the BUGS.md note frames the design issue, ranks the options, and PR #37 implements exactly option (b)."* The catalogue does not promote weighted-options-open-side to a separate sub-shape (one instance is too thin); it notes the singularity factually. Whether weighted options at open-side correlate with longer bracket-grain (instance 3 is two days; the other instances are 49 minutes and one-PR) is a question for `insight-emergent-properties`. Captured factually.

- *Boundary with Pattern 3 (SMALL-to-FIXED lift).* The same observable underlies Pattern 3's instance 4 and this Pattern's instance 3 (arc-07 PR #18 → PR #37). The catalogue does not double-count the *PR pair*; it asks two distinct pattern questions of the same observable. Pattern 3's question: *did this PR drain a queued bucket entry?* (Yes; SMALL → FIXED.) Pattern 4's question: *did the same author author both the open-side text and the close-side fix, and at what time grain?* (Yes; same arc, two calendar days.) Two readings, two threads-within-thread, one observable.

- *Boundary with Pattern 2 (supersession-acknowledgement).* Instance 2 (PR #30 → PR #31) is also Pattern 2's instance 3 (49-minute explicit-reframing). The named-then-fixed bracket reading and the supersession-acknowledgement reading attend to different aspects of the same diff pair: the bracket reading attends to *PR #30's MAYBE block being deleted by PR #31*; the supersession reading attends to *"Smaller than I'd previously framed it"* in PR #31's commit body. Both are real; both are observable; the same diff pair carries both shapes.

- *No additional instances.* Verification did not reveal a fourth bracket in the eight arcs that satisfied the *named-in-durable-text-then-fixed* criterion at a different grain. Bug-fix PRs that *did not* have an open-side text in BUGS.md / commit body / doc-comment do not qualify; the bracket pattern requires the open-side naming.

Provenance:
- arc-08 PR #28 entry = 01KR3903VA7DTNDJKQAFZ6DP8M (instance 1 close-side; "PR #28's commit body names BUGS SMALL #4 directly").
- arc-08 PR #30 entry = 01KR393P15VTJSZ1WGYGZ8ZS01 (instance 2 open-side; the BUGS.md MAYBE block).
- arc-08 PR #31 entry = 01KR397RTYNS34SAGM46YJJRBY (instance 2 close-side; "Smaller than I'd previously framed it").
- arc-08 story-tail = 01KR3A23E11K8F7VNVSM5XY6M2 (instances 1 and 2 framing; named-then-fixed bracket at three grains).
- arc-07 PR #18 entry = 01KR2J1R3HXNZPAHE9118BGBQJ (instance 3 open-side; the 13-line SMALL entry weighting three design options).
- arc-07 PR #37 entry = 01KR2JCF7QEJHEG30TVMWY79CQ (instance 3 close-side; the option-(b) implementation).
- arc-07 story-tail = 01KR2JM67RTQHQYN0223GTKH1V (instance 3 framing; "groundwork → expansion → closure" shape).
- arc-02 harvest entry = 01KR0Z11CKNJRYEZ3T38EAFSC4 (instance 1 open-side recordable-text source).
- `insight-recurrence` Pattern 2 entry = 01KR3CZEM22Y5BRT1F2VQZ6EKZ (boundary with supersession-acknowledgement; instance 2 cross-listed there as instance 3).
- `insight-recurrence` Pattern 3 entry = 01KR3D2G1S7DXYSPDZDRXQBPDX (boundary with SMALL-to-FIXED lift; instance 3 cross-listed there as instance 4).
- `insight-recurrence` framing entry = 01KR3CSQ2YHQ2TD8EAE6DJCTS3.

<!-- Entry-ID: 01KR3D5B59F5DX6BZZPB1VTQB3 -->

---
Entry: Claude Code (caleb) 2026-05-08T09:04:05.056663+00:00
Role: scribe
Type: Note
Title: Pattern 5: v1.41.x patch cadence — four minor cuts cluster early, then 24 consecutive patches under one minor across the rest of the window

Spec: scribe

tags: #insight #recurrence

**Pattern statement.** The 22-day window's release-version ladder runs v1.37.x → v1.38.0 → v1.38.x → v1.39.0 → v1.40.0 → v1.40.x → v1.41.0 → v1.41.x and then stays at v1.41.x for the rest of the window. Four minor cuts (v1.38.0, v1.39.0, v1.40.0, v1.41.0) cluster across PRs 6, 8, 10, 13 — the first six wall-clock days of the window — and ship under arc 03 (zoom) and arc 06 (harpoon, quickselect) and arc 08 (graveyard). After v1.41.0 lands, no further minor cuts occur. Twenty-four consecutive v1.41.x patch versions land across PRs 14 through 37, distributed across arcs 03, 04, 05, 06, 07, and 08.

The pattern is recurrence-of-versioning-shape: minor cuts cluster at capability-introducing PRs early in the window; the post-v1.41.0 era is patch-only. The recurrence is *that the post-v1.41.0 patches recur 24 times in a row*, not that minor cuts recur (4 minors is a small sample). The catalogue counts and names; the *why* is `insight-emergent-properties`'s.

**Instance enumeration with arc-entry citations and version verification.**

The version map is verifiable from `Cargo.toml` post-merge plus the commit subjects' `(v1.X.Y)` parenthetical tags as catalogued at `history-overview`'s segmentation entry (= 01KR0TWHTC1MPK4KJ08Y9SPE6P). The map for the 22-day window:

- **v1.37.1 (PR #1, arc 04, 2026-04-30 17:08)** — `git markers: 1Hz safety-net poll for missed FSEvents (v1.37.1)`. *Pre-window minor base; this is a patch on v1.37.0.*
- **v1.37.2 (PR #4, arc 01, 2026-04-30 20:48)** — `shell: aliases work in :!cmd / ;cmd via $SHELL -i (v1.37.2)`. *Patch.*
- **v1.37.2 (PR #5, arc 02, 2026-04-30 22:53)** — `lazygit investigation + cursor-block fix (v1.37.2)`. *Same-version-tag as PR #4 — the cursor-block fix shipped at the same patch level the shell-aliases fix authored. Sequential merges did not bump the patch.*
- **v1.38.0 (PR #6, arc 03, 2026-05-01 19:47)** — `pane: ^a z fullscreen-toggle (zoom) for the bottom pane (v1.38.0)`. *Minor cut #1: zoom is a capability addition that justifies the minor.*
- **v1.38.1 (PR #7, arc 04, 2026-05-02 11:53)** — `limit: =git / =g shows files in git status (v1.38.1)`. *Patch on v1.38.x — `=git`/`=g` is a filter, not a capability that justifies a minor.*
- **v1.39.0 (PR #8, arc 06, 2026-05-02 18:04)** — `harpoon: per-project pinned working set + =h filter (v1.39.0)`. *Minor cut #2: harpoon is a capability addition.*
- **v1.40.0 (PR #10, arc 06, 2026-05-02 20:52)** — `quick select: ^a u labeled-overlay picker for pane output (v1.40.0)`. *Minor cut #3: quickselect is a capability addition. Two minor cuts inside arc 06 within the same calendar day; arc 06 is the only arc with two minors.*
- **v1.40.1 (PR #11, arc 05, 2026-05-02 21:48)** — `pager: scroll_max accounts for wrapped visual rows (v1.40.1)`. *Patch — wrap-accounting fix.*
- **v1.41.0 (PR #13, arc 08, 2026-05-03 02:41)** — `graveyard: R-undo + per-entry tar.zst + system trash cascade (v1.41.0)`. *Minor cut #4: the graveyard subsystem is a capability addition; this is the last minor cut of the 22-day window.*

**The post-v1.41.0 ladder — 24 consecutive patch versions across arcs 03, 04, 05, 06, 07, 08:**

- v1.41.1 = PR #14 (arc 08; routing fix to PR #13)
- v1.41.2 = PR #15 (arc 04; basename collision + ^C-route)
- v1.41.3 = PR #16 (arc 05; :fg pager seeding)
- v1.41.4 = PR #17 (arc 05; pager n/N multi-col)
- v1.41.5 = PR #18 (arc 07; AGENTS.md rename + MCP hygiene)
- v1.41.6 = PR #19 (arc 07; codex resume)
- v1.41.7 = PR #20 (arc 05; alt-screen scroll bundle)
- v1.41.8 = PR #21 (arc 07; codex MCP config)
- v1.41.9 = PR #22 (arc 03; pane shutdown)
- v1.41.10 = PR #23 (arc 05; help yf)
- v1.41.11 = PR #24 (arc 04; jump git change)
- v1.41.12 = PR #25 (arc 06; input dispatch + key-trace)
- v1.41.13 = PR #26 (arc 03; dim unfocused pane)
- v1.41.14 = PR #27 (arc 04; git staged-vs-unstaged)
- v1.41.15 = PR #28 (arc 08; huge dir cap)
- v1.41.16 = PR #29 (arc 03; skip cursor block)
- v1.41.17 = PR #30 (arc 08; vt100 panic recovery)
- v1.41.18 = PR #31 (arc 08; vt100/ratatui upgrade)
- v1.41.19 = PR #32 (arc 06; chord priority)
- v1.41.20 = PR #33 (arc 05; pager visual line mode)
- v1.41.21 = PR #34 (arc 03; top overlay focus)
- v1.41.22 = PR #35 (arc 05; D opens pager)
- v1.41.23 = PR #36 (arc 05; substring search)
- v1.41.24 = PR #37 (arc 07; MCP socket project-scoped)

**Instance count: four minor cuts (v1.38.0, v1.39.0, v1.40.0, v1.41.0) plus 24 consecutive v1.41.x patches.** The minor-cut count is small (4); the patch-cadence recurrence is the load-bearing observation (24 consecutive patches under one minor).

**Notes on cadence and pattern boundary.**

- *Minor cuts cluster early.* The first minor (v1.38.0) lands on day 2 of the window; the last minor (v1.41.0) lands on day 4. The four minors span *48 hours of the 22-day window*. The post-v1.41.0 era is the remaining ~18 calendar days of merge activity, all under v1.41.x. The clustering is observable; whether it reflects an early phase of capability-additions giving way to a later phase of refinement-and-correction is a question for `insight-emergent-properties`.

- *Arc affiliation of minor cuts.* The four minors land across three arcs (arc 03 once, arc 06 twice, arc 08 once). Arcs 01, 02, 04, 05, 07 do not get a minor cut in the 22-day window. Arc 04 (git-integration) is notable for *not* getting a minor: the five arc-04 PRs span Day-0 to Day-7 of the window and ship at v1.37.1, v1.38.1, v1.41.2, v1.41.11, v1.41.14 — all patches. Arc 05 (pager-surface) is similarly all-patches across its eight PRs (v1.40.1, v1.41.3, v1.41.4, v1.41.7, v1.41.10, v1.41.20, v1.41.22, v1.41.23). Whether the all-patches arcs differ structurally from the minor-introducing arcs is a question for `insight-emergent-properties`.

- *The two-minor arc.* Arc 06 is the only arc with two minor cuts (PR #8 v1.39.0 harpoon; PR #10 v1.40.0 quickselect), both on the same calendar day, separated by 2 hours and 48 minutes. Two distinct capability-introducing PRs back-to-back, each cutting its own minor. The cadence within arc 06's α-phase is observable; the catalogue does not promote it to a sub-shape (one arc, two events).

- *The closing-ladder cadence.* The arc-08 story-tail (= 01KR3A23E11K8F7VNVSM5XY6M2) framed this factually: *"the v1.41.x cadence — one minor cut per arc-α PR; four 1.41.x patches in between for unrelated work landing in arcs 03/05/08; PR #25 at v1.41.12, PR #32 at v1.41.19."* The brief carried this observation forward. The arc-06 story-tail's framing is the arc-grain source; the cumulative-grain reading this entry assembles is broader: 24 consecutive patches, distributed across all five arcs whose PRs land after v1.41.0 (arcs 03, 04, 05, 06, 07, 08 — only arcs 01 and 02 conclude before v1.41.0).

- *Why this is recurrence and not drift.* No PR's commit subject misnames its own version — every `(v1.X.Y)` tag in the commit subject matches the post-merge `Cargo.toml` value. The pattern is at the *project release shape* level, not at the per-PR-description level. Insight-drift's closure entry (= 01KR3BQHTCQ7YGHWQJ2ZE4PJQ4) places this material here implicitly via the boundary rule: no per-PR misnaming at moment of merge means the observation is not drift.

- *Why the catalogue counts but does not interpret.* The pattern's emergent-property reading would name *what kind of release-cadence shape this is* (release-pressure-as-merge-velocity-signal; SemVer-as-honest-versioning; capability-introductions-as-mood-marker; etc.). Each of those is tier-4 and forbidden here. The catalogue states: minor cuts are 4; consecutive v1.41.x patches are 24. The shape recurs (the *recurrence* is the 24 consecutive patches without a minor). The interpretation is `insight-emergent-properties`'s.

- *No additional instances.* Verification did not reveal a pre-window or post-window context that changes the count. PR #1's v1.37.1 is the first version-tag in the window; PR #37's v1.41.24 is the last. The window's release ladder is closed.

Provenance:
- `history-overview` segmentation entry = 01KR0TWHTC1MPK4KJ08Y9SPE6P (full version-to-PR mapping; one source of truth for the version ladder).
- arc-06 story-tail entry not directly read this session; the brief's quote of arc-06 story-tail (*"the v1.41.x cadence — one minor cut per arc-α PR; four 1.41.x patches in between"*) is the framing source. Arc-06 story-tail entry = 01KR2GYQPQRX08SV980SPHHZ80 (cited at arc-08 story-tail provenance block, second-stop reference).
- arc-08 story-tail entry = 01KR3A23E11K8F7VNVSM5XY6M2 (closing-ladder framing; *"v1.41.0 → v1.41.18 as the closing ladder"*).
- Minor-cut PRs:
  - arc-03 PR #6 entry = 01KR108QNEEG64J8W8XJERJTZG (zoom; v1.38.0).
  - arc-06 PR #8 entry source: not directly read this session; arc-06's framing entry = 01KR2G8042HWE419X0ESWKN205 (cited at arc-06 PR #25 provenance, second-stop reference).
  - arc-06 PR #10 entry = 01KR2GH1D9QCGDPZEMWW09R898 (quickselect; v1.40.0; cited at insight-drift Pattern B).
  - arc-08 PR #13 entry = 01KR38VEGHFT9JGRDCXXBFX8V1 (graveyard; v1.41.0; cited at insight-drift Pattern D).
- `insight-drift` closure entry = 01KR3BQHTCQ7YGHWQJ2ZE4PJQ4 (boundary-rule placement: no per-PR misnaming, so this is recurrence not drift).
- `insight-recurrence` framing entry = 01KR3CSQ2YHQ2TD8EAE6DJCTS3.

<!-- Entry-ID: 01KR3D8RH5DNYC37WSGFVETXT3 -->

---
Entry: Claude Code (caleb) 2026-05-08T09:05:58.875678+00:00
Role: scribe
Type: Note
Title: Pattern 6: Implicit-machinery-chain — three instances across three arcs (one within-arc 04, one within-arc 05, one cross-arc 03→05) where commit subjects don't name "this enables that"

Spec: scribe

tags: #insight #recurrence

**Pattern statement.** One PR establishes infrastructure (a struct field, a helper function, a parser shape, a focus-routing model). A later PR consumes that infrastructure (extends the struct, calls the helper, parses richer output, builds on the routing model). The consumer's commit subject and commit body do not name the establisher PR; the chain is real and observable from the diffs but lives only in the code's evolution. The shape is *the chain is implicit*: each PR's text is locally legible, but the cumulative chain that makes a later capability cheap is visible only at cross-PR or cross-arc grain.

This pattern is the inverse of Pattern 4 (named-then-fixed bracket): in the bracket, the open-side PR *names* the issue the close-side PR will fix; in the implicit-machinery-chain, the establisher PR *does not name* what its infrastructure will eventually enable. The two patterns are not mutually exclusive — instance 4's PR #18 names a future fix in BUGS.md AND canonicalizes the marker file PR #37 will consume — but the implicit-machinery-chain pattern's identity is the *not-naming* part.

**Instance enumeration with arc-entry citations.**

1. **Arc 04 within-arc machinery chain — PR #1's `git_files` map → PR #7's filter consumer → PR #24's bracket-jumper consumer → PR #15's `parse_porcelain_statuses` extraction → PR #27's struct-refactor consumer.**
   - PR #1 (`fix/git-marker-1hz-poll`, commit cd8df2e, 2026-04-30 17:08 UTC) builds a 1Hz git poll on `AppState`, populating the `git_files` map.
   - PR #7 (`feat/limit-git`, commit f3ddaf2, 2026-05-02 11:53 UTC) reuses `git_files` for the `=git`/`=g` filter. PR #7's CHANGELOG names the machinery directly: *"the filter stays live as the 1Hz git poll updates `git_files`."*
   - PR #24 (`feat/jump-git-change`, commit 762a0a6, 2026-05-05 16:26 UTC) consumes the same `git_files` map for `]g`/`[g` navigation. PR #24's CHANGELOG names the consistency claim: *"Reuses the same `git_files` map the listing markers consume, so detection is consistent with what the user sees."*
   - PR #15 (`fix/git-status-and-pane-ctrl-c`, commit 5999261, 2026-05-04 11:26 UTC) extracts `parse_porcelain_statuses` as a pure-parser function with five unit tests. The parser is the contract.
   - PR #27 (`feat/git-staged-vs-unstaged`, commit 4e2afd9, 2026-05-06 16:51 UTC) extends the parser's return type from enum to struct, lands cleanly because PR #15 made the parser pure. The five PR-#15 tests get rewritten in-place against the new struct getters; three new tests land for the staged-only / partially-staged / conflict shapes.

   The chain has five PRs spanning seven calendar days. PR #7 and PR #24 *do* name `git_files` in their CHANGELOG entries (both consumers acknowledge the establisher contract by name); PR #27 does *not* name PR #15's parser-purity refactor as the precondition that makes its struct refactor cheap. The chain is partially-named in commit text (the consumer-of-shared-data-map half) and fully-implicit in the parser-extraction-then-extension half. The arc-04 story-tail (= 01KR13CJ5XS5VREYA4741JHDSQ) frames this factually: *"None of the commits says 'this enables that.' But the chain is real, and it's what lets the arc read as additive rather than thrashing."*

   *Cite: arc-04 story-tail = 01KR13CJ5XS5VREYA4741JHDSQ; arc-04 PR #1 entry = 01KR12W1M20SQW3QXT8VC09REK; arc-04 PR #7 entry = 01KR12XTG7E5TC0RNTJ65G67T7; arc-04 PR #15 entry = 01KR130775Q4PKYEN6FE1743DJ; arc-04 PR #24 entry = 01KR1327VZTQAYNNPMBCTC3SSM; arc-04 PR #27 entry = 01KR134PZSQDAFVJK3M35FTKXF.*

2. **Arc 05 within-arc machinery chain — two parallel sub-chains within `PagerView`'s field-accretion.**
   - Sub-chain A: PR #11 (`fix/pager-wrap-bottom`, commit 7b941a4, 2026-05-02 21:48 UTC) lands a `last_body_w: std::cell::Cell<u16>` field on `PagerView` to make wrap-aware `scroll_max` work. That field becomes part of the struct's permanent furniture. PR #33 (`feat/pager-visual-line-mode`, commit cf9e8ff, 2026-05-06 21:35 UTC) lands a `visual: Option<VisualSelection>` field on the same struct — sibling state to `last_body_w`, neither disturbing the other. The arc-05 story-tail (= 01KR2ANRAEFWWR5W9FQP11A0DB) frames this factually: *"PR #33 doesn't have to refactor `PagerView` to add visual mode; it adds a field next to the field PR #11 added, and the struct expands. This is what enables phase γ being cheap."*
   - Sub-chain B: PR #16 (`fix/fg-tail`, commit 34907a3, 2026-05-04 15:48 UTC) introduces the seed-from-buffer pattern (render `task.buffer` into the pager and call `scroll_to_bottom_auto()` before handing the buffer off). PR #35 (`feat/D-opens-pager-in-top-pane`, commit c243549, 2026-05-06 23:53 UTC) ships `display_in_pane`, which the arc-05 tail describes as *"a parallel of `edit_in_pane` for the read path, taking a listing row and launching it into a pty in the top overlay, sharing focus with the bottom pane."* The seed-from-buffer pattern PR #16 introduced is structurally upstream of PR #35's pane-launching mechanism.

   Both sub-chains are within-arc-05; both are implicit at commit-subject level (PR #33's commit subject does not name PR #11's `last_body_w` field; PR #35's commit subject does not name PR #16's seed-from-buffer pattern). The arc-05 story-tail makes the two sub-chains explicit and the implicit-naming observation factual. *Cite: arc-05 story-tail = 01KR2ANRAEFWWR5W9FQP11A0DB; arc-05 PR #11 entry = 01KR2A121DSV81GM4EBCKAVAAM; arc-05 PR #16 entry = 01KR2A2XY61GKZ1W52XQWGFBAH; arc-05 PR #33 entry = 01KR2AAX12XSNRNZPTXJT2TXJA; arc-05 PR #35 entry = 01KR2AD5PV989H58E49E5D18NM.*

3. **Cross-arc 03 → arc 05 machinery chain — PR #34's overlay-focus model → PR #35's launching mechanism.**
   - PR #34 (`fix/top-overlay-focus-switch`, commit 8e9fb2c, 2026-05-06 23:37 UTC) teaches the overlay-vs-pane focus model: `;cmd` overlays can share focus with the bottom pane, with `^a-j`/`^a-k` chord keys bridging the two.
   - PR #35 (`feat/D-opens-pager-in-top-pane`, commit c243549, 2026-05-06 23:53 UTC) lands 16 minutes later. The arc-05 story-tail (= 01KR2ANRAEFWWR5W9FQP11A0DB) frames this factually: *"Without PR #34, opening `$PAGER` as a top overlay traps focus in the overlay; with PR #34, the same spawn produces the docs-and-claude-side-by-side workflow PR #35's CHANGELOG names. The arc-03 → arc-05 link isn't visible in either commit. It's visible in the fact that PR #35 ships at all without re-doing PR #34's work."*

   The chain crosses arc boundaries: PR #34 is arc 03's last PR; PR #35 is arc 05's late PR. PR #35's commit subject and CHANGELOG name the workflow ("docs-and-claude-side-by-side") but do not name PR #34 or the overlay-vs-pane focus model. The 16-minute gap is the closest cross-arc consumer-of-establisher gap in the eight-arc record. *Cite: arc-05 story-tail = 01KR2ANRAEFWWR5W9FQP11A0DB; arc-03 PR #34 entry = 01KR10JBACRS3Z71WTHGBVCPJM; arc-05 PR #35 entry = 01KR2AD5PV989H58E49E5D18NM.*

**Instance count: three.** Three instances across four arc affiliations (arc 04 within-arc; arc 05 within-arc with two sub-chains; arc 03 → arc 05 cross-arc). The within-arc-05 instance (instance 2) carries two sub-chains under one pattern instance — sub-chain A (`last_body_w` → `visual`) and sub-chain B (seed-from-buffer → `display_in_pane`) are both within-arc-05 PagerView field-accretion shape and the catalogue holds them as one pattern instance with two sub-chains.

**Notes on chain shape and pattern boundary.**

- *Implicit-vs-explicit at consumer-side.* Across the three instances, the consumer-side commit text is partially explicit and partially implicit. Arc-04's chain has *partial naming*: PR #7 and PR #24 name `git_files` in their CHANGELOG entries; PR #27 does not name PR #15's parser-purity refactor. Arc-05's chain is *implicit* in commit subjects but partially-named in code-level: PR #33's `visual` field is added next to `last_body_w` without commit-message acknowledgement; PR #35's `display_in_pane` mirrors `edit_in_pane` without commit-message acknowledgement of PR #16's seed-from-buffer pattern. Arc 03 → arc 05's chain is *fully implicit* at commit-message level: neither PR #35's commit subject nor its CHANGELOG names PR #34 or the focus model. The implicit-vs-explicit shading is a sub-shape consideration; the catalogue does not promote it to separate sub-patterns (three instances, with the implicit-vs-explicit shading varying continuously, is too thin for sub-categorization).

- *Time-grain spread.* From 16 minutes (cross-arc 03→05 instance 3) to seven calendar days (within-arc-04 PR #1 → PR #27 spans Day-0 to Day-7). The time-grain spread is wide; what holds across all three is the *implicit chain* shape — establisher ships, consumer ships, the relationship lives in the diff alone (or partially in CHANGELOG cross-references that name the data structure but not the establisher PR).

- *No 1:N or N:1 chains in the catalogue.* All three instances are 1:1 or 1:1+1 chains (one establisher, one or two consumers per chain). Verification did not reveal a 1:N chain (one establisher consumed by N>2 distinct consumer PRs in the eight arcs) or an N:1 chain (N>1 establishers culminating in one consumer). The catalogue does not promote-from-absence; the absence is observable.

- *Why this is recurrence-not-drift.* No establisher PR mis-describes its own diff at moment of merge; the establishers' commit subjects accurately name what their PRs introduce (a poll, a filter, a parser, a focus model). The implicit-chain shape is at the consumer-side description-of-relationship level, not at the per-PR misnaming-at-merge level. The boundary holds.

- *The arc-04 chain partially crosses Pattern 1 (bundle-as-shape).* PR #15 is a Pattern-1 bundle instance (basename-collision parser-extraction + ^C-route guard). The parser-extraction half of PR #15 is what enables PR #27's later struct refactor. The bundling and the chain are both real; the same PR contributes to two patterns through its parser-extraction half.

- *Boundary with Pattern 4 (named-then-fixed bracket).* Pattern 4 is *issue-named-then-fixed*: open-side text exists, close-side fix removes/lifts the text. Pattern 6 is *infrastructure-built-then-consumed*: no open-side issue-text exists; the establisher's commit subject describes its own PR accurately, not as setup for a future PR. The two patterns are different shapes despite both being cross-PR observations.

- *No additional instances.* The arc-07 substrate-vs-registration observation (the arc-07 story-tail's "one socket, two registration files" framing) is *not* an implicit-machinery-chain in this pattern's sense — the substrate (`spyc --mcp` proxy + Unix socket + `discover_live_socket` walk) was established BEFORE the 22-day window and the codex-side parallel implementation built explicitly atop it. PR #19, PR #21, PR #37 all *name* the shared substrate. This is explicit-machinery-share, not implicit-machinery-chain. Captured factually as a non-instance.

Provenance:
- arc-04 story-tail = 01KR13CJ5XS5VREYA4741JHDSQ (instance 1 framing; "*None of the commits says 'this enables that'*").
- arc-04 PR #1 entry = 01KR12W1M20SQW3QXT8VC09REK (instance 1 establisher).
- arc-04 PR #7 entry = 01KR12XTG7E5TC0RNTJ65G67T7 (instance 1 first consumer).
- arc-04 PR #15 entry = 01KR130775Q4PKYEN6FE1743DJ (instance 1 parser-extraction; also Pattern 1 bundle instance).
- arc-04 PR #24 entry = 01KR1327VZTQAYNNPMBCTC3SSM (instance 1 second consumer).
- arc-04 PR #27 entry = 01KR134PZSQDAFVJK3M35FTKXF (instance 1 struct refactor consumer).
- arc-05 story-tail = 01KR2ANRAEFWWR5W9FQP11A0DB (instance 2 framing; both sub-chains; instance 3 framing).
- arc-05 PR #11 entry = 01KR2A121DSV81GM4EBCKAVAAM (instance 2 sub-chain A establisher).
- arc-05 PR #16 entry = 01KR2A2XY61GKZ1W52XQWGFBAH (instance 2 sub-chain B establisher).
- arc-05 PR #33 entry = 01KR2AAX12XSNRNZPTXJT2TXJA (instance 2 sub-chain A consumer).
- arc-05 PR #35 entry = 01KR2AD5PV989H58E49E5D18NM (instance 2 sub-chain B consumer; instance 3 consumer).
- arc-03 PR #34 entry = 01KR10JBACRS3Z71WTHGBVCPJM (instance 3 establisher).
- arc-07 story-tail = 01KR2JM67RTQHQYN0223GTKH1V (substrate-vs-registration shape; named at provenance as the non-instance contrast).
- `insight-recurrence` Pattern 1 entry = 01KR3CW3DBHPTB6K8R8047TBCP (boundary with bundle-as-shape; PR #15 cross-listed there).
- `insight-recurrence` Pattern 4 entry = 01KR3D5B59F5DX6BZZPB1VTQB3 (boundary with named-then-fixed bracket).
- `insight-recurrence` framing entry = 01KR3CSQ2YHQ2TD8EAE6DJCTS3.

<!-- Entry-ID: 01KR3DC7E4B0JC1NN212PYVT56 -->

---
Entry: Claude Code (caleb) 2026-05-08T09:07:47.151252+00:00
Role: scribe
Type: Closure
Title: Closure: insight-recurrence catalogue assembled — six patterns, observations flagged for insight-trajectory and insight-emergent-properties

Spec: scribe

tags: #insight #recurrence

The catalogue is assembled. Six patterns across the eight-arc record. Per-pattern instance counts:

- **Pattern 1: Bundle-as-shape (recurrence reading)** — 6 instances across 5 arcs. Cross-referenced to insight-drift Pattern B (= 01KR3BCQXGGB20V8C6Y6Z1Y944) for the misnaming-at-merge enumeration; this thread claims the recurrence reading and adds the sub-shape distribution observation (2-2-1-1 across noticed-while-shipping / shared-infrastructure / equal-weight / rename-plus-groundwork).
- **Pattern 2: Supersession-acknowledgement** — 3 canonical instances at 3 grains × 3 registers (3.5h-silent / 49min-explicit-reframing / 25min-behavior-described), plus 1 long-grain instance at six calendar days (silent, cross-arc). Total: 4 instances. The arc-08 story-tail's three-grain × three-register matrix is the load-bearing structure.
- **Pattern 3: BUGS.md SMALL/MAYBE-to-FIXED lift** — 6 verified instances across 5 arcs (one brief candidate dropped: arc-06 PR #25 adds a fresh FIXED entry without a SMALL precursor). Genesis instance (arc-02 PR #12) is structurally upstream; five lift instances drain the bucket.
- **Pattern 4: Named-then-fixed bracket** — 3 instances at 3 time grains (one-PR with cross-arc upstream open-side; 49 minutes; two days). PR #18 → PR #37 carries the only instance with weighted design options at the open-side.
- **Pattern 5: v1.41.x patch cadence** — 4 minor cuts cluster in the first 48 hours of the window; 24 consecutive v1.41.x patches recur across the rest of the window. The recurrence is the 24 consecutive patches under one minor.
- **Pattern 6: Implicit-machinery-chain** — 3 instances across 3 arc-affiliations (within-arc 04, within-arc 05 with two sub-chains, cross-arc 03 → 05). Three instances span time grains from 16 minutes to 7 calendar days.

**Six patterns, ~25 instances total** depending on how the count carries the genesis instances and the 24-patch sequence (the sequence-as-recurrence is one shape with 24 elements; each minor cut is a separate observation).

**What this catalogue contributes to the network.**

- *A name for the project-shape patterns the arcs flagged piecemeal.* Each arc's per-PR drift-findings sections and story-tails surfaced specific instances, but the *shape* — *the bucket-drain pattern recurs six times*, *the supersession-acknowledgement shape recurs at three grains and three registers*, *the implicit chain recurs across arcs* — is visible only at the cumulative grain. This thread names the shape in a register the arcs themselves did not own.

- *A correction to the brief's seven-pattern list.* Verification dropped *feature-plus-immediate-hotfix* as a standalone pattern (one true instance, already counted under supersession-acknowledgement) and dropped *arc-06 PR #25* as a SMALL-to-FIXED lift instance (PR #25 adds-to-FIXED without a SMALL precursor). Brief's seven becomes six; brief's seven instances under Pattern 3 become six. Conservative honesty is the analyst register's discipline.

- *A canonical placement for cross-cutting observations.* Insight-drift's closure entry placed three observables (bundle-as-shape, supersession-acknowledgement, BUGS.md SMALL-to-FIXED lift) explicitly. This thread's catalogue holds those three plus three more (named-then-fixed bracket, v1.41.x patch cadence, implicit-machinery-chain). The placement of bundle-as-shape (in both threads, with cross-references) versus supersession-acknowledgement (here only, cross-referenced from insight-drift's pattern-boundary notes) versus BUGS.md SMALL-to-FIXED lift (here only, no cross-reference needed) holds.

- *A factual base for the next two insight threads.* Patterns recur; the *why* is interpretation; the interpretation is the next two threads' work. The catalogue counts and names; the analyst register's discipline at tier-2 is to refuse the *why* questions and hand them forward with citations.

**Cross-thread observation for `insight-trajectory`'s author (Phase 10C, next session).**

`insight-trajectory` will read the eight-arc record against stated plans (ROADMAP.md, the gap analysis at PR #5's `notes/lazygit-gap-analysis.md` and `notes/lazygit-ux-catalogue.md`, the BUGS.md catalog post-PR-#12-harvest, the charter at `ROADMAP.md:3-23`, the architecture description at `onboarding-architecture` entry 0). Its tier-3 question is: *do the project's actual moves track its stated trajectory?* This catalogue's six patterns differ in their relationship to stated plans; the trajectory thread should note the differences:

- **Pattern 3 (SMALL/MAYBE-to-FIXED lift) is highly correlated with stated plans.** PR #12's harvest creates the BUGS.md SMALL/MAYBE buckets from PR #5's gap analysis suspects. Five of six lift instances drain entries that *originated* in stated-plan documents (PR #5's gap analysis → PR #12's harvest → BUGS.md → eventual lift). The lift recurrence is partially a property of the gap-analysis methodology PR #5 introduced; whether the lifts execute against the plan PR #5 wrote, or whether the plan was a post-hoc rationalization of work that would have happened anyway, is squarely tier-3 territory. The trajectory thread can ask: *of the SMALL/MAYBE entries the harvest created, how many were eventually lifted within the 22-day window vs. left in the bucket?* If most are lifted, the trajectory tracks the plan; if most are left, the trajectory diverges. This catalogue does not have the bucket-residual count; the trajectory thread's verification step will produce it.

- **Pattern 5 (v1.41.x patch cadence) is correlated with the SemVer policy that exists nowhere as a stated plan but is observed everywhere as a working pattern.** No ROADMAP entry says *"minor cuts will cluster early; patches will dominate the rest of the window."* The pattern is observable; the policy is implicit. The trajectory thread can ask: *was the post-v1.41.0 patch-only cadence anticipated, or did it emerge from the work itself?* Tier-3 territory; this catalogue declines.

- **Pattern 1 (bundle-as-shape) is uncorrelated with stated plans.** No ROADMAP entry or charter section addresses bundling-discipline. The recurrence is purely a project-shape property at the merge-grain; it has no stated-plan vantage. The trajectory thread can confirm or refute that *no stated-plan document anticipates bundling-shape*; if confirmed, Pattern 1 is a tier-2-only observation that does not extend to tier-3.

- **Pattern 2 (supersession-acknowledgement) is uncorrelated with stated plans.** No ROADMAP entry addresses *how-supersedences-should-be-acknowledged-in-commit-messages*. The three registers are observable; the variance is observable; the absence-of-a-stated-policy on acknowledgement-register is a tier-3 observation the trajectory thread can make explicit (*the trajectory of acknowledgement-register varies across PRs without a stated policy governing the choice*).

- **Pattern 4 (named-then-fixed bracket) is correlated with stated plans, partially.** Instance 3 (PR #18 → PR #37) carries weighted design options at the open-side; the open-side text *is itself a stated micro-plan* with a marked preferred option. PR #37 implements the marked option. The bracket *is* the stated-plan trajectory at micro-scale. Instances 1 and 2 do not carry weighted options at the open-side, so they are not stated-plan trajectories in the same sense. The trajectory thread can ask: *do all named-then-fixed brackets represent stated micro-plans, or is the option-weighting at PR #18 unique?* Tier-3 territory.

- **Pattern 6 (implicit-machinery-chain) is uncorrelated with stated plans by definition.** The pattern's identity is the consumer-side *not-naming* of the establisher; if a stated plan named the chain, the chain wouldn't be implicit. Verification might reveal an architecture document or design-note that names some of the chains — `onboarding-architecture` entry 0 (= 01KR0P4W3ED1QZ8F44PFB2WPDZ) describes the current end-state surfaces — but the per-PR commit-message-grain implicit-naming is the pattern's defining property. The trajectory thread can confirm whether the chains track *some* stated plan even if the per-PR commits don't name it.

**Cross-thread observation for `insight-emergent-properties`'s author (Phase 10D).**

`insight-emergent-properties` will read the catalogue's recurrences and ask *what kind of property each recurrence is*. The tier-4 question takes a recurrence as data and produces a property name (the recurrence reflects *X working pattern* / *Y release dynamic* / *Z communication style*). The catalogue does not pre-name the properties — that's the next thread's job — but flags which patterns have tier-4 weight:

- **Pattern 2 (supersession-acknowledgement) has heavy tier-4 weight.** Three registers at three grains, plus a fourth long-grain instance, is a substantial recurrence. The variance in acknowledgement register (silent / behavior-described / explicit-reframing) is the most acute candidate for emergent-property naming in the catalogue. The temptation to interpret *why* the registers vary was itself the most acute tier-2-discipline test of this thread. The emergent-properties author can name; this thread declines.

- **Pattern 3 (SMALL/MAYBE-to-FIXED lift) has tier-4 weight, partially shared with tier-3.** As noted in the trajectory observation above, this pattern is also correlated with stated plans (PR #5's gap analysis, PR #12's harvest). The emergent-property reading would name the property at the working-discipline grain (*the maintainer treats the BUGS.md bucket as the queue; landing PRs drain it*); the trajectory reading would track the lifts against the stated-plan trajectory. Both readings are real; both are tiered. The emergent-properties thread should not collapse the property into the trajectory.

- **Pattern 5 (v1.41.x patch cadence) has heavy tier-4 weight.** 24 consecutive patches under one minor across ~18 calendar days is a substantial recurrence. The emergent-property reading would name the property at the release-dynamics grain — possibilities include *capability-introductions cluster early; later-window work is refinement-and-correction*; *the maintainer's SemVer policy treats minor as capability-additions only*; *the post-v1.41.0 work is by-shape patches even when the diff weight is feature-comparable to earlier minors*. The catalogue does not name; the emergent-properties author can.

- **Pattern 6 (implicit-machinery-chain) has tier-4 weight.** The implicit-naming-pattern recurs across three arcs and varies from fully-implicit to partially-named. The emergent-property reading would name the property at the communication-style grain or the working-discipline grain. Captured factually here for naming there.

- **Pattern 1 (bundle-as-shape) has shared tier-1 / tier-4 weight.** Insight-drift's Pattern B already named the misnaming-at-merge aspect (tier-1 drift); the recurrence reading here named the project-shape sub-shape distribution (tier-2 recurrence). The emergent-property reading would name *what kind of project produces this bundling distribution* — the 2-2-1-1 sub-shape distribution is rich enough to support property-naming.

- **Pattern 4 (named-then-fixed bracket) has tier-4 weight, with a singular sub-shape (weighted-options-at-open-side) that is one-instance-only.** The emergent-property reading would name the bracket-discipline property; the singular sub-shape might be its own observation (*PR #18's open-side carries a stated micro-plan with weighted options; the close-side at PR #37 implements the marked option*). The catalogue holds this as one instance with a noted sub-shape; the emergent-properties author can name.

**Per-pattern instance counts, final.**

- Pattern 1: 6 instances across 5 arcs (cross-referenced to insight-drift Pattern B for enumeration).
- Pattern 2: 4 instances total (3 canonical at 3 grains × 3 registers; 1 long-grain at six days).
- Pattern 3: 6 instances across 5 arcs (1 genesis + 5 lifts; 1 brief candidate dropped after verification).
- Pattern 4: 3 instances at 3 time grains.
- Pattern 5: 4 minor cuts + 24 consecutive v1.41.x patches.
- Pattern 6: 3 instances across 3 arc-affiliations (one with two sub-chains).

**The thread is left OPEN.** Cross-arc references from `insight-trajectory` and `insight-emergent-properties` will cite this catalogue's per-pattern entry IDs.

Provenance:
- `insight-recurrence` framing entry = 01KR3CSQ2YHQ2TD8EAE6DJCTS3.
- `insight-recurrence` Pattern 1 entry = 01KR3CW3DBHPTB6K8R8047TBCP.
- `insight-recurrence` Pattern 2 entry = 01KR3CZEM22Y5BRT1F2VQZ6EKZ.
- `insight-recurrence` Pattern 3 entry = 01KR3D2G1S7DXYSPDZDRXQBPDX.
- `insight-recurrence` Pattern 4 entry = 01KR3D5B59F5DX6BZZPB1VTQB3.
- `insight-recurrence` Pattern 5 entry = 01KR3D8RH5DNYC37WSGFVETXT3.
- `insight-recurrence` Pattern 6 entry = 01KR3DC7E4B0JC1NN212PYVT56.
- `insight-drift` closure entry = 01KR3BQHTCQ7YGHWQJ2ZE4PJQ4 (boundary rule for placement of cross-cutting observables).
- `history-overview` framing entry = 01KR0TRFWT9W6WMFHC49YSW0BG.
- `history-overview` segmentation entry = 01KR0TWHTC1MPK4KJ08Y9SPE6P.
- arc-08 story-tail = 01KR3A23E11K8F7VNVSM5XY6M2 (the directly-handed-off three-grain × three-register supersession summary).
- All eight arc threads' per-PR entries with their drift-findings sections plus story-tails are the load-bearing infrastructure this catalogue was built atop; the per-pattern entries above cite the specific arc-entry ULIDs.

<!-- Entry-ID: 01KR3DFHA7FRV3BXEH2Z8SFJQN -->

---
Entry: Claude Code (caleb) 2026-05-08T09:09:17.589660+00:00
Role: scribe
Type: Note
Title: Tail: convergence shapes — a small handful of PRs sit at the intersection of multiple pattern instances, and the convergences are observable

Spec: scribe

tags: #insight #recurrence #tail

The catalogue holds six patterns and ~25 instances. The instances overlap: a small handful of PRs surface in two or three of the catalogue's patterns, and the overlaps are not noise — they are convergence shapes worth seeing.

PR #29 is the most-converged-upon successor PR in the eight-arc record. It carries Pattern 2 instance 1 (3.5-hour silent supersession of PR #26) *and* Pattern 2 instance 4 (six-day silent cross-arc supersession of PR #5). Two supersessions converge on the same diff: a within-arc twin at 3.5 hours and a cross-arc lineage at six days, both silent. PR #29's three-condition cursor-block guard is the *close* in both cases. The convergence is not a coincidence of timing — PR #29's *generalization* is what makes both supersessions possible at once. The narrow PR #5 fix and the per-cell DIM modifier PR #26 added are *both* superseded by the same broadening of policy. One generalization, two supersessions, one diff. The arc-03 story-tail named the within-arc twin and the cross-arc lineage separately; reading them together at insight-grain reveals the convergence.

PR #18 is the most-converged-upon establisher PR. It carries Pattern 1 instance 5 (bundle-as-shape: rename + MCP hygiene + the deferred-design BUGS.md note), Pattern 4 instance 3 open-side (the BUGS.md SMALL note that PR #37 will close two days later), and the canonical-marker-file infrastructure (`.spyc-context-<pid>.json`) that PR #37 will consume. Three patterns converge in one bundle: the bundle, the bracket, and the implicit-infrastructure-for-future-fix. The arc-07 story-tail observed this factually with a sentence the catalogue can re-read at insight-grain: *"the BUGS.md note pre-existed the codex-parity expansion that made the note mandatory; the canonical marker file PR #37 needed was already in the codebase by the time PR #19 and PR #21 widened the codepath that fed it."* Three structural moves at once, none of them named in the commit subject as preconditions for any other.

PR #31 is the most-converged-upon close PR. It carries Pattern 2 instance 3 (49-minute explicit-reframing supersession of PR #30), Pattern 3 instance 6 (MAYBE-to-FIXED lift for mode-2026 and OSC-8), and Pattern 4 instance 2 close-side (the 49-minute bracket whose open-side was PR #30's MAYBE block). Three patterns converge in one diff. The diff retracts PR #30's deferral framing, lifts the harvest's MAYBE entries to FIXED, and supersedes PR #30's panic-recovery as the proper-fix-rather-than-defensive-fix. *"Smaller than I'd previously framed it"* is five words doing three patterns' worth of work.

PR #15 sits at a smaller intersection: Pattern 1 instance 1 (bundle-as-shape) *and* Pattern 6 instance 1 partial node (the parser-extraction half is what enables PR #27's later struct refactor). Two patterns, one PR. The bundling (basename-collision parser-extraction + ^C-route guard) and the chain-establishment (parser-purity refactor that makes PR #27 cheap) live in the same diff because the same diff that bundles also establishes the contract.

Four convergence PRs (PR #29, PR #18, PR #31, PR #15) carry between 2 and 3 patterns each. Twenty-some other PR slots in the catalogue carry one pattern each. The convergence-vs-singleton ratio is small — convergence is the exception, not the rule — but the convergences are not random. PR #29 converges *because* its generalization closes multiple supersessions. PR #18 converges *because* its bundle also brackets and infrastructures. PR #31 converges *because* the upgrade *is* the proper fix the prior PR's MAYBE block deferred. PR #15 converges *because* the parser-extraction is both bundle-half and chain-establisher. Each convergence has a structural reason that lives in the diff; the catalogue holds the *what* (these PRs sit at multiple-pattern intersections); the *why-each-particular-convergence-takes-the-shape-it-does* is `insight-emergent-properties`'s.

What the convergence shapes contribute to the network's reading: a future analyst entering the catalogue at any single pattern can follow the convergence cross-references and find that the same handful of PRs reappear. PR #29 in Pattern 2 (twice). PR #18 in Patterns 1 and 4. PR #31 in Patterns 2, 3, 4. PR #15 in Patterns 1 and 6. The catalogue's per-pattern entries cite each convergence factually; this tail names *that the convergences exist as a class*, which no per-pattern entry could see.

The negative-space observation in insight-drift's tail (= 01KR3BT6MNZMWRMHX14QMYZ86Y) named what kinds of drift the 22-day window does not produce. The convergence-shape observation here is its complement at recurrence-grain: what the catalogue's overlapping instance-citations look like when read together. Five-of-six patterns share at least one instance with another pattern; only Pattern 5 (v1.41.x patch cadence) holds entirely separate instances. The convergence-density is itself observable.

Provenance:
- `insight-recurrence` Pattern 1 entry = 01KR3CW3DBHPTB6K8R8047TBCP (PR #18 instance 5; PR #15 instance 1).
- `insight-recurrence` Pattern 2 entry = 01KR3CZEM22Y5BRT1F2VQZ6EKZ (PR #29 instances 1 and 4; PR #31 instance 3).
- `insight-recurrence` Pattern 3 entry = 01KR3D2G1S7DXYSPDZDRXQBPDX (PR #31 instance 6).
- `insight-recurrence` Pattern 4 entry = 01KR3D5B59F5DX6BZZPB1VTQB3 (PR #18 instance 3 open-side; PR #31 instance 2 close-side).
- `insight-recurrence` Pattern 6 entry = 01KR3DC7E4B0JC1NN212PYVT56 (PR #15 parser-extraction half).
- `insight-recurrence` closure entry = 01KR3DFHA7FRV3BXEH2Z8SFJQN.
- arc-03 story-tail = 01KR11S8RG29J98QKN1H0VAA6W (PR #29 within-arc twin and cross-arc lineage observation source).
- arc-07 story-tail = 01KR2JM67RTQHQYN0223GTKH1V (PR #18's three-structural-moves-at-once observation source).
- arc-08 story-tail = 01KR3A23E11K8F7VNVSM5XY6M2 (PR #31's "Smaller than I'd previously framed it" verbatim).
- arc-04 story-tail = 01KR13CJ5XS5VREYA4741JHDSQ (PR #15's parser-extraction-as-chain-establisher observation source).
- `insight-drift` negative-space tail = 01KR3BT6MNZMWRMHX14QMYZ86Y (the negative-space methodology this tail's complement observation echoes).

<!-- Entry-ID: 01KR3DJ9KJY1T9FFP8KDEPTBJ1 -->

---
Entry: Claude Code (caleb) 2026-05-08T09:10:22.938294+00:00
Role: scribe
Type: Note
Title: Tail: the story-tails carried what this thread needed — recurrence is a cumulative-grain pattern, and the arc-grain story-tails were the upstream that made the cumulative-grain reading legible

Spec: scribe

tags: #insight #recurrence #tail

Insight-drift's infrastructure tail (= 01KR3BVVYN37WDWJ5A3D8A5XWH) named that *the arc-side per-PR drift-findings practice* did the load-bearing work for that catalogue. Each arc's per-PR entries flagged drift instances locally; the catalogue's job was assembly. This thread's catalogue had a different upstream. The recurrence patterns came mostly from the *arc story-tails* — the cumulative-grain entries at the arc layer — not from the per-PR drift-findings sections.

The arc-08 story-tail (= 01KR3A23E11K8F7VNVSM5XY6M2) is the load-bearing example. It said: *"three such instances at three time grains (3.5 hours, 49 minutes, 25 minutes) with three different acknowledgement registers (silent in arc 03; behavior-described in PR #14; explicit reframing in PR #31)."* That sentence is the structure of Pattern 2. The arc-08 story-tail wrote it as cumulative-grain observation about how the eight-arc network now reads. The brief carried that observation forward. This thread's Pattern 2 entry verifies the three instances against per-PR entries and adds a fourth long-grain instance the arc-03 story-tail had named separately. But the *matrix* was already there. The arc-08 story-tail did the structural work; this thread did the verification and the additive observation.

The arc-04 story-tail (= 01KR13CJ5XS5VREYA4741JHDSQ) carried Pattern 6's first instance. Its sentence: *"None of the commits says 'this enables that.' But the chain is real, and it's what lets the arc read as additive rather than thrashing."* That observation is the entire structure of Pattern 6 in one paragraph. The arc-05 story-tail (= 01KR2ANRAEFWWR5W9FQP11A0DB) carried the within-arc PagerView field-accretion sub-chains and the cross-arc PR #34 → PR #35 chain. The arc-07 story-tail (= 01KR2JM67RTQHQYN0223GTKH1V) carried Pattern 4 instance 3 and the substrate-vs-registration distinction. The arc-03 story-tail (= 01KR11S8RG29J98QKN1H0VAA6W) carried both the cross-arc PR #5 → PR #29 silent supersession and the within-arc PR #26 → PR #29 silent supersession. The arc-06 story-tail (cited at provenance throughout, not directly read this session) carried the v1.41.x patch cadence framing for Pattern 5.

Five of the six patterns trace their primary cumulative-grain observation to a story-tail. Pattern 1 (bundle-as-shape, recurrence reading) is the exception — it traces to insight-drift's Pattern B, which itself assembled from per-PR drift-findings. The recurrence reading of Pattern 1 is the cross-thread reading insight-drift's closure entry placed; this thread carried the placement. So Pattern 1's upstream is *insight-drift's closure entry* rather than an arc story-tail; even there, the drift-findings practice that fed insight-drift was per-PR, but the cross-thread placement decision was at the cumulative-grain insight-drift closure.

The asymmetry between insight-drift's upstream and this thread's upstream is structural, not coincidental. *Drift* is a per-PR observation: each instance is its own moment of misnaming-at-merge. *Recurrence* is a cross-PR observation: the pattern is the count and the cross-grain structure. Per-PR entries can flag drift at the moment they author the per-PR view; recurrence requires the cumulative grain — the story-tail layer — to *see* the count. The arc story-tails are what made the recurrence patterns legible *as patterns*. Without them, this thread would have needed to derive the patterns from per-PR drift-findings alone, which would have produced a thinner catalogue (the per-PR entries flagged individual instances; only the story-tails *summed*).

What this means for the next two insight threads. `insight-trajectory`'s upstream will likely be a mix: stated-plan documents (ROADMAP.md, the gap analysis, BUGS.md post-harvest, the charter) plus the arc-grain trajectory observations the story-tails carry (e.g., arc-05's lazygit-ux-catalogue §4 deferred-question; arc-07's substrate-vs-registration distinction; arc-04's machinery-chain-without-naming as enabling-additive-rather-than-thrashing). The mix favors the trajectory-thread author; trajectory questions cross both layers. `insight-emergent-properties`'s upstream will run heavier on cumulative-grain synthesis: emergent properties are tier-4, structurally upstream of any single arc, like Pattern F (span-phrasing) was for insight-drift. The infrastructure-tail forecast there held: emergent-properties will run heavier on cumulative observation than assembly.

This thread's contribution at the cumulative grain itself, separate from arc-grain assembly: the convergence-shape observation the prior tail named, the brief-candidate verification (dropping arc-06 PR #25 from Pattern 3; consolidating feature-plus-immediate-hotfix into Pattern 2), and the cross-product observations (Pattern 2's grain × register matrix, Pattern 4's grain × open-side-shape ladder, Pattern 1's sub-shape distribution). These are not work the story-tails did; they are the work the catalogue does *atop* the story-tails. Two-thirds story-tail-assembly, one-third cumulative-grain refinement. The infrastructure-tail's *five-sixths assembly / one-sixth cumulative observation* forecast for this thread held roughly true.

The thread is left OPEN.

Provenance:
- `insight-drift` infrastructure tail = 01KR3BVVYN37WDWJ5A3D8A5XWH (the upstream-naming pattern this tail re-uses with a different upstream).
- arc-03 story-tail = 01KR11S8RG29J98QKN1H0VAA6W (Pattern 2 instances 1 and 4 upstream).
- arc-04 story-tail = 01KR13CJ5XS5VREYA4741JHDSQ (Pattern 6 instance 1 upstream).
- arc-05 story-tail = 01KR2ANRAEFWWR5W9FQP11A0DB (Pattern 6 instance 2 sub-chains and instance 3 upstream).
- arc-06 story-tail entry = 01KR2GYQPQRX08SV980SPHHZ80 (Pattern 5 v1.41.x cadence framing upstream; cited at provenance throughout, not directly read this session).
- arc-07 story-tail = 01KR2JM67RTQHQYN0223GTKH1V (Pattern 4 instance 3 upstream).
- arc-08 story-tail = 01KR3A23E11K8F7VNVSM5XY6M2 (Pattern 2 three-grain × three-register matrix upstream; the load-bearing example).
- `insight-drift` closure entry = 01KR3BQHTCQ7YGHWQJ2ZE4PJQ4 (Pattern 1 cross-thread placement upstream).
- `insight-recurrence` framing entry = 01KR3CSQ2YHQ2TD8EAE6DJCTS3.
- `insight-recurrence` convergence tail = 01KR3DJ9KJY1T9FFP8KDEPTBJ1.

<!-- Entry-ID: 01KR3DM9DBRV6MBA4D516KRR12 -->

---
Entry: Claude Code (caleb) 2026-06-08T23:56:04.702064+00:00
Role: critic
Type: Note
Title: Framing (window-2): #38–#311 continuation opens — six second-window recurrence shapes named, strangler-fig is the window's dominant new shape, tier-2 register and insight-drift boundary carried unchanged

Spec: critic

tags: #insight #recurrence #window-2

This entry opens the **second-window continuation** of `insight-recurrence`. Window-1 (entries above, framing = 01KR3CSQ2YHQ2TD8EAE6DJCTS3) catalogued six event-shape patterns across PRs #1–#37 (the first 22 days). This continuation reads the **#38–#311 substrate** — the next ~32 days, ~273 PRs, reconstructed into seven extended arc threads plus six new `history-seg-*` segment threads, with a Phase-3 readable-arc layer in `history-synthesis` (opener = 01KTMNAVSB3410Y59S3FDMZCHQ) and a second-window spine in `history-overview` (framing = 01KTMN7X7FV45E8E05DN1RV719; segment map = 01KTMN9MRB31A0C8ZWSXX5M5ZQ). The window is ~7.4× the PR volume of window-1, and the recurrence shapes it carries are correspondingly larger-grained: where window-1's recurrences were per-PR shapes counted across single-digit instances, window-2's dominant recurrences are *campaign-scale* — a single migration shape instantiated twice at scale, a phase-numbered train of a dozen same-shape PRs, a verbatim-cut shape recurring ~45×.

**Register and boundary — carried unchanged.** The tier-2 job is unchanged: count instances, name the shape they share, cite real moment entry_ids, do not interpret *why* (tier-4, `insight-emergent-properties`), do not predict (tier-5), never attribute motive to the maintainer. The boundary with `insight-drift` holds: a recurrence is also a drift only if it carries a misnaming-at-the-moment-of-merge aspect. None of the six window-2 shapes below qualify as drift — they are project-working-pattern recurrences, not per-PR descriptive lies — so this continuation is recurrence-only, with no cross-thread re-litigation needed (the window-2 drift fuel the segmentation flagged — the v1.60 same-day #76→#77 reversal, the #139/#140 throttles vestigialized by #141, the markdown #103→#108 width churn — is description-vs-diff material for a drift continuation, not recurrence material). One contrast worth stating up front: window-1 had **no migration-shape pattern at all**; the strangler-fig (Pattern 1 below) is genuinely new to window-2, not an evolution of anything window-1 named.

**The six second-window shapes named, with instance-count claims to verify per entry.**

- **1. Strangler-fig migration (NEW — the window's dominant recurrence).** Grow-alongside → parity → flip → drop, instantiated twice at campaign scale: the MVU runtime (`history-seg-refactor-mvu`, grown behind green CI alongside the old `App::run` loop) and the gix backend (`history-seg-gix-migration`, behind a `SPYC_GIT_BACKEND` escape hatch). *Two instances; window-1 had no equivalent shape.*
- **2. Phase-numbered PR-train (EVOLUTION of window-1 Pattern 6).** A long ordered train of small same-shape PRs under one named campaign, with explicit phase numbers: MVU phase −1..6 + Phases D/E + last-mile; mod-extract pr1..12; decompose-mod-* waves. *Window-1 Pattern 6 (implicit-machinery-chain) was the consumer-not-naming-the-establisher shape; window-2 makes the chain explicit, numbered, and plan-anchored. Two-to-three canonical campaign-trains to verify.*
- **3. Verbatim-relocation recurrence.** The behavior-identical file-split move (`+N/-N` near-balanced diff, `git mv` + `mod…;use…;`, "no behavior change") recurs ~45× across module-decomposition and MVU leaf/handler extraction. *A single micro-shape recurring at the highest instance count in either window.*
- **4. Per-X-repetition-then-generalize.** Do the per-instance work N times, then collapse it into a registry/abstraction: per-agent dispatch repeated (claude/codex/gemini/agy) then collapsed into the `AgentProfile` registry at #176; per-source pager session-skeletons repeated then collapsed into `PagerStream` at #309–#311. *Two surfaces; the abstraction's payoff measured in both (zot at +18min; three sources migrated).*
- **5. Plan-doc-then-execute.** A committed plan doc lands in `history-seg-docs-planning`, then a segment executes it: MVU_PLAN.md (#196) → seg-refactor-mvu; PANE_RECOVERY_PLAN/STARTUP_TABS (#92/#93) → arc-03; V1_5_PLAN → arc-05; AUTO_APPROVAL_PLAN (#86). *Recurs across the window; the plan-as-recorded-rationale spine of the whole reconstruction.*
- **6. Patch-corridor release cadence (EVOLUTION of window-1 Pattern 5).** Window-1 Pattern 5 caught the v1.41.x corridor at 24 patches; window-2 extends that same corridor to v1.41.37, then opens a *longer* v1.50.x corridor (50.0 → 50.82), then v1.51.4, then v1.56.0 at the gix close. *The patch-under-one-minor shape recurs at larger scale; minor cuts stay rare.*

**Methodology — unchanged from window-1.** Each entry states the shape, enumerates instances against the new history substrate (citing the per-moment ULIDs the synthesis arcs carry inline), names the verified count, flags sub-shapes and boundary questions, and contrasts with the window-1 pattern where illuminating. Where a window-2 shape is an evolution of a window-1 pattern, the entry says which and how the shape changed. Two tails follow the closure: a convergence tail (PRs/threads at the intersection of multiple window-2 shapes) and a legibility tail (how the per-moment + synthesis layers made the cumulative reading possible at this volume).

**What this continuation is NOT for** — unchanged: not motive attribution (the most acute temptation here is *why* the maintainer chose strangler-fig twice, or *why* the phase-train is numbered — forbidden at every tier); not emergent-property naming (*the strangler-fig recurs* is tier-2; *it recurs because parity-tests-behind-a-flag is this maintainer's risk-management property* is tier-4); not trajectory-against-plans (the plan-doc-then-execute recurrence correlates heavily with stated plans — that correlation is tier-3, `insight-trajectory`'s; this thread counts the execute-after-plan shape, not whether the moves track the plan); not prediction.

Provenance:
- `insight-recurrence` window-1 framing = 01KR3CSQ2YHQ2TD8EAE6DJCTS3; window-1 closure = 01KR3DFHA7FRV3BXEH2Z8SFJQN (six-pattern window-1 catalogue this continuation extends).
- `insight-recurrence` window-1 Pattern 5 (v1.41.x cadence) = 01KR3D8RH5DNYC37WSGFVETXT3; Pattern 6 (implicit-machinery-chain) = 01KR3DC7E4B0JC1NN212PYVT56 (the two window-1 patterns Patterns 6 and 2 below evolve from).
- `history-overview` second-window framing = 01KTMN7X7FV45E8E05DN1RV719; segment map = 01KTMN9MRB31A0C8ZWSXX5M5ZQ (7 extended arcs + 6 new seg threads, ~123 moments, partition-strict routing).
- `history-synthesis` opener = 01KTMNAVSB3410Y59S3FDMZCHQ (the readable-arc layer this continuation reads for inline moment ULIDs).
- Window-2 substrate threads read for evidence: history-seg-refactor-mvu, -module-decomposition, -gix-migration, -performance, -markdown-rendering, -docs-planning; history-synthesis arcs for arc-01/03/04/05/06/07/08 continuations.
- Release ledger verified by `git log main --format='%s' | grep -oiE 'v1\.[0-9]+\.[0-9]+'`: v1.41.x runs to .37, v1.50.x to .82, plus v1.51.4 and v1.56.0; minor cuts v1.41.0 and v1.50.0 only across the span.

<!-- Entry-ID: 01KTMTMAB6G8JJAFFQF29XVZGB -->

---
Entry: Claude Code (caleb) 2026-06-08T23:56:58.982162+00:00
Role: critic
Type: Note
Title: Pattern 1 (window-2): Strangler-fig migration — two campaign-scale instances (MVU, gix) sharing one grow→parity→flip→drop shape; the window's dominant recurrence, with no window-1 equivalent

Spec: critic

tags: #insight #recurrence #window-2

**Pattern statement (NEW shape).** Two of window-2's largest engineering campaigns instantiate the same migration shape: a new implementation is **grown alongside** the legacy one behind a facade or a guard, brought to **parity** (proven byte-for-byte / behavior-equivalent), **flipped** to default, and then the legacy implementation is **dropped** with the escape hatch removed. The shape recurs **twice at campaign scale** in the window. Window-1 catalogued no migration pattern at all — this is genuinely new to window-2, not an evolution of any prior pattern.

**Instance enumeration.**

1. **MVU runtime migration (`history-seg-refactor-mvu`, #166–#274).** The grow-alongside is literal: the MVU runtime (single Message channel, effect-as-data, off-thread workers, draw accumulator) is grown *beside* the old busy-poll `App::run` loop, phase by phase, each phase behavior-equivalence-tested behind green CI, before the loop body is finally reshaped and the three update entry points collapse into one `App::update(msg)`. The "drop" is the terminal moment recording the Model/View/Update triad structurally complete. The parity discipline is the recorded "behavior-equivalence tests" gate plus the candid trade-off "deep loop/concurrency surgery before launch carries regression risk behavior-equivalence tests don't fully catch (timing, focus, stdin) — accepted, not closed." *Moment citations: 01KTMKVE85DEBMBWYCXY7YHP5E (MVU_PLAN.md APPROVED, the migration's recorded charter); 01KTMM60KR8W18TWXPXDGT9J8Y (the terminal collapse-to-one-entry-point drop). Synthesis arc: 01KTMNFSQP1XJ1TSXS545PR01E.*

2. **gix backend migration (`history-seg-gix-migration`, #283–#292).** The textbook instance, and the synthesis arc names the shape verbatim: "a planned 9-step strangler-fig replacing every `git` subprocess shell-out with the pure-Rust `gix` crate, behind a single facade." Grow-alongside = the `src/git/` facade seam built around the legacy organism first (01KTMMHJ879C24FY2WYYT9F1SF), then gix added additively `default-features = false` (01KTMMJB955RF16D842WFFEBYP). Parity = the status spike "runs only from the parity tests, not the live status path," proving byte-for-byte equivalence before the flip. Flip = #287 behind `SPYC_GIT_BACKEND=subprocess`, recorded verbatim as "a one-release-cycle safety valve so a field regression in the gix flip is a flag flip, not a code restore. Removed in PR 9" (01KTMMMBVCGADASG74RTHKWY97). Drop = #292 removes the last `Command::new("git")`, deletes the escape hatch "exactly as promised," adds the `no_subprocess_git_in_production` guard test, net diff −178 — "the migration ends by removing more than it adds, the recorded signature of a completed strangler-fig" (01KTMMTF0MQ96P7BVN7QQPVBNS). *Synthesis arc: 01KTMNJD6GJM90D0WTTS0X28XR.*

**Instance count: two.** Both are full grow→parity→flip→drop, not partial. The diff/diff-render sub-arc *inside* gix (#289 model → #290 renderer → #291 wire) is a fractal of the same caution — "first an isolated, testable gix data model … no UI flip yet, then a pure in-house renderer not yet wired, then the live wire" — so the shape recurs *within* one instance as well, but the catalogue counts the two campaign-level instances and notes the fractal sub-instance rather than promoting it to a third.

**Sub-shape: the escape-hatch register differs across the two instances.** This is the load-bearing recurrence-reading the per-segment threads could not see, because each owns only one instance:

- **gix uses an explicit runtime flag** (`SPYC_GIT_BACKEND`) as its safety valve, with a recorded one-release-cycle soak window that "lasted exactly the planned window" and was closed on schedule. The rollback unit is a flag flip.
- **MVU uses green CI + behavior-equivalence tests** as its safety valve — there is no runtime flag; the "old loop still runs alongside" *is* the hatch, phase by phase, and each phase stays individually revertable ("a uniform add-the-wake / delete-the-floor split so each migration stays revertable"). The rollback unit is a per-phase PR revert.

Two instances, two safety-valve registers (runtime-flag vs revertable-phases-behind-CI), one migration shape. The recurrence is the shape; the register varies. (*Why* the maintainer chose a flag for gix and CI-phases for MVU is tier-4 — captured factually here.)

**Contrast with window-1.** Window-1's closest analogue was Pattern 6 (implicit-machinery-chain) — establish-then-consume — but that shape never *replaced* a legacy implementation; it accreted capability. The strangler-fig is the first recurrence in either window where the recurring shape's defining feature is *retiring the thing it grew alongside*. The window-1 catalogue had no migration vantage; this is the dominant new shape of the larger window.

**Boundary notes.**
- *Not a phase-train (Pattern 2).* The MVU instance is *also* a phase-numbered train (Pattern 2 instance 1), but the strangler-fig shape and the phase-train shape are distinct readings of the same campaign: Pattern 1 reads the grow→parity→flip→drop lifecycle; Pattern 2 reads the ordered-small-same-shape-PRs delivery mechanism. gix is a strangler-fig (Pattern 1) but is recorded as a "9-step" not a numbered-phase train, so it sits in Pattern 1 only. Same campaign, two shapes — the catalogue does not double-count; it cross-references (see Pattern 2 and the convergence tail).
- *Drift boundary.* Neither instance carries a misnaming-at-merge aspect — the slugs (`refactor/...`, gix PR subjects) describe the moves accurately, and the plan docs record the lifecycle openly. Recurrence-only; not a drift.

Provenance:
- `history-seg-refactor-mvu`: 01KTMKVE85DEBMBWYCXY7YHP5E (PR #196 MVU_PLAN.md APPROVED, strangler-fig charter), 01KTMM60KR8W18TWXPXDGT9J8Y (PR #267–#274 terminal collapse/drop); synthesis arc 01KTMNFSQP1XJ1TSXS545PR01E.
- `history-seg-gix-migration`: 01KTMMHJ879C24FY2WYYT9F1SF (#283 facade seam = grow-alongside), 01KTMMJB955RF16D842WFFEBYP (#284 gix added additively), 01KTMMMBVCGADASG74RTHKWY97 (#286–#287 parity spike then flip behind SPYC_GIT_BACKEND), 01KTMMPRPTSP0J1MTJCT3WW6N1 + 01KTMMR7X2GJAMQE0HD0C0HA6R (#289–#291 model→render→wire fractal sub-instance), 01KTMMTF0MQ96P7BVN7QQPVBNS (#292 drop + guard test + escape-hatch removed, −178 net); synthesis arc 01KTMNJD6GJM90D0WTTS0X28XR.
- Contrast: `insight-recurrence` window-1 Pattern 6 = 01KR3DC7E4B0JC1NN212PYVT56 (implicit-machinery-chain, the closest window-1 analogue; accretes rather than replaces).
- `insight-recurrence` window-2 framing = 01KTMTMAB6G8JJAFFQF29XVZGB.

<!-- Entry-ID: 01KTMTNYS83V5A7PY2MJVV60MF -->

---
Entry: Claude Code (caleb) 2026-06-08T23:58:01.027679+00:00
Role: critic
Type: Note
Title: Pattern 2 (window-2): Phase-numbered PR-train — the implicit-machinery-chain made explicit and numbered; three campaign-trains where small same-shape PRs run under one ordered, plan-anchored campaign

Spec: critic

tags: #insight #recurrence #window-2

**Pattern statement (EVOLUTION of window-1 Pattern 6).** A long ordered train of small, same-shape PRs runs under one named campaign with an **explicit ordering** — phase numbers, `prN` counters, or a wave sequence — where each PR is individually small and revertable and the *campaign* is the unit of architectural intent. Window-1's Pattern 6 (implicit-machinery-chain = 01KR3DC7E4B0JC1NN212PYVT56) was the shape where one PR establishes machinery and a later PR consumes it *without the consumer's commit naming the establisher* — the defining property was the *implicitness*. Window-2 inverts that property at campaign scale: the chains are now **explicit, numbered, and plan-anchored**. The same establish-then-build structure recurs, but the naming that window-1 noted as absent is now the campaign's organizing spine.

**Instance enumeration — three campaign-trains.**

1. **MVU phase-train (`history-seg-refactor-mvu`, #166–#274).** The most explicit numbering in either window. The MVU_PLAN.md records an eight-phase (−1..6) ordered migration; the moments execute it phase by phase, lowest-risk-first, with the ordering recorded verbatim: "The mechanical extractions in Phase 1 below buy ~70% of the review-ability win for ~5% of the architectural risk." Phases run −1/0 (Focus value + test re-baseline, 01KTMKWF071QA1Y5MD5TMMRGHC) → 1 (Message channel, 01KTMKXA7ZKKR421NTPRJ9EQ3M) → 2 (Scheduler, 01KTMKY262M6Y6GH25DYKBY33H) → 3 (every async source migrated, ten PRs folded, 01KTMKZ502R8JYARTMFMKSGB72) → 4 (Effect vocabulary, 01KTMM03Q5F5EA3VEQSJQJZYN0) → 5 (state into Model, fifteen PRs, 01KTMM1A4SE4HRHFQ97PRJCZYD) → 6/D/E (01KTMM27M59N08NB1HSWCESQTR, 01KTMM35YXDZ69C08M78F4XKBB, 01KTMM409Y3RTZPDTVWPQWR6EW) → last-mile (01KTMM4ZMPP9DCW3R3NK5BDA93, 01KTMM60KR8W18TWXPXDGT9J8Y). *The phase number IS the ordering contract.*

2. **mod-extract `prN` train (`history-seg-module-decomposition`, #248–#259).** "twelve verbatim cuts off app/mod.rs," one cut per PR, numbered pr1..12, governed by REFACTOR_PLAN.md Phase-1: "verbatim move + a `mod ...; use ...;` import — no behavior change … Each was one PR" (01KTMMGZWER9304EZM82KTEZMQ). A second wave (decompose-mod-*, #275–#281, 01KTMMHZCN03GA638EHKKCY50E) and per-subsystem waves (#297–#308, 01KTMMM1322W32NGAH5H7MWYEP / 01KTMMN1CSG84TPZ4ZEFN9K62B / 01KTMMPARGNTSQB2Z67G6KBKQ0) extend the same train under the campaign banner. *Synthesis arc: 01KTMNH17RVXAWHF9SJ2Z7MCP9.*

3. **gix 9-step sequence (`history-seg-gix-migration`, #283–#292).** Recorded as a "planned 9-step strangler-fig," with the order pre-committed in code — the dependency-add comment names "PR 6" as where `worktree-mutation` will be enabled, and the flip commit names "Removed in PR 9" before PR 9 exists (01KTMMJB955RF16D842WFFEBYP, 01KTMMMBVCGADASG74RTHKWY97). *The future PR is named by number before it lands — the antithesis of window-1's implicit chain. Synthesis arc: 01KTMNJD6GJM90D0WTTS0X28XR.*

**Instance count: three campaign-trains.** A fourth candidate — the CI-caching campaign (arc-01 #57/#58/#61/#64/#65/#66/#69, "seven PRs in ~36 hours," 01KTMMPDE6S4PA834YDR24SX1H) — is a tight same-shape cluster but is *not* phase-numbered or ordering-contracted; it reads as a burst, not a numbered train. The catalogue does not promote it to a phase-train instance; it is folded here as a same-shape cluster without the explicit-ordering property that defines this pattern. (Three trains, one folded near-miss.)

**The evolution, stated precisely.** Window-1 Pattern 6's three instances (arc-04 git_files chain, arc-05 PagerView accretion, arc-03→05 overlay-focus) shared "None of the commits says 'this enables that'" — the arc-04 story-tail's load-bearing line. Window-2's three trains share the opposite: the plan doc says "this enables that," the PR slug carries the phase number, and the *next* PR is named by index before it exists. The establish-then-build structure is identical; the **communication register flipped from implicit to explicit-and-numbered**. Naming *that the register flipped across windows* is the cumulative-grain observation no single segment owns. (*Why* it flipped — campaign volume forcing legibility? the plan-doc habit? — is tier-4.)

**Sub-shape: phase-numbering carries a recorded risk-ordering, not just a sequence.** The MVU train's numbering is explicitly risk-sorted ("lowest-risk first," the ~70%-win-for-~5%-risk quote), and the gix sequence is explicitly capability-gated (worktree-mutation feature enabled "exactly when needed" at PR 6). The number is not arbitrary; it encodes a risk/dependency order. The mod-extract `prN` train is the flatter instance — its ordering is "one self-contained concern per PR," dependency-light. So within the three-train recurrence there is a 2-1 split: two risk/dependency-ordered trains (MVU, gix), one flat-enumeration train (mod-extract). The catalogue notes the sub-split without promoting the single flat instance to its own shape.

**Boundary notes.**
- *Convergence with Pattern 1.* MVU and gix are *both* Pattern 1 (strangler-fig) and Pattern 2 (phase-train) — the migration lifecycle and the delivery mechanism are distinct readings of the same campaigns. mod-extract is Pattern 2 only (no legacy implementation retired; it is Pattern 3 verbatim-relocation delivered as a numbered train). See the convergence tail.
- *Drift boundary.* The numbered slugs describe their diffs accurately; no misnaming-at-merge. Recurrence-only.

Provenance:
- `history-seg-refactor-mvu`: 01KTMKVE85DEBMBWYCXY7YHP5E (eight-phase plan), 01KTMKWF071QA1Y5MD5TMMRGHC / 01KTMKXA7ZKKR421NTPRJ9EQ3M / 01KTMKY262M6Y6GH25DYKBY33H / 01KTMKZ502R8JYARTMFMKSGB72 / 01KTMM03Q5F5EA3VEQSJQJZYN0 / 01KTMM1A4SE4HRHFQ97PRJCZYD / 01KTMM27M59N08NB1HSWCESQTR / 01KTMM35YXDZ69C08M78F4XKBB / 01KTMM409Y3RTZPDTVWPQWR6EW / 01KTMM4ZMPP9DCW3R3NK5BDA93 / 01KTMM60KR8W18TWXPXDGT9J8Y (phases 0..6/D/E/last-mile).
- `history-seg-module-decomposition`: 01KTMMGZWER9304EZM82KTEZMQ (pr1..12), 01KTMMHZCN03GA638EHKKCY50E / 01KTMMM1322W32NGAH5H7MWYEP / 01KTMMN1CSG84TPZ4ZEFN9K62B / 01KTMMPARGNTSQB2Z67G6KBKQ0 (later waves); synthesis 01KTMNH17RVXAWHF9SJ2Z7MCP9.
- `history-seg-gix-migration`: 01KTMMJB955RF16D842WFFEBYP ("PR 6" pre-named), 01KTMMMBVCGADASG74RTHKWY97 ("Removed in PR 9" pre-named); synthesis 01KTMNJD6GJM90D0WTTS0X28XR.
- Folded near-miss: arc-01 CI-caching cluster 01KTMMPDE6S4PA834YDR24SX1H (same-shape burst, not phase-numbered).
- EVOLVES: `insight-recurrence` window-1 Pattern 6 = 01KR3DC7E4B0JC1NN212PYVT56 (implicit-machinery-chain; window-1 arc-04 story-tail's "None of the commits says 'this enables that'").
- `insight-recurrence` window-2 framing = 01KTMTMAB6G8JJAFFQF29XVZGB; Pattern 1 = 01KTMTNYS83V5A7PY2MJVV60MF.

<!-- Entry-ID: 01KTMTQSTE7QSH8WJ442ADX02F -->

---
Entry: Claude Code (caleb) 2026-06-08T23:58:49.331897+00:00
Role: critic
Type: Note
Title: Pattern 3 (window-2): Verbatim-relocation recurrence — the behavior-identical file-split micro-shape recurs ~45× across decomposition and MVU extraction; the highest instance count in either window

Spec: critic

tags: #insight #recurrence #window-2

**Pattern statement.** A single micro-shape recurs at the highest instance count of either window: the **verbatim relocation** — moving code from an oversized file into a new sibling module with a near-perfectly-balanced `+N/-N` diff, a `mod …; use …;` re-export (or a `git mv` of snapshot fixtures), and an explicit "no behavior change, no API change" claim. Each instance is one PR. The shape is the *unit of work* across two campaigns, recurring roughly **45 times** when the decomposition waves and the MVU leaf/handler extractions are summed.

**Instance enumeration — by wave, since per-PR ULIDs are folded at the moment grain.**

The reconstruction folds the individual verbatim cuts into per-wave moments (per the deep-history bounding rule), so the instance count is read off the recorded PR-range counts the moments carry:

- **mod-extract pr1..12 (#248–#259)** — "twelve verbatim cuts off app/mod.rs," one per PR, REFACTOR_PLAN.md Phase-1 verbatim: "verbatim move + a `mod ...; use ...;` import — no behavior change … Each was one PR." Diff signature recorded: "#248 +541/-502, #257 +431/-429, #258 +514/-501" — the near-balance is the verbatim fingerprint. **12 instances.** *01KTMMGZWER9304EZM82KTEZMQ.*
- **decompose-mod-* (#275–#281)** — "mod.rs split into run/bootstrap/proc/tests/util siblings," ~7 PRs, same verbatim signature plus a guard test `mod_rs_stays_decomposed`. **~7 instances.** *01KTMMHZCN03GA638EHKKCY50E.*
- **UI/render directory splits (#297,298,303,305,306)** — `foo.rs → foo/{mod,…}`, snapshot `.snap` fixtures `git mv`'d into new `snapshots/` subdirs "so the insta/TestBackend net stays intact"; biggest single file `ui/pager.rs` ~2954 lines → six `pager/*.rs`. **5 instances.** *01KTMMM1322W32NGAH5H7MWYEP.*
- **core subsystem splits (#299,300,301,302,304)** — mcp.rs (2190L), keymap/resolver.rs (1878L), key-dispatch, sessions, pane; "predominantly test-extraction." **5 instances.** *01KTMMN1CSG84TPZ4ZEFN9K62B.*
- **state.rs split (#307–#308)** — the 3907-line MVU Model into `state/{mod,apply,dispatch,git,listing,navigation,selection}.rs`; one non-verbatim prep PR (sub-structs) + one large verbatim directory conversion. **1 verbatim instance** (+1 prep, which is the *exception* below). *01KTMMPARGNTSQB2Z67G6KBKQ0.*
- **MVU Phase 1 & 2 leaf/handler extractions (#180–#191)** — "six leaf-struct extractions, then four handler extractions … each a one-PR, behavior-equivalent move." **~10 instances** in the MVU campaign proper, before the decomposition campaign even opens. *01KTMKTBHT9G2VBZNJZFY5EJM4; synthesis 01KTMNFSQP1XJ1TSXS545PR01E.*

**Instance count: ~40–45.** Summing the recorded wave-counts (12 + 7 + 5 + 5 + 1 + ~10) lands at ~40 verbatim relocations, ~45 if the smaller folded cuts in the decompose waves are counted individually. The catalogue states the count as **~45 with honest imprecision** — the exact figure depends on how the folded per-wave PRs are tallied, which the moment grain deliberately does not enumerate one-by-one. This is the single most-recurring micro-shape in either window by an order of magnitude (window-1's densest pattern, bundle-as-shape, had six instances).

**The recurrence reading the per-segment threads could not see: the verbatim shape has a recorded falsifiable signature.** Each instance is identifiable not by slug but by **diff balance** — the `+N/-N` near-equality is the fingerprint, and the synthesis arc verifies it stat-by-stat ("the verbatim-move signature," "diff balances are stat-verified"). The recurrence is therefore *machine-checkable*: a verbatim relocation is one whose insertions ≈ deletions and whose behavior tests are unchanged. This is a sharper recurrence than window-1's shapes, which were identified by reading slugs and CHANGELOGs; here the shape is in the diff arithmetic itself.

**Sub-shape: the exception that proves the verbatim rule.** The state.rs prep PR (#307, the sub-struct grouping) is recorded as "the only API-touching, non-mechanical edit in the slice (inferred from its 15-file +225/-229 ripple)" — i.e. the one PR in the campaign that is *not* verbatim, and it exists precisely to *create clean seams so the next PR can be verbatim*. The pattern thus carries a recurring **prep-then-verbatim** micro-shape at its hardest target: where a file is too coupled to cut cleanly, one non-verbatim seam-cutting PR precedes the verbatim relocation. One instance of this prep-exception (state.rs); the catalogue names it as a noted sub-shape, not a promoted recurrence.

**Boundary notes.**
- *Relationship to Pattern 2.* Verbatim relocation is the *content* of two of Pattern 2's phase-trains (mod-extract pr1..12, the MVU extraction phases). Pattern 2 reads the *ordered-train delivery*; Pattern 3 reads the *per-PR micro-shape inside it*. Same PRs, two grains — exactly the bundle-vs-bracket double-reading window-1 used for PR #18. The catalogue does not double-count: the train is one Pattern-2 instance; each cut is one Pattern-3 instance.
- *Drift boundary.* The verbatim slugs ("extract X", "split Y") describe the diffs accurately, and the "no behavior change" claim is test-backed; no misnaming-at-merge. Recurrence-only. (If any verbatim PR's diff had turned out *not* behavior-identical, that single PR would be drift fuel — but the guard tests and the recorded stat-verification report none in slice.)

Provenance:
- `history-seg-module-decomposition`: 01KTMMGZWER9304EZM82KTEZMQ (pr1..12, +N/-N stats), 01KTMMHZCN03GA638EHKKCY50E (decompose-mod-* + guard test), 01KTMMM1322W32NGAH5H7MWYEP (UI/render git-mv snapshots), 01KTMMN1CSG84TPZ4ZEFN9K62B (core subsystem test-extraction), 01KTMMPARGNTSQB2Z67G6KBKQ0 (state.rs prep-then-verbatim, the exception); synthesis 01KTMNH17RVXAWHF9SJ2Z7MCP9.
- `history-seg-refactor-mvu`: 01KTMKTBHT9G2VBZNJZFY5EJM4 (Phase 1&2 six-leaf + four-handler verbatim extractions); synthesis 01KTMNFSQP1XJ1TSXS545PR01E.
- `insight-recurrence` window-1 densest pattern for scale contrast: Pattern 1 bundle-as-shape = 01KR3CW3DBHPTB6K8R8047TBCP (6 instances).
- `insight-recurrence` window-2 framing = 01KTMTMAB6G8JJAFFQF29XVZGB; Pattern 2 = 01KTMTQSTE7QSH8WJ442ADX02F.

<!-- Entry-ID: 01KTMTSAVQNENHP8ZFWBKTWXN6 -->

---
Entry: Claude Code (caleb) 2026-06-08T23:59:43.066657+00:00
Role: critic
Type: Note
Title: Pattern 4 (window-2): Per-X-repetition-then-generalize — do the per-instance work N times, then collapse it into a registry/abstraction; two surfaces (AgentProfile #176, PagerStream #309–#311), payoff measured in both

Spec: critic

tags: #insight #recurrence #window-2

**Pattern statement.** A capability is implemented **per-instance, repeatedly** — the same shape copied N times across near-identical files or `match` arms — and then, once the repetition cost is visibly paid, **collapsed into a single abstraction** (a trait + registry, or a shared core), with the abstraction's payoff measured immediately by how cheaply the *next* instance lands. The shape recurs across **two distinct surfaces** in the window, each running the full repeat-then-generalize cycle.

**Instance enumeration — two surfaces, both complete cycles.**

1. **Per-agent dispatch → `AgentProfile` registry (`history-arc-07`, #176).** The repetition is recorded explicitly across the arc: gemini arrives as "the third `AgentKind`, carrying its own detection, resume, and session-discovery functions" (01KTMMR37MMRBBRMQK58RFN6R2); the transcript renderer is "reimplemented once per agent across three near-identical files (`codex_transcript.rs`, `claude_transcript.rs`, `agy_transcript.rs`)," with a shared tail-read helper fixing the same 100+ MB hang "twice, once per file" (01KTMMVJ6RR8RM6WNZ9HKQC5C8); agy onboards "the expensive way — detection, resume, status short-id, and a third parallel transcript file" (01KTMMY8R53BC7BJPEHKVWTRD0). Then the collapse: #176 folds "~10 per-agent `match AgentKind` dispatch sites" into "one `AgentProfile` trait plus `REGISTRY`," behavior-preserving ("all existing agent tests pass verbatim") (01KTMMZWBVK4X3QJP7SJW3ZEG2). *Synthesis arc: 01KTMNJRT9FD74NGXQXHH78A3B.*

2. **Per-source pager sessions → `PagerStream` (`history-arc-05`, #309–#311).** The repetition is the "hand-rolled session skeletons" each pager-filling feature carried; the collapse is "an object-safe trait plus shared spawn/wake/id-gate/drain core in `src/app/pager_stream.rs`, onto which transcript scrollback (#309), `:grep` (#310), and git-view diff/show/blame (#311) migrate, collapsing their hand-rolled session skeletons" (01KTMN2XSH67BNFQPAMTSFD81X). The recorded ARCHITECTURE.md line names the generalized result: "Off-thread read/parse is the default architecture for any feature that fills a pager from disk or compute." *Synthesis arc: 01KTMNG0CYN2NW1Y3RBR71J5FP.*

**Instance count: two surfaces.** Both are full cycles (repeat → collapse), not partial generalizations. The catalogue counts the two surfaces; it does not count each per-agent file or each pager source as a separate pattern instance — those are the *repetition phase* of the two instances, not instances themselves.

**The recurrence reading no single arc owns: the payoff is measured the same way on both surfaces — by the cost of the next instance.** This is the load-bearing cross-surface observation:

- **AgentProfile's payoff is measured at +18 minutes:** zot becomes the fifth agent "18 minutes later via 'one impl + one registry line, no dispatch-site edits,' with zero `src/app/mod.rs` changes against agy's tree-wide sweep hours earlier" (01KTMN0ZW93D5P7TPZCVMCWC0B). The before/after is explicit: agy (pre-collapse) touched the tree everywhere; zot (post-collapse) touched one impl + one line.
- **PagerStream's payoff is measured by three same-day migrations:** the three sources (#309/#310/#311) land *onto* the new trait in immediate succession, "collapsing their hand-rolled session skeletons" — the abstraction proves itself by absorbing three consumers at once.

Both surfaces validate the abstraction by the **marginal cost of the next consumer** — the recurrence is not just "abstract after N repeats" but "abstract after N repeats *and the very next instance is cheap, recorded as proof*." That measured-payoff signature is identical across the two surfaces and is visible only when the two are read together.

**Contrast with window-1 Pattern 6 (implicit-machinery-chain).** Window-1's chains *accreted* capability (PagerView field-accretion, git_files chain) but never reached a *collapse* — the machinery stayed parallel/additive. Window-2's per-X-then-generalize is the shape where the accretion *terminates in an abstraction that retires the repetition*. It is the natural successor to Pattern 6's accretion: window-1 accreted and stopped; window-2 accretes, then generalizes. (Indeed arc-07's framing records the collapse as "answering the arc-07 tail's open question about whether a per-peer registration layer would stay parallel or become parametric" — the window-1 question, answered parametric in window-2.)

**Sub-shape: partial-generalization-during-repetition.** Both surfaces show a *foreshadowing* sub-shape — a small piece is generalized mid-repetition before the full collapse: gemini's per-agent arrival already generalizes "one shared sub-concern — the closest-by-start-time picker — over a `SessionCandidate` trait" (01KTMMR37MMRBBRMQK58RFN6R2), and the transcript repetition shares a "tail-read helper" before the registry exists. So the recurrence carries an internal gradient: shared helpers emerge *during* the repetition, then the full trait emerges *after*. Two instances of this foreshadowing sub-shape (one per surface); noted, not promoted.

**Boundary notes.**
- *Distinct from Pattern 1 (strangler-fig).* Strangler-fig *replaces a legacy implementation*; per-X-then-generalize *unifies N parallel implementations of the same new capability*. AgentProfile did not replace a prior backend — it collapsed N copies of new agent-support code. Different shapes.
- *Drift boundary.* The #176 and #309–#311 slugs describe their collapses accurately; behavior-preservation is test-recorded. Recurrence-only.

Provenance:
- `history-arc-07` (synthesis 01KTMNJRT9FD74NGXQXHH78A3B): 01KTMMR37MMRBBRMQK58RFN6R2 (gemini per-peer + SessionCandidate foreshadow), 01KTMMVJ6RR8RM6WNZ9HKQC5C8 (three near-identical transcript files, hang fixed twice), 01KTMMY8R53BC7BJPEHKVWTRD0 (agy onboarded the expensive way), 01KTMMZWBVK4X3QJP7SJW3ZEG2 (#176 AgentProfile collapse, ~10 dispatch sites), 01KTMN0ZW93D5P7TPZCVMCWC0B (zot at +18min, payoff measured).
- `history-arc-05` (synthesis 01KTMNG0CYN2NW1Y3RBR71J5FP): 01KTMN2XSH67BNFQPAMTSFD81X (#309–#311 PagerStream collapse, three sources migrated).
- Contrast/successor: `insight-recurrence` window-1 Pattern 6 = 01KR3DC7E4B0JC1NN212PYVT56 (accretion without collapse; arc-07's parallel-or-parametric open question).
- `insight-recurrence` window-2 framing = 01KTMTMAB6G8JJAFFQF29XVZGB; Pattern 3 = 01KTMTSAVQNENHP8ZFWBKTWXN6.

<!-- Entry-ID: 01KTMTTZQK31H30M2RQ6J3FEY1 -->

---
Entry: Claude Code (caleb) 2026-06-09T00:00:44.459540+00:00
Role: critic
Type: Note
Title: Pattern 5 (window-2): Plan-doc-then-execute — a committed plan doc lands in docs-planning, then a segment executes it; recurs ~5× and is the recorded-rationale spine of the whole window

Spec: critic

tags: #insight #recurrence #window-2

**Pattern statement.** A committed planning document lands in `history-seg-docs-planning` (a `docs/*_PLAN.md` or a ROADMAP/CHANGELOG decision block), and a **separate engineering segment then executes it**. The plan precedes the code, the plan is the recorded design rationale, and the execution lives in a different thread from the planning. The shape recurs across the window as the **plan→execute handoff between the docs-planning segment and the engineering segments**. This is the recurrence that makes the entire #38–#311 reconstruction *recorded-dominant* rather than inference-dominant (the second-window spine notes the maintainer's "habit of committing plan docs and detailed CHANGELOG prose left a dense recorded record").

**Instance enumeration — five plan→execute handoffs across segment boundaries.**

1. **MVU_PLAN.md (#196) → `history-seg-refactor-mvu`.** The plan doc is filed as a typed Decision ("APPROVED — pre-2.0 / road-to-2.0 track") with the full eight-phase (−1..6) migration recorded; the seg-refactor-mvu phases then execute it phase by phase. The plan even records its own bug-class justification verbatim ("motivated by recurring, design-rooted bug classes (grounded in `BUGS.md`), not by aesthetics"). *Plan side: 01KTMKVE85DEBMBWYCXY7YHP5E (in seg-refactor-mvu, where the Decision lives); roadmap-reorg authorizing it: docs-planning 01KTMMWRE9YX58H2RXKYY58QVH ("take the low-risk decomposition now, hold only the deep MVU rewrite").*

2. **PANE_RECOVERY_PLAN + PANE_STARTUP_TABS_PLAN (#92/#93) → `history-arc-03`.** "Two pane plans land from external-contributor analysis: PANE_RECOVERY_PLAN tiers recovery by program kind, PANE_STARTUP_TABS_PLAN adds config-driven startup tabs while deferring real splits" (docs-planning 01KTMMRKSSJ8EN3RC6RB36RSM3). arc-03's toggle/recovery work executes against them, and the arc-03 synthesis explicitly hands the quit→relaunch recovery story to "docs/PANE_RECOVERY_PLAN.md (#92) / history-seg-docs-planning" (01KTMMSVHSFYKGPSD9R8NRFCZY). *Plan→execute across two threads.*

3. **V1_5_PLAN.md → `history-arc-05`.** The pager/task-viewer unification executes a recorded plan: "The recorded plan states the motivation: 'The same pager that handles `! cmd` capture should handle pane history'" (arc-05 01KTMMRS83NW2K9GASEKF5R1T1); the synthesis notes "landed host-first then promote then demote per the plan" in arc-03 (01KTMMMHWAHASBVT11Y96RBKSA). *Plan in docs/V1_5_PLAN.md, execution in the pager and pane arcs.*

4. **AUTO_APPROVAL_PLAN (#86) → deferred-but-recorded execution.** "PR #86 promotes a one-line BUGS wish into the v1.51 AUTO_APPROVAL plan and records a rejection on a security argument: 'Security features should not be built on regex against another tool's UI'" (docs-planning 01KTMMPA6HBDB91KMZPTBPNHX3). The execute side is named-and-deferred (the `:approvals` pager) — a plan→(deferred)execute instance where the handoff is recorded even though the code is pending.

5. **V1_70_PLAN "Mise en Place" (#114) → crate-split / MCP daemon protocol.** "reframes the MCP socket from v1.60's informal peer-discovery channel into a formal typed daemon protocol … and sequences a crate split before the protocol work" (docs-planning 01KTMMSRJ3DEDE2S4VWY5JHEND); the decomposition segment then carries the crate-split groundwork.

**Instance count: five plan→execute handoffs.** All five share the defining property: the plan is committed and recorded *before* the executing PRs, and the execution lands in a different segment thread. The positioning burst (#72/#73/#74, the "noun the agent operates on" thesis, 01KTMMKHEYZ540PV6VVZR33K79) is *not* counted as a plan→execute instance — it is a positioning/thesis doc with no single executing engineering segment; it propagates as a thesis (the synthesis arc's throughline) rather than handing off to one executor. The catalogue declines it as an instance and notes it as the thesis-propagation shape instead.

**The recurrence reading no single segment owns: the plan doc is the supersession ledger.** The docs-planning segment is where architectural *stances supersede each other before any code moves* — "architectural churn recorded before code." The v1.60 "CounterTop" plan is filed on a recursive-composition thesis (#76), reversed within ~4.5h to "siblings + mirror" (#77), then hardened (#79), "the recursion route … 'considered-and-rejected after design discussion with the user'" (01KTMMMQ1VY8ZERQ3NQAF89DN4); the whole v1.60→v1.70→Lean-2.0 plan spine is "each superseding the prior stance." So the plan→execute recurrence has a recorded-supersession sub-property: the *plan* carries the rejected-alternative, and the executing segment inherits only the surviving stance. The recurrence is not just "plan then build" — it is "the plan thread holds the rejected designs so the engineering thread doesn't have to." That division of recorded labor is the cumulative-grain observation.

**Tier boundary — load-bearing for this pattern.** Whether the executing segments *actually track* their plans (did MVU execute all eight phases? did the pane work match PANE_RECOVERY_PLAN's tiers?) is **tier-3, `insight-trajectory`'s question**, not this thread's. This pattern counts the *recurrence of the plan-precedes-execute-across-threads shape*; it does not assess fidelity. The five instances are counted by "a plan doc landed and a segment executed against it," not by "the execution matched the plan." (window-1 had no equivalent because window-1's planning was thinner — the gap analysis at PR #5 was the lone plan-doc; window-2's committed-plan-doc habit is itself new at this density, which is why the shape recurs.)

**Boundary notes.**
- *Drift boundary.* Plan slugs and CHANGELOG decision blocks describe their content accurately; the recorded rejections are honest. No misnaming-at-merge. Recurrence-only.
- *Relationship to Pattern 1/2.* MVU_PLAN.md (instance 1 here) is *also* the charter of the Pattern 1 strangler-fig and the Pattern 2 phase-train. The plan-doc is the upstream of the campaign shapes; this pattern reads the *plan→execute handoff*, Patterns 1/2 read the *campaign that results*. See convergence tail.

Provenance:
- `history-seg-docs-planning` (synthesis 01KTMNHGWSQK3S2QZ68D3WBCGC): 01KTMMRKSSJ8EN3RC6RB36RSM3 (pane plans #92/#93), 01KTMMPA6HBDB91KMZPTBPNHX3 (AUTO_APPROVAL #86), 01KTMMSRJ3DEDE2S4VWY5JHEND (V1_70 #114), 01KTMMWRE9YX58H2RXKYY58QVH (Lean-2.0 roadmap-reorg #179 authorizing decomposition+MVU), 01KTMMMQ1VY8ZERQ3NQAF89DN4 (v1.60 plan supersession churn), 01KTMMKHEYZ540PV6VVZR33K79 (positioning thesis — declined as instance, noted as thesis-propagation).
- Execute sides: `history-seg-refactor-mvu` 01KTMKVE85DEBMBWYCXY7YHP5E (MVU_PLAN Decision); `history-arc-03` 01KTMMMHWAHASBVT11Y96RBKSA (V1.5 host-first-per-plan) + 01KTMMSVHSFYKGPSD9R8NRFCZY (hands recovery to PANE_RECOVERY_PLAN); `history-arc-05` 01KTMMRS83NW2K9GASEKF5R1T1 (V1_5_PLAN motivation executed).
- Tier-3 handoff: `insight-trajectory` owns plan-fidelity (does execution track the plan); this pattern counts the handoff shape only.
- `insight-recurrence` window-2 framing = 01KTMTMAB6G8JJAFFQF29XVZGB; Pattern 4 = 01KTMTTZQK31H30M2RQ6J3FEY1.

<!-- Entry-ID: 01KTMTWQR1TJTNDNPZH3X6JDHP -->

---
Entry: Claude Code (caleb) 2026-06-09T00:01:33.081199+00:00
Role: critic
Type: Note
Title: Pattern 6 (window-2): Patch-corridor release cadence — window-1's v1.41.x corridor extends to .37, then a longer v1.50.x corridor (.0→.82), then v1.51.4 and v1.56.0; the same patch-under-one-minor shape at larger scale

Spec: critic

tags: #insight #recurrence #window-2

**Pattern statement (EVOLUTION of window-1 Pattern 5).** Window-1 Pattern 5 (= 01KR3D8RH5DNYC37WSGFVETXT3) named the v1.41.x patch cadence: after v1.41.0 shipped, "no more minor cuts land in the 22-day window — every subsequent merge is a v1.41.x patch," verified as a 24-patch ladder (v1.41.0 → ~v1.41.24). Window-2 extends *the very same corridor* further and then recurs the shape at a larger scale: the patch-under-one-minor cadence is the project's standing release shape, and minor cuts stay rare across the whole #38–#311 window.

**Instance enumeration — verified against the release ledger (`git log main --format='%s' | grep -oiE 'v1\.[0-9]+\.[0-9]+'`).**

1. **The v1.41.x corridor continues past window-1's cut-off.** Window-1's Pattern 5 caught the corridor at v1.41.24 (its 22-day boundary). The ledger shows the *same corridor* runs on into window-2 to **v1.41.37** — 13 more patches under the same minor, with no v1.42–v1.49 minor cut in between. The window-1 recurrence was not a 22-day artifact; it was the leading edge of a corridor that kept running.

2. **A longer v1.50.x corridor opens and recurs the shape at larger scale.** v1.50.0 cuts in window-2 (`history-arc-01` #51, 01KTMMMPZDCN2D25KSX9G1R6Y1), and the ledger shows the v1.50.x corridor running from **.0 to .82** — a *longer* patch ladder under one minor than the v1.41.x corridor window-1 catalogued. The patch-under-one-minor shape recurs, and the corridor length grows.

3. **v1.51.4 — the cadence is recorded as ritual.** PRs #143/#170 "add a `release-debug` profile and cut v1.51.4 — 'the first tagged release since v1.50.0,' now purely a CHANGELOG ritual" (`history-arc-01` 01KTMMY535PG8SJAN371WVMKPN). The recorded "purely a CHANGELOG ritual" line is the cadence naming itself.

4. **v1.56.0 — a minor cut tied to a campaign completion.** The gix migration closes with "released as 1.56.0" (`history-seg-gix-migration` 01KTMMTF0MQ96P7BVN7QQPVBNS). This is one of the rare minor-cut moments, and it is tied to a strangler-fig *drop* (Pattern 1) — the minor bump marks a capability-replacement landing, consistent with a "minor = capability-level change" working shape.

**Instance/scale claim: minor cuts stay rare; patch corridors recur and lengthen.** The ledger confirms the minor cuts in the relevant span are sparse — v1.41.0 and v1.50.0 are the `.0` cuts across the core window, with v1.51.0 and v1.56.0 the only other minors named, against ~120 patch versions. The recurrence is the **patch-corridor-under-one-minor**, instantiated at v1.41.x (now .37), v1.50.x (.82), and bracketed by the v1.51/v1.56 minors. The shape window-1 caught is the same; window-2 shows it is a *standing* cadence, not a window-bounded one, and that the corridors grow longer.

**The recurrence reading window-1 could not have: the corridor outlives the observation window.** Window-1's Pattern 5 was epistemically careful — it claimed "24 patches in the 22-day window" and flagged that whether the cadence would continue was tier-5 (forbidden, reserved for emergent-properties). Window-2's ledger now *retrospectively* shows the v1.41.x corridor did continue (to .37), and that a second corridor (v1.50.x) replicated the shape at greater length. This is not window-2 *predicting* — it is window-2 *counting what the larger ledger now contains*, which the smaller window could not see. The recurrence is confirmed at the cumulative grain: two long patch corridors, both under one minor, with rare capability-marking minor cuts between them. (Stating *why* the corridors lengthen — release-pressure? a SemVer-minor=capability policy stated nowhere but observed everywhere, as window-1's closure flagged — is tier-4, `insight-emergent-properties`'s.)

**Sub-shape: the minor cut correlates with a campaign/capability landing.** v1.50.0 opens the V1.5 pager-unification era; v1.56.0 marks the gix-migration drop; v1.41.0 (window-1) marked the first feature-complete cut. The recurring sub-observation: **minor cuts in this project bracket capability-level changes, patches carry everything else.** Three minor-cut instances across both windows fit this; the catalogue notes the sub-shape factually (the policy is observed, never stated — window-1's closure already flagged "the SemVer policy that exists nowhere as a stated plan but is observed everywhere").

**Boundary notes.**
- *Tier boundary.* "Was the patch-only cadence anticipated or did it emerge?" is tier-3/tier-4 (window-1's closure routed it to both `insight-trajectory` and `insight-emergent-properties`). This pattern counts the corridors; it does not assess intent or name the property.
- *Drift boundary.* Version bumps in slugs/CHANGELOG match the diffs; no misnaming-at-merge. Recurrence-only.

Provenance:
- Release ledger verified this session: `git log main --format='%s' | grep -oiE 'v1\.[0-9]+\.[0-9]+' | sort -u` → v1.41.x to .37, v1.50.x to .82, plus v1.51.4, v1.56.0; `.0` minor cuts in span = v1.41.0, v1.50.0 (+ v1.51.0, v1.56.0).
- `history-arc-01` (synthesis 01KTMNMAPV4N1KFHSPAVYN9VP4): 01KTMMMPZDCN2D25KSX9G1R6Y1 (#51 cuts v1.50.0), 01KTMMY535PG8SJAN371WVMKPN (#143/#170 v1.51.4 "first tagged release since v1.50.0 … purely a CHANGELOG ritual").
- `history-seg-gix-migration`: 01KTMMTF0MQ96P7BVN7QQPVBNS (#292 released as 1.56.0, tied to the strangler-fig drop).
- EVOLVES: `insight-recurrence` window-1 Pattern 5 = 01KR3D8RH5DNYC37WSGFVETXT3 (v1.41.0→.24 corridor, 22-day window); window-1 closure cross-tier flag for cadence = 01KR3DFHA7FRV3BXEH2Z8SFJQN.
- Sub-shape cross-ref: Pattern 1 strangler-fig drop = 01KTMTNYS83V5A7PY2MJVV60MF (v1.56.0 marks the gix drop).
- `insight-recurrence` window-2 framing = 01KTMTMAB6G8JJAFFQF29XVZGB; Pattern 5 = 01KTMTWQR1TJTNDNPZH3X6JDHP.

<!-- Entry-ID: 01KTMTYBDZ99R8XBA44E6NTZRQ -->

---
Entry: Claude Code (caleb) 2026-06-09T00:02:32.582238+00:00
Role: critic
Type: Closure
Title: Closure (window-2): #38–#311 recurrence continuation assembled — six shapes (2 new, 2 evolved, 2 native), ~60 instances; observations flagged for insight-trajectory and insight-emergent-properties

Spec: critic

tags: #insight #recurrence #window-2

The second-window continuation is assembled. Six recurrence shapes across the #38–#311 record (7 extended arcs + 6 new seg threads, ~123 moments). Per-shape instance counts:

- **Pattern 1: Strangler-fig migration** — 2 campaign-scale instances (MVU, gix), each full grow→parity→flip→drop, with a 2-register sub-shape (runtime-flag escape hatch for gix vs revertable-phases-behind-CI for MVU). **NEW shape; no window-1 equivalent.** Entry 01KTMTNYS83V5A7PY2MJVV60MF.
- **Pattern 2: Phase-numbered PR-train** — 3 campaign-trains (MVU phases −1..6/D/E, mod-extract pr1..12, gix 9-step), 1 folded near-miss (CI-caching burst, no explicit ordering), 2-1 risk-ordered/flat sub-split. **EVOLUTION of window-1 Pattern 6 (implicit→explicit-and-numbered).** Entry 01KTMTQSTE7QSH8WJ442ADX02F.
- **Pattern 3: Verbatim-relocation** — ~40–45 instances across module-decomposition + MVU extraction; identified by `+N/-N` diff-balance signature (machine-checkable), with a prep-then-verbatim exception sub-shape (state.rs #307). **Highest instance count in either window. Native to window-2.** Entry 01KTMTSAVQNENHP8ZFWBKTWXN6.
- **Pattern 4: Per-X-repetition-then-generalize** — 2 surfaces (AgentProfile #176, PagerStream #309–#311), each a full repeat→collapse cycle, payoff measured by next-instance cost (zot +18min; 3 sources migrated), partial-generalization-during-repetition sub-shape. **Successor to window-1 Pattern 6's accretion (accrete-then-collapse vs accrete-and-stop). Native to window-2.** Entry 01KTMTTZQK31H30M2RQ6J3FEY1.
- **Pattern 5: Plan-doc-then-execute** — 5 plan→execute handoffs across segment boundaries (MVU_PLAN, pane plans, V1_5_PLAN, AUTO_APPROVAL, V1_70), positioning-thesis declined as instance, recorded-supersession sub-property (plan thread holds rejected designs). **Native to window-2 (window-1's planning was too thin to recur).** Entry 01KTMTWQR1TJTNDNPZH3X6JDHP.
- **Pattern 6: Patch-corridor release cadence** — v1.41.x corridor extends to .37, v1.50.x corridor .0→.82, v1.51.4/v1.56.0 minors; ledger-verified; minor-cut-marks-capability sub-shape. **EVOLUTION of window-1 Pattern 5 (same shape, confirmed standing + corridors lengthen).** Entry 01KTMTYBDZ99R8XBA44E6NTZRQ.

**Six shapes, ~60 instances total** (2 strangler-figs + 3 phase-trains + ~45 verbatim relocations + 2 generalizations + 5 plan-executes + ~3 release corridors/minors), depending on how the folded waves are tallied — the verbatim-relocation count carries the imprecision and is stated as ~45.

**Window-1 → window-2 lineage of the catalogue itself.** The six window-2 shapes split 2 new / 2 evolved / 2 native:
- **New (no window-1 vantage):** Pattern 1 strangler-fig. Window-1 had no migration shape; the larger window's defining engineering move is replacing legacy implementations behind parity gates.
- **Evolved from a window-1 pattern:** Pattern 2 (from window-1 Pattern 6 implicit-machinery-chain — the chain went explicit and numbered) and Pattern 6 (from window-1 Pattern 5 v1.41.x cadence — same corridor, now shown standing and lengthening). Pattern 4 is a *successor* to window-1 Pattern 6's accretion without being a strict evolution — it is the collapse window-1's accretion never reached.
- **Native to window-2 (no window-1 precursor, born of the window's scale):** Pattern 3 verbatim-relocation (the decomposition campaign did not exist in 22 days) and Pattern 5 plan-doc-then-execute (window-1 had one plan doc, not a recurring plan→execute handoff).

Three of window-1's six patterns (bundle-as-shape, supersession-acknowledgement, BUGS.md SMALL→FIXED lift) are not re-catalogued here — they are per-PR shapes that surely still occur in window-2, but the continuation's job is the *new and evolved campaign-scale* shapes the larger window surfaces, not re-counting window-1's per-PR shapes across 273 PRs. (BUGS→ROADMAP promotion does recur — docs-planning 01KTMMYFCAPJDTD83XH66BE7P8 — and is the window-2 echo of the SMALL→FIXED lift, folded into Pattern 5's recorded-supersession sub-property rather than given its own entry.)

**Cross-thread observation for `insight-trajectory`'s author (tier-3).** The window-2 shapes differ in plan-correlation, sharply:
- **Pattern 5 (plan-doc-then-execute) is the trajectory thread's primary fuel.** It *counts the handoff*; whether each execution tracked its plan (all eight MVU phases? PANE_RECOVERY's tiers?) is the tier-3 question this thread explicitly declined. The five plan docs (MVU_PLAN, V1_5/V1_60/V1_70_PLAN, AUTO_APPROVAL, PANE_RECOVERY) are the stated-trajectory substrate.
- **Pattern 1 (strangler-fig) is highly plan-correlated** for MVU (MVU_PLAN records the full lifecycle) and gix (the "9-step … Removed in PR 9" plan is in-code). The trajectory thread can ask: did the flip/drop land where the plan said?
- **Pattern 6 (release cadence) is plan-uncorrelated** — no doc states "patches dominate; minors mark capability." Observed everywhere, stated nowhere (carried over from window-1's closure flag).
- **Patterns 2/3/4 are governed by recorded conventions** (REFACTOR_PLAN Phase-1 verbatim rule, the CLAUDE.md ~800-line ceiling at 01KTMMJVZMX3SJCBSK8YF896YP) rather than roadmap trajectory; the trajectory thread can treat those conventions as micro-plans.

**Cross-thread observation for `insight-emergent-properties`'s author (tier-4).** Which shapes carry tier-4 weight:
- **Pattern 1 (strangler-fig) carries heavy tier-4 weight** — two instances with two escape-hatch registers (flag vs CI-phases) is a rich substrate for naming a risk-management property. The temptation to name *why parity-behind-a-flag is this maintainer's habit* was this thread's most acute tier-2-discipline test; declined here.
- **Pattern 3 (verbatim-relocation, ~45×) carries tier-4 weight** — a micro-shape recurring 45× with a machine-checkable diff signature invites a property at the working-discipline grain (the ceiling-driven decomposition reflex).
- **Pattern 4 (repeat-then-generalize) carries tier-4 weight** — the measured-next-instance-payoff signature across two surfaces is an abstraction-discipline property candidate.
- **Pattern 6 (release cadence) carries heavy tier-4 weight** — two long corridors + capability-marking minors is the release-dynamics property window-1's closure already pre-flagged, now with more data.
- **Pattern 2/5** carry tier-3-shared weight (plan-and-convention-governed); emergent-properties should not collapse the plan-execute property into the trajectory reading.

**Per-shape counts, final.**
- Pattern 1: 2 instances (MVU, gix), 1 fractal sub-instance (gix diff sub-arc).
- Pattern 2: 3 campaign-trains, 1 folded near-miss.
- Pattern 3: ~40–45 verbatim relocations, 1 prep-exception.
- Pattern 4: 2 surfaces, full cycles.
- Pattern 5: 5 plan→execute handoffs, 1 declined (thesis).
- Pattern 6: 2 patch corridors (v1.41.x→.37, v1.50.x→.82) + capability-marking minors (v1.50.0, v1.56.0).

**The thread is left OPEN.** Cross-references from an extended `insight-trajectory` / `insight-emergent-properties` window-2 pass will cite this continuation's per-shape entry IDs.

Provenance:
- Window-2 entries: framing 01KTMTMAB6G8JJAFFQF29XVZGB; P1 01KTMTNYS83V5A7PY2MJVV60MF; P2 01KTMTQSTE7QSH8WJ442ADX02F; P3 01KTMTSAVQNENHP8ZFWBKTWXN6; P4 01KTMTTZQK31H30M2RQ6J3FEY1; P5 01KTMTWQR1TJTNDNPZH3X6JDHP; P6 01KTMTYBDZ99R8XBA44E6NTZRQ.
- Window-1 lineage: window-1 closure 01KR3DFHA7FRV3BXEH2Z8SFJQN; Pattern 5 01KR3D8RH5DNYC37WSGFVETXT3; Pattern 6 01KR3DC7E4B0JC1NN212PYVT56.
- Substrate spine: history-overview second-window framing 01KTMN7X7FV45E8E05DN1RV719 + segment map 01KTMN9MRB31A0C8ZWSXX5M5ZQ; history-synthesis opener 01KTMNAVSB3410Y59S3FDMZCHQ.
- Convention anchor cited above: history-seg-module-decomposition CLAUDE.md ceiling Decision 01KTMMJVZMX3SJCBSK8YF896YP; BUGS→ROADMAP echo 01KTMMYFCAPJDTD83XH66BE7P8.

<!-- Entry-ID: 01KTMV0568M86NZC6XZENTSB6D -->

---
Entry: Claude Code (caleb) 2026-06-09T00:03:25.686088+00:00
Role: critic
Type: Note
Title: Tail (window-2): convergence shapes — the MVU campaign and #176/#309–#311 sit at the intersection of multiple window-2 shapes; the convergences cluster on the architectural spine

Spec: critic

tags: #insight #recurrence #window-2 #tail

The continuation holds six shapes and ~60 instances. As in window-1, a small set of campaigns surface in two or three shapes at once, and the convergences are not noise — they cluster on the window's single architectural spine (route_key → MVU → decomposition → gix, per the segment map's cross-segment topology, 01KTMN9MRB31A0C8ZWSXX5M5ZQ).

**The MVU campaign is the most-converged-upon work in the window-2 record.** It carries Pattern 1 (strangler-fig: grow-alongside the old `App::run` loop → behavior-equivalence parity → flip → collapse to one `App::update`), Pattern 2 (phase-numbered train: phases −1..6 + D/E + last-mile), Pattern 3 (verbatim-relocation: the ~10 Phase-1&2 leaf/handler extractions are verbatim cuts), and Pattern 5 (plan-doc-then-execute: MVU_PLAN.md #196 is the charter the seg executes). **Four shapes converge on one campaign.** The convergence is structural, not coincidental: the strangler-fig *needs* the phase-train to stay revertable; the phase-train's early phases *are* verbatim relocations; and the whole thing executes a committed plan. One campaign, four shapes, because the migration's risk-management *is* the small-revertable-PR delivery *is* the verbatim-cut groundwork *is* the plan. Citations: P1/P2/P5 charter 01KTMKVE85DEBMBWYCXY7YHP5E; P3 extraction 01KTMKTBHT9G2VBZNJZFY5EJM4; P1 drop 01KTMM60KR8W18TWXPXDGT9J8Y.

**The gix campaign is the second triple-convergence.** It carries Pattern 1 (the textbook strangler-fig, facade→parity-spike→flip-behind-flag→drop), Pattern 2 (the "9-step" pre-numbered sequence, "Removed in PR 9"), and Pattern 6 (its drop is released as v1.56.0, a capability-marking minor cut). **Three shapes converge.** Like MVU, the convergence has a structural reason: the migration *is* the numbered sequence *is* what the minor-version bump marks. Citations: P1 facade/flip/drop 01KTMMHJ879C24FY2WYYT9F1SF / 01KTMMMBVCGADASG74RTHKWY97 / 01KTMMTF0MQ96P7BVN7QQPVBNS; P2 pre-numbering 01KTMMJB955RF16D842WFFEBYP; P6 v1.56.0 at the same drop moment 01KTMMTF0MQ96P7BVN7QQPVBNS.

**The module-decomposition campaign converges Patterns 2 and 3.** mod-extract pr1..12 is one Pattern-2 phase-train whose every element is a Pattern-3 verbatim relocation — the cleanest "train-of-verbatim-cuts" convergence, governed by one CLAUDE.md ceiling Decision (01KTMMJVZMX3SJCBSK8YF896YP) that is itself a recorded convention (Pattern 5's cousin). Citation: 01KTMMGZWER9304EZM82KTEZMQ.

**The #176 / #309–#311 generalizations converge Pattern 4 with the spine.** AgentProfile (#176) and PagerStream (#309–#311) are each a Pattern-4 repeat-then-generalize, and both land *on* the MVU-era substrate — PagerStream lives in `src/app/pager_stream.rs` (post-MVU app module) and AgentProfile's status resolution is noted as "downstream in history-seg-refactor-mvu (#234)." The two product-surface payoffs (arc-05, arc-07) reach their abstraction *because* the spine's runtime work made the seam available. Citations: P4 AgentProfile 01KTMMZWBVK4X3QJP7SJW3ZEG2; P4 PagerStream 01KTMN2XSH67BNFQPAMTSFD81X.

**Convergence-density observation (the class-level reading no per-shape entry owns).** Four campaigns (MVU, gix, decomposition, the #176/#309 generalizations) carry between 2 and 4 shapes each; the verbatim-relocation instances *outside* those campaigns are near-zero (the shape barely exists except inside the decomposition/MVU trains). This is the inverse of window-1's convergence profile: window-1's convergences were a *handful of individual PRs* (PR #29, #18, #31, #15) sitting at pattern intersections, with most PRs carrying one pattern (01KR3DJ9KJY1T9FFP8KDEPTBJ1). Window-2's convergences are *whole campaigns* — the shapes co-occur at the campaign grain, not the PR grain, because window-2's recurrences are themselves campaign-scale. The unit of convergence scaled up with the unit of recurrence.

Five of the six shapes share at least one campaign with another shape; only Pattern 6 (release cadence) holds partly-separate instances (the v1.41.x/v1.50.x corridors are not themselves campaigns) — exactly as window-1's Pattern 5 (the only window-1 pattern with fully separate instances) was the release-cadence pattern. The release cadence is the "spans the others without being one of them" shape in both windows. A reader entering at any one shape can follow the convergence cross-references and find the same four campaigns reappear: MVU in P1/P2/P3/P5, gix in P1/P2/P6, decomposition in P2/P3, the generalizations in P4 + the spine.

Provenance:
- P1 01KTMTNYS83V5A7PY2MJVV60MF; P2 01KTMTQSTE7QSH8WJ442ADX02F; P3 01KTMTSAVQNENHP8ZFWBKTWXN6; P4 01KTMTTZQK31H30M2RQ6J3FEY1; P5 01KTMTWQR1TJTNDNPZH3X6JDHP; P6 01KTMTYBDZ99R8XBA44E6NTZRQ; closure 01KTMV0568M86NZC6XZENTSB6D.
- Campaign moment anchors: MVU 01KTMKVE85DEBMBWYCXY7YHP5E / 01KTMKTBHT9G2VBZNJZFY5EJM4 / 01KTMM60KR8W18TWXPXDGT9J8Y; gix 01KTMMHJ879C24FY2WYYT9F1SF / 01KTMMMBVCGADASG74RTHKWY97 / 01KTMMJB955RF16D842WFFEBYP / 01KTMMTF0MQ96P7BVN7QQPVBNS; decomposition 01KTMMGZWER9304EZM82KTEZMQ / 01KTMMJVZMX3SJCBSK8YF896YP; generalizations 01KTMMZWBVK4X3QJP7SJW3ZEG2 / 01KTMN2XSH67BNFQPAMTSFD81X.
- Cross-segment topology: history-overview segment map 01KTMN9MRB31A0C8ZWSXX5M5ZQ (route_key → MVU → decomposition → gix spine).
- Window-1 convergence tail for contrast: 01KR3DJ9KJY1T9FFP8KDEPTBJ1 (PR-grain convergences; Pattern 5 the only fully-separate-instances pattern).

<!-- Entry-ID: 01KTMV1SBQ1PPH8A2WM4TSNX1N -->

---
Entry: Claude Code (caleb) 2026-06-09T00:04:14.394983+00:00
Role: critic
Type: Note
Title: Tail (window-2): the synthesis arc layer carried what this continuation needed — at 273-PR volume the per-moment log alone could not surface campaign-scale recurrence; the readable-arc layer summed it

Spec: critic

tags: #insight #recurrence #window-2 #tail

Window-1's closing tail (01KR3DM9DBRV6MBA4D516KRR12) named that the *arc story-tails* — the cumulative-grain entries — were the upstream that made window-1's recurrence patterns legible, because "recurrence requires the cumulative grain to *see* the count." Window-2 confirms that asymmetry and sharpens it: at ~7.4× the PR volume, the per-moment log alone could not surface campaign-scale recurrence at all, and a *new* layer — the `history-synthesis` readable-arc entries — did the summing the story-tails did in window-1.

**The synthesis arcs were this continuation's load-bearing upstream.** Each window-2 pattern traces its shape to a synthesis arc that had already *named the shape as a throughline* before this thread counted it:
- Pattern 1's "strangler-fig" word is verbatim from the gix synthesis arc ("a planned 9-step strangler-fig … the recorded signature of a completed strangler-fig," 01KTMNJD6GJM90D0WTTS0X28XR) and the MVU synthesis arc's "strangler-fig march" title (01KTMNFSQP1XJ1TSXS545PR01E).
- Pattern 3's "verbatim-move signature … diff balances are stat-verified" is the module-decomposition synthesis arc's own phrasing (01KTMNH17RVXAWHF9SJ2Z7MCP9).
- Pattern 4's "hardcoded peers → AgentProfile registry" is the arc-07 synthesis title (01KTMNJRT9FD74NGXQXHH78A3B); the "+18 minutes / one impl + one registry line" payoff is its recorded moment.
- Pattern 5's plan→execute handoffs are the docs-planning synthesis arc's "one thesis propagated across positioning, plans, and triage" spine (01KTMNHGWSQK3S2QZ68D3WBCGC).

The synthesis layer did the *intra-segment* arc work (one subsystem's shape); this thread did the *cross-segment* recurrence work (the same shape across two or three segments). The strangler-fig is the clearest case: the gix synthesis arc named gix-as-strangler-fig and the MVU synthesis arc named MVU-as-strangler-march, but *neither could see that the two are the same recurring shape* — each synthesis arc owns one segment. Naming "strangler-fig recurs twice across two segments" is the cross-segment count only this thread's grain reaches. The synthesis arcs summed within a subsystem; this thread sums across subsystems.

**The asymmetry window-1 named holds, with a third layer added.** Window-1 had two layers feeding recurrence: per-PR drift-findings (per-PR grain) and arc story-tails (cumulative-per-arc grain). Window-2 has three: per-moment entries (per-PR/per-wave grain), synthesis readable-arcs (cumulative-per-segment grain), and this insight continuation (cross-segment grain). The added middle layer was *necessary* at this volume — window-1's eight arcs each fit in one analyst's working memory; window-2's thirteen threads and ~123 moments did not, so the synthesis layer's "one readable arc per segment, every claim carrying an inline `[entry_id]`" is what let a cross-segment reader hold all thirteen shapes at once without re-deriving them from ~273 raw PRs. The legibility infrastructure scaled with the substrate.

**What made the recurrence machine-checkable this window, beyond legibility.** Two window-2 shapes carry *executable* signatures the per-moment layer recorded, which window-1's shapes lacked: Pattern 3's verbatim relocations are identifiable by `+N/-N` diff balance (and the decomposition's own `mod_rs_stays_decomposed` / `no_subprocess_git_in_production` guard tests *enforce* the campaign invariants in CI), and Pattern 1's gix drop is guarded by a test that fails if any production source spawns `git`. The recurrence is not only legible in prose — parts of it are checked by the codebase itself. That is a sharper substrate than window-1's slug-and-CHANGELOG reading, and it lowered this thread's inference burden: the verbatim count and the strangler-fig completion are diff- and test-verified facts, not analyst readings.

**This continuation's own contribution atop the synthesis layer**, separate from per-segment assembly: the cross-segment shape-count (strangler-fig ×2, phase-train ×3, repeat-then-generalize ×2 surfaces), the window-1→window-2 lineage classification (2 new / 2 evolved / 2 native, in the closure), the escape-hatch-register sub-shape that needed both Pattern-1 instances side by side to see (flag vs CI-phases), the campaign-grain convergence-density observation (the prior tail), and the ledger-verified confirmation that window-1's v1.41.x corridor was the leading edge of a standing cadence. These are not work the synthesis arcs did; they are the cross-segment reading this thread does *atop* them. Roughly two-thirds synthesis-arc-assembly, one-third cross-segment refinement — the same ratio window-1's tail reported against its story-tail upstream, holding at the larger scale with the extra middle layer absorbing the volume.

The thread is left OPEN.

Provenance:
- Synthesis arc upstreams: gix 01KTMNJD6GJM90D0WTTS0X28XR; MVU 01KTMNFSQP1XJ1TSXS545PR01E; module-decomposition 01KTMNH17RVXAWHF9SJ2Z7MCP9; arc-07 01KTMNJRT9FD74NGXQXHH78A3B; arc-05 01KTMNG0CYN2NW1Y3RBR71J5FP; docs-planning 01KTMNHGWSQK3S2QZ68D3WBCGC; synthesis opener 01KTMNAVSB3410Y59S3FDMZCHQ.
- Machine-checkable signatures: verbatim guard test + ceiling Decision 01KTMMJVZMX3SJCBSK8YF896YP; gix drop guard test 01KTMMTF0MQ96P7BVN7QQPVBNS.
- Window-1 infrastructure/legibility tail this echoes: 01KR3DM9DBRV6MBA4D516KRR12 (story-tails as recurrence upstream; the cumulative-grain asymmetry).
- This continuation's entries: framing 01KTMTMAB6G8JJAFFQF29XVZGB; closure 01KTMV0568M86NZC6XZENTSB6D; convergence tail 01KTMV1SBQ1PPH8A2WM4TSNX1N.

<!-- Entry-ID: 01KTMV38HW8N06YTPG0VQS5NYH -->
