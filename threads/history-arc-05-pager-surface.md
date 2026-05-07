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

---
Entry: Claude Code (caleb) 2026-05-07T22:51:22.089698+00:00
Role: scribe
Type: Note
Title: PR #20 (feat/scroll-altscreen-hint): three-concern bundle; alt-screen hint partially executes catalogue §2

Spec: scribe

tags: #history #arc-05

PR #20 opens phase β. Commit subject reads "feat: alt-screen scroll hint + [pane] default_command + gd-vs-HEAD (v1.41.7)" (commit ee07307, 2026-05-05) — three concerns under one slug, named explicitly in the subject. The segmentation entry on `history-overview` (= 01KR0TWHTC1MPK4KJ08Y9SPE6P) flagged this PR as a hard cluster-boundary call: "PR #20 (`feat/scroll-altscreen-hint`) bundles three unrelated concerns: alt-screen scroll hint, `[pane] default_command`, and `gd`-vs-HEAD. Filed under arc 05 (pager) on the basis of the alt-screen hint as the user-visible headline; the bundling itself is drift fuel for the insight layer."

Diff: 11 files, +161/-14. The three halves do not share a code path; they share a PR.

**Half 1 — Alt-screen scroll-mode hint (the catalogue §2 alignment partner).**

The PR's `### Added` CHANGELOG entry on this half reads verbatim: "Alt-screen scroll-mode hint. `^a v` against a full-screen TUI (codex, claude post-startup, vim, htop, lazygit) now flashes `scroll: on — alt-screen app, no scrollback (use the app's own history)` instead of the generic `(j/k nav, s save, Esc exit)` message. Alt-screen apps don't write to main-screen scrollback, so there's nothing for `^a v` to surface — the hint redirects the user to the app's built-in history viewer rather than letting them think scroll-back is broken. Detection via vt100's `Screen::alternate_screen()`. Single-screen apps (bash, plain shells) keep the old flash." (commit ee07307, 2026-05-05).

The diff shape (`git diff ee07307^1..ee07307^2 -- src/app/mod.rs`) shows the implementation: in the `Action::PaneScrollEnter` arm, the active pane's `is_alternate_screen()` is checked before flashing the message; an `if on_alt_screen` branch chooses the alt-screen hint, otherwise the generic message fires. The detection path is one method call on the `Pane` (which delegates to vt100's `Screen::alternate_screen()`); the routing is a one-branch conditional in the existing flash-info path.

**Catalogue §2 alignment — verified against arc 02.** The arc 02 investigation entry (= 01KR0YXXZRQR24CSNAK4Q7808T) names PR #20 explicitly: "PR #20 (`feat/scroll-altscreen-hint`, ee07307) ships an alt-screen scroll-hint component aligned with §2 in pattern (DIM hint at a transient row when context shifts), though PR #20 narrows to alt-screen detection rather than the broader options-map idea." Catalogue §2 ("Context-sensitive footer") proposes (per the same investigation entry, quoting the catalogue text): "into the prompt row, not the status bar... a `context_hints()` accessor on each overlay returning a `Vec<(key, label)>`; paint via `Style::DIM` when the prompt is otherwise idle." PR #20 honors the *spirit* — a context-sensitive hint surfaced when context shifts (the scroll-mode flash gains a context-aware variant) — without executing the catalogue's proposed shape (a per-overlay `context_hints()` accessor). The flash-info path is hardcoded in one `Action` arm; no `context_hints()` accessor is introduced.

The arc 02 disposition stands: **PARTIAL EXECUTION of catalogue §2**, narrowed to alt-screen detection. This entry confirms that disposition against the diff and back-references arc 02's investigation entry as the catalogue §2 source.

**Half 2 — `[pane] default_command` config key.**

The CHANGELOG entry on this half reads verbatim: "`[pane] default_command` config key. `^a c` (new pane tab) pre-fills its prompt with this command instead of the hardcoded `\"claude\"`. Precedence: `$SPYC_PANE_CMD` env var > config > `\"claude\"` fallback. The env var still wins so users can experiment per-shell without editing config; the new key just fixes the default for users who've switched to codex (or anything else) as their daily driver." (commit ee07307, 2026-05-05).

The diff (`src/config/mod.rs`, 62 insertions) introduces a new `PaneConfig` struct with a `default_command: Option<String>` field, the on-disk `FilePane` shape with `serde(deny_unknown_fields)`, and a per-field merge in `Config::merge`. The call site in `src/app/mod.rs::start_new_tab_prompt` is rewritten to thread the precedence chain explicitly: `std::env::var("SPYC_PANE_CMD").ok().or_else(|| self.state.config.pane.default_command.clone()).unwrap_or_else(|| "claude".to_string())`. The doc-comment on the call site names the precedence verbatim.

**Half 3 — `gd` now matches what the `~` marker says.**

The CHANGELOG entry on this half reads verbatim: "`gd` now matches what the `~` marker says. `gd` was running bare `git diff` (working-tree-vs-index) and flashing 'no unstaged changes' on rows the listing had marked dirty with `~`, because once you `git add` a file the diff lives in the index and unstaged is empty. `~` flags anything different from HEAD, so `gd` is now `git diff HEAD` — covers staged + unstaged + still folds in untracked-as-new — and the empty-flash now says 'no uncommitted changes'. `gD` (`--cached`) is unchanged for the 'what would commit' view." (commit ee07307, 2026-05-05).

The `src/app/mod.rs` diff shows the surgery: an `else { args.push("HEAD"); }` branch lands next to the existing `if cached { args.push("--cached"); }`, the empty-flash label moves from "unstaged" to "uncommitted," and the success label moves from "git diff (+ new)" to "git diff HEAD (+ new)." The doc-comment added to the diff names the marker-semantics alignment: "`gd` shows diff-vs-HEAD (staged + unstaged) so it matches the `~` marker semantics."

**Sequence-grain link to arc 04.** The `~` marker logic is the surface arc 04's PR #27 (`feat/git-staged-vs-unstaged`, 4e2afd9, 2026-05-06) extends one day later by splitting `GitFileStatus` from enum to struct with staged + unstaged halves. PR #20's `gd`-vs-HEAD shift aligns `gd` with the marker's pre-PR-#27 semantics ("anything different from HEAD"); PR #27 then enriches the marker to a two-cell display without disturbing the `gd` shift. The chain is implicit; arc 04's PR #27 entry (= 01KR134PZSQDAFVJK3M35FTKXF) names the marker-fidelity work without back-referencing PR #20.

**Drift findings flagged for the insight layer**:
- Three-concern bundle under one `feat/` slug. The segmentation entry already flagged this (= 01KR0TWHTC1MPK4KJ08Y9SPE6P, drift findings); arc 05 reconfirms against the diff. Each half is a clean, individually shippable change; the bundling itself is the drift, not any individual half.
- The catalogue §2 alignment is partial-not-direct. A reader who sees only PR #20's commit subject and CHANGELOG would not know the alt-screen hint is the alignment partner for an explicit roadmap item. The link is detectable only via `notes/lazygit-ux-catalogue.md` §2 (relocated to `BUGS.md` post-PR-#12) and arc 02's investigation entry. The PR itself does not cite the catalogue.
- The `gd`-vs-HEAD shift is a semantic-correction-on-an-existing-marker move — the kind of thing PR #36 (`fix/search-substring-match`, three days later) also embodies in the matcher domain. Same shape, different surfaces; flagged here for the eventual recurrence-or-emergence insight thread.

Provenance:
- ee07307 (PR #20 feat/scroll-altscreen-hint, 2026-05-05).
- `git diff ee07307^1..ee07307^2 -- CHANGELOG.md`: three `### Added` entries (alt-screen hint, `[pane] default_command`, `gd` HEAD) quoted verbatim above.
- `git diff ee07307^1..ee07307^2 -- src/app/mod.rs`: alt-screen branch in `Action::PaneScrollEnter`; `args.push("HEAD")` branch in `gd` path; precedence chain in `start_new_tab_prompt`.
- `git diff ee07307^1..ee07307^2 -- src/config/mod.rs`: `PaneConfig` struct, `FilePane` on-disk shape, `Config::merge` per-field merge.
- `Cargo.toml:3` post-merge: `version = "1.41.7"`.
- `history-arc-02-lazygit-investigation-and-harvest` investigation entry = 01KR0YXXZRQR24CSNAK4Q7808T (catalogue §2 source; PR #20 named at this entry as the §2 alignment partner with PARTIAL EXECUTION disposition).
- `history-overview` segmentation entry = 01KR0TWHTC1MPK4KJ08Y9SPE6P (three-concern bundle drift flag).
- `history-overview` PR #5 special-handling entry = 01KR0TYF5F11DA8P5HNPA20DBK (mandatory back-reference contract for arc 05's PR #20 entry).
- `history-arc-04-git-integration` PR #27 entry = 01KR134PZSQDAFVJK3M35FTKXF (`GitFileStatus` enum-to-struct refactor; downstream of PR #20's `~` marker semantic-alignment shift).
- `history-arc-05-pager-surface` framing entry = 01KR29ZCRYY132QKB0HKRRRERQ.
- `history-arc-05-pager-surface` PR #11 entry = 01KR2A121DSV81GM4EBCKAVAAM.
- `history-arc-05-pager-surface` PR #16 entry = 01KR2A2XY61GKZ1W52XQWGFBAH.
- `history-arc-05-pager-surface` PR #17 entry = 01KR2A4DCY3BR45ZQ7FQ2YQE4Q.

<!-- Entry-ID: 01KR2A6TT516XA5FEGVBXYPWD7 -->

---
Entry: Claude Code (caleb) 2026-05-07T22:52:24.140035+00:00
Role: scribe
Type: Note
Title: PR #23 (feat/help-yf-and-percent-docs): yf joins the y-prefix family; help text gains % substitution and default_command precedence

Spec: scribe

tags: #history #arc-05

PR #23 closes phase β. Commit subject reads "feat: yf yanks cursor path + help-text discoverability fixes (v1.41.10)" (commit eb6ddf6, 2026-05-05). Diff: 8 files, +81/-3. Two concerns share the slug — a new keybinding (`yf`) and two help-text additions — and unlike PR #20's bundle, both halves serve the same surface (the help-as-pager surface gaining discoverability).

**Half 1 — `yf` joins the y-prefix family.**

The PR's `### Added` CHANGELOG entry on this half reads verbatim: "`yf` — yank cursor file path (or all picks) to clipboard. New binding in the `y`-prefix family. Yields absolute paths so the receiving shell resolves them correctly regardless of where the user pastes them. With picks active, joins them newline-separated for one-per-line consumption (`xargs`, `git restore $(pbpaste)`, etc.). Came from a real-world ask — 'easy way to copy a file path for a one-off `git restore`' — that previously had to route through `!git restore %`." (commit eb6ddf6, 2026-05-05).

The diff distributes the work across the keymap and dispatch layers: `src/keymap/action.rs` (2 insertions) registers a new `Action` variant; `src/keymap/resolver.rs` (1 insertion) wires `yf` into the resolver; `src/app/mod.rs` (40 insertions) implements the yank-path-to-clipboard handler; `FEATURES.md` (3 insertions) documents the binding. The picks-active branch joins absolute paths newline-separated, mirroring the listing's existing pick-aware semantics in other y-prefix bindings.

**Half 2 — help text discoverability fixes.**

The PR's `### Changed` CHANGELOG entry on this half reads verbatim: "`?` help text discoverability. Added the long-missing `%` substitution under the `!` row (`%` = cursor file or all picks, shell-quoted; `%%` = literal percent), so a new user looking at the help can find the substitution mechanism without having to read source or remember the spy heritage. Updated the pane default-command row to reflect the v1.41.7 precedence chain (`$SPYC_PANE_CMD` env > `[pane] default_command` config > `\"claude\"` fallback) — the prior text only mentioned the env var." (commit eb6ddf6, 2026-05-05).

The `src/ui/help.rs` diff (14 insertions / 1 deletion) lands the two help-text additions on the same multi-column help-pager surface PR #17 made search-correct. The `%` substitution row had been a documented capability missing from `?`'s discoverable surface; the default-command row mirrors PR #20's precedence chain so a reader of `?` can see the env-var-then-config-then-claude order without going to the changelog.

**Sequence-grain dependency on PR #20.** The default_command precedence row in PR #23's help diff names the chain `$SPYC_PANE_CMD env > [pane] default_command config > "claude" fallback`. That precedence is exactly the one PR #20 established (PR #20's entry above quotes the call site in `start_new_tab_prompt`). PR #20 added the config field and the call-site precedence; PR #23 surfaces both to the help surface. The two PRs land 13 hours apart wall-clock and read as one capability shipped in two halves: PR #20 makes the config exist; PR #23 makes the config legible.

**Sequence-grain dependency on PR #17.** The help pager renders in two columns when the terminal is wide enough; the same multi-column surface PR #17 fixed for search correctness is the surface PR #23 extends. Without PR #17, the new `%` substitution row and the rewritten default-command row would land on a search-broken surface — `n/N` to find them would still pin to the bottom of the chunk. Phase α's mechanics fix is what makes phase β's discoverability addition land on a navigable surface.

**Sequence-grain forward note.** The `%` substitution row in PR #23 reads as a documentation extension of an existing surface (the `!`-capture path was already in place per `ARCHITECTURE.md:97-101`, "`!` captured commands also use a slave PTY since v1.12.0"). PR #36's substring matcher (the arc-05 outlier) does not touch the `%` substitution path; the two are orthogonal. The forward chain from PR #23's help-text discoverability fixes is into the eventual broader-help-text catalogue §5 ("Scoped `?` help") — a roadmap item carried at arc 02's investigation entry as a deferral pending §4's machinery, with no executing PR in the 22-day window. PR #23 widens the existing dump rather than scoping it; the §5 deferral remains.

**Drift findings flagged for the insight layer**:
- The `feat/` prefix scopes PR #23 as feature work; the `### Changed` half (help text) is documentation-of-existing-behavior under what the diff shape supports as a feature-classification by association with the `yf` half. The CHANGELOG itself splits the two halves cleanly (`### Added` for `yf`, `### Changed` for the help text), so the drift is at the slug-level only.
- The "real-world ask" framing in the CHANGELOG (`yf` "came from a real-world ask") is the kind of provenance signal that, if recurring across the window, would feed a recurrence reading at the insight layer. Captured here without interpretation.

Provenance:
- eb6ddf6 (PR #23 feat/help-yf-and-percent-docs, 2026-05-05).
- `git diff eb6ddf6^1..eb6ddf6^2 -- CHANGELOG.md`: `### Added` (yf) and `### Changed` (help text) entries quoted verbatim above.
- `git show eb6ddf6 --stat`: 8 files changed, 81 insertions, 3 deletions; `src/app/mod.rs` carries 40 insertions; `src/ui/help.rs` carries 14 insertions / 1 deletion.
- `Cargo.toml:3` post-merge: `version = "1.41.10"`.
- `history-arc-05-pager-surface` framing entry = 01KR29ZCRYY132QKB0HKRRRERQ.
- `history-arc-05-pager-surface` PR #11 entry = 01KR2A121DSV81GM4EBCKAVAAM.
- `history-arc-05-pager-surface` PR #16 entry = 01KR2A2XY61GKZ1W52XQWGFBAH.
- `history-arc-05-pager-surface` PR #17 entry = 01KR2A4DCY3BR45ZQ7FQ2YQE4Q (multi-column help-pager surface PR #23 extends).
- `history-arc-05-pager-surface` PR #20 entry = 01KR2A6TT516XA5FEGVBXYPWD7 (`[pane] default_command` precedence chain genesis; PR #23 surfaces it to help text).
- `history-arc-02-lazygit-investigation-and-harvest` investigation entry = 01KR0YXXZRQR24CSNAK4Q7808T (catalogue §5 deferral; PR #23 widens the existing dump rather than executing §5's scoping).
- `history-overview` segmentation entry = 01KR0TWHTC1MPK4KJ08Y9SPE6P.

<!-- Entry-ID: 01KR2A8PW1GRF82G4X8R7HFP6H -->

---
Entry: Claude Code (caleb) 2026-05-07T22:53:35.102244+00:00
Role: scribe
Type: Note
Title: PR #33 (feat/pager-visual-line-mode): VisualSelection lands on PagerView; the pager becomes a mode; direction alignment with catalogue §4

Spec: scribe

tags: #history #arc-05

PR #33 opens phase γ — the pager-as-mode shift. Commit subject reads "feat: pager visual line mode for range yank (v1.41.20)" (commit cf9e8ff, 2026-05-06). Diff: 7 files, +407/-4. The bulk lands in two files: `src/ui/pager.rs` (283 insertions) and `src/app/mod.rs` (82 insertions). This is the largest single-PR diff in arc 05 by line count.

**The capability shipped.**

The PR's `### Added` CHANGELOG entry reads verbatim (the relevant block in full): "Pager visual line mode for range yank. `V` in any pager view enters vi-style visual line mode: the anchor is set at the top visible line and `j` / `k` / `^d` / `^u` / `^f` / `^b` / `PageDown` / `PageUp` / `Space` / `g` / `G` / `Home` / `End` extend the cursor end (auto-scrolling when the cursor leaves the viewport). The selection is highlighted with the muted indigo cursor-bg-dim across the range and the active cursor row gets the brighter cursor-bg, so it reads like vi's visual cursor. The status footer shows `-- VISUAL --  L{lo}-L{hi}  ({n} lines)` so the range is unambiguous before you commit. `y` / `Y` yanks the inclusive range to the system clipboard via `pbcopy` and exits; `Esc` or `V` cancels without yanking. While the mode is active unrelated keys (`/`, `:`, `f`, `l`, `w`, etc.) are swallowed — exit visual mode first to use them, so a stray `/` doesn't silently reinterpret your selection mid-flight. Top-level `y` (yank source) and `Y` (yank visible) are unchanged outside visual mode. Also surfaced in the pager `?` help." (commit cf9e8ff, 2026-05-06).

**The struct shape.**

The `src/ui/pager.rs` diff introduces a new `VisualSelection { anchor: usize, cursor: usize }` struct with a `range()` method that returns the inclusive `(low, high)` range with `min`/`max` chosen so the order is anchor-direction-agnostic. The struct is added to `PagerView` as `visual: Option<VisualSelection>` and initialized to `None` in three constructors (`new`, `new_ansi`, and a third builder visible in the diff). The doc-comment names the mode boundary explicitly: "Mutually exclusive with the search/jump prompts (entering them cancels visual mode)." The boundary is the load-bearing invariant — visual mode is not a layered overlay over search; entering one cancels the other.

**The dispatch interception.**

The `src/app/mod.rs` diff (82 insertions) places an `if view.is_visual()` block at the top of the pager-key dispatch path, before any other key handling. The block routes the motion family to `view.visual_move(delta, viewport)` (which auto-scrolls when the cursor leaves the viewport), the `g` / `G` / `Home` / `End` family to `view.visual_jump_to(line, viewport)`, the `y`/`Y` to `view.yank_visual_to_clipboard()` with a flash on success or failure, and `Esc` / `V` to `view.cancel_visual()`. The unknown-key arm is the diagnostic shape worth pulling out:

```
_ => {
    // Unknown key while in visual mode — ignore so a
    // stray `/` or `:` doesn't silently trigger a
    // search/jump that the visual selection wasn't
    // expecting. User must Esc out first.
    return PostAction::None;
}
```

Swallow rather than fall through. The CHANGELOG framing ("a stray `/` doesn't silently reinterpret your selection mid-flight") is honored by an explicit drop-on-the-floor in dispatch, with a code-comment naming the reason. This is mode-discipline at the dispatch layer.

**Catalogue §4 alignment — direction, not execution.**

Arc 02's investigation entry (= 01KR0YXXZRQR24CSNAK4Q7808T) frames the catalogue §4 ("Generalized pager picker") disposition for PRs in this arc: "PR #33 (`feat/pager-visual-line-mode`, cf9e8ff, 2026-05-06)... additional pager-surface accretion that extends the read-through-pager direction the catalogue proposes. Confidence: DIRECTION ALIGNMENT, not direct execution of any specific catalogue item." That disposition holds against the diff. Catalogue §4's specific recommendation (per arc 02's investigation entry, quoting the catalogue): "extend the pager into a generalized pick-from-list mode" via "a `PagerView::picker_items: Vec<(Label, Action)>` field with Enter-to-fire dispatch."

PR #33 ships a *different* mode on the pager — range selection for yank, not pick-from-list with Enter-to-fire. The `VisualSelection` field is structurally analogous to `picker_items` (both are `Option`-shaped state on `PagerView` that gates a mode), but the semantics differ: `VisualSelection` selects a *line range* for one terminal action (yank), where `picker_items` would select *one of many discrete options* and dispatch an `Action`. PR #33 honors the §4 direction at the level of "the pager is the natural surface to host modes," without instantiating §4's specific picker pattern.

The arc-02 disposition is **DIRECTION ALIGNMENT**. This entry back-references arc 02's investigation entry to confirm the disposition against the diff.

**Sequence-grain dependency on PR #11.**

The `last_body_w: std::cell::Cell<u16>` field PR #11 added to `PagerView` for wrap-correct `scroll_max` is visible alongside the new `visual: Option<VisualSelection>` field in PR #33's struct diff. The two fields are sibling state on `PagerView`, neither disturbing the other. Phase α's mechanics field is the kind of small struct addition that, by being correct and contained, makes phase γ's mode addition cheap — PR #33 doesn't have to refactor `PagerView` to add visual mode, just add a sibling field.

**Drift findings flagged for the insight layer**:
- The CHANGELOG names `pbcopy` specifically for the clipboard path. The yank-to-clipboard plumbing on macOS-only `pbcopy` is the platform contract; whether other platforms (Linux `xclip` / `wl-copy`, Windows `clip.exe`) are supported is determinable from the `view.yank_visual_to_clipboard()` implementation, which is not in the snippets quoted here. Captured for the insight layer's portability reading if relevant.
- Visual mode swallowing `/` `:` `f` `l` `w` is the kind of "mode discipline at dispatch" that arc 03's PR #34 entry (= 01KR10JBACRS3Z71WTHGBVCPJM) noted at the overlay-vs-pane boundary. Same pattern, different surface. Flagged for the eventual recurrence-or-emergence insight thread.

Provenance:
- cf9e8ff (PR #33 feat/pager-visual-line-mode, 2026-05-06).
- `git diff cf9e8ff^1..cf9e8ff^2 -- CHANGELOG.md`: `### Added` entry quoted verbatim above.
- `git diff cf9e8ff^1..cf9e8ff^2 -- src/ui/pager.rs`: `VisualSelection` struct definition; `range()` method; `visual: Option<VisualSelection>` field on `PagerView` with mutually-exclusive-with-prompt doc-comment.
- `git diff cf9e8ff^1..cf9e8ff^2 -- src/app/mod.rs`: `if view.is_visual()` interception block; unknown-key swallow arm with reason comment.
- `git show cf9e8ff --stat`: 7 files changed, 407 insertions, 4 deletions.
- `Cargo.toml:3` post-merge: `version = "1.41.20"`.
- `history-arc-02-lazygit-investigation-and-harvest` investigation entry = 01KR0YXXZRQR24CSNAK4Q7808T (catalogue §4 source; PR #33 named at this entry as DIRECTION ALIGNMENT).
- `history-overview` PR #5 special-handling entry = 01KR0TYF5F11DA8P5HNPA20DBK (back-reference contract: PR #33 / PR #35 may back-reference arc 02 on read-through-pager direction; not mandatory).
- `history-arc-03-pane-behavior` PR #34 entry = 01KR10JBACRS3Z71WTHGBVCPJM (mode-discipline-at-dispatch precedent at overlay-vs-pane boundary).
- `history-arc-05-pager-surface` framing entry = 01KR29ZCRYY132QKB0HKRRRERQ.
- `history-arc-05-pager-surface` PR #11 entry = 01KR2A121DSV81GM4EBCKAVAAM (`last_body_w` cell on `PagerView`; sibling field to `visual`).
- `history-arc-05-pager-surface` PR #20 entry = 01KR2A6TT516XA5FEGVBXYPWD7.
- `history-arc-05-pager-surface` PR #23 entry = 01KR2A8PW1GRF82G4X8R7HFP6H.

<!-- Entry-ID: 01KR2AAX12XSNRNZPTXJT2TXJA -->
