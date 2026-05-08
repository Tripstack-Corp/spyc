# history-narrative-arc — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: history-narrative-arc
Created: 2026-05-08T18:09:08.993895+00:00

---
Entry: Claude Code (caleb) 2026-05-08T18:09:08.993895+00:00
Role: scribe
Type: Note
Title: Framing: two histories, one artifact, twenty-two days

Spec: scribe

tags: #narrative #final

This thread is for one reader. Derek, you authored the work; the eight-arc reconstruction across thirteen prior threads catalogued it. What you have not yet had reason to read is the catalogue itself. This entry, and the five that follow, are an attempt to make the catalogue worth reading without forcing you through 140 entries to do it.

The spine the narrative holds: spyc's first 22 days produced two parallel histories — what the code did, and how the code was talked about. The two agree more often than they don't. Where they diverge, they diverge along axes the catalogues now name with specificity. The divergence is not large in any single PR; it is structural across the window. The catalogue's value to you is that the structure is now legible at a grain no commit log shows.

Four insight threads sit downstream of the eight arc threads. `insight-drift` counted misnaming-at-merge: 16 instances across 6 patterns, dominantly description-layer. `insight-recurrence` named event-shape recurrence at 6 patterns, with the strongest being a 4-instance grain × register matrix of how supersessions get acknowledged. `insight-trajectory` tracked stated-plan documents against execution: across three independent stated-plan surfaces, fifteen honor instances against zero exact executions of positive recommendations. `insight-emergent-properties` read the inherited counts as artifact-grain properties: six properties named, two declined.

The narrative does not re-do that work. It walks the network at story grain, drawing on what the catalogues most consistently surface. Six observations have enough cross-thread weight that any honest reading carries them: the gap-analysis methodology PR #5 introduced and its three suspects' eight-day disposition trace; the cursor-block lineage from PR #5 to PR #29 across six calendar days; the MCP-bridge bracket from PR #18 to PR #37 across two days; the panic-recovery / dep-bump pair at 49 minutes; the fifteen-and-zero asymmetry across three stated-plan documents; and what the eight-arc shape adds up to as artifact properties.

The voice contract: analyst register. Claims about the artifact, the diffs, the patterns the network surfaced. Quotes from your commit bodies, CHANGELOG entries, BUGS.md notes, doc-comments, ROADMAP.md, AGENTS.md, SECURITY.md attributed by `(commit <sha>, <date>)`. No motive attribution; the catalogues' work-vs-worker line holds in the narrative the same way it held in the arcs and the insight threads. The narrative names the artifact's commit-history layer as carrying register variance; it does not name what the maintainer was thinking when the variance landed.

The provenance contract: every entry ends with a `Provenance:` block citing the arc and insight-thread entries the narrative draws on, plus the commit SHAs and verbatim quotations. The discipline is what lets the narrative carry interpretive weight without overreach.

What follows: five middle entries, one each on a thread the network surfaced; then a closing entry on what the practice produced. The walk is meant to be done in one sitting.

Provenance:
- `history-overview` framing entry (the spine of the eight-arc reconstruction; voice contract source) = 01KR0TRFWT9W6WMFHC49YSW0BG.
- `history-overview` segmentation entry (the eight-arc segmentation; 36 PRs accounted for) = 01KR0TWHTC1MPK4KJ08Y9SPE6P.
- `insight-drift` closure entry (16 instances / 6 patterns; description-layer dominance) = 01KR3BQHTCQ7YGHWQJ2ZE4PJQ4.
- `insight-recurrence` closure entry (6 patterns; supersession-kind × register matrix) = 01KR3DFHA7FRV3BXEH2Z8SFJQN.
- `insight-trajectory` closure entry (eight stated-plan documents; fifteen honor instances + zero exactly-as-specified positive-recommendation executions) = 01KR3F9EF9WF9Q34FRAR2XPSZS.
- `insight-emergent-properties` closure entry (six properties promoted; two declined) = 01KR3HZNC62XT11V4CW2GZ5V0S.
- `insight-emergent-properties` four-tier ladder tail (the assembly-vs-cumulative ratio inverting across the ladder) = 01KR3J1JKR98RY75JBYA8EKC1X.

<!-- Entry-ID: 01KR4CESQSGA7061ZTY9TM4QWX -->

---
Entry: Claude Code (caleb) 2026-05-08T18:10:44.944649+00:00
Role: scribe
Type: Note
Title: The gap analysis as method: 399 lines, three suspects, eight days to disposition

Spec: scribe

tags: #narrative #final

PR #5 is the only PR in the 22-day window with the `investigate/` prefix. Commit subject: "lazygit investigation + cursor-block fix (v1.37.2)" (commit 0691666, 2026-04-30). Of the 444 insertions, 399 are documentation in a freshly-created `notes/` directory; the actual code change is seven lines in `src/pane/widget.rs` guarding the cursor block on a new condition. The diff weight inverts the title: the investigation is the load-bearing half, the code fix is the smaller and narrower one.

What makes the investigation worth pausing on, six entries into a five-entry walk, is what the methodology produced once it ran.

`notes/lazygit-gap-analysis.md` named three top suspects for the user-reported "rendering / conflict issues." Each was identified verbatim in the diff. §1: *"Spurious cursor block from `widget.rs`. spyc unconditionally reverse-videoes the cell at `screen.cursor_position()`, even when the child has set DEC ?25l (cursor hidden). vt100 already exposes `screen.hide_cursor()`, but `src/pane/widget.rs:43–55` never reads it. lazygit hides the cursor and draws its own selection highlight, so a stray reverse-video square sits on some panel — visually reads exactly as 'rendering glitch.'"* §2: *"No mouse, anywhere. Mouse capture is not enabled on the host terminal..."* §3: *"Synchronized-output (mode 2026) tearing... vt100 0.15 has no parse arm for 2026 — bytes are dropped... during a fast diff scroll or commit-list page-down the renderer reads a half-finished frame and paints it. Looks like flicker / a sliver of stale text under the new content for one frame."* (commit 0691666, 2026-04-30.)

Eight calendar days later, all three are dispositioned.

§1 closed at PR #29 (`fix/skip-pane-cursor-block-when-uninvited`, commit bdb8d87, 2026-05-06). The narrow guard PR #5 shipped — `if !self.screen.hide_cursor()`, single condition — generalizes to a three-condition guard: `focused && !alternate_screen() && !hide_cursor()`. The guard's policy comment lands as a verbatim three-numbered rationale in the diff, and names the alt-screen TUIs the broader class catches: *"nvim, vim, less, htop, lazygit, claude in TUI mode"* (commit bdb8d87, 2026-05-06). Six days from spec to generalization.

§2 closed across the window as deferred-as-non-goal. No PR adds mouse capture in `src/main.rs::setup_terminal`. `ROADMAP.md:445-447` names "mouse support beyond what already exists" as an explicit non-goal of the project. The suspect was good enough to identify; the disposition is policy, not work. The trajectory thread treats deferred-as-non-goal as honor — the suspect was not ignored, it was answered against the charter.

§3 closed at PR #31 (`chore/vt100-and-ratatui-upgrade`, commit 105db8d, 2026-05-06). The vt100 0.15 → 0.16 trio bump (which forces ratatui 0.29 → 0.30 because vt100 0.16 needs `unicode-width ≥0.2.1` and ratatui 0.29 pinned `=0.2.0`, which then forces ansi-to-tui 7 → 8) ships with the commit-body claim *"Also retires the two MAYBE entries from BUGS.md about mode 2026 (synchronized output) and OSC 8 (terminal hyperlinks) — both should now parse correctly under 0.16"* (commit fc1789d, 2026-05-06). The BUGS.md MAYBE entry PR #12 had lifted from the gap analysis on Day 4 is removed in PR #31's diff; a `(fixed, v1.41.18)` block names mode 2026 directly.

Three-for-three disposition is the structural fact. None of the three suspects was wrong; none was abandoned; each found its terminal state at the grain its content warranted. The chain itself is worth tracing because no single thread of commit messages shows it: §1 lives in PR #5's gap-analysis text, then in PR #12's BUGS.md harvest, then in arc 03's PR #29 narration. §3 lives in PR #5's gap-analysis text, then in PR #12's BUGS.md harvest, then in arc 08's PR #30 BUGS.md MAYBE expansion, then in PR #31's commit body and FIXED block. The five-PR, four-entry, eight-day chain is what the catalogues now hold.

The asymmetry in the verifications is worth acknowledging. §1 closes with durable-record incompleteness — PR #29's diff *behaviorally* extinguishes the case PR #12's BUGS.md text describes, but the BUGS.md text itself stays in the SMALL bucket post-merge. The arc-03 PR #29 entry caught this honestly: PR #29 removes a different SMALL line (a user's nvim-beam report), behaviorally addresses the case PR #12's text describes, and leaves the original residual in place. The behavior and the durable-record cleanup don't track 1:1. §3 closes with test-coverage gap — vt100 0.16 is the "proper fix" for the parser bug per the commit body, but no test in PR #31 exercises the specific `screen.rs:934.unwrap()` byte stream. The verification rests on the maintainer's claim more than on a regression test, with PR #30's `catch_unwind` safety net silently catching any persistent edge case if one exists.

These honesty notes are not failures of the closure. They are the catalogue's record of what *exact-as-specified* would have meant, and where the closure actually landed. Three for three on disposition; two of the three carry verification-mode asterisks; one carries policy-of-the-project as its honor.

What this exhibits at trajectory grain is the point you'd miss if you read only the commit log. The gap analysis is the only document in the network whose internal sub-recommendations end up dispositioned three-for-three within the window. The UX catalogue's four adapt recommendations (§2 context-sensitive footer, §3-tip-half, §4 generalized pager picker, §5 scoped help) all land in modified shape; zero land exactly-as-specified. The ROADMAP additions that PR #5 imported verbatim from the catalogue's "Top 3 to consider first" — §4 generalized pager picker, §2 context-sensitive prompt-row hint, §5 scoped `?` help — also end up at zero exact executions, with §4 in particular spawning four PRs of DIRECTION ALIGNMENT (PR #33, PR #35, PR #8, PR #10) without the specific `PagerView::picker_items: Vec<(Label, Action)>` field.

The gap analysis disposition rate is the outlier: three for three at exact-state disposition (one resolved-clean, two resolved-with-asterisk; or in negative-recommendation framing, one honored-as-non-goal and two resolved). Other documents authored at the same vantage produce trajectory shape but not exact-state honor. This is one of the asymmetries the network surfaces; later entries in this narrative will make it more general.

Provenance:
- 0691666 (PR #5 investigate/lazygit-support, 2026-04-30) — gap-analysis suspects §1, §2, §3 verbatim from `notes/lazygit-gap-analysis.md`.
- bdb8d87 (PR #29 fix/skip-pane-cursor-block-when-uninvited, 2026-05-06) — §1 closure; three-condition guard policy comment with the alt-screen TUI list verbatim.
- 105db8d / fc1789d (PR #31 chore/vt100-and-ratatui-upgrade, 2026-05-06) — §3 closure; commit body retires the BUGS.md MAYBE entries verbatim.
- e210e58 (PR #12 chore/clean-notes, 2026-05-03) — BUGS.md harvest of the gap-analysis suspects.
- arc-02 investigation entry = 01KR0YXXZRQR24CSNAK4Q7808T (the gap-analysis text source; suspects quoted verbatim).
- arc-02 harvest entry = 01KR0Z11CKNJRYEZ3T38EAFSC4 (the BUGS.md residual that bridges the cursor-block lineage).
- arc-03 PR #29 entry = 01KR10G02J2234D0WBMWMYC35M (§1 closure with durable-record-incompleteness honesty note).
- arc-08 PR #31 entry = 01KR397RTYNS34SAGM46YJJRBY (§3 closure with test-coverage-gap honesty note).
- `insight-trajectory` Document #1 entry = 01KR3ENV1WP6R9SFRE1QME291S (three-for-three disposition; resolved-with-asterisk framing).
- `insight-trajectory` Document #2 entry = 01KR3ESJ42TT0ZGJHGHJ5CTNYC (UX catalogue 4-of-4 skips honored / 0-of-4 adapts exactly-executed).
- `insight-trajectory` Document #3 entry = 01KR3EW3166JZ59TDR8PYMGN4T (ROADMAP additions zero-of-three exactly-executed).
- `onboarding-product-charter` entry 0 = 01KR0P18MCE1H57Q5ZTAGKAJNH (the §2 honored-as-non-goal disposition source; non-goals at `ROADMAP.md:445-447`).

<!-- Entry-ID: 01KR4CHQ6XEXYC4FCC9AYKDB5V -->

---
Entry: Claude Code (caleb) 2026-05-08T18:12:04.373848+00:00
Role: scribe
Type: Note
Title: The cursor-block lineage: PR #5 to PR #29, six days, three guards, two silent supersessions

Spec: scribe

tags: #narrative #final

The gap analysis named the cursor-block case in suspect §1; PR #5's seven-line fix shipped a guard against the explicit-hide-cursor path that lazygit triggers. Six days later, PR #29 arrived at a three-condition guard that catches the broader class. Between the two, on the same calendar day as PR #29, PR #26 added a per-cell DIM modifier on unfocused panes. The three diffs together produce the lineage worth tracing because none of the three commits acknowledges the prior two.

Look at the code, post-PR-#5, post-PR-#26, pre-PR-#29:

```rust
if !self.screen.hide_cursor() {
    let (cy, cx) = self.screen.cursor_position();
    if cy < draw_rows && cx < draw_cols {
        let mut s = cell_ref.style().add_modifier(Modifier::REVERSED);
        if !self.focused {
            s = s.add_modifier(Modifier::DIM);
        }
        cell_ref.set_style(s);
    }
}
```

PR #5 gave us the outer `if !self.screen.hide_cursor()`. PR #26 (`feat/dim-unfocused-pane`, commit 20fba00, 2026-05-06 14:16 UTC) added the inner `if !self.focused` dim branch as part of a broader per-cell DIM treatment. Then post-PR-#29:

```rust
let want_block_cursor =
    self.focused && !self.screen.alternate_screen() && !self.screen.hide_cursor();
if want_block_cursor {
    let (cy, cx) = self.screen.cursor_position();
    if cy < draw_rows && cx < draw_cols {
        let s = cell_ref.style().add_modifier(Modifier::REVERSED);
        cell_ref.set_style(s);
    }
}
```

Three things happened at once. The guard generalized from one condition to three. PR #26's dim branch became unreachable code under the new guard and was removed. The policy comment landed as a verbatim three-numbered rationale block: *"1. Pane is focused. Otherwise the user's eye isn't here and a block in an unfocused pane is just visual clutter / a pseudo-second-cursor that competes with the real input target above (the file list). 2. Child hasn't switched to the alternate screen. Full-screen TUIs (nvim, vim, less, htop, lazygit, claude in TUI mode) paint their own cursor in their own shape — beam in nvim insert mode, e.g. — and our hard-coded block clobbers it with the wrong shape and color. 3. Child hasn't explicitly hidden the cursor (DEC ?25l). Net effect: a plain shell / REPL on the main screen still gets the visibility cue (where the next char will land); alt-screen TUIs and unfocused panes show their natural state."* (commit bdb8d87, 2026-05-06.)

Two supersessions live in this diff. One is cross-arc: PR #29 generalizes PR #5's narrow lazygit-shaped guard from six calendar days back, and the new guard's policy comment lists the broader class verbatim ("nvim, vim, less, htop, lazygit, claude in TUI mode") without naming PR #5 as predecessor. The other is within-arc, 3.5 hours: PR #29's diff edits code PR #26's diff added the same morning, dropping the dim branch PR #26 had introduced, again without naming PR #26 as predecessor. The CHANGELOG entry talks about nvim's beam in insert mode being clobbered. It does not say "this generalizes v1.37.3's narrow fix." It does not say "this drops the dim branch v1.41.13 added."

What `insight-recurrence` named at Pattern 2 and `insight-emergent-properties` named at Property 3 is that the silence here is not generic carelessness. Both supersessions are the same *kind* of supersession — guard-policy generalization — and both carry the same register, silent, regardless of whether the time grain is six calendar days or 3.5 hours. The kind correlates with the register more strongly than elapsed time correlates with it.

Compare to two other supersession instances in the network and the matrix becomes legible. PR #14 (`fix/undo-command`, commit c7419c1, 2026-05-03) ships 25 minutes after PR #13's graveyard subsystem. PR #14's CHANGELOG describes the bug behavior accurately and verbatim — *"`:undo` and `:graveyard` returned 'unknown command'. State's command dispatcher routes a fixed list of names to App's terminal-touching arms; `undo` and `graveyard` weren't on it, so they hit the unknown-command fallthrough before App's handler could see them. Added both to the punt list."* (commit c7419c1, 2026-05-03.) PR #14 names the bug, names the routing-vs-handler split, does not name PR #13. The register is behavior-described; the supersession-kind is missing-wire fix.

PR #31 ships 49 minutes after PR #30. Its commit body opens *"The vt100 bump is the proper fix for the `screen.rs:934.unwrap()` panic (caught defensively in v1.41.17). Smaller than I'd previously framed it"* (commit fc1789d, 2026-05-06). Five words doing the explicit reframing: the predecessor PR's framing is named and walked back in the artifact's commit-history layer. Register: explicit-reframing; kind: design-framing revision.

Three within-day instances populating distinct cells of a 3×3 grain × register matrix; one fourth long-grain instance reusing the silent register at six days for a structurally similar (guard-policy generalization) supersession-kind. `insight-recurrence` Pattern 2 makes the matrix factual. `insight-emergent-properties` Property 3 names the property at artifact grain: the codebase's commit-history layer carries register variation that aligns with what the diff performs more closely than with elapsed time alone.

That property is one of the things the catalogue can name and the commit log alone cannot. A reader of `git log` sees the four supersessions as four unrelated events. A reader of `git log` plus the per-PR diffs sees four supersessions as four moments where one PR superseded another at varying grains. A reader of the catalogue sees the four as one matrix populated by kind. The matrix is not in any single commit's record; it is in the cumulative reading of all four together at the artifact-grain.

There is one more honesty note worth surfacing here. The arc-02 harvest entry had projected that PR #29 would "fully extinguish" the BUGS.md cursor-block residual PR #12 had lifted from the gap analysis. The arc-03 PR #29 entry caught what actually landed: PR #29 removes a different SMALL line (a user's nvim-beam report), behaviorally addresses the case PR #12's text describes, and leaves the original residual in BUGS.md SMALL post-merge. The cursor-block reverse-video text PR #12 placed — *"pane widget always paints a reverse-video cursor block at `screen.cursor_position()` even when the child has set `DEC ?25l`..."* — is still there post-PR-#29. The behavior is closed; the durable record is not. Whether the residual class is genuinely still latent under the three-condition guard, or whether the BUGS.md text was simply not re-checked against the new policy, is not narratable from any diff. The catalogue records the mismatch and stops there.

Provenance:
- 0691666 (PR #5 investigate/lazygit-support, 2026-04-30) — narrow `if !self.screen.hide_cursor()` guard.
- 20fba00 (PR #26 feat/dim-unfocused-pane, 2026-05-06 14:16 UTC) — per-cell DIM treatment; cursor-block dim branch.
- bdb8d87 (PR #29 fix/skip-pane-cursor-block-when-uninvited, 2026-05-06 17:54 UTC) — three-condition guard; policy comment verbatim above.
- c7419c1 (PR #14 fix/undo-command, 2026-05-03) — behavior-described supersession at 25 minutes; CHANGELOG verbatim above.
- fc1789d (PR #31 chore/vt100-and-ratatui-upgrade, 2026-05-06) — explicit-reframing supersession at 49 minutes; "Smaller than I'd previously framed it" verbatim.
- arc-03 PR #29 entry = 01KR10G02J2234D0WBMWMYC35M (the supersession-ladder narration; durable-record-incompleteness note).
- arc-03 story-tail = 01KR11S8RG29J98QKN1H0VAA6W ("nothing in either commit says 'this supersedes PR #5'" framing).
- arc-08 PR #14 entry = 01KR38XPJ07ZFQHH1TG6X461WN (behavior-described supersession; "Repro: type `:undo` → flash 'unknown command: undo'" verbatim).
- arc-08 PR #31 entry = 01KR397RTYNS34SAGM46YJJRBY (explicit-reframing supersession).
- arc-08 story-tail = 01KR3A23E11K8F7VNVSM5XY6M2 (the three-grain × three-register matrix's first cumulative-grain naming).
- `insight-recurrence` Pattern 2 = 01KR3CZEM22Y5BRT1F2VQZ6EKZ (the four-instance matrix verified).
- `insight-emergent-properties` Property 3 = 01KR3HMF3F7A5EBXBQYEWHYR3Z (artifact-grain property named).

<!-- Entry-ID: 01KR4CM50GPNZ5JEK5J94SA8Z1 -->
