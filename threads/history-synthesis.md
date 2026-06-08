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

---
Entry: Claude Code (caleb) 2026-06-08T22:27:05.176746+00:00
Role: scribe
Type: Note
Title: Arc: history-seg-markdown-rendering — pager-hosted rendered/source dual-view, generalized across content types and reconciled width budgets

Spec: scribe

tags: #history #synthesis

Arc: history-seg-markdown-rendering — pager-hosted rendered/source dual-view, generalized across content types and reconciled width budgets
Span: 2026-05-13 → 2026-05-24 · 10 moments · 4 supersessions
Narrative:
  This arc summarizes the whole history-seg-markdown-rendering thread; it is secondary to the per-moment log there and does not replace it. The lifecycle anchor is the `m` (rendered↔source) toggle: it stops hard-resetting scroll to line 0 and instead stashes per-side scroll in a `saved_alt_scroll` slot, with a proportional projection on first visit because "the rendered and source views have different line counts" [01KTMMG3MTKFGRKYEJGHN65QY8]. That dual-view model — rendered and source as two views of one buffer with independent positions — is the invariant the rest of the arc threads through. A separate one-shot event-ordering fix keeps loose-list bullets attached to their item text via a `just_started_item` flag, a pulldown-cmark gotcha surfaced by dogfooding BUGS.md itself [01KTMMGRQ8WXQQA9Y0AWPS5X9Z].

  The arc's longest tension is width and line-break handling, which the diff shape suggests was tuned in a tight feedback loop (inferred). Tables gain an optional `table_width_hint` so they expand to the pager body width while prose stays at the 80-column budget [01KTMMHF832JPTVJ91GGHG8AFM]. Soft breaks are then overridden to render as hard breaks to fix the `**Key:** value` metadata-stack case — the recorded rationale candidly names the cost, "small trade for the metadata case actually working" [01KTMMJ3WCPSR7PH80MBES90JW]. The table hint is corrected to subtract the line-number gutter so wide tables stop overflowing, its safety margin explicitly citing both prior moments [01KTMMJW20G78SSRS5TCFCRN1P]. Then the soft-break override is reverted: paragraphs reflow at the pager width again, and the metadata case is re-achieved narrowly via a `force_hard_breaks_before_keyed_lines` preprocessor [01KTMMKR5K08NMX743ZN7132WQ]. This reads as supersession-with-preservation — keep the win, drop the collateral prose damage.

  Two correctness/extensibility moments harden the broader rendering surface: syntect gains a user grammar directory and bare-filename detection so formats like `Makefile` resolve, with tree-sitter explicitly deferred to the roadmap [01KTMMMZT01JK92DWDBMNJASWD]; and the $EDITOR-return path is consolidated onto the shared `build_pager_view_for_file` builder so a markdown file edited with `v` comes back rendered, restoring the dual-view invariant through the round-trip [01KTMMNW2TV95JZ1QG2HVWA0J7].

  The dual-view machinery then generalizes to a second content type: a new `src/ui/json.rs` opens `.json` as canonical pretty JSON with `m` toggling pretty↔raw, deliberately mirroring markdown's `m` key, lines/alt_lines slot pattern, and fail-open philosophy, with `.jsonl` excluded to preserve its one-record-per-line affordance [01KTMMPPPBJYJVVAGK5WSP6Y9P]. A closing topology note records that across this slice the renderer is a single `src/ui/markdown.rs`; the directory decomposition into `src/ui/markdown/{mod,renderer,wrap,tests}.rs` happens later, out of slice, at #297 [01KTMMR0PQQYXE3YTA9WSK3E9D].
Lineage: moment [01KTMMG3MTKFGRKYEJGHN65QY8] -> [01KTMMHF832JPTVJ91GGHG8AFM] -> [01KTMMJ3WCPSR7PH80MBES90JW] -> [01KTMMJW20G78SSRS5TCFCRN1P] (supersedes #103) -> [01KTMMKR5K08NMX743ZN7132WQ] (supersedes #107); and [01KTMMG3MTKFGRKYEJGHN65QY8] -> [01KTMMNW2TV95JZ1QG2HVWA0J7] (restores dual-view through edit) / [01KTMMPPPBJYJVVAGK5WSP6Y9P] (extends dual-view to JSON)
Open / unsettled: tree-sitter is recorded as the intended future highlighting engine, deferred and pairing with the spyc-render-core crate split [01KTMMMZT01JK92DWDBMNJASWD]; JSON folding / path-indicator / search-within-structure / `:jq` are deferred to v1.50.73+ [01KTMMPPPBJYJVVAGK5WSP6Y9P]; the renderer's file→directory decomposition is out of slice and tracked in history-seg-module-decomposition [01KTMMR0PQQYXE3YTA9WSK3E9D].
Confidence: Uniformly high — every moment carries verbatim CHANGELOG (and ROADMAP, for #118) recorded rationale plus diff-level evidence; inferred reads are confined to intent and tuning-loop framing and are marked.
Provenance: 01KTMMG3MTKFGRKYEJGHN65QY8, 01KTMMGRQ8WXQQA9Y0AWPS5X9Z, 01KTMMHF832JPTVJ91GGHG8AFM, 01KTMMJ3WCPSR7PH80MBES90JW, 01KTMMJW20G78SSRS5TCFCRN1P, 01KTMMKR5K08NMX743ZN7132WQ, 01KTMMMZT01JK92DWDBMNJASWD, 01KTMMNW2TV95JZ1QG2HVWA0J7, 01KTMMPPPBJYJVVAGK5WSP6Y9P, 01KTMMR0PQQYXE3YTA9WSK3E9D

<!-- Entry-ID: 01KTMNHBEZQYY8WY4CKBH57P5E -->

---
Entry: Claude Code (caleb) 2026-06-08T22:27:11.750822+00:00
Role: scribe
Type: Note
Title: Arc: history-seg-docs-planning — one thesis ("the noun the agent operates on") propagated across positioning, plans, and triage

Spec: scribe

tags: #history #synthesis

Arc: history-seg-docs-planning — one thesis ("the noun the agent operates on") propagated across positioning, plans, and triage
Span: 2026-05-09 → 2026-05-30 · 7 moments · 4 supersessions
Narrative:
  This secondary reconstruction summarizes history-seg-docs-planning; every claim carries an inline moment ref. The positioning spine starts with a three-PR same-day burst (#72/#73/#74) that rewrites README, AGENTS, and the deck to one thesis recorded verbatim: "The file commander is the noun the agent operates on, not the chrome around it" [01KTMMKHEYZ540PV6VVZR33K79]. That same moment records the origin of the squash-merge convention later segments inherit (AGENTS #73, attributed to an external catalogue review). The thesis is re-quoted verbatim in the Yazi competitive review (#157), which benchmarks the nearest neighbour feature-by-feature and recommends deferring native OSC-72 drag-and-drop; its same-day follow-up #158 executes those edits — rewriting the ROADMAP DnD entry and promoting cwd-export-on-quit [01KTMMVPG9JPMQ9N5CGB84KGVG].

  The plan-doc spine shows architectural churn recorded before code. The v1.60 "CounterTop" plan is filed on a recursive-composition thesis (#76), then reversed within ~4.5h to "siblings + mirror" (#77), then hardened with a capability-negotiation compatibility matrix (#79) — the CHANGELOG records the recursion route as "considered-and-rejected after design discussion with the user" [01KTMMMQ1VY8ZERQ3NQAF89DN4]. PR #86 promotes a one-line BUGS wish into the v1.51 AUTO_APPROVAL plan and records a rejection on a security argument: "Security features should not be built on regex against another tool's UI" [01KTMMPA6HBDB91KMZPTBPNHX3]. The v1.70 "Mise en Place" plan (#114) reframes the MCP socket from v1.60's informal peer-discovery channel into a formal typed daemon protocol (stations/plates/orders/bells), one-protocol-three-clients, and sequences a crate split before the protocol work [01KTMMSRJ3DEDE2S4VWY5JHEND].

  Two pane plans (#92/#93) land from external-contributor analysis: PANE_RECOVERY_PLAN tiers recovery by program kind, PANE_STARTUP_TABS_PLAN adds config-driven startup tabs while deferring real splits — consistent with the recorded project convention that pane multiplicity means tabs, not tmux-style splits [01KTMMRKSSJ8EN3RC6RB36RSM3]. PR #179 then reorganizes the ROADMAP to "Lean 2.0" and flips REFACTOR_PLAN from "hold the whole plan until after 2.0" to "take the low-risk decomposition now, hold only the deep MVU rewrite," with the recorded trigger "the file crossed ~12k lines... Decomposition also unblocks the 2.x crate split" [01KTMMWRE9YX58H2RXKYY58QVH].

  Underneath the big plan docs runs a recurring triage discipline (#62/#71/#159/#160/#171): TODO flips with honest notes, the BUGS→ROADMAP promotion cadence, and a version-stamped FIXED ledger — the same promotion pattern that turned the auto-approval line into a plan, operating at lower altitude [01KTMMYFCAPJDTD83XH66BE7P8].
Lineage: moment [01KTMMKHEYZ540PV6VVZR33K79] -> moment [01KTMMVPG9JPMQ9N5CGB84KGVG] (positioning thesis spine); moment [01KTMMMQ1VY8ZERQ3NQAF89DN4] -> moment [01KTMMSRJ3DEDE2S4VWY5JHEND] -> moment [01KTMMWRE9YX58H2RXKYY58QVH] (v1.60→v1.70→Lean-2.0 plan spine, each superseding the prior stance); moment [01KTMMPA6HBDB91KMZPTBPNHX3] and moment [01KTMMYFCAPJDTD83XH66BE7P8] are the BUGS→plan→ROADMAP promotion cadence
Open / unsettled: implementation of the per-agent settings curation + `:approvals` pager (deferred to agent-integration, named at [01KTMMPA6HBDB91KMZPTBPNHX3]); the v1.70 crate split + decomposition Phases 1-2 and held-post-2.0 MVU rewrite are owned by history-seg-refactor-mvu (this segment records only the planning decisions, [01KTMMSRJ3DEDE2S4VWY5JHEND]/[01KTMMWRE9YX58H2RXKYY58QVH]); the "real splits" ask stays a deferred future direction [01KTMMRKSSJ8EN3RC6RB36RSM3]. Several FIXED-ledger code fixes land in engineering segments, not here [01KTMMYFCAPJDTD83XH66BE7P8].
Confidence: every moment is marked recorded (verbatim plan-doc/CHANGELOG quotes, diff-verified); all inferred-intent reads are explicitly flagged per moment at "high" confidence. Recorded-dominant ratio carried up unchanged.
Provenance: 01KTMMKHEYZ540PV6VVZR33K79, 01KTMMMQ1VY8ZERQ3NQAF89DN4, 01KTMMPA6HBDB91KMZPTBPNHX3, 01KTMMRKSSJ8EN3RC6RB36RSM3, 01KTMMSRJ3DEDE2S4VWY5JHEND, 01KTMMVPG9JPMQ9N5CGB84KGVG, 01KTMMWRE9YX58H2RXKYY58QVH, 01KTMMYFCAPJDTD83XH66BE7P8

<!-- Entry-ID: 01KTMNHGWSQK3S2QZ68D3WBCGC -->

---
Entry: Claude Code (caleb) 2026-06-08T22:27:38.863272+00:00
Role: scribe
Type: Note
Title: Arc: history-seg-gix-migration — a 9-PR strangler-fig from git subprocess to in-process gitoxide

Spec: scribe

tags: #history #synthesis

Arc: history-seg-gix-migration — a 9-PR strangler-fig from git subprocess to in-process gitoxide
Span: 2026-06-05 → 2026-06-06 · 8 moments · 6 supersessions
Narrative:
  The segment is a planned 9-step strangler-fig replacing every `git` subprocess shell-out with the pure-Rust `gix` crate, behind a single facade. It begins by building the wrapper around the legacy organism: a `src/git/` facade module is carved out as "the single boundary between spyc and any git backend," with every `Command::new("git")` call relocated verbatim from `sysinfo.rs` / `app/util.rs` / `app/git_state.rs` so "there is exactly one place that owns `Command::new(\"git\")`" — no gix yet, a pure relocation that keeps the one-way `app`-depends-on-`git` dependency rule [01KTMMHJ879C24FY2WYYT9F1SF]. The backend is then made available additively: the `gix` crate (v0.84) lands with `default-features = false` and a hand-trimmed feature set (status/diff/blame/discovery + `parallel`), with networking, credentials, and ~40 default features off, and the recorded comment pre-naming "PR 6" as where `worktree-mutation` will be enabled — explicit forward planning of the migration order [01KTMMJB955RF16D842WFFEBYP]. The C-free / static-musl motivation reads as a packaging concern (inferred): a pure-Rust git lib removes the runtime dependency on a `git` install.

  The first real swap is chosen for lowest risk: repository discovery (gitdir + branch), a read-only lookup with small stable output, lands in a new `src/git/discovery.rs` and is deleted from `sysinfo.rs`, proving the gix wiring end-to-end before the riskier paths [01KTMMKCGA2NKTRHBH3VPW37AJ] (lowest-risk-first ordering inferred from diff scope; confidence med). The hot path then follows the segment's canonical pattern, recorded as a typed Decision: a parity spike (PR #286) builds a gix `repo_status` backend alongside the subprocess one and proves byte-for-byte equivalence — "this backend runs only from the parity tests, not the live status path" — and only then (PR #287) flips status to gix by default, gated by an `SPYC_GIT_BACKEND=subprocess` escape hatch framed verbatim as "a one-release-cycle safety valve so a field regression in the gix flip is a flag flip, not a code restore. Removed in PR 9" [01KTMMMBVCGADASG74RTHKWY97]. The gix backend deliberately matches subprocess defaults (collapsed untracked dirs, 0.5 rename similarity) so markers are unchanged.

  Worktree operations migrate next, as the pre-committed PR 6: create/list/remove move to gix with the `worktree-mutation` feature enabled exactly when needed (the feature-flag discipline from the dependency add paying off), riding alongside a layout change that groups worktrees under a per-repo `<repo>.worktrees/<branch>` dir to avoid parent-dir clutter and same-name collisions [01KTMMNW4ZDF0R1C5631XRD593]. The richest domain — diff/show/blame — is split into a model→render→wire sub-arc mirroring the status parity→flip caution: first an isolated, testable gix data model (~1900 lines, "no UI flip yet") [01KTMMPRPTSP0J1MTJCT3WW6N1], then a pure in-house renderer ("pure; not yet wired"), then the live wire that routes the pager through gix diff_model + the renderer and unlocks capabilities the subprocess pager could not cheaply offer — word-level highlighting, a side-by-side `|` toggle, a themed blame gutter — because spyc now owns the structured diff in-process [01KTMMR7X2GJAMQE0HD0C0HA6R].

  The segment closes on the terminal supersession (PR 9): the last `Command::new("git")` call sites are removed, the escape hatch is deleted exactly as promised, and a `no_subprocess_git_in_production` guard test is added — the "Strangler-fig closing guard" that scans each file's production portion and fails if any production source spawns `git` [01KTMMTF0MQ96P7BVN7QQPVBNS]. The CHANGELOG records "spyc no longer runs the `git` binary at all … repo discovery, status, diff, show, blame, and worktree create/list/remove all run in-process via the pure-Rust `gix` crate," released as 1.56.0. The net diff is -178 — the migration ends by removing more than it adds, the recorded signature of a completed strangler-fig.
Lineage: moment [01KTMMHJ879C24FY2WYYT9F1SF] -> [01KTMMJB955RF16D842WFFEBYP] -> [01KTMMKCGA2NKTRHBH3VPW37AJ] -> [01KTMMMBVCGADASG74RTHKWY97] -> [01KTMMNW4ZDF0R1C5631XRD593] -> [01KTMMPRPTSP0J1MTJCT3WW6N1] -> [01KTMMR7X2GJAMQE0HD0C0HA6R] -> [01KTMMTF0MQ96P7BVN7QQPVBNS]
Open / unsettled: The closing moment records the migration complete — zero git-subprocess usages in production, enforced by an executable guard test. One named residual: the `git` binary is still used to build fixtures in spyc's own test suite (explicitly excluded from the guard by the production-portion scan), a deliberate carve-out rather than an open item. The escape hatch's planned one-release-cycle soak window "lasted exactly the planned window" and is closed.
Confidence: High recorded-rationale density. Most moments quote verbatim from src/git/mod.rs and status.rs docs, Cargo.toml comments, and CHANGELOG entries; seven carry "confidence: high." The single med-confidence read is the lowest-risk-first ordering rationale for the discovery swap, explicitly flagged at the moment layer. Code-shape and supersession claims are pickaxe-verified (e.g. removal of `porcelain_raw`, `subprocess_backend`, four `Command::new("git")` sites).
Provenance: 01KTMMHJ879C24FY2WYYT9F1SF, 01KTMMJB955RF16D842WFFEBYP, 01KTMMKCGA2NKTRHBH3VPW37AJ, 01KTMMMBVCGADASG74RTHKWY97, 01KTMMNW4ZDF0R1C5631XRD593, 01KTMMPRPTSP0J1MTJCT3WW6N1, 01KTMMR7X2GJAMQE0HD0C0HA6R, 01KTMMTF0MQ96P7BVN7QQPVBNS

<!-- Entry-ID: 01KTMNJD6GJM90D0WTTS0X28XR -->

---
Entry: Claude Code (caleb) 2026-06-08T22:27:50.784005+00:00
Role: scribe
Type: Note
Title: Arc: history-arc-07-codex-and-mcp-bridge — hardcoded peers → AgentProfile registry

Spec: scribe

tags: #history #synthesis

Arc: history-arc-07-codex-and-mcp-bridge — hardcoded peers → AgentProfile registry
Span: 2026-05-10 → 2026-06-01 · 11 moments · 3 supersessions
Narrative:
  This arc summarizes the #38–#311 continuation window only; it is secondary to the per-moment log in history-arc-07-codex-and-mcp-bridge and does not replace it. The framing moment states the throughline: the claude+codex MCP bridge generalizes into a multi-agent surface, "hardcoded peers → declarative agent registry," answering the arc-07 tail's open question about whether a per-peer registration layer would stay parallel or become parametric [01KTMMPRDQ4Y8Q0D5AD9ZND5SA]. The expansion begins by multiplying the parallel-per-peer shape: gemini arrives as the third `AgentKind`, carrying its own detection, resume, and session-discovery functions, though one shared sub-concern — the closest-by-start-time picker — is generalized over a `SessionCandidate` trait [01KTMMR37MMRBBRMQK58RFN6R2]. The active peer becomes visible as a status-bar segment whose short-id resolves per-kind at render time [01KTMMRZ2K1RSQN8HNSHZS03E1].

  A hardening sub-thread runs alongside the expansion, all on the shared substrate or its per-peer branches: a claude resume-enter race fixed with a two-phase keystroke injection [01KTMMT76DJR9MACFF973R44PK]; MCP context-freshness, where agent-initiated mutations now write the context file synchronously while human edits stay debounced, plus codex-resolver hygiene that drops a private timestamp parser for the shared `parse_iso8601_to_epoch_secs` [01KTMMWZXSEMHKYCD0GGNSNR3E]; and a 20s socket IO deadline so a wedged MCP server surfaces as a clean JSON-RPC error instead of hanging the agent [01KTMN2RJ3V39M24PWFHXJ3R7K]. The diff shape suggests the substrate stayed singular — one socket, one proxy — and kept absorbing correctness work while the peer set grew (inferred).

  The per-peer cost peaks at the transcript surface: `^a v` learns to read an agent's on-disk JSONL instead of scraping the terminal grid, driven by the codex-specific constraint that it "keeps its history in a DECSTBM scroll region ... so it can never be screen-scraped" — and the renderer is reimplemented once per agent across three near-identical files (`codex_transcript.rs`, `claude_transcript.rs`, `agy_transcript.rs`), with a shared tail-read helper fixing the same 100+ MB hang "twice, once per file" [01KTMMVJ6RR8RM6WNZ9HKQC5C8]. Antigravity (agy) onboards as the fourth peer the expensive way — detection, resume, status short-id, and a third parallel transcript file — and the chronology (agy's onboarding hours before the registry merge the same day) reads as the motivation made nearly legible as cause-and-effect [01KTMMY8R53BC7BJPEHKVWTRD0].

  The generalization lands at #176: the "~10 per-agent `match AgentKind` dispatch sites" collapse into one `AgentProfile` trait plus `REGISTRY` in a new `src/agent/` module, with `AgentKind` demoted to the on-disk persistence tag and behavior migrated to profiles — asserted behavior-preserving, "all existing agent tests pass verbatim" [01KTMMZWBVK4X3QJP7SJW3ZEG2]. The dividend is immediate and measurable: zot becomes the fifth agent 18 minutes later via "one impl + one registry line, no dispatch-site edits," with zero `src/app/mod.rs` changes against agy's tree-wide sweep hours earlier [01KTMN0ZW93D5P7TPZCVMCWC0B]. A process-stats line on the activity monitor (pid/uptime/rss/threads/panes) gives the operator visibility into the multi-agent host's thread and memory growth [01KTMN1TSVDFN7SWK34QF2SWY5].
Lineage: moment [01KTMMR37MMRBBRMQK58RFN6R2] -> [01KTMMRZ2K1RSQN8HNSHZS03E1] -> [01KTMMVJ6RR8RM6WNZ9HKQC5C8] -> [01KTMMY8R53BC7BJPEHKVWTRD0] -> [01KTMMZWBVK4X3QJP7SJW3ZEG2] (supersedes every per-peer dispatch site since #19) -> [01KTMN0ZW93D5P7TPZCVMCWC0B]
Open / unsettled: zot ships partial — transcript scrollback and `--session <path>` resume deferred until its on-disk layout is confirmed, now expressible as an unimplemented trait method rather than an absent file [01KTMN0ZW93D5P7TPZCVMCWC0B]; the claude phased-write injection timing stays claude-shaped after the registry since `AgentProfile` carries selectors, not a unified injector [01KTMMT76DJR9MACFF973R44PK]; codex status short-id (UUID in rollout filename) and per-CLI token usage remain deferred [01KTMMRZ2K1RSQN8HNSHZS03E1]; agent-status off-render-thread resolution is downstream in history-seg-refactor-mvu (#234).
Confidence: High across the window; one moment (#142 activity monitor) is med on its multi-agent motivation — the metrics' purpose is recorded generically, not as caused by agent growth — high on mechanics [01KTMN1TSVDFN7SWK34QF2SWY5]. All other moments carry verbatim CHANGELOG / module-doc / AGENTS.md rationale plus pickaxe verification (#176 trait is net-new).
Provenance: 01KTMMPRDQ4Y8Q0D5AD9ZND5SA, 01KTMMR37MMRBBRMQK58RFN6R2, 01KTMMRZ2K1RSQN8HNSHZS03E1, 01KTMMT76DJR9MACFF973R44PK, 01KTMMVJ6RR8RM6WNZ9HKQC5C8, 01KTMMWZXSEMHKYCD0GGNSNR3E, 01KTMMY8R53BC7BJPEHKVWTRD0, 01KTMMZWBVK4X3QJP7SJW3ZEG2, 01KTMN0ZW93D5P7TPZCVMCWC0B, 01KTMN1TSVDFN7SWK34QF2SWY5, 01KTMN2RJ3V39M24PWFHXJ3R7K

<!-- Entry-ID: 01KTMNJRT9FD74NGXQXHH78A3B -->

---
Entry: Claude Code (caleb) 2026-06-08T22:27:55.677888+00:00
Role: scribe
Type: Note
Title: Arc: history-arc-08-recoverability-and-deps (#38–#311 continuation) — staying correct and alive under adversarial/edge conditions

Spec: scribe

tags: #history #synthesis

Arc: history-arc-08-recoverability-and-deps (#38–#311 continuation) — staying correct and alive under adversarial/edge conditions
Span: 2026-05-08 → 2026-05-28 · 8 moments (1 framing + 7 PR moments) · 4 supersessions
Narrative:
  This secondary reconstruction covers only the #38–#311 continuation window of history-arc-08 (the "Continuation framing" Note onward); the baseline recoverability PRs are the first-window arc. The framing Note declares the continuation extends the same theme across eleven later PRs and lists its sub-throughlines [01KTMMNZGF280TRX1M7C616KGM]. Every claim below carries an inline moment ref.

  The security spine is the "shrink the unsafe surface" pair. PR #83 cuts unsafe sites 36→2 via a per-thread `with_state_root` test override, `rustix` safe wrappers, and parameter-passing instead of env mutation; it names the two residuals as `:setenv` ("user-driven, intentional") and `install_signal_handlers`, and records a mechanism-level rejection of a `signal-hook` migration that broke the `tcsetpgrp` restore path with `EINTR` [01KTMMRWF583H2AY49TF7M6WQT]. PR #154 supersedes that characterization: `:s` stops calling `unsafe { std::env::set_var }` (recorded as undefined behavior "now that spyc runs worker threads") and routes overrides through a thread-safe `crate::envset` store merged into child spawns, taking unsafe-env-mutation to zero and leaving only the signal-handler block [01KTMN1PMZRV0KVHN1H6FB48B1].

  The path-anchoring spine converges `project_home` into the single anchor across three same-week PRs (#91/#101/#102): worktree entry/exit re-anchors PROJECT_HOME, session save anchors on it (init "no longer requires a literal `.git`... defaults to the launch dir unconditionally"), and selection-paste sends paths relative to it; this supersedes the prior cwd-relative defaulting [01KTMMWD5MCGJ5DM208DS27HRK]. Alongside, PR #98 reconciles `:q`/`:quit` with `Q`'s save lifecycle via a typed `CommandResult::Quit`, superseding the handler that merely flipped `should_quit` — recorded symptom: "long-time `:q` users could quit thousands of times and still see 'no saved sessions'" [01KTMMYVX1S3K0GPJPWNN3NXQ0].

  Huge-tree resilience continues: PR #87 caps the listing watcher's recursive subdir walk at 256, "same shape as the `MAX_ENTRIES` cap" from the first-window PR #28, with rationale living in source doc-comments (the PR carried no CHANGELOG) [01KTMMTRQ21DMSTBQ38W9SYSQB]. Three edge-case fixes round out the slice: PR #55 advertises `COLORTERM=truecolor` to spawned panes, the one moment attributed to an automated (Gemini) reviewer [01KTMMPY4VXGTWWXW21RF6XYV2]; PR #97 makes Linux clipboard yank work via a new `src/clipboard.rs` fallback chain (wl-copy→xclip→xsel), deduping two inline `pbcopy` sites and closing public issue #2 [01KTMMY020Q2946BB69FEVR4QN]; PR #127 lets Enter and the `D`/`v` guards follow symlinks-to-directories via `target_is_dir` while `R`/picks intentionally still act on the symlink itself [01KTMN0NXJW02PGHZ3N0XFDFXF]. PR #124 makes the graveyard recovery surface from first-window PR #13 discoverable (`?` opens help, entry-hint flash) — additive, not corrective [01KTMMZTVFPNJ14QHF15EVA3MA].
Lineage: moment [01KTMMRWF583H2AY49TF7M6WQT] -> moment [01KTMN1PMZRV0KVHN1H6FB48B1] (unsafe-surface spine: #154 removes the `:setenv` residual #83 named); moment [01KTMMWD5MCGJ5DM208DS27HRK] is the PROJECT_HOME convergence superseding cwd-relative path ops; cross-window, [01KTMMTRQ21DMSTBQ38W9SYSQB] extends first-window PR #28's cap and [01KTMMZTVFPNJ14QHF15EVA3MA] extends first-window PR #13's graveyard viewer
Open / unsettled: one intentional unsafe site remains — `install_signal_handlers` on raw `libc`, kept after the `signal-hook` rejection [01KTMMRWF583H2AY49TF7M6WQT]/[01KTMN1PMZRV0KVHN1H6FB48B1]. The huge-tree cap accepts up-to-one-second marker staleness by design [01KTMMTRQ21DMSTBQ38W9SYSQB]. Several moments name pre-existing bug threads (bug-q-command-skips-session-save, bug-listing-watcher-recursive-hang, bug-yank-clipboard-pbcopy-linux) as their tracking context.
Confidence: predominantly recorded — most PRs carry rich CHANGELOG rationale (verbatim-quoted, diff/pickaxe-verified). Recorded-via-source-doc rather than CHANGELOG for PR #87 (marked). All inferred-intent reads flagged per moment at "high" confidence; recorded-dominant ratio carried up.
Provenance: 01KTMMNZGF280TRX1M7C616KGM, 01KTMMPY4VXGTWWXW21RF6XYV2, 01KTMMRWF583H2AY49TF7M6WQT, 01KTMMTRQ21DMSTBQ38W9SYSQB, 01KTMMWD5MCGJ5DM208DS27HRK, 01KTMMY020Q2946BB69FEVR4QN, 01KTMMYVX1S3K0GPJPWNN3NXQ0, 01KTMMZTVFPNJ14QHF15EVA3MA, 01KTMN0NXJW02PGHZ3N0XFDFXF, 01KTMN1PMZRV0KVHN1H6FB48B1

<!-- Entry-ID: 01KTMNJVN84AEGVJXXK0B769H1 -->

---
Entry: Claude Code (caleb) 2026-06-08T22:28:28.088621+00:00
Role: scribe
Type: Note
Title: Arc: history-arc-06-input-and-overlays (#38–#311 continuation) — input dispatch grows a routing layer, a vi grammar, and a trace consumer

Spec: scribe

tags: #history #synthesis

Arc: history-arc-06-input-and-overlays (#38–#311 continuation) — input dispatch grows a routing layer, a vi grammar, and a trace consumer
Span: 2026-05-07 → 2026-05-29 · 9 moments (continuation indices 8+) · 3 supersessions
Narrative:
  This arc summarizes the #38–#311 continuation appended to arc 06 — the input-dispatch / chord-resolution / vim-binding / command-line-input slice, eleven PRs written as nine moments and explicitly distinct from the first-window #1–#37 head entries [01KTMMN5RTFKWX352K9XBR4V0D]. The continuation framing names three threads carried forward: a routing layer (#82), a vi grammar growing in the resolver (#44, #112, #123, #126), and the jump-history `?` trigger (#95, #163). The window opens on a render-side gotcha rather than a dispatch change — PR #39 restores the host cursor for alt-screen pty children via `frame.set_cursor_position`, recorded as a regression-tail of the v1.41.18 alt-screen guard; keystrokes reached the nvim child correctly, only the visual cursor was missing [01KTMMP1HT4HHTN0KCRGZ3S8DK].

  The vi grammar then accretes across the window. PR #44 gives the pager vi's `^v` rectangle (block visual mode) on top of line-visual `V`, with the key grammar living inside the pager rather than the global resolver — the pager-as-self-contained-surface shape that PR #82 later special-cases in routing [01KTMMQ0GAARS6ZPT0B6044VEF]. PR #63 adds `:` command tab-completion over a canonical `SPYC_COMMANDS` list, and its load-bearing engineering choice is a CI contract: every entry "must round-trip through `dispatch_command` without falling into the 'unknown command' branch," the same decision-table-as-tested-invariant shape #82 applies to routing three days later [01KTMMR3QDMNK8EX8SMM6GJ3D8]. PR #112 swaps the copy/move/mkdir prompts from `Prompt::simple` to `Prompt::shell` to gain the vi line editor, deliberately disabling shared shell-history nav for them [01KTMMVMS5H1PBSH30EDW6049Q]. PR #123 grows the resolver's `PendingSeq` enum from the baseline's `Normal`/`G` into a real operator-pending grammar — `d` becomes an arming key (`dd`/`Ndd` delete, `ZZ` quit), `Enter` becomes the sole open/descend primary, with the recorded scope rule "The explicit count *ignores picks*"; PR #126 folds in the `S`/`gs` sort chords the same day [01KTMMXGV8ZBCEQXRSBXN5HYAH].

  The arc's dispatch keystone is a typed Decision: PR #82 collapses the five inline routing guards in `handle_key` into a pure `route::route_key(snap, key) -> KeyDestination` over a `RouteSnapshot`, recorded as closing "the routing-refactor TODO filed in v1.50.25 after five inline-routing bugs in a week" and making routing decisions unit-testable without a TUI ("future routing bugs land as a failing test row rather than a sixth inline patch") [01KTMMS40G4T6XANRGGGF11092]. The recorded rationale points forward to the MVU channel work downstream of this seam (cross-ref history-seg-refactor-mvu) and to a future `RemotePeer(socket)` destination. The jump-history `?` affordance reaches completeness in two passes: PR #95 wires `?` for the `J` prompt (spy parity), then PR #163 extends it to Normal mode mid-prompt so it survives `Esc k` recall — a supersession of #95's empty-buffer-only trigger that adds the path without removing the original [01KTMMTPHKSD76E1CG00CDF3TV].

  Two baseline seams get follow-through inside the window. PR #120 adds TX-side timestamps to `--key-trace` annotated with elapsed-since-RX, driven by a real input-lag report (Justin, inside tmux) — the first real consumer of the diagnostic PR #25 shipped ahead of any use, moving the baseline's "diagnostic ahead of a consumer" bet from speculative toward "validated" [01KTMMWGABE7B3ACCTC22ZAC89]. PR #155 then supersedes PR #25's own post-chord bounce guard: the inline `focus_chord_completed` swallow ran before the chord resolver and ate the completing `j` of a fast second `^a-j`, so it is extracted into a pure tested `is_post_chord_bounce` gated on `!resolver_pending` — "a key arriving mid-chord is a legitimate completion, not a bounce" — the same decision-as-pure-function + regression-rows shape #63 and #82 established this window [01KTMMYGF5RESXYKP5TTAPFRM7].
Lineage: moment [01KTMMS40G4T6XANRGGGF11092] (route_key, supersedes inline handle_key guards) -> [01KTMMTPHKSD76E1CG00CDF3TV] (#163 supersedes #95 empty-buffer `?`) -> [01KTMMYGF5RESXYKP5TTAPFRM7] (#155 supersedes PR #25 inline bounce guard). The vi-grammar moments [01KTMMQ0GAARS6ZPT0B6044VEF] -> [01KTMMVMS5H1PBSH30EDW6049Q] -> [01KTMMXGV8ZBCEQXRSBXN5HYAH] extend (not supersede) the resolver/pager modal idiom; [01KTMMWGABE7B3ACCTC22ZAC89] extends PR #25's --key-trace seam.
Open / unsettled: PR #44 records a still-open caveat — block-visual columns are character-based (Unicode scalars), not display-width, so wide CJK/emoji count as 1; "full display-width-aware block selection is future work." PR #39 scopes out forwarding the child's cursor *shape* (beam vs block) as "a separate piece of work." PR #82's recorded rationale names a forward seam not yet built in this slice: a `RemotePeer(socket)` destination for "v1.60 Phase 3 (input forwarding)." This arc covers only the continuation indices 8+; the #1–#37 head entries (picker overlays, baseline dispatch correctness through #25/#32) are the separate first-window arc and are not re-summarized here.
Confidence: High recorded-rationale density. Every moment quotes verbatim from CHANGELOG.md blocks the squash merges carried (these are the maintainer's recorded prose); eight of nine carry "confidence: high." Inferred reads are confined to intent framing (vi-parity motivation, the five-bug-driven extraction) and are marked inferred at the moment layer. All eleven PRs are squash merges on main with no surviving feature branches; supersessions are pickaxe-verified (e.g. `git log -S 'focus_chord_completed'` ties #155 back to PR #25's `bfc4a18`).
Provenance: 01KTMMN5RTFKWX352K9XBR4V0D, 01KTMMP1HT4HHTN0KCRGZ3S8DK, 01KTMMQ0GAARS6ZPT0B6044VEF, 01KTMMR3QDMNK8EX8SMM6GJ3D8, 01KTMMS40G4T6XANRGGGF11092, 01KTMMTPHKSD76E1CG00CDF3TV, 01KTMMVMS5H1PBSH30EDW6049Q, 01KTMMWGABE7B3ACCTC22ZAC89, 01KTMMXGV8ZBCEQXRSBXN5HYAH, 01KTMMYGF5RESXYKP5TTAPFRM7

<!-- Entry-ID: 01KTMNKX58D0V25E2WFYD8SVYP -->

---
Entry: Claude Code (caleb) 2026-06-08T22:28:36.459385+00:00
Role: scribe
Type: Note
Title: Arc: history-arc-03-pane-behavior — the bottom pane matures into a durable, tab-bearing pty container (migration, hide-not-destroy, lifecycle hardening)

Spec: scribe

tags: #history #synthesis

Arc: history-arc-03-pane-behavior — the bottom pane matures into a durable, tab-bearing pty container (migration, hide-not-destroy, lifecycle hardening)
Span: 2026-05-07 → 2026-05-29 · 11 moments · 8 supersessions
Narrative:
  This arc summarizes the #38–#311 continuation window only (the #46–#162 pane-behavior slice); it is secondary to the per-moment log in history-arc-03-pane-behavior and does not replace it. The framing moment sets the shape: the arc-03 head treated the bottom pane as a single tmux-like visual + child-process region, and the continuation is that surface fanning out — a pane becomes a container of tabs, each tab wrapping a shared pty kernel, with the rest of the slice as consequence management [01KTMMKJWX5X3WS2QD90MY153F]. The enabling move is V1.5 Phase 6: the pty kernel is extracted into a shared `PtyHost` and made movable between a pane tab and a background task in both directions without killing the child, landed host-first then promote then demote per the plan [01KTMMMHWAHASBVT11Y96RBKSA].

  Once tabs and ptys are first-class, focus and identity get disentangled. Switching tabs now pulls focus into the pane, generalizing a convention that already held for tab creation [01KTMMNC6G5B7Y73KC0ZN1BEMH]. Each pane gains its own resume session id via a claim-tracking closest-match picker keyed on `spawn_epoch_secs`, so multiple panes in one cwd no longer collapse onto a single conversation — "a pane is no longer 'the thing running in this cwd' but 'the thing that spawned at this instant'" [01KTMMPE3J1R4KEC6DXRS9Q5DF]. Paste gains a `top_overlay` arm so it stays in a `V`-opened editor instead of misrouting to the bottom pane, extending the overlay-as-pane model to the paste event path [01KTMMQPCA7E5VN78TTBZ43MQY]. The exited-tab end of life is hardened to treat an exited tab as recoverable: meta-only dismiss, and the stale `[exited N]` label scrubbed at save and restore [01KTMMRPVC83MXP3YS6VHS9YJZ].

  The keystone is the visibility model flip. `toggle_pane` stops dropping the pane container — which had cascaded through `Drop for PtyHost` to SIGKILL every child, so "daily-drivers lost their conversation every time they wanted the full screen for a few seconds" — and instead flips a `pane_hidden` flag; the toggle is later made a pure no-op when empty, and `^a ^a` jumps to the last-active tab now that tabs persist [01KTMMSVHSFYKGPSD9R8NRFCZY]. The recorded rationale is external-contributor sourced (the feature-pane-toggle-preserve-context thread). A focus-chord pair corrects its own layering: the `^a-k`/`^a-j` switch from inside a `^a-v` scrollback pager is moved from an unreachable pager-local handler to the `route_key` layer where a meta key actually fires [01KTMMVBB7ZNVH9YM9813VEBXJ].

  Two more moments separate conflated signals and harden the substrate. Active-tab style gains a REVERSED fill so "you are here" stops rendering identically to an amber-bold activity tab, and background-tab activity is detected in the event-loop pre-drain rather than only at render time [01KTMMW7N1HQ6Q2A90D1CPZ230]. The vt100 parser worker recovers from a poisoned mutex instead of crashing, a RAII `ParserWorker` joins the thread on every teardown path, and `^a v` / `Pane::resize` are made safe on a degenerate 0-row pane — all tightening the lifecycle the #46 PtyHost extraction set up [01KTMMXD1KSTYMEPXSQZN45HXB]. A cwd-handling pair aligns pane spawning with where the user is now: F9 resumes in the current listing dir, and the "pane cwd:" prompt gets its own history bucket [01KTMMY9KR4746DNJY7ZPKGG1G].
Lineage: moment [01KTMMMHWAHASBVT11Y96RBKSA] -> [01KTMMNC6G5B7Y73KC0ZN1BEMH] -> [01KTMMPE3J1R4KEC6DXRS9Q5DF] -> [01KTMMRPVC83MXP3YS6VHS9YJZ] -> [01KTMMSVHSFYKGPSD9R8NRFCZY] (the destroy-on-toggle supersession) -> [01KTMMVBB7ZNVH9YM9813VEBXJ] -> [01KTMMXD1KSTYMEPXSQZN45HXB]
Open / unsettled: pane-to-task demotion has an "empty start" buffer-recovery asymmetry (vim's ^z parity) and a promoted tab keeps the task's `dumb` TERM so alt-screen TUIs still won't work [01KTMMMHWAHASBVT11Y96RBKSA]; the broader quit→relaunch recovery story is deliberately separated from this in-session toggle and lives in docs/PANE_RECOVERY_PLAN.md (#92) / history-seg-docs-planning [01KTMMSVHSFYKGPSD9R8NRFCZY]; #162 does not migrate existing pane_history files, leaving stale directory entries until cleaned [01KTMMY9KR4746DNJY7ZPKGG1G].
Confidence: Uniformly high — every moment carries verbatim CHANGELOG / V1_5_PLAN.md rationale plus diff and pickaxe evidence (`pane_hidden` first appears in #94; `toggle_pane` traces to the PR #6 zoom commit). The continuation's four-axis grouping in the framing note is marked pure sequence-inference; each grouped moment carries its own recorded rationale [01KTMMKJWX5X3WS2QD90MY153F].
Provenance: 01KTMMKJWX5X3WS2QD90MY153F, 01KTMMMHWAHASBVT11Y96RBKSA, 01KTMMNC6G5B7Y73KC0ZN1BEMH, 01KTMMPE3J1R4KEC6DXRS9Q5DF, 01KTMMQPCA7E5VN78TTBZ43MQY, 01KTMMRPVC83MXP3YS6VHS9YJZ, 01KTMMSVHSFYKGPSD9R8NRFCZY, 01KTMMVBB7ZNVH9YM9813VEBXJ, 01KTMMW7N1HQ6Q2A90D1CPZ230, 01KTMMXD1KSTYMEPXSQZN45HXB, 01KTMMY9KR4746DNJY7ZPKGG1G

<!-- Entry-ID: 01KTMNM51MHPYVXWWK39J85T0E -->

---
Entry: Claude Code (caleb) 2026-06-08T22:28:42.265197+00:00
Role: scribe
Type: Note
Title: Arc: history-arc-01-foundation-hygiene (#38–#311 continuation) — the hygiene rails tuned, measured, and extended

Spec: scribe

tags: #history #synthesis

Arc: history-arc-01-foundation-hygiene (#38–#311 continuation) — the hygiene rails tuned, measured, and extended
Span: 2026-05-08 → 2026-06-06 · 11 moments (1 framing + 10 PR moments) · supersessions concentrated in the cache campaign + advisory clearing
Narrative:
  This secondary reconstruction covers only the #38–#311 continuation of history-arc-01 (the "Continuation framing" Note onward); the three v1.37.2 PRs that established the rails are the first-window arc. The framing Note states the continuation throughline (inferred): the rails maturing "from established-but-cold into measured-and-fast," with the test surface climbing 770→929 tests across the window [01KTMMKT8E97SAHMKW6GJHTX6E]. Every claim below carries an inline moment ref.

  The CI-caching campaign is the densest moment: seven PRs (#57/#58/#61/#64/#65/#66/#69) in ~36 hours, each with a verbatim cost-accounting commit body — recorded target "cut cold-cache CI from ~6 min toward ~1.5." It drops `Cargo.lock` from the cargo and target cache keys (each patch bump had been busting the cache), swaps cargo-deny to a sha256-verified prebuilt, adds rustup/coverage caches, and disables CI incremental; it supersedes the cache + cargo-deny-install shape from first-window PRs #2/#3. The moment also surfaces the load-bearing fact that CI is not a merge gate in this repo (a broken #64 reached `main` and was fixed forward by #65) [01KTMMPDE6S4PA834YDR24SX1H]. Release cuts bracket the window: PR #51 cuts v1.50.0 [01KTMMMPZDCN2D25KSX9G1R6Y1], and PRs #143/#170 add a `release-debug` profile and cut v1.51.4 — "the first tagged release since v1.50.0," now purely a CHANGELOG ritual [01KTMMY535PG8SJAN371WVMKPN].

  The test-surface expansion (#56/#59/#60/#199/#200) builds the behavior oracle along four axes — widget snapshots, narrow property tests, a pty/vt100 roundtrip, and App-harness routing/session-restore regressions — each closing a named plan item, all cfg(test)-additive; this is the recorded substrate behind the MVU refactor's "behavior-equivalence behind green CI" claim (cross-ref history-seg-refactor-mvu) [01KTMMQN283Z4FYP184WT4015X]. PR #295 is the direct payoff: a ratatui 0.30.1 bump + crossterm dedupe asserted no-behavior-change precisely because the #56 snapshots are unchanged [01KTMMZ7PWE0F54CZJGT317SED].

  The supply-chain rail grows a temporal axis (#88: a scheduled weekly deps-drift pipeline catching advisories off the push path) [01KTMMSWZPN5FJTTCCRZ7CA445], and then visibly earns its keep (#167: `time` 0.3.45→0.3.47 clears RUSTSEC-2026-0009 and two stale deny.toml ignores are retired — recorded as the first window-clear of the gate) [01KTMMX44WY1DPSCGSN27C77S0]. Toolchain work pins Rust to exact 1.96.0 so a floating stable plus the `-D warnings` gate can't break commits with no code change, and reconciles stale MSRV prose to 1.85 (#164/#165) — the moment flags as PURE SEQUENCE-INFERENCE that the 1.85→1.88 MSRV bump itself is owned by another segment [01KTMMVZKGKKZ0Z8VY1Q8TW3D2]. Cross-compile tooling (#117/#188) adds `make lint-linux` to catch OS-gated lints (a `cfg(target_os = "linux")` `collapsible_if`) that macOS-host clippy compiled out [01KTMMV2X1VV9QPY0YA4AZPE05]. The window closes with the comment-hygiene / "aislop" trilogy (#293/#294/#296) — a detect→tool→gate arc cleaning up drift left by the gix migration, impl-extraction, and 800-LoC decomposition [01KTMMV2X1VV9QPY0YA4AZPE05 framing; 01KTMN0F2B7PDYT29J4YG2GN99].
Lineage: moment [01KTMMMPZDCN2D25KSX9G1R6Y1] -> moment [01KTMMY535PG8SJAN371WVMKPN] (release-cut cadence: v1.50.0 -> v1.51.4); moment [01KTMMPDE6S4PA834YDR24SX1H] supersedes first-window PR #2/#3 cache+cargo-deny shape; moment [01KTMMSWZPN5FJTTCCRZ7CA445] -> moment [01KTMMX44WY1DPSCGSN27C77S0] (supply-chain rail gains a temporal axis, then clears its first advisory); moment [01KTMMQN283Z4FYP184WT4015X] -> moment [01KTMMZ7PWE0F54CZJGT317SED] (test oracle built, then leaned on for the ratatui bump)
Open / unsettled: the 1.85→1.88 MSRV bump is owned by another segment (flagged PURE SEQUENCE-INFERENCE at [01KTMMVZKGKKZ0Z8VY1Q8TW3D2]); aislop is deliberately kept out of `make check` as advisory-only [01KTMN0F2B7PDYT29J4YG2GN99]; the comment-hygiene cleanup is explicitly downstream of history-seg-gix-migration and the MVU decomposition (cross-refs named in the moments).
Confidence: recorded-dominant — most PRs are squash merges whose second parent retains the full pre-squash commit body, so rationale is verbatim-quotable; PR #88 and PR #117 are subject-only squashes with rationale taken from in-diff header comments / the diff (marked). All inferred-intent reads flagged per moment at "high"; one PURE SEQUENCE-INFERENCE explicitly flagged for the coordinator.
Provenance: 01KTMMKT8E97SAHMKW6GJHTX6E, 01KTMMMPZDCN2D25KSX9G1R6Y1, 01KTMMPDE6S4PA834YDR24SX1H, 01KTMMQN283Z4FYP184WT4015X, 01KTMMSWZPN5FJTTCCRZ7CA445, 01KTMMV2X1VV9QPY0YA4AZPE05, 01KTMMVZKGKKZ0Z8VY1Q8TW3D2, 01KTMMX44WY1DPSCGSN27C77S0, 01KTMMY535PG8SJAN371WVMKPN, 01KTMMZ7PWE0F54CZJGT317SED, 01KTMN0F2B7PDYT29J4YG2GN99

<!-- Entry-ID: 01KTMNMAPV4N1KFHSPAVYN9VP4 -->
