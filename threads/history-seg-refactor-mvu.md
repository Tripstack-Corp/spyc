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

---
Entry: Claude Code (caleb) 2026-06-08T21:59:39.509379+00:00
Role: scribe
Type: Note
Title: PR #203–#212 — Phase 3: every async source migrated onto the channel as wakeups (add-wake / delete-floor)

Spec: scribe

tags: #history #refactor-mvu

Moment: refactor-mvu — Reconstructed: across sub-phases 3a–3d, each independent async source (fs watcher, git worker, per-pane PTY, captures/tasks, grep, finder, MCP) is converted to push a `Message` wakeup into the unified channel; the adaptive poll floor and `MAX_IDLE_CAP` are then deleted source-by-source, leaving a fully event-driven loop with 0 idle wakes.   [kind: refactor]
When: 2026-05-31 · PR #203–#212 (refactor/mvu-phase-3a…3d-*) · commits 9dc2773 (3a) … a219d1b (3d remove-cap)
Recorded rationale: "Phase 3 DONE: every event source wakes the unified channel; the run loop is fully event-driven (0 idle wakes when no deadline is armed)… each sub-phase split add-the-wake / delete-the-floor so the floor backstopped every wake migration before its poll was removed." — docs/MVU_PLAN.md (Phase 3). And the lost-wakeup safety: "keep the monotonic `AtomicU64 parser_gen`… The worker bumps it and, only on a `wake_pending` 0→1 CAS, sends one `PaneOutput{tab}`. The main loop, on `PaneOutput{tab}`, clears `wake_pending` first, then calls `drain_output()`… at worst one redundant wakeup, never a lost tail/final echo." — docs/MVU_PLAN.md (Message)
Inferred intent: the recurring "add wake (both paths live) → delete floor" split makes every migration independently revertable with the poll as a backstop. Verified across the slice: #205 and #208 are both literally "delete the … poll floor" PRs that supersede the wake-add PRs immediately preceding them; #212 removes `MAX_IDLE_CAP` entirely. Pickaxe `git log -S 'MAX_IDLE_CAP'` shows it introduced earlier and removed at "MVU Phase 3d PR4 — remove MAX_IDLE_CAP; loop is fully event-driven."   confidence: high
Supersedes: the busy-poll drain model (adaptive `poll_ms`, the 16ms typing-burst hack, the 100ms idle-pane floor, `MAX_IDLE_CAP`) — all named in MVU_PLAN.md's "Current state" and removed by this phase. The loop ends blocking on `recv()`, kicked on reader death by a new `ReaderExited` wake. (verified: `MAX_IDLE_CAP` deleted, `ReaderExited` added in #212 diff)

Reconstructed — the per-source migration, folded:
+ #203 (3a) fs watcher → `FsEvent`, git worker → `GitResult` (generation-drop kept); adds `src/app/sources.rs` (+354).
+ #204 (3b PR1) pane PTY: introduces the `SinkId`-keyed wake slot; parser-worker emits coalesced `PaneOutput{tab}` via the lost-wakeup-safe clear-then-read dirty bit (both paths live; `src/pane/mod.rs` +216).
+ #205 (3b PR2) deletes the pane component of the poll floor — supersedes #204's "both paths live."
+ #206 (3c PR1) `PtyHost` runtime wake slot (plumbing, no behavior change — the demote/promote/`:fg`/`^Z` swap needs a slot, not a spawn-time closure).
+ #207 (3c PR2) captures + tasks wake the channel (floor still present).
+ #208 (3c PR3) deletes the last (streaming) poll floor — supersedes #207.
+ #209 (3d PR1) grep worker wakes the channel (cap still present).
+ #210 (3d PR2) F-finder walker wakes the channel.
+ #211 (3d PR3) MCP requests wake the channel via a git-style forwarder (reply + synchronous `write_context` stay adjacent for single-connection read-after-write).
+ #212 (3d PR4) removes `MAX_IDLE_CAP`; loop blocks on `recv()`; `ReaderExited` death-wake + loop-top check replaces the cap's reader-death-latency role; a 1Hz `CaptureTick` ticks the streaming elapsed-timer the cap used to drive.

The plan notes the design subtlety that data stays on the per-source channels (finder/grep send payloadless `FindOutput`/`GrepOutput` wakes via a `WakingSender`); the literal `{matches}` payload is deferred to Phase 5's ReassignSink. Ten PRs, one folded moment — the throughline is uniform (each source → wake), and the only decisions are the per-source add/delete split already captured.

Provenance:
- 9dc2773 (PR #203, 2026-05-31) — `src/app/sources.rs` +354; fs/git onto the channel.
- 2e696e5/239ed44 (PR #204/#205) — pane wake + pane-floor deletion; `src/pane/mod.rs` +216.
- 89a6941/bfa8e24/d404577 (PR #206–#208) — wake slot, capture/task wake, streaming-floor deletion.
- 57abd94/94beed8/ed0d0a8 (PR #209–#211) — grep/finder/MCP wakes.
- a219d1b (PR #212, 2026-05-31) — removes `MAX_IDLE_CAP`, adds `ReaderExited`/`CaptureTick`; loop blocks on `recv()`.
- docs/MVU_PLAN.md — Phase 3 (3a–3d) + Message (lost-wakeup coalescing) sections (quoted).
- prior entry 01KTMKY262M6Y6GH25DYKBY33H (this thread, Phase 2) — supplied the pane floor this phase finally deletes.

<!-- Entry-ID: 01KTMKZ502R8JYARTMFMKSGB72 -->

---
Entry: Claude Code (caleb) 2026-06-08T22:00:12.054053+00:00
Role: scribe
Type: Note
Title: PR #213–#217 — Phase 4: widen PostAction into the Effect vocabulary; run_effects becomes the sole executor

Spec: scribe

tags: #history #refactor-mvu

Moment: refactor-mvu — Reconstructed: the anemic two-variant `PostAction` is widened into a `#[non_exhaustive] enum Effect` in a new `src/app/effect.rs`, and `run_effects` becomes the single side-effect executor for clipboard, signals, send-to-pane, and terminal-title — moving five inline `clipboard::copy` sites, `kill_pg`/SIGSTOP/SIGCONT, and `update_term_title` behind effect data.   [kind: refactor]
When: 2026-05-31..2026-06-01 · PR #213 (effect-scaffold) commit 3e978f4 · #214 clipboard · #215 signal · #216 send-title · #217 docs/mvu-phase4-done commit 63f2149
Recorded rationale: "Phase 4 DONE (PRs #213–#216, each behavior-equivalent behind green CI + a per-PR adversarial verification workflow): `run_effects` (in the new `src/app/effect.rs`) is the sole side-effect executor for clipboard / signal / send-to-pane / terminal-title. Vocabulary: `ForegroundExec` (via a `From<PostAction> for Vec<Effect>` shim), `CopyToClipboard` + `ClipMsg`, `#[cfg(unix)] SignalGroup` + `SigOk`, `SendToPane` + `PaneTarget` + `PaneInput`, `SetTerminalTitle`." — docs/MVU_PLAN.md (Phase 4 DONE, added/edited PR #217)
Inferred intent: the migration is kept surgical — "Only `ApplyResult::Post` was widened to `Post(Vec<Effect>)` — the other two result enums and the `update`-signature collapse stay in Phase 5." The scope is scoped honestly: "`^C`: only signal-delivery sites become `SignalGroup`; prompt-cancel/buffer-clear/flash-hint stay pure transitions." And the corrected Done metrics expose an earlier vacuous grep: "the original cross-file grep was vacuous — those handler modules were already at 0 because the IO lived in `mod.rs`. `clipboard::copy` now appears exactly 5× repo-wide (1 in `run_effects` + 3 `pager.rs` footer yanks + 1 `yank_quick_select`…)."   confidence: high
Supersedes: the `PostAction` enum (only `None`+`Spawn` modeled) named in MVU_PLAN.md's "PostAction / inline side-effect anemia" bug class, and the 5 inline `clipboard::copy` IO sites — generalized into the `Effect` vocabulary with `run_effects` as sole executor. The `ForegroundExec` lands via a `From<PostAction>` shim so call sites stay byte-identical. (verified: `effect.rs` introduced in #213 diff, +439/−291 across 7 files)

Reconstructed — the four-PR slice, folded:
+ #213 Effect scaffold + `ForegroundExec` via the `From` shim (reuses Phase 1's parking executor); `src/app/effect.rs` new, `pager_handler.rs`/`state.rs` rewired.
+ #214 `CopyToClipboard` effect for the 4 status-bar yanks.
+ #215 `SignalGroup` effect for pause/resume (`kill_pg` STOP/CONT).
+ #216 `SendToPane` + `SetTerminalTitle` (final Phase 4 slice).
The plan records what was deliberately left inline as loop-intrinsic — "the producer is the loop, so threading through a non-existent handler return buys nothing": the paste bracketed-write, the resume-injection two-phase write (needs a `Tab`/`SinkId` target — Phase 5), the context-write debounce, `interrupt_task`'s SIGINT (flashes the pager footer not the status bar), and `write_context` (protects the MCP read-after-write contract). The typed `ForegroundDone`/`PostWork` message was explicitly NOT added — the spawn after-work runs inline in `run_effects`'s `ForegroundExec` arm (deferred to Phase 5). #217 is the doc milestone recording all of this.

Provenance:
- 3e978f4 (PR #213, 2026-05-31) — adds `src/app/effect.rs`; +439/−291 over 7 files; `From<PostAction> for Vec<Effect>` shim.
- d03fb79/82f18a6/04b9eaf (PR #214/#215/#216, 2026-05-31..06-01) — clipboard, signal, send-to-pane+title effects.
- 63f2149 (PR #217 docs/mvu-phase4-done, 2026-06-01) — docs/MVU_PLAN.md +26/−5 recording Phase 4 DONE + corrected metrics (quoted).
- docs/MVU_PLAN.md — Phase 4 + Effect class (A)/(B) sections.
- prior entry 01KTMKXA7ZKKR421NTPRJ9EQ3M (this thread, Phase 1) — supplied the parking `ForegroundExec` reused here.

<!-- Entry-ID: 01KTMM03Q5F5EA3VEQSJQJZYN0 -->

---
Entry: Claude Code (caleb) 2026-06-08T22:00:50.151500+00:00
Role: scribe
Type: Note
Title: PR #218–#232 — Phase 5: state migrated into the Model (GitState, harpoon, PaneSnapshot), carrier unification, exits-as-messages

Spec: scribe

tags: #history #refactor-mvu

Moment: refactor-mvu — Reconstructed: scattered/duplicated facts and live-host reads are folded into the Model — `git_info`+`git_files` → one `GitState`, harpoon moved off `App`, a `PaneSnapshot` replaces live-vt100 reads for yank/`gf`, `last_grid` eliminated, chdir becomes a synchronous `Effect::ChangeDir` — and the `CommandResult`/`PromptResult` carriers gain `Post(Vec<Effect>)` while capture/task exits become channel messages.   [kind: refactor]
When: 2026-06-01..2026-06-02 · PR #218–#226 (mvu-phase5-pr0…pr7), #230 (pr9 carrier-unification), #232 (pr8 exits-as-messages) · commits 3c73580 … 18a56ef
Recorded rationale: "Phase 5 — Physically split Model/Runtime/ViewState + de-IO audit. Introduce `Runtime` and `Model`/`ViewState`; move `git_worker_tx` into Runtime (channel reunited), `harpoon` into Model, eliminate `last_grid`… chdir fork (explicit, not mechanical)… (a) model chdir as a synchronous blocking effect run inline before the next Message (preferred — same-frame visibility, no async listing)." — docs/MVU_PLAN.md (Phase 5)
Inferred intent: this phase attacks the "state-out-of-sync" bug class — "one `git` value feeds top-bar + per-file markers; reunites the torn git channel; removes `last_grid`." The squash subjects confirm each step: #220 "fold git_info/git_files into AppState.git: GitState"; #221 "move harpoon/pane_prompt_buf/last_pane_prompt to AppState"; #222 "route via a Model PaneSnapshot, not the live host"; #224 "eliminate last_grid; thread GridDims as motion params"; #225 "chdir → synchronous Effect::ChangeDir." The chdir fork chose option (a) (synchronous blocking effect) as the plan flagged preferred.   confidence: high
Supersedes: (1) the torn git channel (`App.git_result_rx` + `AppState.git_worker_tx`) and the duplicated `git_info`/`git_files` — unified into `GitState` (#220) + git_result_rx moved to Runtime (#219); (2) `harpoon`'s misplacement on `App` (#221); (3) render's live-`PtyHost` reads for the yank/`gf`/scrollback arms → `PaneSnapshot` + `Effect::ReadPaneText` (#222/#223/#226); (4) `last_grid` — eliminated (#224, pickaxe `git log -S 'last_grid'` confirms removal at "MVU Phase 5 PR6"); (5) inline `:cd` chdir → deferred `Effect::ChangeDir` via the new `CommandResult::Post(Vec<Effect>)` carrier (#230); (6) in-handler `child.wait()` for capture/task exits → `CaptureExit`/`TaskExited` channel messages with a bounded `reap_exit`/`ExitOutcome` digest (#232).

Reconstructed — the slice, folded:
+ #218 extract `apply_git_worker_result` → `git_state.rs` (prep).
+ #219 introduce the Runtime cluster, move `git_result_rx` in (channel reunion begins).
+ #220 fold git into `AppState.git: GitState` (pickaxe: `struct GitState` first appears here).
+ #221 move harpoon + pane-prompt buffers into AppState.
+ #222 route via a Model `PaneSnapshot` not the live host (the borrow-checker fix the plan deferred from 3b to here).
+ #223 `yank_pane`/scrollback via `Effect::ReadPaneText`.
+ #224 eliminate `last_grid`; thread `GridDims` as motion params.
+ #225 chdir → synchronous `Effect::ChangeDir`.
+ #226 `gf` → `ReadPaneText(Pickable)` + `GotoFile`.
+ #230 (PR9) carrier unification: `CommandResult::Post(Vec<Effect>)` so the pure `:cd` arm emits `Effect::ChangeDir` instead of chdir-ing inline (diff shows `Post(Vec<Effect>)` + the `:cd` arm returning `CommandResult::Post(vec![Effect::ChangeDir{…}])`).
+ #232 (PR8) exits-as-messages (Design B): `pub enum ExitOutcome { Exited { code, success }, … }` + `digest_exit`/`reap_exit` in `src/pane/pty_host.rs`; capture/task exit observed via channel message, closing the Phase-3c deferral that the reader "can't call `child.wait()` — `portable_pty` needs `&mut self`."
Note #230 (carrier, 2026-06-01) lands before #232 (exits, 2026-06-02) by date despite the higher PR number — both are "PR8/PR9" out of merge order.

Provenance:
- 3c73580/07d2b2f/fc7c98d/c21e0eb/2ff07f9/29ded59/42866bc/4a00bcb/463de58 (PR #218–#226, 2026-06-01) — the nine Phase-5 migration PRs (subjects quoted above).
- 0aaa73c (PR #230, 2026-06-01) — `CommandResult::Post(Vec<Effect>)` + `:cd → Effect::ChangeDir`; `src/app/state.rs` +73.
- 18a56ef (PR #232, 2026-06-02) — `ExitOutcome`/`digest_exit`/`reap_exit`; `src/pane/pty_host.rs` +101, `src/app/streaming.rs` reshaped.
- docs/MVU_PLAN.md — Phase 5 + the PaneSnapshot/borrow-checker contract (quoted).
- prior entry 01KTMKZ502R8JYARTMFMKSGB72 (this thread, Phase 3) — left the capture/task exit on-channel message as a deferral this phase closes.

<!-- Entry-ID: 01KTMM1A4SE4HRHFQ97PRJCZYD -->

---
Entry: Claude Code (caleb) 2026-06-08T22:01:20.492096+00:00
Role: scribe
Type: Note
Title: PR #233–#239 — Phase 6 PR-A/B/C: single :command table, off-thread agent status, loop-step extraction

Spec: scribe

tags: #history #refactor-mvu

Moment: refactor-mvu — Reconstructed: the three-way `:command` punt-list is replaced by one `COMMAND_TABLE` registry (killing the "unknown command" footgun), agent status-line short-id resolution moves off the render thread, and the busy parts of `App::run` are extracted into `loop_steps.rs`/`sources.rs`/`streaming.rs`/`key_dispatch.rs` step functions.   [kind: refactor]
When: 2026-06-02 · PR #233 (command-table) commit 2176aac · #234 (agent-status-offthread) commit 1458a37 · #235–#239 (PR-C1…C6 loop-step extraction)
Recorded rationale: "Phase 6 — Pure-of-IO View + command table. Replace the three-way `:command` punt-list with one command table `{name, completion_visible, handler: PureDomain|TerminalTouching}`; `SPYC_COMMANDS` derived from it. Done: adding an App-handled `:command` can no longer flash 'unknown command' (regression-tests the `:undo`/`:limit` footgun); `App::run` is ~100 lines." — docs/MVU_PLAN.md (Phase 6). Squash subjects: #233 "single :command registry (COMMAND_TABLE)"; #234 "resolve agent status-line short-id off the render thread"; #235 "extract 7 loop steps to loop_steps.rs."
Inferred intent: this realizes the "two/three-site `:command` punt-list footgun" bug-class row — the prose split (NotHandled walls + unknown-command fallthrough + hand-synced `SPYC_COMMANDS`) becomes one table where "forgetting registration becomes an obvious missing entry, not a runtime flash." The #234 off-thread move is a render-purity prerequisite (status-line work must not block the draw). #235–#239 are the mechanical loop-step carve-out that shrinks `App::run` toward the ~100-line target.   confidence: high
Supersedes: the three-site `:command` dispatch (the NotHandled/unknown-command/`SPYC_COMMANDS` split named in MVU_PLAN.md's bug-class table) → unified `COMMAND_TABLE` (#233; `src/app/state.rs` +225/−... ); on-render-thread agent-status resolution → off-thread (#234). The COMMAND_TABLE here is later relocated to its own module in last-mile #272/#273.

Reconstructed — the slice, folded:
+ #233 (PR-A) single `:command` registry `COMMAND_TABLE` in `src/app/state.rs`.
+ #234 (PR-B) agent status-line short-id resolved off the render thread (a `perf:` commit; `src/app/sources.rs` +5, mod.rs reshaped).
+ #235 (PR-C1) extract 7 loop steps to `loop_steps.rs`.
+ #236 (PR-C2) fold fs-ingest + refresh-debounce into `sources.rs`.
+ #237 (PR-C3) extract pane-output drain to `streaming.rs`.
+ #238 (PR-C4+C5) extract restore-resumes + context-write to `loop_steps.rs`.
+ #239 (PR-C6) extract paste/resize dispatch bodies to `key_dispatch.rs`.
The C-series is pure relocation (no decision beyond "shrink the loop"); folded here.

Provenance:
- 2176aac (PR #233, 2026-06-02) — `COMMAND_TABLE` registry; `src/app/state.rs` +225.
- 1458a37 (PR #234, 2026-06-02) — off-thread agent status resolution; `src/app/mod.rs` reshaped, sources.rs +5.
- 1bd3348/ce5904c/2604625/572965b/83380ac (PR #235–#239, 2026-06-02) — loop-step extractions into loop_steps.rs/sources.rs/streaming.rs/key_dispatch.rs.
- docs/MVU_PLAN.md — Phase 6 + bug-class table row "two/three-site `:command` punt-list footgun" (quoted).
- prior entry 01KTMM03Q5F5EA3VEQSJQJZYN0 (this thread, Phase 4) — established `run_effects` as executor, which the command-table handlers (`PureDomain|TerminalTouching`) target.

<!-- Entry-ID: 01KTMM27M59N08NB1HSWCESQTR -->

---
Entry: Claude Code (caleb) 2026-06-08T22:01:51.909626+00:00
Role: scribe
Type: Note
Title: PR #240–#242 — Phase D: physically split App into Model / Runtime / ViewState

Spec: scribe

tags: #history #refactor-mvu

Moment: refactor-mvu — Reconstructed: the three-type split becomes physical — `ViewState` is introduced (pager group + render caches), OS-handle/`PtyHost` fields move into `Runtime`, and the remaining ~48 `App` fields are sorted into `ViewState`, leaving `App = { state: AppState, runtime: Runtime, view: ViewState }` as three disjoint fields.   [kind: refactor]
When: 2026-06-02 · PR #240 (mvu-phase-d1-viewstate) commit 7e015f4 · #241 (d2-runtime) commit b3170d7 · #242 (d3-viewstate-rest) commit 5b7ec7b
Recorded rationale: Squash subjects — #240 "MVU split D1 — introduce ViewState (pager group + render caches)"; #241 "MVU split D2 — move OS-handle/PtyHost fields into Runtime"; #242 "MVU Phase D3 — move remaining 48 App fields into ViewState." Underlying plan: "State is three types under `src/app/`, with `App = { model, runtime, view }`. … Runtime: OS handles + threads + channels; never serialized; never seen by `update()`. ViewState: render ephemerals + caches… `last_grid` is eliminated." — docs/MVU_PLAN.md (Target state / three types)
Inferred intent: Phase 5 had already migrated the data into the right conceptual buckets via backward-compat accessors; Phase D performs the literal struct surgery, which is why the diffs are wide-but-shallow (#240 +305/−286 over 11 files, #241 +323/−252 over 13, #242 +401/−488 over 11) — field relocations rippling through every reader. The end-state ARCHITECTURE.md records `App` owning "three disjoint fields … `state: AppState` (the Model …), `runtime: Runtime` …, and `view: ViewState`."   confidence: high
Supersedes: the monolithic `App` struct (all fields flat) — partitioned into Model/Runtime/ViewState. Builds directly on the conceptual migration in entry 01KTMM1A4SE4HRHFQ97PRJCZYD (Phase 5). (verified: ViewState/Runtime field relocations span 11–13 files per PR)

Reconstructed: this is the borrow-checker payoff the plan engineered for — once `runtime`, `state`, and `view` are disjoint fields, `render(&model, &view, &runtime)` can hold `&state`/`&view` reads concurrently with a `&mut runtime` resize, and `update(&mut model, &mut view, …)` can run without touching Runtime at all. The three PRs are sequenced introduce-ViewState → move-handles-to-Runtime → sweep-the-rest-into-ViewState, each behavior-equivalent. Folded as one moment (the split is one logical operation across three mechanical PRs).

Provenance:
- 7e015f4 (PR #240, 2026-06-02) — introduce `ViewState`; 11 files +305/−286.
- b3170d7 (PR #241, 2026-06-02) — move OS-handle/`PtyHost` fields into `Runtime`; 13 files +323/−252.
- 5b7ec7b (PR #242, 2026-06-02) — move remaining 48 App fields into ViewState; 11 files +401/−488.
- docs/MVU_PLAN.md — Target state + three-types section (quoted); ARCHITECTURE.md "Three-type state split" (end-state).
- prior entry 01KTMM1A4SE4HRHFQ97PRJCZYD (this thread, Phase 5) — did the conceptual migration this physically realizes.

<!-- Entry-ID: 01KTMM35YXDZ69C08M78F4XKBB -->

---
Entry: Claude Code (caleb) 2026-06-08T22:02:18.166483+00:00
Role: scribe
Type: Note
Title: PR #243–#247 — Phase E: draw accumulator, RunCtx, coalesce, dispatch_effective, render_frame/teardown

Spec: scribe

tags: #history #refactor-mvu

Moment: refactor-mvu — Reconstructed: the event loop's body is restructured into composable pieces — a draw accumulator replaces the scattered `needs_draw`/`draw_reason` flags, loop-step signatures collapse to `&mut RunCtx`, the source-coalescing match becomes `sources::coalesce_recv`, dispatch becomes `dispatch_effective` returning a `DispatchFlow`, and the draw/teardown bodies are extracted to `render_frame`/`run_teardown`.   [kind: refactor]
When: 2026-06-02 · PR #243 (e1-draw-accumulator) commit ac7318a · #244 (e3-runctx) · #245 (e4-coalesce) · #246 (e5-dispatch) commit a827566 · #247 (e6e7-render-teardown) commit 7ccef44
Recorded rationale: Squash subjects — #243 "MVU Phase E1 — Draw accumulator for the event loop"; #244 "MVU Phase E3 — collapse loop-step signatures to &mut RunCtx"; #245 "MVU Phase E4 — extract coalesce match into sources::coalesce_recv"; #246 "MVU Phase E5 — extract dispatch into dispatch_effective + DispatchFlow"; #247 "MVU Phase E6+E7 — render_frame + run_teardown." Underlying plan: "`needs_draw`/`draw_reason` collapse into `update()`'s `dirty: Option<RedrawReason>` return." — docs/MVU_PLAN.md (View)
Inferred intent: with state physically split (Phase D), Phase E is the loop-body cleanup that makes `App::run` legible and small — each PR extracts one structural concern out of the loop. The diffs are mod.rs-only and net-shrinking the loop (#243 +56/−39, #246 +145/−91, #247 +91/−65), consistent with the "loop reaches ~100 lines" target. `RunCtx` (#244) bundles the loop-step parameters so step fns share one `&mut` context rather than long arg lists.   confidence: high
Supersedes: the scattered `needs_draw`/`draw_reason` draw-flag handling (→ draw accumulator, #243); long per-step argument lists (→ `&mut RunCtx`, #244); the inline source-coalescing match (→ `sources::coalesce_recv`, #245); inline dispatch (→ `dispatch_effective` + `DispatchFlow`, #246); the inline draw/teardown bodies (→ `render_frame`/`run_teardown`, #247). Builds on entry 01KTMM35YXDZ69C08M78F4XKBB (Phase D).

Reconstructed: this phase is almost entirely within `src/app/mod.rs` (the loop file) and `src/app/sources.rs` — it does not move data, it reshapes control flow. Folded as one moment: five mechanical loop-body extractions, no behavior change, each named above. After Phase E the loop is the recv → coalesce → dispatch → accumulate-draw → render_frame skeleton the plan targeted, leaving only the last-mile purity work.

Provenance:
- ac7318a (PR #243, 2026-06-02) — draw accumulator; mod.rs +56/−39.
- 24ae5d1/c0b9cd9 (PR #244/#245, 2026-06-02) — `&mut RunCtx` signature collapse; `sources::coalesce_recv`.
- a827566 (PR #246, 2026-06-02) — `dispatch_effective` + `DispatchFlow`; mod.rs +145/−91.
- 7ccef44 (PR #247, 2026-06-02) — `render_frame` + `run_teardown`; mod.rs +91/−65.
- docs/MVU_PLAN.md — View section (`dirty: Option<RedrawReason>` collapse, quoted).
- prior entry 01KTMM35YXDZ69C08M78F4XKBB (this thread, Phase D) — supplied the disjoint fields the reshaped loop reads.

<!-- Entry-ID: 01KTMM409Y3RTZPDTVWPQWR6EW -->

---
Entry: Claude Code (caleb) 2026-06-08T22:02:50.698318+00:00
Role: scribe
Type: Note
Title: PR #260–#266 — Last-mile: arch-docs milestone, mutation-free render path, dispatch_prompt de-IO

Spec: scribe

tags: #history #refactor-mvu

Moment: refactor-mvu — Reconstructed: the last-mile pass opens with the ARCHITECTURE.md milestone declaring MVU "landed," then makes the render path mutation-free — a pre-frame `prepare_frame` settles rows/grid and resize/drain moves out of the draw path so `render_inner` takes `&self` — behind a `TestBackend` full-frame snapshot net, and de-IOs `dispatch_prompt`.   [kind: refactor]
When: 2026-06-03 · PR #260 (lastmile-arch-docs) commit f921164 · #261 git-worker-tx · #262 render-snapshots · #263 prepare-frame · #264 render-drain · #265 render-self · #266 dispatch-prompt-deio commit 9269fa8
Recorded rationale: "spyc follows the Elm/Model-View-Update pattern. The structural migration … has landed; a final purity pass is in progress… Three-type state split. `App` owns three disjoint fields … Single message channel … `App::run` is event-driven: it blocks on `recv` / `recv_timeout` (0 wakes at idle …) — there is no `event::poll`, no adaptive busy-poll. … `run_effects` is the sole executor; handlers return `Vec<Effect>` and never touch the OS directly. This makes 'forgot to clear `pending_X`' and inline-IO bug classes structurally hard." — ARCHITECTURE.md (added/edited PR #260). And the named remaining work: "collapsing the three update entry points (`ApplyResult`/`CommandResult`/`PromptResult`) into one `update(&mut Model, &mut ViewState, msg, now) -> Vec<Effect>`; moving the last inline side-effects behind effects; and making the render pass mutation-free (a pre-frame `prepare` step) behind a ratatui `TestBackend` snapshot net." — ARCHITECTURE.md (Remaining last-mile work)
Inferred intent: #260 is the documentation milestone (also lists the `Effect` end-state vocabulary: ForegroundExec, CopyToClipboard, SignalGroup, SendToPane, SetTerminalTitle, ReadPaneText, ChangeDir). The render-purity sub-PRs execute the "mutation-free render" goal in order: #262 lands the `TestBackend` + insta snapshot net first (the safety harness), then #263 extracts `prepare_frame` to settle list rows/grid before the draw, #264 relocates pane/overlay resize+drain out of the draw path, #265 makes `render_inner` take `&self`. #266 de-IOs `AppState::dispatch_prompt` (moving IO arms to the executor). #261 completes the git-channel reunion (moves `git_worker_tx` to Runtime; Model records requests via an outbox).   confidence: high
Supersedes: render's residual `&mut self` mutation during draw → mutation-free `&self` `render_inner` (#263–#265); `dispatch_prompt`'s inline IO arms → executor effects (#266); the last `git_worker_tx` on AppState → Runtime outbox (#261, completing the Phase-5 reunion). Builds on entry 01KTMM409Y3RTZPDTVWPQWR6EW (Phase E).

Reconstructed: the snapshot-net-first ordering (#262 before #263–#265) is the strangler-fig discipline applied to the riskiest remaining surgery — full-frame `TestBackend` snapshots pin the rendered output before the draw path is restructured. This pass tracks the View row of MVU_PLAN's goals: "zero clipboard/title/fs IO (the achievable purity goal), `&mut` only for ratatui `StatefulWidget`s." Folded: #261/#262 (channel + snapshot net) and #263–#266 (render purity + prompt de-IO) as one last-mile-render moment.

Provenance:
- f921164 (PR #260, 2026-06-03) — ARCHITECTURE.md +/− (the "MVU has landed" + Effect vocabulary + remaining-work section, quoted); AGENTS.md updated.
- 938a1fd (PR #261) — git_worker_tx → Runtime, Model outbox.
- 6fb5cb5 (PR #262) — `TestBackend` + insta full-frame snapshot net.
- e5c6d9c/8bf7aa2/675e182 (PR #263/#264/#265, 2026-06-03) — `prepare_frame`; resize+drain out of draw; `render_inner(&self)`.
- 9269fa8 (PR #266, 2026-06-03) — de-IO `AppState::dispatch_prompt`.
- prior entry 01KTMM409Y3RTZPDTVWPQWR6EW (this thread, Phase E) — left the render/dispatch loop body for this purity pass.

<!-- Entry-ID: 01KTMM4ZMPP9DCW3R3NK5BDA93 -->

---
Entry: Claude Code (caleb) 2026-06-08T22:03:25.678784+00:00
Role: scribe
Type: Note
Title: PR #267–#274 — Last-mile: unified Update enum → single App::update entry point, command-table module, decide_focus

Spec: scribe

tags: #history #refactor-mvu

Moment: refactor-mvu — Reconstructed: the three separate update entry points (`ApplyResult`/`CommandResult`/`PromptResult`) collapse into one — a unified `Update` enum with `From` conversions (Stage 3B), bridges consuming it (3C), and finally a single `App::update(msg)` entry (3D) — alongside relocating the `:command` registry into `command_table.rs`, extracting pure agent resume-strippers/resolvers into `src/agent/resume.rs`, and extracting `decide_focus` as a pure tested fn.   [kind: refactor]
When: 2026-06-03 · PR #267 (update-enum) commit c6beaa4 · #268 (update-collapse) · #269 (update-entry) commit 266c306 · #270 agent-strippers · #271 resume-resolver · #272 cmd-table-move · #273 cmd-handlers · #274 decide-focus commit a1e487a
Recorded rationale: Squash subjects — #267 "add unified Update enum + From conversions (Stage 3B)"; #268 "bridges consume the unified Update enum (Stage 3C)"; #269 "single App::update(msg) entry point (Stage 3D)"; #272 "relocate the :command registry into command_table.rs (Part A.1)"; #273 "table-driven :command dispatch with compile-checked handlers (Part A.2)"; #274 "extract decide_focus as a pure fn behind tests (Stage 5)." Underlying plan: "Collapse the three update entry pairs into one `update(&mut Model, &mut ViewState, Message, now) -> Vec<Effect>`." — docs/MVU_PLAN.md (Phase 5); and ARCHITECTURE.md's named last-mile work: "collapsing the three update entry points (`ApplyResult`/`CommandResult`/`PromptResult`) into one `update(…)`."
Inferred intent: this closes the single-`update()` goal the plan deferred from Phase 5 and ARCHITECTURE.md flagged as in-progress. The staged sequence (3B add enum + From shims → 3C bridges consume it → 3D one entry) is the strangler-fig pattern applied to the result-enum collapse: the unified enum coexists with the three carriers before they're removed. #272/#273 finish the Phase-6 command table by giving it its own module (`command_table.rs`) with compile-checked handlers (so a missing registration is a compile error, not a runtime "unknown command" flash). #270/#271 lift the pure agent resume logic into `src/agent/resume.rs`; #274 extracts `decide_focus` as a pure tested fn — both are de-IO/purity tidy-ups.   confidence: high
Supersedes: the three-way update carrier (`ApplyResult`/`CommandResult`/`PromptResult` entry points) → one `App::update(msg)` (#267→#269); the in-`state.rs` `COMMAND_TABLE` from Phase-6 entry 01KTMM27M59N08NB1HSWCESQTR → relocated to `command_table.rs` with compile-checked handlers (#272/#273). Builds on entry 01KTMM4ZMPP9DCW3R3NK5BDA93 (last-mile render).

Reconstructed — the slice, folded:
+ #267 unified `Update` enum + `From` conversions (Stage 3B).
+ #268 bridges consume the unified `Update` enum (Stage 3C).
+ #269 single `App::update(msg)` entry point (Stage 3D) — the collapse completes.
+ #270 move pure resume-strippers into `src/agent/resume.rs`.
+ #271 move resume-target resolvers into `agent/resume.rs` (Stage 4.2).
+ #272 relocate the `:command` registry into `command_table.rs` (Part A.1).
+ #273 table-driven `:command` dispatch with compile-checked handlers (Part A.2).
+ #274 extract `decide_focus` as a pure fn behind tests (Stage 5).
This is the terminal moment of the MVU campaign in this slice: the Model/View/Update triad is structurally complete — one channel, one `update` entry, effects-as-data executed by `run_effects`, a mutation-free render path, and a compile-checked command table. The campaign continues into the module-decomposition track (see history-seg-module-decomposition, #248–#308) which carves the now-MVU `src/app/` further; the gix work proceeds on history-seg-gix-migration.

Provenance:
- c6beaa4 (PR #267, 2026-06-03) — unified `Update` enum + From; Stage 3B.
- 4b60ae8/266c306 (PR #268/#269, 2026-06-03) — bridges consume Update; single `App::update(msg)` entry (3C/3D).
- df32eb7/794b2a7 (PR #270/#271, 2026-06-03) — `src/agent/resume.rs` strippers + resolvers.
- 0b8c8b2/aaec6847 (PR #272/#273, 2026-06-03) — `command_table.rs` relocation + compile-checked handlers.
- a1e487a (PR #274, 2026-06-03) — `decide_focus` pure fn behind tests.
- docs/MVU_PLAN.md (Phase 5 collapse) + ARCHITECTURE.md (Remaining last-mile work) — quoted.
- prior entries 01KTMM27M59N08NB1HSWCESQTR (Phase 6 command table) and 01KTMM4ZMPP9DCW3R3NK5BDA93 (last-mile render) in this thread.

<!-- Entry-ID: 01KTMM60KR8W18TWXPXDGT9J8Y -->
