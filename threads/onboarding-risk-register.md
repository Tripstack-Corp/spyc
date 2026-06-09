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

---
Entry: Claude Code (caleb) 2026-06-09T05:18:16.461887+00:00
Role: critic
Type: Note
Title: Onboarding refresh: risk register from #37 to #311 (v1.56.0)

Spec: security-audit

Purpose: Refresh of the risk register from #37 (v1.41.24) to #311 (v1.56.0). The #37-era entry (`01KR0P9JC8Z3DF6FQ1GJPF3VKA`) stays as a point-in-time snapshot; this entry re-runs all five numbered cross-checks against the current tree and re-scores the volatile seams after the MVU runtime, the 800-LoC decomposition, and the gix migration churned nearly every file.

Observed (current volatile / load-bearing seams):
- **High recent-churn surface.** The #38–#311 window rebuilt the control flow (MVU runtime, `history-seg-refactor-mvu`) and split the tree under a codified ~800-line ceiling (`history-seg-module-decomposition`, CLAUDE.md convention from PR #282). Nearly every file moved. Treat any pre-#311 line-number citation in older docs/threads as suspect — the decomposition relocated handlers wholesale. `insight-emergent-properties` Property 9 (`01KTMVKP814X`) names this exact inversion: window-1 grew additively by stacking; window-2 grows subtractively by splitting.
- **`src/app/mod.rs` is still 1009 lines** (`wc -l src/app/mod.rs`), over the 800 ceiling. It is the single remaining over-ceiling production file — the next-largest are `src/ui/pager/tests.rs` 794 and `src/app/tasks.rs` 792 (both under). So the ceiling is *almost* met; `app/mod.rs` is the one live exception and the focal seam for the next decomposition pass. `insight-emergent-properties` Property 11 (`01KTMVPK0QRK`): abstraction seams precipitate from repetition — the next split should fall out of where `app/mod.rs` repeats.
- **Partial gix migration — 8 subprocess git sites remain.** `gix = 0.84` (Cargo.toml) went in-process for status/diff/blame/worktree (#283–#292, `history-seg-gix-migration`), but `grep -rn 'Command::new("git")' src` returns 8 live sites: `src/git/blame.rs:113`, `src/git/status.rs:484`, `src/git/status.rs:506`, `src/git/worktree.rs:370`, `src/git/discovery.rs:52`, `src/git/diff_model/mod.rs:219` (6 production), plus `src/app/state/tests/mod.rs:302` and `:373` (2 test fixtures). The "drop-subprocess" was SCOPED, not total — do NOT read the gix thread as "subprocess git is gone." Each remaining production site is a fork+exec dependency on a `git` binary on PATH.
- **Not-gated-on-green-CI practice persists.** `history-arc-01-foundation-hygiene` documents `make check` / CI as the gate; but `insight-drift` window-2 Pattern A′ (`01KTMTQ8RT04`) records squash-subject understatement at structural scale and that the honest framing relocated to CHANGELOG + plan docs — i.e. the recorded "why" lives in the tree, not in pre-merge review state. `insight-emergent-properties` Property 13 (`01KTMVST4Q84`) is the counterweight: green CI + behavior-equivalence/guard tests ARE the precondition the strangler-figs ran against (949 test fns). The risk is not "no tests" — it is that the merge gate is the maintainer's local `make check`, not an enforced branch-protection CI status, so a red push is structurally possible.
- **Crash-recovery posture retained.** `Cargo.toml:107` keeps `panic = "unwind"` (comment block `Cargo.toml:97-102`) so `std::panic::catch_unwind` in the pane vt100 path still recovers from vt100's unwrap-on-edge-case panic (`history-arc-08` PR #30, `01KR393P15VT`). Flipping to `abort` re-introduces the "one panicking byte stream kills spyc" failure mode.
- **MCP socket discovery invariant** is now in `src/mcp/mod.rs:50-54` (PID-scoped `~/.local/state/spyc/mcp-<pid>.sock`) with stale-socket cleanup gated to no-peer errors only (`src/mcp/server.rs:58-62`). Preserve the locality + no-race-delete invariants when touching `src/mcp/`.

Drift findings (all 5 numbered cross-checks, by name):
- **#1 Version coherence — [done — finding].** `Cargo.toml` = `version = "1.56.0"`, edition 2024, rust-version 1.88. `CHANGELOG.md` head is `[Unreleased]` then `[1.56.0] - 2026-06-06` (`CHANGELOG.md:6,28`) — coherent. BUT the #37-era README "v1.21.1" banner is GONE (README is now badge-less), and `README.md:73` advertises "**Rust** 1.85+" while the actual MSRV is **1.88** (`Cargo.toml` rust-version). So the version drift moved: the version-string drift the #37 entry flagged is resolved; a NEW MSRV-string drift opened (README says 1.85, Cargo says 1.88).
- **#2 SECURITY.md supported-vs-shipped — [n/a — no supported-versions matrix].** `SECURITY.md` (137 lines) is a posture document ("what we do / don't do") with a "When to revisit this document" trigger list (`SECURITY.md:122-136`), not a per-version support table. Nothing to compare against current major 1.x. Same disposition as the #37 entry — confirmed still n/a.
- **#3 Generated contracts — [done — none committed].** No `*openapi*.json`, no generated SDK, no committed contract artifact. The MCP RPC surface is hand-rolled in `src/mcp/protocol.rs`; there is no schema artifact to drift against. Confirmed.
- **#4 Translated docs — [n/a — no i18n].** No `i18n/`, no localized README; English-only remains the stated non-goal. Confirmed.
- **#5 MCP/API tool descriptions vs manifests — [done].** `src/mcp/protocol.rs:130-285` exposes exactly 10 tools (`get_spyc_context`, `navigate_to`, `set_filter`, `pick_files`, `clear_picks`, `get_file_content`, `search_paths`, `search_content`, `search_picks`, `search_inventory`). `AGENTS.md:149-172` documents all of them by name. No published manifest (`server.json`/`*.mcpb`) exists, so source IS the manifest — and the in-tree AGENTS.md mirror is coherent with the source. No drift.

Inferred:
- The dominant near-term risk shifted from docs-version-drift to **structural-debt-residue**: one over-ceiling file (`app/mod.rs` 1009) and 8 lingering subprocess-git sites are the two unfinished tails of otherwise-completed campaigns. — confidence: high — basis: both are the single remaining exceptions to a near-complete rule (800-LoC ceiling; in-process gix). How to apply: when you touch `app/mod.rs` or `src/git/{blame,status,worktree,discovery,diff_model}.rs`, finish the campaign in the same PR rather than widening the exception.
- The merge-gate risk (local `make check`, not enforced CI status) is real but bounded by a strong test substrate (949 tests, guard/equivalence tests). — confidence: medium — basis: `insight-emergent-properties` Property 13 vs the absence of `.github/workflows` + Bitbucket branch-protection being maintainer-config not in-tree. How to apply: do not assume a green pipeline ran on `main`; run `make check` locally before trusting HEAD.

Next query: `watercooler_search(query="risk drift volatile app/mod gix subprocess", thread_topic="onboarding-risk-register", code_path=".")`

Related:
- `onboarding-docs-contracts` — the doc-sync rule the README-MSRV drift violates.
- `onboarding-security` — sibling re-scoring the gix subprocess attack surface and the unsafe count.
- `insight-drift` window-2 (`01KTMTNWVQJZ` framing; A′ `01KTMTQ8RT04`) and `insight-emergent-properties` Properties 7–13 (`01KTMVGQPG64`…`01KTMVST4Q84`) — the deep drift/risk pattern corpus.
- `history-seg-gix-migration`, `history-seg-module-decomposition`, `history-arc-01-foundation-hygiene` — campaign detail.

Provenance:
- Commands: `wc -l src/app/mod.rs` (1009); `find src -name '*.rs' -exec wc -l {} + | sort -rn | head` (top files); `grep -rn 'Command::new("git")' src` (8 sites listed inline); `grep -E 'gix|version|rust-version' Cargo.toml`; `grep -n 'panic' Cargo.toml` (`:107` unwind).
- Files: `Cargo.toml` (version 1.56.0 / rust-version 1.88 / `:97-107` panic block), `README.md:73` (Rust 1.85+), `CHANGELOG.md:6,28`, `SECURITY.md:122-136`, `src/mcp/protocol.rs:130-285`, `AGENTS.md:149-172`, `src/mcp/mod.rs:50-54`, `src/mcp/server.rs:58-62`, `src/main.rs:314`.
- History/insight entry_ids consulted: `insight-drift` window-2 `01KTMTNWVQJZ`/`01KTMTQ8RT04`; `insight-emergent-properties` `01KTMVKP814X`(P9)/`01KTMVPK0QRK`(P11)/`01KTMVST4Q84`(P13); `history-arc-08` `01KR393P15VT`(vt100 PR #30); #37-era snapshot `01KR0P9JC8Z3DF6FQ1GJPF3VKA`.

<!-- Entry-ID: 01KTND28QCJS2SJNXVSVGJGYAJ -->
