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

---
Entry: Claude Code (caleb) 2026-05-07T10:38:23.836828+00:00
Role: scribe
Type: Note
Title: PR #6 (feat/pane-zoom): tmux-style zoom, [ZOOM] tag in divider, v1.38.0 minor cut

Spec: scribe

tags: #history #arc-03

PR #6 is the first move in arc 03 and the first time the bottom pane gets a new visual-state mode. Commit subject reads "pane: ^a z fullscreen-toggle (zoom) for the bottom pane (v1.38.0)" (commit 10c9276, 2026-05-01). Diff: 12 files, +157/-23. Cargo.toml bumps `version = "1.37.3"` to `version = "1.38.0"` — a minor bump, the only minor bump in arc 03; the other four PRs all ride on patch bumps within the v1.41.x range.

**The capability.** `^a z` (and the screen-style alias `^w z`) toggles a tmux-style zoom on the bottom pane: the file list collapses to 0 rows and the pane fills the middle region between the status bar and the prompt. The CHANGELOG entry frames the design choice verbatim: "Tmux-style — the status bar and prompt row stay visible, focus is forced into the pane on zoom-on and the prior focus is restored on un-zoom. The user's preferred `pane_height_pct` is preserved untouched so the prior split returns exactly on un-zoom. A `[ZOOM]` tag renders in the divider while active. `^a +` / `^a -` are no-ops while zoomed (with a status flash). Closing the pane (`^a \\`) clears the zoom flag. Requested by a daily user."

**The state shape.** Two new fields land on `AppState` (`src/app/state.rs:117-127` post-merge): `pane_zoomed: bool` and `pane_focus_before_zoom: Option<bool>`. The doc-comments name the contract: zoom preserves the user's `pane_height_pct` untouched so un-zoom restores the prior split exactly; the `Option<bool>` captures focus state at zoom-on and restores it at zoom-off. Layout, sizing, and pane-spawn computations gate through a new `App::effective_pane_pct(&self) -> u16` helper that returns 100 when zoomed and `pane_height_pct` otherwise — six call sites in `src/app/mod.rs` adopt the helper instead of reading `pane_height_pct` directly, so zoom-vs-split renders correctly without per-call-site branching.

**Resize-on-toggle.** `App::toggle_pane_zoom` ends with a deliberate dimension-rebroadcast loop: read `crossterm::terminal::size()`, compute the new layout against `effective_pane_pct()`, and call `entry.pane.resize(pane_rect.height, pane_rect.width)` for every tab. The inline comment names the consequence verbatim: "Resize all pty children to the new pane rect so their child shells re-render at the right dimensions; otherwise Claude's UI is the wrong size until the next terminal resize." The "Claude" reference in the policy comment locates this PR's primary consumer of the bottom pane, which is consistent with the README and FEATURES.md framing of the pane as Claude's home.

**The keymap and divider tag.** `Action::TogglePaneZoom` lands on the action enum (`src/keymap/action.rs:104-216`); `KeyCode::Char('z' | 'Z')` after `^w` resolves to it (`src/keymap/resolver.rs:171-172`). The divider tag-rendering block in the status row (`src/app/mod.rs:1948-2015`) refactors from a single `tag` slot into two: `zoom_tag = " [ZOOM]"` painted in the prompt-prefix theme color with `Modifier::BOLD`, and the existing `scroll_tag = " [SCROLL]"` painted in the pick color. Both tags can be live simultaneously (the budget reservation accounts for both lengths), so a zoomed pane in scroll mode shows ` [ZOOM] [SCROLL]`. The two-tag-coexistence reading is the diff's, not an inferred constraint.

**Drift findings flagged for the insight layer.**

- The PR title prefix is `feat/` and the commit subject opens with `pane:`. Both align with the diff (a feature add). No commit-vs-diff drift here; flagged for the insight layer's positive-control row of the eventual drift catalogue.
- `pane_focus_before_zoom: Option<bool>` is the first save-and-restore mechanism for `pane_focused` state; PR #34's overlay focus-switch later in this arc reuses the same `pane_focused: bool` slot for a different save-and-restore axis (overlay vs. bottom pane). Whether the two state-machines coexist cleanly in the post-PR-#34 surface is for arc 03's closure entry to summarize and for the insight layer to interpret.
- `Cargo.toml` bumps to v1.38.0 — a minor bump, consistent with the new-capability framing. The remaining four arc-03 PRs ride on patch bumps within v1.41.x; the version cadence reads in two phases (one minor cut at PR #6, then four patch cuts at PRs #22/#26/#29/#34), but the cadence is not arc 03's load-bearing fact and is named here only as a recurrence-friendly observation.
- The CHANGELOG's "Requested by a daily user" sentence is verbatim user-attribution external signal. Arc 03 quotes it without attributing motive: the commit message attests to the request; nothing more is narratable from the diff.

Provenance:
- 10c9276 (PR #6 feat/pane-zoom, 2026-05-01) — full PR.
- 0691666 → 329222b — parent and tip SHAs for the diff inspection.
- `git diff 0691666..329222b -- Cargo.toml`: `version = "1.37.3"` → `version = "1.38.0"`.
- `git diff 0691666..329222b -- src/app/state.rs`: `pane_zoomed: bool` and `pane_focus_before_zoom: Option<bool>` added at `AppState` lines 117-127 post-merge; same pair added to the test-default `AppState` constructor at lines 1388-1392 post-merge.
- `git diff 0691666..329222b -- src/app/mod.rs`: `effective_pane_pct(&self) -> u16` helper added; six call sites adopted; `toggle_pane_zoom` body with the focus save/restore + pty resize loop; divider tag-rendering block split into `zoom_tag` + `scroll_tag` with bold + pick coloring.
- `git diff 0691666..329222b -- src/keymap/action.rs`: `Action::TogglePaneZoom` enum variant + display-string.
- `git diff 0691666..329222b -- src/keymap/resolver.rs`: `KeyCode::Char('z' | 'Z')` mapped after `^w`; new `ctrl_w_z_zooms_pane` test added; the previous `ctrl_w_unknown_is_ignored` test's probe key changed from `'z'` to `'q'`.
- `git diff 0691666..329222b -- CHANGELOG.md`: 12 lines added; new "Pane zoom (fullscreen toggle)" entry under `[Unreleased]` `### Added`.
- `git diff 0691666..329222b -- FEATURES.md`: 4 lines added; `^a z` documented under the pane-keys table.
- `git diff 0691666..329222b -- README.md`: 1 line added; `^a z` row in the keys table.
- `git diff 0691666..329222b -- BUGS.md`: 9 lines added at the SMALL section head (gum picker, wezterm-picker idea, scroll-mode top/bottom marker, pane focused ^c forwarding); not the same items as the BUGS.md additions in PR #12. PR #6's BUGS.md additions are unrelated to the zoom feature in the same diff.
- `history-arc-03-pane-behavior` framing entry = 01KR106N6HSW66R76HN9VJPF1Q.
- `onboarding-architecture` entry 0 = 01KR0P4W3ED1QZ8F44PFB2WPDZ (pane PTY ownership surface; this PR adds the first save-and-restore mechanism on `pane_focused`).

<!-- Entry-ID: 01KR108QNEEG64J8W8XJERJTZG -->

---
Entry: Claude Code (caleb) 2026-05-07T10:39:31.716553+00:00
Role: scribe
Type: Note
Title: PR #22 (feat/pane-shutdown-cleanup): SIGTERM → 250ms grace → SIGKILL → reap, with a Drop safety net

Spec: scribe

tags: #history #arc-03

PR #22 is the second move in arc 03 and the first time arc 03's recurring concern shifts away from visual state to child-process lifetime. Commit subject reads "fix: clean shutdown of pane child trees on tab close + spyc quit (v1.41.9)" (commit a3338fa, 2026-05-05). Diff: 7 files, +141/-2. The diff is procedurally shaped — a four-stage shutdown sequence, plus a backstop — and the entry below tracks those stages in order.

**The bug being fixed.** The CHANGELOG entry's lede reads verbatim: "`^a x` / `^a K` (close tab) and `Q` / `:q` / `^D` (quit spyc) used to drop the pane without signalling its child, leaving processes orphaned — most painfully `npm run dev` / `vite` / etc., where the whole `node` → `esbuild` → workers tree kept running and stayed bound to its dev-server port." The mechanism named in the same entry: `portable_pty::Child`'s default Drop is a no-op, so dropping a `Pane` without an explicit kill leaves the kernel-side process alive.

**S1 — SIGTERM the process group.** `Pane::shutdown(grace: Duration)` lands on `src/pane/mod.rs` (`+90` lines). The first stage reads `pid = self.child.process_id()`, then under `#[cfg(unix)]` calls `libc::kill(-(pid as libc::pid_t), libc::SIGTERM)`. The negative-PID call is the load-bearing detail: it sends SIGTERM to the entire process group, not just the immediate child. The doc-comment names why verbatim: "(negative PID — reaches every grandchild, which is the actual user-reported scenario: `npm run dev` → node → esbuild → workers all need to die when the tab closes)." The doc also names the assumption that makes the negative-PID semantics correct: "portable-pty calls `setsid` for spawned children on Unix, so the child's PID is also its process-group leader — sending to `-pid` reaches the whole tree."

**S2 — Poll for voluntary exit, capped at `grace`.** A `deadline = Instant::now() + grace` loop calls `child.try_wait()` every 20 ms; a non-`None` exit status short-circuits to "reap and return successful." The 250 ms grace is set at both call sites (the close-tab path and the quit path) — the comment-side rationale at the close-tab call site (`src/pane/tabs.rs::PaneTabs::remove_at`) reads: "going through `shutdown` here gives well-behaved children a chance to flush their own state first."

**S3 — Escalate to SIGKILL on the process group.** Grace expired, the same negative-PID `libc::kill` call fires with `SIGKILL`. The doc-comment names the safety property verbatim: "SIGKILL is uncatchable so a final blocking `wait()` is safe (it won't hang)."

**S4 — Reap.** `self.child.wait()` blocks until the kernel reports the exit; `self.exit_status = Some(status)`; `self.closed = true`. No zombies left behind.

**The Drop backstop.** A new `impl Drop for Pane` block lands at `src/pane/mod.rs::487-510` (post-merge) as the panic-and-error-propagation safety net. The doc-comment names the trade-off explicitly: "We can't sleep here without making Drop slow, so this skips the SIGTERM grace period and goes straight to SIGKILL on the process group. The orderly close-tab and quit paths call `shutdown` explicitly first, so this rarely fires for a 'well-behaved' exit." Drop is a hard SIGKILL by design; the orderly path is the soft SIGTERM-then-SIGKILL.

**The two call sites.** `PaneTabs::remove_at(idx)` (`src/pane/tabs.rs::244-264` post-merge) wraps the `tabs.remove(idx)` line with a preceding `self.tabs[idx].pane.shutdown(Duration::from_millis(250))` call; the function's doc-comment names the close-tab path explicitly. `App::run`'s clean-exit tail (`src/app/mod.rs::1710-1722` post-merge) iterates every active tab and calls `entry.pane.shutdown(Duration::from_millis(250))`; the inline comment names the quit-path consequence verbatim: "quitting spyc with a frontend dev server in a pane would leave the whole node/esbuild/worker tree orphaned and still bound to its port" — the bug the PR fixes.

**The early-return short-circuit.** If `self.closed` is already true (the reader thread saw EOF), `Pane::shutdown` skips signal delivery and merely harvests `exit_status` from a non-blocking `try_wait`. This handles the "child already exited; we're just here for cleanup" case without raising the kill-signal-on-already-dead-pid issue.

**Drift findings flagged for the insight layer.**

- The branch is `feat/pane-shutdown-cleanup` (feature prefix), but the commit subject reads "fix:" — and BUGS.md `### FIXED ###` records the entry as `(fixed, v1.41.9)`. The diff is unambiguously a fix (orphaned children was a bug, not a missing feature). Title-prefix-vs-commit-subject drift; the commit subject is correct against the diff. Captured for the eventual drift catalogue.
- The shutdown machinery does not surface to the user — no flash, no log, no escalation-occurred indicator. A user whose `npm run dev` ignores SIGTERM (rare but possible — uncatchable means SIGKILL works regardless) gets a 250 ms latency on tab-close that they cannot distinguish from a fast voluntary exit. Whether this no-feedback design is correct or load-bearing for a future "process tree didn't shut down cleanly" warning is for the insight layer to interpret.
- The doc-comment leans on `portable_pty` calling `setsid` for the negative-PID semantics. If a future portable_pty version changes that behavior — or if a custom spawn path bypasses portable_pty — the negative-PID kill silently widens or narrows. Captured for the insight layer's "load-bearing-on-an-upstream-invariant" row.
- This PR's policy comment cites "the actual user-reported scenario" without naming a BUGS.md or thread anchor for the report itself; the report's text appears only in the CHANGELOG and BUGS.md `### FIXED ###` entries this PR adds, both written from after-fix vantage. The pre-fix bug-report text is not durable in the repo.

Provenance:
- a3338fa (PR #22 feat/pane-shutdown-cleanup, 2026-05-05) — full PR.
- 193f7ad → 2021de0 — parent and tip SHAs for the diff inspection.
- `git diff 193f7ad..2021de0 -- src/pane/mod.rs`: +90 lines; `Pane::shutdown(grace: Duration)` body at lines 256-323 post-merge; `impl Drop for Pane` at lines 487-510 post-merge.
- `git diff 193f7ad..2021de0 -- src/pane/tabs.rs`: +12 lines; `PaneTabs::remove_at` at lines 244-264 post-merge; doc-comment naming the close-tab path verbatim.
- `git diff 193f7ad..2021de0 -- src/app/mod.rs`: +13 lines; quit-path shutdown loop at lines 1710-1722 post-merge.
- `git diff 193f7ad..2021de0 -- CHANGELOG.md`: 15 lines added under `[Unreleased]` `### Fixed`; the "leaving processes orphaned" lede quoted verbatim above.
- `git diff 193f7ad..2021de0 -- BUGS.md`: 9 lines added to `### FIXED ###` recording the v1.41.9 fix; SMALL section unchanged by this PR.
- `git diff 193f7ad..2021de0 -- Cargo.toml`: `version = "1.41.8"` → `version = "1.41.9"`.
- `history-arc-03-pane-behavior` framing entry = 01KR106N6HSW66R76HN9VJPF1Q.
- `history-arc-03-pane-behavior` PR #6 entry = 01KR108QNEEG64J8W8XJERJTZG.
- `onboarding-architecture` entry 0 = 01KR0P4W3ED1QZ8F44PFB2WPDZ — current-state pane PTY ownership; this PR is the genesis of `Pane::shutdown` and `impl Drop for Pane`.

<!-- Entry-ID: 01KR10ASW7YSX4MB8G28X2C9N4 -->

---
Entry: Claude Code (caleb) 2026-05-07T10:40:27.801752+00:00
Role: scribe
Type: Note
Title: PR #26 (feat/dim-unfocused-pane): SGR 2 on the unfocused side, BUGS.md SMALL extinguished

Spec: scribe

tags: #history #arc-03

PR #26 is the third move in arc 03 and the second visual-state move (PR #6 zoom was the first). Commit subject reads "feat: dim unfocused side so focus is obvious at a glance (v1.41.13)" (commit 20fba00, 2026-05-06). Diff: 7 files, +47/-5. Two source files carry the work: `src/pane/widget.rs` (+13/-1) and `src/ui/list_view.rs` (+13/-1). The diff is short; the entry is too.

**The behavior.** Each side renders a per-cell `Modifier::DIM` (SGR 2) when it is not the input target. When the pane has focus, the file list above renders DIM on every non-cursor row. When the list has focus, the pty pane below renders DIM on every cell. The CHANGELOG names the design property verbatim: "SGR 2 lands as ~50% lightness on every supported terminal — no theme work or layout shift, just a clean visual cue for 'input goes here vs. there.'"

**The two diffs read as one pattern.** Both `PaneWidget::render` and `ListView::render` gain the same shape: a `let dim = if self.focused { Modifier::empty() } else { Modifier::DIM };` binding above the per-cell loop, then `style.add_modifier(dim)` (or in the list case, `marker_style.add_modifier(dim)` and `name_style.add_modifier(dim)`) applied to the non-cursor rendering. The cursor-row treatment is left untouched: the file list's `cursor_bg_dim` already handled the focused-vs-pane-focused cursor coloring before this PR; the pane's cursor-block had its own focused/unfocused dim distinction (which PR #29 will modify hours later — see PR #29 entry). The CHANGELOG names the stack: "The cursor row's existing `cursor_bg_dim` treatment stacks on top so the highlighted row stays distinguishable in either state."

**The BUGS.md item this PR closes.** `BUGS.md` SMALL drops the line "darken screen on unfocused pane to better distinguish focus" (one-line removal); `### FIXED ###` gains a `(fixed, v1.41.13)` entry recording the per-cell DIM modifier. The dropped SMALL line is the explicit precedent — pane-vs-list focus ambiguity was a recorded concern before this PR; the fix lifts it from SMALL to FIXED in one diff.

**Cross-references inside arc 03.**

- PR #29 (next per-PR entry below; same calendar day, 3.5 hours after this PR's merge) modifies the pane's *cursor-block* treatment, not its general-cell treatment. PR #29 drops the cursor-block-when-unfocused dim branch entirely (because under PR #29's three-condition guard, an unfocused pane never paints a cursor block at all). PR #26's general-cell DIM modifier on the pane is left intact by PR #29; the two visual-state mechanisms compose cleanly. The within-arc supersession is on the cursor-block specifically, not on PR #26's per-cell DIM.
- PR #34 (final per-PR entry below; same calendar day, 9.4 hours after this PR's merge) reuses PR #26's `PaneWidget` DIM behavior to indicate unfocused-overlay state. The render call at `src/app/mod.rs::2316` post-PR-#34 sets `PaneWidget::focused: overlay_focused`, where `overlay_focused = !self.state.pane_focused`. The DIM-when-unfocused machinery PR #26 added becomes the visual cue PR #34 leans on — without PR #26, PR #34's overlay-focus-switch would have no native rendering to differentiate "overlay holds focus" from "bottom pane holds focus."

**Drift findings flagged for the insight layer.**

- The branch is `feat/dim-unfocused-pane` (feature prefix); the commit subject reads "feat:" and the CHANGELOG buckets the change under `### Changed`, not `### Added` or `### Fixed`. Three-way prefix-vs-subject-vs-changelog: feature/feature/changed. The Changed bucket reads as the most accurate against the diff (existing surface re-renders with a new modifier; no new capability gates).
- The dropped SMALL line ("darken screen on unfocused pane to better distinguish focus") was a one-line BUGS.md item. Whether it was the maintainer's own pre-emptive note or a user report is not narratable from the diff alone — the line carries no attribution. Captured here as a flag for the insight layer's BUGS.md-source catalogue.
- This PR's per-cell DIM on `PaneWidget` is the load-bearing rendering for PR #34's overlay focus indicator. The dependency goes pane-widget → overlay-rendering — the overlay shares the `PaneWidget` render path. PR #26 lands the mechanism; PR #34 extends its consumer set. Within-arc reuse is captured for the eventual recurrence reading.

Provenance:
- 20fba00 (PR #26 feat/dim-unfocused-pane, 2026-05-06) — full PR.
- bfc4a18 → 7683e22 — parent and tip SHAs for the diff inspection.
- `git diff bfc4a18..7683e22 -- src/pane/widget.rs`: +13/-1; `let dim = if self.focused { Modifier::empty() } else { Modifier::DIM };` binding added at lines 25-34 post-merge; `cell_style(cell).add_modifier(dim)` at line 44 post-merge.
- `git diff bfc4a18..7683e22 -- src/ui/list_view.rs`: +13/-1; matching `let dim = ...` binding above the per-row loop; `(marker_style.add_modifier(dim), name_style.add_modifier(dim))` for non-cursor rows.
- `git diff bfc4a18..7683e22 -- BUGS.md`: 1 line removed from SMALL ("darken screen on unfocused pane to better distinguish focus"); 5 lines added to `### FIXED ###` for `(fixed, v1.41.13)`.
- `git diff bfc4a18..7683e22 -- CHANGELOG.md`: 10 lines added under `[Unreleased]` `### Changed`; SGR 2 / ~50% lightness sentence quoted verbatim above.
- `git diff bfc4a18..7683e22 -- FEATURES.md`: 4 lines added; "The whole unfocused side dims" entry under the "switching between the file list and the pane" section.
- `git diff bfc4a18..7683e22 -- Cargo.toml`: `version = "1.41.12"` → `version = "1.41.13"`.
- `history-arc-03-pane-behavior` framing entry = 01KR106N6HSW66R76HN9VJPF1Q.
- `history-arc-03-pane-behavior` PR #6 entry = 01KR108QNEEG64J8W8XJERJTZG.
- `history-arc-03-pane-behavior` PR #22 entry = 01KR10ASW7YSX4MB8G28X2C9N4.

<!-- Entry-ID: 01KR10CGQ8NV7FYX39YZTR0FPM -->

---
Entry: Claude Code (caleb) 2026-05-07T10:42:21.882346+00:00
Role: scribe
Type: Note
Title: PR #29 (fix/skip-pane-cursor-block-when-uninvited): three-condition guard generalizing PR #5 — and editing PR #26's cursor-block branch hours after it landed

Spec: scribe

tags: #history #arc-03

PR #29 is the fourth move in arc 03 and the rendering-correctness move. It is also the entry whose internal shape is most explicitly a supersession ladder — two earlier guards on the same code surface get superseded in this PR's diff. Commit subject reads "fix: skip pane cursor block for unfocused / alt-screen panes (v1.41.16)" (commit bdb8d87, 2026-05-06). Diff: 6 files, +49/-13. The single source file is `src/pane/widget.rs` (+22/-6 net on the cursor-block block).

The supersession ladder reads as:

- **Round 1 — PR #5 (arc 02), 2026-04-30.** The narrow `screen.hide_cursor()` guard. Catches the lazygit case the gap analysis named in "Top suspects" §1: lazygit hides the cursor and draws its own selection highlight. Misses every TUI app that *doesn't* hide the cursor but does paint its own (nvim's beam in insert mode, vim's block, etc.).
- **Round 2 — PR #26 (this arc), 2026-05-06 14:16.** The pane-widget DIM modifier on every cell when unfocused. Touches the general-cell rendering, not the cursor-block guard; the cursor-block kept its `Modifier::REVERSED` paint with a separate `if !self.focused { add_modifier(DIM) }` branch for unfocused-pane dimming.
- **Round 3 — PR #29 (this entry), 2026-05-06 17:54.** The three-condition guard. Generalizes PR #5; drops PR #26's cursor-block dim-when-unfocused branch.

**The new guard.** The pre-PR-#29 code (post-PR-#5, post-PR-#26) reads:

```rust
if !self.screen.hide_cursor() {
    let (cy, cx) = self.screen.cursor_position();
    if cy < draw_rows && cx < draw_cols {
        // ...
        let mut s = cell_ref.style().add_modifier(Modifier::REVERSED);
        if !self.focused {
            s = s.add_modifier(Modifier::DIM);
        }
        cell_ref.set_style(s);
    }
}
```

The post-PR-#29 code reads:

```rust
let want_block_cursor =
    self.focused && !self.screen.alternate_screen() && !self.screen.hide_cursor();
if want_block_cursor {
    let (cy, cx) = self.screen.cursor_position();
    if cy < draw_rows && cx < draw_cols {
        // ...
        let s = cell_ref.style().add_modifier(Modifier::REVERSED);
        cell_ref.set_style(s);
    }
}
```

The transformation does three things at once: the guard goes from one condition (`!hide_cursor()`) to three (`focused && !alternate_screen() && !hide_cursor()`); the dim-when-unfocused branch is removed (because under the new guard, an unfocused pane never reaches this block); and the policy comment lands as a verbatim three-numbered-rationale block.

**The policy comment, verbatim.** The diff's new comment reads:

> "1. Pane is focused. Otherwise the user's eye isn't here and a block in an unfocused pane is just visual clutter / a pseudo-second-cursor that competes with the real input target above (the file list).
> 2. Child hasn't switched to the alternate screen. Full-screen TUIs (nvim, vim, less, htop, lazygit, claude in TUI mode) paint their own cursor in their own shape — beam in nvim insert mode, e.g. — and our hard-coded block clobbers it with the wrong shape and color.
> 3. Child hasn't explicitly hidden the cursor (DEC ?25l).
>
> Net effect: a plain shell / REPL on the main screen still gets the visibility cue (where the next char will land); alt-screen TUIs and unfocused panes show their natural state."

The named alt-screen TUIs ("nvim, vim, less, htop, lazygit, claude in TUI mode") are the empirical answer to the question PR #5's gap analysis raised — the apps PR #5's hide-cursor-only guard misses. lazygit *also* sets hide-cursor and was caught by PR #5; the addition of nvim, vim, less, htop, and claude-in-TUI-mode names the broader class.

**Back-reference: arc 02 investigation entry (= 01KR0YXXZRQR24CSNAK4Q7808T) — gap-analysis "Top suspects" §1.** PR #5's gap-analysis text reads (preserved verbatim in arc 02's investigation entry): "spyc unconditionally reverse-videoes the cell at `screen.cursor_position()`, even when the child has set DEC ?25l (cursor hidden). vt100 already exposes `screen.hide_cursor()`, but `src/pane/widget.rs:43–55` never reads it. lazygit hides the cursor and draws its own selection highlight, so a stray reverse-video square sits on some panel — visually reads exactly as 'rendering glitch'." Suspect §1 was the explicit motivating case for PR #5's narrow guard. PR #29's three-condition guard generalizes from "the case where the child set DEC ?25l" to "the broader class of cases where spyc has no business painting its own block."

**Back-reference: arc 02 harvest entry (= 01KR0Z11CKNJRYEZ3T38EAFSC4) — BUGS.md SMALL cursor-block-reverse-video item.** PR #12 lifted the gap-analysis suspect §1 text into BUGS.md as a SMALL item three days after PR #5's partial fix landed. The arc-02 harvest entry's projection: "Arc 03's PR #29 entry will narrate how the BUGS.md residual gets fully extinguished." The empirical resolution: PR #29's three-condition guard *behaviorally* extinguishes the case PR #12's text describes (the alt-screen-without-hide-cursor cases the projection had in mind), but the BUGS.md text itself — verbatim "pane widget always paints a reverse-video cursor block at `screen.cursor_position()` even when the child has set `DEC ?25l` (cursor hidden)" — is **not removed by PR #29's diff**. Inspection of `BUGS.md` at PR #29's tip confirms the PR #12-added cursor-block-reverse-video block is still present in the SMALL bucket post-merge.

What PR #29's BUGS.md diff actually removes is a *different* SMALL line: "user reported: block cursor in insert mode on nvim even when that is not my cursor (ntd: we should remove any cursor overrides?)" — a separate user-report entry, predating PR #5's gap analysis. PR #29 removes that line, behaviorally addresses the case PR #12's text describes, and leaves PR #12's text in BUGS.md SMALL. The harvest entry's "fully extinguished" projection lands as half-true: the *behavior* is extinguished; the *durable-record cleanup* is incomplete. Whether that incompleteness is intentional (PR #12's text describes a residual class still latent under the three-condition guard) or oversight (the durable record was not re-checked against the new guard) is not narratable from the diff alone.

**Within-arc supersession: PR #26's cursor-block dim-when-unfocused branch.** PR #26 (3.5 hours earlier) added per-cell DIM to the unfocused PaneWidget's general-cell rendering. The cursor-block treatment retained its own `if !self.focused { add_modifier(DIM) }` branch (preserved from earlier code, untouched by PR #26). PR #29 drops that cursor-block dim branch entirely — under the new three-condition guard, an unfocused pane never enters the cursor-block paint path at all, so the dim branch becomes unreachable code. PR #26's general-cell DIM modifier on the unfocused pane survives PR #29 untouched; the supersession is on the cursor-block specifically. Two visual-state mechanisms (PR #26's per-cell DIM and PR #29's three-condition cursor-block guard) coexist post-PR-#29, with PR #29's guard preempting the cursor-block-dim case PR #26 kept.

**The user-visible motivation, from the CHANGELOG.** The fix's lede reads verbatim: "Spyc used to paint a reverse-block at the pty cursor position unconditionally (modulo `?25l`-hidden), which fought with TUI apps that draw their own cursor — most visibly nvim's beam in insert mode, where users saw a block when the app was clearly asking for a beam." nvim's beam is the named example; the broader class is the policy comment's alt-screen-TUI list.

**Drift findings flagged for the insight layer.**

- The branch is `fix/skip-pane-cursor-block-when-uninvited` (fix prefix); the commit subject reads "fix:"; CHANGELOG buckets under `### Fixed`. All three align. Captured for the eventual drift catalogue's positive-control row.
- The arc-02 harvest entry's projection that PR #29 would "extinguish the BUGS.md residual" is behaviorally true and durable-record-incomplete. The mismatch between behavioral coverage and durable-record cleanup is the kind of discrepancy the insight layer's drift / cross-thread-fidelity reading should pick up — a residual flagged in BUGS.md outlives its underlying-bug-fix because no one re-checked the record. Captured here for that reading.
- PR #29's policy comment names "claude in TUI mode" alongside the standard alt-screen TUIs (nvim, vim, less, htop, lazygit). The "claude in TUI mode" reference locates this PR's primary user (the bottom pane's most common occupant per README/FEATURES.md framing) inside the bug class. Same self-locating-as-user signal as PR #6's `toggle_pane_zoom` resize-comment ("otherwise Claude's UI is the wrong size"). Captured for the recurrence reading.
- PR #29 lands 3.5 hours after PR #26 on the same calendar day. The cursor-block dim branch PR #29 drops was presumably read against a PR #26 — pre-merge or post-merge — and the supersession-within-three-hours shape is the kind of fast-iteration cadence the insight layer's velocity reading should observe. Captured factually, not interpreted.
- The diff also removes the FEATURES.md line "Pane cursor shows as a bright reverse-video block when focused, dim block when unfocused" and replaces it with the new three-condition policy — durable-doc cleanup tracks the diff exactly here, in contrast to the BUGS.md residual question above.

Provenance:
- bdb8d87 (PR #29 fix/skip-pane-cursor-block-when-uninvited, 2026-05-06) — full PR.
- 306b43f → b2f3e2e — parent and tip SHAs for the diff inspection.
- `git diff 306b43f..b2f3e2e -- src/pane/widget.rs`: the cursor-block guard-and-paint block at lines 48-79 post-merge; pre-state at lines 51-65 pre-merge with `if !self.screen.hide_cursor()` wrapping `if !self.focused { add_modifier(DIM) }`; post-state with `let want_block_cursor = self.focused && !self.screen.alternate_screen() && !self.screen.hide_cursor();` and the dim branch removed.
- `git diff 306b43f..b2f3e2e -- CHANGELOG.md`: 13 lines added under `[Unreleased]` `### Fixed`; nvim-beam lede quoted verbatim above.
- `git diff 306b43f..b2f3e2e -- BUGS.md`: 2 lines removed from SMALL ("user reported: block cursor in insert mode on nvim even when that is not my cursor"); 6 lines added to `### FIXED ###` for `(fixed, v1.41.16)`. PR #12-added cursor-block-reverse-video text in SMALL is not touched.
- `git show bdb8d87^2:BUGS.md` confirms the PR #12-added cursor-block-reverse-video block ("pane widget always paints a reverse-video cursor block at `screen.cursor_position()` even when the child has set `DEC ?25l`...") remains in BUGS.md SMALL post-merge.
- `git diff 306b43f..b2f3e2e -- FEATURES.md`: 7 lines added / 2 lines removed; the focused/unfocused-block sentence replaced with the three-condition policy.
- `git diff 306b43f..b2f3e2e -- Cargo.toml`: `version = "1.41.15"` → `version = "1.41.16"`.
- `history-arc-02-lazygit-investigation-and-harvest` investigation entry = 01KR0YXXZRQR24CSNAK4Q7808T — gap-analysis "Top suspects" §1 text; PR #5's narrow `screen.hide_cursor()` guard.
- `history-arc-02-lazygit-investigation-and-harvest` harvest entry = 01KR0Z11CKNJRYEZ3T38EAFSC4 — BUGS.md SMALL cursor-block-reverse-video item; the "fully extinguished" projection.
- `history-arc-03-pane-behavior` framing entry = 01KR106N6HSW66R76HN9VJPF1Q.
- `history-arc-03-pane-behavior` PR #6 entry = 01KR108QNEEG64J8W8XJERJTZG.
- `history-arc-03-pane-behavior` PR #22 entry = 01KR10ASW7YSX4MB8G28X2C9N4.
- `history-arc-03-pane-behavior` PR #26 entry = 01KR10CGQ8NV7FYX39YZTR0FPM (within-arc supersession partner).

<!-- Entry-ID: 01KR10G02J2234D0WBMWMYC35M -->

---
Entry: Claude Code (caleb) 2026-05-07T10:43:38.913990+00:00
Role: scribe
Type: Note
Title: PR #34 (fix/top-overlay-focus-switch): meta-key fall-through, overlay-as-pane focus model, ;cmd no longer traps

Spec: scribe

tags: #history #arc-03

PR #34 is the fifth and final move in arc 03. It is the focus-routing move — the one that extends `pane_focused`'s meaning from "list-vs-bottom-pane" to "list-or-overlay-vs-bottom-pane." Commit subject reads "fix: ;cmd overlay shares focus with bottom pane (v1.41.21)" (commit 8e9fb2c, 2026-05-06). Diff: 6 files, +79/-16. The single source file is `src/app/mod.rs` (+50/-8) — three distinct touch points, walked in order below.

**The bug being fixed.** Pre-fix, the `;cmd` interactive-overlay code path was an unconditional key takeover. The pre-PR comment in the dispatch block reads verbatim: "Top overlay (interactive `;` command) owns all keys — it's a full takeover of the top area. The user exits by quitting the subprocess itself (q in top, :q in vim, etc.)." With a bottom pane already running claude or zsh, that meant the user had to quit the overlay subprocess to glance at the lower pane. The CHANGELOG names the user-visible workflow this PR enables: "`;less docs/architecture.md`, `^a-j` into claude, do work, `^a-k` back to scroll the doc, repeat."

**Touch point 1 — the dispatch fall-through.** The pre-PR overlay-eats-everything block (`src/app/mod.rs::2849-2852` post-merge area) becomes a guarded fall-through: `is_meta = is_spyc_meta_when_pane_focused(key, self.state.resolver.is_pending())` and `bottom_owns = has_bottom && self.state.pane_focused`. If the key is neither a meta key (ctrl-a/ctrl-w/ctrl-backslash/F10) nor a key that the bottom pane should own, the overlay still gets it via `overlay.send_key(key)` — preserving the in-overlay `q` / `:q` / scrolling semantics. Otherwise the dispatch falls through to the chord resolver (for meta keys) or the pane-forwarding block (for bottom-pane-focused keys). The replaced policy comment names the change verbatim: "Used to be an unconditional takeover ('the user exits by quitting the subprocess itself'), which was fine when only the overlay existed — but if the user has a bottom pane too (e.g. claude open), they couldn't pop down to it without quitting the overlay first."

**Touch point 2 — overlay rendering tracks focus.** The overlay's `frame.render_widget(PaneWidget { ..., focused: true }, overlay_area)` becomes `focused: overlay_focused`, where `overlay_focused = !self.state.pane_focused`. Symmetrically, the bottom-pane render (a few lines below, in the same draw block) changes from `focused: false` to `focused: self.state.pane_focused`. Both render paths now read the same `pane_focused` boolean as their input-target signal, with the overlay treating it as the inverted slot. The visual-state consequence is that the unfocused side (overlay or bottom pane) renders with `Modifier::DIM` per `PaneWidget`'s rendering — which is precisely the behavior PR #26 (entry above) added to `PaneWidget`. PR #34 leans on PR #26's machinery without modifying it; the overlay-focus-switch becomes visible to the user because the unfocused half dims.

**Touch point 3 — overlay spawn forces focus to the overlay.** Three call sites that spawn into `self.top_overlay` (the `;cmd` dispatch path, the prompt-completion path, and the D-key file-open path that PR #35 introduces but lands on this same release stream) all gain a `self.state.pane_focused = false;` line immediately after `self.top_overlay = Some(p);`. The inline comment names the rationale verbatim: "Initial focus is on the new overlay so the user can drive the subprocess directly. ^a-j hands focus to the bottom pane (when one is open)." New overlays steal focus on spawn; subsequent `^a-j`/`^a-k` chord taps flip between overlay and bottom pane.

**Touch point 4 — focus flash labels.** `App::flash_focus` (or its equivalent at the focus-toggle path) gains a branch: when the non-pane side is currently the overlay (i.e. `self.top_overlay.is_some()`), the flash reads `focus: overlay`; otherwise it still reads `focus: spyc`. The inline comment names the user-visible ambiguity this resolves: "When a `;cmd` overlay is showing the spyc-list slot, the 'non-pane' side is the overlay subprocess, not the file list. Label accordingly so the user can read what just got focus instead of guessing."

**The state model after PR #34.** Two save-and-restore axes now share `pane_focused: bool`:

- PR #6's zoom: `pane_focus_before_zoom: Option<bool>` saves `pane_focused` at zoom-on, restores it at zoom-off. Zoom forces focus into the pane.
- PR #34's overlay: no save-and-restore; spawn forces focus to overlay (`pane_focused = false`); chord-toggle flips between overlay and bottom pane.

The two mechanisms compose without conflict: zoom-while-overlay-is-up is not exercised by any diff in arc 03 (no commit lands a `;less` plus `^a-z` interaction); whether the composition is correct is a question for whoever runs that workflow, not narratable from the diff. Captured for the eventual insight layer's "implicit-state-coexistence" reading.

**The BUGS.md item this PR closes.** `BUGS.md` SMALL drops the line "using editor in top pane prevents switching to bottom pane" (one-line removal at the top of the SMALL bucket); `### FIXED ###` gains a `(fixed, v1.41.21)` entry recording the fall-through machinery. The dropped SMALL line is the explicit precedent — the focus-trap was a recorded concern; the fix lifts it from SMALL to FIXED in one diff, the same shape PR #26 used. (Two unrelated SMALL items also reposition in this PR's BUGS.md diff: `D in spyc pane should open in $PAGER in the top pane` and `/ should match within names - it seems to assume ^`. Both are unrelated to the overlay focus-switch and read as incidental SMALL-bucket reordering.)

**Drift findings flagged for the insight layer.**

- The branch is `fix/top-overlay-focus-switch` (fix prefix); the commit subject reads "fix:"; CHANGELOG buckets under `### Fixed`. All three align. Positive-control row for the drift catalogue.
- `pane_focused: bool` becomes a three-meaning slot in the post-PR-#34 surface: (a) list-vs-pane focus, (b) overlay-vs-pane focus when the overlay is up, (c) the source axis for PR #6's zoom save-and-restore. The single boolean is doing more work than its type advertises. Captured for the insight layer's "names-that-out-grew-their-scope" reading.
- The `is_spyc_meta_when_pane_focused` helper-name reads as a hint that meta-key fall-through was already a concept somewhere upstream (possibly bottom-pane-focused-meta-key handling, possibly chord-resolver pre-empting). The diff calls into the helper but does not introduce it. Whether the helper is from this PR or pre-existing is determinable only from its definition site; flagged here without resolution.
- The CHANGELOG entry's named user-visible workflow (`;less docs/architecture.md`, then `^a-j` into claude, then back) is the third arc-03 PR to surface "claude in the bottom pane" as the implicit primary user (PR #6's resize comment, PR #29's policy-comment list, PR #34's CHANGELOG workflow). Recurrence-reading material for the insight layer; arc 03 names the recurrence factually only.
- New overlays-steal-focus reverses the previous default (spawn put focus on `pane_focused: true` at the bottom-pane path). Whether any pre-PR-#34 muscle memory expected the bottom pane to keep focus across overlay spawns is not narratable from the diff; captured for the insight layer's behavior-change-as-fix reading.

Provenance:
- 8e9fb2c (PR #34 fix/top-overlay-focus-switch, 2026-05-06) — full PR.
- cf9e8ff → ef24eb4 — parent and tip SHAs for the diff inspection.
- `git diff cf9e8ff..ef24eb4 -- src/app/mod.rs`: +50/-8; four touch points walked above.
- `git diff cf9e8ff..ef24eb4 -- CHANGELOG.md`: 18 lines added under `[Unreleased]` `### Fixed`; the workflow lede ("`;less docs/architecture.md`, `^a-j` into claude...") quoted verbatim above.
- `git diff cf9e8ff..ef24eb4 -- BUGS.md`: 1 line removed from SMALL ("using editor in top pane prevents switching to bottom pane"); 8 lines added to `### FIXED ###` for `(fixed, v1.41.21)`; two unrelated SMALL items reposition.
- `git diff cf9e8ff..ef24eb4 -- FEATURES.md`: 7 lines added / 3 lines removed under "Three modes of running commands"; `^a-j`/`^a-k` overlay-vs-pane focus flip described.
- `git diff cf9e8ff..ef24eb4 -- Cargo.toml`: `version = "1.41.20"` → `version = "1.41.21"`.
- `history-arc-03-pane-behavior` framing entry = 01KR106N6HSW66R76HN9VJPF1Q.
- `history-arc-03-pane-behavior` PR #6 entry = 01KR108QNEEG64J8W8XJERJTZG (zoom save-and-restore, the other `pane_focused` axis).
- `history-arc-03-pane-behavior` PR #26 entry = 01KR10CGQ8NV7FYX39YZTR0FPM (the `PaneWidget` DIM machinery PR #34 leans on).
- `history-arc-03-pane-behavior` PR #29 entry = 01KR10G02J2234D0WBMWMYC35M.

<!-- Entry-ID: 01KR10JBACRS3Z71WTHGBVCPJM -->
