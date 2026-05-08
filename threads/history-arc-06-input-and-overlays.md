# history-arc-06-input-and-overlays — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: history-arc-06-input-and-overlays
Created: 2026-05-08T00:36:51.422338+00:00

---
Entry: Claude Code (caleb) 2026-05-08T00:36:51.422338+00:00
Role: scribe
Type: Note
Title: Framing: arc 06 — picker overlays and dispatch correctness across a 2+2 calendar split, cadence option A

Spec: scribe

tags: #history #arc-06

Arc title: `input-and-overlays`. Date span: 2026-05-02 (PR #8) to 2026-05-06 (PR #32). Member PRs:

- 62fc129 (PR #8 feat/harpoon, 2026-05-02) — "harpoon: per-project pinned working set + =h filter (v1.39.0)" (commit 62fc129, 2026-05-02).
- 9043547 (PR #10 feat/quickselect, 2026-05-02) — "quick select: ^a u labeled-overlay picker for pane output (v1.40.0)" (commit 9043547, 2026-05-02).
- bfc4a18 (PR #25 fix/input-dispatch-hardening, 2026-05-06) — "fix: input dispatch hardening + --key-trace diagnostic switch (v1.41.12)" (commit bfc4a18, 2026-05-06).
- a7867fb (PR #32 fix/chord-priority-over-user-keymap, 2026-05-06) — "fix: chord prefixes beat user keybindings on the second key (v1.41.19)" (commit a7867fb, 2026-05-06).

**Cadence choice: option A (per-PR), with the framing naming the two phases the diff dates make obvious.** Six head entries: framing → 4 per-PR entries → closure. Arc 05 at 8 PRs adopted option A' (per-PR plus phase-grouping doing closure-summarization work that scaled awkwardly at that count); arc 06 at 4 PRs sits below the threshold A' was designed for. Plain A reads cleanly; the 2+2 wall-clock split is small enough that calling out the phases here is a reading aid rather than load-bearing structural work the closure can't carry.

**Phase grouping** (a reading aid for the 2+2 calendar split):

- **Phase α — picker overlays** (PRs #8, #10; both 2026-05-02). Two new picker-shaped overlays land within hours of each other. PR #8 ships harpoon — a per-project pinned working set with an `H1`..`H9` direct-slot-jump chord, an `Hh` modal menu for reorder/delete/jump, an `Ha`/`Hx` append/remove pair, and an `=h` listing filter that surfaces ancestor directories of harpooned paths. PR #10 ships quick select — a `^a u` labeled-overlay scanner over the visible pane output with alphabetic 1- or 2-letter labels (URLs, paths, git SHAs, IPv4, user-defined regex), case-as-intent (lowercase yank, uppercase "open"). Both ship as standalone overlays carrying their own picker structures (`HarpoonMenu`, `QuickSelect`); neither extends `PagerView`.
- **Phase β — dispatch correctness** (PRs #25, #32; both 2026-05-06). Four days later, two fixes to how key events reach handlers. PR #25 ships a "couldn't reproduce, two plausible failure modes addressed" defensive bundle (post-chord bounce-suppression on `^a-j`/`^a-k` plus a stranded-paste flash) bundled with a `--key-trace` / `SPYC_KEY_TRACE` diagnostic switch. PR #32 fixes a single rule: when a chord prefix is pending, the next key resolves the chord instead of being preempted by user keybindings.

A connection between the phases is detectable from the diffs but not asserted in the commits: phase α introduces the new `H` chord prefix family in PR #8, and PR #32's CHANGELOG names `H1`..`H9` explicitly among the broken-chord cases the fix addresses. Whether the connection is sequence-grain (phase α's new chord family surfaced phase β's dispatch question) or coincidental (the dispatch bug was always latent and would have surfaced with any user keybinding hitting an existing chord's second key) is for the per-PR entries to read against the diffs and tests, not for the framing to assert.

**Diagnosis (pattern register from the 10-pattern menu):** capability-accretion (precedent in arcs 04 and 05) followed by corrective hardening. Phase α is the same shape arcs 04 and 05 register: surfaces grow, capabilities accrete, the user gets new ways to summon things by keystroke. Phase β is the corrective follow-on as the surface widens enough that dispatch correctness becomes its own concern — the chord-prefix tree now has more branches, the focus-switch chord is ridden harder, and the picker-overlays have keys of their own that interact with the resolver's pending-state machinery. The interim themes entry (= 01KR2DYTPNCY5J5HPB99GT0J5M) named pattern 8 (reference-inventory) and pattern 10 (hub-and-pivot) as candidate diagnoses. Reference-inventory reads as a forced fit — none of the four PRs pauses to enumerate "what input means" — and hub-and-pivot reads as overstated for a 4-PR arc whose phase split is already visible in the dates. Capability-accretion-with-corrective-second-half is the register that fits the diff shape most directly; arc 04 and arc 05 are the precedents.

**Mandatory back-references (PR #8 and PR #10 to catalogue §4)**: arc 02's investigation entry (= 01KR0YXXZRQR24CSNAK4Q7808T) is the back-reference hub for catalogue §4 ("Generalized pager picker"). Arc 02 named PR #8 and PR #10 from the catalogue side as "parallel-but-different" — picker-shaped overlays that don't extend `PagerView::picker_items: Vec<(Label, Action)>`. Arc 05's PR #33 entry (= 01KR2AAX12XSNRNZPTXJT2TXJA) and PR #35 entry (= 01KR2AD5PV989H58E49E5D18NM) are the cross-arc parallel-pattern partners — both held DIRECTION ALIGNMENT with §4 from the pager-as-mode side; PR #8 and PR #10 hold PARALLEL PATTERN with §4 from the standalone-overlay side. Arc 05's closure (= 01KR2AJVZA1E85YSKHF4FNRQQ3) and story-tail (= 01KR2ANRAEFWWR5W9FQP11A0DB) named the cumulative reading: four PRs across two arcs hold §4 alignment; zero PRs execute the `PagerView::picker_items` shape; the deferral question goes to the insight layer.

PR #10's labeled-overlay shape might be expected to invoke catalogue §1 ("Numbered panels & direct-jump"), which the catalogue ranks **skip**. The PR #10 entry will refute honestly: PR #10's labels are alphabetic — a 23-letter alphabet `abcdefghilmnoprstuvwxyz` (skipping `q`/`Q` to leave the exit binding intact and `j`/`k` to spare reflexive vi motions), 1-letter when matches are few and 2-letter when many — not numeric, so §1's numbered-panels pattern is structurally not what PR #10 ships.

**Cross-thread back-link**: this thread continues from `history-overview` (segmentation entry = 01KR0TWHTC1MPK4KJ08Y9SPE6P; PR #5 special-handling entry = 01KR0TYF5F11DA8P5HNPA20DBK), arc 02 (investigation entry = 01KR0YXXZRQR24CSNAK4Q7808T, the back-reference hub for catalogue §4), arc 03 (seams-aside = 01KR11TME2KF5QFQ45GJYG8MC7, the `pane_focused`-three-meanings observation that the focus-axis branch of PR #25 rides), and arc 05 (framing = 01KR29ZCRYY132QKB0HKRRRERQ, PR #33 = 01KR2AAX12XSNRNZPTXJT2TXJA, PR #35 = 01KR2AD5PV989H58E49E5D18NM, story-tail = 01KR2ANRAEFWWR5W9FQP11A0DB). Arc 06 follows arcs 01–05 in baseline-write order; arc 07 (`history-arc-07-codex-and-mcp-bridge`) is named at the closure entry for the next session.

The arc-content entries that follow this framing narrate PR #8, PR #10, PR #25, and PR #32 in arc order (which is also chronological within this arc). The closure entry forward-references arc 07. This thread remains OPEN for cross-arc references and the eventual Phase 3 insight layer.

Provenance:
- 62fc129 (PR #8 feat/harpoon, 2026-05-02).
- 9043547 (PR #10 feat/quickselect, 2026-05-02).
- bfc4a18 (PR #25 fix/input-dispatch-hardening, 2026-05-06).
- a7867fb (PR #32 fix/chord-priority-over-user-keymap, 2026-05-06).
- `history-overview` framing entry = 01KR0TRFWT9W6WMFHC49YSW0BG (voice contract source).
- `history-overview` segmentation entry = 01KR0TWHTC1MPK4KJ08Y9SPE6P (arc-06 member-PR list).
- `history-overview` PR #5 special-handling entry = 01KR0TYF5F11DA8P5HNPA20DBK (back-reference contract for arc 06's PR #8 / PR #10).
- `history-arc-02-lazygit-investigation-and-harvest` investigation entry = 01KR0YXXZRQR24CSNAK4Q7808T (catalogue §4 source; back-reference hub).
- `history-arc-03-pane-behavior` seams-aside = 01KR11TME2KF5QFQ45GJYG8MC7 (`pane_focused`'s three meanings; focus-axis observation relevant to PR #25's chord-completion stamp).
- `history-arc-05-pager-surface` framing = 01KR29ZCRYY132QKB0HKRRRERQ (cadence A' precedent that arc 06 inherits but does not need at 4 PRs).
- `history-arc-05-pager-surface` PR #33 entry = 01KR2AAX12XSNRNZPTXJT2TXJA (cross-arc DIRECTION ALIGNMENT partner for catalogue §4).
- `history-arc-05-pager-surface` PR #35 entry = 01KR2AD5PV989H58E49E5D18NM (cross-arc DIRECTION ALIGNMENT partner for catalogue §4).
- `history-arc-05-pager-surface` closure = 01KR2AJVZA1E85YSKHF4FNRQQ3 (cumulative §4 reading after arc 05).
- `history-arc-05-pager-surface` story-tail = 01KR2ANRAEFWWR5W9FQP11A0DB (deferred §4 question to insight layer).
- interim themes entry = 01KR2DYTPNCY5J5HPB99GT0J5M (pattern-8 / pattern-10 candidate diagnoses; arc 06 settles on capability-accretion-with-corrective-second-half).
- `onboarding-architecture` entry 0 = 01KR0P4W3ED1QZ8F44PFB2WPDZ (current-state surface descriptions; `app/mod.rs` 9087-line dispatch surface).
- `onboarding-overview` entry 0 = 01KR0NZNJ3KM6BJY09Q4P9D0NE (front door).

<!-- Entry-ID: 01KR2G8042HWE419X0ESWKN205 -->
