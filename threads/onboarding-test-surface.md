# onboarding-test-surface — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: onboarding-test-surface
Created: 2026-05-07T07:45:09.812501+00:00

---
Entry: Claude Code (caleb) 2026-05-07T07:45:09.812501+00:00
Role: tester
Type: Plan
Title: Onboarding: test and CI surface map

Spec: tester

Purpose: Inventory what's tested, what gates each merge, and which validation surfaces a contributor must run before pushing. The CI lives entirely in Bitbucket Pipelines; there is no `.github/` directory in this repo.

Observed:
- **`make check` is the canonical local gate.** `Makefile:36` defines `check: fmt-check lint test deny`. Behavior:
  - `fmt-check` → `cargo fmt --all -- --check` (`Makefile:54`).
  - `lint` → `cargo clippy --locked --all-targets -- -D warnings` (`Makefile:46`). Clippy is configured with `pedantic` and `nursery` warn-by-default in `Cargo.toml:81-83`, with a documented allow-list (`Cargo.toml:84-144`) — read those before silencing a new lint.
  - `test` → `cargo test --locked --all-targets` (`Makefile:42`). The `--locked` flag everywhere ensures `Cargo.lock` drift fails loudly rather than pulling fresh transitive deps (`SECURITY.md:36-39`).
  - `deny` → `cargo deny --all-features check` (`Makefile:62`); covers advisories, licenses, sources, and bans per `deny.toml`.
- **Tests forced single-threaded.** `bitbucket-pipelines.yml:43-44` notes "Tests forced single-threaded by the Makefile (XDG_STATE_HOME race)." `make test` invokes `cargo test --locked --all-targets`; the single-threading is enforced via the test harness (most likely `[env] RUST_TEST_THREADS=1` or `--test-threads=1` somewhere in the test setup; see also the XDG-state caveat in `ARCHITECTURE.md:114-128` for why state tests can't run in parallel).
- **CI shape (Bitbucket Pipelines).** `bitbucket-pipelines.yml:1-77`:
  - Image: `rust:1.85-slim` — pinned to MSRV.
  - `pull-requests:**` and `branches.main` both run two parallel steps: `quality` (apt installs `make`+`git`, rustup-adds `rustfmt` `clippy`, installs `cargo-deny --locked`, runs `make check`) and `coverage` (rustup-adds `llvm-tools-preview`, installs `cargo-llvm-cov --locked`, runs `cargo llvm-cov --locked --all-targets --fail-under-lines 35`).
  - There is no `default:` block: branch pushes without a PR do not run CI ("open the PR for feedback", `bitbucket-pipelines.yml:57-65`).
  - Coverage gate is a **35% line-coverage floor** with the `--fail-under-lines 35` flag (`bitbucket-pipelines.yml:54`). It's a ratcheting floor — bump it as coverage rises (`ROADMAP.md:53-54`, `TODO.md:39-43`).
- **Test inventory.**
  - 55 source files under `src/` (`find src -name '*.rs' | wc -l`).
  - 2 integration test files: `tests/filesystem.rs`, `tests/keymap_roundtrip.rs`.
  - 577 total `#[test]` occurrences across `src` + `tests` (`grep -rE "#\\[test\\]" --include="*.rs" src tests | wc -l`). Note: `ROADMAP.md:53` reports "358 tests" as the historical milestone, so the test count has nearly doubled since then.
  - Test sites by area (per `ROADMAP.md:53-58` + `TODO.md:50-71`): keymap resolver (77), state modules (picks 6 / inventory 7 / cursor 5 / ignore 11 / history 14 / sessions 11), DSL→resolver round-trips (5), `tests/filesystem.rs` (6), `tests/keymap_roundtrip.rs` (5), startup health check (7), snapshot tests via `insta` + `TestBackend` for the status bar (4).
- **Snapshot-test infrastructure is wired** (`Cargo.toml:53` `insta = "1"` as `[dev-dependencies]`), used today only for status-bar widgets (4 snapshots). Remaining widgets (list_view, pager — ANSI / hex / line numbers / search highlight, line_edit modes) are tracked in `TODO.md:73-76` and `ROADMAP.md:114-118`.
- **Pre-commit hook (optional, off by default).** `make install-hooks` copies `scripts/git-hooks/pre-commit` into `.git/hooks/`; runs `make check` on each commit; bypassable with `git commit --no-verify` (`Makefile:170-176`, `SECURITY.md:54-56`).
- **What is NOT in CI today.** No PTY integration test (`TODO.md:78-81`, `ROADMAP.md:119-120` track the planned single test). No property tests (`TODO.md:83-86`, `ROADMAP.md:121-123`). No fuzzing — explicitly out of scope per `SECURITY.md:108-111` because "there's no untrusted-input parsing path in production code worth fuzzing today."

Inferred:
- Validation strategy is "lint-heavy, deny-heavy, snapshot-light" by maintainer choice. — confidence: high — basis: pedantic+nursery clippy forced via `-D warnings` (`Makefile:46`), `cargo-deny` over `cargo-audit` for advisory coverage (`SECURITY.md:42-43`), no fuzzing (`SECURITY.md:108-111`), snapshot infra wired but only one widget covered (`TODO.md:73-76`). How to apply: when adding a feature, prioritize *unit tests on the State half* (`src/app/state.rs`) over snapshot tests on the View half until the Elm-architecture seam matures.
- The 35% coverage floor will need a deliberate raise to keep being load-bearing. — confidence: medium — basis: with 577 tests vs 358 at the time the floor was set (`ROADMAP.md:53`), today's measured coverage almost certainly exceeds 35%. The floor still catches regressions but doesn't actively push coverage up. How to apply: if a PR feels safe, consider whether a small floor-bump should land in the same PR.

Drift findings: not required for this topic.

Validation checklist for handoff (use as a literal checklist before any merge):
1. `make check` passes locally on the contributor's branch.
2. `cargo llvm-cov --locked --all-targets --fail-under-lines 35` (only if coverage-related changes; otherwise CI runs it).
3. If state-machine logic changed, add `#[test]` cases in the relevant `src/state/<module>.rs` `#[cfg(test)]` block.
4. If a new `:command` was added, verify the punt-list update in `AppState::dispatch_command` per `AGENTS.md:61`.
5. If a new keybinding was added, walk the four-step recipe in `AGENTS.md:60-61` and run the keymap round-trip tests.
6. If user-visible behavior changed, update the doc-set per `AGENTS.md:65-77` *in the same commit* — this is treated as part of the validation gate, not a follow-up.

Next query: `watercooler_search(query="ci coverage make-check tests", thread_topic="onboarding-test-surface", code_path=".")`

Related:
- `onboarding-overview` — front door.
- `onboarding-architecture` — explains why state-side tests are the primary surface (the Update half of MVU is where domain logic lives).
- `onboarding-developer-experience` — local install of `cargo-deny`, `cargo-llvm-cov`, the optional pre-commit hook.
- `onboarding-docs-contracts` — the doc-sync rule that step #6 of the validation checklist enforces.

Provenance:
- Files read: `Makefile:36-62,170-176`, `bitbucket-pipelines.yml:1-77`, `Cargo.toml:53,81-144`, `deny.toml:72-94,104-124`, `SECURITY.md:36-43,54-56,108-111`, `ROADMAP.md:53-58,114-123`, `TODO.md:39-86`, `ARCHITECTURE.md:114-128`.
- Commands run: `find src -name '*.rs' | wc -l` (55), `find tests -name '*.rs'` (filesystem.rs, keymap_roundtrip.rs), `grep -rE "#\\[test\\]" --include="*.rs" src tests | wc -l` (577).
- Sibling entry_ids: `onboarding-overview = 01KR0NZNJ3KM6BJY09Q4P9D0NE`, `onboarding-architecture = 01KR0P4W3ED1QZ8F44PFB2WPDZ`, `onboarding-risk-register = 01KR0P9JC8Z3DF6FQ1GJPF3VKA`.

<!-- Entry-ID: 01KR0PBH6T1AK4VA0JSE75390F -->

---
Entry: Claude Code (caleb) 2026-06-09T05:17:54.833778+00:00
Role: tester
Type: Plan
Title: Onboarding refresh: test and CI surface at #311 (v1.56.0) — 577 → 949 test fns, snapshot/property/pty axes, and the honest "CI is not a merge gate" caveat

Spec: tester

Purpose: Refresh of the test/CI surface from #37 (577 test fns, snapshot infra "wired but only status-bar covered") to #311 / v1.56.0 — the suite roughly doubled and the snapshot/property/pty axes the #37 entry tracked as TODO are now landed. The canonical local gate (`make check`) and the Bitbucket-only CI shape are unchanged in spirit; the counts and inventory are what moved.

Observed:
- **949 test fns** today (`grep -rE "#\[(tokio::)?test\]" --include="*.rs" src tests | wc -l` = 949), up from the 577 the #37 entry recorded. The `make check` gate is unchanged: `Makefile:37` `check: fmt-check lint test deny`; `test` = `cargo test --locked --all-targets` (`Makefile:42-43`), `lint` = `cargo clippy --locked --all-targets -- -D warnings` (`Makefile:46-47`), `deny` = `cargo deny --all-features check` (`Makefile:84`). Pedantic+nursery clippy with a documented allow-list still lives in `Cargo.toml`.
- **Integration tests are now three** (#37 had two): `tests/filesystem.rs`, `tests/keymap_roundtrip.rs`, and the new `tests/pane_roundtrip.rs` — the pty/vt100 roundtrip from PR #60 (`#[cfg(unix)]`, spawns `cat` via portable-pty, drains through vt100, asserts row 0).
- **Snapshot tests expanded from 4 to 14** glyph-level insta snapshots under `src/ui/snapshots/`: list_view (3), prompt (3: simple / vi-insert / vi-normal), status (4: mono/powerline × basic/suffix-or-agent) — the original status-bar 4 plus PR #56's 10 new widget snapshots. Pager-specific test logic lives in `src/ui/pager/tests.rs` (which is itself a large file, 794 LoC — also a 800-LoC-ceiling watch item).
- **Property tests landed (#59):** `proptest` dev-dep + 5 properties across `src/shell/expand.rs` (`shell_quote` round-trip via a test-only POSIX decoder), `src/state/ignore.rs` (`Mask` union/self-match), and `src/keymap/resolver/tests/bindings.rs` (count composition, leading-zero handling). The #37 entry tracked these as "not in CI today / TODO.md [S]".
- **App-harness regression tests (#199/#200)** drive the full resolver→route→dispatch path pty-free (routing/focus, overlay key-consumption, esc-closes-overlay) plus per-agent session-restore coverage — TEST_IMPROVEMENT_PLAN Phase 2.
- **CI is Bitbucket-only**, `bitbucket-pipelines.yml` (no `.github/workflows`). Image is now `rust:1.85` (non-slim, bakes in make/git/curl; was `rust:1.85-slim` at #37) — `bitbucket-pipelines.yml:22`. Three pipeline triggers: `branches.main` and `pull-requests:'**'` each run parallel `quality` (`make check`) + `coverage` steps (`bitbucket-pipelines.yml:203-213`); a `custom: weekly-deps` schedule runs the advisory/outdated drift report (`bitbucket-pipelines.yml:215-222`). Still **no `default:` block** — a branch push without an open PR runs no CI.
- **Coverage gate** is still the ratcheting `--fail-under-lines 35` floor, now invoked as `CARGO_TARGET_DIR=target-cov CARGO_INCREMENTAL=0 cargo llvm-cov --locked --all-targets --fail-under-lines 35` (`bitbucket-pipelines.yml:191`). The instrumented build was isolated into its own `target-cov` cache (PR #64/#65) and `CARGO_INCREMENTAL=0` (PR #69) disables stale-across-runner incremental metadata.
- **Validation commands a contributor runs** (from the Makefile): `make doctor` (preflight), `make` (debug build), `make test`, `make check` (the exact CI gate), optionally `make lint-linux` (clippy for the musl target — catches OS-gated lints the host clippy compiles out; needs zig + cargo-zigbuild, `Makefile:59-68`) and `make install-hooks` (pre-commit runs `make check`). `make aislop` is an advisory net-new-slop scan, deliberately NOT part of `check` (`Makefile:86-106`).

Inferred:
- The MVU refactor's "behavior-equivalence behind green CI (all 786 tests → now 949)" claim is load-bearing on exactly this surface. — confidence: high — basis: `docs/MVU_PLAN.md:13,318-320,549` repeatedly asserts each phase "behavior-equivalent behind green CI (all 786 tests passing)"; that oracle is the snapshot/property/harness build-out reconstructed in `history-arc-01` entry 01KTMMQN283Z4FYP184WT4015X ("a large internal rewrite can only be asserted as no-behavior-change if there is a behavior oracle"). MVU_PLAN's frozen "786" is a point-in-time figure; the live count is 949.
- **Coverage gap:** the 35% line floor almost certainly trails real coverage (suite doubled since the floor was set) and there is no branch-coverage gate. `docs/TEST_IMPROVEMENT_PLAN.md` is still "plan, not yet implemented" (`docs/TEST_IMPROVEMENT_PLAN.md:1-4`, citing "732 tests" — itself now stale vs 949) and names the remaining risk as workflow-composition (full `App` orchestration, pane/pty, background tasks, session restore, MCP socket lifecycle), only partially closed by #199/#200. — confidence: medium — basis: floor unchanged at `--fail-under-lines 35`; TEST_IMPROVEMENT_PLAN status line.

IMPORTANT honest caveat (carry this forward): **this repo does NOT gate merges on green CI.** A broken pipeline can and did reach `main`: PR #64 shipped an invalid `--target-dir` flag to `cargo llvm-cov` and broke main's Coverage step; #65 was a fix-forward. Per `history-arc-01` entry 01KTMMPDE6S4PA834YDR24SX1H: "The repo doesn't gate merges on green pipelines so it landed and broke main's Coverage step." Treat `make check` green locally as the real gate, not the PR pipeline status.

Next query: `watercooler_search(query="ci coverage make-check tests snapshot property pty", thread_topic="onboarding-test-surface", code_path=".")`

Related:
- `onboarding-developer-experience` — `make check` / `make lint-linux` / `make doctor` local mirror of the CI quality step.
- `onboarding-architecture` — state-side (Update half) is still where most unit tests sit; MVU runtime detail in `history-seg-refactor-mvu`.
- the history corpus — `history-arc-01-foundation-hygiene` carries the deep detail: test-surface expansion 01KTMMQN283Z4FYP184WT4015X (#56/#59/#60/#199/#200), CI-caching campaign + "CI is not a merge gate" 01KTMMPDE6S4PA834YDR24SX1H.

Provenance:
- Files read: `Makefile:37-106,59-68`, `bitbucket-pipelines.yml:22,191,203-222`, `Cargo.toml:1-8`, `docs/MVU_PLAN.md:13,318-320,549`, `docs/TEST_IMPROVEMENT_PLAN.md:1-30`. Listed `src/ui/snapshots/` (14 .snap), `src/ui/pager/tests.rs`, `tests/*.rs` (filesystem, keymap_roundtrip, pane_roundtrip).
- Commands run: test-fn count = 949; integration-test ls; snapshot ls.
- History entry_ids consulted: 01KTMMQN283Z4FYP184WT4015X (test-surface expansion), 01KTMMPDE6S4PA834YDR24SX1H (CI-caching + no-merge-gate), 01KTMMVZKGKKZ0Z8VY1Q8TW3D2 (toolchain pin / INSTALL MSRV reconcile).
- Prior #37 entry refreshed: 01KR0PBH6T1AK4VA0JSE75390F.

<!-- Entry-ID: 01KTND1M6375FEN2AH79JJT7RN -->
