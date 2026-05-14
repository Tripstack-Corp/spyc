# migration-github-origin-to-tripstack-corp — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: migration-github-origin-to-tripstack-corp
Created: 2026-05-14T21:15:26.201332+00:00

---
Entry: Claude Code (caleb) 2026-05-14T21:15:26.201332+00:00
Role: planner
Type: Plan
Title: First entry — plan to repoint github origin from calebjacksonhoward/spyc to Tripstack-Corp/spyc

Spec: planner-architecture

tags: #migration #github-remote #orphan-branch

# Context

The spyc clone has been dual-origin since the threads infrastructure landed: code lives on `git@bitbucket.org:tripstack/spyc.git` (Derek's PR review surface), and threads live on `git@github.com:calebjacksonhoward/spyc.git` (the personal-repo github mirror that watercooler-site is currently registered against). That setup carried the project through the v1.41 → v1.50 window and the PR #87 hang fix, but it has a structural problem: Derek wants to use Watercooler directly, not just receive thread summaries, and he can't onboard against `calebjacksonhoward/spyc` without authorizing a GitHub organization he doesn't have. The product gap is filed separately — see cross-references below — and Derek's workaround was to create the `Tripstack-Corp` GitHub organization, register `Tripstack-Corp/spyc` at `https://github.com/Tripstack-Corp/spyc`, and add Caleb as admin so this migration can be driven from Caleb's side.

This thread is the plan to move spyc's github remote from `calebjacksonhoward/spyc` to `Tripstack-Corp/spyc`, including the orphan threads branch, and to disengage the old repo from Watercooler so future MCP-driven entries land on the new repo.

# Pre-migration state (as of this entry)

- `bitbucket` remote → `git@bitbucket.org:tripstack/spyc.git` (unchanged by this migration).
- `origin` remote → `git@github.com:calebjacksonhoward/spyc.git` (to be repointed).
- `main` at `f06201f` on local, bitbucket, and origin (github) — fully aligned post-PR-87 squash-merge.
- `watercooler/threads` orphan branch at `fa17d89` (or later, since this entry's commit will advance it) on local and origin (github); bitbucket has the previous snapshot.
- Watercooler worktree at `/home/caleb/.watercooler/worktrees/spyc` tracking `origin/watercooler/threads`; the worktree's `.git` remote config currently resolves `origin` to the calebjacksonhoward URL.
- No project-level `.watercooler/config.toml` in the spyc tree — the global config at `~/.watercooler/config.toml` is used, no spyc-specific URL hardcoded.

# Decisions baked into the plan

- **Local remote naming post-migration:** repoint the existing `origin` URL to `Tripstack-Corp/spyc` (one `git remote set-url`). No rename of remotes. `bitbucket` stays. Tracking branches keep their names; only the URL `origin` resolves to changes.
- **Old repo (`calebjacksonhoward/spyc`) after migration:** leave it alone. No branch deletion. No archive. Disengagement from watercooler-site is sufficient to ensure no further MCP-driven entries land there.

# Migration sequence

The execution plan is captured in detail on the local plan file at `/home/caleb/.claude/plans/alrighty-an-interesting-new-drifting-micali.md`. Summary in catalogue voice:

1. Verify clean state — `main` aligned across local / bitbucket / origin (github); orphan branch aligned between local and origin.
2. Add `tripstack-corp` as a temporary remote pointing at `git@github.com:Tripstack-Corp/spyc.git`; fetch (the new repo is near-empty).
3. Push `main`, `watercooler/threads`, and tags (`v1.50.0` at minimum) to `tripstack-corp`. Verify via `git ls-remote tripstack-corp` that all three landed.
4. **User action:** log into watercooler-site, disengage `calebjacksonhoward/spyc`. After this, no further MCP-driven entries land on the calebjacksonhoward repo.
5. **User action:** log into watercooler-site, authorize Watercooler against the `Tripstack-Corp` organization, and register `Tripstack-Corp/spyc` as the tracked repo. This is also when Derek can onboard as a Tripstack-Corp member.
6. Repoint local `origin` URL to Tripstack-Corp/spyc (`git remote set-url origin …`). Fetch picks up the same commits we just pushed.
7. Update the watercooler worktree's remote URL to match the main clone's repointed `origin` so future MCP-driven entries push to Tripstack-Corp/spyc, not calebjacksonhoward.
8. Remove the temporary `tripstack-corp` remote (since `origin` now resolves to the same place).
9. Verify auto-push: post a test entry through MCP, confirm it lands on Tripstack-Corp/spyc and *not* on calebjacksonhoward/spyc.
10. (Optional) snapshot push the orphan branch to `bitbucket` for parity, same pattern as the previous one-off.

Steps 4 and 5 are user-driven web-UI actions; the rest is automatable from this session once approval and ordering are confirmed. The migration plan as a whole is reversible until step 4 — backing out before disengagement is a `git remote set-url` revert and a force-push to recover.

# Cross-references — the watercooler product gap that drove this

Derek's blocked onboarding surfaced a real product UX gap. Filed in four places:

- **GitHub issue (back-end / OAuth scope):** https://github.com/mostlyharmless-ai/watercooler-cloud/issues/797
- **GitHub issue (form / GUI):** https://github.com/mostlyharmless-ai/watercooler-site/issues/55
- **Watercooler thread (back-end):** `ux-collaborator-on-personal-repo-onboarding` on `mostlyharmless-ai/watercooler-cloud` — entry `01KRM5DM1AF276ES8X6QFX983P`.
- **Watercooler thread (front-end):** `gui-onboarding-repo-vs-org-authorization` on `mostlyharmless-ai/watercooler-site` — entry `01KRM5EER1KBJP0MB5AFCMG27C`.

The migration is a workaround for the gap; the GitHub issues + product threads are the product-side fix that closes it for future users in the same situation.

# Validation criteria for "migration complete"

- After step 3: `git ls-remote git@github.com:Tripstack-Corp/spyc.git` shows `refs/heads/main` at `f06201f` (or later), `refs/heads/watercooler/threads` at the latest entry SHA, and `refs/tags/v1.50.0`.
- After step 6: `git remote -v` shows `origin` resolving to `git@github.com:Tripstack-Corp/spyc.git` and `bitbucket` unchanged. `git rev-parse origin/main` returns the expected `main` SHA.
- After step 9: a test entry's commit appears on `Tripstack-Corp/spyc`'s orphan branch; `calebjacksonhoward/spyc`'s orphan branch is *unchanged* since the disengagement in step 4.
- End-to-end: Derek can complete watercooler-site onboarding as a Tripstack-Corp member, pick up `Tripstack-Corp/spyc` as a tracked repo, and post a thread entry. (This is the user-observable proof that the migration solved the original problem.)

# What this thread tracks from here

- Steps 1–3, 6–8, and the verification work after each: a follow-up entry recording what actually happened, any deviations from the plan, and the final repo state.
- The user-driven steps 4 and 5: an entry capturing when they happened and any surprises in the watercooler-site UI flow.
- Closure: an entry once Derek successfully posts his first entry from his own Watercooler session. That's the load-bearing proof, and the natural place to close out this thread.

Provenance:
- Local plan file: `/home/caleb/.claude/plans/alrighty-an-interesting-new-drifting-micali.md` (full executable detail).
- Pre-migration `main` SHA: `f06201f` (PR #87 squash, merged 2026-05-14).
- Pre-migration orphan SHA: `fa17d89` (this entry's commit will advance it).
- Linked product issues and threads: enumerated above.
- Earlier thread on this catalogue describing why threads live on github: implicit in the onboarding seeds and `history-overview`; explicit naming-of-the-split is in `human-exploration-of-nonhuman-threads`.
- Identity fallback: no `set_agent` tool surfaced this session; identity asserted via Role + Spec lines and `agent_func`.

<!-- Entry-ID: 01KRM5G544P02SNE96D6HWTEWX -->
