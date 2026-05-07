# history-arc-05-pager-surface — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: history-arc-05-pager-surface
Created: 2026-05-07T22:47:18.136862+00:00

---
Entry: Claude Code (caleb) 2026-05-07T22:47:18.136862+00:00
Role: scribe
Type: Note
Title: Framing: arc 05 — pager surface accretes through three phases plus an outlier; cadence A' (per-PR + phase-grouping)

Spec: scribe

tags: #history #arc-05

Arc title: `pager-surface`. Date span: 2026-05-02 (PR #11) to 2026-05-07 (PR #36). Member PRs:

- 7b941a4 (PR #11 fix/pager-wrap-bottom, 2026-05-02) — "pager: scroll_max accounts for wrapped visual rows (v1.40.1)" (commit 7b941a4, 2026-05-02).
- 34907a3 (PR #16 fix/fg-tail, 2026-05-04) — "fix: :fg seeds pager from buffer + scrolls to bottom (v1.41.3)" (commit 34907a3, 2026-05-04).
- 4f2f3ad (PR #17 fix/help-pager-search-multicol, 2026-05-05) — "fix: pager n/N follows match into column 2 of multi-col views (v1.41.4)" (commit 4f2f3ad, 2026-05-05).
- ee07307 (PR #20 feat/scroll-altscreen-hint, 2026-05-05) — "feat: alt-screen scroll hint + [pane] default_command + gd-vs-HEAD (v1.41.7)" (commit ee07307, 2026-05-05).
- eb6ddf6 (PR #23 feat/help-yf-and-percent-docs, 2026-05-05) — "feat: yf yanks cursor path + help-text discoverability fixes (v1.41.10)" (commit eb6ddf6, 2026-05-05).
- cf9e8ff (PR #33 feat/pager-visual-line-mode, 2026-05-06) — "feat: pager visual line mode for range yank (v1.41.20)" (commit cf9e8ff, 2026-05-06).
- c243549 (PR #35 feat/D-opens-pager-in-top-pane, 2026-05-06) — "feat: D opens cursor file in $PAGER as top overlay (v1.41.22)" (commit c243549, 2026-05-06).
- f505ee5 (PR #36 fix/search-substring-match, 2026-05-07) — "fix: / and = match by substring, not anchored prefix (v1.41.23)" (commit f505ee5, 2026-05-07).

**Arc 05 is the cadence-at-scale test.** Arc 01's reflection tail (= 01KR0XR504ZR10Y242JERT4K9S) named arc 05 explicitly: "We won't know whether [option A] stays right until arc 05 — the largest arc, eight PRs — sits down and tries it. Five entries for three PRs is light; thirteen for eight is heavy enough that the closure entry alone won't be doing the heavy summarization." Arc 05 inherits that question and answers it with a deliberate refinement.

**Cadence choice: option A' — per-PR entries plus phase-grouping in this framing entry.** Ten head entries: framing → 8 per-PR entries → closure. Same per-PR grain arc 01's option A established, but the framing entry organizes the eight PRs into three phases (α / β / γ) plus one outlier, so the closure entry doesn't have to summarize eight PRs from scratch. The phase-grouping does the structural work that the closure can't carry alone at this PR count.

Rationale:
- Arc 01's per-PR cadence was set as the precedent and arcs 03/04 inherited it for their 5-PR shapes. Arc 02's option B (phase-not-PR, 4 head entries for 2 PRs) was a principled departure for an investigation-then-harvest pair. Arc 05 at 8 PRs is heavier than 5 but no PR pair coheres into one move the way PR #5 + PR #12 do, so a wholesale shift to option B would lose per-PR granularity without gaining narrative coherence.
- PR #20 carries a mandatory back-reference to arc 02's investigation entry (= 01KR0YXXZRQR24CSNAK4Q7808T) for catalogue §2 alignment. PR #33 and PR #35 are optional back-reference targets for catalogue §4 direction-alignment. All three need stable per-PR entry IDs to be cited from arc 02 (which already names them from the other side) and from any eventual Phase 3 insight thread. Option B would collapse the citation targets.
- The phase-grouping is the load-bearing structural work the closure entry alone cannot carry at 8 PRs. Inheriting per-PR cadence and lifting phase-structure into the framing is a refinement, not a departure: A' generalizes A by pushing the framing entry to do summarization work that scaled awkwardly at this count.

Future arcs (06 at 4 PRs, 08 at 5 PRs) inherit this freely — at those PR counts, plain option A may still suffice; the closure verdict will name what arc 05 found.

**Phase grouping** (offered as the reading aid; verified against the diffs):

- **Phase α — pager mechanics** (PRs #11, #16, #17): how the pager scrolls, seeds, and searches across its own visual surface. All three ship under `### Fixed` in the CHANGELOG. Pre-existing pager job, with bugs in it.
- **Phase β — discoverability and surface accretion** (PRs #20, #23): the pager (and help-as-pager) surface gains user-facing discoverability and config infrastructure. PR #20 bundles three concerns; the alt-screen-hint half is the catalogue §2 alignment partner. PR #23 delivers `yf` plus help-text fixes that surface PR #20's `[pane] default_command` precedence chain to readers of `?`.
- **Phase γ — pager-as-mode** (PRs #33, #35): the pager extends from a thing-that-displays into a mode-you-enter (`V` for visual line mode → range yank) and a mode-you-launch-into-from-the-listing (`D` opens the cursor file in `$PAGER` as a top overlay). Both directionally align with arc 02's catalogue §4 ("Generalized pager picker") at the level of "render *into* the pager" rule, without directly executing the `PagerView::picker_items` + Enter-to-fire pattern that §4 specifies.
- **Outlier — search semantics** (PR #36): `Matcher::Prefix` → `Matcher::Substring`. Affects `/` (listing search) and `=` (limit filter) — not the pager's `n/N` search. Filed in arc 05 by the segmentation entry's read-surface direction; the fit reads as curatorial rather than structural. The per-PR entry resolves the "fix vs behavior change" framing flagged by `history-overview` segmentation entry (= 01KR0TWHTC1MPK4KJ08Y9SPE6P).

The arc shape, as a one-paragraph reading: the pager starts the window as a thing that scrolls correctly, seeds correctly, and searches correctly across its own surface (phase α — the existing job, with small bugs). Halfway through, it gains discoverability and a config field that surfaces to users via the help text (phase β). It ends the window as a mode the user enters with `V` for range-yank and launches into from the listing with `D` (phase γ). PR #36 sits adjacent — the same window's listing search-semantics shift, filed here by curatorial choice.

**Cross-thread back-link**: this thread continues from `history-overview` (segmentation entry = 01KR0TWHTC1MPK4KJ08Y9SPE6P; PR #5 special-handling entry = 01KR0TYF5F11DA8P5HNPA20DBK) and from arc 02 (investigation entry = 01KR0YXXZRQR24CSNAK4Q7808T, the back-reference hub for catalogue §2 and §4). Arc 05 follows arcs 01, 02, 03, 04 in baseline-write order; arc 06 (`history-arc-06-input-and-overlays`) is named at the closure entry for the next session.

The arc-content entries that follow this framing narrate PR #11, PR #16, PR #17, PR #20, PR #23, PR #33, PR #35, and PR #36 in arc order (which is also chronological within this arc). The closure entry forward-references arc 06. This thread remains OPEN for cross-arc references and the eventual Phase 3 insight layer.

Provenance:
- 7b941a4 (PR #11 fix/pager-wrap-bottom, 2026-05-02).
- 34907a3 (PR #16 fix/fg-tail, 2026-05-04).
- 4f2f3ad (PR #17 fix/help-pager-search-multicol, 2026-05-05).
- ee07307 (PR #20 feat/scroll-altscreen-hint, 2026-05-05).
- eb6ddf6 (PR #23 feat/help-yf-and-percent-docs, 2026-05-05).
- cf9e8ff (PR #33 feat/pager-visual-line-mode, 2026-05-06).
- c243549 (PR #35 feat/D-opens-pager-in-top-pane, 2026-05-06).
- f505ee5 (PR #36 fix/search-substring-match, 2026-05-07).
- `history-overview` framing entry = 01KR0TRFWT9W6WMFHC49YSW0BG (voice contract source).
- `history-overview` segmentation entry = 01KR0TWHTC1MPK4KJ08Y9SPE6P (arc-05 member-PR list; PR #36 fix-vs-behavior-change drift flag; PR #20 three-concern bundle drift flag).
- `history-overview` PR #5 special-handling entry = 01KR0TYF5F11DA8P5HNPA20DBK (back-reference contract: PR #20 mandatory; PR #33 / PR #35 optional, direction-alignment).
- `history-arc-01-foundation-hygiene` reflection tail = 01KR0XR504ZR10Y242JERT4K9S (cadence-at-scale question source).
- `history-arc-02-lazygit-investigation-and-harvest` investigation entry = 01KR0YXXZRQR24CSNAK4Q7808T (catalogue §2 + §4 source; back-reference hub).
- `history-arc-03-pane-behavior` PR #34 entry = 01KR10JBACRS3Z71WTHGBVCPJM (overlay-as-pane focus model — PR #35's launching-into-overlay rides this surface).
- `onboarding-architecture` entry 0 = 01KR0P4W3ED1QZ8F44PFB2WPDZ (current-state pane PTY ownership and pager surface description).
- `onboarding-overview` entry 0 = 01KR0NZNJ3KM6BJY09Q4P9D0NE (front door).

<!-- Entry-ID: 01KR29ZCRYY132QKB0HKRRRERQ -->
