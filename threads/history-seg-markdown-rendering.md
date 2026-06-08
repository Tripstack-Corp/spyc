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
