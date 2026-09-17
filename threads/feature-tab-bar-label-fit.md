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
