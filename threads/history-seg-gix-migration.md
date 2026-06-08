# history-seg-gix-migration — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: history-seg-gix-migration
Created: 2026-06-08T22:09:43.979790+00:00

---
Entry: Claude Code (caleb) 2026-06-08T22:09:43.979790+00:00
Role: scribe
Type: Note
Title: PR #283 — the src/git/ facade seam (enabling precondition)

Spec: scribe

tags: #history #gix-migration

Moment: gix migration — Reconstructed: a `src/git/` facade module is carved out as the single boundary between spyc and any git backend; every `Command::new("git")` call is relocated verbatim behind it. No gix yet. [kind: refactor]
When: 2026-06-05 · PR #283 (refactor/gix-facade-seam) · commit 5adbb226 (squash); feature-branch commit 5196a706
Recorded rationale: "refactor(git): introduce src/git/ facade seam (no gix yet)" — commit 5196a706, 2026-06-05. The module doc (src/git/mod.rs) states the intent verbatim: "The single boundary between spyc and any git backend. Today every function here shells out to the `git` binary — relocated verbatim from `sysinfo.rs` / `app/util.rs` / `app/git_state.rs` so there is exactly one place that owns `Command::new("git")`. The gitoxide migration then swaps each backend *in place* behind this seam, one domain at a time, so the call sites never change again."
Inferred intent: this is the enabling precondition for a safe backend swap — collect the scattered subprocess git calls into one seam first, so subsequent PRs change only the implementation, never the call sites. Evidence: the diff moves code OUT of sysinfo.rs (-156), app/util.rs (-45), app/git_state.rs (-34 net) and INTO new src/git/{mod,status,diff,worktree}.rs (+269); it is a pure relocation with no behavior change. confidence: high
Supersedes: the pre-existing scattered git-subprocess sites in sysinfo.rs / app/util.rs / app/git_state.rs — consolidates ownership of `Command::new("git")` into src/git/. (cross-ref the older history-arc-04-git-integration thread, which covers the original subprocess git integration this seam now fences off.)

The facade is deliberately pure infrastructure: src/git/mod.rs notes it "takes paths and returns owned data / bytes. It has no `App` dependency and never touches ratatui, so `app` depends on `git` and never the reverse (the CLAUDE.md one-way dependency rule)." The PR also marks itself "PR 1/9 of gix migration" in the squash subject, establishing a planned 9-step arc. The new src/git/status.rs at this stage is still the subprocess backend (its `porcelain_raw` spawns `git status --porcelain -unormal`); the seam exists, the implementation is untouched. This is the strangler-fig scaffold: build the wrapper around the legacy organism before strangling it.

Provenance:
- 5adbb226 (PR #283 refactor/gix-facade-seam, 2026-06-05) — squash merge; +283/-235 across 13 files: creates src/git/{mod,status,diff,worktree}.rs, strips sysinfo.rs (-156), app/util.rs (-45), app/git_state.rs (-34)
- 5196a706 (feature-branch commit, 2026-06-05) — pre-squash subject "introduce src/git/ facade seam (no gix yet)"
- src/git/mod.rs (module doc) — quoted boundary/one-way-dependency rationale

<!-- Entry-ID: 01KTMMHJ879C24FY2WYYT9F1SF -->
