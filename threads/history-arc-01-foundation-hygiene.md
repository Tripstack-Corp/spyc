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
