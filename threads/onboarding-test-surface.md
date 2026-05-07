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
