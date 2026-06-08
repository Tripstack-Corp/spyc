# history-seg-refactor-mvu — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: history-seg-refactor-mvu
Created: 2026-06-08T21:56:29.259875+00:00

---
Entry: Claude Code (caleb) 2026-06-08T21:56:29.259875+00:00
Role: scribe
Type: Note
Title: PR #166 — let-chains MSRV sweep (refactor prep)

Spec: scribe

tags: #history #refactor-mvu

Moment: refactor-mvu — Reconstructed: a tree-wide `if let` → `if-let-chains` rewrite bumps the MSRV gate and collapses nested guard pyramids ahead of the decomposition campaign.   [kind: convention]
When: 2026-05-29 · PR #166 (refactor/let-chains-sweep) · commit d75cab3
Recorded rationale: "refactor: adopt if-let chains across the tree (MSRV 1.85 → 1.88)" — commit d75cab3 subject
Inferred intent: a flatten-the-control-flow prep pass landing the day before the Phase-1 extractions begin (#180, 2026-05-30); the diff shape — `if !chord_locked { if let Some(action) = user.find(&ev) {` → `if !chord_locked && let Some(action) = user.find(&ev) {` in `src/keymap/resolver.rs` — is mechanical guard-collapse across 21 files (551+/565-), consistent with shrinking nesting depth before extracting handlers. — evidence: ledger date 2026-05-29 = one day pre-#180; diff touches resolver/mcp/pager/sessions/tabs broadly with no behavior change.   confidence: high
Supersedes: (none)

Reconstructed: this is the lead-in to the `src/app/` decomposition. It is not itself MVU work, but it raises the language floor (Rust 1.88 let-chains) and de-nests guard logic across the tree, which makes the verbatim handler moves in Phase 1 cleaner. Largest single touch is `src/mcp.rs` (201 lines reshaped) and `src/ui/pager.rs` (64). No new types, no decision rationale beyond the subject — folded as the prep moment of this thread.

Provenance:
- d75cab3 (PR #166 refactor/let-chains-sweep, 2026-05-29) — `if-let` → let-chain collapse across 21 files; sample `src/keymap/resolver.rs` merges a nested `if let` into one `&&`-chained guard; +551/-565, zero behavior change.

<!-- Entry-ID: 01KTMKSAG0R0FQ70HRQW1H4EKW -->
