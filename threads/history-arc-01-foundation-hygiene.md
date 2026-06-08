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

---
Entry: Claude Code (caleb) 2026-05-07T09:55:03.677437+00:00
Role: scribe
Type: Note
Title: Tail: note to future readers — what this thread records and why it's shaped this way

Spec: scribe

tags: #history #arc-01

If you're reading this thread cold: it's the baseline narration of arc 01 — the first of eight arcs that reconstruct spyc's first 22 days of merged work. The reconstruction was written after-the-fact (2026-05-07), but the head entries above are deliberately in present tense to preserve the as-if-real-time conceit the spine's framing entry sets. None of it actually happened in present tense. The voice is a choice; we're being honest about that here in the tail because the head can't be — the head is busy holding the line.

How the thread is shaped:

- Index 0 — framing. Names the arc's three PRs, the date anchor, the cadence choice (option A: framing + per-PR + closure), and the rationale for keeping PR #4 in the arc on structural-binding grounds.
- Indices 1–3 — one entry per PR, in arc order. PR #2 first (the CI rail to `make check`), PR #3 second (cargo-deny + `--locked` + `SECURITY.md` + `deny.toml` riding on PR #2's rail), PR #4 third (the user-visible shell-alias fix and the v1.37.2 release-cut that ships all three).
- Index 4 — closure. Forward-references arc 02 and lists the cadence + voice precedents.
- Indices 5 and 6 — these tail entries. Looser, retrospective, with first-person plural and direct address allowed where they aid flow.

The head/tail boundary is visual: head entries are clinical, segmented, sequence-faithful, grounded in commits and file:line spans. Tail entries are conversational and reflective. If a tail entry reads like a head entry, it's failing at its job.

How to read this thread:

Top to bottom for the as-it-happened narration. Drop into the per-PR entries (indices 1–3) for diff-level specifics — file paths, line counts, CHANGELOG quotes. The framing entry tells you why the arc exists at all and why option A was the cadence choice. The closure entry is the forward bridge to arc 02. These two tails (5 and 6) carry the human-accessible context that tells you what to do with the rest.

Cross-references:
- The spine of the eight-arc reconstruction is `history-overview`. Its segmentation entry (index 1) is what places PR #2, PR #3, and PR #4 together in arc 01 to begin with.
- Arc 02 is `history-arc-02-lazygit-investigation-and-harvest` (forward-referenced by the closure entry above). It picks up the same calendar day as arc 01 and inherits the v1.37.2 commit-subject question PR #4's drift findings raised.
- The remaining six arcs — 03 through 08 — exist or will exist as `history-arc-NN-<slug>` topics named in the spine's closure entry.

What this thread doesn't claim to do: identify recurring patterns, name emergent properties, attribute mindset, or pre-empt the insight layer (Phase 3). The narration here is baseline. Drift findings are flagged, not interpreted. If a recurrence-or-emergence insight thread eventually exists, follow its links back into arc 01's per-PR entries — that's where the data points live, with full provenance behind every line.

Provenance:
- No new commit references; tails reflect on the head entries which carry full SHA provenance.
- `history-arc-01-foundation-hygiene` head entries 0–4 = 01KR0W6FR7T01ZJR84MRKWA13A, 01KR0W81XE4K3G7BBSP42GE1HH, 01KR0W9QF3P9E529E6J3XQMXDV, 01KR0WBKNMQF231X2T8KTGD9KS, 01KR0WD8428XFNTJV11MXX59NF.
- `history-arc-01-foundation-hygiene` reflection tail (index 5) = 01KR0XR504ZR10Y242JERT4K9S.
- `history-overview` spine entries 0–3 = 01KR0TRFWT9W6WMFHC49YSW0BG, 01KR0TWHTC1MPK4KJ08Y9SPE6P, 01KR0TYF5F11DA8P5HNPA20DBK, 01KR0V01TAJVSZFE5ZNMCZHQSF.

<!-- Entry-ID: 01KR0XSCA6AD371NHQBZ7HTS3V -->

---
Entry: Claude Code (caleb) 2026-06-08T22:11:00.555182+00:00
Role: scribe
Type: Note
Title: Continuation framing (#38–#311 window): the hygiene segment matures

Spec: scribe

tags: #history #arc-01

Moment: continuation-framing — Reconstructed: this thread, originally the 3-PR establishing arc (#2,#3,#4, v1.37.2, 2026-04-30), is extended forward to carry the CI/release/toolchain/supply-chain hygiene slice of the #38–#311 continuation window (2026-05-08 → 2026-06-06).   [kind: segment-topology]
When: 2026-05-08 → 2026-06-06 · PRs #51,#56–#61,#64–#66,#69,#88,#117,#143,#164,#165,#167,#170,#188,#199,#200,#293–#296
Recorded rationale: the arc-01 head established `make check` as the single CI gate (PR #2) and hung cargo-deny + `--locked` + `SECURITY.md` + `deny.toml` on it (PR #3); the continuation narrates how that rail is *tuned and extended* across the next month — see arc-01 PR #2 entry = 01KR0W81XE4K3G7BBSP42GE1HH and PR #3 entry = 01KR0W9QF3P9E529E6J3XQMXDV.
Inferred intent: the segment throughline reads as "spyc's build/test/release/supply-chain discipline maturing from established-but-cold into measured-and-fast" — evidence: the 05-09 cache campaign carries explicit wall-clock targets in commit bodies ("cut cold-cache CI from ~6 min toward ~1.5", commit 793df19), and the test-surface expands from the v1.37.2 baseline (770 → 929 tests across the window per commit bodies 3d682ec, 2af9f03). confidence: high
Supersedes: (none — extends arc 01 forward; supersession callouts are per-moment below)

This is the continuation segment for arc-01's concern (foundation hygiene) across the #38–#311 window. The arc-01 head (entries 0–6 above, 2026-05-07 authoring) narrated the three v1.37.2 PRs that *established* the hygiene rails. This continuation narrates the month in which those rails were tuned, measured, and extended: a CI-caching campaign with stated wall-clock budgets, a test-surface expansion, a clutch of release cuts, a toolchain pin + MSRV reconciliation, supply-chain advisory clearing + a scheduled drift report, and a late "aislop"/comment-hygiene cleanup pass.

Two cross-segment threads matter. (1) The test-infra moment (#56,#59,#60,#199,#200) is the data behind the later MVU refactor's "behavior-equivalence behind green CI" claim — cross-ref `history-seg-refactor-mvu`; the snapshot/property/pty/harness tests are what made a large internal rewrite assertable as no-behavior-change. (2) The comment-hygiene PRs (#293,#294,#296) are explicitly downstream of the gix migration and the impl-extraction / 800-LoC decomposition (commit 2af9f03 names "the gix migration, the impl-extraction sweep, and the 800-LoC decomposition") — cross-ref `history-seg-gix-migration` and the refactor segment.

Recorded-rationale note: PRs #51,#56–#61,#64–#66,#69,#143,#164,#165,#167,#170,#188,#199,#200,#293–#296 are squash merges whose **second parent retains the full pre-squash commit body** (`git show <sha>^2`), so recorded rationale is unusually rich for this slice. PR #88 and PR #117 are subject-only squashes — rationale there is the merge subject + diff, marked inferred. Cadence: this continuation does not repeat the per-PR shape of the arc-01 head; it folds into ~10 moments per the bounding rule, naming every PR.

Provenance:
- arc-01 head entries 0–6 = 01KR0W6FR7T01ZJR84MRKWA13A, 01KR0W81XE4K3G7BBSP42GE1HH, 01KR0W9QF3P9E529E6J3XQMXDV, 01KR0WBKNMQF231X2T8KTGD9KS, 01KR0WD8428XFNTJV11MXX59NF, 01KR0XR504ZR10Y242JERT4K9S, 01KR0XSCA6AD371NHQBZ7HTS3V.
- 793df19 (PR #57^2, 2026-05-09) — "cut cold-cache CI from ~6 min toward ~1.5".
- 3d682ec (PR #164, 2026-05-29) — "full test suite (770 tests)"; 2af9f03 (PR #293, 2026-06-06) — "929 tests".

<!-- Entry-ID: 01KTMMKT8E97SAHMKW6GJHTX6E -->

---
Entry: Claude Code (caleb) 2026-06-08T22:11:34.984073+00:00
Role: scribe
Type: Note
Title: PR #51: v1.50.0 release cut — the pager/task-viewer unification ships

Spec: scribe

tags: #history #arc-01

Moment: release — Reconstructed: the [Unreleased] backlog since v1.41.1 is cut as the dated v1.50.0 release; CHANGELOG + Cargo.toml/lock version bump only.   [kind: new-capability]
When: 2026-05-08 · PR #51 (release/v1.50.0) · commit 4f59994 (merge), 4f59994^2 "release: v1.50.0"
Recorded rationale: "The pager / task-viewer unification. The pager grew from \"overlay you summon\" into \"renderer you mount anywhere\" — `^a-v` is a real pager, `D` opens files in-pager, `:task-to-pane` and `:pane-to-task` move pty hosts between display containers, MCP socket discovery is project-scoped (no more cross-project attachment), and a long tail of daily-driver UX fixes from internal usage." — CHANGELOG.md `## [1.50.0] - 2026-05-08` header (commit 4f59994^2).
Inferred intent: a release-cut moment in the same shape as arc-01's v1.37.2 (PR #4) — collect [Unreleased] into a dated block, bump the version. evidence: diff is 3 files (CHANGELOG.md +11, Cargo.toml version, Cargo.lock), zero src/* touch. confidence: high
Supersedes: extends the release cadence first observed at arc-01 PR #4 (v1.37.2 cut) = 01KR0WBKNMQF231X2T8KTGD9KS — same mechanism (CHANGELOG block + version bump), now a recurring move.

PR #51 is the first hygiene-segment moment and the first release cut in the continuation window. It is mechanically identical to arc-01's PR #4 v1.37.2 cut: no application logic, just the version-bump-and-date ritual. The feature content it ships (pager-as-renderer, `:task-to-pane`/`:pane-to-task`) belongs to other segments; this entry records only that v1.50.0 is *cut* here, establishing the version baseline (1.50.0) the entire CI-caching campaign will iterate patch versions against over the next day.

The patch-version mechanics matter for the very next moment: each subsequent patch bump rewrites `Cargo.lock`'s `name = "spyc"` version line, which becomes the load-bearing fact behind the 05-09 cache-key restructure (see next entry — commit 793df19's "every patch version bump rewrote [the lockfile]" diagnosis).

Provenance:
- 4f59994 (PR #51 release/v1.50.0, 2026-05-08) — CHANGELOG.md +11 (`## [1.50.0] - 2026-05-08` block), Cargo.toml version, Cargo.lock.
- CHANGELOG.md `## [1.50.0]` header text quoted verbatim above.
- arc-01 PR #4 entry = 01KR0WBKNMQF231X2T8KTGD9KS (the v1.37.2 release-cut this mirrors).

<!-- Entry-ID: 01KTMMMPZDCN2D25KSX9G1R6Y1 -->

---
Entry: Claude Code (caleb) 2026-06-08T22:12:23.867671+00:00
Role: scribe
Type: Note
Title: PRs #57,#58,#61,#64,#65,#66,#69: the CI-caching campaign — cut CI wall-clock

Spec: scribe

tags: #history #arc-01

Moment: ci-caching-campaign — Reconstructed: a single-day (mostly 05-09) sequence of seven CI changes restructures the Bitbucket cache keys, swaps cargo-deny to a verified prebuilt, adds rustup/coverage caches, relaxes the target cache key, and disables CI incremental compilation — with explicit wall-clock budgets in every commit body.   [kind: refactor]
When: 2026-05-09 (#57,#58,#61,#64,#65,#66) · 2026-05-11 (#69) · PRs #57 (pipeline-cache-improvements, 4561884), #58 (rustup-cache, f779192), #61 (relax-target-and-fat-image, 02f2117), #64 (separate-coverage-cache, 742957b), #65 (coverage-target-dir-env, b2c6c6f), #66 (cache-version-file, 3d79913), #69 (disable-cargo-incremental, c4a2e0b)
Recorded rationale: "Two changes that together cut cold-cache CI from ~6 min toward ~1.5" (commit 4561884^2, 2026-05-09). And later: "warm-cache CI should be measured in tens of seconds" (commit 02f2117^2). The throughline names itself in the commit bodies: cut CI wall-clock.
Inferred intent: a measured optimization campaign, not a one-shot — each PR cites a specific observed CI cost and the next-largest remaining cost, in sequence. evidence: #58 body "After the cargo-deny prebuilt swap, the next visible CI cost was `rustup component add`"; #64 "~18s observed in the v1.50.11 PR"; #69 "Pipeline #380's warm-cache run showed... ~3 min vs ~6 min... should be closer to <1 min". confidence: high
Supersedes: rewrites the cache + cargo-deny-install shape laid down in arc-01 PR #2/#3 — the `target` cache keyed on `Cargo.lock` + `rust-toolchain.toml` (PR #2, entry 01KR0W81XE4K3G7BBSP42GE1HH) and `cargo install cargo-deny --locked` (PR #3, entry 01KR0W9QF3P9E529E6J3XQMXDV) are both superseded here.

This is the densest hygiene moment in the window: seven PRs in ~36 hours, all on `bitbucket-pipelines.yml`, each with a verbatim cost-accounting commit body (preserved on `^2`). The campaign's spine:

1. **#57 — cache-key restructure + cargo-deny prebuilt.** The `cargo` cache ($CARGO_HOME) drops `Cargo.lock` from its key, keying on `rust-toolchain.toml` only: "Previously the key included Cargo.lock, which every patch version bump rewrote (the lockfile records `name = \"spyc\" version = \"...\"`), so each patch PR busted the whole cache" (4561884^2). cargo-deny moves from `cargo install --locked` (~3 min) to a sha256-verified prebuilt tarball from the EmbarkStudios GitHub release, pinned to 0.19.4, idempotent on warm cache. This directly supersedes the PR #3 install line.
2. **#58 — rustup cache.** New `rustup` cache scoped to $RUSTUP_HOME keyed on `rust-toolchain.toml`, wired into both quality + coverage steps; "rustup component add is idempotent: on a warm cache the step becomes a near-no-op" (f779192^2).
3. **#61 — relax target key + fat image.** The `target` cache also drops `Cargo.lock` (now toolchain-only): "cargo's per-crate fingerprint hashes each crate's actual inputs... so restoring a stale target/ against a different Cargo.lock is *safe*" (02f2117^2). Base image `rust:1.85-slim` → `rust:1.85` to bake in make/git/curl and drop the ~13s apt-get step. This supersedes the PR #2 `target` key.
4. **#64 → #65 — coverage isolation, with a stumble.** #64 adds a dedicated `target-cov` cache because "`cargo llvm-cov` injects `-C instrument-coverage` RUSTFLAGS so its target/ contents are *not* reusable by the un-instrumented Quality build" (742957b^2) — but used `--target-dir`, a flag `cargo llvm-cov` does not accept. #65 is a same-pattern hot-fix: "it failed with `error: invalid option '--target-dir'`. The repo doesn't gate merges on green pipelines so it landed and broke main's Coverage step" (b2c6c6f^2) — switched to `CARGO_TARGET_DIR=target-cov`. This is a gotcha worth flagging: **CI is not a merge gate in this repo**, so a broken pipeline reaches `main` and is fixed forward.
5. **#66 — explicit cache busting.** New `.ci-cache-version` file added to all four cache keys because "Bitbucket caches are immutable per key — once a cache is uploaded for a given key, subsequent runs *never* upload over it" (3d79913^2), so newly-added crates (proptest from #59) never entered the cache. Bumping the integer forces a fresh upload.
6. **#69 — disable CI incremental.** `CARGO_INCREMENTAL=0` on the CI cargo invocations: incremental metadata "include[s] build paths and timestamps that don't match the fresh runner's filesystem state" so warm-cache hits silently rebuilt; "Standard big-Rust-shop CI pattern (rust-lang itself)... Local dev keeps incremental on" (c4a2e0b^2). Bumped `.ci-cache-version` to 2.

The campaign reads as iterative profiling: measure the dominant cost, eliminate it, re-measure. The "CI is not a merge gate" fact (surfaced by #65's regression reaching main) is the same property arc-01 PR #3's drift findings implied (TODO.md doc-lag landing on main); here it has a concrete cost.

Provenance:
- 4561884 (PR #57, 2026-05-09) — bitbucket-pipelines.yml: cargo cache key Cargo.lock→toolchain; cargo-deny prebuilt block (VERSION/SHA256 0.19.4); commit body quoted.
- f779192 (PR #58, 2026-05-09) — bitbucket-pipelines.yml: `rustup` cache def + wired into quality/coverage steps.
- 02f2117 (PR #61, 2026-05-09) — bitbucket-pipelines.yml: `target` cache key relaxed to toolchain-only; image rust:1.85-slim→rust:1.85.
- 742957b (PR #64, 2026-05-09) — bitbucket-pipelines.yml: `target-cov` cache def; (broken) `--target-dir target-cov` on cargo llvm-cov.
- b2c6c6f (PR #65, 2026-05-09) — bitbucket-pipelines.yml: hot-fix to `CARGO_TARGET_DIR=target-cov`; commit body names the invalid-option failure.
- 3d79913 (PR #66, 2026-05-09) — new `.ci-cache-version` (init 1) added to all four cache key file-lists.
- c4a2e0b (PR #69, 2026-05-11) — `CARGO_INCREMENTAL=0` on quality + coverage steps; `.ci-cache-version`→2.
- arc-01 PR #2 entry = 01KR0W81XE4K3G7BBSP42GE1HH (the cache + target key this supersedes).
- arc-01 PR #3 entry = 01KR0W9QF3P9E529E6J3XQMXDV (the `cargo install cargo-deny --locked` this supersedes).

<!-- Entry-ID: 01KTMMPDE6S4PA834YDR24SX1H -->

---
Entry: Claude Code (caleb) 2026-06-08T22:13:45.938421+00:00
Role: scribe
Type: Note
Title: PRs #56,#59,#60,#199,#200: test-surface expansion — the green-CI substrate for the refactor

Spec: scribe

tags: #history #arc-01

Moment: test-infra — Reconstructed: the test surface expands along four axes — widget snapshot tests (#56), narrow property tests (#59), a pty/vt100 roundtrip integration test (#60), and App-harness routing/focus + session-restore regression tests (#199,#200) — each closing a named TEST_IMPROVEMENT_PLAN / TODO.md item, all cfg(test)-additive with zero runtime behavior change.   [kind: new-capability]
When: 2026-05-09 (#56,#59,#60) · 2026-05-30 (#199,#200) · PRs #56 (widget-snapshots, 1e535b8), #59 (property-tests, e62ad23), #60 (pty-roundtrip, 2136a80), #199 (test-routing-focus, 6225392), #200 (test-session-restore, 2606a0b)
Recorded rationale: #56 "Adds 10 new insta snapshots for the four widgets called out in TODO.md's \"[M] Snapshot tests on widgets\" item" (1e535b8^2). #59 "Adds the `proptest` dev-dep and one `proptest!` block per site, matching the TODO.md \"[S] Property tests (narrow)\" item. Five properties across three sites" (e62ad23^2). #60 "New tests/pane_roundtrip.rs... closes the TODO.md \"[L] One pty integration test\" item" (2136a80^2). #199/#200 "TEST_IMPROVEMENT_PLAN Phase 2... Test-only, additive cfg(test) — zero runtime behavior change" (6225392^2, 2606a0b^2).
Inferred intent: a deliberate, plan-driven build-out of the test surface (snapshot/property/pty/harness), each PR retiring a sized TODO item or a TEST_IMPROVEMENT_PLAN phase-2 item — not ad-hoc coverage. evidence: every commit body names its plan item and flips it `[x]`; test counts climb 794 (#199) → 800 (#200) → 929 (later). confidence: high
Supersedes: (none — net-new test surface; builds on the `make check` gate from arc-01 PR #2 = 01KR0W81XE4K3G7BBSP42GE1HH)

This moment is the load-bearing substrate for a claim made elsewhere in the reconstruction: the MVU refactor's "behavior-equivalence behind green CI" assertion (cross-ref `history-seg-refactor-mvu`). A large internal rewrite can only be asserted as no-behavior-change if there is a behavior oracle; this moment is where that oracle is built.

The four axes, each grounded in the commit body:
- **#56 widget snapshots (05-09):** 10 insta snapshots — list_view (3), pager (4: ANSI, pretty-hex, line-number gutter widening, search highlight), prompt (3: simple, vi insert, vi normal). Glyph-level (symbols-only) harness mirroring the existing status-bar pattern: "catches layout, gutter, and search-bar regressions but not pure styling/color drift" (1e535b8^2). These are the snapshots PR #295 (ratatui bump) later relies on as unchanged.
- **#59 property tests (05-09):** `proptest` dev-dep + 5 properties across `shell::expand::shell_quote` (round-trip via a test-only POSIX decoder, "real, not a tautology against the encoder itself"), `state::ignore::Mask` (union-over-patterns, literal self-match), `keymap::resolver::Resolver` (count composition, leading-zero handling). Adds proptest to Cargo.lock — the crate that motivated PR #66's `.ci-cache-version` mechanism (the immutable-cache problem).
- **#60 pty roundtrip (05-09):** `tests/pane_roundtrip.rs`, `#[cfg(unix)]`, spawns `cat` via portable-pty, drains through vt100, asserts row 0. "validates the integration contract spyc relies on (portable-pty + vt100) without going through any spyc-internal wiring" (2136a80^2).
- **#199/#200 harness regressions (05-30):** #199 adds 3 routing/focus workflow tests on the App harness (prompt-wins-over-list, overlay-pager-consumes-keys, esc-closes-overlay) driving the full resolver→route→dispatch path pty-free. #200 adds agent reconstruct-restore per-profile coverage (claude/codex/agy/gemini/Other resume semantics) + a session disk round-trip test. Both explicitly TEST_IMPROVEMENT_PLAN Phase 2 (routing/focus), both `make check + make lint-linux green` (the lint-linux from PR #188, see cross-compile moment).

#56,#59,#60 land on 2026-05-09 — interleaved with the cache campaign on the same day; #59's proptest addition is what later forces PR #66's cache-busting fix. #199/#200 land three weeks later as the refactor approaches, which fits their role as the App-harness oracle.

Provenance:
- 1e535b8 (PR #56, 2026-05-09) — src/ui/{list_view,pager,prompt}.rs harnesses + 10 .snap files; commit body quoted.
- e62ad23 (PR #59, 2026-05-09) — Cargo.toml proptest dev-dep; property blocks in src/keymap/resolver.rs, src/shell/expand.rs, src/state/ignore.rs.
- 2136a80 (PR #60, 2026-05-09) — tests/pane_roundtrip.rs (+110).
- 6225392 (PR #199, 2026-05-30) — src/app/mod.rs +56 (3 routing tests); body cites 794 tests.
- 2606a0b (PR #200, 2026-05-30) — src/agent/mod.rs +71, src/state/sessions.rs +64; body cites 800 tests.
- arc-01 PR #2 entry = 01KR0W81XE4K3G7BBSP42GE1HH (the `make check` gate these run under).

<!-- Entry-ID: 01KTMMQN283Z4FYP184WT4015X -->

---
Entry: Claude Code (caleb) 2026-06-08T22:14:24.748987+00:00
Role: scribe
Type: Note
Title: PR #88: scheduled weekly deps-drift pipeline — advisories caught off the push path

Spec: scribe

tags: #history #arc-01

Moment: supply-chain — Reconstructed: a `weekly-deps` custom Bitbucket pipeline is added, running `cargo deny check advisories` (hard-fail) + `cargo outdated` + `cargo tree --duplicates` (soft), wired to a UI-configured schedule rather than push.   [kind: new-capability]
When: 2026-05-14 · PR #88 (ci/weekly-deps-drift) · commit b806df1 (merge)
Recorded rationale: not recorded as a body (squash merge; subject only: "ci: weekly deps-drift pipeline (advisories + outdated)"). The rationale is carried in the `bitbucket-pipelines.yml` header comment added by the diff (read directly, it IS the design statement).
Inferred intent: close the gap where RUSTSEC advisories land against unchanged deps on weeks with no commits — the push-path quality gate cannot catch those. evidence: the added header comment "It does NOT run on push — it's wired to a Bitbucket *schedule*... Quality-gate runs on PRs cover the push path; this run covers the gap"; the step comment on `cargo deny check advisories` "picks up new advisories against existing deps even on weeks where we shipped no commits". confidence: high
Supersedes: extends the cargo-deny supply-chain rail from arc-01 PR #3 = 01KR0W9QF3P9E529E6J3XQMXDV — that PR made cargo-deny a push-time gate; this adds the time-based dimension cargo-deny's static gate could not cover.

PR #88 adds a third CI step (`&deps-status`, "Dependency drift report") alongside Quality and Coverage, plus rewrites the pipeline header to document a `weekly-deps` custom pipeline. The pipeline reuses the exact pinned cargo-deny prebuilt block from PR #57 (same VERSION 0.19.4 / SHA256, same idempotent install dance) and adds `cargo install cargo-outdated --locked`.

The severity split is deliberate and documented in the diff: `cargo deny check advisories` hard-fails (re-fetches the RUSTSEC DB every run), while `cargo outdated --root-deps-only || true` and `cargo tree --duplicates || true` are soft — "we want the report visible in the log... not a red build that trains us to ignore it". The schedule itself lives in the Bitbucket repo UI, not YAML: "Bitbucket Cloud doesn't expose schedules in YAML; the pipeline name and the UI schedule are the contract." Failures route through "the existing Bitbucket → Slack integration."

This is the supply-chain rail growing a temporal axis. Arc-01 PR #3 made advisory-checking a *push-time* gate via `make check`'s `deny` target; on a quiet week, a freshly-published RUSTSEC advisory against an existing dep would not be caught until the next commit. The weekly schedule closes that gap. The PR also touches BUGS.md (−5) and TODO.md (+51) — bookkeeping folded here, no separate moment.

Provenance:
- b806df1 (PR #88 ci/weekly-deps-drift, 2026-05-14) — bitbucket-pipelines.yml: header rewrite + `&deps-status` step (+61); BUGS.md −5, TODO.md +51.
- bitbucket-pipelines.yml header + step comments (post-merge) — quoted above; the recorded rationale for a subject-only squash.
- arc-01 PR #3 entry = 01KR0W9QF3P9E529E6J3XQMXDV (the cargo-deny push-time gate this extends).
- PR #57 entry = 01KTMMPDE6S4PA834YDR24SX1H (the prebuilt cargo-deny block reused here).

<!-- Entry-ID: 01KTMMSWZPN5FJTTCCRZ7CA445 -->

---
Entry: Claude Code (caleb) 2026-06-08T22:14:55.356792+00:00
Role: scribe
Type: Note
Title: PRs #117,#188: cross-compile lint + doctor — catching Linux-only lints from a Mac

Spec: scribe

tags: #history #arc-01

Moment: cross-compile-tooling — Reconstructed: `make doctor` is reordered to check rustup first (#117), and a `make lint-linux` target is added that runs clippy for the musl Linux target from macOS via zig as the C cross-compiler — fixing a class of OS-gated lint that host clippy compiled out and never saw (#188).   [kind: gotcha]
When: 2026-05-22 (#117) · 2026-05-30 (#188) · PRs #117 (fix-make-doctor, 7894add), #188 (fix/linux-clippy-cross, 8156895)
Recorded rationale: #117 not recorded as a body (squash merge; subject only: "Merged in fix-make-doctor"). #188 recorded: "clipboard.rs's Linux copy_impl had a nested `if WAYLAND_DISPLAY { if let Some(r) = ... }` that clippy's collapsible_if flags as a let-chain. The block is cfg(target_os = \"linux\"), so host (macOS) clippy compiles it out and never saw it — the MSRV-1.88 let-chain sweep missed it and it failed only on Linux CI" (8156895^2).
Inferred intent: the development host is macOS but CI (and a deploy target) is Linux; OS-gated code can only be linted from the host that compiles that branch in. evidence: #188 body "the only way to lint cfg(target_os = \"linux\") code from a Mac and catch this class of OS-gated lint before it reaches CI"; the `make doctor` diff (#117) checks for `zig` and `cargo-zigbuild`, the cross-compile toolchain. confidence: high
Supersedes: #117 reorders the `doctor` target in Makefile (genesis predates this window); #188 adds a net-new `lint-linux` target. (no prior moment in this thread superseded)

These two PRs read as one concern: spyc is developed on macOS but ships/CIs on Linux, and the gap between them is a recurring source of breakage.

**#117 (05-22)** is a one-line Makefile fix: the `doctor` target's rustup probe is moved to run *first* (before rustc/cargo) and loses a `| head -1` pipe. Subject-only squash; the diff shows the `doctor` target already checks zig + cargo-zigbuild as prerequisites, so the cross-compile toolchain is already a tracked build dependency by this point.

**#188 (05-30)** makes the gap concrete. A `collapsible_if` clippy lint inside a `cfg(target_os = "linux")` block in `src/clipboard.rs` was invisible to macOS-host clippy (the branch is compiled out on the host), and "the MSRV-1.88 let-chain sweep missed it" — so it failed only on Linux CI. The fix collapses the if into a let-chain *and*, more durably, adds `make lint-linux`: clippy for `x86_64-unknown-linux-musl` run from macOS via cargo-zigbuild's `zig cc` wrapper ("translates the target triple so zstd-sys builds"). After this PR, the test-infra PRs #199/#200 both cite `make check + make lint-linux green` as their bar — the cross-lint becomes part of the local gate.

The #188 gotcha also names a supersession-adjacent fact: an "MSRV-1.88 let-chain sweep" happened (in another segment) that this PR patches a miss from. That MSRV-1.88 move is the same one PR #167 (supply-chain moment) leans on to unblock `time` 0.3.47.

Provenance:
- 7894add (PR #117 fix-make-doctor, 2026-05-22) — Makefile: `doctor` target reorders rustup probe to first line, drops `| head -1`.
- 8156895 (PR #188 fix/linux-clippy-cross, 2026-05-30) — Makefile +21 (`lint-linux` target via cargo-zigbuild); src/clipboard.rs collapsible_if → let-chain (8 lines); commit body quoted.
- PR #199/#200 entry = 01KTMMQN283Z4FYP184WT4015X (cite `make lint-linux green` as their bar).

<!-- Entry-ID: 01KTMMV2X1VV9QPY0YA4AZPE05 -->

---
Entry: Claude Code (caleb) 2026-06-08T22:15:29.121972+00:00
Role: scribe
Type: Note
Title: PRs #164,#165: pin Rust toolchain to 1.96.0; reconcile INSTALL.md MSRV to 1.85

Spec: scribe

tags: #history #arc-01

Moment: toolchain-pin — Reconstructed: `rust-toolchain.toml` is pinned from the floating `stable` channel to an exact `1.96.0` (#164), and stale INSTALL.md prose claiming MSRV 1.80 is reconciled to the authoritative 1.85 (#165).   [kind: convention]
When: 2026-05-29 · PRs #164 (chore/pin-rust-toolchain-1.96, 3d682ec), #165 (docs/install-msrv-1.85, e25f10e)
Recorded rationale: #164 "`rust-toolchain.toml` floated on the `stable` channel, so the compiler and clippy silently tracked whatever stable was current. Combined with the `cargo clippy --locked -- -D warnings` commit gate, a new stable shipping a new lint could break commits mid-work with no code change. Pin the exact version (1.96.0, released 2026-05-25) so toolchain upgrades are an explicit, reviewable bump." (3d682ec^2). #165 "INSTALL.md said \"Minimum supported Rust version: 1.80\" but `Cargo.toml`'s authoritative `rust-version` is 1.85... Fix the stale prose." (e25f10e^2).
Inferred intent: make toolchain upgrades a reviewable event rather than an ambient surprise, because the `-D warnings` gate turns any new clippy lint into a build break. evidence: #164 names the failure mode ("break commits mid-work with no code change") and verifies green ("770 tests"); the floating-channel→pinned-version diff in rust-toolchain.toml. confidence: high
Supersedes: hardens the `cargo clippy --locked -- -D warnings` gate the cache campaign and arc-01 PR #3 already established — the gate was the thing that made a floating toolchain dangerous.

This moment separates two distinct version concepts that are easy to conflate:

- **Pinned dev toolchain** (#164): the exact compiler CI and devs build *with*. Moves from `channel = "stable"` to `channel = "1.96.0"` in `rust-toolchain.toml`. The rationale is precise: a floating stable + a `-D warnings` clippy gate means any new stable's new lint breaks commits with no code change. Pinning makes the upgrade reviewable. Documented in INSTALL.md, no version bump ("dev-toolchain change, no user-visible behavior").
- **MSRV / `rust-version`** (#165): the *minimum* Rust a consumer needs, which lives in `Cargo.toml`. #165 only fixes stale INSTALL.md prose (1.80 → 1.85) to match the authoritative `Cargo.toml` value, noting "deny.toml, ROADMAP, SECURITY, and the CI image all already say 1.85."

Honesty note on the MSRV timeline: PR #165 reconciles prose *to 1.85* on 05-29, but by PR #167 (same day, supply-chain moment) `Cargo.toml`'s `rust-version` already reads **1.88** (verified: `Cargo.toml` at 3d682ec^2~1 shows `rust-version = "1.88"`), and the v1.51.4 CHANGELOG (PR #170) states "MSRV 1.88." The 1.85→1.88 MSRV bump itself is **not in this slice** — it landed in another segment (the MSRV-1.88 let-chain sweep referenced by PR #188). So #165's reconciliation is to a value (1.85) that the repo was simultaneously moving past; this entry records the reconciliation as a doc-hygiene fix, not as the MSRV decision. PURE SEQUENCE-INFERENCE on which segment owns the 1.88 bump — flagged for the coordinator.

Provenance:
- 3d682ec (PR #164, 2026-05-29) — rust-toolchain.toml channel stable→1.96.0; INSTALL.md +5; CHANGELOG +8; commit body quoted.
- e25f10e (PR #165, 2026-05-29) — INSTALL.md single-line 1.80→1.85.
- Cargo.toml at 3d682ec^2~1 — `rust-version = "1.88"` (the MSRV already past 1.85, owned by another segment).
- PR #188 entry = 01KTMMV2X1VV9QPY0YA4AZPE05 (names the "MSRV-1.88 let-chain sweep").

<!-- Entry-ID: 01KTMMVZKGKKZ0Z8VY1Q8TW3D2 -->
