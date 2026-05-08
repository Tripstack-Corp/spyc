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
