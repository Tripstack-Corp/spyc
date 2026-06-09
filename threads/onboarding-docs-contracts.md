# onboarding-docs-contracts — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: onboarding-docs-contracts
Created: 2026-05-07T07:46:22.382618+00:00

---
Entry: Claude Code (caleb) 2026-05-07T07:46:22.382618+00:00
Role: scribe
Type: Plan
Title: Onboarding: docs and contract surface map

Spec: docs

Purpose: Map the docs / contract / config surfaces that must stay synchronized with code, name the maintainer's "keep in sync" rule that binds them, and call out the surfaces that are actually drifting today.

Observed:
- **The doc-sync rule is explicit and treated as a gate, not a nice-to-have.** `AGENTS.md:65-77` lists the surfaces that must be updated *in the same commit* as code changes affecting user-visible behavior, keybindings, or project status:
  - `README.md` — positioning, install instructions, keybinding tables.
  - `FEATURES.md` — complete feature reference.
  - `AGENTS.md` — module index, conventions, "what it does" summary.
  - `ARCHITECTURE.md` — only when an architectural decision changes.
  - `DESIGN.md` — only when the UI design language changes.
  - `ROADMAP.md` — move shipped items to "Done (recent)", update track status.
  - `BUGS.md` — move fixed bugs to FIXED section.
  - `CHANGELOG.md` — add entry under Unreleased.
  - `INSTALL.md` — if build/install steps change.
  - `src/ui/help.rs` — if keybindings or user-facing commands change.
  `CONTRIBUTING.md:104-115` reinforces the rule with "Stale docs are bugs."
- **`ARCHITECTURE.md:157-174`** lists the same set in compressed form ("Documentation contract") and labels each with its scope: stable principles vs forward plan vs release notes vs known bugs vs landing page vs in-app help.
- **Top-level docs at the repo root** (verified by `ls -1`): `README.md`, `AGENTS.md`, `ARCHITECTURE.md`, `BUGS.md`, `CHANGELOG.md`, `CONTRIBUTING.md`, `DESIGN.md`, `FEATURES.md`, `INSTALL.md`, `LAUNCH_PREP.md`, `LICENSE`, `REFACTOR_PLAN.md`, `ROADMAP.md`, `SECURITY.md`, `TODO.md`. Plus `docs/` (presentation HTML, logo PNG, logo SVG, screen_shot.png).
- **In-tree contract surfaces** (config / tool / output formats):
  - `.spycrc.toml` config DSL — parsed by `src/config/dsl.rs`. The fully-commented default template is generated at runtime by `spyc --print-config` (`README.md:281-303`, `AGENTS.md:27`); writing it through `spyc --print-config > ~/.spycrc.toml` is the documented bootstrap path. The surface itself is the union of `[layout]`, `[colors]`, the `keymap = [...]` DSL, ignore masks, etc.
  - MCP RPC surface — `src/mcp.rs` (the JSON-RPC server) + `src/mcp_cmd.rs` (command channel types). Tool surface documented for in-pane Claude in `AGENTS.md:94-115`. There is **no** separate published manifest (no `server.json`, no `*.mcpb`, no Helm chart) — the source is the manifest.
  - `.spyc-context-<pid>.json` markers + `~/.local/state/spyc/mcp-<pid>.sock` — load-bearing for the project-scoped MCP socket discovery added in v1.41.24 (`CHANGELOG.md:9-31`). Format and ownership rules documented in `ARCHITECTURE.md:135-155` and `SECURITY.md:101-107`.
  - `~/.local/state/spyc/` state files (`inventory.json`, `marks.json`, `history.json`, `sessions/<epoch-ms>.json`) — XDG-compliant (`ARCHITECTURE.md:114-128`); migration / health-check rules in `src/state/health.rs`.
- **Build / supply-chain config surfaces** (each is a contract with CI):
  - `Cargo.toml`, `Cargo.lock` (committed), `rust-toolchain.toml` (`stable`, MSRV 1.85 via image pin), `deny.toml` (advisory ignores + license allow-list + source allow-list).
  - `Makefile` is the canonical surface (`make check` is what CI runs); `Justfile` is a parallel-but-thinner alternative (no `deny`, no `install`, no cross-compile checksums).
  - `bitbucket-pipelines.yml` is the only CI surface; there is no `.github/workflows/`.
- **Release / distribution manifests at root**: none today. There is no `Dockerfile`, no `Containerfile`, no Helm chart, no `server.json`, no `*.mcpb` manifest, no `.changeset/`, no `.goreleaser.yaml`, no `release-please-config.json`. `LAUNCH_PREP.md` and `ROADMAP.md` Distribution track scope future release automation.
- **Governance files at root**: `SECURITY.md` (vulnerability disclosure + threat model), `CONTRIBUTING.md` (PR workflow + conventions). Missing relative to common practice: no `CODE_OF_CONDUCT.md`, no `CODEOWNERS`, no PR/issue templates. All three are tracked as `LAUNCH_PREP` / `ROADMAP.md:415-419` distribution-track items, not active gaps.
- **In-app help is treated as a doc surface.** `src/ui/help.rs` is the `?` overlay; `AGENTS.md:75` and `CONTRIBUTING.md:113` both list it under "keep in sync." Not a generated artifact — hand-maintained in source.

Drift findings:
- **#1 Version coherence — `[done — finding recorded]`.** `README.md:14` advertises `v1.21.1`; `Cargo.toml:3` is `1.41.24`. CHANGELOG (`CHANGELOG.md:1-100`) tracks the v1.41.x series so the maintainer's release flow is current; the README "v1.21.1" line in the centred header just hasn't been bumped. (Also flagged in `onboarding-risk-register`.)
- **#2 SECURITY.md supported-vs-shipped — `[n/a — no supported-versions claim]`.** `SECURITY.md:1-137` is a posture document, not a per-version support matrix; nothing to compare against.
- **#3 Generated contracts — `[n/a — none committed]`.** No `*openapi*.json`, generated SDK, or contract artifact exists in the tree.
- **#4 Translated documentation — `[n/a — no translations]`.** No `i18n/` directory; localization is an explicit non-goal (`ROADMAP.md:434-436`).
- **#5 MCP / API tool descriptions vs manifests — `[n/a — no manifests]`.** No `server.json`, no `*.mcpb`. Source-of-truth is `src/mcp.rs`. (Future publication will turn this load-bearing.)
- Additional findings (outside the canonical five):
  - Found: `INSTALL.md:57` says MSRV "1.80"; actual MSRV is 1.85 (`Cargo.toml:8`, `rust-toolchain.toml`, `bitbucket-pipelines.yml:8`, `README.md:53`).
  - Found: `CONTRIBUTING.md:160-172` "Project structure" lists `src/app.rs` (single file); actual layout is `src/app/` (directory module with `mod.rs` + `state.rs`). `AGENTS.md:38` is correct.
  - Found: `TODO.md:99-104` references `cargo-audit` as the CI advisory tool; the pipeline actually uses `cargo-deny` (`bitbucket-pipelines.yml:38-44`, `SECURITY.md:42-43`).
  - Found: `REFACTOR_PLAN.md:5` describes `app/mod.rs` as "currently ~7400 lines"; actual is 9087 lines.

Inferred:
- The maintainer's doc-sync rule is the strongest contract in the repo and CONTRIBUTING.md:115 explicitly elevates it ("Stale docs are bugs"). — confidence: high — basis: identical lists in `AGENTS.md:65-77`, `CONTRIBUTING.md:104-115`, and `ARCHITECTURE.md:157-174` (compressed). How to apply: when reviewing a PR, treat missing doc updates as a blocker, not a polish item.
- `AGENTS.md` is the de-facto authoritative surface for module shape and conventions; when other docs disagree (CONTRIBUTING.md project structure, REFACTOR_PLAN.md line counts, INSTALL.md MSRV), `AGENTS.md` is current and the others are stale. — confidence: high — basis: every cross-check that produced a finding above. How to apply: when in doubt, read AGENTS.md first.

Next query: `watercooler_search(query="docs sync architecture features changelog", thread_topic="onboarding-docs-contracts", code_path=".")`

Related:
- `onboarding-overview` — front door.
- `onboarding-risk-register` — sibling that catalogues the same drift findings with a risk lens.
- `onboarding-test-surface` — validation step #6 enforces the doc-sync rule per-commit.
- `onboarding-developer-experience` — `make check` does not currently include a doc-sync linter, so this contract is enforced at review-time, not CI-time.

Provenance:
- Files read: `AGENTS.md:65-77,94-115`, `CONTRIBUTING.md:104-115,160-172`, `ARCHITECTURE.md:114-174`, `README.md:14,53,281-303`, `INSTALL.md:57`, `bitbucket-pipelines.yml:8,38-44`, `SECURITY.md:1-137`, `Cargo.toml:1-50,55-70`, `rust-toolchain.toml:1-5`, `deny.toml:1-94,104-124`, `Makefile:36-62`, `Justfile:1-49`, `ROADMAP.md:415-419,434-436`, `LAUNCH_PREP.md:1-40`, `TODO.md:99-104`, `REFACTOR_PLAN.md:1-15`, `CHANGELOG.md:1-100`.
- Commands run: `ls -1`, `ls -1 docs`, `find src -name 'help.rs'`.
- Sibling entry_ids: `onboarding-overview = 01KR0NZNJ3KM6BJY09Q4P9D0NE`, `onboarding-risk-register = 01KR0P9JC8Z3DF6FQ1GJPF3VKA`, `onboarding-test-surface = 01KR0PBH6T1AK4VA0JSE75390F`.

<!-- Entry-ID: 01KR0PDR0APW4N1QC03C8JWSQX -->

---
Entry: Claude Code (caleb) 2026-06-09T05:19:15.623002+00:00
Role: scribe
Type: Plan
Title: Onboarding refresh: docs and contract surface map from #37 to #311 (v1.56.0)

Spec: docs

Purpose: Refresh of the docs / contract surface map from #37 (v1.41.24) to #311 (v1.56.0). The #37-era entry (`01KR0PDR0APW4N1QC03C8JWSQX`) stays as a snapshot; this entry re-enumerates every root + docs/ markdown file, re-assesses currency vs the MVU/gix/decomposed reality, and re-runs all five numbered cross-checks. The window's defining shift: docs became plan-doc-RICH and the recorded "why" relocated INTO committed artifacts under squash-merge.

Observed (current doc/contract surfaces):
- **Root markdown (15 files, `ls *.md`):** `README.md`, `AGENTS.md`, `ARCHITECTURE.md`, `BUGS.md`, `CHANGELOG.md`, `CLAUDE.md`, `CONTRIBUTING.md`, `DESIGN.md`, `FEATURES.md`, `INSTALL.md`, `LAUNCH_PREP.md`, `REFACTOR_PLAN.md`, `ROADMAP.md`, `SECURITY.md`, `TODO.md`. (`CLAUDE.md` is NEW since #37 — it carries the ~800-line ceiling convention from PR #282.)
- **docs/ markdown (10 files + 1 html + 3 images, `ls docs/`):** `MVU_PLAN.md`, `V1_5_PLAN.md`, `V1_60_PLAN.md`, `V1_70_PLAN.md`, `REFACTOR_PLAN.md` (lives at root), `AUTO_APPROVAL_PLAN.md`, `PANE_RECOVERY_PLAN.md`, `PANE_STARTUP_TABS_PLAN.md`, `PATH_HANDOFF_PLAN.md`, `TEST_IMPROVEMENT_PLAN.md`, `YAZI_COMPETITIVE_REVIEW.md`; plus `presentation.html`, `screen_shot.png`, `spyc-logo.png`, `spyc-logo.svg`.
- **Plan-doc set — executed vs aspirational (cite `insight-trajectory` window-2, framing `01KTMTN3M67G`):**
  - EXECUTED: `docs/MVU_PLAN.md` — 8-phase strangler-fig migration executed phase-by-phase against named done-criteria; the window's near-exact positive execution (`01KTMTPYARF1`). `docs/V1_5_PLAN.md` — six-phase pager/task-viewer unification, shipped v1.50.0, second near-exact execution (`01KTMTSTYW0Z`). `REFACTOR_PLAN.md` — Phases 1–2 executed; its ~1500-line target was SUPERSEDED by CLAUDE.md's ~800-line ceiling, Phase 3 delegated wholesale to MVU_PLAN (`01KTMTR9XYV5`).
  - ASPIRATIONAL (filed, "plan not yet implemented" at #311): `docs/V1_60_PLAN.md` (CounterTop) and `docs/V1_70_PLAN.md` (Mise en Place) — both forward-architecture, authoring is the in-window event, execution is beyond the window (`01KTMTV4HNBF`). `AUTO_APPROVAL_PLAN.md` / `PANE_RECOVERY_PLAN.md` / `PANE_STARTUP_TABS_PLAN.md` — three feature plans, all not-yet-implemented, each carrying an honored negative-register element (`01KTMTWQ02AV`). So a reader must NOT treat the docs/ plan set as describing shipped behavior: MVU/V1_5/REFACTOR = history; V1_60/V1_70/AUTO_APPROVAL/PANE_* = intent.
- **ARCHITECTURE.md / DESIGN.md currency vs MVU/gix/decomposed reality:** these are the "only update when the decision changes" surfaces (per the doc-sync list). The MVU runtime, the gix backend, and the decomposed `src/app/{key_dispatch,pager_handler,render,state}` tree are all architectural decisions that landed this window — verify ARCHITECTURE.md names them before citing it as current (the module-shape source of truth is now `AGENTS.md`, which DOES enumerate the decomposed tree at `AGENTS.md:41-70` incl. `src/mcp/` facade and `src/app/state/*` handlers).
- **CHANGELOG.md is the de-facto rationale record.** `insight-emergent-properties` Property 10 (`01KTMVN7JZ7T`, "rationale-externalized-under-squash"): squash-merge erases pre-merge commit-body rationale, so the recorded "why" relocated into CHANGELOG + plan docs in the tree. The `[Unreleased]` section (`CHANGELOG.md:6+`) currently carries dense "why" prose (e.g. the `make aislop` baseline-gate rationale, the ratatui 0.30.1 upgrade reasoning). Treat CHANGELOG as a primary design-rationale source, not just release notes.
- **MCP tool surface (cross-check #5 surface):** `src/mcp/protocol.rs:130-285` is the source of truth (10 tools); `AGENTS.md:149-172` is the in-tree mirror Claude reads. No published manifest.

Drift findings (all 5 numbered cross-checks, by name):
- **#1 Version coherence — [done — finding].** `Cargo.toml` = 1.56.0 / rust-version 1.88. `CHANGELOG.md:28` head release = `[1.56.0] - 2026-06-06` — coherent. The #37-era README "v1.21.1" banner is resolved (README is badge-less now). NEW drift: `README.md:73` says "**Rust** 1.85+" but MSRV is **1.88** (`Cargo.toml` rust-version). MSRV-string is stale in README; update in the same commit per the doc-sync rule.
- **#2 SECURITY.md supported-vs-shipped — [n/a — no supported-versions matrix].** `SECURITY.md` is a posture doc with a "When to revisit" trigger list (`SECURITY.md:122-136`), not a per-version table. Nothing to compare against major 1.x. Confirmed n/a (unchanged from #37).
- **#3 Generated contracts — [done — none committed].** No `*openapi*.json`, no generated SDK, no committed contract artifact. `presentation.html` is hand-authored, not generated. Confirmed.
- **#4 Translated docs — [n/a — no i18n].** No `i18n/`, no localized markdown. Confirmed.
- **#5 MCP/API tool descriptions vs manifests — [done].** Compared `src/mcp/protocol.rs:130-285` (10 tool names) against `AGENTS.md:149-172`: every tool the source exposes is documented in AGENTS.md (`get_spyc_context`, `navigate_to`, `set_filter`, `pick_files`, `clear_picks`, `get_file_content`, `search_paths`, `search_content`, `search_picks`, `search_inventory`). No retired/renamed/missing tool. The two surfaces are coherent — no drift.

Inferred:
- `AGENTS.md` is now the authoritative in-tree surface for both module shape AND the MCP tool contract; when a plan doc or ARCHITECTURE.md disagrees, AGENTS.md is current. — confidence: high — basis: cross-check #5 coherence + AGENTS.md:41-70 enumerating the decomposed tree. How to apply: read AGENTS.md first when reconciling docs.
- The docs/ plan set is bimodal (executed-history vs filed-intent) and must be read through `insight-trajectory` window-2 to avoid mistaking a filed plan for shipped behavior. — confidence: high — basis: trajectory window-2 dispositioned all ten plan docs. How to apply: before citing any `docs/*_PLAN.md` as current behavior, check its trajectory disposition.

Next query: `watercooler_search(query="docs plan-doc changelog rationale mcp tools", thread_topic="onboarding-docs-contracts", code_path=".")`

Related:
- `onboarding-risk-register` — sibling logging the same README-MSRV drift with a risk lens.
- `onboarding-security` — SECURITY.md currency (cross-check #2 surface).
- `insight-trajectory` window-2 (`01KTMTN3M67G`) — the executed-vs-aspirational plan-doc dispositions.
- `insight-emergent-properties` Property 10 (`01KTMVN7JZ7T`) — rationale-externalized-under-squash, why CHANGELOG is the "why" record.

Provenance:
- Commands: `ls *.md` (15 root), `ls docs/` (10 .md + html + 3 images), `grep -nE '^## \[' CHANGELOG.md` (`:6` Unreleased, `:28` 1.56.0), `grep -niE 'Rust|version' README.md` (`:73` Rust 1.85+), `grep -E '"name"' src/mcp/protocol.rs` (10 tools).
- Files: `Cargo.toml` (1.56.0/1.88), `CHANGELOG.md:6,28`, `README.md:73`, `SECURITY.md:122-136`, `src/mcp/protocol.rs:130-285`, `AGENTS.md:41-70,149-172`.
- History/insight entry_ids: `insight-trajectory` window-2 `01KTMTN3M67G`/`01KTMTPYARF1`/`01KTMTR9XYV5`/`01KTMTSTYW0Z`/`01KTMTV4HNBF`/`01KTMTWQ02AV`; `insight-emergent-properties` P10 `01KTMVN7JZ7T`; #37-era snapshot `01KR0PDR0APW4N1QC03C8JWSQX`.

<!-- Entry-ID: 01KTND42QMQYH4HMD61DJW11DH -->
