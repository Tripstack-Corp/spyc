# Engagement records — code review and cleanup, Aug 2026

The briefs, proposals and deliverables from the two engagements that ran before
the pre-2.1 review (`docs/archive/review-2.1/`). Kept because the deliverables
name which PR closed which finding, which is the only place that mapping exists
outside individual commit messages.

| File | What it is | Outcome |
|---|---|---|
| `spyc-review-remediation-prompt.md` (+ `v2`, `v3`, `v4`) | The engagement brief, four drafts. `v4` is the one the deliverable cites. | superseded by the deliverable |
| `review-remediation-deliverable.md` | F1–F7, twelve PRs | all merged |
| `agents-md-trim-proposal.md` | F7 — AGENTS.md was 43 KB of always-loaded context | executed, #243 |
| `mouse-rs-decomposition-proposal.md` | F4 Part B — split `src/app/mouse.rs` | executed, #244–#246 |

## What is deliberately NOT archived here

Two documents stayed in `docs/drafts/` because they still carry unfinished work.
Moving them would have filed them as done:

- **`pane-identity-transport-proposal.md`** (C7) — a design proposal recommending
  pane id in the MCP `initialize` handshake. Never built. `pane_id` exists today
  only as an optional *tool argument* on `report_status` / `register_scope` /
  `release_scope` — which is the option the proposal explicitly rejects, because
  attribution then fails silently whenever an agent forgets to send it.
  `readers.rs` still documents the cursor-independent session-wide allowed set
  that per-pane roots were meant to retire.
- **`cleanup-engagement-deliverable.md`** — C1–C9 are merged, but its
  "Found along the way — not fixed" section lists seven items and they are not
  all closed. Item 2 asks SECURITY.md to state that pane attribution is not
  authorization — an env-supplied pane id is forgeable by the agent, and so is
  the per-pane socket, since both are owned by the same uid the agent runs as.
  SECURITY.md still does not say it.

## A note on the AGENTS.md trim

F7 was executed and the section it targeted did shrink — `## What it does` went
from 16,427 bytes to 8,272. The file as a whole did not: it was 43,256 bytes when
the proposal called it too large and is larger now, with `## Architecture` (the
guard-checked module index, plus its per-entry prose) grown from 10,895 to 20,457
and holding the largest share.

That is not an unclosed comment — the cut that was approved was made. It is a
fresh observation, recorded here so the next person to open the trim proposal
knows the finding is more true than when it was written, not less.
