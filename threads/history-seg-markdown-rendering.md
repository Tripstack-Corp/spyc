# history-seg-markdown-rendering — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: history-seg-markdown-rendering
Created: 2026-06-08T22:08:54.949872+00:00

---
Entry: Claude Code (caleb) 2026-06-08T22:08:54.949872+00:00
Role: scribe
Type: Note
Title: PR #85 — m toggle preserves per-side scroll (markdown lifecycle)

Spec: scribe

tags: #history #markdown-rendering

Moment: markdown-rendering — Reconstructed: the pager's `m` (rendered↔source) toggle stops hard-resetting scroll to line 0; per-side scroll is stashed in a new `saved_alt_scroll` slot and restored on return, with a proportional projection on first visit to a side.   [kind: new-capability]
When: 2026-05-13 · PR #85 (fix/markdown-toggle-scroll-preserve) · commit 82dc9d8
Recorded rationale: "`m` (Markdown toggle) preserves scroll position. Pre-fix behavior was a hard reset to line 0 every time — the comment said 'preserving an absolute index would land somewhere arbitrary' because the rendered and source views have different line counts. Replaced with per-side memory: each side's scroll is stashed in a `saved_alt_scroll` slot on toggle, and restored when the user comes back. First-ever visit to a side has no memory yet, so we fall back to a proportional projection (`old_scroll * (new_total - 1) / (old_total - 1)`)" — CHANGELOG.md (added PR #85). Also adds the `[markdown] open_as_rendered` `.spycrc.toml` preference ("Default `true` (rendered first, `m` toggles to source — current behavior)").
Inferred intent: this is the first lifecycle moment for the rendering surface — it treats rendered and source as two views of one buffer with independent positions, the model that PR #125 later has to defend across the $EDITOR round-trip. evidence: src/ui/pager.rs +22..+97 introduces `saved_alt_scroll: Option<u16>` on PagerView and the toggle math `let num = u32::from(old_scroll) * (new_total - 1) as u32` with `max_index` clamp; four new unit tests pin the rule (proportional-first-time, exact-round-trip, clamp-to-bounds, no-alt no-op).
                  confidence: high
Supersedes: (none — establishes the rendered/source dual-view lifecycle)

The diff touches the largest surface of any moment in this slice on the pager itself: src/ui/pager.rs gains 117 lines, with the scroll-stash slot threaded through three PagerView constructors (src/ui/pager.rs +24,+32,+40,+48). The proportional fallback exists because the two sides have different line counts — restoring an absolute index would, per the quoted comment, "land somewhere arbitrary." The companion `open_as_rendered` config lands in src/config/mod.rs (+62) and src/config/default.spycrc.toml (+9), with the markdown-first default preserving prior behavior.

This is the rendering surface as hosted by the pager — see history-arc-05-pager-surface for the pager itself. The rendered/source duality established here is the lifecycle invariant that PR #125 (rerender-after-edit) must restore through the $EDITOR path.

Provenance:
- 82dc9d8 (PR #85 fix/markdown-toggle-scroll-preserve, 2026-05-13) — src/ui/pager.rs +117/-? (saved_alt_scroll slot + toggle projection + 4 tests); src/config/mod.rs +62, src/config/default.spycrc.toml +9 (open_as_rendered knob); src/app/mod.rs +30/-? wiring; BUGS.md +2, CHANGELOG.md +27
- CHANGELOG.md (added PR #85) — quoted scroll-preservation + open_as_rendered rationale

<!-- Entry-ID: 01KTMMG3MTKFGRKYEJGHN65QY8 -->

---
Entry: Claude Code (caleb) 2026-06-08T22:09:16.475296+00:00
Role: scribe
Type: Note
Title: PR #96 — loose-list bullets stay attached (just_started_item guard)

Spec: scribe

tags: #history #markdown-rendering

Moment: markdown-rendering — Reconstructed: the markdown renderer stops orphaning loose-list bullet glyphs onto their own line; a one-shot `just_started_item` flag set by `Tag::Item` suppresses the `Tag::Paragraph` line-flush that was discarding the bullet.   [kind: gotcha]
When: 2026-05-16 · PR #96 (fix/markdown-loose-list-bullet-attach) · commit b0a321c
Recorded rationale: "Markdown pager: loose-list bullets stay attached to their item text. Viewing a `.md` file with blank-line-separated bullets (a *loose* list per CommonMark — what BUGS.md happens to be) rendered as `•` alone on one line, then the item text on the next line. Cause: pulldown-cmark wraps each loose-list item in a `Paragraph`, and the paragraph-start handler unconditionally flushed `current` to start a new line — dumping the bullet glyph that `Tag::Item` had just pushed. Fix: new `just_started_item` flag set by `Tag::Item`, honored by `Tag::Paragraph` to suppress exactly that flush." — CHANGELOG.md (added PR #96)
Inferred intent: a pulldown-cmark event-ordering gotcha, not a design shift — tight lists were already correct because pulldown-cmark omits the Paragraph wrapper for them. evidence: src/ui/markdown.rs +25 declares `just_started_item: bool`, +47 the Paragraph handler checks `if self.just_started_item { self.just_started_item = false; ... }`, +59 `Tag::Item` sets it true; a loose-list regression test added at +75 (`loose_list_keeps_bullet_attached_to_item_text`).
                  confidence: high
Supersedes: (none — corrects a renderer event-ordering bug, no prior thread moment)

This is purely additive to the single-file renderer (src/ui/markdown.rs +50/-? ; only CHANGELOG + Cargo bump alongside). The flag lives "True for exactly one event after `Tag::Item`" (src/ui/markdown.rs +17 doc comment) — a minimal state machine fix rather than a rewrite of the bullet-rendering path.

Recorded rationale notes the trigger was dogfooding: BUGS.md itself is a loose list, so the maintainer's own bug tracker surfaced the defect.

Provenance:
- b0a321c (PR #96 fix/markdown-loose-list-bullet-attach, 2026-05-16) — src/ui/markdown.rs +50 (just_started_item flag on Tag::Item / Tag::Paragraph + loose-list test); CHANGELOG.md +13
- CHANGELOG.md (added PR #96) — quoted loose-list / pulldown-cmark Paragraph-wrapper rationale

<!-- Entry-ID: 01KTMMGRQ8WXQQA9Y0AWPS5X9Z -->

---
Entry: Claude Code (caleb) 2026-06-08T22:09:40.626674+00:00
Role: scribe
Type: Note
Title: PR #103 — markdown tables expand to pager width (table_width_hint)

Spec: scribe

tags: #history #markdown-rendering

Moment: markdown-rendering — Reconstructed: the markdown renderer gains an optional `table_width_hint` argument; `render()` now takes the actual pager body width and computes per-column caps proportionally (clamped 24..60) instead of hard-capping tables at the 80-column prose budget.   [kind: new-capability]
When: 2026-05-19 · PR #103 (feat/markdown-tables-use-pager-width) · commit 386b7bc
Recorded rationale: "Markdown tables expand to the pager body width. Tables used to be hard-capped at the 80-column prose budget with a 24-cell per-column ceiling, so on a wide terminal a two-column reference table looked cramped (~30-cell columns) with the right half of the pager empty. Now the renderer takes an optional width hint, the file-open path passes the actual pager body width (90% of terminal, minus borders), and the per-column cap is computed proportionally — clamped between 24 (the old minimum, preserves existing tight behavior on small terminals) and 60 (avoids 200-cell-wide single columns on ultrawides). Prose intentionally still wraps at 80 columns regardless of the hint — long prose lines are unpleasant to read." — CHANGELOG.md (added PR #103)
Inferred intent: separates two width budgets — tables follow the terminal, prose stays at CONTENT_WIDTH — establishing the `table_width_hint` plumbing that PR #108 then has to correct for the gutter and PR #110 reuses for prose. evidence: src/ui/markdown.rs signature change `pub fn render(source, theme, table_width_hint: Option<usize>)` (src/ui/markdown.rs +74), Renderer carries the hint (+82, +90 "Target total width for tables"); src/app/mod.rs +10 passes the pager body width at file-open.
                  confidence: high
Supersedes: PR #85's implicit single-width model — render() now distinguishes table width from prose width. The new signature is the seed the next two table/prose moments build on.

The diff reworks the doc comments that previously described a single CONTENT_WIDTH cap (src/ui/markdown.rs -33..-36 old "Maximum visual width of a single table column" → +44 "Hard ceiling on a single table column even with vast amounts of terminal real estate"). The 24/60 clamp is the explicit tradeoff: floor preserves small-terminal behavior, ceiling avoids ultrawide single-column blowups.

This is a fix-on-feature pair with PR #108 (gutter overflow), which discovers the hint didn't account for the line-number gutter. See that moment.

Provenance:
- 386b7bc (PR #103 feat/markdown-tables-use-pager-width, 2026-05-19) — src/ui/markdown.rs +81/-22 (render() table_width_hint arg, proportional per-column cap, 24/60 clamp); src/app/mod.rs +10 (pass pager body width at open); CHANGELOG.md +14
- CHANGELOG.md (added PR #103) — quoted table-width rationale incl. 24/60 clamp and prose-stays-80 caveat

<!-- Entry-ID: 01KTMMHF832JPTVJ91GGHG8AFM -->

---
Entry: Claude Code (caleb) 2026-06-08T22:10:03.891986+00:00
Role: scribe
Type: Note
Title: PR #107 — soft line breaks render as hard breaks

Spec: scribe

tags: #history #markdown-rendering

Moment: markdown-rendering — Reconstructed: the renderer overrides CommonMark soft-break handling so each source `\n` flushes a line; `Event::SoftBreak | Event::HardBreak => self.flush_line()` collapses both events to a hard break.   [kind: new-capability]
When: 2026-05-20 · PR #107 (feat/markdown-hard-line-breaks) · commit daae050
Recorded rationale: "Markdown soft line breaks render as hard breaks. CommonMark spec joins consecutive non-blank lines into one reflowed paragraph, which collapsed common patterns like `**To:** Alice\n**From:** Bob\n**Status:** Draft` into a single wrapped line. Now each source line renders on its own row — matches Discord / Slack / chat-style rendering and the way technical docs with `**Key:**` metadata expect to look. Prose authored at 80-col source wrap shows as several short lines instead of one reflowed paragraph; small trade for the metadata case actually working." — CHANGELOG.md (added PR #107)
Inferred intent: a deliberate deviation from CommonMark to fix the `**Key:** value` metadata-stack case, accepting a known regression (80-col prose breaks into short lines) that PR #110 lands three days later to undo. evidence: src/ui/markdown.rs -17..-21 old separate SoftBreak/HardBreak arms → +33 merged `Event::SoftBreak | Event::HardBreak => self.flush_line()`; regression test `soft_breaks_render_as_hard_breaks` at +42.
                  confidence: high
Supersedes: (none yet) — but the CHANGELOG itself flags the prose tradeoff, which becomes the problem statement for PR #110.

This is a small, intentional spec deviation (src/ui/markdown.rs +35/-7). The recorded rationale is unusually candid about the cost: it names the exact regression ("Prose authored at 80-col source wrap shows as several short lines") and calls it "small trade for the metadata case actually working." That self-flagged tradeoff is what PR #110 (reflow-prose) reverses — see the next moment, which supersedes this one.

Provenance:
- daae050 (PR #107 feat/markdown-hard-line-breaks, 2026-05-20) — src/ui/markdown.rs +35/-7 (SoftBreak|HardBreak → flush_line + test); CHANGELOG.md +11
- CHANGELOG.md (added PR #107) — quoted soft-break-as-hard-break rationale incl. self-flagged prose tradeoff

<!-- Entry-ID: 01KTMMJ3WCPSR7PH80MBES90JW -->
