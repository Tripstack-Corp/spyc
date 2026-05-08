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

---
Entry: Claude Code (caleb) 2026-05-08T08:29:52.046665+00:00
Role: scribe
Type: Note
Title: Pattern A: Commit-subject vs. diff-scope understatement — five instances across five arcs

Spec: scribe

tags: #insight #drift

**Pattern statement.** The commit subject describes a narrower scope than the diff actually covers — either by listing fewer concerns than the diff carries, by using a prefix (`chore/`, `investigate/`, `fix:`) that mismatches the diff's user-visible weight, or by carrying a version tag that does not match the version the diff actually cuts. The drift is at the subject-line level; in every verified instance the CHANGELOG, BUGS.md, or commit body carries the more honest framing. A reader scanning subjects only points toward a different shape than the diff supports.

**Instance enumeration with arc-entry citations.**

1. **PR #2 (arc 01) — CI subject hides a 139-line src/* sweep.** Commit subject reads *"ci: align with make check, add target cache + pre-commit hook"* (commit d9b9360, 2026-04-30). The diff bundles 139 lines of src/* lint-fix code under `### CI / Tooling` — accurately captured in the CHANGELOG, less so in the commit subject. *Cite: arc-01 PR #2 entry = 01KR0W81XE4K3G7BBSP42GE1HH (drift-findings block: "the commit subject reads as pure CI work … but the diff bundles 139 lines of src/* lint-fix code").*

2. **PR #4 (arc 01) — `:!cmd`/`;cmd` subject understates `Pane::spawn`-broad change.** Commit subject scopes the change to *":!cmd / ;cmd via $SHELL -i (v1.37.2)"* (commit 1f41b4b, 2026-04-30). The diff also touches `src/pane/mod.rs::Pane::spawn`, which is the path used for pane child processes broadly — not only the `:!cmd` / `;cmd` overlay routes. The CHANGELOG names `Pane::spawn` directly; the drift is subject-line-level only. *Cite: arc-01 PR #4 entry = 01KR0WBKNMQF231X2T8KTGD9KS (drift-findings block).*

3. **PR #5 (arc 02) — `investigate/` understates a 444-line diff plus a code fix; `(v1.37.2)` mislabels the version the diff cuts.** Commit subject reads *"lazygit investigation + cursor-block fix (v1.37.2)"* (commit 0691666, 2026-04-30). Two sub-instances of the same pattern in one PR: (i) the `investigate/` prefix understates a 444-line diff that includes a 7-line cursor-block code fix in `src/pane/widget.rs` alongside 399 lines of investigation notes; (ii) the `(v1.37.2)` tag is wrong against the diff — `Cargo.toml` moves from `1.37.2` to `1.37.3`, and a new `## [1.37.3] - 2026-04-30` block lands in CHANGELOG.md with the `[1.37.2]` block PR #4 cut sitting unmodified above it. The release the diff actually ships is v1.37.3. *Cite: arc-02 investigation entry = 01KR0YXXZRQR24CSNAK4Q7808T (drift-findings block; "The `(v1.37.2)` commit-subject tag is the genuine drift; the diff is correct" and "The PR title prefix `investigate/` understates the diff content").* The arc-01 → arc-02 hand-off proved out: arc 01 flagged the `(v1.37.2)` overlap without prejudgement (01KR0WBKNMQF231X2T8KTGD9KS); arc 02 resolved against the diff.

4. **PR #18 (arc 07) — `chore/` prefix carries a panic-fix and a direct-mode-fallback fix.** Commit subject reads *"chore: AGENTS.md rename + MCP hygiene fixes (v1.41.5)"* (commit bad8bfc, 2026-05-05). The bundle includes `ensure_mcp_json` shape-safety (panic-fix on a malformed `.mcp.json`) and `Pane::spawn`'s `context_path` parameter widening (direct-mode-fallback fix that closes a Claude-Code MCP discovery bug for non-`start_dir` panes) — both of which the CHANGELOG places under `### Fixed`. The chore-vs-fix prefix-vs-section split is the drift. *Cite: arc-07 PR #18 entry = 01KR2J1R3HXNZPAHE9118BGBQJ (drift-findings block: "commit subject prefix is `chore`; the bundle includes one panic-fix … and one direct-mode-fallback fix … that would land naturally under `fix:`").*

5. **PR #31 (arc 08) — commit subject names two crates; diff and commit body name three.** Commit subject reads *"chore: upgrade vt100 0.15 → 0.16, ratatui 0.29 → 0.30 (v1.41.18)"* (commit 105db8d, 2026-05-06). The diff and commit body name three: vt100 + ratatui + ansi-to-tui 7 → 8 (forced by the transitive `unicode-width` pin). The third crate is the dep-graph cost of the second — not a separable decision — but the commit subject's two-name framing understates the change's footprint. A reader scanning the commit log without arc threads sees a two-crate `chore`; the diff is a three-crate trio with one hidden coupling. *Cite: arc-08 PR #31 entry = 01KR397RTYNS34SAGM46YJJRBY (drift-findings block: "commit subject names two crates … the diff and commit body name three").*

**Instance count: five.** All five candidate instances pre-collected by the brief verified against arc-entry citations. No revisions to the count.

**Notes on counting convention and pattern boundary.**

- *Sub-instance counting in PR #5.* The arc-02 investigation entry surfaces two distinct sub-instances of Pattern A in one PR: the `investigate/` prefix understatement and the `(v1.37.2)` version-tag mismatch. The catalogue counts these together as one instance (PR #5) per the brief's framing, since both are subject-line drifts at the same PR. Counting them separately would yield six total Pattern-A instances, but the structural shape is *one PR with two surface-level subject drifts*, and counting it twice would over-state the pattern's frequency.

- *Reverse-polarity instance noted, not tallied.* PR #31's commit body claims *"six call sites in pane/mod.rs"* but only four hunks are visible in the source diff (per the arc-08 PR #31 entry's narration; the discrepancy "is verifiable: two of the four hunks are in `apply_scroll`-style methods that could plausibly be counted as multiple call sites depending on counting convention"). This is the inverse polarity — commit body *over*-claims relative to diff — and is not a Pattern-A instance per the pattern's stated direction (subject *under*-states diff). Captured here for completeness; not added to the count.

- *Scope of Pattern A vs. scope of Pattern B.* Pattern A is "the subject's *description* misses the diff's *scope*"; Pattern B is "the diff *contains* multiple thematically-distinct concerns *under one slug*." PR #2, PR #18, and PR #31 surface in both A and B catalogues — the drift instance there is dual: the diff bundles (B), and the subject names fewer than what's bundled (A). PR #4 is A-only (the diff doesn't bundle distinct concerns; it touches a broader code path than the subject scopes to). PR #5 is A-only (the diff bundles investigation notes plus a code fix, but the *subject* drift is the prefix and the version tag — not the bundle composition). The catalogue does not double-count: each instance appears in the pattern that best names what is being misdescribed, with cross-mention noted.

**Sub-shapes within Pattern A (not warranting separate patterns).**

- *Prefix-vs-content drift* (PRs #5, #18): the slug prefix tells the reader *what kind of change* this is (`investigate/`, `chore/`); the diff says otherwise.
- *Scope-vs-touch drift* (PR #4): the slug names *which feature* is changing; the diff touches more code paths than the named feature.
- *Bundle-headline drift* (PRs #2, #31): the slug names *some of* the concerns or units (CI work; two crate names); the diff carries more.
- *Version-tag drift* (PR #5 sub-instance): the parenthetical `(v1.37.X)` names the wrong version; the diff actually cuts a different one.

These are sub-shapes, not separate patterns. The unifying observation: across five PRs in five arcs (arc 01 twice, arc 02, arc 07, arc 08), the commit subject under-describes the diff in some structural way, and in every case the CHANGELOG / commit body / version files carry the corrected description.

Provenance:
- arc-01 PR #2 entry = 01KR0W81XE4K3G7BBSP42GE1HH (PR #2 drift-findings block).
- arc-01 PR #4 entry = 01KR0WBKNMQF231X2T8KTGD9KS (PR #4 drift-findings block; v1.37.2-overlap question forwarded to arc 02).
- arc-02 investigation entry = 01KR0YXXZRQR24CSNAK4Q7808T (PR #5 sub-instances; v1.37.2 vs. v1.37.3 resolution against the diff).
- arc-07 PR #18 entry = 01KR2J1R3HXNZPAHE9118BGBQJ (PR #18 drift-findings block; chore-vs-fix prefix-vs-section split).
- arc-08 PR #31 entry = 01KR397RTYNS34SAGM46YJJRBY (PR #31 drift-findings block; trio-vs-pair commit-subject; reverse-polarity "six call sites" inverse-claim).
- `history-overview` segmentation entry = 01KR0TWHTC1MPK4KJ08Y9SPE6P (original drift-findings block flagging PR #5 and PR #20).
- `insight-drift` framing entry = 01KR3B7KW5QNRWHG6YTV9QSF07 (analyst register and methodology declared).

<!-- Entry-ID: 01KR3BA3CZWA6TZCDKNJZDPAGH -->
