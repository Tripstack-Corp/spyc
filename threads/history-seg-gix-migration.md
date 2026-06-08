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

---
Entry: Claude Code (caleb) 2026-06-08T22:10:15.522732+00:00
Role: scribe
Type: Note
Title: PR #284 — add the gix (gitoxide) dependency, lean/pure-Rust/C-free

Spec: scribe

tags: #history #gix-migration

Moment: gix migration — Reconstructed: the `gix` crate (v0.84) is added to Cargo.toml with `default-features = false` and a hand-trimmed feature set; ~40 default features and all networking/credentials stay off. [kind: new-capability]
When: 2026-06-05 · PR #284 (feat/gix-dependency) · commit 70db0671 (squash); feature-branch commit b122b812
Recorded rationale: "feat(git): add gix (gitoxide) dependency — lean, pure-Rust, C-free" — commit b122b812, 2026-06-05. The Cargo.toml comment block is the verbatim design rationale: "Git plumbing — the gitoxide crate. Pure-Rust (zlib-rs is the default compression backend now; no C, no `*-sys`, so static musl builds stay clean). Replaces the `git` subprocess shell-outs behind the `src/git/` facade, one domain at a time. `default-features = false` + a trimmed set (status / diff / blame / discovery, plus `parallel` for the multi-threaded status walk that keeps huge-tree refreshes cheap); networking, credentials, worktree archive/stream, progress reporting, and ~40 other default features stay off. `worktree-mutation` gets added when worktree create/remove migrates (PR 6)."
Inferred intent: the C-free / static-musl motivation reads as a packaging concern — a pure-Rust git lib removes the runtime dependency on a `git` install and keeps static builds clean, which a subprocess approach cannot. The feature trimming pre-plans the migration order (status/diff/blame/discovery now; worktree-mutation deferred to "PR 6"). Evidence: Cargo.toml:42 enabled features [sha1, status, dirwalk, blame, blob-diff, revision, index, attributes, excludes, parallel]; Cargo.lock +994 lines. confidence: high
Supersedes: (none) — additive; this only makes the backend available. The facade call sites still run subprocess git at this point.

The dependency add is pure plumbing with zero call-site change (only Cargo.toml +21 and Cargo.lock +994). The Cargo.toml comment is unusually explicit forward planning: it names the exact PR ("PR 6") where `worktree-mutation` will be enabled, which matches the eventual worktree PR #288. `parallel` is justified by the status hot path ("multi-threaded status walk that keeps huge-tree refreshes cheap"), connecting to the existing background git worker.

Provenance:
- 70db0671 (PR #284 feat/gix-dependency, 2026-06-05) — squash merge; Cargo.toml +21, Cargo.lock +994
- b122b812 (feature-branch commit, 2026-06-05) — pre-squash subject "add gix (gitoxide) dependency — lean, pure-Rust, C-free"
- Cargo.toml:30-52 (at 70db0671) — quoted gix dependency comment block

<!-- Entry-ID: 01KTMMJB955RF16D842WFFEBYP -->
