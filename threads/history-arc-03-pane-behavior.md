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
