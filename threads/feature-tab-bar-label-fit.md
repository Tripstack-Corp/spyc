# feature-tab-bar-label-fit

Topic: feature-tab-bar-label-fit
Status: OPEN
Ball: Claude Code

---
Entry: Claude Code 2026-09-17T07:26:44Z
Index: 0
Role: planner
Type: Note
Title: Plan: seam-aware, distinctness-preserving tab label croppin…

Spec: planner-architecture

## Context

PR #482 (`feat/pane-startup-tabs-gh`, Tripstack-Corp/spyc) adds startup tabs from `.spycrc.toml` and, in commit `a1516c7`, a tab bar that fits every tab instead of dropping overflow. The fit lives in `src/app/mouse/tab_hit.rs::fit_tabs` and is the single source of truth for both the renderer (`render/chrome.rs`) and the mouse hit-test.

Today's stage two is structure-blind: it pops one character off whichever label is currently longest. Caleb's screenshot of a nine-tab bar shows the result: `coordin`, `discipl`, `waterco`, `topo-oc`, `codex-`. Cuts land mid-word, one ends on a hyphen, and nothing tells the reader the name was cut. With `watercooler`, `watercooler-cloud` and `watercooler-dashboard` open together, prefix cropping would yield three identical `waterco` labels, which defeats the label's only job: telling tabs apart.

Explicit per-tab names are already in this PR (`[[pane.tab]] label = "..."`), so no config work is needed; the heuristic only has to do well for unnamed tabs.

## Goal

Cropped labels should (a) cut at natural seams, (b) stay distinct from each other, (c) visibly signal that they were cropped, and (d) spare the active tab until the others are exhausted. All of it inside the one pure function, unit-tested, with the renderer untouched.

## Design

Each label is modelled as a mutable fit record: the original text, `head_elided`, the current core text, `tail_cut`. Painted text = `…` if head elided, plus the core, plus `…` if tail cut, plus the surviving padding spaces. Width is always measured from the painted text through `ui::display_width`, so the renderer's existing debug assertion keeps holding.

**Seams.** A cut is allowed before a separator character (`-`, `_`, `.`, `/`, space) that has non-separator text on both sides, and at a lowercase-to-uppercase transition (`TradingAgents` → `Trading`). Runs of separators count as one seam.

**Reclaim order** (unchanged stage one, new stage two):

1. Padding spaces, trailing before leading, rightmost tab first. As today.
2. While the bar still overflows, pick the widest label in the eligible pool (ties go to the rightmost, as today) and apply the first op that applies and nets at least one column:
   - **Tail-segment drop**: cut at the last seam, mark `tail_cut`. `topo-oceans` → `topo…`. Refused when the resulting core equals another tab's current core, case-insensitively (`watercooler-cloud` → `watercooler` collides with the `watercooler` tab).
   - **Head-segment drop**: only when the label's first segment equals another tab's first segment. Cut after the first seam, mark `head_elided`. `watercooler-cloud` → `…cloud`. This is what keeps siblings distinct: the shared head carried no information.
   - **Character shave**: pop characters until the painted width falls by at least one (the first shave pays for the `…`), mark `tail_cut`. `codex` → `code…`.
   - **Floor**: a label already at three painted columns or fewer (`co…`) is cleared to empty instead of shaved further. The `[N]` bracket still identifies it, and an empty label reads as deliberate where `c…` reads as broken.
3. **Active tab last.** The active tab stays out of the pool while any other tab still has a non-empty label. It joins only when every other label is gone. In practice moderate pressure never touches it, and extreme pressure yields `[1][2]…[8][9] bash`, which is the right shape.

Distinctness is checked against the *current* core of every other tab, so an already-cropped neighbour counts. Labels are compared case-insensitively because scroll mode uppercases the active label.

Termination: every op nets at least one column or is inapplicable, and the floor clears a label outright, so the loop reaches the fixed-chrome floor exactly as today. `tab_spans` and the renderer's overflow break remain the backstop below that floor.

## Where it lands

- `src/app/mouse/tab_hit.rs`: `fit_tabs` grows a seam finder and the op ladder; the `tab_layout` doc comment is rewritten to state the new order. Still pure, still the one function the renderer and hit-test share.
- `src/app/render/chrome.rs`: no change. It already paints `label_text` from the layout.
- Tests in `tab_hit.rs`: update `fit_shaves_longest_label_first` to the new expected forms; add tests for seam cuts (`topo-oceans` → `topo…`), camelCase seams, the collision refusal plus head-drop (`watercooler` / `watercooler-cloud` / `watercooler-dashboard` stay distinct), the ellipsis on a character shave, the three-column floor, active-tab exemption, and a sweep over bar widths asserting no overflow and no two non-empty labels equal. Each new test is broken once against the old algorithm to confirm it reports.
- `FEATURES.md`: one bullet under the pane section describing the fit order (the PR's fit commit did not document itself; this covers both).
- `docs/KEYBINDINGS.md`, `DESIGN.md`, `ARCHITECTURE.md`: untouched. No binding, no UI-language change, no architectural decision.

## Out of scope

- A short-name config field: `[[pane.tab]] label` already exists in this PR.
- Abbreviation heuristics (vowel stripping): less readable than a seam cut, not worth the surprise.
- Changing the fixed chrome (`─[N]` + status cell).

## Delivery

Committed to `feat/pane-startup-tabs-gh` as one conventional commit, pushed to the PR for Derek's review. Merge is Derek's call, not ours.

<!-- Entry-ID: 01M2Q45F7SM4P1VFMKQ499TS70 -->

---
Entry: Claude Code 2026-09-17T07:33:30Z
Index: 1
Role: implementer
Type: Note
Title: Implemented as planned: commit 01618ef on PR #482

Spec: implementer-code

## Landed

Commit `01618ef` on `feat/pane-startup-tabs-gh` (Tripstack-Corp/spyc PR #482), pushed. Implements the plan in entry 0 without deviation.

- `src/app/mouse/tab_hit.rs`: `fit_tabs` now takes the active index; stage two is the op ladder (tail-segment drop with collision refusal → head-segment drop on a shared first segment → character shave with `…` → floor clear at three columns), applied to the widest eligible label, active tab exempt while any other label is non-empty. Seams: separator runs (`- _ . / space`) with text on both sides, plus lowercase→uppercase transitions. `Fit` records carry `original` (for the first-segment comparison, which cropping must not change), `core`, `head_elided`, `tail_cut`. The doc comment on `tab_layout` states the order.
- `src/app/render/chrome.rs`: untouched, as planned.
- `FEATURES.md`: one bullet under Multi-tab describing the fit order and pointing at `^a r` / `[[pane.tab]] label` for an explicit name.

## Evidence

- 20 tests in `tab_hit`, 8 of them new or changed. Each new mechanism was disabled in turn (seams empty, ellipsis blank, floor zero, active exemption off) and the matching tests failed; restored, all green.
- Hand-computed expectations matched on first run: `topo-oceans` → `topo…`; `TradingAgents` → `Trading…`; `watercooler` / `watercooler-cloud` / `watercooler-dashboard` at 41 columns → `watercool…` / `…cloud` / `…dashboard`; `bash` at the floor → `ba…`, one column less → empty; active `coordinator` intact at moderate pressure, `co…` only after the other two are empty.
- Sweep over every bar width from the fixed-chrome floor to 220 with twelve realistic labels: no overflow, no two labels above the floor equal.
- `make check-ci`: fmt and clippy clean; the only test failures are the two machine-local ones already known on this box (the prose guard tripping on an untracked `.serena/` memory file, and the tracked-`.mcp.json` cleanup test blocked by this machine's global git ignore). CI on the previous head was green; the run for `01618ef` is in flight.

## Known limit, documented in the sweep test

A pure character shave of two names that differ only past the cut (`claude1` / `claude2`) can still collide. Segment cuts are collision-checked; character shaves are not, because shaving further would collide just the same. The `[N]` bracket is the backstop there.

Merge is Derek's; nothing further pending from this side.

<!-- Entry-ID: 01M2Q4HW6GW2BK5579BMXZ0Z1N -->

---
Entry: Claude Code 2026-09-17T08:11:26Z
Index: 2
Role: implementer
Type: Note
Title: Reverted: seam-aware cropping withdrawn from PR #482 at Cal…

Spec: implementer-code

Caleb reviewed the landed change and called it off: "That last segmentation bloated for an unneeded feature. We can control the names in the toml file and save the code."

Commit `01618ef` is reverted by `1c21c8d` on `feat/pane-startup-tabs-gh`; the tree is byte-identical to `5b6c980` (verified with `git diff --quiet`). PR #482 keeps the simpler fit from `a1516c7` (padding spaces, then one character off the widest label). Explicit tab names via `[[pane.tab]] label` in `.spycrc.toml` and `^a r` are the sanctioned way to get readable short labels.

Entries 0 and 1 stand as the record of the design and its evaluation, should the question come back. Nothing further planned here.

<!-- Entry-ID: 01M2Q6Q9Y1H3J8A4NDBGDENAE3 -->
