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

---
Entry: Claude Code (caleb) 2026-06-09T05:17:41.622113+00:00
Role: scribe
Type: Note
Title: Onboarding refresh: front door re-stated at #311 (v1.56.0), deep-history corpus as the way in

Spec: pm

Purpose: Refresh of onboarding-overview from #37 (v1.41.24, 2026-05-07) to #311 (v1.56.0, 2026-06-08). The #37-era entry (01KR0NZNJ3KM6BJY09Q4P9D0NE) stands as a point-in-time snapshot; this entry re-states the front-door framing at #311, re-points the sibling index and reading order, and — most importantly — routes readers to the deep-history + insight corpus as the canonical way to understand how the codebase got here.

Plain-language framing (#311): spyc is a vi-keyboard-driven file commander that AI agents can BOTH pair with in a pane AND query over MCP. Two panes: the top is a keyboard-driven, vim-flavoured, git-aware file commander; the bottom is a child agent process (Claude Code or codex first-class; Gemini and Antigravity also supported) (`README.md:30-40`). The differentiator is not "Claude in a pane" — it is that "the file commander is the noun the agent operates on, not the chrome around it" (`README.md:21-33`): the agent queries spyc over a local MCP socket for cursor/picks/inventory/filter/git-branch instead of you copy-pasting paths. It was born as `cspy` and the architecture was formed pre-PR-#1 — see `history-arc-00-genesis`.

Observed:
- Identity at #311: name `spyc`, version `1.56.0`, edition `2024`, MSRV `rust-version = 1.88` (`Cargo.toml`). 162 `.rs` files / ~53,818 LoC across a decomposed tree (`src/app/{key_dispatch,pager_handler,render,state}`, `src/git/diff_model`, `src/ui/{diff_render,markdown,pager}`, `src/agent`, `src/config`, `src/keymap/resolver`, `src/mcp`, `src/pane`, `src/state/sessions`).
- CI is `bitbucket-pipelines.yml` only; no `.github/workflows`. Bitbucket is canonical; the GitHub remotes (`origin`=calebjacksonhoward, `tripstack-corp`=Tripstack-Corp) are stale mirrors — see `history-three-repo-lineage`.
- The deep-history corpus now exists and is the refresh's advantage. Window #1–#311 + pre-#1 genesis is fully reconstructed. Front-door readers should use it in this order:
  1. `history-overview` — the reconstruction spine (8 arcs + segment threads, two-layer code/decision history).
  2. `history-arc-00-genesis` — origin (cspy→spyc; architecture formed before PR #1).
  3. Arc threads `history-arc-01`..`history-arc-08` and segment threads `history-seg-*` (refactor-mvu, module-decomposition, gix-migration, docs-planning, performance, markdown-rendering) for per-area depth.
  4. `history-synthesis` — cross-arc synthesis.
  5. The 4 insight tiers — `insight-recurrence`, `insight-drift`, `insight-emergent-properties`, `insight-trajectory` — for the patterns/forces above any single PR.

Sibling index (refreshed seeds; reference by topic — their refresh entries exist):
- `onboarding-product-charter` — what spyc is and the MCP-bridge bet, now matured (multi-agent registry, MVU/architectural-maturity turn).
- `onboarding-team-map` — single-developer project (Derek Marshall / caleb); no CODEOWNERS; Bitbucket-squash authorship topology.
- `onboarding-architecture` — the MVU runtime + sync `std::thread + mpsc` + MCP socket (deep detail in `history-seg-refactor-mvu`).
- `onboarding-working-map` — module index across the decomposed 162-file tree.
- `onboarding-risk-register` — version/size drift; the 800-LoC ceiling not yet fully met.
- `onboarding-test-surface` — 949 test fns + integration/property/snapshot/pty-roundtrip.
- `onboarding-docs-contracts` — the keep-in-sync doc surfaces.
- `onboarding-developer-experience`, `onboarding-release-process`, `onboarding-security`, `onboarding-entry-path`.

Reading order for a first-time engineer (#311): `onboarding-product-charter` → `onboarding-team-map` → `history-overview` (skim) → `onboarding-architecture` → `onboarding-working-map` → `onboarding-entry-path`; pull `history-seg-*` / `insight-*` threads on demand for any area you touch. Security reviewer: start at `onboarding-security` + `onboarding-risk-register`.

Five questions this seed answers:
- What does this product do? → `onboarding-product-charter` (+ `README.md:21-40`).
- Who owns which path? → `onboarding-team-map`.
- How is the code shaped, and how did it get that shape? → `onboarding-architecture` + `onboarding-working-map` + the history corpus (`history-overview` spine).
- What runs in CI / what is the validation surface? → `onboarding-test-surface` (+ `bitbucket-pipelines.yml`).
- Where do docs drift from code? → `onboarding-risk-register` + `onboarding-docs-contracts` (+ `insight-drift`).

Inferred:
- The history/insight corpus is now the highest-leverage onboarding asset, above the static seed prose. — confidence: high — basis: 8 arc threads + 6+ seg threads + 4 insight tiers reconstruct the full #1–#311 window with PR-level provenance; the seeds are now an index into it, not the primary record.

Major #37→#311 shifts (each carries deep detail in a history/insight thread):
- MVU runtime introduced — the project moved from ad-hoc state mutation toward a Model-View-Update loop (`history-seg-refactor-mvu`).
- gix migration — `gix = 0.84` in-process git for status/diff/blame/worktree (`history-seg-gix-migration`); subprocess git scoped-down but NOT fully removed (8 `Command::new("git")` sites remain in `src/git/*` + one test).
- Module decomposition — the ~12k-line `app/mod.rs` and friends split under an 800-LoC ceiling (`history-seg-module-decomposition`); largest remaining is `src/app/mod.rs` at 1009 lines (a live drift item).
- Multi-agent expansion — claude→codex→gemini→agy→zot via the AgentProfile registry (`history-arc-07-codex-and-mcp-bridge`; `src/agent/mod.rs`).
- v1.41 → v1.56 — feature accretion gave way to plan-driven architecture (`insight-trajectory` window-2 mode-shift; `insight-emergent-properties` Property 7 plan-doc-as-executable-spec).

Next query: `watercooler_search(query="history overview arcs reading order", thread_topic="history-overview", code_path=".")`

Related:
- `onboarding-product-charter`, `onboarding-team-map` — refreshed siblings this run.
- the history/insight corpus — `history-overview` (spine), `history-arc-00-genesis` (origin), `history-synthesis` (cross-arc), and the 4 insight tiers carry the deep "why" the seeds only index.

Provenance:
- Files read: `README.md:1-40,21-40`, `ROADMAP.md:1-45`, `Cargo.toml`, `src/agent/mod.rs` (AgentProfile registry), `bitbucket-pipelines.yml` (presence). Commands: `find ... CODEOWNERS` (none), `find src -name '*.rs' | wc -l` (162), `grep -rn 'Command::new("git")' src` (8 sites).
- Prior seed entry read in full: onboarding-overview 01KR0NZNJ3KM6BJY09Q4P9D0NE.
- History/insight threads consulted (existence verified via watercooler_search): history-overview, history-arc-00-genesis, history-synthesis, history-seg-refactor-mvu, history-seg-module-decomposition, history-seg-gix-migration, history-arc-07-codex-and-mcp-bridge, history-three-repo-lineage, insight-recurrence, insight-drift, insight-emergent-properties, insight-trajectory.

<!-- Entry-ID: 01KTND16ZTZXV11FR8KY25MSMJ -->
