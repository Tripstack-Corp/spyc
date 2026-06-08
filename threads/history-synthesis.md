# history-synthesis — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: history-synthesis
Created: 2026-06-08T22:23:32.301521+00:00

---
Entry: Claude Code (caleb) 2026-06-08T22:23:32.301521+00:00
Role: scribe
Type: Note
Title: Phase 3 arc layer: how to read the #38–#311 synthesis (opener)

Spec: scribe

tags: #history #synthesis

Purpose: open the **Phase 3 arc layer** for the #38–#311 window. The per-moment `history-seg-*` / `history-arc-*` threads are the durable, queryable record — honest but not always readable as a subsystem-evolution *arc*. This thread holds one **readable arc entry per segment**, generated from that segment's committed moments. The arcs are **secondary by rule**: they summarize, never replace, the per-moment logs; every substantive claim in an arc carries an inline moment reference `[entry_id]`, recorded rationale may be quoted, and inferred reads stay marked `(inferred)`. No claim appears in an arc that is not already in the atomic layer.

Arc entry shape (per segment): `Arc: <segment> — <throughline>` · `Span: <first→last date · N moments · M supersessions>` · `Narrative` (2–5 short paragraphs, reconstruction voice, inline `[entry_id]` per claim) · `Lineage` (the supersession spine) · `Open/unsettled` · `Confidence` (recorded-vs-inferred carried up) · `Provenance` (every moment entry_id summarized).

The 13 arcs that follow correspond to the segment map in `history-overview` (segmentation entry 01KTMN9MRB31A0C8ZWSXX5M5ZQ): six new `history-seg-*` segments and seven extended `history-arc-*` continuations. Read `history-overview` first for the cross-segment topology; read an arc here for one subsystem's shape; follow an arc's `[entry_id]` links into the per-moment thread for the verifiable detail.

Provenance:
- history-overview second-window framing = 01KTMN7X7FV45E8E05DN1RV719; segment map = 01KTMN9MRB31A0C8ZWSXX5M5ZQ.
- This thread parallels the first window's `history-narrative-arc` (the #1–#37 synthesis).

<!-- Entry-ID: 01KTMNAVSB3410Y59S3FDMZCHQ -->

---
Entry: Claude Code (caleb) 2026-06-08T22:26:13.619682+00:00
Role: scribe
Type: Note
Title: Arc: history-seg-refactor-mvu — a strangler-fig march from a 12.5k-line monolith to an Elm Model-View-Update core

Spec: scribe

tags: #history #synthesis

Arc: history-seg-refactor-mvu — a strangler-fig march from a 12.5k-line monolith to an Elm Model-View-Update core
Span: 2026-05-29 → 2026-06-03 · 14 moments · 9 supersessions
Narrative:
  The segment opens with prep, not architecture. A tree-wide `if let` → let-chain rewrite bumps MSRV 1.85 → 1.88 and collapses nested guard pyramids the day before the first extraction [01KTMKSAG0R0FQ70HRQW1H4EKW]; the diff shape (mechanical guard-collapse across 21 files, +551/-565, no behavior change) reads as flattening control flow before handlers move (inferred). The structural groundwork follows: the ~12.5k-line `src/app/mod.rs` monolith decomposes into a twelve-file `src/app/` directory across two staged passes — six leaf-struct extractions, then four handler extractions — each a one-PR, behavior-equivalent move, with the recorded goal "No file in `src/app/` over ~1500 lines … Side effects modeled as data (`Effect::Spawn { ... }`) so handlers are unit-testable without a real PTY / real signal / real disk" [01KTMKTBHT9G2VBZNJZFY5EJM4]. The staging rationale is recorded verbatim: "The mechanical extractions in Phase 1 below buy ~70% of the review-ability win for ~5% of the architectural risk." That pass lands an anti-monolith guard test (`mod_rs_stays_decomposed`, CEILING=8_500) and a candid scope correction — the planned ≤4500 target was unmet at 8,427 because "the real pre-refactor file was ~12.5k."

  The design itself is then approved as a typed Decision: `docs/MVU_PLAN.md` lands as an eight-phase (−1…6) strangler-fig migration, "APPROVED — pre-2.0 / road-to-2.0 track," reversing an earlier "hold the MVU rewrite until 2.0 + ~2 weeks" gate [01KTMKVE85DEBMBWYCXY7YHP5E]. The recorded rationale is bug-class-driven, not aesthetic: "This migration is motivated by recurring, design-rooted bug classes (grounded in `BUGS.md`), not by aesthetics" — focus-model confusion, key-routing shape bugs, signal mis-routing, the `:command` punt-list footgun, state-out-of-sync, and the WriteContext self-refresh loop, each mapped to the phase that closes it. The plan records surviving "four adversarial review lenses (Rust-feasibility, sync-only, incrementalism, scope-honesty)."

  Execution proceeds phase by phase, lowest-risk first. Phase 0 collapses ~8 focus booleans and ~10 `pane_focused = false` sites into one `Focus` enum field with a single writer [01KTMKWF071QA1Y5MD5TMMRGHC]; Phase −1 re-baselines `test_state()` into a builder and relaxes the absolutist "zero test edits" invariant to "no assertion/expected-value edits" after it "was falsified against `test_state()`." Phase 1 births the single `mpsc::Receiver<Message>` with a parkable crossterm reader and a parking-aware `ForegroundExec`, landing early because "the always-on reader otherwise races vim/less for stdin" [01KTMKXA7ZKKR421NTPRJ9EQ3M]. Phase 2 re-expresses scattered `elapsed()` timers as armed `Deadline`s on a new `Scheduler`, retaining the pane-presence floor because the wake sources do not exist yet [01KTMKY262M6Y6GH25DYKBY33H]. Phase 3 (ten PRs, one folded moment) migrates every async source onto the channel with a uniform "add the wake / delete the floor" split so each migration stays revertable, ending with `MAX_IDLE_CAP` removed and "the run loop … fully event-driven (0 idle wakes when no deadline is armed)" [01KTMKZ502R8JYARTMFMKSGB72].

  The later phases tighten the triad. Phase 4 widens the anemic two-variant `PostAction` into a `#[non_exhaustive] enum Effect` with `run_effects` as "the sole side-effect executor for clipboard / signal / send-to-pane / terminal-title," reusing Phase 1's parking executor via a `From<PostAction>` shim [01KTMM03Q5F5EA3VEQSJQJZYN0]. Phase 5 folds duplicated facts into the Model — `git_info`+`git_files` → one `GitState`, harpoon off `App`, a `PaneSnapshot` replacing live-vt100 reads, `last_grid` eliminated, chdir as a synchronous `Effect::ChangeDir` [01KTMM1A4SE4HRHFQ97PRJCZYD]. Phase D makes the three-type split physical — `App = { state: AppState, runtime: Runtime, view: ViewState }` — the borrow-checker payoff the plan engineered so `render(&model, &view, &runtime)` can read concurrently with a `&mut runtime` resize [01KTMM35YXDZ69C08M78F4XKBB]. Phase 6 replaces the three-way `:command` punt-list with one `COMMAND_TABLE` [01KTMM27M59N08NB1HSWCESQTR] and Phase E reshapes the loop body (draw accumulator, `RunCtx`, `coalesce_recv`, `dispatch_effective`, `render_frame`/`run_teardown`) toward the ~100-line target [01KTMM409Y3RTZPDTVWPQWR6EW]. The last-mile pass declares MVU "landed" in ARCHITECTURE.md, makes the render path mutation-free behind a `TestBackend` snapshot net [01KTMM4ZMPP9DCW3R3NK5BDA93], then collapses the three update entry points (`ApplyResult`/`CommandResult`/`PromptResult`) into one `App::update(msg)` and relocates the command table to its own module with compile-checked handlers [01KTMM60KR8W18TWXPXDGT9J8Y].
Lineage: moment [01KTMKSAG0R0FQ70HRQW1H4EKW] -> [01KTMKTBHT9G2VBZNJZFY5EJM4] -> [01KTMKVE85DEBMBWYCXY7YHP5E] -> [01KTMKWF071QA1Y5MD5TMMRGHC] -> [01KTMKXA7ZKKR421NTPRJ9EQ3M] -> [01KTMKY262M6Y6GH25DYKBY33H] -> [01KTMKZ502R8JYARTMFMKSGB72] -> [01KTMM03Q5F5EA3VEQSJQJZYN0] -> [01KTMM1A4SE4HRHFQ97PRJCZYD] -> [01KTMM35YXDZ69C08M78F4XKBB] -> [01KTMM27M59N08NB1HSWCESQTR] -> [01KTMM409Y3RTZPDTVWPQWR6EW] -> [01KTMM4ZMPP9DCW3R3NK5BDA93] -> [01KTMM60KR8W18TWXPXDGT9J8Y]
Open / unsettled: The terminal moment records the Model/View/Update triad as structurally complete within this slice, naming no open MVU item. Two continuations are named, not as debts but as parallel tracks: module-decomposition (#248–#308, segment history-seg-module-decomposition) carves the now-MVU `src/app/` further, and the gix work proceeds on history-seg-gix-migration. The plan's recorded honest trade-off remains a standing caution: deep loop/concurrency surgery before launch "carries regression risk behavior-equivalence tests don't fully catch (timing, focus, stdin)" — accepted, not closed.
Confidence: High recorded-rationale density. Most moments quote verbatim from docs/MVU_PLAN.md, ARCHITECTURE.md, REFACTOR_PLAN.md, or squash subjects; all fourteen carry "confidence: high." Inferred reads are confined to intent framing (the let-chains-as-prep purpose, the lowest-risk-first ordering, the borrow-checker motivation) and are marked inferred at the moment layer; code-shape claims are pickaxe- or diff-verified.
Provenance: 01KTMKSAG0R0FQ70HRQW1H4EKW, 01KTMKTBHT9G2VBZNJZFY5EJM4, 01KTMKVE85DEBMBWYCXY7YHP5E, 01KTMKWF071QA1Y5MD5TMMRGHC, 01KTMKXA7ZKKR421NTPRJ9EQ3M, 01KTMKY262M6Y6GH25DYKBY33H, 01KTMKZ502R8JYARTMFMKSGB72, 01KTMM03Q5F5EA3VEQSJQJZYN0, 01KTMM1A4SE4HRHFQ97PRJCZYD, 01KTMM35YXDZ69C08M78F4XKBB, 01KTMM27M59N08NB1HSWCESQTR, 01KTMM409Y3RTZPDTVWPQWR6EW, 01KTMM4ZMPP9DCW3R3NK5BDA93, 01KTMM60KR8W18TWXPXDGT9J8Y

<!-- Entry-ID: 01KTMNFSQP1XJ1TSXS545PR01E -->
