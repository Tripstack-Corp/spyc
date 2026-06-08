# history-seg-refactor-mvu — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: history-seg-refactor-mvu
Created: 2026-06-08T21:56:29.259875+00:00

---
Entry: Claude Code (caleb) 2026-06-08T21:56:29.259875+00:00
Role: scribe
Type: Note
Title: PR #166 — let-chains MSRV sweep (refactor prep)

Spec: scribe

tags: #history #refactor-mvu

Moment: refactor-mvu — Reconstructed: a tree-wide `if let` → `if-let-chains` rewrite bumps the MSRV gate and collapses nested guard pyramids ahead of the decomposition campaign.   [kind: convention]
When: 2026-05-29 · PR #166 (refactor/let-chains-sweep) · commit d75cab3
Recorded rationale: "refactor: adopt if-let chains across the tree (MSRV 1.85 → 1.88)" — commit d75cab3 subject
Inferred intent: a flatten-the-control-flow prep pass landing the day before the Phase-1 extractions begin (#180, 2026-05-30); the diff shape — `if !chord_locked { if let Some(action) = user.find(&ev) {` → `if !chord_locked && let Some(action) = user.find(&ev) {` in `src/keymap/resolver.rs` — is mechanical guard-collapse across 21 files (551+/565-), consistent with shrinking nesting depth before extracting handlers. — evidence: ledger date 2026-05-29 = one day pre-#180; diff touches resolver/mcp/pager/sessions/tabs broadly with no behavior change.   confidence: high
Supersedes: (none)

Reconstructed: this is the lead-in to the `src/app/` decomposition. It is not itself MVU work, but it raises the language floor (Rust 1.88 let-chains) and de-nests guard logic across the tree, which makes the verbatim handler moves in Phase 1 cleaner. Largest single touch is `src/mcp.rs` (201 lines reshaped) and `src/ui/pager.rs` (64). No new types, no decision rationale beyond the subject — folded as the prep moment of this thread.

Provenance:
- d75cab3 (PR #166 refactor/let-chains-sweep, 2026-05-29) — `if-let` → let-chain collapse across 21 files; sample `src/keymap/resolver.rs` merges a nested `if let` into one `&&`-chained guard; +551/-565, zero behavior change.

<!-- Entry-ID: 01KTMKSAG0R0FQ70HRQW1H4EKW -->

---
Entry: Claude Code (caleb) 2026-06-08T21:57:02.956874+00:00
Role: scribe
Type: Note
Title: PR #180–#191 — Phases 1 & 2: extract leaves + medium handlers out of the mod.rs monolith

Spec: scribe

tags: #history #refactor-mvu

Moment: refactor-mvu — Reconstructed: the ~12.5k-line `src/app/mod.rs` monolith is decomposed into a 12-file `src/app/` directory across two staged passes — six leaf-struct extractions (Phase 1) then four handler extractions (Phase 2) — each a one-PR, behavior-equivalent, zero-test-edit move.   [kind: refactor]
When: 2026-05-30 · PR #180–#185 (Phase 1 leaves), #186/#192/#195 (milestone docs), #187/#189/#190/#191/#193/#194 (Phase 2 + extra extracts)
Recorded rationale: "Smaller, more reviewable, more testable code. Concretely: No file in `src/app/` over ~1500 lines… Side effects modeled as data (`Effect::Spawn { ... }`) so handlers are unit-testable without a real PTY / real signal / real disk." — REFACTOR_PLAN.md (Goal). And the staging rationale: "The mechanical extractions in Phase 1 below buy ~70% of the review-ability win for ~5% of the architectural risk." — REFACTOR_PLAN.md (Why we're not doing this right now)
Inferred intent: a strangler-fig prerequisite — get the leaves and handlers out under green CI before the loop surgery starts. The plan's own done-criteria confirm scope honesty: "⚠️ `wc -l src/app/mod.rs ≤ 4500` is not met (8,427). That number was estimated off a ~7.4k baseline; the real pre-refactor file was ~12.5k." — REFACTOR_PLAN.md (Phase 2 done-criteria).   confidence: high
Supersedes: (none — this is the structural groundwork; later MVU phases build on these modules)

Reconstructed: Phase 1 lifted self-contained data structs that had accreted in `mod.rs` "by inertia, not coupling" (REFACTOR_PLAN.md) — each a verbatim move + `mod …; use …;`:
+ #184 `BackgroundTasks`/`TaskStatus` → `src/app/tasks.rs`
+ #180 `PagerHistory` → `src/app/pager_history.rs`
+ #181 `FindPicker` → `src/app/find_picker.rs`
+ #182 `GrepSession` → `src/app/grep_session.rs`
+ #183 `Prompt`/`PromptKind` → `src/app/prompt.rs`
+ #185 `PendingCapture` → `src/app/capture.rs`
The one-way-dependency rule kept App-coupled `&mut self` methods (spawn_capture, drain) in `app` — "the methods are Phase 2 material, not Phase 1." Result: ~12,450 → ~11,757 LOC.

Phase 2 moved the larger handlers via the child-module `impl App` pattern (private child reads App's private state via the descendant-module rule, so almost nothing became `pub`):
+ #187 `render` + `compute_layout` → `src/app/render.rs` (1114)
+ #189 `handle_pager_key` → `src/app/pager_handler.rs` (1078)
+ #190 `dispatch_command` → `src/app/commands.rs` (321)
+ #191 `handle_key` + mode sub-handlers → `src/app/key_dispatch.rs` (916)
Plus two follow-ons not in the original Phase 2 table: #193 extracted `apply_inner` action dispatch → `src/app/actions.rs` (448 lines, mod.rs −422); #194 extracted save/restore_session → `src/app/session.rs` (340, mod.rs −322). Net: 12,450 → 10,672 → **8,427** lines.

Milestone docs in this window: #186 marked REFACTOR_PLAN Phase 1 complete; #192 marked Phase 2 complete (and noted #188 added `make lint-linux` after a latent `collapsible_if` in `clipboard.rs` failed only on Linux CI — host macOS clippy never lints `cfg(target_os="linux")`). #195 documented the `src/app/` module layout in AGENTS.md/ARCHITECTURE.md and added a hard **anti-monolith guardrail** — a `mod_rs_stays_decomposed` test with `const CEILING = 8_500` that fails CI if `mod.rs` creeps back: "If you hit this: extract a module, don't bump the ceiling… Don't just raise CEILING."

Provenance:
- dd6aa9c/a632b10/116c0fe/6c90857/54ec969/d82600b (PR #180–#185, 2026-05-30) — six leaf-struct extractions, ~1500-line modules each, +structs only.
- 59b6588/673e430/2efbff1/c646415c (PR #187/#189/#190/#191, 2026-05-30) — render/pager-key/command/key-dispatch handler extractions.
- 8ff286d (PR #193, 2026-05-30) — `actions.rs` 448 lines, mod.rs −422. 23f3202 (PR #194) — `session.rs` 340 lines, mod.rs −322.
- REFACTOR_PLAN.md — Goal, Phase 1/2 tables and done-criteria (quoted).
- 33d5454 (PR #195, 2026-05-30) — ARCHITECTURE.md module index + `src/app/mod.rs` anti-monolith guard test (CEILING=8_500).

<!-- Entry-ID: 01KTMKTBHT9G2VBZNJZFY5EJM4 -->
