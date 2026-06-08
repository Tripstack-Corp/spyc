# history-seg-performance — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: history-seg-performance
Created: 2026-06-08T22:09:58.124347+00:00

---
Entry: Claude Code (caleb) 2026-06-08T22:09:58.124347+00:00
Role: scribe
Type: Note
Title: PR #99 — Tier-0 git-poll mtime cache (stat-before-spawn)

Spec: scribe

tags: #history #performance

Moment: performance — Reconstructed: the 1 Hz git safety-poll short-circuits to a stat-only path when `.git/index` and `.git/HEAD` mtimes are unchanged, eliminating the per-second `git status` subprocess on quiet repos.   [kind: new-capability]
When: 2026-05-19 · PR #99 (perf/git-status-mtime-cache) · commit 68099128
Recorded rationale: "perf: skip 1Hz git poll subprocess when index/HEAD mtimes unchanged" — commit subject. CHANGELOG (added PR #99): "The 1 Hz git poll short-circuits when `.git/index` and `.git/HEAD` haven't moved... it was firing `git status --porcelain -unormal` every second regardless — cheap on a small tree, real CPU on a ~110k-file working tree... Cache the mtime pair... on the next tick stat both files and bail before any subprocess spawns if the mtimes match."
Inferred intent: opening move of the under-load campaign — make the idle polling cost proportional to actual repo change, not wall-clock. Evidence: diff is +85 lines in `src/app/state.rs`, +4 in `src/app/mod.rs`, no symbol removed (pure addition). confidence: high
Supersedes: (none) — the poll itself predates this slice; this caps its cost.

The poll is framed in the CHANGELOG as a deliberate safety net for "the rare case where FSEvents drops the `.git/index.lock` → `.git/index` atomic-rename notification (the macOS FSEvents inode-replacement edge case)." The fix narrows that net's cost rather than removing it: cache the (index mtime, HEAD mtime) pair from the last successful poll; next tick stats both and bails before any subprocess if they match. The CHANGELOG is explicit about scope — the cache is "scoped to the poll path only — the event-driven refresh (`refresh_listing` after fsevents debounce) still recomputes unconditionally because working-tree edits change file mtimes but NOT `.git/index`/`HEAD`, and a cache hit there would silently miss ` M filename` markers." Cache is reset on chdir.

This scoping caveat (poll-path cache must not leak into the event-driven refresh) becomes a recurring constraint across the campaign — see the throttle work in PR #137 and the starvation fix in PR #156, both of which re-touch the same `refresh_listing` invalidation boundary.

Provenance:
- 68099128 (PR #99 perf/git-status-mtime-cache, 2026-05-19) — `src/app/state.rs` +85 (mtime-pair cache + stat-before-spawn), `src/app/mod.rs` +4 (poll tick wiring), CHANGELOG +19.
- CHANGELOG.md (added PR #99) — quoted rationale above.

<!-- Entry-ID: 01KTMMHXYR7E0PYD7AZTHFAR8E -->
