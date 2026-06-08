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

---
Entry: Claude Code (caleb) 2026-06-08T22:12:07.081193+00:00
Role: scribe
Type: Note
Title: PR #288 — worktree create/list/remove on gix; per-repo .worktrees/ grouping

Spec: scribe

tags: #history #gix-migration

Moment: gix migration — Reconstructed: worktree operations (create/list/remove) migrate to gix; the `worktree-mutation` gix feature is enabled as pre-planned, and the on-disk layout changes to group worktrees under a per-repo `<repo>.worktrees/<branch>` dir. [kind: supersession]
When: 2026-06-06 · PR #288 (feat/gix-worktree) · commit b9dcdfa0 (squash); feature-branch commit a7a06ce5
Recorded rationale: "feat(git): group worktrees under <repo>.worktrees/<branch>" — commit a7a06ce5, 2026-06-06. CHANGELOG.md verbatim: "`W n` now groups worktrees under a per-repo `<repo>.worktrees/<branch>` dir (e.g. `~/src/foo.worktrees/feature`) instead of a bare `<repo_parent>/<branch>` sibling — no more cluttering the parent dir or colliding with unrelated same-named dirs. Part of the git → gix migration."
Inferred intent: two changes ride together here — the backend swap (subprocess `git worktree` → gix worktree-mutation) and a UX/layout fix (group worktrees in a dedicated per-repo dir to avoid parent-dir clutter and same-name collisions). The Cargo.toml comment from PR #284 had pre-committed to enabling `worktree-mutation` "when worktree create/remove migrates (PR 6)", and this is PR 6 of the 9-step arc. Evidence: src/git/worktree.rs +743/-105 (the largest single-file change in the segment), Cargo.toml +5 (worktree-mutation feature), Cargo.lock +29. confidence: high
Supersedes: the subprocess `git worktree` calls in src/git/worktree.rs (the verbatim-relocated subprocess version from PR #283) — replaced with gix mutation. Also supersedes the old bare-sibling worktree layout with the grouped `<repo>.worktrees/<branch>` scheme.

worktree.rs nearly quadruples (125 → ~760 LoC), consistent with worktree mutation requiring more in-process bookkeeping than a `git worktree add/remove` shell-out (ref resolution, index/HEAD setup). The feature-flag discipline from #284 pays off exactly here: the heavier `worktree-mutation` feature stays off until this PR needs it, keeping earlier builds lean.

Provenance:
- b9dcdfa0 (PR #288 feat/gix-worktree, 2026-06-06) — squash merge; src/git/worktree.rs +743/-105, Cargo.toml +5, Cargo.lock +29, CHANGELOG.md +12
- a7a06ce5 (feature-branch commit, 2026-06-06) — pre-squash subject "group worktrees under <repo>.worktrees/<branch>"
- CHANGELOG.md (at a7a06ce5) — quoted worktree-grouping entry
- Cargo.toml:41 comment (at #284) — pre-planned "worktree-mutation gets added when worktree create/remove migrates (PR 6)"

<!-- Entry-ID: 01KTMMNW4ZDF0R1C5631XRD593 -->

---
Entry: Claude Code (caleb) 2026-06-08T22:12:52.450035+00:00
Role: scribe
Type: Note
Title: PR #289 — gix diff/show/blame data model (no UI flip yet)

Spec: scribe

tags: #history #gix-migration

Moment: gix migration — Reconstructed: an in-process gix data model for diff/show/blame lands (src/git/diff_model/ + src/git/blame.rs + src/git/model.rs) — ~1900 lines of new producers, with the UI still on the old path. The model layer of a model→render→wire sub-arc. [kind: new-capability]
When: 2026-06-06 · PR #289 (feat/gix-diff-model) · commit dd518f58 (squash); feature-branch commit (feat/gix-diff-model tip)
Recorded rationale: "feat(git): gix diff/show/blame data model (no UI flip yet)" — feature-branch commit, 2026-06-06. The "(no UI flip yet)" parenthetical is the load-bearing signal: this PR builds the structured model only.
Inferred intent: diff/show/blame is the last and richest domain, so it is split into three PRs mirroring the status parity→flip discipline — first an isolated, testable data model (#289), then a pure renderer (#290), then the wiring/flip (#291). Building the model in isolation lets the gix diff output be validated before any pixel of UI depends on it, the same alongside-then-flip caution used for status. Evidence: new src/git/diff_model/{mod,build,blob}.rs (+547/+587/+359), src/git/blame.rs (+244), src/git/model.rs (+185); src/git/diff.rs only +7 (subprocess diff still intact); zero src/ui/ changes in this PR. confidence: high
Supersedes: (none yet) — additive model layer alongside the still-live subprocess diff/show/blame in src/git/diff.rs. Supersession is deferred to the wire PR #291.

The split into diff_model/build.rs (constructing the diff from gix blob-diff) and diff_model/blob.rs (blob fetch/decode) plus a standalone model.rs (+185) and blame.rs (+244) reads as separating the gix plumbing (produce structured diff/blame data) from any presentation concern — consistent with the facade's pure-infrastructure rule established in PR #283 (no ratatui in src/git/). The `blob-diff` and `blame` gix features enabled back in #284 are what this PR consumes.

Provenance:
- dd518f58 (PR #289 feat/gix-diff-model, 2026-06-06) — squash merge; +1930/-4 across 8 files: new src/git/diff_model/{mod,build,blob}.rs, src/git/blame.rs (+244), src/git/model.rs (+185), src/git/diff.rs (+7), src/git/mod.rs (+3)
- feat/gix-diff-model branch tip — pre-squash subject "gix diff/show/blame data model (no UI flip yet)"

<!-- Entry-ID: 01KTMMPRPTSP0J1MTJCT3WW6N1 -->

---
Entry: Claude Code (caleb) 2026-06-08T22:14:02.652087+00:00
Role: scribe
Type: Note
Title: PR #290–#291 — in-house diff/show/blame renderer, then wire it live

Spec: scribe

tags: #history #gix-migration

Moment: gix migration — Reconstructed: the render→wire half of the diff sub-arc. PR #290 adds a pure in-house diff/show/blame renderer (src/ui/diff_render.rs + blame_render.rs) that is "not yet wired"; PR #291 wires the gix model + renderer into the live pager, adding word-level diff highlighting and a side-by-side toggle. [kind: supersession]
When: 2026-06-06 · PR #290 (feat/gix-diff-render) commit 0aa4e929 · PR #291 (feat/gix-diff-wire) commit f31582b9
Recorded rationale:
  - #290: "feat(ui): in-house diff/show/blame renderer (pure; not yet wired)" — feature-branch commit, 2026-06-06. "(pure; not yet wired)" continues the model→render→wire staging.
  - #291: "feat(ui): word-level diff highlighting + calmer washes" — feature-branch commit 755d4ac4. CHANGELOG.md verbatim: "Modified lines get word-level highlighting — the whole line gets a dim wash and the actually-changed tokens a brighter tint... Press `|` in the pager to toggle between side-by-side and the classic unified view; narrow terminals fall back to unified automatically. Blame gets a per-commit-colored author/date gutter."
Inferred intent: #290 keeps the renderer pure and unwired so it can be built and (presumably) snapshot-tested in isolation before replacing the live view — the same caution as the status parity spike, applied to UI. #291 is the flip: it consumes the gix diff_model (#289) through the new renderer (#290) in the live pager, supersedes the old subprocess-fed diff view, and adds capabilities the subprocess `git diff` pager could not cheaply offer (word-level token highlighting, side-by-side, themed blame gutter) because spyc now owns the structured diff in-process. Evidence: #290 is src/ui-only (diff_render.rs +804, blame_render.rs +182, theme.rs +111, config +25) with no app/ wiring; #291 rewires app/git_state.rs (+176/-...), adds app/git_view_session.rs (+442), touches pager.rs (+15), diff_render.rs (+319). confidence: high
Supersedes: the subprocess-fed diff/show/blame view path through the pager. #291 routes the live view through gix diff_model + the in-house renderer; src/git/diff.rs shrinks (-16 net) as the renderer takes over presentation. Builds on the diff model (entry 01KTMMPRPTSP0J1MTJCT3WW6N1) and the facade seam (01KTMMHJ879C24FY2WYYT9F1SF).

The new app/git_view_session.rs (+442) reads as the stateful glue between a gix diff/blame model and the pager (scroll, side-by-side toggle, large-diff handling), separating session state from the pure renderer — consistent with the project's pure-infrastructure / pure-render layering. The side-by-side toggle (`|`) and narrow-terminal fallback are presentation features unlocked by owning the structured diff rather than piping pre-rendered `git diff` text.

Provenance:
- 0aa4e929 (PR #290 feat/gix-diff-render, 2026-06-06) — squash merge; +1125/-1 across 7 files: src/ui/diff_render.rs (+804), blame_render.rs (+182), theme.rs (+111), config/default.spycrc.toml (+11), config/mod.rs (+14), ui/mod.rs (+2)
- f31582b9 / 755d4ac4 (PR #291 feat/gix-diff-wire, 2026-06-06) — squash merge; +980/-130 across 20 files: app/git_view_session.rs (+442), app/git_state.rs (+176), ui/diff_render.rs (+319), pager.rs (+15), theme.rs (+31), CHANGELOG.md (+15)
- CHANGELOG.md (at 755d4ac4) — quoted word-level highlighting / side-by-side toggle / blame gutter entry

<!-- Entry-ID: 01KTMMR7X2GJAMQE0HD0C0HA6R -->

---
Entry: Claude Code (caleb) 2026-06-08T22:14:37.904543+00:00
Role: scribe
Type: Decision
Title: PR #292 — drop the last git subprocess; guard test; release 1.56.0

Spec: scribe

tags: #history #gix-migration

Moment: gix migration — Reconstructed: the terminal supersession. The last `Command::new("git")` call sites are removed, the `SPYC_GIT_BACKEND=subprocess` escape hatch is deleted, and a `no_subprocess_git_in_production` guard test is added that fails if any production source spawns `git`. Released as 1.56.0. [kind: supersession]
When: 2026-06-06 · PR #292 (refactor/gix-drop-subprocess) · commit b841bcf7 (squash); feature-branch commit db4c293c
Recorded rationale: "refactor(git): drop the last git subprocess + release 1.56.0" — commit db4c293c, 2026-06-06. CHANGELOG.md [1.56.0] verbatim: "spyc no longer runs the `git` binary at all. The git → gix (gitoxide) migration is complete: repo discovery, status, diff, show, blame, and worktree create/list/remove all run in-process via the pure-Rust `gix` crate — no more `git` subprocess spawns, fragile porcelain parsing, or dependency on a `git` install at runtime. A guard test enforces zero `git`-subprocess usages in production code. (The `git` binary is still used to build fixtures when running spyc's own test suite.)"
Inferred intent: this is "PR 9" of the arc — the close of the strangler-fig. With every domain on gix, the subprocess fallbacks are now dead weight, so they are deleted and replaced by an executable invariant. The guard test (src/git/mod.rs `mod no_subprocess_git_in_production`) is described in its own doc as the "Strangler-fig closing guard: production code must never spawn the `git` binary — every git operation runs in-process via gix"; it scans each source file's production portion (everything before the first `#[cfg(test)]` marker) for git-subprocess usage, allowing test fixtures to still construct throwaway repos with real `git`. Evidence: removes `porcelain_raw`, `subprocess_backend()`, `repo_status_entries` env-branch from status.rs (-90), deletes src/git/diff.rs subprocess producers (-95, four `Command::new("git")` sites), trims pager.rs (-62), bootstrap.rs/state.rs (-net); Cargo.toml/Cargo.lock version bump (+/-2). confidence: high
Supersedes: the entire subprocess git backend — closes the chain begun at the facade seam (PR #283, entry 01KTMMHJ879C24FY2WYYT9F1SF). Directly removes the `SPYC_GIT_BACKEND=subprocess` escape hatch introduced at the status flip (PR #287, entry 01KTMMMBVCGADASG74RTHKWY97), exactly as that PR's doc promised ("Removed in PR 9"). Verified via pickaxe: the removed lines include `pub fn porcelain_raw`, `fn subprocess_backend`, and four `Command::new("git")` call sites in src/git/diff.rs.

The net diff is -178 (+92/-270): the migration ends by removing more than it adds, the signature of a completed strangler-fig. The promise made in the #287 CHANGELOG ("it will be removed once gix status has soaked") and the #287 doc ("Removed in PR 9") are both kept here — the one-release-cycle safety valve lasted exactly the planned window. The guard test converts the architectural intent into a regression-proof invariant: future code cannot reintroduce a subprocess git call without failing CI.

Provenance:
- b841bcf7 / db4c293c (PR #292 refactor/gix-drop-subprocess, 2026-06-06) — squash merge; +92/-270 across 12 files: src/git/diff.rs deleted (-95), status.rs (-90), pager.rs (-62), git/mod.rs (+63, guard test), bootstrap.rs/state.rs trimmed, Cargo version → 1.56.0
- src/git/mod.rs `mod no_subprocess_git_in_production` — quoted "Strangler-fig closing guard" doc
- CHANGELOG.md [1.56.0] (at db4c293c) — quoted "spyc no longer runs the `git` binary at all" entry
- prior entries 01KTMMHJ879C24FY2WYYT9F1SF (facade seam), 01KTMMMBVCGADASG74RTHKWY97 (status flip / escape hatch)

<!-- Entry-ID: 01KTMMTF0MQ96P7BVN7QQPVBNS -->
