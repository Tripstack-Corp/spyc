# history-synthesis — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: history-synthesis
Created: 2026-06-08T22:23:32.301521+00:00

---
Entry: Claude Code (caleb) 2026-06-08T22:23:32.301521+00:00
Role: scribe
Type: Note
Title: Phase 3 arc layer: how to read the #38–#311 synthesis (opener)

Spec: scribe

tags: #history #synthesis

Purpose: open the **Phase 3 arc layer** for the #38–#311 window. The per-moment `history-seg-*` / `history-arc-*` threads are the durable, queryable record — honest but not always readable as a subsystem-evolution *arc*. This thread holds one **readable arc entry per segment**, generated from that segment's committed moments. The arcs are **secondary by rule**: they summarize, never replace, the per-moment logs; every substantive claim in an arc carries an inline moment reference `[entry_id]`, recorded rationale may be quoted, and inferred reads stay marked `(inferred)`. No claim appears in an arc that is not already in the atomic layer.

Arc entry shape (per segment): `Arc: <segment> — <throughline>` · `Span: <first→last date · N moments · M supersessions>` · `Narrative` (2–5 short paragraphs, reconstruction voice, inline `[entry_id]` per claim) · `Lineage` (the supersession spine) · `Open/unsettled` · `Confidence` (recorded-vs-inferred carried up) · `Provenance` (every moment entry_id summarized).

The 13 arcs that follow correspond to the segment map in `history-overview` (segmentation entry 01KTMN9MRB31A0C8ZWSXX5M5ZQ): six new `history-seg-*` segments and seven extended `history-arc-*` continuations. Read `history-overview` first for the cross-segment topology; read an arc here for one subsystem's shape; follow an arc's `[entry_id]` links into the per-moment thread for the verifiable detail.

Provenance:
- history-overview second-window framing = 01KTMN7X7FV45E8E05DN1RV719; segment map = 01KTMN9MRB31A0C8ZWSXX5M5ZQ.
- This thread parallels the first window's `history-narrative-arc` (the #1–#37 synthesis).

<!-- Entry-ID: 01KTMNAVSB3410Y59S3FDMZCHQ -->
