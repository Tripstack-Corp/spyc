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

---
Entry: Claude Code (caleb) 2026-05-08T08:31:18.928920+00:00
Role: scribe
Type: Note
Title: Pattern B: Bundle-as-shape — six instances across five arcs, the densest drift pattern in the network

Spec: scribe

tags: #insight #drift

**Pattern statement.** A single PR carries multiple thematically-distinct concerns under one slug. The concerns do not share a root cause, do not necessarily share files, and would be individually shippable. The PR ships them as one because they were noticed-while-shipping, because they share infrastructure introduced for one of the concerns, or because the maintainer's framing at moment-of-merge collapsed them into a single unit. The drift is at the slug level: one slug names one concern (or one prefix-marked kind of change), and the diff carries more.

**Instance enumeration with arc-entry citations.**

1. **PR #15 (arc 04) — basename-collision (87L) + ^C-route guard (5L).** Commit subject reads *"fix: ^C → pane child + git markers don't leak across same-name files (v1.41.2)"* (commit 5999261, 2026-05-04). Two halves: a 75-line refactor extracting `parse_porcelain_statuses` from `git_file_statuses` plus 5 new unit tests in `src/sysinfo.rs` (the basename-collision fix); a 5-line guard tightening on the `^C` footgun in `src/app/mod.rs::App::handle_key` (the pane-control fix). The two halves do not share a root cause and do not share files. The commit subject's left-to-right order is the *inverse* of the diff weight — title leads with the smaller half. *Cite: arc-04 PR #15 entry = 01KR130775Q4PKYEN6FE1743DJ (drift-findings block: "The two halves do not share a root cause. They share a PR.").*

2. **PR #20 (arc 05) — alt-screen scroll hint + `[pane] default_command` + `gd`-vs-HEAD.** Commit subject reads *"feat: alt-screen scroll hint + [pane] default_command + gd-vs-HEAD (v1.41.7)"* (commit ee07307, 2026-05-05). Three concerns named explicitly in the subject; only one is the user-visible headline (the alt-screen hint). The three halves do not share a code path. The bundling itself is the drift, not any individual half. The arc-05 PR #20 entry confirms the disposition against the diff: *"Each half is a clean, individually shippable change; the bundling itself is the drift."* *Cite: arc-05 PR #20 entry = 01KR2A6TT516XA5FEGVBXYPWD7 (drift-findings block); also flagged at history-overview segmentation entry = 01KR0TWHTC1MPK4KJ08Y9SPE6P.*

3. **PR #10 (arc 06) — quickselect feature + `gf`/`gF` scroll-mode `### Fixed` half.** Commit subject reads *"quick select: ^a u labeled-overlay picker for pane output (v1.40.0)"* (commit 9043547, 2026-05-02). The PR ships under a `feat/` slug, but the CHANGELOG carries both an `### Added` block (the quickselect feature) and a `### Fixed` block (`gf`/`gF` honoring scroll mode via the new `pickable_text` helper). The fix shares infrastructure with the new feature — `pickable_text` is the contract introduced for quickselect, with `gf`/`gF` becoming the second consumer — but the `### Fixed` half is a separately-noticeable bug whose existence is independent of quickselect. *Cite: arc-06 PR #10 entry = 01KR2GH1D9QCGDPZEMWW09R898 (drift-findings block: "The `feat/quickselect` PR ships a `### Fixed` half … bundled under the feature slug").*

4. **PR #25 (arc 06) — input-dispatch hardening + `--key-trace` diagnostic infrastructure.** Commit subject reads *"fix: input dispatch hardening + --key-trace diagnostic switch (v1.41.12)"* (commit bfc4a18, 2026-05-06). Two concerns named at the subject. The hardening half itself contains two distinct guards (post-chord bounce-suppression on `Action::PaneFocusDown`/`PaneFocusUp`; stranded-paste flash on `Event::Paste` fall-through) under one bug-report symptom. The diagnostic half is a 64-line new module (`src/key_trace.rs`) plus a CLI argument plus a startup banner — infrastructure shipped *with* the defensive fix specifically so the next user-report comes with a reproduction log. The infrastructure has no immediate consumer beyond the bug PR #25 already addresses. *Cite: arc-06 PR #25 entry = 01KR2GMSNX29CWFN154QBK6TJ3 (drift-findings block; bundle-pattern recurrence within arc 06 noted at this entry against PR #10 and arc-04 PR #15).*

5. **PR #18 (arc 07) — AGENTS.md rename + MCP hygiene fixes + a deferred-design BUGS.md note.** Commit subject reads *"chore: AGENTS.md rename + MCP hygiene fixes (v1.41.5)"* (commit bad8bfc, 2026-05-05). The slug names two halves; the diff carries effectively three (the rename, the MCP hygiene bundle of `ensure_mcp_json` panic-fix + `Pane::spawn` `context_path` parameter widening + `term_title::wrap_*_tmux` test lock, and a 13-line BUGS.md `### SMALL ###` entry that pre-names a design issue with three solution options). The BUGS.md note is the third half whose load-bearing role becomes legible only at PR #37 two days later — the note frames the design issue, ranks the options (*"Option (b) feels most spyc-shaped"*), and PR #37 implements exactly option (b) and removes the note. *Cite: arc-07 PR #18 entry = 01KR2J1R3HXNZPAHE9118BGBQJ (drift-findings block: "The PR title weights the rename and the MCP hygiene as equal halves … the diff weights the MCP hygiene at 80+% of the substantive changes"; the BUGS.md note as a third bundled half).*

6. **PR #14 (arc 08) — routing fix (2L) + `.gitignore` (2L) + `CLAUDE.md` (1L).** Commit subject reads *"fix: route :undo / :graveyard to App's handler (v1.41.1)"* (commit c7419c1, 2026-05-03). The load-bearing change is two lines added to `AppState::dispatch_command`'s punt list; the same diff also adds 2 lines to `.gitignore` and 1 line to `CLAUDE.md` (still that filename pre-PR-#18's rename). The arc-08 PR #14 entry names the bundling as *"tangential; not load-bearing for the supersession reading,"* which is honest — the bundling is small-scale, the load-bearing change is the routing fix, and the `.gitignore`/`CLAUDE.md` additions ride along. The drift instance is real but lower-amplitude than the others. *Cite: arc-08 PR #14 entry = 01KR38XPJ07ZFQHH1TG6X461WN (drift-findings block: "PR #14 also adds two lines to `.gitignore` … The diff's content for `.gitignore` is not the load-bearing part of the PR but is bundled in").*

**Instance count: six.** All six candidate instances pre-collected by the brief verified against arc-entry citations. No revisions to the count.

**Notes on counting convention and pattern boundary.**

- *Density across arcs.* Pattern B has six instances spread across five arcs (arc 04, arc 05, arc 06 ×2, arc 07, arc 08). The arc with two instances is arc 06 — the only arc to surface Pattern B twice in its own four-PR span. Whether arc 06 has two instances *because* it has only four PRs (a smaller denominator inflates the visible rate) or because the arc's subject matter (input + overlays) lends itself to bundle-as-shape (the diff that introduces a picker contract naturally invites secondary consumers under the same slug) is a question the analyst register should decline. Captured factually.

- *Smallest vs. largest instance.* PR #20 is the densest (three concerns named in the subject); PR #14 is the smallest (the load-bearing change is two lines; the bundled additions are five lines total). Pattern B holds across an order-of-magnitude variation in instance amplitude. The pattern's identity is structural (subject names one concern, diff carries multiple) rather than diff-size-based.

- *Cross-mention with Pattern A.* PRs #18 and #31 surface in Pattern A's catalogue too. The drift in those PRs is dual: the diff bundles concerns the slug doesn't fully name (B), *and* the slug's prefix understates the user-visible weight of the bundled concerns (A). The catalogue does not double-count. PR #15 and PR #20 are B-only because the subjects accurately enumerate the concerns the diff carries — the drift is the *bundle's existence*, not a description-level mismatch on what's bundled.

- *Cross-reference for `insight-recurrence`.* Bundle-as-shape is observable as drift (this thread, where the unit is the *misnaming-at-moment-of-merge*) and as recurrence (where the unit would be *bundling happens N times across the project*). Both threads should treat it. The closure entry to this thread carries the canonical placement recommendation for the next session's author.

**Sub-shapes within Pattern B (not warranting separate patterns).**

- *Bundle-of-noticed-while-shipping* (PRs #15, #14): two unrelated fixes ride one PR because both were spotted in proximity. No shared infrastructure.
- *Bundle-of-shared-infrastructure* (PRs #10, #25): a feature half and a fix or diagnostic half ride one PR because both consume a contract introduced for the feature.
- *Bundle-of-equal-weight-concerns* (PR #20): three concerns each independently shippable and roughly comparable in size, all under one `feat/` slug.
- *Bundle-of-rename-plus-groundwork-plus-deferred-design-note* (PR #18): a rename half + a hygiene half + a BUGS.md design note that brackets future work; the PR is doing three different kinds of structural move at once.

These are sub-shapes, not separate patterns. The unifying observation: across six PRs in five arcs, a single slug carries multiple concerns the diff treats as separable. The bundling pattern is the densest of the six drift patterns in the network.

Provenance:
- arc-04 PR #15 entry = 01KR130775Q4PKYEN6FE1743DJ.
- arc-05 PR #20 entry = 01KR2A6TT516XA5FEGVBXYPWD7.
- arc-06 PR #10 entry = 01KR2GH1D9QCGDPZEMWW09R898.
- arc-06 PR #25 entry = 01KR2GMSNX29CWFN154QBK6TJ3.
- arc-07 PR #18 entry = 01KR2J1R3HXNZPAHE9118BGBQJ.
- arc-08 PR #14 entry = 01KR38XPJ07ZFQHH1TG6X461WN.
- `history-overview` segmentation entry = 01KR0TWHTC1MPK4KJ08Y9SPE6P (PR #20 three-concern bundle drift flag at original source).
- `insight-drift` framing entry = 01KR3B7KW5QNRWHG6YTV9QSF07.
- `insight-drift` Pattern A entry = 01KR3BA3CZWA6TZCDKNJZDPAGH (cross-mention of PRs #18 and #31).

<!-- Entry-ID: 01KR3BCQXGGB20V8C6Y6Z1Y944 -->

---
Entry: Claude Code (caleb) 2026-05-08T08:32:16.439246+00:00
Role: scribe
Type: Note
Title: Pattern C: Bucket-vs-content asymmetry — three instances, with an arc-08-internal inverse-asymmetry pair

Spec: scribe

tags: #insight #drift

**Pattern statement.** The CHANGELOG bucket category (`### Added`, `### Changed`, `### Fixed`, `### Removed`, `### Security`, `### CI / Tooling`) does not match the diff shape. A diff that adds public API surface lands under `### Fixed`. A dep upgrade that resolves a panic lands under `### Changed` rather than `### Fixed`. A change whose commit subject reads `fix:` lands under `### Changed` while the BUGS.md entry it closes lifts to FIXED. The drift is at the classification-surface level: the bucket choice and the content the bucket holds carry different framings of what the change is.

**Instance enumeration with arc-entry citations.**

1. **PR #28 (arc 08) — `### Fixed` for additive defensive work.** Commit subject reads *"fix: cap directory listings at 50k entries to avoid hangs (v1.41.15)"* (commit 306b43f, 2026-05-06). The CHANGELOG places the change under `### Fixed`. By the diff-shape measure this is `### Added` work: a constant (`MAX_ENTRIES: usize = 50_000`), a public field (`Listing::truncated`), an extracted public function (`pub fn read_capped(dir, cap)`), and three unit tests. The bucket choice reads as honoring the user-reported origin (BUGS SMALL #4: a fix to a regression-from-good-experience) rather than the diff shape (additive). *Cite: arc-08 PR #28 entry = 01KR3903VA7DTNDJKQAFZ6DP8M (drift-findings block: "PR #28's CHANGELOG bucket is `### Fixed`, not `### Added`. The diff adds a constant, a field, an extracted public function, and three tests — by feature-add measure this is `### Added` work").*

2. **PR #31 (arc 08) — `### Changed` for a fix-shaped dep bump.** Commit subject reads *"chore: upgrade vt100 0.15 → 0.16, ratatui 0.29 → 0.30 (v1.41.18)"* (commit 105db8d, 2026-05-06). The CHANGELOG places the change under `### Changed`. The commit body names the bump as *"the proper fix for the `screen.rs:934.unwrap()` panic (caught defensively in v1.41.17)"* — explicitly fix-framed. The bucket choice does not match the commit-body framing. *Cite: arc-08 PR #31 entry = 01KR397RTYNS34SAGM46YJJRBY (drift-findings block: "The CHANGELOG bucket is `### Changed`, not `### Fixed`, even though the bump is 'the proper fix for the screen.rs:934.unwrap() panic'").*

3. **PR #36 (arc 05) — four-way classification asymmetry.** Commit subject reads *"fix: / and = match by substring, not anchored prefix (v1.41.23)"* (commit f505ee5, 2026-05-07). Four surfaces classify the same change four different ways: commit subject `fix:`; CHANGELOG `### Changed` (deliberate semantic shift); BUGS.md SMALL entry lifted to FIXED with `(fixed, v1.41.23)` tag; doc-comment characterizes the prior behavior as *"consistently surprising"* (regression-repair framing). The diff is structurally a behavior change in matcher semantics — anchored prefix replaced with substring; existing tests had to be rewritten because their data assumed the old semantics. The arc-05 PR #36 entry resolves the disposition: *"behavior change, framed and shipped as fix because the prior behavior was registered in BUGS.md SMALL as a user-reported bug,"* and the framing is *"internally consistent within the project's own classifications"* — but the four surfaces still classify differently. *Cite: arc-05 PR #36 entry = 01KR2AFHD42DHX6XQS7S6VK4M5 (drift-findings block: "The `fix/` slug + `fix:` subject + `### Changed` CHANGELOG section + BUGS.md FIXED entry classification asymmetry is the genuine drift").*

**Instance count: three.** All three candidate instances pre-collected by the brief verified against arc-entry citations. No revisions to the count.

**Notes on counting convention and pattern boundary.**

- *The arc-08-internal inverse-asymmetry pair.* PR #28 and PR #31 sit one calendar day apart in arc 08 and are the catalogue's two cleanest inverse-polarity instances of Pattern C. PR #28 over-claims-as-fix (additive content sitting under `### Fixed`); PR #31 under-claims-as-changed (fix content sitting under `### Changed`). The two PRs have opposite asymmetries: PR #28's diff is structurally an addition that the bucket frames as a fix; PR #31's diff is structurally a fix that the bucket frames as a change. That both occur in arc 08 within one day is observable; whether they read as a coherent classification policy (the maintainer treats user-report-origin as outweighing diff shape for `### Fixed` placement) or as two independent classification choices is for `insight-emergent-properties` to consider, not this thread.

- *PR #36 as the broadest-surface instance.* PR #28 and PR #31 each show one bucket-vs-content asymmetry per PR (one CHANGELOG bucket disagreeing with one diff shape). PR #36 shows four-way asymmetry across four surfaces (subject + CHANGELOG + BUGS.md + doc-comment). The three instances span single-axis to four-axis classification drift; the pattern's amplitude varies but its identity (bucket-or-classification choice does not match diff shape or framing-of-content) is consistent.

- *Cross-mention with Pattern A.* Pattern A's PR #31 instance and Pattern C's PR #31 instance describe the same PR through two different drift lenses: A is *commit-subject names two crates; diff names three*; C is *CHANGELOG bucket is `### Changed`; the bump is fix-framed*. These are two separate observable drifts at the same PR; the catalogue counts each in its own pattern.

- *Pattern's stated direction is bidirectional.* Pattern A is unidirectional (subject *understates* diff). Pattern C is bidirectional (bucket *over-* or *under-claims* relative to content). The PR #28 / PR #31 inverse pair makes the bidirectional reading explicit. The pattern's identity is *misalignment*, not *direction-of-misalignment*.

Provenance:
- arc-08 PR #28 entry = 01KR3903VA7DTNDJKQAFZ6DP8M (drift-findings block; user-report-origin framing for the `### Fixed` placement choice).
- arc-08 PR #31 entry = 01KR397RTYNS34SAGM46YJJRBY (drift-findings block; recurring asymmetry across arc 08 noted).
- arc-05 PR #36 entry = 01KR2AFHD42DHX6XQS7S6VK4M5 (drift-findings block; four-way asymmetry).
- `insight-drift` framing entry = 01KR3B7KW5QNRWHG6YTV9QSF07.
- `insight-drift` Pattern A entry = 01KR3BA3CZWA6TZCDKNJZDPAGH (cross-mention of PR #31 in Pattern A's catalogue).

<!-- Entry-ID: 01KR3BEGGEYB9VKTJ32WJNDG93 -->

---
Entry: Claude Code (caleb) 2026-05-08T08:33:25.883633+00:00
Role: scribe
Type: Note
Title: Pattern D: Documented-vs-wired drift at moment of merge — one instance, with observer-side naming at seed level

Spec: scribe

tags: #insight #drift

**Pattern statement.** A capability documented in CHANGELOG, help text, FEATURES.md, README, or other reader-facing surface is not actually wired in code at moment-of-merge. The user who reads the documentation and types the documented incantation receives a non-feature: an unknown-command flash, a no-op, a panic, or silent absence. The drift is at the moment of merge: between the diff that ships the documentation and the diff that wires the capability, the CHANGELOG temporarily lies. The pattern is most legible when the wire-up arrives in a separately-scoped follow-up PR — the gap between the two PRs is the period during which the lie is on `main`.

**Instance enumeration with arc-entry citations.**

1. **PR #13 (arc 08) — `:undo` shipped under `### Added`, not wired in `AppState::dispatch_command`'s punt list; closed by PR #14 25 minutes later.** PR #13's commit subject reads *"graveyard: R-undo + per-entry tar.zst + system trash cascade (v1.41.0)"* (commit 6b2be36, 2026-05-03). The CHANGELOG `### Added` block names `:undo` as a capability the user can invoke. PR #13's App-side handler exists; the routing does not. `AppState::dispatch_command` returns `CommandResult::NotHandled` for command names whose handler lives on App's terminal-touching half via a fixed punt list; `undo` and `graveyard` are not on the list. The user who reads the CHANGELOG and types `:undo` receives *"unknown command: undo"* — verbatim from PR #14's commit body (commit 24c49a0, 2026-05-03): *"Repro: type `:undo` → flash 'unknown command: undo'."*

   PR #14 ships 25 minutes later (merge 03:06 UTC, vs PR #13's merge at 02:41 UTC) with two lines added to the punt list. The bug existed on `main` for 25 minutes. *Cite: arc-08 PR #13 entry = 01KR38VEGHFT9JGRDCXXBFX8V1 (drift-findings block: "The CHANGELOG's `### Added` bucket for the graveyard contains the `:undo` reference; the same `:undo` is broken at merge time because `AppState::dispatch_command`'s punt list does not include it. The drift between 'documented capability' and 'wired capability' is the seed `onboarding-risk-register` entry 0 names"). Cite: arc-08 PR #14 entry = 01KR38XPJ07ZFQHH1TG6X461WN (the wire-up that closes the drift).*

**Instance count: one.** The single candidate instance pre-collected by the brief verified against arc-entry citations. No revisions to the count.

**Observer-side naming at seed level.**

The `onboarding-risk-register` seed entry 0 (= 01KR0P9JC8Z3DF6FQ1GJPF3VKA, dated 2026-05-07T07:44:05) catalogues this exact bug class with PR #14's release tag verbatim: *"Bitten on `:undo` (v1.41.1) and `:limit` historically."* The seed predates arc 08 (the arc-thread author) by hours; the maintainer-or-onboarding-author had named the bug class with the v1.41.1 release tag at risk-register level before arc 08 narrated the per-PR shape. The seed's *"and `:limit` historically"* fragment names a prior occurrence of the same bug class outside the 22-day window — the dual-`:command`-dispatch foot-gun has been hit before with `:limit`, and was hit again with `:undo`.

This is what observer-side naming of a Pattern-D instance looks like: the bug class is documented at risk-register level with the specific release tag of the most recent occurrence; the per-PR commit messages do not cite each other (PR #14's CHANGELOG describes the bug accurately and does not name PR #13 — the supersession is silent at commit-message level, explicit at seed level).

**Notes on counting convention and pattern boundary.**

- *Why one instance is worth flagging.* The brief framed Pattern D as *"the cleanest case where the CHANGELOG temporarily lied,"* which the verification confirms. Single-instance patterns risk being promoted to "the pattern is recurring" by analyst over-reach; the catalogue holds the count at one and notes that the dual-dispatch foot-gun *as a class* has prior occurrence (the seed's `:limit` reference) outside the verified window. *As an instance of the documented-vs-wired drift pattern at moment of merge during the 22-day window, the count is one.*

- *What distinguishes Pattern D from Pattern A.* Pattern A is at the subject-line / version-tag / classification-prefix level: the slug *describes* the diff inaccurately. Pattern D is at the documented-vs-wired level: the CHANGELOG / help text / FEATURES.md *promises a capability* that is not actually present in code. Pattern A's drifts are descriptive; Pattern D's drift is functional. A user affected by Pattern A might fail to find the right PR when grepping commit history; a user affected by Pattern D types the documented command and the application doesn't do the documented thing.

- *Why this is not Pattern E.* Pattern E (within-PR self-correction) is when a single PR's own diff retracts or reframes its own framing in the same diff. Pattern D's PR #13 → PR #14 pair is a *between-PR* correction, not a within-PR one. PR #13's CHANGELOG is not retracted by PR #13's own diff; it is closed by PR #14's two-line punt-list addition 25 minutes later. The catalogue places this instance in Pattern D and not Pattern E because the supersession is between two PRs, not intra-diff.

- *Why this is not pure Pattern B (bundle).* PR #13 is a feature-add that bundles four primitives (graveyard archive schema, viewer keymap, `:undo` one-shot, system-trash cascade) plus an R-prompt blast-radius surfacing. The bundling itself is observable but not inherently drift — the four primitives genuinely belong together as one capability. The Pattern-D drift is specifically the punt-list omission: of the four bundled primitives, two (`:undo` and `:graveyard`) are wired-in-handler-but-not-in-router; the other two are wired in both places. The drift is local to the routing surface, not the bundling shape.

- *Cadence consideration for the maintainer-experience axis (observation only).* The 25-minute gap between PR #13 and PR #14 is the second-shortest within-arc gap in the eight-arc record (arc 03 has a similar near-supersession; arc 08 has both this 25-minute pair and the 49-minute PR #30 → PR #31 pair). That the documented-vs-wired drift was caught and closed within 25 minutes is observable; the catalogue declines to evaluate whether 25 minutes is fast or slow. The seed records the bug class; the per-PR thread records the close.

Provenance:
- arc-08 PR #13 entry = 01KR38VEGHFT9JGRDCXXBFX8V1 (drift-findings block; CHANGELOG-vs-routing observation).
- arc-08 PR #14 entry = 01KR38XPJ07ZFQHH1TG6X461WN (the close; commit body verbatim *"Repro: type `:undo` → flash 'unknown command: undo'"*; supersession-silent-at-commit-message-level observation).
- `onboarding-risk-register` entry 0 = 01KR0P9JC8Z3DF6FQ1GJPF3VKA (seed-level naming verbatim *"Bitten on `:undo` (v1.41.1)"*; dual-dispatch foot-gun cataloguing).
- `onboarding-architecture` entry 0 = 01KR0P4W3ED1QZ8F44PFB2WPDZ (the dual-dispatch architecture: `AppState::apply` returns `ApplyResult` with no terminal access; the routing/handler split is the surface where Pattern D landed).
- arc-08 framing entry = 01KR38QZ1XQ6EP2A4QC94DRD80 (within-arc twin-pair count: this 25-min pair and the 49-min PR #30 → PR #31 pair).
- `insight-drift` framing entry = 01KR3B7KW5QNRWHG6YTV9QSF07.

<!-- Entry-ID: 01KR3BGMAKS4AZNZE2QFXH10W4 -->
