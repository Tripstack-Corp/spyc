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

---
Entry: Claude Code (caleb) 2026-06-08T22:26:21.223909+00:00
Role: scribe
Type: Note
Title: Arc: history-arc-05-pager-surface — pager as universal render surface (V1.5) → universal arrival surface (PagerStream #309–#311)

Spec: scribe

tags: #history #synthesis

Arc: history-arc-05-pager-surface — pager as universal render surface (V1.5) → universal arrival surface (PagerStream #309–#311)
Span: 2026-05-07 → 2026-06-08 · 11 moments · 9 supersessions
Narrative:
  This arc summarizes the #38–#311 continuation window only; it is secondary to the per-moment log in history-arc-05-pager-surface, which it does not replace. The framing moment sets the throughline: the pager stops being one overlay among many and becomes "spyc's universal read-surface / scrollback adapter," and the window's payoff is a stream-source abstraction [01KTMMP0ZVARBSD274T5V4A05P]. The window opens with a standalone routing fix — `^C` inside the pager dispatches contextually instead of leaking to the background spyc-list flash row [01KTMMPV14VAPT445FE1TS1AJ7] — which reads as the first instance of the "a key reached the wrong handler" shape that recurs through the window (inferred).

  The central architectural move is the V1.5 "pager / task-viewer unification," landed as four staged phases: a `Mount` enum (`Overlay|TopPane|LowerPane`) on `PagerView`, a new `src/ui/scrollback.rs` vt100→styled-lines adapter, the `^a-v` pane-scroll rewrite from "a flat byte buffer" into a real lower-pane-mounted pager, and `D` retargeted from spawning `$PAGER` to the in-app pager [01KTMMRS83NW2K9GASEKF5R1T1]. The recorded plan states the motivation: "The same pager that handles `! cmd` capture should handle pane history." The mount generalization immediately costs a four-PR regression wave the next morning — snapshot geometry, viewport-height miscompute for `LowerPane`, and `?`-help dropping the slot-mounted pager — each a place the generalization left an Overlay-only assumption [01KTMMTW9ADXAW9B6SZPDPD7MG]. The meta-key passthrough fixes name the recurring routing-guard shape explicitly: the commit calls it "the fourth instance of the same pattern" [01KTMMVVRDCY22WSYSRMTFGCMY].

  A long affordance-and-repair tail follows on the same surface: title-header provenance on yanks [01KTMMWQNM0KA5GBBVJWSQE8A5]; the `^a-v` scrollback pager re-homed from App-level singleton to per-tab state, encoding a pty-bound vs content-bound distinction [01KTMMXKYZ78J9AGSK60HJF9BV]; per-file scroll position persisted to disk, then a same-window repair when it "shipped but didn't work" [01KTMMYJYCF9GJ0JRAA649Q5BW]; a wave of legibility affordances — exit-status glyphs, an `[EOF — exit N]` content line, an altscreen hint, and amber→blue scrollback signal [01KTMN09307X7E73BREP8D7T3R]; and a placement-cursor sub-mode plus slot-aware editor launch [01KTMN185R410PMH15J2KN8CKD].

  The arc closes with the `PagerStream` abstraction: an object-safe trait plus shared spawn/wake/id-gate/drain core in `src/app/pager_stream.rs`, onto which transcript scrollback (#309), `:grep` (#310), and git-view diff/show/blame (#311) migrate, collapsing their hand-rolled session skeletons [01KTMN2XSH67BNFQPAMTSFD81X]. The recorded ARCHITECTURE.md line names the result: "Off-thread read/parse is the default architecture for any feature that fills a pager from disk or compute." The diff shape suggests the payoff the framing pointed toward: having made the pager the universal place content is rendered, this window unifies how content arrives at it (inferred).
Lineage: moment [01KTMMRS83NW2K9GASEKF5R1T1] -> [01KTMMTW9ADXAW9B6SZPDPD7MG] -> [01KTMMVVRDCY22WSYSRMTFGCMY] -> [01KTMMXKYZ78J9AGSK60HJF9BV] -> [01KTMMYJYCF9GJ0JRAA649Q5BW] -> [01KTMN09307X7E73BREP8D7T3R] -> [01KTMN185R410PMH15J2KN8CKD] -> [01KTMN2XSH67BNFQPAMTSFD81X]
Open / unsettled: The picker-into-pager `picker_items` field still does not exist [01KTMN2XSH67BNFQPAMTSFD81X]; the catalogue §4 "render into the pager" direction resolved via the more general PagerStream route rather than the specific picker shape. The routing-guard refactor the meta-key commits wanted "before v1.60 Phase 3" is tracked downstream in history-seg-refactor-mvu (#180/#189) [01KTMMVVRDCY22WSYSRMTFGCMY]. The pane-to-task buffer-recovery and inherited-TERM gotchas live in arc-03, not here.
Confidence: Uniformly high across the window — every moment carries recorded-rationale (CHANGELOG / V1_5_PLAN.md / ARCHITECTURE.md verbatim) backing each substantive claim; inferred reads are confined to intent framing and are marked.
Provenance: 01KTMMP0ZVARBSD274T5V4A05P, 01KTMMPV14VAPT445FE1TS1AJ7, 01KTMMRS83NW2K9GASEKF5R1T1, 01KTMMTW9ADXAW9B6SZPDPD7MG, 01KTMMVVRDCY22WSYSRMTFGCMY, 01KTMMWQNM0KA5GBBVJWSQE8A5, 01KTMMXKYZ78J9AGSK60HJF9BV, 01KTMMYJYCF9GJ0JRAA649Q5BW, 01KTMN09307X7E73BREP8D7T3R, 01KTMN185R410PMH15J2KN8CKD, 01KTMN2XSH67BNFQPAMTSFD81X

<!-- Entry-ID: 01KTMNG0CYN2NW1Y3RBR71J5FP -->

---
Entry: Claude Code (caleb) 2026-06-08T22:26:31.252815+00:00
Role: scribe
Type: Note
Title: Arc: history-seg-performance — under-load cost made proportional to actual change, then guarded against staleness

Spec: scribe

tags: #history #synthesis

Arc: history-seg-performance — under-load cost made proportional to actual change, then guarded against staleness
Span: 2026-05-19 → 2026-05-28 · 9 moments · 6 supersessions
Narrative:
  This is a secondary reconstruction over the committed moments in history-seg-performance; every claim below carries an inline moment ref. The arc opens with two git-poll cost cuts. PR #99 caches the (index mtime, HEAD mtime) pair so the 1 Hz safety poll bails before spawning a subprocess when those files have not moved — recorded as "skip 1Hz git poll subprocess when index/HEAD mtimes unchanged" [01KTMMHXYR7E0PYD7AZTHFAR8E]. PR #100 then moves `git status` to a background worker, adds huge-tree adaptive backoff (poll 1s→10s, debounce 500ms→3s, `-unormal`→`-uno`), and makes a cached-repo chdir do zero git subprocesses; it supersedes #99's single-slot caches with a multi-slot decision cache and self-invalidating raw-status cache [01KTMMK08N55944H24EY4RM81E]. The worker-thread plus generation-counter discard pattern introduced here recurs later in the arc (inferred linkage noted in the moment).

  A concentrated 2026-05-26 typing-latency push follows. PR #135 arms a 250ms typing-burst window that tightens the poll cadence to 16ms, and records the structural fix it defers verbatim: "let pane output wake the main loop directly... is the proper solution but a larger refactor" [01KTMMKR90HM2V9VZYSDFS1WCZ]. PR #138 replaces the unconditional 1 Hz `.spyc-context.json` write with a `context_dirty` flag — the recorded diagnosis is that spyc was perturbing claude's external file-watcher, not lagging in its own loop, triaged using the `A` activity monitor [01KTMMMSG4DXNDQZ66KVXXBE93]. PR #137 throttles git-worker re-spawns from `refresh_listing` (the boundary #99 deliberately left uncached) and adds a second `A`-monitor internals line [01KTMMP87YEBGTXTHDPH0BJJY0].

  The pane-parsing sub-chain attacks the chatty-pane symptom. PRs #139/#140 are interim mitigations (cap pane renders, then defer the active-pane vt100 drain inside the burst), each conceding the prior was insufficient [01KTMMQEW38T70F1GDME1FDJ8M]. PR #141 is the architectural fix the cluster had deferred since #135: each `Pane` gains a parser worker thread feeding an `Arc<Mutex<vt100::Parser>>`, so per-iteration main-thread cost is bounded by render plus input dispatch regardless of pane output; it retires #139/#140 as "vestigial under the worker-thread parser" [01KTMMSKB3JSCEJJHN4DB5K4H2]. PR #144 then caches the status-line agent short-id behind a 30s TTL — recorded as the per-frame hotspot that was "65% main-thread CPU" via a symbolicated `sample` [01KTMMTSMP1F44C1ZPY2PP8J7M].

  PR #156 closes the arc on balance: the watcher-driven `refresh_listing` debounce moves from a pure trailing-edge to a `should_fire_refresh` predicate with a `max_defer` cap, so the throttling #100/#137 introduced cannot swing into indefinite staleness under continuous fs activity; it adds the arc's first regression tests for this pipeline [01KTMMW22XTS48TGVY3DS1HPPA].
Lineage: moment [01KTMMHXYR7E0PYD7AZTHFAR8E] -> moment [01KTMMK08N55944H24EY4RM81E] -> moment [01KTMMP87YEBGTXTHDPH0BJJY0] -> moment [01KTMMW22XTS48TGVY3DS1HPPA] (poll/refresh-cost spine); and moment [01KTMMKR90HM2V9VZYSDFS1WCZ] -> moment [01KTMMQEW38T70F1GDME1FDJ8M] -> moment [01KTMMSKB3JSCEJJHN4DB5K4H2] (typing/pane-parse spine, mitigations superseded by the worker-thread parser)
Open / unsettled: huge-tree path trades away the untracked `?` marker (named at [01KTMMK08N55944H24EY4RM81E]; the `-uno` myth is later debunked in history-arc-04 #168). The throttle/staleness tension between #137 and #156 is resolved within the arc; no further deferrals remain named. The `context_dirty`, `with_screen`, and `drain_output` surfaces are noted (inferred) as later reshaped by the MVU work in history-seg-refactor-mvu.
Confidence: every moment is marked recorded (rich CHANGELOG/commit rationale, verified by pickaxe/diff); inferred-intent reads are explicitly flagged per moment and confidence is "high" throughout. The arc carries that recorded-dominant ratio up unchanged.
Provenance: 01KTMMHXYR7E0PYD7AZTHFAR8E, 01KTMMK08N55944H24EY4RM81E, 01KTMMKR90HM2V9VZYSDFS1WCZ, 01KTMMMSG4DXNDQZ66KVXXBE93, 01KTMMP87YEBGTXTHDPH0BJJY0, 01KTMMQEW38T70F1GDME1FDJ8M, 01KTMMSKB3JSCEJJHN4DB5K4H2, 01KTMMTSMP1F44C1ZPY2PP8J7M, 01KTMMW22XTS48TGVY3DS1HPPA

<!-- Entry-ID: 01KTMNG8WCGXX9SXWPXA84YY91 -->

---
Entry: Claude Code (caleb) 2026-06-08T22:26:53.845301+00:00
Role: scribe
Type: Note
Title: Arc: history-seg-module-decomposition — verbatim cuts shrink every oversized file under a codified ~800-line ceiling

Spec: scribe

tags: #history #synthesis

Arc: history-seg-module-decomposition — verbatim cuts shrink every oversized file under a codified ~800-line ceiling
Span: 2026-06-02 → 2026-06-08 · 6 moments · 5 supersessions
Narrative:
  The segment runs as one decomposition campaign in distinct waves, each a near-perfect insertion/deletion balance — the verbatim-move signature. It opens with a twelve-PR wave (one cut per PR) relocating self-contained concerns out of `src/app/mod.rs` into sibling `src/app/*.rs` files, governed by the recorded `REFACTOR_PLAN.md` Phase-1 convention "verbatim move + a `mod ...; use ...;` import — no behavior change, no API change. Each was one PR" [01KTMMGZWER9304EZM82KTEZMQ]. The diffs (#248 +541/-502, #257 +431/-429, #258 +514/-501) align with that verbatim signature. A second wave — the recorded "800-LoC campaign" — then carves the structural core REFACTOR_PLAN.md had deliberately left in mod.rs (the run event loop → `run.rs`, `App::new` → `bootstrap.rs`, process I/O → `proc.rs`, leaf helpers → `util.rs`), pushing mod.rs below the Phase-2 floor and seeding the test-extraction convention via a guard test `mod_rs_stays_decomposed` ("If you hit this: extract a module, don't bump the ceiling") [01KTMMHZCN03GA638EHKKCY50E].

  The campaign's governing rule is then codified as a typed Decision: a new `CLAUDE.md` "architectural contract" writes the standing invariant verbatim — "No `.rs` over ~800 lines without a solid reason. Oversized files make diffs impossible to reason about … A module root holding its own core *type definitions* is a legitimate 'solid reason'; a pile of helpers is not" [01KTMMJVZMX3SJCBSK8YF896YP]. This supersedes the earlier ~1500-line `src/app/`-scoped target with a ~800 ceiling that applies repo-wide, which (inferred) is why the subsequent waves reach into `ui/`, `mcp/`, `keymap/`, `state/`, and `git/`. AGENTS.md and ARCHITECTURE.md are refreshed to record the campaign by name and flip the MVU narrative from "in progress" to "done" (cross-ref history-seg-refactor-mvu).

  Under the new ceiling the per-subsystem waves convert oversized single files into directory modules (`foo.rs` → `foo/{mod, …}`). The render subsystems go first: markdown, diff-render, render, pager-handler, and the campaign's single biggest file `ui/pager.rs` (~2954 lines → six pager/*.rs), with snapshot `.snap` fixtures `git mv`'d into new `snapshots/` subdirs so the insta/TestBackend net stays intact [01KTMMM1322W32NGAH5H7MWYEP]. The non-render core follows — the 2190-line `src/mcp.rs` and 1878-line `keymap/resolver.rs` are the headline cuts, split into protocol/server/config/readers and thematically-partitioned test trees, with key-dispatch, sessions, and pane following the same pattern; the sessions/resolver/pane cuts are predominantly test-extraction, confirming the secondary convention that large `#[cfg(test)]` modules move to dedicated `tests.rs`/`tests/` trees [01KTMMN1CSG84TPZ4ZEFN9K62B].

  The campaign closes on its largest single target, the 3907-line `src/app/state.rs` (the MVU Model). It takes two PRs: a non-verbatim prep step that groups loose git-cache and pane fields into sub-structs so the split has clean seams — the only API-touching, non-mechanical edit in the slice (inferred from its 15-file +225/-229 ripple) — then the large verbatim directory conversion into `state/{mod,apply,dispatch,git,listing,navigation,selection}.rs` plus a partitioned tests tree [01KTMMPARGNTSQB2Z67G6KBKQ0]. That close also carries the campaign's last convention edit: the `no_subprocess_git_in_production` scan gains a test-file skip recognizing "`tests.rs`, `*_tests.rs`, or any file under a `tests/` directory," making explicit the test-extraction convention every prior decompose PR had followed implicitly. The slice ends with no `src/app/` file over the guard ceiling and the per-subsystem layout matching the present `src/` tree.
Lineage: moment [01KTMMGZWER9304EZM82KTEZMQ] -> [01KTMMHZCN03GA638EHKKCY50E] -> [01KTMMJVZMX3SJCBSK8YF896YP] -> [01KTMMM1322W32NGAH5H7MWYEP] -> [01KTMMN1CSG84TPZ4ZEFN9K62B] -> [01KTMMPARGNTSQB2Z67G6KBKQ0]
Open / unsettled: The closing moment records the slice as complete — no `src/app/` file over the guard ceiling, the layout matching the present tree — and names no deferred item. The ~800-line ceiling and the test-file naming convention persist as standing, executable invariants (the guard test fails CI on regression) rather than open work.
Confidence: High recorded-rationale density. Each moment quotes verbatim from CLAUDE.md, AGENTS.md, REFACTOR_PLAN.md, git/mod.rs, or pre-squash subjects; all six carry "confidence: high." Inferred reads are confined to intent (the ceiling driving the repo-wide reach, the prep-then-split shape as an answer to a too-coupled file) and stay marked inferred at the moment layer; diff balances are stat-verified.
Provenance: 01KTMMGZWER9304EZM82KTEZMQ, 01KTMMHZCN03GA638EHKKCY50E, 01KTMMJVZMX3SJCBSK8YF896YP, 01KTMMM1322W32NGAH5H7MWYEP, 01KTMMN1CSG84TPZ4ZEFN9K62B, 01KTMMPARGNTSQB2Z67G6KBKQ0

<!-- Entry-ID: 01KTMNH17RVXAWHF9SJ2Z7MCP9 -->
