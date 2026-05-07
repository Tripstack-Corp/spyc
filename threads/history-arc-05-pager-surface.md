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

---
Entry: Claude Code (caleb) 2026-05-07T22:54:49.561802+00:00
Role: scribe
Type: Note
Title: PR #35 (feat/D-opens-pager-in-top-pane): D launches $PAGER as top overlay; the pager becomes launchable from the listing; direction alignment with catalogue §4

Spec: scribe

tags: #history #arc-05

PR #35 is the second move in phase γ. Commit subject reads "feat: D opens cursor file in $PAGER as top overlay (v1.41.22)" (commit c243549, 2026-05-06). Diff: 9 files, +81/-13. The bulk lands in `src/app/mod.rs` (47 insertions); the keymap and help layers carry the rest.

**The capability shipped.**

The PR's `### Changed` CHANGELOG entry reads verbatim: "`D` now opens the cursor file in `$PAGER` as a top overlay (was: flash the date/time). Mirror of `V` for $EDITOR and a natural use of the focus-sharing overlay landed in v1.41.21. Workflow this enables: `D` on `docs/architecture.md`, `^a-j` into claude, work, `^a-k` to scroll the doc — without quitting less. The old date utility is still reachable via the typed command `:date`. `D` flashes an error on directories ('D: cannot page a directory') and when `$PAGER` is unset. Updated `?` help and FEATURES.md." (commit c243549, 2026-05-06). The capability statement is dense; the verbs that matter are *opens*, *as a top overlay*, and *focus-sharing*.

**The launching mechanism — `display_in_pane`.**

The `src/app/mod.rs` diff (47 insertions) introduces a private method on `App`:

```
fn display_in_pane(&mut self) {
    let Some(row) = self.state.rows.get(self.state.cursor.index) else { return; };
    let path = row.path.clone();
    if row.kind == EntryKind::Dir {
        self.state.flash_error("D: cannot page a directory");
        return;
    }
    let argv = shell::resolve_pager();
    if argv.is_empty() {
        self.state.flash_error("no $PAGER set");
        return;
    }
    let cmd = format!("{} {}", argv.join(" "), shell::shell_quote(&path.display().to_string()));
    let (rows, cols) = Self::top_overlay_size(self.effective_pane_pct(), self.pane_tabs.is_some());
    let cwd = self.state.listing.dir.clone();
    match Pane::spawn(&cmd, rows, cols, &cwd, &self.context_path) {
        Ok(p) => { self.top_overlay = Some(p); self.state.pane_focused = false; }
        Err(e) => self.state.flash_error(format!("spawn: {e}")),
    }
}
```

The doc-comment names the mirror relationship: "Mirror of `edit_in_pane` for the read path. Common workflow: `D` on a doc, `^a-j` into claude, work, `^a-k` to scroll." The `Action::DisplayInPane` arm in the dispatch lands as a one-line call to `self.display_in_pane()`; the keymap diff in `src/keymap/action.rs` (10 insertions / 4 deletions) and `src/keymap/resolver.rs` (4 insertions / 2 deletions) registers the action and binds `D` to it.

**The displaced `D` behavior.**

The CHANGELOG's parenthetical ("was: flash the date/time") names the prior binding. The diff preserves the date utility behind a typed command — `src/app/mod.rs` adds a `:date` arm:

```
if input == "date" {
    let _ = self.apply(&Action::Date);
    return PostAction::None;
}
```

with a comment naming the migration: "Used to be bound to `D` but `D` now opens the cursor file in $PAGER (the common request); the date utility lives on as a typed command for the rare hand-on-keyboard moment you actually want it." The displaced behavior is preserved on a less-common surface; the prime keystroke (`D`) is reassigned to the more-common request.

**Sequence-grain dependency on PR #34 (arc 03).**

The CHANGELOG names the dependency explicitly: "a natural use of the focus-sharing overlay landed in v1.41.21." PR #34 (`fix/top-overlay-focus-switch`, 8e9fb2c, 2026-05-06) — arc 03's last move — taught the overlay-vs-pane focus model that `;cmd` overlays can share focus with the bottom pane (`pane_focused = false` on overlay open, `^a-j`/`^a-k` chord keys bridging the two). PR #35's `display_in_pane` rides exactly that surface: the spawn into `top_overlay` plus the explicit `self.state.pane_focused = false` gives the user the docs-and-claude-side-by-side workflow the CHANGELOG names. Without PR #34, opening `$PAGER` as `top_overlay` would trap focus in the overlay; with PR #34, the same spawn produces the focus-sharing workflow.

Arc 03's PR #34 entry (= 01KR10JBACRS3Z71WTHGBVCPJM) named the model. Arc 05's PR #35 is the first new feature to consume it.

**The launching pattern — what it is structurally.**

The `display_in_pane` shape is a *launching pattern*: take the cursor row, validate (kind, env), build a command via `resolve_pager` + `shell_quote`, size the overlay via `top_overlay_size` + `effective_pane_pct`, capture cwd + context_path, and `Pane::spawn` into `top_overlay`. The pager is the destination, but the mechanism is general: any "open this listing row in a launched pty alongside the bottom pane" feature could ride the same dance with a different `resolve_*` helper. The `shell::resolve_pager` and `shell::shell_quote` re-exports trace back to arc 01's PR #4 (`fix/shell-aliases`, 1f41b4b, 2026-04-30), which introduced the `src/shell/mod.rs` module — the launching mechanism here consumes infrastructure landed on Day 0.

**Catalogue §4 alignment — direction, not execution.**

Per arc 02's investigation entry (= 01KR0YXXZRQR24CSNAK4Q7808T), PR #35 holds DIRECTION ALIGNMENT with catalogue §4: it extends the read-through-pager direction the catalogue proposes ("render *into* the pager") to a launchable-from-listing surface. The catalogue's specific recommendation — "extend the pager into a generalized pick-from-list mode" via `PagerView::picker_items: Vec<(Label, Action)>` — is not directly executed: PR #35 launches an external `$PAGER` (less, most, etc.) into a pty overlay; it does not populate spyc's internal `PagerView` with picker items. The pager-as-mode reading at PR #35 is "the listing now launches into the pager as a read-mode alongside other modes (the bottom pane, claude)," not "the pager hosts a generalized picker."

The arc-02 disposition holds: **DIRECTION ALIGNMENT**, no direct catalogue-item execution. This entry back-references arc 02's investigation entry to confirm the disposition against the diff.

**Drift findings flagged for the insight layer**:
- BUGS.md SMALL had an entry pre-existing this PR: "D in spyc pane should open in $PAGER in the top pane." PR #35 lifts the entry to FIXED with verbatim provenance ("(fixed, v1.41.22) `D` now opens the cursor file in `$PAGER`..."). The same arc 05 PR #36 (next) does the same lifting move with the substring matcher; two such lifts in two consecutive PRs flagged for the insight layer's recurrence catalogue.
- The launching mechanism (`display_in_pane`) is private to `App` and not factored as a standalone helper. A future feature that wants the same shape (open this script in `$EDITOR` alongside, open this directory in `$FILE_MANAGER` alongside) would either re-implement the dance or refactor `display_in_pane` / `edit_in_pane` into a common helper. The seam is named in the doc-comments ("Mirror of `edit_in_pane`") but not factored.

Provenance:
- c243549 (PR #35 feat/D-opens-pager-in-top-pane, 2026-05-06).
- `git diff c243549^1..c243549^2 -- CHANGELOG.md`: `### Changed` entry quoted verbatim above.
- `git diff c243549^1..c243549^2 -- src/app/mod.rs`: `display_in_pane` method body (quoted in full above), `:date` typed-command migration with comment, `Action::DisplayInPane` dispatch arm.
- `git diff c243549^1..c243549^2 -- BUGS.md`: SMALL line "D in spyc pane should open in $PAGER in the top pane" removed; FIXED entry added at v1.41.22.
- `git diff c243549^1..c243549^2 -- src/keymap/action.rs` / `src/keymap/resolver.rs`: `Action::DisplayInPane` registration; `D` binding.
- `Cargo.toml:3` post-merge: `version = "1.41.22"`.
- `history-arc-02-lazygit-investigation-and-harvest` investigation entry = 01KR0YXXZRQR24CSNAK4Q7808T (catalogue §4 source; PR #35 named at this entry as DIRECTION ALIGNMENT).
- `history-overview` PR #5 special-handling entry = 01KR0TYF5F11DA8P5HNPA20DBK (back-reference contract: PR #33 / PR #35 may back-reference arc 02 on read-through-pager direction).
- `history-arc-03-pane-behavior` PR #34 entry = 01KR10JBACRS3Z71WTHGBVCPJM (overlay-as-pane focus model; PR #35 is the first feature to consume it).
- `history-arc-01-foundation-hygiene` PR #4 entry = 01KR0WBKNMQF231X2T8KTGD9KS (`src/shell/mod.rs` genesis; `resolve_pager` and `shell_quote` re-exports consumed here).
- `history-arc-05-pager-surface` framing entry = 01KR29ZCRYY132QKB0HKRRRERQ.
- `history-arc-05-pager-surface` PR #33 entry = 01KR2AAX12XSNRNZPTXJT2TXJA (sibling phase-γ entry).

<!-- Entry-ID: 01KR2AD5PV989H58E49E5D18NM -->

---
Entry: Claude Code (caleb) 2026-05-07T22:56:07.059315+00:00
Role: scribe
Type: Note
Title: PR #36 (fix/search-substring-match): Matcher::Prefix → Matcher::Substring; behavior change framed as fix; arc 05's outlier

Spec: scribe

tags: #history #arc-05

PR #36 closes arc 05 and is the arc's outlier. Commit subject reads "fix: / and = match by substring, not anchored prefix (v1.41.23)" (commit f505ee5, 2026-05-07). Diff: 8 files, +73/-13. The bulk lands in `src/app/state.rs` (39 insertions / 3 deletions) and `src/app/mod.rs` (15 insertions / 5 deletions); BUGS.md, CHANGELOG.md, FEATURES.md, and the help row carry the rest.

**Why this PR is the outlier in arc 05.**

The framing entry above named PR #36 as the outlier: the matcher shift affects `/` (listing search) and `=` (limit filter) — neither of which is the pager's `n/N` search. The pager surface is unchanged by this PR; the listing/filter surface gains the semantic shift. The segmentation entry on `history-overview` (= 01KR0TWHTC1MPK4KJ08Y9SPE6P) filed PR #36 in arc 05 by read-surface direction and flagged it under drift findings: "PR #36 (`fix/search-substring-match`) reads as a behavior change ('/ and = match by substring, not anchored prefix') framed as a fix. Whether this is regression repair or behavior-change-as-fix is determinable only from a prior-state inspection." This entry resolves that question against the diff.

**The diff: `Matcher::Prefix` → `Matcher::Substring`.**

The `src/app/mod.rs` diff rewrites the `Matcher` enum. Pre-PR-36 (visible in the diff's `-` lines): `pub enum Matcher { Prefix(String), Glob(Pattern), Never, ... }` with `Self::Prefix(q) => lower.starts_with(q)` in `matches()`. Post-PR-36 (the `+` lines): `pub enum Matcher { Substring(String), Glob(Pattern), Never, ... }` with `Self::Substring(q) => lower.contains(q.as_str())` in `matches()`. The doc-comment on the enum is rewritten verbatim:

```
/// Search / filter matcher: case-insensitive substring for plain
/// text, glob for anything with `*`, `?`, or `[`. Used by `/`
/// (search) and `=` (limit filter). Substring (not anchored at the
/// start) so `/env` finds `.env`, `.envrc`, and `environment.toml`
/// — anchored prefix mode hid dot-prefixed files behind their
/// leading `.` and was consistently surprising. Globs are still
/// available for users who want anchoring (`env*`, `.env*`).
```

The change is a one-variant-rename plus a one-method-body change (`starts_with` → `contains`). The semantics shift is real: `/env` now finds `.env`, `.envrc`, `environment.toml` (substring); previously it found only names starting with literal `env` (anchored prefix). Globs (`*`, `?`, `[`) preserve the prior anchored semantics: `/env*` still re-anchors at the start, `*env*` is the explicit substring form. The escape hatches preserve the prior behavior for users who want it.

**The test rewrites.**

The `src/app/state.rs` diff (39 insertions / 3 deletions) does two things: rewrite existing tests whose data assumed prefix semantics, and add new tests pinning the substring semantics. The rewrites are diagnostic:

```
// Pick names with no shared substrings so the wrap behavior is
// unambiguous under substring matching: only `foo` contains `f`.
let s = state_with_rows(&["foo", "bar", "baz"]);
assert_eq!(s.find_match("f", 1, false), Some(0));
```

— the `find_wraps_around` test had used `["alpha", "beta", "gamma"]` searching for `"a"`, which under prefix matched only "alpha" but under substring matches all three (all contain `a`). The rewrite picks data where only one row contains the search token, isolating the wrap behavior from the matcher behavior. Same shape on `apply_search_prev_finds_match` (`"a"` → `"lph"`).

The new tests pin the substring semantics directly: `find_substring_matches_dot_prefixed_file` (`.env`, `.envrc`, `environment.toml` all match `env`); `find_substring_is_case_insensitive` (`README.md` matches `readme`, `Cargo.toml` matches `CARGO`); `find_glob_remains_anchored` (`env*` still matches only `envoy`, hiding `.env`).

**Fix vs. behavior change — disposition resolved.**

The diff is, structurally, a behavior change in matcher semantics: anchored prefix is replaced with substring; the `Matcher` enum's variant name changes; the `matches()` body changes from `starts_with` to `contains`; existing tests had to be rewritten because their data assumed the old semantics. By the criterion of "is this a different return value for the same input," yes, this is a behavior change.

The classification across surfaces splits, however:
- **Commit subject**: `fix:` prefix.
- **CHANGELOG**: `### Changed` (not `### Fixed`).
- **BUGS.md**: a SMALL entry pre-existing this PR — "/ should match within names - it seems to assume ^ e.g. env won't match .env" — is lifted to FIXED with `(fixed, v1.41.23)` framing.
- **Doc-comment**: "anchored prefix mode hid dot-prefixed files behind their leading `.` and was consistently surprising" — this is regression-repair framing.

The CHANGELOG's `### Changed` placement is the most honest classification of the diff: a deliberate semantic shift, with escape hatches preserved. The commit-subject `fix:` and the BUGS.md FIXED lift are honest to the user-experience-improvement framing — the prior behavior was registered in BUGS.md SMALL as a user-reported issue, and this PR resolves the registered issue. The doc-comment splits the difference: it acknowledges the shift while characterizing the prior behavior as "consistently surprising."

**The disposition: behavior change, framed and shipped as fix because the prior behavior was registered in BUGS.md SMALL as a user-reported bug.** The drift the segmentation entry flagged holds — the `fix/` prefix and `fix:` subject overstate the regression-repair characterization; the diff is genuinely a semantic shift. But the framing is internally consistent within the project's own classifications: BUGS.md tracked the prior behavior as a bug; this PR resolves the registered bug; the CHANGELOG's `### Changed` placement preserves the honest semantic-shift framing for readers who don't follow BUGS.md.

**Sequence-grain note.** PR #35 (previous, `feat/D-opens-pager-in-top-pane`) also lifted a BUGS.md SMALL entry to FIXED ("D in spyc pane should open in $PAGER in the top pane"). PR #36 does the same lift on the matcher entry. Two consecutive arc-05 PRs that lift BUGS.md SMALLs to FIXED is a recurrence shape worth flagging for the insight layer.

**Drift findings flagged for the insight layer**:
- The `fix/` slug + `fix:` subject + `### Changed` CHANGELOG section + BUGS.md FIXED entry classification asymmetry is the genuine drift. Captured here against the diff for the eventual insight layer's classification-surface catalogue.
- The escape hatches (`env*` for anchored, `*env*` for explicit substring) preserve the prior behavior at the user level. A future feature that needs anchored matching at the *code* level (an LSP integration that needs strict prefix resolution, say) cannot use `Matcher` directly — the substring shift is not reversible at the enum level. The seam is named in the doc-comment ("Globs are still available for users who want anchoring") but does not generalize to the code-internal use case.
- This PR closes the 22-day window: commit f505ee5, the second-to-last PR before PR #37 (`fix/mcp-socket-project-scoped-discovery`, a303251, 2026-05-07) which arc 07 will narrate. PR #37 lands 36 minutes after PR #36 wall-clock; arc 07 inherits a still-warm window-end.

Provenance:
- f505ee5 (PR #36 fix/search-substring-match, 2026-05-07).
- `git diff f505ee5^1..f505ee5^2 -- CHANGELOG.md`: `### Changed` entry quoted verbatim above.
- `git diff f505ee5^1..f505ee5^2 -- src/app/mod.rs`: `Matcher` enum rewrite (`Prefix` → `Substring`); `matches()` body change (`starts_with` → `contains`); doc-comment quoted verbatim above.
- `git diff f505ee5^1..f505ee5^2 -- src/app/state.rs`: existing-test rewrites (`find_wraps_around`, `apply_search_prev_finds_match`); new tests `find_substring_matches_dot_prefixed_file`, `find_substring_is_case_insensitive`, `find_glob_remains_anchored`.
- `git diff f505ee5^1..f505ee5^2 -- BUGS.md`: SMALL "/ should match within names - it seems to assume ^ e.g. env won't match .env" removed; FIXED entry added at v1.41.23.
- `Cargo.toml:3` post-merge: `version = "1.41.23"`.
- `history-overview` segmentation entry = 01KR0TWHTC1MPK4KJ08Y9SPE6P (PR #36 fix-vs-behavior-change drift flag source).
- `history-arc-05-pager-surface` framing entry = 01KR29ZCRYY132QKB0HKRRRERQ (outlier framing).
- `history-arc-05-pager-surface` PR #35 entry = 01KR2AD5PV989H58E49E5D18NM (BUGS.md SMALL → FIXED lift, immediate prior-PR precedent).
- `history-arc-05-pager-surface` PR #11 entry = 01KR2A121DSV81GM4EBCKAVAAM.
- `history-arc-05-pager-surface` PR #16 entry = 01KR2A2XY61GKZ1W52XQWGFBAH.
- `history-arc-05-pager-surface` PR #17 entry = 01KR2A4DCY3BR45ZQ7FQ2YQE4Q.
- `history-arc-05-pager-surface` PR #20 entry = 01KR2A6TT516XA5FEGVBXYPWD7.
- `history-arc-05-pager-surface` PR #23 entry = 01KR2A8PW1GRF82G4X8R7HFP6H.
- `history-arc-05-pager-surface` PR #33 entry = 01KR2AAX12XSNRNZPTXJT2TXJA.

<!-- Entry-ID: 01KR2AFHD42DHX6XQS7S6VK4M5 -->

---
Entry: Claude Code (caleb) 2026-05-07T22:57:56.079911+00:00
Role: scribe
Type: Note
Title: Closure: arc 05 baseline complete; cadence-at-scale results from option A'; arc 06 (input-and-overlays) follows

Spec: scribe

tags: #history #arc-05

Arc 05 baseline narration complete. Eight PRs, three phases plus one outlier, written across ten head entries (framing + 8 per-PR + this closure). The arc spans 2026-05-02 (PR #11) to 2026-05-07 (PR #36), the longest baseline-arc span in the reconstruction tied with arc 04. The pager (and help-as-pager) surface accretes capability from "scrolls / seeds / searches correctly" (phase α) through "discoverability and config" (phase β) to "mode-you-enter and launchable-from-listing" (phase γ), with PR #36's matcher shift filed adjacent on read-surface direction.

**Forward reference: arc 06 — `history-arc-06-input-and-overlays`.** Picks up next. Member PRs: PR #8 (`feat/harpoon`, 62fc129, 2026-05-02), PR #10 (`feat/quickselect`, 9043547, 2026-05-02), PR #25 (`fix/input-dispatch-hardening`, bfc4a18, 2026-05-06), PR #32 (`fix/chord-priority-over-user-keymap`, a7867fb, 2026-05-06). Arc 06 carries a **mandatory back-reference to arc 02**: PRs #8 and #10 are named at arc 02's investigation entry (= 01KR0YXXZRQR24CSNAK4Q7808T) as catalogue §4 PARALLEL PATTERN ("not direct execution of `notes/lazygit-ux-catalogue.md` §4 'Generalized pager picker'"). Arc 05 just shipped two more catalogue §4 direction-alignments (PR #33 visual-line-mode, PR #35 D-launches-pager); arc 06 ships the catalogue §4 *parallel* pattern in PRs #8 and #10. Three PRs across two arcs all align with §4 directionally without one of them executing §4's specific picker pattern; this is the kind of distribution arc 02's catalogue framing was built to make legible.

**Cadence-at-scale verdict — option A' tested at 8 PRs.**

Arc 01's reflection tail (= 01KR0XR504ZR10Y242JERT4K9S) named arc 05 as the explicit cadence-at-scale test point. Verdict against the lived experience of writing this thread:

- **Per-PR grain held at 8 PRs.** Eight per-PR entries with provenance blocks each plus a framing and a closure scaled without strain. Each per-PR entry remained focused on its diff and ran 400–800 words; none collapsed into a one-paragraph summary that arc 01's contemplated single-summary-entry would have produced.
- **The phase-grouping in the framing was load-bearing.** Three phases (α / β / γ) plus one outlier organized the eight PRs around their structural role rather than chronology. Without phase-grouping, the closure entry would carry the entire summarization burden at 8 PRs — exactly what arc 01's reflection tail flagged ("thirteen [entries] for eight is heavy enough that the closure entry alone won't be doing the heavy summarization"). The framing took on that summarization role explicitly, leaving the closure free to do this verdict-and-forward-pointing work.
- **Per-PR entry IDs preserved for the back-reference network.** PR #20's mandatory back-reference to arc 02's investigation entry, and PR #33 / PR #35's optional back-references on catalogue §4 direction-alignment, both required stable per-PR entry IDs as citation targets. Option B (phase-not-PR) would have collapsed these citation targets; option A would have shipped 13 entries; option A' gave 10 entries with the per-PR citation precision intact.

**Recommendation forward: arcs 06 (4 PRs) and 08 (5 PRs) inherit option A.** At those PR counts, plain option A (framing + per-PR + closure) is sufficient — the closure can summarize 4–5 PRs without straining. A' is the right shape only when the closure-summarization burden gets heavy enough that the framing needs to share it, which arc 05 found to be the case at 8. Future arcs at 6+ PRs (none in the remaining schedule) should consider A'. Option B remains the right shape for arcs whose material genuinely cohere into phases-not-PRs (as arc 02's investigation-then-harvest pair did).

**Catalogue §4 cumulative reading after arc 05.** Catalogue §4 ("Generalized pager picker") was ranked at arc 02's investigation entry as "Highest-leverage of the lazygit borrows" with the proposed shape: `PagerView::picker_items: Vec<(Label, Action)>` field with Enter-to-fire dispatch. After arc 05, four PRs across two arcs hold direction alignment with §4 without any executing the specific picker pattern: arc 06's PR #8 (harpoon, parallel picker overlay), arc 06's PR #10 (quickselect, parallel picker overlay), arc 05's PR #33 (visual-line-mode on the pager), arc 05's PR #35 (D launches into $PAGER). The shape arc 02's catalogue named — `PagerView::picker_items` — has not landed in the 22-day window. Whether it lands, whether the parallel patterns make it unnecessary, or whether the §4 deferral is structural is a question for the eventual insight layer to take up.

**Voice / sequence-over-timing audit on this thread:**
- Banned mindset words referencing the maintainer ("wants," "thinks," "believes," "decided," "feels," "intends to" without "the commit message," "is concerned that"): no occurrences in head entries.
- Hedge tokens used freely from the whitelist ("appears to," "reads as," "consistent with," "the diff shape suggests," "points toward," "aligns with," "the commit message indicates"): present across entries.
- Verbatim commit-subject and CHANGELOG quoting: every per-PR entry carries at least one verbatim quote with `(commit <sha>, <date>)` attribution.
- No clock-padding language ("minutes later," "in the same hour"): sequence-faithful framing privileged ("first move," "next move," "phase α closes," "phase β opens").

**Drift findings carried into the insight layer (cumulative across arc 05):**
- PR #16 bundles the `:fg` fix with an orthogonal `env_test_lock()` fix; commit-subject scopes to `:fg` only.
- PR #20 bundles three fully-orthogonal concerns (alt-screen hint, default_command, gd-vs-HEAD) under one `feat/` slug; flagged at `history-overview` segmentation, reconfirmed against the diff.
- PR #23's `feat/` prefix covers a `### Changed` half (help-text discoverability) — slug-level drift, CHANGELOG-level honest.
- PR #33 names `pbcopy` specifically for clipboard plumbing; portability surface flagged for the insight layer.
- PR #36's classification splits across surfaces: commit-subject `fix:`, CHANGELOG `### Changed`, BUGS.md FIXED. Diff is genuinely a behavior change; the project tracks it as a fix because the prior behavior was registered in BUGS.md SMALL.
- Two consecutive arc-05 PRs (PR #35, PR #36) lift BUGS.md SMALLs to FIXED — a recurrence shape worth flagging for the insight layer's lift-pattern catalogue.
- Implicit forward chains within arc 05: PR #11's `last_body_w` cell on `PagerView` is sibling state to PR #33's `visual: Option<VisualSelection>` field; PR #16's seed-from-buffer pattern is the path PR #35's `display_in_pane` mirror-of-`edit_in_pane` extends in spirit (launch into a pty, render content, share focus).
- Cross-arc implicit chains: PR #20's `gd`-vs-HEAD shift aligns `gd` with the `~` marker semantics that arc 04's PR #27 enriches one day later; PR #35's `display_in_pane` consumes arc 03's PR #34 overlay-as-pane focus model; PR #35's `resolve_pager` and `shell_quote` consume arc 01's PR #4 `src/shell/mod.rs` infrastructure; PR #16's `env_test_lock()` retires arc 01's PR #2 `--test-threads=1` workaround on `Makefile:test`.

**Arc 05 done-definition self-check**:
- ☑ `watercooler_health` reported Healthy against the spyc code_path.
- ☑ All required reads completed (`history-overview` 6 entries; `history-arc-01-foundation-hygiene` 7 entries; arc 02 investigation entry 01KR0YXXZRQR24CSNAK4Q7808T plus full thread summary; arc 03 story-tail 01KR11S8RG29J98QKN1H0VAA6W plus seams-aside 01KR11TME2KF5QFQ45GJYG8MC7 plus full thread summary; arc 04 story-tail 01KR13CJ5XS5VREYA4741JHDSQ plus full thread summary; `onboarding-architecture` entry 0; `onboarding-overview` entry 0).
- ☑ Diagnosis stated in framing entry: option A' (per-PR with phase-grouping in framing); rationale documented.
- ☑ Cadence shape stated explicitly: A'.
- ☑ Phase grouping (α / β / γ + outlier) verified against diffs.
- ☑ PR #20 entry back-references arc 02's investigation entry (= 01KR0YXXZRQR24CSNAK4Q7808T) for catalogue §2 PARTIAL EXECUTION.
- ☑ PR #33 / PR #35 entries verify against catalogue §4: DIRECTION ALIGNMENT, no direct picker_items execution.
- ☑ PR #36 entry resolves the "fix vs. behavior change" framing against the diff.
- ☑ Each head entry follows the head voice contract.
- ☑ Provenance blocks on every entry.
- ☑ Tags `#history` and `#arc-05` carried on every entry; tail entries will also carry `#tail`.
- ☑ No insight-layer claims (no recurrence-naming, emergent-property naming, forward predictions, motive attribution).
- ☑ No PR #11/16/17/20/23/33/35/36 promoted to Decision type; all entries are Notes.
- ☑ Thread will remain OPEN.

The two tail entries that follow this closure operate under the new tail voice contract: story-shaped, no formula, honest to the underlying material rather than to a formal scaffold. The first tail tells the story of the pager-becoming-a-mode-not-just-a-display arc; the second is a seams-aside on two forward-pointing observations the story-tail glosses past. Tail count was chosen to match the material, not a target.

Provenance:
- All 8 SHAs and PR numbers from `git log --grep='Merged in' --reverse --format='%h %ai %s'` (subset for arc 05).
- `history-arc-05-pager-surface` head entries 0–9 = 01KR29ZCRYY132QKB0HKRRRERQ, 01KR2A121DSV81GM4EBCKAVAAM, 01KR2A2XY61GKZ1W52XQWGFBAH, 01KR2A4DCY3BR45ZQ7FQ2YQE4Q, 01KR2A6TT516XA5FEGVBXYPWD7, 01KR2A8PW1GRF82G4X8R7HFP6H, 01KR2AAX12XSNRNZPTXJT2TXJA, 01KR2AD5PV989H58E49E5D18NM, 01KR2AFHD42DHX6XQS7S6VK4M5, this closure.
- `history-overview` entries 0–5 = 01KR0TRFWT9W6WMFHC49YSW0BG, 01KR0TWHTC1MPK4KJ08Y9SPE6P, 01KR0TYF5F11DA8P5HNPA20DBK, 01KR0V01TAJVSZFE5ZNMCZHQSF, 01KR0XNGQ0GS2QYN855X25NPZ7, 01KR0XPRBJVH80FNZ9XN7DG01E.
- `history-arc-01-foundation-hygiene` reflection tail = 01KR0XR504ZR10Y242JERT4K9S (cadence-at-scale source question).
- `history-arc-02-lazygit-investigation-and-harvest` investigation entry = 01KR0YXXZRQR24CSNAK4Q7808T (catalogue §2 + §4 source).
- `history-arc-03-pane-behavior` PR #34 entry = 01KR10JBACRS3Z71WTHGBVCPJM (overlay-focus-sharing model PR #35 consumes); story-tail = 01KR11S8RG29J98QKN1H0VAA6W (tail register precedent); seams-aside = 01KR11TME2KF5QFQ45GJYG8MC7 (tail register precedent).
- `history-arc-04-git-integration` story-tail = 01KR13CJ5XS5VREYA4741JHDSQ (tail register precedent for capability-accretion narration).
- Forward reference: `history-arc-06-input-and-overlays` (next arc, mandatory back-reference to arc 02 catalogue §4).
- `history-arc-07-codex-and-mcp-bridge` and `history-arc-08-recoverability-and-deps` (named in `history-overview` closure entry's checklist).

<!-- Entry-ID: 01KR2AJVZA1E85YSKHF4FNRQQ3 -->

---
Entry: Claude Code (caleb) 2026-05-07T22:59:30.764025+00:00
Role: scribe
Type: Note
Title: Tail: arc 05 told as a story — the pager goes from a thing that displays to a mode you live in

Spec: scribe

tags: #history #arc-05 #tail

What changes between PR #11 and PR #36 isn't just the pager's capabilities — it's what the pager *is for*.

The arc opens with three fixes to a pager that, structurally, is a thing that displays text. PR #11 fixes a wrap-accounting bug so trailing logical lines aren't lost at the bottom of the viewport. PR #16 fixes a seed-from-buffer bug so `:fg`'s pager doesn't open empty when a long-running task gets resumed. PR #17 fixes a multi-column scroll-translation bug so `n` after `/` lands on the actual match instead of pinning the viewport to the bottom of column 2. All three ship under `### Fixed`. None of them adds capability; all three repair the pager's existing job. Phase α is the pager doing what it's already doing, correctly.

The arc ends — five days later — with a pager you press `V` to enter as a mode for range-yank, a pager you press `D` to launch from the listing as a top overlay alongside the bottom pane, and a search semantics that finds `.env` when you type `env` instead of betraying you because of a leading dot. None of those things are "the pager displays." They're "the pager *is* something the user does." The shape of the surface stayed the same. Its grammar in the user's hands didn't.

The middle is the part where this gets interesting, because the middle is also where the catalogue §4 thread runs through arc 05 without ever quite landing on it.

Arc 02's investigation entry catalogued five lazygit borrow/adapt/skip recommendations and ranked §4 ("Generalized pager picker") as "highest-leverage." The proposal was specific: extend `PagerView` with a `picker_items: Vec<(Label, Action)>` field, wire `Enter` to fire the action under the cursor, let any list-of-options surface (project chooser, worktree picker, branch checkout) be a pager mode rather than a fifth overlay. That field doesn't exist after PR #36. What does exist is two pager-mode extensions in arc 05 (PR #33 visual-line-mode, PR #35 D-launches-pager) and two parallel picker overlays in arc 06 (PR #8 harpoon, PR #10 quickselect, both shipping earlier in the window outside this arc). Four PRs aligned with §4 directionally; zero PRs executing §4's specific shape. Arc 02 named the catalogue framing this way: DIRECTION ALIGNMENT, not direct execution. After arc 05 that framing reads as load-bearing — the catalogue's *direction* (pager as the host of modes; render *into* the pager rather than splintering into overlays) is honored across both arcs, while the specific picker_items pattern remains undone. Whether that means the parallel patterns made the picker pattern unnecessary, or whether the picker pattern is structurally still ahead, isn't narratable from the diffs in arc 05.

PR #20's bundle is the drift moment of the arc. Three concerns under one `feat/` slug — the alt-screen scroll hint, `[pane] default_command`, and `gd`-vs-HEAD — and the cluster the segmentation entry (= 01KR0TWHTC1MPK4KJ08Y9SPE6P) flagged as "drift fuel for the insight layer" because the bundling itself is the artifact, not any individual half. From inside arc 05 a few things become legible that weren't from outside. The alt-screen hint is the catalogue §2 alignment partner — partial execution, the broader options-map idea unfulfilled. The `[pane] default_command` config is the piece PR #23 will surface to the help text 13 hours later (arc 05's second-most-load-bearing implicit chain; PR #23's diff names the precedence chain in the help row exactly to the form PR #20 added). And the `gd`-vs-HEAD shift is the same shape as arc 04's PR #27 GitFileStatus enum-to-struct work — both are "make the existing marker mean what it visibly says it means" moves. None of these halves are wrong; the bundling means a reader scanning commit subjects sees one feature, when the diff is three.

The phase α → phase γ implicit chain inside arc 05 is also worth pausing on, because nothing in the commits names it. PR #11 lands a `last_body_w: std::cell::Cell<u16>` field on `PagerView` to make wrap-aware `scroll_max` work. That field becomes part of the struct's permanent furniture. Four days and four PRs later, PR #33 lands a `visual: Option<VisualSelection>` field on the same struct — sibling state to `last_body_w`, neither disturbing the other. PR #33 doesn't have to refactor `PagerView` to add visual mode; it adds a field next to the field PR #11 added, and the struct expands. This is what *enables* phase γ being cheap. PR #11's mechanics work was small and contained, which is what made the room for visual-mode to land later as additive surgery rather than restructuring. PR #16's seed-from-buffer pattern (render `task.buffer` into the pager and call `scroll_to_bottom_auto()` before handing the buffer off) is a similar shape: small contained correction now, structural enabler later — PR #35's `display_in_pane` reads as a parallel of `edit_in_pane` for the read path, taking a listing row and launching it into a pty in the top overlay, sharing focus with the bottom pane. None of this is "designed up front." It reads as small correct moves making large additive moves cheap.

PR #35 is the move that exposes how much arc 03's ground-clearing was carrying for arc 05. PR #34 (arc 03's last move, two days earlier) taught the overlay-vs-pane focus model — `;cmd` overlays can share focus with the bottom pane, `^a-j`/`^a-k` chord keys bridging the two. Without PR #34, opening `$PAGER` as a top overlay traps focus in the overlay; with PR #34, the same spawn produces the docs-and-claude-side-by-side workflow PR #35's CHANGELOG names. The arc-03 → arc-05 link isn't visible in either commit. It's visible in the fact that PR #35 ships at all without re-doing PR #34's work.

PR #36 is the outlier, and it's worth being honest about. The matcher shift affects the listing — `/` and `=` — not the pager's `n/N`. The segmentation entry on `history-overview` filed PR #36 in arc 05 by read-surface direction; the per-PR entry above was upfront that the fit is curatorial rather than structural. A reader filing PR #36 as arc 04 (git-integration's correctness shape) or as a one-PR side-arc would not be wrong. What PR #36 *does* share with arc 05 is the project-internal classification asymmetry — the diff is genuinely a behavior change, the CHANGELOG places it under `### Changed`, the commit subject says `fix:`, and BUGS.md lifts a SMALL entry to FIXED. PR #35 lifted a BUGS.md SMALL the same way the day before. The lift-from-SMALL-to-FIXED move recurs at the close of arc 05 in two consecutive PRs; it's a local pattern, not a forward prediction.

The eight PRs span five days. The pager surface they shape doesn't quite become what arc 02's catalogue §4 named at the start of the window, but it becomes something — modes-not-just-display, launchable-from-listing, search-that-doesn't-betray. Whether that's "the picker pattern, taking a different route" or "the picker pattern, deferred indefinitely" isn't yet decidable from the data points arc 05 ships. It's the question carried forward into the eventual insight layer.

Provenance:
- No new commit references; this entry reflects on the head entries which carry full SHA provenance.
- `history-arc-05-pager-surface` head entries 0–9 = 01KR29ZCRYY132QKB0HKRRRERQ (framing), 01KR2A121DSV81GM4EBCKAVAAM (PR #11), 01KR2A2XY61GKZ1W52XQWGFBAH (PR #16), 01KR2A4DCY3BR45ZQ7FQ2YQE4Q (PR #17), 01KR2A6TT516XA5FEGVBXYPWD7 (PR #20), 01KR2A8PW1GRF82G4X8R7HFP6H (PR #23), 01KR2AAX12XSNRNZPTXJT2TXJA (PR #33), 01KR2AD5PV989H58E49E5D18NM (PR #35), 01KR2AFHD42DHX6XQS7S6VK4M5 (PR #36), 01KR2AJVZA1E85YSKHF4FNRQQ3 (closure).
- `history-overview` segmentation entry = 01KR0TWHTC1MPK4KJ08Y9SPE6P (PR #20 three-concern bundle drift flag; PR #36 fix-vs-behavior-change drift flag).
- `history-arc-02-lazygit-investigation-and-harvest` investigation entry = 01KR0YXXZRQR24CSNAK4Q7808T (catalogue §2 + §4 source; §4 ranking; PARTIAL EXECUTION + DIRECTION ALIGNMENT framings).
- `history-arc-03-pane-behavior` PR #34 entry = 01KR10JBACRS3Z71WTHGBVCPJM (overlay-as-pane focus model; PR #35's prerequisite).
- `history-arc-03-pane-behavior` story-tail = 01KR11S8RG29J98QKN1H0VAA6W (recurring-concern shape; this tail's contrasting capability-accretion shape).
- `history-arc-04-git-integration` story-tail = 01KR13CJ5XS5VREYA4741JHDSQ (capability-accretion register precedent).
- `history-arc-04-git-integration` PR #27 entry = 01KR134PZSQDAFVJK3M35FTKXF (`GitFileStatus` enum-to-struct; same shape as PR #20's `gd`-vs-HEAD marker-correction move in a different domain).

<!-- Entry-ID: 01KR2ANRAEFWWR5W9FQP11A0DB -->

---
Entry: Claude Code (caleb) 2026-05-07T23:00:17.182420+00:00
Role: scribe
Type: Note
Title: Two seams the story-tail walks past: PR #35's launching mechanism, and PR #36's matcher escape hatch

Spec: scribe

tags: #history #arc-05 #tail

Two seams worth pulling out separate from the story-tail above, because they're the kind of thing a reader hitting the head entries can verify in five minutes and easily slide past in the broader narrative.

PR #35's `display_in_pane` is structurally a *launching pattern*, not a feature. Take the cursor row, validate (kind, env), build a command via `shell::resolve_pager` + `shell::shell_quote`, size the overlay via `top_overlay_size` + `effective_pane_pct`, capture cwd + context_path, and `Pane::spawn` into `top_overlay`. The doc-comment names the mirror relationship to `edit_in_pane` ("Mirror of `edit_in_pane` for the read path"); both methods are private to `App`, both inhabit the same shape, and neither is factored as a standalone helper. The catalogue §4 picker pattern arc 02 named — `pick a row, fire an Action against it` — could ride this exact dance with a different `resolve_*` step at the front. The launching machinery exists. What doesn't exist is the picker-cursor-into-launcher dispatch that connects a pager-mode pick to the `Pane::spawn` step. A future feature wanting "pick a worktree, launch into `$PAGER` showing its log" or "pick a project, launch into `$EDITOR` opening its README" would have to either re-implement this dance or refactor the two private mirror-methods into a common helper. The seam is not whether the mechanism is reusable — it visibly is. It's where the connection point lives, and right now it lives in nobody's accessor.

PR #36's `env*` escape hatch reads as a small forward-pointing observation about reversibility. The doc-comment names it: "Globs are still available for users who want anchoring (`env*`, `.env*`)." At the user level, the prior anchored behavior is preserved — type `env*` to re-anchor, type `*env*` to make the substring explicit. At the *code* level the substring shift is one-way: `Matcher::Prefix` no longer exists; the enum variant is `Matcher::Substring`, and `matches()` calls `contains()` rather than `starts_with()`. A future feature that needs anchored matching at code-internal granularity (an LSP integration that has to resolve a symbol strictly against a name, an importer that wants prefix-matched namespaces, a watcher whose path-filter has to anchor at the start) cannot reach for `Matcher::Prefix` — that path is gone. The escape hatch protects user-facing surfaces; it does not protect program-internal call sites. Whatever wants strict prefix at the code level has to either route through `Matcher::Glob` with an explicit `*` suffix, or build a separate matcher entirely. Knowing which one the next feature will reach for is the seam.

Both observations are forward-pointing in the way the story-tail isn't, but neither is a prediction. They're notes about where the next bug-or-design-question is most likely to land, given what the diffs in arc 05 actually shaped.

Provenance:
- No new commit references; this entry reflects on the head entries which carry full SHA provenance.
- `history-arc-05-pager-surface` PR #35 entry = 01KR2AD5PV989H58E49E5D18NM (`display_in_pane` body quoted in full; `Mirror of edit_in_pane` doc-comment).
- `history-arc-05-pager-surface` PR #36 entry = 01KR2AFHD42DHX6XQS7S6VK4M5 (`Matcher::Prefix` → `Matcher::Substring` rewrite; "Globs are still available for users who want anchoring" doc-comment).
- `history-arc-05-pager-surface` story-tail above = 01KR2ANRAEFWWR5W9FQP11A0DB.
- `history-arc-02-lazygit-investigation-and-harvest` investigation entry = 01KR0YXXZRQR24CSNAK4Q7808T (catalogue §4 picker pattern; `pager.picker_cursor` proposed shape that the launching mechanism could host).
- `history-arc-03-pane-behavior` seams-aside = 01KR11TME2KF5QFQ45GJYG8MC7 (precedent for seams-aside register; same forward-pointing-not-predicting voice).

<!-- Entry-ID: 01KR2AQ5M13KAR1M7A4561B5GM -->
