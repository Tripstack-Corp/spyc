# history-arc-03-pane-behavior — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: history-arc-03-pane-behavior
Created: 2026-05-07T10:37:15.658499+00:00

---
Entry: Claude Code (caleb) 2026-05-07T10:37:15.658499+00:00
Role: scribe
Type: Note
Title: Framing: arc 03 — recurring-concern shape, per-PR variety, cadence option A

Spec: scribe

tags: #history #arc-03

Arc title: `pane-behavior`. Date span: 2026-05-01 (PR #6) to 2026-05-06 (PR #34); five PRs, six calendar days. Member PRs:

- 10c9276 (PR #6 feat/pane-zoom, 2026-05-01) — "pane: ^a z fullscreen-toggle (zoom) for the bottom pane (v1.38.0)" (commit 10c9276, 2026-05-01).
- a3338fa (PR #22 feat/pane-shutdown-cleanup, 2026-05-05) — "fix: clean shutdown of pane child trees on tab close + spyc quit (v1.41.9)" (commit a3338fa, 2026-05-05).
- 20fba00 (PR #26 feat/dim-unfocused-pane, 2026-05-06) — "feat: dim unfocused side so focus is obvious at a glance (v1.41.13)" (commit 20fba00, 2026-05-06).
- bdb8d87 (PR #29 fix/skip-pane-cursor-block-when-uninvited, 2026-05-06) — "fix: skip pane cursor block for unfocused / alt-screen panes (v1.41.16)" (commit bdb8d87, 2026-05-06).
- 8e9fb2c (PR #34 fix/top-overlay-focus-switch, 2026-05-06) — "fix: ;cmd overlay shares focus with bottom pane (v1.41.21)" (commit 8e9fb2c, 2026-05-06).

**Diagnosis: pattern 10 register, no pivot, with per-entry shape variety.** Arc 03 reads as a recurring-concern arc — the same surface (the bottom pane: its visual state, its child-process lifetime, its focus routing, its rendering correctness) returns five times across six calendar days as new conditions surface new corners. It is not a single feature build (arcs 03/04 line items aren't one new capability), not an incident (no failure cascade), not an investigation (no `notes/` deliverable, no gap analysis). The closest match to the voice catalogue is **pattern 10's hub-and-pivot shape** — a long-running thread where the same surface is re-examined under new constraints — *minus the explicit pivot*. The five PRs do not divide neatly into "phase A / phase B"; they accrete.

The framing register stays PM-flavored (pattern 10's natural voice) without leaning heavily into it. Each per-PR head entry below picks its own internal shape, driven by what the diff actually is rather than a uniform house style:

- **PR #6 (zoom)**: feature-shaped, compact. A new capability lands; the entry is short.
- **PR #22 (shutdown-cleanup)**: **operational-sweep flavor** (pattern 2). The diff *is* a procedural sequence — SIGTERM the process group, wait 250 ms, escalate to SIGKILL, reap — and the entry honors that with a numbered-stage register.
- **PR #26 (dim-unfocused)**: brief feature note. The diff is short (+47/-5 across two widget files); the entry is too.
- **PR #29 (cursor-block generalization)**: **plan-supersession-ladder shape** (pattern 1). The entry is structurally the supersession of PR #5's narrow `screen.hide_cursor()` guard — and as the same calendar day brings PR #26 first and PR #29 hours later, also the supersession of one branch of PR #26's cursor-block treatment. Two supersessions to make visible at once; the ladder shape is the one that lets both land cleanly.
- **PR #34 (overlay focus switch)**: focus-routing-shaped, compact. The diff is small but cuts across `pane_focused` state, overlay rendering, and chord-resolver fall-through; the entry walks those three threads in order.

The brief named this hybrid as the natural fit and the diagnosis above commits to it. Mixing internal forms within one arc is not aesthetic indulgence — it's letting the entry-shape track the entry-material, which is the voice contract's actual instruction once "house style" is taken off the table.

**Cadence choice: option A — five per-PR entries plus framing and closure (seven head entries).** Five PRs is the natural per-PR shape; the segmentation entry on `history-overview` (= 01KR0TWHTC1MPK4KJ08Y9SPE6P) lists five distinct concerns with no obvious phase boundary, and the per-PR cadence preserves the order in which the five corners surfaced. Option B (phase-not-PR) does not fit — pane-behavior does not decompose into "investigation phase / harvest phase" the way arc 02 did. Option C (consolidated) loses the per-PR provenance grain that PR #29's mandatory back-reference contract relies on. Arc 01's option A precedent inherits.

**Mandatory back-reference contract for PR #29 (per `history-overview` entry 2 = 01KR0TYF5F11DA8P5HNPA20DBK).** PR #29's per-PR entry below cites:

- Arc 02's investigation entry (= 01KR0YXXZRQR24CSNAK4Q7808T) for gap-analysis "Top suspects" §1: spyc's unconditional reverse-video cursor block, which PR #5 partially fixed with a narrow `screen.hide_cursor()` guard. PR #29 generalizes that guard.
- Arc 02's harvest entry (= 01KR0Z11CKNJRYEZ3T38EAFSC4) for the BUGS.md SMALL cursor-block-reverse-video item PR #12 lifted out of the gap analysis. The behavioral relationship between PR #29's three-condition guard and PR #12's residual text is the second axis of the back-reference; the per-PR entry resolves what gets extinguished where.

**The recurring-concern observation, named factually only.** The pane-behavior surface returns five times in six days. Visual state surfaces in PRs #6 (zoom) and #26 (dim-unfocused). Child-process lifetime surfaces in PR #22 (shutdown). Rendering correctness surfaces in PR #29 (cursor-block guard). Focus routing surfaces in PR #34 (overlay-focus switch). Whether the recurrence reads as an emergent property, a known-surface-being-iterated, or just five PRs that happened to land on adjacent calendar days is for the insight layer to interpret. Arc 03 records the sequence without claiming a pattern.

**Cross-thread back-link.** This thread continues from `history-overview` and the prior arc threads:

- `history-overview` framing (entry 0) = 01KR0TRFWT9W6WMFHC49YSW0BG.
- `history-overview` segmentation (entry 1) = 01KR0TWHTC1MPK4KJ08Y9SPE6P (arc 03's three-paragraph rationale at "Arc 03 — `pane-behavior`").
- `history-overview` PR #5 special-handling (entry 2) = 01KR0TYF5F11DA8P5HNPA20DBK (back-reference contract).
- `history-arc-01-foundation-hygiene` framing (entry 0) = 01KR0W6FR7T01ZJR84MRKWA13A (cadence option A precedent).
- `history-arc-02-lazygit-investigation-and-harvest` investigation entry (entry 1) = 01KR0YXXZRQR24CSNAK4Q7808T (gap-analysis §1 hub).
- `history-arc-02-lazygit-investigation-and-harvest` harvest entry (entry 2) = 01KR0Z11CKNJRYEZ3T38EAFSC4 (BUGS.md residual hub).
- `history-arc-02-lazygit-investigation-and-harvest` closure (entry 3) = 01KR0Z3673Z27FJ4GV92FYV4QJ (forward-references arc 03).

The per-PR entries that follow this framing narrate PRs #6, #22, #26, #29, #34 in arc order. The closure entry forward-references arc 04 (`git-integration`). This thread remains OPEN for cross-arc references — arc 04 may back-reference PR #22 if any pane-related git-marker work touches `pane/mod.rs`; insight-layer threads will likely cite arc 03 entries for the recurrence reading.

Provenance:
- 10c9276 (PR #6 feat/pane-zoom, 2026-05-01) — `git show --stat 10c9276`: 12 files changed, +157/-23.
- a3338fa (PR #22 feat/pane-shutdown-cleanup, 2026-05-05) — `git show --stat a3338fa`: 7 files changed, +141/-2.
- 20fba00 (PR #26 feat/dim-unfocused-pane, 2026-05-06) — `git show --stat 20fba00`: 7 files changed, +47/-5.
- bdb8d87 (PR #29 fix/skip-pane-cursor-block-when-uninvited, 2026-05-06) — `git show --stat bdb8d87`: 6 files changed, +49/-13.
- 8e9fb2c (PR #34 fix/top-overlay-focus-switch, 2026-05-06) — `git show --stat 8e9fb2c`: 6 files changed, +79/-16.
- `history-overview` segmentation entry = 01KR0TWHTC1MPK4KJ08Y9SPE6P (arc-03 rationale at "Arc 03 — `pane-behavior`" paragraph; cluster-boundary call on PR #15 named in the same entry).
- `history-overview` PR #5 special-handling entry = 01KR0TYF5F11DA8P5HNPA20DBK (PR #29 back-reference contract; "arc 03 entry on PR #29 must back-reference arc 02").
- `history-arc-02-lazygit-investigation-and-harvest` investigation entry = 01KR0YXXZRQR24CSNAK4Q7808T (gap-analysis "Top suspects" §1; PR #29 supersedes PR #5's narrow guard).
- `history-arc-02-lazygit-investigation-and-harvest` harvest entry = 01KR0Z11CKNJRYEZ3T38EAFSC4 (BUGS.md SMALL cursor-block-reverse-video item; PR #29 behaviorally extinguishes the case).
- `onboarding-architecture` entry 0 = 01KR0P4W3ED1QZ8F44PFB2WPDZ (current-state pane PTY ownership; the surface arc 03's PRs touch).

<!-- Entry-ID: 01KR106N6HSW66R76HN9VJPF1Q -->
