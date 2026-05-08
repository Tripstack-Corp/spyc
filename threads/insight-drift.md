# insight-drift — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: insight-drift
Created: 2026-05-08T08:28:30.467228+00:00

---
Entry: Claude Code (caleb) 2026-05-08T08:28:30.467228+00:00
Role: scribe
Type: Note
Title: Framing: insight-drift opens — the analyst register declared, six patterns named, methodology for tier-1 observation

Spec: scribe

tags: #insight #drift

`insight-drift` is the first of four insight-layer threads that read across the eight baseline arcs. Its job is **tier-1 work in the insight aggressiveness taxonomy: pure observation, always allowed**. Name patterns. Count their instances. Cite specific arc entries where each instance was observed. No analyst framing of *why* the pattern occurs (that's `insight-emergent-properties`'s tier-4 territory). No motive attribution (forbidden absolutely at every tier). The job is to count, name, and cite — and to do so in a register the arcs themselves did not own.

**Why this thread is needed.** The eight arc threads each carry "Drift findings flagged for the insight layer" sections in their per-PR and tail entries. Across the eight arcs the drift findings are scattered — the same pattern surfaces in arc 04, arc 05, arc 07 with different specific PRs each time, and the arc author at the time of writing flagged the instance without claiming the pattern. This thread *assembles* those scattered observations into one coherent reading.

**Voice contract — the analyst register, not the maintainer's voice.**

The insight threads use a more confident analytic register than the arcs. *"The pattern is X"* is permitted when X is observable across multiple instances. Synthesis across arcs without per-PR present-tense narration is permitted — this thread describes patterns, not retells events. Headers are permitted where the material is taxonomy-shaped (one entry per pattern). First-person plural — *we* the cumulative reading, *the catalogue* — is permitted sparingly when used meaningfully.

What stays banned, unmodified from the arc voice contract: motive attribution to the maintainer (no *Derek wanted X / decided Y / thought Z*); invented technical details (provenance still required at every entry, including the arc-entry ULIDs cited); clock-padding language (*"twenty-five minutes later"* only when sequence-load-bearing, never as drama); fabricated patterns (one instance gets named *one instance*, not promoted to *the pattern is recurring*).

The honesty contracts hold without modification. The analyst register *adds* confidence to the description; it does not relax the verification.

**The six patterns named, with instance-count claims to verify per entry.**

- **A. Commit-subject vs. diff-scope understatement** — the commit subject describes a narrower scope than the diff actually covers. *Brief named five candidate instances; this thread verifies five.*
- **B. Bundle-as-shape** — a single PR carries multiple thematically-distinct concerns under one slug. *Brief named six candidate instances across five arcs; this thread verifies six.*
- **C. Bucket-vs-content asymmetry** — the CHANGELOG bucket category does not match the diff shape. *Brief named three candidate instances; this thread verifies three, with an arc-08-internal inverse-asymmetry pair worth noting.*
- **D. Documented-vs-wired drift at moment of merge** — capability documented in CHANGELOG/help/etc. but not actually wired in code. *Brief named one candidate instance; this thread verifies one, with observer-side naming at seed level.*
- **E. Within-PR self-correction** — a PR's own diff retracts or reframes its own framing. *Brief flagged a counting-convention question; this thread reads one true within-PR instance plus one between-PR reframing in the same 49-minute pair, and treats them under one entry with the convention named.*
- **F. Span-phrasing inconsistency** — the spine and most arcs use *"22-day window"* language; the actual PR-merge window is shorter (~7-8 calendar days). *Brief named one candidate instance; this thread verifies one, structurally upstream of all eight arcs because it inherits from the spine's framing entry.*

**Methodology.**

For each pattern, the entry below catalogues the candidate instances pre-collected by the brief; verifies each against the arc-entry ULID it cites; names the instance count revealed by verification; and flags any boundary-of-pattern question the verification reveals. Instance citations name the arc-entry ULID, not just the topic name. Where verification reveals an instance the brief named doesn't actually exist, the entry says so and revises the count. Where verification reveals an additional instance not pre-collected, the entry adds it with a note.

**Cadence — one entry per pattern, no compression.**

The brief permitted compression (combining patterns, dropping a pattern with weak structure) or expansion (sub-dividing a pattern into sub-shapes). Verification revealed each of the six patterns has substantive instance count and clean boundaries; no compression warranted. Pattern E's counting-convention question is flagged at the entry but does not rise to sub-division — the within-PR-vs-between-PR distinction is a single observation about how the PR #30 → PR #31 49-minute pair is shaped, not two patterns competing for the same material.

**What `insight-drift` is NOT for.**

NOT recurrence-of-events across arcs (that is `insight-recurrence`'s tier-2 work — same-shape happening across multiple PRs is recurrence; that-particular-PR-mislabeled-its-own-diff at moment of merge is drift). NOT trajectory-against-stated-plan (that is `insight-trajectory`'s tier-3 work). NOT emergent-property naming (that is `insight-emergent-properties`'s tier-4 work). NOT forward-prediction (that is tier-5, in `insight-emergent-properties` only, with each prediction tied to a recurrence it extrapolates from). NOT motive attribution to the maintainer (forbidden absolutely at every tier — *especially* important for drift, because drift is observable from outside the maintainer's mind, and any temptation to explain *why* a commit subject understates a diff would cross into motive).

A drift instance may *also* be a recurrence instance (Pattern B in particular — bundling-at-moment-of-merge is drift; bundling-as-recurrent-project-shape is recurrence; same observable, different framing question). The cross-reference observation for `insight-recurrence`'s author is in this thread's closure entry.

Provenance:
- `history-overview` framing entry = 01KR0TRFWT9W6WMFHC49YSW0BG — the spine entry that introduces *"22 days of merged work"* phrasing; structurally upstream of Pattern F.
- `history-overview` segmentation entry = 01KR0TWHTC1MPK4KJ08Y9SPE6P — carries the pre-collected drift findings flagged for the insight layer; PRs #5, #15, #20, #36, #14, #30 → #31 all named at that entry's drift-findings block.
- The eight arc threads, all OPEN: history-arc-01-foundation-hygiene through history-arc-08-recoverability-and-deps. Per-PR drift-findings sections in their entries are the source of the candidate instance lists this thread verifies.
- `onboarding-risk-register` entry 0 = 01KR0P9JC8Z3DF6FQ1GJPF3VKA — observer-side naming of Pattern D's bug class with PR #14's release tag verbatim (*"Bitten on `:undo` (v1.41.1)"*).
- `watercooler_health` against the spyc code_path reports Healthy at session start (server v0.4.6.dev0; threads-repo URL `git@github.com:calebjacksonhoward/spyc.git`; branch parity clean).
- Federated search to watercooler-cloud's `onboarding-spyc-rust-bitbucket` namespace not attempted at this entry; the brief permits writing the insight thread from spyc-side arc entries alone, and the catalogue is well-sourced from the eight arcs' drift findings without external sourcing.

<!-- Entry-ID: 01KR3B7KW5QNRWHG6YTV9QSF07 -->
