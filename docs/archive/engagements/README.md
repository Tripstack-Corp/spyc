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
| `cleanup-engagement-deliverable.md` | C1–C9, eight PRs | all merged; filed here 2026-08-19, see its banner |

## What is deliberately NOT archived here

One document stays in `docs/drafts/` because it still carries unstarted work.
Moving it would file it as done:

- **`pane-identity-transport-proposal.md`** (C7) — a design proposal
  recommending pane id in the MCP `initialize` handshake. Accepted for 2.2
  (`docs/drafts/V2_2_PLAN.md`), not yet built. `pane_id` exists today only as
  an optional *tool argument* on `report_status` / `register_scope` /
  `release_scope` — which is the option the proposal explicitly rejects,
  because attribution then fails silently whenever an agent forgets to send it.
  `readers.rs` still documents the cursor-independent session-wide allowed set
  that per-pane roots were meant to retire.

`cleanup-engagement-deliverable.md` was held out for the same reason until
2026-08-19. Its item 2 — SECURITY.md should say pane attribution is not
authorization — was closed by #429, and item 3 (spyc's DECCKM gap) by #259.
The three that remain are standing observations, not engagement work; its
banner names them.

## A note on the AGENTS.md trim

F7 was executed and the section it targeted did shrink — `## What it does` went
from 16,427 bytes to 8,272. The file as a whole did not, and then grew: it was
43,256 bytes when the proposal called it too large, and had reached 52,084 with
`## Architecture` (the guard-checked module index, plus its per-entry prose) at
20,457 and holding the largest share. A second cut (#429) took the file to
**47,752** and `## Architecture` to 15,971 by moving per-entry mechanics into
ARCHITECTURE.md while keeping every module name — still above the 43 KB that
prompted the proposal, and still the largest single section.

That is not an unclosed comment — both cuts that were approved were made. It is
a standing observation, recorded here so the next person to open the trim
proposal knows the finding survives two rounds of it.
