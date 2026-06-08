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

---
Entry: Claude Code (caleb) 2026-06-08T22:10:42.606566+00:00
Role: scribe
Type: Note
Title: PR #285 — first real swap: repo discovery (gitdir + branch) on gix

Spec: scribe

tags: #history #gix-migration

Moment: gix migration — Reconstructed: the first domain actually swapped from subprocess to gix — repository discovery (locating the gitdir + reading the current branch) — lands in a new src/git/discovery.rs and is deleted from sysinfo.rs. [kind: supersession]
When: 2026-06-05 · PR #285 (feat/gix-discovery) · commit ef4fc297 (squash); feature-branch commit 8e0c70b5
Recorded rationale: "feat(git): migrate repo discovery (gitdir + branch) to gix" — commit 8e0c70b5, 2026-06-05. (squash subject: "PR 3/9 of gix migration" implied by the arc; detailed body is on the feature branch.)
Inferred intent: discovery is chosen as the first real swap because it is the lowest-risk domain — a read-only lookup with a small, stable output (a path + a branch name) and no porcelain-parsing surface. Migrating it first proves the gix wiring end-to-end before the riskier status hot path. Evidence: +115 src/git/discovery.rs, -99 sysinfo.rs; call sites in app/bootstrap.rs and app/state.rs rewired (state.rs +32/-... net). confidence: med (lowest-risk-first ordering is inferred from the diff scope; the arc comment in Cargo.toml at #284 listed discovery among the trimmed feature set)
Supersedes: the subprocess discovery path in sysinfo.rs (removed -99) — first concrete strangle of the legacy backend established by the facade seam in PR #283.

This is where the parity-then-flip arc begins in earnest, though discovery is swapped outright rather than run in parallel — its output is small enough that a side-by-side parity spike was apparently unnecessary (inferred from the absence of any parity-test module in this diff, in contrast to the status PR that follows). The src/git/mod.rs gains `pub mod discovery;` (+1). Call sites in app/bootstrap.rs (+2/-2) and app/state.rs are the only consumers touched, confirming the facade seam held: the migration changed the implementation, not the callers.

Provenance:
- ef4fc297 (PR #285 feat/gix-discovery, 2026-06-05) — squash merge; +136/-115 across 6 files: new src/git/discovery.rs (+115), sysinfo.rs (-99), app/state.rs/bootstrap.rs rewired
- 8e0c70b5 (feature-branch commit, 2026-06-05) — pre-squash subject "migrate repo discovery (gitdir + branch) to gix"

<!-- Entry-ID: 01KTMMKCGA2NKTRHBH3VPW37AJ -->

---
Entry: Claude Code (caleb) 2026-06-08T22:11:15.365772+00:00
Role: scribe
Type: Decision
Title: PR #286–#287 — status parity spike, then flip the hot path to gix

Spec: scribe

tags: #history #gix-migration

Moment: gix migration — Reconstructed: the canonical parity→flip pattern. PR #286 builds a gix `repo_status` backend ALONGSIDE the subprocess one and proves byte-for-byte equivalence with assert-equal parity tests; PR #287 then flips the status hot path to gix by default, gated by an `SPYC_GIT_BACKEND=subprocess` escape hatch. [kind: supersession]
When: 2026-06-05 · PR #286 (feat/gix-status-parity) commit 95971ecc · and PR #287 (feat/gix-status-flip) commit 5edcc569 (squashes ef4fc297→68ce2680, 9e66cd06)
Recorded rationale:
  - #286: "feat(git): gix status parity spike — alongside subprocess, assert-equal" — commit 95971ecc. src/git/status.rs doc verbatim: "PR 4 is a parity *spike*: this backend runs only from the parity tests, not the live status path (still the subprocess + `parse_porcelain_statuses`)." And: "subprocess truth by the parity tests below."
  - #287: "feat(git): flip the status hot-path to gix (worker + cache)" — commit 5edcc569. CHANGELOG.md verbatim: "Git status now uses gix (gitoxide) instead of shelling out to `git status`... Output is validated byte-for-byte against `git status --porcelain` across a parity corpus, so markers are unchanged. Safety valve for the rollout: set `SPYC_GIT_BACKEND=subprocess` to revert the status backend to the legacy `git` subprocess (a temporary escape hatch; it will be removed once gix status has soaked)."
Inferred intent: this two-step is the risk-management spine of the migration — never flip a hot path blind. #286 introduces a shared intermediate (`decode_porcelain` → `StatusEntry` → `map_to_listing`) that BOTH backends feed, so the gix output can be diffed against `git status --porcelain` truth in `mod parity_tests` before anything live changes. #287 swaps the default and adds `repo_status_entries(repo_root, listing_dir)` which branches on `subprocess_backend()`; the gix path is now the worker's default. Evidence: status.rs +598/-... at #286 introduces `repo_status` using gix::status plumbing (tree_index_track_renames, index_worktree_rewrites at 0.5 similarity); #287 status.rs +185 adds the env-gated dispatch + worker rewire (app/state.rs +75/-..., -149 sysinfo.rs). confidence: high
Supersedes: the subprocess status path (`porcelain_raw` + `parse_porcelain_statuses` in sysinfo.rs). #287 reduces it to a flag-gated fallback ("flip is a flag flip, not a code restore"); sysinfo.rs loses -149 lines. Builds on the facade seam (entry 01KTMMHJ879C24FY2WYYT9F1SF) and the dependency add (01KTMMJB955RF16D842WFFEBYP).

The gix `repo_status` deliberately matches subprocess defaults: `untracked_files(Collapsed)` to mirror `git`'s collapsed `?? sub/` directory output, and `gix_diff::Rewrites::default()` (0.5 rename similarity, "in fact gix's default", "matching"). #287's escape-hatch comment frames the rollout philosophy verbatim: "a one-release-cycle safety valve so a field regression in the gix flip is a flag flip, not a code restore. Removed in PR 9." This is the throughline of the whole segment — parity, then flip, then (eventually, PR #292 = "PR 9") drop.

Provenance:
- 68ce2680 / 95971ecc (PR #286 feat/gix-status-parity, 2026-06-05) — status.rs +598 (gix repo_status + parity_tests + shared decode_porcelain/map_to_listing), sysinfo.rs -116
- 9e66cd06 / 5edcc569 (PR #287 feat/gix-status-flip, 2026-06-05) — status.rs +185 (repo_status_entries env-gated dispatch, subprocess_backend()), app/state.rs +75, sysinfo.rs -149, CHANGELOG.md +9
- src/git/status.rs (parity spike doc + escape-hatch doc) — quoted
- CHANGELOG.md (at 5edcc569) — quoted gix-status rollout entry

<!-- Entry-ID: 01KTMMMBVCGADASG74RTHKWY97 -->
