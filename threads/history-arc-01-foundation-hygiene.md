# history-arc-01-foundation-hygiene — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: history-arc-01-foundation-hygiene
Created: 2026-05-07T09:27:15.833070+00:00

---
Entry: Claude Code (caleb) 2026-05-07T09:27:15.833070+00:00
Role: scribe
Type: Note
Title: Framing: arc 01, the establishing arc — three PRs that ship as v1.37.2

Spec: scribe

tags: #history #arc-01

Arc title: `foundation-hygiene`. Date anchor: 2026-04-30 (single calendar day; "Day 0" of the 22-day window). Member PRs:

- d9b9360 (PR #2 chore/ci-hygiene, 2026-04-30) — "ci: align with make check, add target cache + pre-commit hook" (commit d9b9360, 2026-04-30).
- 32ebf2c (PR #3 chore/security-hygiene, 2026-04-30) — "security: cargo-deny, --locked, SECURITY.md" (commit 32ebf2c, 2026-04-30).
- 1f41b4b (PR #4 fix/shell-aliases, 2026-04-30) — "shell: aliases work in :!cmd / ;cmd via $SHELL -i (v1.37.2)" (commit 1f41b4b, 2026-04-30).

**Arc 01 is the establishing arc for the eight-arc reconstruction.** This thread is the first of eight baseline arc threads to be written against the segmentation published on `history-overview` (entries 0–3). The cadence shape, voice habits, and provenance grain that arc 01 sets become the precedent that arcs 02–08 either inherit or consciously break. The thread reads accordingly: small enough to validate the contracts on a 3-PR set, faithful enough to the per-PR sequence that scaling to larger arcs (arc 05 has eight PRs) is a stylistic continuation, not a re-design.

**Cadence choice: option A — three sequential per-PR arc-content entries** (in addition to this framing entry and a closure entry). Five entries total: framing → PR #2 → PR #3 → PR #4 → closure.

Rationale (precedent for arcs 02–08):
- The voice contract on `history-overview` entry 0 frames the narration as "as if a watercooler scribe had been present while the work landed" (commit-thread tradition: per-event entries, not per-day digests). Per-PR entries match that voice naturally.
- Sequence-faithful narration is granular at the PR boundary. A consolidated arc-content entry collapses three distinct moves into one observation; a per-PR cadence preserves the order of the rails as they were laid down.
- Scaling consideration: arcs 03–08 carry 4–8 PRs each. A per-PR cadence scales linearly into each. A consolidated cadence either forces an unwieldy single-entry summary at arc 05's eight PRs, or breaks the precedent silently when an arc gets large enough — better to pick the scaling shape now.
- Back-references from later arcs (arc 03 → arc 02, arc 05 → arc 02, arc 06 → arc 02 per the special-handling entry) target specific PR-level entry IDs. A per-PR cadence makes those references precise.

Future arcs may break this cadence with a stated reason. Arc 02 (lazygit-investigation-and-harvest, 2 PRs) is a candidate for a different shape because PR #5's investigation deliverable and PR #12's harvest closer read as one move with two phases; the arc-02 author has standing to consolidate. Arcs with a cluster-boundary call (arc 06 with the harpoon/quickselect picker pair, arc 08 with the PR #30 → PR #31 panic-then-upgrade pair) may also choose differently. The precedent is per-PR; departures should name themselves.

**PR #4 disposition: kept in arc 01.** The Phase 1 segmentation flagged PR #4 as a hard boundary call ("shell-execution infrastructure that belongs alone or with the `!`-capture surface"), and the brief required rationale beyond timing.

The unifying concern of arc 01 reads as "spyc's baseline correctness needed tightening at three different layers before any forward motion." PR #2 fixes the CI layer — the gate was inlining cargo commands and missing the `--test-threads=1` constraint, leaving CI red on main per its own CHANGELOG ("CI was inlining `cargo test --all-targets` without that flag and hitting the race, leaving CI red on `main`"). PR #3 fixes the supply-chain layer — no `cargo-deny`, no `--locked`, no `SECURITY.md`. PR #4 fixes the shell-execution layer — `:!cmd` and `;cmd` silently dropped user aliases and rc-file PATH because `sh -c` runs non-interactively. Three layers, three PRs, one calendar day, one v1.37.2 release.

The release-cut shape is the load-bearing fact that turns "Day 0 polish" into "Day 0 release." PR #4's CHANGELOG diff promotes the entire `[Unreleased]` block (Fixed + Changed + CI / Tooling + Security) into `## [1.37.2] - 2026-04-30`. Without PR #4, the work in PR #2 and PR #3 sits in `[Unreleased]` indefinitely. PR #4 is both a headline user-visible fix and the version-cutter that ships PR #2 + PR #3 as a v1.37.2 release. That binds the three PRs into one arc with structural force, not just timing.

The alternative — "PR #4 to a one-PR side-arc on shell-execution infrastructure" — was considered and rejected. No other PR in the 22-day window obviously belongs to that hypothetical arc; the `!`-capture core landed pre-window per `ARCHITECTURE.md:97-101` ("`!` captured commands also use a slave PTY since v1.12.0"). A solo arc with one PR adds no narrative value over a per-PR entry inside arc 01.

**Cross-thread back-link**: this thread continues from `history-overview`:
- Framing entry 0 = 01KR0TRFWT9W6WMFHC49YSW0BG.
- Segmentation entry 1 = 01KR0TWHTC1MPK4KJ08Y9SPE6P.
- PR #5 special-handling entry 2 = 01KR0TYF5F11DA8P5HNPA20DBK.
- Closure entry 3 = 01KR0V01TAJVSZFE5ZNMCZHQSF.

The arc-content entries that follow this framing narrate PR #2, PR #3, and PR #4 in sequence. The closure entry forward-references arc 02. This thread remains OPEN for cross-arc references.

Provenance:
- d9b9360 (PR #2 chore/ci-hygiene, 2026-04-30).
- 32ebf2c (PR #3 chore/security-hygiene, 2026-04-30).
- 1f41b4b (PR #4 fix/shell-aliases, 2026-04-30).
- `CHANGELOG.md` post-PR-#4 state (commit 1f41b4b): `## [1.37.2] - 2026-04-30` block contains Fixed + Changed + CI / Tooling + Security sections; `## [Unreleased]` reads "(Nothing pending; see [1.37.2] for the most recent release.)"
- `ARCHITECTURE.md:97-101` — pre-window `!`-capture-via-PTY context for the rejected alternative-arc consideration.
- `history-overview` entry 0 = 01KR0TRFWT9W6WMFHC49YSW0BG (voice contract source).
- `history-overview` entry 1 = 01KR0TWHTC1MPK4KJ08Y9SPE6P (segmentation; PR #4 boundary call source).
- `history-overview` entry 2 = 01KR0TYF5F11DA8P5HNPA20DBK (PR #5 disposition; back-reference contract).
- `history-overview` entry 3 = 01KR0V01TAJVSZFE5ZNMCZHQSF (closure; arc thread name list).

<!-- Entry-ID: 01KR0W6FR7T01ZJR84MRKWA13A -->

---
Entry: Claude Code (caleb) 2026-05-07T09:28:07.601165+00:00
Role: scribe
Type: Note
Title: PR #2 (chore/ci-hygiene): the rails get wired to make check

Spec: scribe

tags: #history #arc-01

PR #2 is the first move in arc 01 and the first move of the 22-day window. Commit subject reads "ci: align with make check, add target cache + pre-commit hook" (commit d9b9360, 2026-04-30). Diff: 9 files, +122/-76. Three concerns are bundled under the `chore/` prefix, and the PR's own CHANGELOG entry under "### CI / Tooling" names them in order.

**The CI rail switches from inlined cargo commands to `make check`.** The previous `bitbucket-pipelines.yml` step inlined `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test --all-targets`. The CHANGELOG names the cost: "CI was inlining `cargo test --all-targets` without that flag and hitting the race, leaving CI red on `main`" — the missing flag is `--test-threads=1`, required because two state-module tests mutate `XDG_STATE_HOME` and race when parallel. After this PR, the pipeline script reads `make check`; the Makefile owns the gate definition and the `--test-threads=1` constraint moves into `Makefile:test`. The CHANGELOG entry frames the consequence as "Calling `make check` keeps CI and local on the same exact gate." This rail will carry the cargo-deny extension that PR #3 lands next.

**Target-cache and pre-commit hook ride the same PR.** A `target` cache definition is added to `bitbucket-pipelines.yml` alongside the existing `cargo` cache, both keyed on `Cargo.lock` and `rust-toolchain.toml`. A new `Makefile` target `install-hooks` writes `scripts/git-hooks/pre-commit` (10 new lines) into `.git/hooks/pre-commit`, and the hook itself runs `make check` on every commit (bypassable with `git commit --no-verify`, named in the install-hooks target's own echoed reminder).

**The 139-line src/* lint-clean sweep is the price of entry.** Five files outside infrastructure carry diff: `src/app/mod.rs` (43 lines), `src/ui/markdown.rs` (60 lines), `src/ui/pager.rs` (21 lines), `src/fs/ops.rs` (10 lines), `src/ui/line_edit.rs` (5 lines). The CHANGELOG attributes these to a "**Code-tree `cargo fmt --all` sweep** to clear pre-existing formatting drift" and adds explicitly: "No behavior changes." Inspection of `src/ui/markdown.rs` is consistent with that framing — the diff reads as whitespace and bracketing normalization, not logic. The drift cleared here is the drift that would otherwise have failed the new gate the moment CI started enforcing it; the sweep and the gate-tightening land in the same commit so neither half ships broken.

**Sequence-grain detail for arc 03 / arc 08 cross-references**: at PR #2 the Makefile's `check` target is `fmt-check + lint + test` only — no `deny` target exists yet. The `cargo audit --ignore RUSTSEC-2026-0009` step is preserved in `bitbucket-pipelines.yml` outside `make check`. The supply-chain extension to the rail arrives in PR #3.

**Drift findings flagged for the insight layer**:
- The commit subject reads as pure CI work ("ci: align with make check, add target cache + pre-commit hook"), but the diff bundles 139 lines of src/* lint-fix code — accurately captured under `### CI / Tooling` in the CHANGELOG, less so in the commit subject. A reader scanning subjects only points toward "no source changes here," which the diff does not match.
- This PR lands under `[Unreleased]` in the CHANGELOG; it does not bump the version. The release that ships these CI changes is cut by PR #4 as v1.37.2 (see arc 01 framing entry).

Provenance:
- d9b9360 (PR #2 chore/ci-hygiene, 2026-04-30).
- `Makefile:147-154` (post-merge state) — `install-hooks` target.
- `bitbucket-pipelines.yml:14-44` (post-merge state) — `target` cache definition and `make check` invocation; `cargo audit` step preserved.
- `scripts/git-hooks/pre-commit:1-10` (new file) — the hook body.
- `src/app/mod.rs`, `src/fs/ops.rs`, `src/ui/line_edit.rs`, `src/ui/markdown.rs`, `src/ui/pager.rs` — the 139-line sweep; characterization grounded in the CHANGELOG ("No behavior changes") plus inspection of `src/ui/markdown.rs` diff hunks.
- `CHANGELOG.md` post-PR-#2 state, "### CI / Tooling" section under `[Unreleased]`.
- `onboarding-developer-experience` entry 0 = 01KR0PFHHCNVJPNJSTPA3VW62J — current-state seed describing `make check` + `make install-hooks` as established surface; this PR is the genesis of `make install-hooks` and the genesis of CI calling `make check`.
- `history-arc-01-foundation-hygiene` framing entry = 01KR0W6FR7T01ZJR84MRKWA13A.

<!-- Entry-ID: 01KR0W81XE4K3G7BBSP42GE1HH -->

---
Entry: Claude Code (caleb) 2026-05-07T09:29:02.279116+00:00
Role: scribe
Type: Note
Title: PR #3 (chore/security-hygiene): cargo-deny rides the rail; SECURITY.md and deny.toml land

Spec: scribe

tags: #history #arc-01

PR #3 is the second move in arc 01. With PR #2's `make check` rail in place, this PR extends what runs on the rail. Commit subject reads "security: cargo-deny, --locked, SECURITY.md" (commit 32ebf2c, 2026-04-30). Diff: 5 files, +468/-23. Two new files dominate the line count: `SECURITY.md` (136 lines, new) and `deny.toml` (268 lines, new).

**`make check` becomes the supply-chain gate.** The Makefile diff reads `check: fmt-check lint test deny` — adding `deny` to the previous `fmt-check + lint + test`. A new target `deny` calls `cargo deny --all-features check` after a guard that fails-loud if `cargo-deny` is not installed. In `bitbucket-pipelines.yml` the `cargo install cargo-audit` line is replaced with `cargo install cargo-deny` and the standalone `cargo audit --ignore RUSTSEC-2026-0009` step is removed: cargo-deny absorbs both the advisory check and four other concerns the audit step never covered (licenses, sources, bans, yanked-crate detection). The pipeline header comment is rewritten to match: "spyc CI — fmt, clippy, tests, supply-chain (cargo-deny), coverage."

**`--locked` propagates across every cargo invocation.** The Makefile diff adds `--locked` to `RELEASE_FLAGS`, to the `cargo test` line, and to the `cargo clippy` line; the pipelines diff adds `--locked` to `cargo llvm-cov`. The PR's own CHANGELOG entry names the consequence: "Prevents a CI-time `Cargo.lock` drift from silently pulling fresh transitive deps; failures are loud."

**`SECURITY.md` lands as a 136-line policy doc, not a template.** The PR's CHANGELOG entry characterizes the file: "honest posture doc covering threat model, supply-chain controls, build/install trust chain, and known caveats. Avoids signing/SBOM theater for an internal tool with no published binary distribution channel." The current-state seed `onboarding-security` entry 0 cites this exact file at line counts (`SECURITY.md:1-137`, `SECURITY.md:9-31` for threat model, `SECURITY.md:33-57` for supply-chain controls, `SECURITY.md:60-66` for distribution posture, `SECURITY.md:115-120` for reporting channel). PR #3 is the genesis of every line the security seed cites.

**`deny.toml` lands as a 268-line config with documented advisory ignores.** The current-state seed `onboarding-security` entry 0 cites this file at `deny.toml:72-94` (the documented ignores: time 0.3.45, yaml-rust, bincode, paste, serial — each with a `reason` field naming the dep-graph route and the reason it is tolerable), `deny.toml:104-124` (license allow-list reflecting "the licenses present in our actual dep graph as of v1.37.1"), and `deny.toml:258` (source allow-list: only `crates.io`). PR #3 is the genesis of every line the security seed cites here too.

**`make dist-sign` scaffolding lands without being wired into CI.** A new `dist-sign` Makefile target produces a detached GPG signature on `dist/checksums-sha256.txt`, with `GPG_KEY` as an opt-in environment variable for key selection. The CHANGELOG entry names the choice explicitly: "Not used today (we don't ship prebuilt binaries); SECURITY.md documents the intentional gap so a future signing rollout has a ready landing spot." This aligns with the `onboarding-security` seed's reading of the signing posture as "theater-avoided" today, "load-bearing the moment public artifacts ship."

**Sequence-grain dependency on PR #2**: this PR's `make check` extension only earns its keep because PR #2 made CI call `make check` in the first place. With the rail laid by PR #2, adding `deny` to the gate becomes a one-line change in `Makefile:check` plus the new `deny` target body; without PR #2, the cargo-deny invocation would have to be wired into `bitbucket-pipelines.yml` directly and would not be runnable locally as part of the pre-commit hook. The PRs read as one-then-two; the second extends the first.

**Drift findings flagged for the insight layer**:
- The commit subject groups three concerns ("cargo-deny, --locked, SECURITY.md"). Diff inspection shows a fourth: `make dist-sign`. Captured in CHANGELOG, omitted from the commit subject.
- This PR lands under `[Unreleased]` in the CHANGELOG; like PR #2, it does not bump the version. The release that ships PR #3's policy + tooling is cut by PR #4 as v1.37.2.
- `TODO.md:99-104` continues to read "cargo-audit" after this PR (the `onboarding-risk-register` seed flags the same drift in current-state). The migration from cargo-audit to cargo-deny in this PR is complete in code and CI; only the doc lags. This is a one-PR-introduces-a-doc-drift signal worth carrying into the insight layer.

Provenance:
- 32ebf2c (PR #3 chore/security-hygiene, 2026-04-30).
- d9b9360 (PR #2 chore/ci-hygiene, 2026-04-30) — the rail this PR extends; named for sequence dependency.
- `Makefile:33-65` (post-merge state) — `check` target reads `fmt-check lint test deny`; `deny` target body; `--locked` on test/lint.
- `Makefile:18,135-149` (post-merge state) — `RELEASE_FLAGS := --locked --release`; `dist-sign` target body.
- `bitbucket-pipelines.yml:1-50` (post-merge state) — header comment rewrite; `cargo install cargo-deny` replacing `cargo install cargo-audit`; `--locked` on `cargo llvm-cov`.
- `SECURITY.md` (new file, 136 lines).
- `deny.toml` (new file, 268 lines).
- `CHANGELOG.md` post-PR-#3 state, "### Security" section under `[Unreleased]`.
- `onboarding-security` entry 0 = 01KR0PKS884SXRAKZ8A790Q438 — current-state seed; this PR is the genesis surface.
- `onboarding-developer-experience` entry 0 = 01KR0PFHHCNVJPNJSTPA3VW62J — current-state seed citing `make check = fmt-check + lint + test + deny`; this PR is what completes the four-step gate.
- `history-arc-01-foundation-hygiene` framing entry = 01KR0W6FR7T01ZJR84MRKWA13A.
- `history-arc-01-foundation-hygiene` PR #2 entry = 01KR0W81XE4K3G7BBSP42GE1HH.

<!-- Entry-ID: 01KR0W9QF3P9E529E6J3XQMXDV -->

---
Entry: Claude Code (caleb) 2026-05-07T09:30:04.155296+00:00
Role: scribe
Type: Note
Title: PR #4 (fix/shell-aliases): the user-facing fix and the v1.37.2 cut

Spec: scribe

tags: #history #arc-01

PR #4 is the third move in arc 01 and the only PR in this arc that touches application logic. Commit subject reads "shell: aliases work in :!cmd / ;cmd via $SHELL -i (v1.37.2)" (commit 1f41b4b, 2026-04-30). Diff: 7 files, +157/-10. The version-bump suffix `(v1.37.2)` in the commit subject is load-bearing — see release-cut below.

**The bug being fixed.** The PR's CHANGELOG entry leads with the user-visible failure verbatim: "A user running `:!gemma` (where `gemma` is an alias for a local `llama.cpp` invocation) got `sh: gemma: command not found`." The diagnosis named in the same entry: "spyc spawned `sh -c <cmd>` regardless of the user's `$SHELL`, and even setting `$SHELL` would not have helped: aliases / functions live in interactive rc files (`.zshrc`, `.bashrc`) which non-interactive shells don't load." The fix has to resolve `$SHELL` and pass `-i` so rc-file aliases load.

**The fix introduces a new module: `src/shell/mod.rs` (114 lines, new).** The module's docstring reads as a policy statement: "Running a child process from a TUI requires tearing the terminal state down so the child can own the tty, then restoring our state when it exits. The actual teardown helpers live in `main.rs` because they touch the `Tui` value directly; this module supplies the policy (which binary, which args, whether a file is viewable)." The module exports `resolve_editor`, `resolve_pager`, `user_shell_invocation`, and re-exports `expand_percent` and `shell_quote` from a sub-module. `user_shell_invocation` returns `(shell_path, [args...])` and selects between `-i` and plain `-c` by shell family; the CHANGELOG names the families: "shells that source rc files in interactive mode (`zsh`, `bash`, `fish`, `ksh`, `mksh`); POSIX `sh` / `dash` get plain `-c` since they don't read rc files in `-i` mode anyway."

**Two call sites adopt the helper.** `src/app/mod.rs` gains 8 lines (the `:!cmd` capture path, named in the CHANGELOG as `spawn_capture`); `src/pane/mod.rs` gains 13 lines (the pane spawn path, named as `Pane::spawn`). Both routes through `shell::user_shell_invocation`. The `;cmd` route — also named in the commit subject — flows through one of these call sites by way of the same helper.

**The CHANGELOG entry names a known tradeoff verbatim.** "Tradeoff: heavy `.zshrc` / `.bashrc` setups (oh-my-zsh banners, p10k init) may now print init noise into capture pagers; well-behaved rc files gate that behind `[[ -t 1 ]]` / `[[ $- == *i* ]]` and stay quiet." The fix accepts that boundary explicitly rather than working around it.

**The release cut: PR #4 is the v1.37.2 release.** `Cargo.toml` bumps `version = "1.37.1"` to `version = "1.37.2"`. The `CHANGELOG.md` diff reshapes `[Unreleased]` into `## [1.37.2] - 2026-04-30`. Inspection of the post-merge `CHANGELOG.md` confirms the `[1.37.2]` block contains four sub-sections: **Fixed** (this PR's shell-alias work), **Changed** (the prior `make install` → `~/.local/bin` work), **CI / Tooling** (PR #2's content, verbatim), **Security** (PR #3's content, verbatim). `[Unreleased]` post-merge reads "(Nothing pending; see [1.37.2] for the most recent release.)" The release cut packages the three arc-01 PRs together as one user-visible release.

**Sequence-grain consequence**: PR #4 is the structural binding force for arc 01. The headline is the shell-alias fix, but the version-cut work in CHANGELOG and Cargo.toml is what turns PR #2 + PR #3 + PR #4 from three independent commits into a coherent v1.37.2 release. Without PR #4, the work in PR #2 and PR #3 sits in `[Unreleased]` with no version bump.

**Drift findings flagged for the insight layer**:
- PR #5 (next in arc 02, 2026-04-30) carries `(v1.37.2)` in its commit subject too, despite v1.37.2 being cut by this PR with `[Unreleased]` reading "(Nothing pending; …)" immediately afterward. Resolution of the version-tag overlap is for arc 02 to handle; flagged here for the cross-arc reference.
- PR #4's commit subject scopes the change to "`:!cmd` / `;cmd`" but the diff also touches `src/pane/mod.rs::Pane::spawn` — which is the path used for pane child processes broadly, not only the `:!cmd` / `;cmd` overlay routes. Title-vs-diff scope mismatch; the CHANGELOG names `Pane::spawn` directly, so the drift is subject-line-level only.
- `FEATURES.md` is updated as part of this PR (6 lines added, named in the CHANGELOG: "FEATURES.md updated to describe the new behavior"). Doc-with-code on this PR is consistent with the documentation contract that the `onboarding-docs-contracts` seed will name in current-state.

Provenance:
- 1f41b4b (PR #4 fix/shell-aliases, 2026-04-30).
- 32ebf2c (PR #3 chore/security-hygiene, 2026-04-30) — content packaged into v1.37.2 by this release-cut.
- d9b9360 (PR #2 chore/ci-hygiene, 2026-04-30) — content packaged into v1.37.2 by this release-cut.
- `src/shell/mod.rs` (new file, 114 lines) — module docstring quoted; `resolve_editor`, `resolve_pager`, `user_shell_invocation` exports.
- `src/app/mod.rs` (post-merge) — `spawn_capture` adoption, 8-line diff.
- `src/pane/mod.rs` (post-merge) — `Pane::spawn` adoption, 13-line diff.
- `Cargo.toml:3` (post-merge) — `version = "1.37.2"`.
- `CHANGELOG.md` post-PR-#4 state — `## [1.37.2] - 2026-04-30` block; `[Unreleased]` parenthetical.
- `FEATURES.md` (post-merge) — 6 lines added.
- `onboarding-developer-experience` entry 0 = 01KR0PFHHCNVJPNJSTPA3VW62J.
- `history-arc-01-foundation-hygiene` framing entry = 01KR0W6FR7T01ZJR84MRKWA13A.
- `history-arc-01-foundation-hygiene` PR #2 entry = 01KR0W81XE4K3G7BBSP42GE1HH.
- `history-arc-01-foundation-hygiene` PR #3 entry = 01KR0W9QF3P9E529E6J3XQMXDV.

<!-- Entry-ID: 01KR0WBKNMQF231X2T8KTGD9KS -->

---
Entry: Claude Code (caleb) 2026-05-07T09:30:57.508878+00:00
Role: scribe
Type: Note
Title: Closure: arc 01 baseline complete; arc 02 (lazygit-investigation-and-harvest) follows

Spec: scribe

tags: #history #arc-01

Arc 01 baseline narration complete. Three PRs cover three layers of baseline correctness — CI gate, supply-chain, shell-execution — and ship together as v1.37.2 on 2026-04-30. PR #2 wires CI to call `make check`; PR #3 hangs cargo-deny + `--locked` + `SECURITY.md` + `deny.toml` on that rail; PR #4 lands the user-visible shell-alias fix and cuts the v1.37.2 release that ships all three. The arc reads as one calendar day's work in three sub-moves, with PR #4 as the structural binding (release-cut) and PR #2 → PR #3 as a rail-then-extension dependency.

**Forward reference: arc 02 — `history-arc-02-lazygit-investigation-and-harvest`** picks up next. Arc 02 covers PR #5 (`investigate/lazygit-support`, commit 0691666, 2026-04-30) and PR #12 (`chore/clean-notes`, commit e210e58, 2026-05-03). PR #5 is the only PR in the 22-day window prefixed `investigate/` and dominates its diff with a 399-line investigation deliverable plus a partial cursor-block fix; PR #12 harvests the investigation notes into `BUGS.md`. Arc 02 is special-handled per `history-overview` entry 2 (= 01KR0TYF5F11DA8P5HNPA20DBK): own-arc with mandatory back-references from arc 03 (cursor-block follow-up at PR #29), arc 05 (alt-screen hint at PR #20; pager-direction at PR #33 / PR #35), and arc 06 (picker pattern at PR #8 / PR #10).

**Cross-arc continuity note from arc 01 to arc 02**: PR #5 lands on the same calendar day as the three arc-01 PRs (2026-04-30) and its commit subject carries `(v1.37.2)` despite PR #4 having cut v1.37.2 with `[Unreleased]` reading "(Nothing pending; …)" immediately afterward. The version-tag overlap is for arc 02 to resolve in its own narration; arc 01 flags it without prejudgement (see PR #4 entry's drift findings).

**Cadence precedent set by this thread**: option A — framing + per-PR + closure (5 entries for a 3-PR arc). Future arcs may inherit or break with stated reason; arc 02 is a candidate for departure given its 2-PR shape with phase-not-PR structure (investigation + harvest). The framing entry of this thread (= 01KR0W6FR7T01ZJR84MRKWA13A) carries the rationale.

**Voice contract precedent set by this thread**: third-person observational, present tense, sequence-faithful narration with no clock-padding ("first move," "next move," "extends the rail" — not "minutes later," "in the same hour"). Verbatim commit-subject quoting honored on every per-PR entry. Hedge tokens used freely from the whitelist; banned mindset words avoided. The full audit appears in this session's hand-off summary to the operator.

This thread remains OPEN for cross-arc references (arc 03 may back-reference PR #2 or PR #3 if a later src-tree change touches the same files; arc 08 may reference PR #2's `target` cache when narrating the dep-upgrade work).

**Arc 01 done-definition self-check**:
- ☑ `watercooler_health` reported Healthy against the spyc code_path.
- ☑ All four `history-overview` entries read (indices 0–3).
- ☑ `onboarding-developer-experience` entry 0 read.
- ☑ `onboarding-security` entry 0 read.
- ☑ Thread `history-arc-01-foundation-hygiene` exists with framing (index 0), three per-PR entries (PR #2, PR #3, PR #4), and this closure entry — five entries total.
- ☑ Framing entry contains: arc title (`foundation-hygiene`), member PRs with SHAs, date range (2026-04-30 to 2026-04-30), cadence-shape rationale (option A), establishing-arc observation, back-link to `history-overview` 0–3.
- ☑ Each per-PR entry contains at least one verbatim commit-subject quote attributed `(commit <sha>, <date>)`.
- ☑ Every entry ends with a `Provenance:` block.
- ☑ Voice contract held: no banned words referencing the maintainer's mindset; no clock-padding language; sequence-over-timing privileged.
- ☑ All entries tagged `#history` and `#arc-01`.
- ☑ No PR #5 or PR #12 content (those are arc 02).
- ☑ No insight-layer claims — narration is baseline.
- ☑ Thread left OPEN.

Provenance:
- d9b9360 (PR #2 chore/ci-hygiene, 2026-04-30) — first commit in arc 01.
- 32ebf2c (PR #3 chore/security-hygiene, 2026-04-30).
- 1f41b4b (PR #4 fix/shell-aliases, 2026-04-30) — last commit in arc 01; v1.37.2 release-cut.
- 0691666 (PR #5 investigate/lazygit-support, 2026-04-30) — first commit of arc 02; named here for forward reference.
- e210e58 (PR #12 chore/clean-notes, 2026-05-03) — second commit of arc 02; named here for forward reference.
- `history-overview` entry 2 = 01KR0TYF5F11DA8P5HNPA20DBK — back-reference contract for arc 02.
- `history-arc-01-foundation-hygiene` framing entry = 01KR0W6FR7T01ZJR84MRKWA13A.
- `history-arc-01-foundation-hygiene` PR #2 entry = 01KR0W81XE4K3G7BBSP42GE1HH.
- `history-arc-01-foundation-hygiene` PR #3 entry = 01KR0W9QF3P9E529E6J3XQMXDV.
- `history-arc-01-foundation-hygiene` PR #4 entry = 01KR0WBKNMQF231X2T8KTGD9KS.

<!-- Entry-ID: 01KR0WD8428XFNTJV11MXX59NF -->

---
Entry: Claude Code (caleb) 2026-05-07T09:54:23.316481+00:00
Role: scribe
Type: Note
Title: Tail: looking back at arc 01 — what reads as load-bearing now

Spec: scribe

tags: #history #arc-01

Looking back at the five head entries above, what reads as load-bearing now is different from what looks like the headline at first glance.

The headline of arc 01 reads as the shell-alias fix in PR #4 — it's the only user-visible bug fix in the arc, and the commit subject puts the version tag in parentheses next to the slug. But on a re-read, the load-bearing fact is that PR #4 is the version-cutter. Without it, PR #2's CI work and PR #3's supply-chain work sit in `[Unreleased]` indefinitely; with it, the three become v1.37.2 on a single calendar day. The framing entry already named this as the structural-binding argument for keeping PR #4 in the arc, but reading it across three per-PR entries makes the cleaner reading land: arc 01 isn't three independent moves, it's one release in three correctness layers.

The cadence-shape choice (option A — framing + per-PR + closure, five entries for three PRs) was the right call here. We won't know whether it stays right until arc 05 — the largest arc, eight PRs — sits down and tries it. Five entries for three PRs is light; thirteen for eight is heavy enough that the closure entry alone won't be doing the heavy summarization. Arc 02 (two PRs, investigation + harvest) is an explicit candidate for departure from this cadence, and the arc 01 closure entry flags it. The precedent is set; departures should name themselves.

Two questions arc 01 carried forward without resolving:

The v1.37.2 commit-subject overlap on PR #5 — next arc, same calendar day, same version tag despite `[Unreleased]` reading "Nothing pending" right after PR #4 cut the release. Arc 01 flagged this in PR #4's drift findings and in the closure entry, then handed it to arc 02 to resolve in its own narration. From where we sit, this is exactly the cross-arc forward-reference the spine's back-reference contract was designed for — arc 02 inherits a question rather than a fact, and that's fine.

The PR #4 `Pane::spawn` touch being broader than the named overlay routes (`:!cmd` / `;cmd`). The commit subject scopes the change narrowly; the diff scope is wider. The CHANGELOG names `Pane::spawn` directly, so the drift is subject-line-level only — but if a future arc 03 entry on pane behavior wants to back-reference where pane-spawn-via-`$SHELL` first lands, this PR is the genesis. That back-reference will probably matter more than it currently looks.

What's not in arc 01 that someone might expect: any narration of the `make check` rail extending further (it does, in arcs 03 and 08), any narration of `cargo-deny` actually catching something (no incident in the 22-day window), any narration of `make dist-sign` getting wired into CI (it doesn't — and PR #3's CHANGELOG entry says so plainly). Arc 01 is the establishing arc; it sets the rails, it doesn't watch them in motion.

Provenance:
- No new commit references; tails reflect on the head entries which carry full SHA provenance.
- `history-arc-01-foundation-hygiene` framing entry = 01KR0W6FR7T01ZJR84MRKWA13A.
- `history-arc-01-foundation-hygiene` PR #2 entry = 01KR0W81XE4K3G7BBSP42GE1HH.
- `history-arc-01-foundation-hygiene` PR #3 entry = 01KR0W9QF3P9E529E6J3XQMXDV.
- `history-arc-01-foundation-hygiene` PR #4 entry = 01KR0WBKNMQF231X2T8KTGD9KS.
- `history-arc-01-foundation-hygiene` closure entry = 01KR0WD8428XFNTJV11MXX59NF.
- `history-overview` segmentation entry = 01KR0TWHTC1MPK4KJ08Y9SPE6P (PR #4 boundary call source).
- `history-overview` PR #5 special-handling entry = 01KR0TYF5F11DA8P5HNPA20DBK (back-reference contract; the v1.37.2 commit-subject question handed forward to arc 02).

<!-- Entry-ID: 01KR0XR504ZR10Y242JERT4K9S -->
