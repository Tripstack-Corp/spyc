# onboarding-risk-register — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: onboarding-risk-register
Created: 2026-05-07T07:44:05.308574+00:00

---
Entry: Claude Code (caleb) 2026-05-07T07:44:05.308574+00:00
Role: critic
Type: Note
Title: Onboarding: risk register from bootstrap inspection

Spec: security-audit

Purpose: Surface volatile paths, drift risks, and load-bearing seams that future readers should be aware of before changing anything significant. Findings come from running every cross-check on this bootstrap; nothing is theoretical.

Observed (volatile / load-bearing seams):
- `src/app/mod.rs` is 9087 lines (`wc -l src/app/mod.rs`) and is the focal point of the in-flight Elm-architecture refactor (`ARCHITECTURE.md:42-62`, `REFACTOR_PLAN.md:1-15`, `ROADMAP.md:88-113`). New code that lands here without considering State-side placement makes the seam wider, not narrower.
- The `:command` dispatch has TWO sites: `AppState::dispatch_command` (pure-domain) and `App::dispatch_command` (terminal-touching). State runs first; "any new command handled in `App` MUST be added to state's punt list" (`AGENTS.md:61`). Forgetting it produces an "unknown command:" flash even though the handler exists. Bitten on `:undo` (v1.41.1) and `:limit` historically.
- The MCP socket discovery path was just (v1.41.24, `CHANGELOG.md:9-31`) hardened from "first reachable socket on the host" to "project-scoped via `.spyc-context-<pid>.json` ancestor walk." Anything that touches `src/mcp.rs` discovery or `.mcp.json` writing should preserve the locality invariant — a parent-dir spyc must never shadow a child-dir spyc.
- `cargo-deny` advisory ignores in `deny.toml:72-94` are five long-lived transitive issues (time/yaml-rust/bincode/paste/serial). Each carries a documented `reason`. New ignores must follow that pattern; don't silently add a bare ID.
- Crash recovery posture: `Cargo.toml:60-70` keeps `panic = "unwind"` in the release profile because `pane::Pane::process_bytes_safe` uses `std::panic::catch_unwind` to recover from `vt100` 0.15's known unwrap-on-edge-case panic. Switching to `panic = "abort"` would re-introduce the "one panicking byte stream takes down spyc" failure mode. Worth knowing before "optimizing" the release profile.

Drift findings:
- **#1 Version coherence — `[done — finding recorded]`.** `README.md:14` advertises spyc as "v1.21.1" but `Cargo.toml:3` is `version = "1.41.24"`. The CHANGELOG (`CHANGELOG.md:1-100`) clearly tracks the v1.41.x series so the maintainer's release flow is current; the README banner just hasn't been bumped. ~20 minor versions of drift in user-facing positioning is enough to mention up front.
- **#2 SECURITY.md supported-vs-shipped — `[n/a — SECURITY.md does not list supported versions]`.** `SECURITY.md:1-137` is a "what we do / don't do" posture document, not a per-version support matrix. There is no "Supported versions" claim to compare against, so the cross-check is genuinely n/a here.
- **#3 Generated contracts — `[n/a — none committed]`.** No `*openapi*.json`, generated SDK clients, or committed contract files exist in the repo. The MCP RPC surface is hand-rolled in `src/mcp.rs`; there is no schema artifact to drift against.
- **#4 Translated documentation — `[n/a — no translations]`.** No `i18n/` directory, no localized README. `ROADMAP.md:434-436` lists localization as an explicit non-goal ("English only").
- **#5 MCP / API tool descriptions vs manifests — `[n/a — no manifests]`.** No `server.json`, no `*.mcpb` manifest, no Helm chart. MCP tool surface is described in `AGENTS.md:94-115` and source-of-truth lives in `src/mcp.rs`. With nothing published as a manifest, there is no second surface to drift against. (If publication is added — e.g. an MCP registry entry — this check turns load-bearing.)

Additional drift findings observed during the bootstrap (outside the canonical five but worth flagging):
- Found: `INSTALL.md:57` says "Minimum supported Rust version: **1.80**" but the actual MSRV is 1.85 — `Cargo.toml:8` (`rust-version = "1.85"`), `rust-toolchain.toml:2` (channel `stable` is fine but `bitbucket-pipelines.yml:8` uses `image: rust:1.85-slim`). The README is correct ("Rust 1.85+", `README.md:53`); INSTALL.md is the outlier.
- Found: `CONTRIBUTING.md:160-172` lists `src/app.rs` (a single file) under "Project structure," but the actual layout is the directory module `src/app/` with `mod.rs` and `state.rs` (verified by `ls src/`). `AGENTS.md:38` is correct. The CONTRIBUTING.md section is stale relative to the post-handler-extraction shape (`ROADMAP.md:55-60` Phases 0–4, completed).
- Found: `REFACTOR_PLAN.md:5` describes `app/mod.rs` as "currently ~7400 lines" but the file is now 9087 lines (`wc -l src/app/mod.rs`). The line-count target ("no file in `src/app/` over ~1500 lines", `REFACTOR_PLAN.md:13`) is unaffected, but the "current" baseline is stale by ~23%. The refactor itself is explicitly deferred ("Pre-2.0", `REFACTOR_PLAN.md:21-30`).
- Found: `TODO.md:99-104` documents "cargo-audit in CI quality gate" as completed work, but the CI step actually installs and runs `cargo-deny` (`bitbucket-pipelines.yml:38-44`, `SECURITY.md:42-43`). This is a label-not-content drift — the *intent* (advisories check on every build) is preserved, the *tool name* in the TODO entry is just outdated.
- Found: `Cargo.toml:7` lists `repository = "https://bitbucket.org/tripstack/spyc"` but the active git remote at this clone is `git@github.com:calebjacksonhoward/spyc.git` — a personal mirror predating any future GitHub move. `LAUNCH_PREP.md:21-25` shows the GitHub move as still-open (canonical org account undecided). This is *expected* for the local clone but worth surfacing because anyone cloning from Bitbucket will see Cargo.toml's `repository` field as truth, while readers in this clone will see two different upstreams.
- Found: `Cargo.toml:1-50` declares an MSRV of 1.85 with edition 2024 and `clap` v4 in the dep graph; `clap`'s default-features include the `string` feature, which historically required a recent rustc. The current MSRV is fine; just noting that edition 2024 + pedantic clippy + nursery clippy all share the constraint that future MSRV bumps need a coordinated `rust-toolchain.toml` + `Cargo.toml` + `bitbucket-pipelines.yml` update.

Inferred:
- The dominant near-term risk is *docs drift in user-onboarding surfaces* (README version banner, INSTALL.md MSRV, CONTRIBUTING.md project structure). — confidence: high — basis: three of the four "additional findings" land in surfaces a brand-new contributor reads first. How to apply: any PR that touches version, MSRV, or directory layout should update those three files in the same commit per the doc-sync rule (`AGENTS.md:65-77`).
- The largest maintenance risk that is *not* docs-drift is the `src/app/mod.rs` size + dual `:command` dispatch foot-gun. — confidence: high — basis: `REFACTOR_PLAN.md` explicitly defers the structural fix; `AGENTS.md:61` documents the foot-gun and the symptom. How to apply: when adding a `:` command, grep for the punt list (search for `"bprev"`) and add the new command's name there *and* implement the App-side handler.

Next query: `watercooler_search(query="risk drift volatile", thread_topic="onboarding-risk-register", code_path=".")`

Related:
- `onboarding-overview` — front door.
- `onboarding-docs-contracts` — the doc-sync rule that the additional drift findings violate.
- `onboarding-architecture` — context for the `app/mod.rs` / Elm-refactor seam.
- `onboarding-team-map` — sibling that surfaced the `CONTRIBUTING.md` Project-structure stale section.

Provenance:
- Files read in full or relevant range: `README.md:14,53`, `Cargo.toml:3,7,8,55-70,107-109`, `INSTALL.md:57`, `rust-toolchain.toml:1-5`, `bitbucket-pipelines.yml:8,38-44`, `SECURITY.md:1-137`, `CONTRIBUTING.md:160-172`, `AGENTS.md:38,60-77,94-115`, `ARCHITECTURE.md:42-62`, `REFACTOR_PLAN.md:1-40`, `ROADMAP.md:80-113,434-436`, `TODO.md:99-104`, `deny.toml:72-94`, `CHANGELOG.md:1-100`.
- Commands run: `wc -l src/app/mod.rs src/mcp.rs src/app/state.rs`, `find src -name '*.rs' | wc -l`, `find . -maxdepth 3 -iname 'CODEOWNERS'`, `git log --oneline -20 --name-only`, `git remote get-url origin`.
- Sibling entry_ids: `onboarding-overview = 01KR0NZNJ3KM6BJY09Q4P9D0NE`, `onboarding-architecture = 01KR0P4W3ED1QZ8F44PFB2WPDZ`, `onboarding-team-map = 01KR0P2PC2GBFJ9Z0GPGXZ48Y4`, `onboarding-working-map = 01KR0P6W0YEPJTT0C3CP48NGKV`.

<!-- Entry-ID: 01KR0P9JC8Z3DF6FQ1GJPF3VKA -->
