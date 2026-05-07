# history-overview — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: history-overview
Created: 2026-05-07T09:02:08.889240+00:00

---
Entry: Claude Code (caleb) 2026-05-07T09:02:08.889240+00:00
Role: scribe
Type: Note
Title: Framing: two-layer reconstruction of spyc's first 22 days

Spec: scribe

tags: #history #segmentation

Purpose: open the spine for a two-layer reconstruction of spyc's first 22 days of merged work — commits d9b9360 (PR #2, 2026-04-30) through a303251 (PR #37, 2026-05-07), 36 PRs total. Layer 1 will be a set of baseline arc threads, each narrating a chronological cluster of PRs in third-person observational voice — as if a watercooler scribe had been present while the work landed. Layer 2 will be analytic insight threads (drift, recurrence, trajectory, emergence) written after the baselines exist and citing arc-entry IDs by ULID. This entry on `history-overview` opens that spine; the segmentation proposal that follows lays out the arcs to be written.

Audience: Derek Marshall, the maintainer and sole author of these 36 PRs (`onboarding-team-map` confirms single-developer authorship over the window). The exercise is positioned to demonstrate that threading-of-intent over a real commit history has standalone value distinct from `git log` and `CHANGELOG.md` — a scribe-voice arc thread surfaces concerns that a date-ordered log of commit subjects flattens.

Voice contract for every arc and insight thread:
- Mode: third-person observational, present tense.
- Hedge whitelist (encouraged): appears to, reads as, consistent with, the diff shape suggests, points toward, aligns with, the commit message indicates.
- Banned in reference to the maintainer's mindset: wants, thinks, believes, decided, feels, intends to (without "the commit message"), is concerned that. No first-person reconstruction. No "I" or "we" speaking as the author.
- Verbatim commit-message quoting is encouraged; attribute as `(commit <sha>, <date>)`.

Provenance contract for every arc and insight thread:
- Every entry ends with a `Provenance:` block listing commit SHAs cited (with PR number and merge date), file:line spans cited where applicable, and cross-thread references (entry IDs).
- Entries missing the Provenance block are subject to observer challenge.

Inputs validated this session:
- `watercooler_health` against this code_path reports Healthy; Threads Repo URL `git@github.com:calebjacksonhoward/spyc.git`; Branch Parity clean.
- 36 merge commits read in chronological order via `git log --grep='Merged in' --reverse --format='%h %ai %s'` (the leading commit `d9b9360` carries PR #2 because PR #1 merged second by wall-clock; positional index and PR number diverge across the window — arcs cite real PR numbers from the `Merged in ... pull request #N` subject).
- Three onboarding seeds read before any write: `onboarding-overview`, `onboarding-product-charter`, `onboarding-architecture`. The seed `onboarding-architecture` describes the current end-state of the MCP-bridge, alt-screen, pager, and pane surfaces; arcs may reveal those surfaces as late convergences rather than design-from-the-start. Where an arc trajectory points toward late convergence on a surface the seed asserts as present, the arc thread will flag the gap explicitly rather than override the seed silently.

Closure for this thread is intentionally deferred: the spine remains OPEN for arc-session cross-references. No arc threads are written this session; only this spine.

Provenance:
- d9b9360 (PR #2 chore/ci-hygiene, 2026-04-30) — first commit in the 22-day window.
- a303251 (PR #37 fix/mcp-socket-project-scoped-discovery, 2026-05-07) — last commit in the 22-day window.
- onboarding-overview entry 0 = 01KR0NZNJ3KM6BJY09Q4P9D0NE
- onboarding-product-charter entry 0 = 01KR0P18MCE1H57Q5ZTAGKAJNH
- onboarding-architecture entry 0 = 01KR0P4W3ED1QZ8F44PFB2WPDZ
- `watercooler_health` output: server v0.4.6.dev0; Threads Repo URL `git@github.com:calebjacksonhoward/spyc.git`; Branch Parity clean ✓
- Command run for the chronological list: `git log --grep='Merged in' --reverse --format='%h %ai %s'`

<!-- Entry-ID: 01KR0TRFWT9W6WMFHC49YSW0BG -->
