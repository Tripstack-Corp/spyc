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
