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

---
Entry: Claude Code (caleb) 2026-05-07T22:48:12.621932+00:00
Role: scribe
Type: Note
Title: PR #11 (fix/pager-wrap-bottom): scroll_max walks visual rows when wrap is on; last_body_w cell lands on PagerView

Spec: scribe

tags: #history #arc-05

PR #11 is the first move in arc 05 and the first move in phase α (pager mechanics). Commit subject reads "pager: scroll_max accounts for wrapped visual rows (v1.40.1)" (commit 7b941a4, 2026-05-02). Diff: 4 files, +145/-11. The version bump in the commit subject is load-bearing: 138 of the 145 insertions land in `src/ui/pager.rs`, and Cargo.toml moves to `1.40.1`.

**The bug in user-visible terms.** The PR's CHANGELOG entry under `### Fixed` reads verbatim: "Pager: trailing logical lines were unreachable when long lines wrapped. A file with N logical lines (some long enough to wrap to multiple visual rows) showed 'Bot' before all content was visible — `scroll_max` capped the scroll using logical-line count, so wrapped portions of earlier lines consumed the visual budget and pushed the last few lines off-screen." (commit 7b941a4, 2026-05-02). The reproduction is named in the same entry: "Reported on `docs/spyc-logo.svg` (154 lines, several path elements wrap; lines 151-154 never appeared at 'Bot')." A reader who hits "Bot" expects to have seen everything; the wrap budget being mis-accounted broke that contract.

**The fix shape.** The CHANGELOG entry continues verbatim: "`scroll_max` now walks lines from the end summing visual rows when wrap is on, using a `last_body_w` cache the renderer updates each frame. Wrap-off pagers and multi-column pickers keep the original logical-line bound. Two regression tests in `pager::tests`." The walk-from-the-end approach reads as "how many logical lines fit in the viewport, going backward, when each line might consume multiple visual rows" — symmetric to the existing logical-line bound but priced in visual rows. The `last_body_w` cache is the bridge between the renderer (which knows the body width) and `scroll_max` (which needs the width to compute wrap rows).

**Sequence-grain consequence for arc 05.** The `last_body_w` cell appears in the `PagerView` struct as a `std::cell::Cell<u16>` field with a doc-comment that names its purpose for the bug it solves: "lines that wrap to multiple rows don't cause the trailing logical lines to fall off the viewport at 'Bot'." This field survives unchanged into PR #33's `feat/pager-visual-line-mode` work, where the same `PagerView` struct gains the `visual: Option<VisualSelection>` field alongside `last_body_w`. The forward chain is implicit but real: PR #11's struct addition is the small piece of pager-state machinery PR #33 inherits, and PR #33 adds its own field next to it without disturbing the cell. Phase α's mechanics work is what makes phase γ's mode work cheap.

**Wrap-off and multi-column preservation.** The CHANGELOG names the boundary explicitly: "Wrap-off pagers and multi-column pickers keep the original logical-line bound." PR #17 (`fix/help-pager-search-multicol`, three days later) lands in the multi-column branch of the same `scroll_max` family — the help pager renders in two columns when wide enough, and PR #11's preservation of the original logical-line bound for multi-column views is what keeps PR #17's work scoped to the search path rather than re-touching `scroll_max`. The mechanics phase reads as three small fixes that don't collide because each one respects the others' boundaries.

**Drift findings flagged for the insight layer**:
- The commit subject scopes the change cleanly to `scroll_max` accounting for wrapped visual rows. The CHANGELOG names the user-visible failure (the trailing-lines bug) and the reproduction file (`docs/spyc-logo.svg`) verbatim. No drift between subject, CHANGELOG, and diff at this PR; clean baseline for the rest of arc 05's drift comparisons.
- The version bump to v1.40.1 makes this the patch following PR #10's v1.40.0 (`feat/quickselect`, arc 06). PR #11 cuts a release alone (no bundling with adjacent arc-06 or arc-04 PRs that landed within hours); the release-cut shape here reads as "small bug fix, ship it now" rather than the bundled-cut PR #4 modeled in arc 01.

Provenance:
- 7b941a4 (PR #11 fix/pager-wrap-bottom, 2026-05-02).
- `git diff 7b941a4^1..7b941a4^2 -- CHANGELOG.md`: `### Fixed` entry quoted verbatim above.
- `git show 7b941a4 --stat`: 4 files changed, 145 insertions, 11 deletions; `src/ui/pager.rs` carries 138 insertions / 8 deletions.
- `src/ui/pager.rs` post-merge: `last_body_w: std::cell::Cell<u16>` field on `PagerView` (doc-comment quoted above; visible at the same struct in PR #33's diff).
- `Cargo.toml:3` post-merge: `version = "1.40.1"`.
- `history-arc-05-pager-surface` framing entry = 01KR29ZCRYY132QKB0HKRRRERQ.
- Forward references: PR #17 (4f2f3ad, 2026-05-05) preserves wrap-off / multi-column branch; PR #33 (cf9e8ff, 2026-05-06) inherits `last_body_w` cell on `PagerView` struct.
- `history-overview` segmentation entry = 01KR0TWHTC1MPK4KJ08Y9SPE6P (arc-05 member list).

<!-- Entry-ID: 01KR2A121DSV81GM4EBCKAVAAM -->
