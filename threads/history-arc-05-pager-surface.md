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

---
Entry: Claude Code (caleb) 2026-05-07T22:49:13.841111+00:00
Role: scribe
Type: Note
Title: PR #16 (fix/fg-tail): :fg seeds pager from buffer + scrolls to bottom; bundled with env_test_lock for parallel-test flake

Spec: scribe

tags: #history #arc-05

PR #16 is the second move in phase α. Commit subject reads "fix: :fg seeds pager from buffer + scrolls to bottom (v1.41.3)" (commit 34907a3, 2026-05-04). Diff: 14 files, +97/-17. The two-day gap from PR #11 is the longest within phase α and spans arc 04's PR #15 (`fix/git-status-and-pane-ctrl-c`, 2026-05-04) by two minutes wall-clock — the `^C`-route change in PR #15's CHANGELOG also appears in PR #16's diff context, but the surfaces are orthogonal (PR #15 in `app/mod.rs` `^C` dispatch; PR #16 in `:fg` foreground-task pager seeding).

**The bug.** The PR's `### Fixed` CHANGELOG entry leads verbatim: "`:fg` opened the pager scrolled to the top with the live tail off-screen. Resuming a backgrounded `cargo build` (or any chatty task) showed an empty pager, or — once the next chunk landed — content scrolled to row 0 with the latest output invisible, so it looked like nothing was running." (commit 34907a3, 2026-05-04). The user signal is named: a backgrounded long-running task resumed via `:fg` reads as "broken" because the pager's seed-from-empty plus tail-on-arrival behavior pushed any already-buffered output off the visible viewport.

**The root cause.** The CHANGELOG entry continues verbatim: "Root cause: `foreground_task`'s Running branch built the `PagerView` with `lines: Vec::new()` and only the streaming-tick repopulated it on the next chunk." The fix is named in the same paragraph: "Now seeded the same way `:task N`'s peek viewer is — render `task.buffer` into the pager and call `scroll_to_bottom_auto()` before handing the buffer to `pending_capture`." Two existing surfaces — `:task N`'s peek viewer and `:fg`'s resume — converge on one seeding pattern. The path that didn't seed-from-buffer was the bug; the path that did was the model.

**The bundle: env_test_lock.** PR #16 also lands a parallel-execution fix unrelated to the `:fg` work. The CHANGELOG entry under the same `### Fixed` section reads verbatim: "Flaky test suite under parallel execution. Several state-module tests (graveyard / harpoon / inventory / marks / sessions) and the shell-module tests mutate process-global env vars (`XDG_STATE_HOME`, `SHELL`) and raced when run in parallel, surfacing as random `NotFound` errors deep inside graveyard restores or wrong-shell-path assertions. `make check` was papered over with `--test-threads=1`; the CI Coverage step ran parallel and was failing intermittently. Added a single shared `crate::state::env_test_lock()` mutex; each affected test holds it for its full body. 15 consecutive parallel runs now pass."

The diff shape confirms the bundle: `src/state/graveyard.rs`, `src/state/harpoon.rs`, `src/state/inventory.rs`, `src/state/marks.rs`, `src/state/mod.rs`, `src/state/sessions.rs`, and `src/shell/mod.rs` each gain a small lock-acquisition addition; `src/state/mod.rs` gains the `env_test_lock()` helper itself; `Makefile` drops the `--test-threads=1` paper-over (per the diff: 4 lines / 3 deletions on `Makefile`). The `:fg` fix itself touches `src/app/mod.rs` (14 lines), `src/app/state.rs` (6 lines), and `BUGS.md` (16 lines). Two unrelated concerns under one `fix/fg-tail` slug.

**The Makefile diff is small but worth pulling out.** Arc 01's PR #2 (`chore/ci-hygiene`, d9b9360) wired `make check` to `Makefile:test` and the test path carried `--test-threads=1` to work around the env-var race. PR #16's `env_test_lock()` makes the lock structural, so the `--test-threads=1` workaround can come out of the Makefile. The arc-01 → arc-05 thread is implicit but real: arc 01 paid the workaround cost to make CI green; PR #16 here pays the structural cost so the workaround can retire. No commit names this; the diff edit on `Makefile` is the trace.

**Drift findings flagged for the insight layer**:
- The commit subject scopes the change to `:fg` seeding behavior. The diff includes a fully-orthogonal parallel-test fix across seven state and shell module files. The CHANGELOG names both fixes under one `### Fixed` section, so the drift is at the commit-subject level only — but the title-prefix `fix/fg-tail` does not signal the bundle.
- BUGS.md is updated as part of this PR (16 lines). The current-state seed `onboarding-architecture` (entry 0 = 01KR0P4W3ED1QZ8F44PFB2WPDZ) describes `:fg` and `pending_capture` as part of the captured-shell plumbing pattern: "`^Z` from a streaming `!` pager moves the `(child, writer, output_rx, buffer)` tuple from `App.pending_capture` into a `BackgroundTasks` collection." PR #16 is the genesis of the seed-from-buffer pattern on the resume path that completes the round trip the seed describes.

Provenance:
- 34907a3 (PR #16 fix/fg-tail, 2026-05-04).
- `git diff 34907a3^1..34907a3^2 -- CHANGELOG.md`: both `### Fixed` entries quoted verbatim above.
- `git show 34907a3 --stat`: 14 files changed, 97 insertions, 17 deletions.
- `src/state/mod.rs` post-merge: `crate::state::env_test_lock()` helper (named in the CHANGELOG; the call sites in `graveyard`, `harpoon`, `inventory`, `marks`, `sessions`, and `shell` test bodies are visible in the corresponding files' diffs).
- `Makefile` post-merge: `--test-threads=1` paper-over removed (4 lines / 3 deletions in the diff).
- `Cargo.toml:3` post-merge: `version = "1.41.3"`.
- `history-arc-05-pager-surface` framing entry = 01KR29ZCRYY132QKB0HKRRRERQ.
- `history-arc-05-pager-surface` PR #11 entry = 01KR2A121DSV81GM4EBCKAVAAM.
- `history-arc-01-foundation-hygiene` PR #2 entry = 01KR0W81XE4K3G7BBSP42GE1HH (`--test-threads=1` workaround genesis on `Makefile:test`).
- `onboarding-architecture` entry 0 = 01KR0P4W3ED1QZ8F44PFB2WPDZ (`pending_capture` / `BackgroundTasks` round-trip surface).
- `history-overview` segmentation entry = 01KR0TWHTC1MPK4KJ08Y9SPE6P.

<!-- Entry-ID: 01KR2A2XY61GKZ1W52XQWGFBAH -->

---
Entry: Claude Code (caleb) 2026-05-07T22:50:02.548693+00:00
Role: scribe
Type: Note
Title: PR #17 (fix/help-pager-search-multicol): scroll_to_match maps global → chunk-local in multi-column views

Spec: scribe

tags: #history #arc-05

PR #17 closes phase α. Commit subject reads "fix: pager n/N follows match into column 2 of multi-col views (v1.41.4)" (commit 4f2f3ad, 2026-05-05). Diff: 5 files, +106/-3. The bulk of the change is concentrated in `src/ui/pager.rs` (82 insertions / 1 deletion); BUGS.md (12 insertions), CHANGELOG.md (11 insertions), and the version bump (Cargo.toml + Cargo.lock) round out the rest.

**The bug.** The PR's `### Fixed` CHANGELOG entry leads verbatim: "`/...` then `n n n` in the help pager left the view stuck at the bottom." (commit 4f2f3ad, 2026-05-05). The user-visible failure is precise and small: search for a token in the help overlay, advance with `n`, watch the viewport pin itself to the bottom and the actual match disappear.

**The diagnosis.** The CHANGELOG entry continues verbatim: "The help overlay renders in two columns when the terminal is wide enough; in multi-column mode `scroll` is interpreted per-column (each column applies the same offset within its own chunk), but `scroll_to_match` was treating it as a global line offset. A match in column 2 produced a `scroll` value larger than `scroll_max` (= longest-chunk - viewport_h), got clamped to the bottom, and pinned every column at the end of its chunk — hiding the actual match." Two scroll semantics (per-column vs global line offset) used interchangeably by `scroll_to_match`; the per-column one is correct in multi-column mode and the bug is the conflation.

**The fix.** The CHANGELOG entry continues verbatim: "Now translates the match's global line index to a chunk-local offset before assigning to `self.scroll`. Single-column pagers behave unchanged. Pinned by a regression test." The translation is the surgery: take the global match index, divide by the chunk length, and use the remainder as the per-column scroll. Single-column pagers preserve their existing semantics because the global-vs-local distinction collapses when there is only one column.

**Sequence-grain dependency on PR #11.** The wrap-off / multi-column boundary PR #11 preserved (its CHANGELOG named it: "Wrap-off pagers and multi-column pickers keep the original logical-line bound") is exactly the boundary PR #17 lands inside. PR #11 left `scroll_max` for multi-column views computed against `longest-chunk - viewport_h` — that's the same `scroll_max` PR #17's diagnosis names ("`scroll_max` (= longest-chunk - viewport_h)"). PR #11 didn't break the multi-column case; PR #17 fixes a different scroll-related bug in the same code region. Phase α reads as three small fixes in `src/ui/pager.rs` that respect each other's boundaries.

**Sequence-grain forward note.** The help pager's two-column rendering is the same surface PR #23 (next, `feat/help-yf-and-percent-docs`) extends — PR #23 adds the `%` substitution row and the default-command precedence row to `src/ui/help.rs` (14 insertions / 1 deletion in PR #23's diff on that file), and the help text fits within the per-column budget PR #17 just made search-correct. PR #17 is the "search works in this overlay" fix that makes PR #23's discoverability addition land on a search-navigable surface.

**Drift findings flagged for the insight layer**:
- The commit subject scopes the change cleanly to `n/N` and "column 2 of multi-col views." The CHANGELOG diagnosis quotes a specific user-experience symptom ("stuck at the bottom") and names the precise scroll-clamp pathway that produces it. No drift between subject, CHANGELOG, and diff at this PR.
- The bug-class is a unit-mismatch: `scroll_to_match` mixed global-line and per-column-line units. The fix narrows to the translation point. A future feature that wants global navigation (search-across-columns ordering, say) would have to confront the same unit asymmetry from the other side; the seam is named in the diff but not generalized.

Provenance:
- 4f2f3ad (PR #17 fix/help-pager-search-multicol, 2026-05-05).
- `git diff 4f2f3ad^1..4f2f3ad^2 -- CHANGELOG.md`: `### Fixed` entry quoted verbatim above.
- `git show 4f2f3ad --stat`: 5 files changed, 106 insertions, 3 deletions; `src/ui/pager.rs` carries 82 insertions / 1 deletion.
- `Cargo.toml:3` post-merge: `version = "1.41.4"`.
- `history-arc-05-pager-surface` framing entry = 01KR29ZCRYY132QKB0HKRRRERQ.
- `history-arc-05-pager-surface` PR #11 entry = 01KR2A121DSV81GM4EBCKAVAAM (multi-column / wrap-off boundary preserved at PR #11; PR #17 lands inside that boundary).
- `history-arc-05-pager-surface` PR #16 entry = 01KR2A2XY61GKZ1W52XQWGFBAH.
- Forward reference: PR #23 (eb6ddf6, 2026-05-05) extends `src/ui/help.rs` with `%` substitution and default-command precedence rows on the same multi-column help-pager surface.
- `history-overview` segmentation entry = 01KR0TWHTC1MPK4KJ08Y9SPE6P.

<!-- Entry-ID: 01KR2A4DCY3BR45ZQ7FQ2YQE4Q -->
