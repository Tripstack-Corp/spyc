# history-three-repo-lineage — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: history-three-repo-lineage
Created: 2026-05-18T08:24:09.520319+00:00

---
Entry: Claude Code (caleb) 2026-05-18T08:24:09.520319+00:00
Role: scribe
Type: Note
Title: Why spyc has three git remotes: Bitbucket origin, personal-GH bridge, Tripstack-Corp future home

Spec: docs

A durable explainer for the three git remotes the local spyc clone tracks. Written so a future agent or onboarding human can answer "why are there three?" without trawling commits.

## TL;DR

| Remote | URL | Role | Visibility | Created | Status |
|---|---|---|---|---|---|
| `bitbucket` | `git@bitbucket.org:tripstack/spyc.git` | **Original** Tripstack-internal repo · Derek's PR review surface | Tripstack-internal | (predates GH mirrors) | Active dev — 8 PRs ahead of GH (#89–#96) |
| `origin` | `git@github.com:calebjacksonhoward/spyc.git` | **Temporary** WC threads-sync target · personal GH mirror | Private | 2026-05-07 | Current WC-registered namespace |
| `tripstack-corp` | `git@github.com:Tripstack-Corp/spyc.git` | **Intended** permanent threads-sync home · org-owned GH | Private | 2026-05-14 | Awaiting WC OAuth authorization |

## Why three remotes?

The short story:

1. **`bitbucket` is the original.** Tripstack's internal code repo. Derek reviews PRs here. This is where development actually happens — features and fixes land via Bitbucket pull requests first.

2. **`origin` (`calebjacksonhoward/spyc`) exists because Watercooler currently only registers GitHub namespaces.** Bitbucket is out of WC's scope, so to host the `watercooler/threads` orphan branch (and have it appear on a WC dashboard), the threads needed a *GitHub* home. Derek didn't have a personal-account GitHub org he could use, and Watercooler's OAuth couldn't onboard a `tripstack`-org Bitbucket. So Caleb (`calebjacksonhoward`) registered the namespace as a temporary bridge to host the threads orphan branch.

3. **`tripstack-corp` (`Tripstack-Corp/spyc`) is the intended permanent home.** Derek created the `Tripstack-Corp` GitHub organization on 2026-05-14 so the threads-sync repo can eventually live under a Tripstack-controlled namespace instead of a personal account. Caleb is admin so the threads-sync migration can be driven from Caleb's side. The migration is *blocked on Watercooler-upstream OAuth scope* — the WC web app currently can't grant access to an org that the registering user isn't already a personal-account member of in the right way.

## The Watercooler product gap (upstream)

This three-repo arrangement is a **workaround for a Watercooler limitation**, not an idealized architecture. The relevant upstream issues:

- **Back-end OAuth scope:** [`mostlyharmless-ai/watercooler-cloud#797`](https://github.com/mostlyharmless-ai/watercooler-cloud/issues/797) — onboarding an org-owned repo when the onboarding user isn't already authorized through the org's GitHub installation.
- **Form/GUI:** [`mostlyharmless-ai/watercooler-site#55`](https://github.com/mostlyharmless-ai/watercooler-site/issues/55) — the dashboard form for switching the registered repo.

The migration plan thread [`migration-github-origin-to-tripstack-corp`](https://watercoolerdev.com/dashboard?repo=calebjacksonhoward%2Fspyc&branch=*&thread=migration-github-origin-to-tripstack-corp) (entry `01KRM5G544P02SNE96D6HWTEWX`, 2026-05-14) is the canonical record of the migration intent and quotes Derek's framing of the workaround.

## Current sync state (as of 2026-05-18, captured live)

`main` branch:

| Remote | Tip SHA | Drift vs origin/main |
|---|---|---|
| `bitbucket/main` | `b0a321c` | **+8 commits** (PRs #89–#96 not yet on GH) |
| `origin/main` | `f06201f` | baseline (PR #87) |
| `tripstack-corp/main` | `f06201f` | 0 (in sync with origin) |

The drift on `bitbucket/main` reflects that *Bitbucket is the active dev surface*; the GitHub mirrors lag and get caught up periodically. This is one-way: GitHub never accepts pushes that aren't first landed on Bitbucket.

`watercooler/threads` orphan branch (the threads data branch):

| Remote | Tip SHA |
|---|---|
| `origin` | `8b8900c` |
| `bitbucket` | `8b8900c` |
| `tripstack-corp` | `8b8900c` |

All three in lock-step. WC MCP auto-pushes to `origin`; subsequent manual pushes fan out to `bitbucket` and `tripstack-corp` to keep parity. (Note: WC threads on the dashboard read from `origin` = `calebjacksonhoward/spyc`; the other two are mirrors with no dashboard hookup yet.)

`ci/drop-default-pipeline` (a CI cleanup branch):

| Remote | Tip SHA |
|---|---|
| `origin` | `714684e` |
| `bitbucket` | `714684e` |
| `tripstack-corp` | absent |

Minor — not yet pushed to `tripstack-corp`. Flag, not fix.

## Operational implications

When the upstream WC OAuth is fixed and Derek (or Tripstack IT) authorizes `Tripstack-Corp/spyc` as the WC repo namespace:

- `origin` will be repointed from `calebjacksonhoward/spyc` to `Tripstack-Corp/spyc` (per the migration plan thread).
- The WC dashboard URL will change from `repo=calebjacksonhoward%2Fspyc` to `repo=Tripstack-Corp%2Fspyc`.
- The `calebjacksonhoward/spyc` repo becomes archivable.
- Bitbucket continues as the code dev surface; the only GH change is which org owns the WC-registered mirror.

## Code repo

- GitHub: [Tripstack-Corp/spyc](https://github.com/Tripstack-Corp/spyc) — description: *"A Rust TUI file commander where the AI agent in the side pane can query the file commander itself"*, private, created 2026-05-14.
- GitHub: [calebjacksonhoward/spyc](https://github.com/calebjacksonhoward/spyc) — no description, private, created 2026-05-07.
- Bitbucket: `tripstack/spyc` — Tripstack-internal, predates both GH mirrors, is the canonical PR review surface.

## Cross-links

- Migration plan: [`migration-github-origin-to-tripstack-corp`](https://watercoolerdev.com/dashboard?repo=calebjacksonhoward%2Fspyc&branch=*&thread=migration-github-origin-to-tripstack-corp).
- WC upstream OAuth issue: [`mostlyharmless-ai/watercooler-cloud#797`](https://github.com/mostlyharmless-ai/watercooler-cloud/issues/797).
- WC upstream form/GUI issue: [`mostlyharmless-ai/watercooler-site#55`](https://github.com/mostlyharmless-ai/watercooler-site/issues/55).

<!-- Entry-ID: 01KRX2YRNMPARTPFK3CW3R50FG -->
