# onboarding-overview — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: onboarding-overview
Created: 2026-05-07T07:38:41.221996+00:00

---
Entry: Claude Code (caleb) 2026-05-07T07:38:41.221996+00:00
Role: scribe
Type: Note
Title: Onboarding: repository overview and reading order

Spec: docs

Purpose: Front door for spyc onboarding. spyc is a vi-keyboard-driven Rust TUI file commander that runs an MCP server on a PID-scoped Unix domain socket so Claude Code (running in spyc's split pane) can query and mutate the live file-list state. This entry indexes the other seed threads and gives a reading order for first-time readers.

Observed:
- Repo identity at bootstrap: name `spyc`, Cargo version `1.41.24` (`Cargo.toml:2-3`), license `BSD-3-Clause` (`Cargo.toml:6`), MSRV `1.85` (`Cargo.toml:8`, `rust-toolchain.toml:2`, `bitbucket-pipelines.yml:8`), upstream `repository = "https://bitbucket.org/tripstack/spyc"` (`Cargo.toml:7`), active git remote `git@github.com:calebjacksonhoward/spyc.git` (a personal mirror — see `onboarding-product-charter` and the GitHub-move decision in `LAUNCH_PREP.md:21-25`), single binary at `src/main.rs` (`Cargo.toml:10-12`), branch `main` at commit `a303251` clean.
- Sibling index (planned this run; raw entry_id ULIDs in Provenance):
  - `onboarding-product-charter` — what spyc is, who it's for, the MCP-bridge bet (Roadmap thesis at `ROADMAP.md:3-23`).
  - `onboarding-team-map` — single-developer project (Derek Marshall, 321 commits in last 6 months); no CODEOWNERS.
  - `onboarding-architecture` — sync-only `std::thread + mpsc`, Elm-architecture target, MCP socket transport (per `ARCHITECTURE.md`).
  - `onboarding-working-map` — module index across the 55 `.rs` files in `src/`, anchored by `AGENTS.md:38-56`.
  - `onboarding-risk-register` — version-skew + MSRV-skew drift findings, `app/mod.rs` size, `cargo-deny` ignores in `deny.toml:72-94`.
  - `onboarding-test-surface` — ~577 `#[test]` sites, `make check` quality gate, 35% line-coverage floor (`bitbucket-pipelines.yml:54`).
  - `onboarding-docs-contracts` — the eight-doc "keep in sync" contract in `AGENTS.md:65-77` and `ARCHITECTURE.md:157-174`.
  - `onboarding-developer-experience` — `Makefile` (canonical), `Justfile` (alt), `make doctor`, cross-compile via `cargo-zigbuild`, optional pre-commit hook.
  - `onboarding-release-process` — local `make install` only today; no published artifacts; release automation tracked in `ROADMAP.md` Distribution and `LAUNCH_PREP.md`.
  - `onboarding-security` — internal Tripstack tool, no network code, MCP socket gated by FS perms; threat model in `SECURITY.md`.
  - `onboarding-entry-path` — recommended first tasks per role.
- Five questions this seed answers:
  - What does this product do? → `onboarding-product-charter`.
  - Who is responsible for which path? → `onboarding-team-map`.
  - How is the code shaped? → `onboarding-architecture` + `onboarding-working-map`.
  - What runs in CI, and what's the validation surface? → `onboarding-test-surface`.
  - Where are docs likely to drift from code? → `onboarding-risk-register` + `onboarding-docs-contracts`.

Inferred:
- Reading order for a first-time engineer: `onboarding-product-charter` → `onboarding-team-map` → `onboarding-architecture` → `onboarding-working-map` → `onboarding-entry-path`, then `onboarding-risk-register` / `onboarding-test-surface` / `onboarding-docs-contracts` as needed. — confidence: high — basis: the engineering surfaces only make sense after the MCP-bridge thesis (`ROADMAP.md:3-23`) is internalized; everything else is supporting infrastructure (`ROADMAP.md:17-22`).
- Reading order for a security reviewer: start at `onboarding-security` and `onboarding-risk-register`. — confidence: high — basis: `SECURITY.md` is the maintainer's own framing of scope.
- This is the first Watercooler bootstrap on this repo. — confidence: high — basis: `watercooler_list_threads` returned "No threads found", `watercooler_search` returned "Graph not available" (no baseline graph yet), pulse_snapshot disabled.

Next query: `watercooler_search(query="onboarding overview", thread_topic="onboarding-overview", code_path=".")`

Related:
- `onboarding-product-charter` — what spyc is and the bet it represents.
- `onboarding-team-map` — accountability for the code.
- `onboarding-architecture` — how the engine is shaped.
- `onboarding-working-map` — module index.
- `onboarding-risk-register` — drift and volatile surfaces.
- `onboarding-test-surface` — validation gates.
- `onboarding-docs-contracts` — the keep-in-sync surfaces.
- `onboarding-developer-experience` — local build / lint / test / cross-compile.
- `onboarding-release-process` — install + future release automation.
- `onboarding-security` — threat model and supply-chain controls.
- `onboarding-entry-path` — first tasks for new contributors.

Provenance:
- Files read in full: `README.md`, `ARCHITECTURE.md`, `AGENTS.md`, `CONTRIBUTING.md`, `SECURITY.md`, `Cargo.toml`, `bitbucket-pipelines.yml`, `Makefile`, `Justfile`, `deny.toml`, `rust-toolchain.toml`, `ROADMAP.md`, `build.rs`, `src/main.rs`, `LAUNCH_PREP.md` (first 40 lines), `REFACTOR_PLAN.md` (first 40 lines), `INSTALL.md` (first 60 lines), `DESIGN.md` (first 80 lines), `CHANGELOG.md` (first 100 lines), `BUGS.md` (first 120 lines), `TODO.md`.
- Commands run: `pwd`, `git remote get-url origin`, `git branch --show-current`, `git log --oneline -1`, `git status --short`, `git log --oneline -20 --name-only`, `git log --use-mailmap --since="6 months ago" --format='%an' | sort | uniq -c`, `find src -name '*.rs' | wc -l`, `grep -rE "#\\[test\\]" --include="*.rs" src tests | wc -l`, `wc -l src/app/mod.rs src/mcp.rs src/app/state.rs`, `find . -maxdepth 3 -iname 'CODEOWNERS'`.
- Watercooler tools called: `watercooler_health`, `watercooler_whoami`, `watercooler_roles`, `watercooler_list_threads(scan=true)`, `watercooler_pulse_snapshot`, `watercooler_search(query="architecture decision implementation test docs release", query_operator="OR")`.
- Sibling entry_ids: pending for every sibling at write time; back-fill is the responsibility of subsequent seed writers.

<!-- Entry-ID: 01KR0NZNJ3KM6BJY09Q4P9D0NE -->
