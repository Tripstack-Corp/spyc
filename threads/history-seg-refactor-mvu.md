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

---
Entry: Claude Code (caleb) 2026-06-08T21:57:38.295108+00:00
Role: scribe
Type: Decision
Title: PR #196 — MVU_PLAN.md: the strangler-fig Elm-architecture migration is APPROVED, pre-2.0

Spec: scribe

tags: #history #refactor-mvu

Moment: refactor-mvu — Reconstructed: `docs/MVU_PLAN.md` lands as the approved, adversarially-vetted design for the Model-View-Update rewrite — an 8-phase (−1…6) strangler-fig migration that grows MVU machinery alongside the existing busy-poll loop, reversing an earlier "hold until 2.0 + ~2 weeks" gate.   [kind: convention]
When: 2026-05-30 · PR #196 (docs/mvu-plan) · commit 5b3ba59 (added docs/MVU_PLAN.md)
Recorded rationale: "Status (2026-05-30): APPROVED — pre-2.0 / road-to-2.0 track. This is the detailed design for `REFACTOR_PLAN.md`'s Phase 3 (the Model-View-Update rewrite). It is a strangler-fig migration: the MVU machinery grows alongside the existing `App::run` busy-poll loop and never replaces it in one step." — docs/MVU_PLAN.md (added PR #196). And the sequencing reversal: "Sequencing decision (2026-05-30): this lands pre-2.0, reversing the earlier 'hold the MVU rewrite until 2.0 + ~2 weeks' gate. Rationale: 2.0 should ship on the cleaner foundation, not carry a big-bang refactor as launch overhang. This is only safe because of the strangler-fig design — every phase is behavior-equivalent behind green CI (all 786 tests, no assertion edits)." — docs/MVU_PLAN.md.
Inferred intent: the doc IS the recorded rationale; it is motivated by concrete bug classes, not aesthetics — "This migration is motivated by recurring, design-rooted bug classes (grounded in `BUGS.md`), not by aesthetics." The doc's bug-class table names Focus-model confusion (~8 scattered booleans), key-routing shape bugs (route.rs's five-in-one-week history), `^C`/signal mis-routing, the two/three-site `:command` punt-list footgun (bitten on `:undo`, `:limit`), state-out-of-sync (git_files vs git_info), PostAction anemia, forgot-to-clear `pending_X`, and the WriteContext self-refresh loop — each mapped to the phase that closes it, with honest "partial scope" notes.   confidence: high
Supersedes: REFACTOR_PLAN.md Phase 3 (the "block-out-a-week big-bang" framing) — "The strangler-fig design makes every phase behavior-equivalent behind green CI and independently revertable, so it lands incrementally pre-2.0… Follow `docs/MVU_PLAN.md`." (REFACTOR_PLAN.md, edited PR #196 era)

Reconstructed: the target pattern is the canonical ratatui Elm architecture — "a `Model`, a `Message` enum, `update(model, msg) → effects`, `view(model)`, one message channel, and side-effects represented as data and run by the runtime. spyc already has the bones of this (`AppState::apply → ApplyResult` is a partial Update; `render.rs` is the View; `PostAction` is effect-as-data; `route.rs` is a pure router). The work is to finish and unify them." (docs/MVU_PLAN.md, Target pattern.)

The migrating-from snapshot is named precisely: `App::run` (~920 lines) is a busy-poll loop that non-blocking-drains 7+ independent sources, computes an adaptive `poll_ms` (16/100/500ms), and blocks only on `event::poll`. "There is no single message channel." The target loop is ~100 lines blocking on one `mpsc::Receiver<Message>`. State splits into three types — Model (pure domain, `src/app/model.rs`), Runtime (OS handles/threads/channels), ViewState (render ephemerals) — with `App = { model, runtime, view }`. The borrow-checker contract is explicit: `update()` takes `&mut Model` and returns `Vec<Effect>` (not `(Model, Vec<Effect>)`), so owned-data effects outlive the borrow.

The phase table (the spine of this thread): −1 re-baseline test fixtures · 0 Focus-as-one-value (lands first, daily-driver fix) · 1 single Input channel + parkable reader · 2 timer/deadline layer + pane floor · 3 migrate each source onto the channel (3a–3d) · 4 widen PostAction into the Effect vocabulary · 5 physically split Model/Runtime/ViewState + de-IO audit · 6 pure-of-IO View + command table, loop reaches ~100 lines. The plan records it "survived four adversarial review lenses (Rust-feasibility, sync-only, incrementalism, scope-honesty)" and states the honest trade-off: deep loop/concurrency surgery before a public launch carries regression risk behavior-equivalence tests don't fully catch (timing, focus, stdin) — accepted to avoid a post-launch refactor overhang.

This moment frames every subsequent entry in this thread; sibling segments `history-seg-module-decomposition` (#248–#308) and `history-seg-gix-migration` proceed in parallel.

Provenance:
- 5b3ba59 (PR #196 docs/mvu-plan, 2026-05-30) — added docs/MVU_PLAN.md (the full design + bug-class table + phase table); quoted verbatim above.
- docs/MVU_PLAN.md — Status/Sequencing, Target pattern, Current state, three-type split, Phases table, Risks.
- 662e744 — earlier commit "docs: add MVU migration plan (REFACTOR_PLAN Phase 3 detailed design)" confirming the doc's pre-squash provenance.

<!-- Entry-ID: 01KTMKVE85DEBMBWYCXY7YHP5E -->

---
Entry: Claude Code (caleb) 2026-06-08T21:58:11.596857+00:00
Role: scribe
Type: Note
Title: PR #197–#198 — Phase 0/−1: Focus as one Model value + test-fixture re-baseline

Spec: scribe

tags: #history #refactor-mvu

Moment: refactor-mvu — Reconstructed: the MVU work bootstraps with the lowest-risk phase first — a single `Focus` enum field replaces ~8 scattered booleans / ~10 copy-pasted `pane_focused = false` sites — paired with a fixture re-baseline so future additive field growth touches one line.   [kind: new-capability]
When: 2026-05-30 · PR #197 (refactor/mvu-phase-0-focus) commit 0d78b74; PR #198 (refactor/mvu-phase-1-fixtures) commit 7052f56
Recorded rationale: "Phase 0 — Focus as one Model value (lands first). Highest bug-leverage, lowest risk, zero loop change… Add `Focus` + `focus` field; replace the ~10 copy-pasted `pane_focused = false` sites with one transition point; reimplement `route_snapshot()` to project the focus-axis inputs from `model.focus`… Make `pane_focused` a thin accessor so the compiler flags every writer." — docs/MVU_PLAN.md (Phase 0). Fixtures: "`test_state()` and its derivatives become a single base + struct-update builder (`AppState { focus: …, ..base() }`)… so future field additions touch one fixture line. No assertion changes." — docs/MVU_PLAN.md (Phase −1)
Inferred intent: Phase 0 is shipped ahead of the loop work as a standalone daily-driver bug fix (the focus-axis paste-to-wrong-surface / V-editor `^a-c` class). The diff confirms the design: `src/app/state.rs` gains `pub enum Focus { FileList, Pane(SinkId-less variant), Overlay, Pager(crate::ui::pager::Mount) }` and `pub focus: Focus`, with `pane_focused()` reduced to `matches!(self.focus, Focus::Pane)`; `src/app/route.rs` (+18) reprojects the focus-axis inputs; render DIM cue in `src/app/render.rs` derives from focus.   confidence: high
Supersedes: the ~10 inline `pane_focused = false` mutation sites and ~8 focus booleans named in MVU_PLAN.md's bug-class table — collapsed to one writer path. (verified: Focus enum first appears in this PR's `src/app/state.rs` diff)

Reconstructed: this realizes the first row of the MVU bug-class table — "Phase 0: one `Focus` enum field with a single writer path; route.rs's focus-axis inputs, the render DIM cue, and `^C` signal-delivery all derive from it, so on the focus axis the dimmed half, input target, and paste target can't disagree." The done-criterion was `grep 'self.state.pane_focused\s*='` showing zero matches outside the single transition fn. Touches span actions.rs, commands.rs, key_dispatch.rs, pager_handler.rs, render.rs, route.rs, session.rs, state.rs (mod.rs +66/−... net).

Phase −1 (#198, +175 in mod.rs, state.rs reshaped) re-baselined `test_state()` into a base + struct-update builder mirroring route.rs's `..idle()` pattern — the absolutist "zero test edits" invariant was relaxed to "no assertion/expected-value edits; mechanical fixture/constructor churn for added or relocated fields is permitted" after it "was falsified against `test_state()` — an exhaustive `AppState` literal with no `Default` tail" (MVU_PLAN.md, Phases preamble). This unblocks every later phase that adds Model fields.

Provenance:
- 0d78b74 (PR #197 refactor/mvu-phase-0-focus, 2026-05-30) — adds `enum Focus` + `focus` field in `src/app/state.rs` (+58); reprojects `route.rs` (+18); 9 files, +138/−43.
- 7052f56 (PR #198 refactor/mvu-phase-1-fixtures, 2026-05-30) — `src/app/mod.rs` +175 fixture builder, `state.rs` reshaped; no assertion edits.
- docs/MVU_PLAN.md — Phase 0 and Phase −1 sections + bug-class table row 1 (quoted).
- prior entry 01KTMKVE85DEBMBWYCXY7YHP5E (this thread, MVU_PLAN moment) — the plan these PRs execute.

<!-- Entry-ID: 01KTMKWF071QA1Y5MD5TMMRGHC -->

---
Entry: Claude Code (caleb) 2026-06-08T21:58:39.299235+00:00
Role: scribe
Type: Note
Title: PR #201 — Phase 1: the single Message channel + parkable input reader + ForegroundExec

Spec: scribe

tags: #history #refactor-mvu

Moment: refactor-mvu — Reconstructed: the unified `mpsc::Receiver<Message>` is born — a parkable crossterm reader thread feeds `Message::Input(Event)` and the loop switches from `event::poll` to `recv_timeout`, with the inline foreground-exec path rerouted through a parking-aware `ForegroundExec` so reader and child don't both read the tty.   [kind: new-capability]
When: 2026-05-31 · PR #201 (refactor/mvu-phase-1-input-channel) · commit fe7f1cb (src/app/mod.rs +705/−... ; docs/MVU_PLAN.md +23)
Recorded rationale: "Phase 1 — Single channel for Input + parkable reader. Introduce the one `mpsc::Receiver<Message>`; move crossterm input onto it via a parkable reader thread (blocking `event::read`, Press-filtered for Key only), and reroute the still-inline `run_child_in_foreground` through a parking-aware minimal `ForegroundExec` executor (lands early because the always-on reader otherwise races vim/less for stdin)." — docs/MVU_PLAN.md (Phase 1)
Inferred intent: the stdin-contention hazard forces the parking machinery to land now, not later. The resolved mechanism is recorded as NOT a self-pipe: "crossterm 0.28 has no public mid-read interrupt… So the reader loops on `event::poll(10ms)` (finite — uses `try_lock`, so a parked reader holds no lock) and checks a park flag between polls… Park lands within ~one poll interval. (Done — `spawn_input_reader`/`ForegroundExec` in `src/app/mod.rs`.)" The diff confirms: `enum Message { Input(Event) }` (only one variant for now, "grown variant-by-variant as each source migrates"), `let reader_handle = spawn_input_reader(msg_tx)`, a `ForegroundExec { park: reader_handle.park.clone() }`, and `msg_rx.recv_timeout(Duration::from_millis(poll_ms))` replacing the old `event::poll`.   confidence: high
Supersedes: the inline `run_child_in_foreground` call path (rerouted through the parking executor) and the direct `event::poll`/`event::read` in `App::run` (now fed by the reader thread). (verified: `recv_timeout`/`spawn_input_reader` first appear in this PR's mod.rs diff)

Reconstructed: the loop's adaptive cadence is preserved — `recv_timeout(poll_ms)` keeps the same 16/100/500 timing as the old poll, so behavior is unchanged while the plumbing inverts. The hard done-criterion was "no keystroke leakage to/from a foreground `$EDITOR`/`$PAGER` (round-trip smoke + park-gate test); reader forwards Paste/Resize, drops only non-Press Key events," and the phase is recorded as "revertable by deleting the reader + park gate + FG reroute together." This is the keystone of the migration: every later source (3a–3d) plugs new `Message` variants into the receiver this PR creates.

Provenance:
- fe7f1cb (PR #201 refactor/mvu-phase-1-input-channel, 2026-05-31) — `src/app/mod.rs` +705/−198: adds `enum Message`, `spawn_input_reader`, `ForegroundExec` with shared park flag, `recv_timeout` loop; docs/MVU_PLAN.md +23 (records the resolved park mechanism).
- docs/MVU_PLAN.md — Phase 1 + Effect class (B) ForegroundExec + Risks "Stdin contention" (quoted).
- prior entry 01KTMKVE85DEBMBWYCXY7YHP5E (this thread) — the plan this executes.

<!-- Entry-ID: 01KTMKXA7ZKKR421NTPRJ9EQ3M -->

---
Entry: Claude Code (caleb) 2026-06-08T21:59:03.604693+00:00
Role: scribe
Type: Note
Title: PR #202 — Phase 2: timer/deadline scheduler with a pane-presence floor

Spec: scribe

tags: #history #refactor-mvu

Moment: refactor-mvu — Reconstructed: the `elapsed()`-vs-poll-cadence timers become `Message::Tick(Deadline)`s armed against a new `Scheduler`; the loop blocks on `recv_timeout(min(next_deadline − now, pane_idle_floor))`, with `now` threaded so timing is a pure function of inputs.   [kind: refactor]
When: 2026-05-31 · PR #202 (refactor/mvu-phase-2-deadlines) · commit d649de7 (src/app/scheduler.rs +103 new; src/app/mod.rs +205/−32)
Recorded rationale: "Phase 2 — Timer/deadline layer with pane floor. Replace `elapsed()`-vs-poll-cadence timers with `Message::Tick(Deadline)`; loop blocks on `recv_timeout(min(next_deadline - now, pane_idle_floor))`. Keep the pane floor (the 16/100/500 cadence as a floor when a pane/overlay/capture is present) — deleting the idle-pane poll before Phase 3b would regress streaming visibility. Thread `now` so timing logic is a pure fn of inputs." — docs/MVU_PLAN.md (Phase 2)
Inferred intent: the pane floor is deliberately retained because the wake sources (3b) don't exist yet — removing the idle-pane poll now would break streaming visibility. The diff confirms a new `src/app/scheduler.rs` with `pub enum Deadline { GitPoll, ActivityRollover, … }`, `pub struct Scheduler` exposing `arm/disarm/next`, and `arm_resume_deadlines(scheduler, tabs)`. The plan enumerates the deadlines: GitPoll/ActivityRollover/RefreshQuiet/ContextWrite/RestoreSettle/ResumeEnter (ScrollThrottle excluded — "it's an in-arm event-gap dedup, not a wakeup timer").   confidence: high
Supersedes: the scattered `elapsed()` timer checks in `App::run` (1Hz git poll, 1s activity rollover, 150ms context-write debounce, 300ms resume-enter) — re-expressed as armed `Deadline`s on the `Scheduler`. (verified: `enum Deadline`/`struct Scheduler` first appear in this PR's scheduler.rs)

Reconstructed: the done-criteria were a behavior-preservation bar — "idle agent pane streams ≤100ms unchanged; idle CPU at 0 draws/sec preserved; a timing-equivalence harness asserts a Message arriving 5ms into a 150ms debounce does not prematurely fire it." Threading `now` (rather than calling `Instant::now()` inside transitions) is what makes the timing logic testable and is the seam Phase 5's pure `update(&mut Model, …, now)` later relies on.

Provenance:
- d649de7 (PR #202 refactor/mvu-phase-2-deadlines, 2026-05-31) — adds `src/app/scheduler.rs` (Deadline enum + Scheduler arm/disarm/next + arm_resume_deadlines); `src/app/mod.rs` +205/−32 switches the loop to deadline-bounded recv_timeout.
- docs/MVU_PLAN.md — Phase 2 + the `Tick(Deadline)` Message variant note (quoted).
- prior entry 01KTMKXA7ZKKR421NTPRJ9EQ3M (this thread, Phase 1 channel) — supplies the receiver this phase computes timeouts against.

<!-- Entry-ID: 01KTMKY262M6Y6GH25DYKBY33H -->
