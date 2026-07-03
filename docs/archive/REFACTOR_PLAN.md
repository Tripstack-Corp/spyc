# Refactor plan — `app/mod.rs` decomposition

> **Status (2026-05-30): Phases 1 & 2 COMPLETE.** All six Phase 1
> struct extractions (PRs #180–#185) and all four Phase 2 medium
> extractions (PRs #187, #189, #190, #191) have landed; `app/mod.rs` is
> down from ~12.5k to **8,427 lines**, and `src/app/` is now a 12-file
> directory. The rendering pass, pager-key router, colon-command
> dispatch, and keyboard-input cluster are all out. **The MVU rewrite
> (Phase 3) is now a pre-2.0 / road-to-2.0 track** — see the detailed
> design in [`docs/MVU_PLAN.md`](docs/MVU_PLAN.md). This **reverses** the
> earlier "hold the MVU rewrite until 2.0 + ~2 weeks" gate (decision log,
> 2026-05-30): 2.0 should ship *on* the cleaner foundation rather than
> carry a refactor as launch overhang, and the strangler-fig design makes
> it safe to land incrementally (every phase behavior-equivalent behind
> green CI). The "When to start" / "Why we're not doing this right now"
> sections below are **historical context** for the original whole-plan
> hold and no longer gate the MVU work.

Working doc for the staged decomposition of `app/mod.rs` (now ~12k
lines, ~150 fns) into smaller, more reviewable units. ROADMAP
already mentions the eventual Model-View-Update (Elm-style) target;
this doc is the *staged path* there, with cheap wins first and the
big architectural rewrite at the end.

## Goal

Smaller, more reviewable, more testable code. Concretely:

- No file in `src/app/` over ~1500 lines.
- Handlers ~5–20 lines each (vs. 50–200 today), composed of pure
  data transformations.
- Side effects modeled as data (`Effect::Spawn { ... }`) so handlers
  are unit-testable without a real PTY / real signal / real disk.
- Render and event handling fully separated.

## Why we're not doing this *right now*

- Pre-2.0. Architectural rewrite delays launch and can't be reverted
  cheaply.
- Shape is still emerging. `:pause`/`:resume` shipped *today*; the
  MCP search tools shipped two weeks ago; the markdown table renderer
  is a week old. Committing to architecture before shape stabilizes
  risks abstracting around the wrong joints.
- The mechanical extractions in **Phase 1** below buy ~70% of the
  review-ability win for ~5% of the architectural risk.

## When to start

Pick up when at least two of these are true:

- 2.0 has shipped and stabilized for ~2 weeks.
- A bug fix touches three different handlers in `app/mod.rs` that
  duplicate the same kind of logic (e.g. "forgot to clear
  `pending_X` when cancelling Y") — the codebase telling you the
  seam is wrong.
- You find yourself opening `app/mod.rs` and using search to navigate
  rather than scrolling, more than once a session.
- You'd hesitate to take an outside contributor's PR because of how
  hard the change-set would be to review against the megafile.

## Focus required

- **Phase 1**: ~30–60 min per extraction, low cognitive load.
  Mechanical moves with no behavior change. Can be done
  incrementally between feature work without breaking flow.
- **Phase 2**: ~half-day per extraction. Real focus needed because
  some interfaces have to be designed (e.g. the pager-key handler
  surface). Don't context-switch mid-extraction.
- **Phase 3 (MVU)**: superseded by the strangler-fig design in
  [`docs/MVU_PLAN.md`](docs/MVU_PLAN.md). The original framing here said
  "block out a full week; partial conversions are worse than either alone"
  — that assumed a big-bang. The strangler-fig plan instead lands ~8
  phases, **each behavior-equivalent behind green CI and independently
  revertable**, so partial conversion is the *intended* steady state and it
  interleaves with feature work like Phases 1–2 did. Per-phase focus is
  ~half-day to multi-day depending on the phase (the single-channel loop
  rewrite is the heaviest, and it's last).

---

## Phase 1 — Cheap mechanical extractions ✅ DONE (2026-05-30)

Self-contained types + impls that landed in `app/mod.rs` by inertia,
not coupling. Each was a verbatim move + a `mod ...; use ...;` import
— no behavior change, no API change. Each was one PR.

| # | Extract | Target file | PR | Status |
|---|---------|-------------|----|--------|
| 1 | `BackgroundTasks` + `BackgroundTask` + `TaskStatus` | `src/app/tasks.rs` | #184 | ✅ |
| 2 | `PagerHistory` + `MAX_PAGER_HISTORY` | `src/app/pager_history.rs` | #180 | ✅ |
| 3 | `FindPicker` + `refilter`/`drain_walk` | `src/app/find_picker.rs` | #181 | ✅ |
| 4 | `GrepSession` | `src/app/grep_session.rs` | #182 | ✅ |
| 5 | `Prompt` + `PromptKind` + `simple()`/`shell()` ctors | `src/app/prompt.rs` | #183 | ✅ |
| 6 | `PendingCapture` | `src/app/capture.rs` | #185 | ✅ |

**Scoping note:** items 1, 3, 4, 6 in the original plan also listed
App-coupled methods (`spawn_capture`, `strip_crlf`, grep drain, etc.).
Those take `&mut self` and read App state directly, so they stayed in
`app` and the extracted modules are the *data* structs only (fields
`pub` so `app` reads them). This is the one-way-dependency rule, not a
shortcut — the methods are Phase 2 (handler) material, not Phase 1.

Result: `app/mod.rs` went from ~12,450 → 11,757 LOC (~700 off). Less
than the ~1000 estimate because the method bodies stayed; the structs
alone are smaller than the table's LOC guesses. Each PR shipped on its
own with the full gate green and zero test edits.

### Phase 1 done-criteria

- `wc -l src/app/mod.rs` down by ~1000 from its pre-extraction size
  (the six Phase 1 moves are ~1000 LOC; the absolute target floats
  with the file's current size).
- Every extracted module compiles standalone with no imports back
  into `app/mod.rs` (one-way dependency only).
- All tests still pass (no test edits — pure moves).
- `cargo clippy --locked --all-targets -- -D warnings` clean.

---

## Phase 2 — Medium extractions ✅ DONE (2026-05-30)

All four planned extractions landed, one PR each, same child-module
`impl App` pattern: the moved methods live in a private child module
and read App's private state via the descendant-module rule, so almost
nothing became `pub` — only the handful of entry points called from
`app` or sibling modules. Explicit imports throughout (the codebase
denies `wildcard_imports`).

| # | Extract | Target file | PR | Status |
|---|---------|-------------|----|--------|
| 7 | `render` + `compute_layout` + status/harpoon render | `src/app/render.rs` (1114) | #187 | ✅ |
| 8 | `handle_pager_key` | `src/app/pager_handler.rs` (1078) | #189 | ✅ |
| 9 | `dispatch_command` (terminal-touching `:` arms) | `src/app/commands.rs` (321) | #190 | ✅ |
| 10 | `handle_key` + mode sub-handlers (`apply_user`, prompt/vi-prompt, confirm handlers, `undo_last_remove`) | `src/app/key_dispatch.rs` (916) | #191 | ✅ |

`app/mod.rs`: 12,450 (pre-Phase-1) → 10,672 (post-Phase-1) → **8,427**
(post-Phase-2). `src/app/` is now a 12-file directory.

### Phase 2 done-criteria

- ✅ `app/mod.rs` no longer contains any rendering code or any
  pager-key / top-level-key handling — those are the qualitative goals
  and they're met. The render pass, the pager-key router, the colon
  command dispatch, and the keyboard-input cluster are all out.
- ✅ Tests still pass (785, no test edits); `make check` +
  `make lint-linux` clean. **Note:** Phase 2 surfaced that the host
  (macOS) clippy never lints `cfg(target_os = "linux")` code — a latent
  `collapsible_if` in `clipboard.rs` failed only on Linux CI. Fixed in
  #188, which also added `make lint-linux` (cross-lint via zig) to catch
  this class locally. Run it for any OS-gated change.
- ⚠️ `wc -l src/app/mod.rs ≤ 4500` is **not** met (8,427). That number
  was estimated off a ~7.4k baseline; the real pre-refactor file was
  ~12.5k, so the four planned moves can't reach it. What remains is
  largely what the criterion *intends* to stay — the App struct,
  `App::new`, and the ~920-line `run` event loop — plus things not in
  the Phase 2 plan that are candidates for a follow-on pass if we want
  mod.rs smaller before Phase 3: `apply_inner` (the ~370-line action
  dispatcher), `update_term_title`, `save_session`/`restore_session`,
  the agent resume free-fns (`command_without_*_resume`,
  `resolve_*_resume_target`), and the in-file test modules. None block
  Phase 3; treat them as optional tidy-up, not required scope.

---

## Phase 3 — MVU rewrite

> **Detailed design: [`docs/MVU_PLAN.md`](docs/MVU_PLAN.md)** — a
> strangler-fig, 8-phase (Phase -1 … 6) migration with Model/Runtime/
> ViewState split, a single `Message` channel, a four-class `Effect`
> vocabulary, and a `Focus` value. **Now a pre-2.0 / road-to-2.0 track**
> (2026-05-30 decision): Phase 0 (Focus-as-one-value) lands first as a
> daily-driver bug fix; Phases 1–6 land incrementally before launch, after
> the test-harness de-risking. The sketch in this section is the original
> high-level target; `docs/MVU_PLAN.md` supersedes it with concrete,
> adversarially-vetted phases.

The architectural change — but **not** the block-out-a-week big-bang the
original framing below assumed. The strangler-fig design makes every phase
behavior-equivalent behind green CI and independently revertable, so it lands
incrementally pre-2.0 and interleaves with the other road-to-2.0 tracks rather
than requiring one unbroken multi-day push. Follow `docs/MVU_PLAN.md`.

### Target shape

```rust
struct Model { /* all state */ }

enum Message {
    Key(KeyEvent),
    PaneOutput(usize, Vec<u8>),
    CaptureExit { id: u32, status: ExitStatus },
    GrepBatch(Vec<GrepMatch>),
    /* ... */
}

enum Effect {
    Spawn { cmd: String, cwd: PathBuf, sink: SinkId },
    SignalGroup { pgid: i32, sig: i32 },
    OpenInPager { argv: Vec<String> },
    WriteFile { path: PathBuf, bytes: Vec<u8> },
    /* ... */
}

fn update(model: &mut Model, msg: Message) -> Vec<Effect> { ... }
fn view(model: &Model, frame: &mut Frame) { ... }

// Runtime: pulls Messages from sources (terminal, mpsc rxs, fs
// watcher), calls update, runs Effects (which may produce more
// Messages), calls view. Single owner of side effects.
```

### Why the wait pays off

- **Testability collapses to 1-2 lines per case.** `assert_eq!(update(&mut m, Msg::PauseTask(123)), vec![Effect::SignalGroup { ... }]);`
- **Effect runtime is the only place side effects live.** All
  PTY-spawning, signal-sending, file-writing, suspend_tui — one
  module owns them. Tests double the runtime.
- **Handlers shrink to 5–20 lines.** Today's 700-line
  `handle_pager_key` becomes ~30 small fns in a `pager_update.rs`
  module of similar total LOC, but each fn is independently
  reviewable.
- **Bug class eliminated.** The "forgot to clear pending_X when
  cancelling Y" bug we hit several times in v1.27.x–v1.32.x is
  structurally impossible: `update` returns the new state from
  scratch (or a clearly-isolated mutation), and effects can't
  smuggle hidden mutations past the type system.

### Phase 3 done-criteria

- One `Model` struct holding all state; no state lives elsewhere.
- One `Message` enum, one `Effect` enum, one `update` fn, one `view`
  fn. The runtime loop is ~50 lines.
- Side effects only happen inside the runtime's effect executor —
  not in `update`, not in `view`.
- Tests can construct a `Model`, send a `Message`, assert the
  resulting `(Model, Vec<Effect>)` without needing a PTY, the
  filesystem, or a TUI.

---

## Decision log

(Append decisions here as we go, so future-us understands *why*
the plan changed.)

- **2026-04-29**: Plan written. Holding Phase 1 until after 2.0
  ships. Mechanical extractions OK to interleave with feature work
  if review friction gets bad before then.
- **2026-05-30**: Go on Phases 1–2 *now*, as the road-to-2.0
  decomposition track (ROADMAP "Lean 2.0" sequencing). Trigger: the
  file crossed ~12k lines, navigation is search-not-scroll, and the
  agent-registry work showed how many fixes touch multiple handlers
  in the megafile. Decomposition also unblocks the 2.x crate split
  (`docs/archive/V1_70_PLAN.md`) — can't split a 12k-line monolith. Phase 3
  (MVU) still held until 2.0 has shipped + stabilized ~2 weeks.
  *(Superseded later the same day — see the "GATE REVERSED" entry below;
  MVU moved pre-2.0.)*
- **2026-05-30**: Phase 1 complete (PRs #180–#185). Six struct
  extractions, one PR each, no behavior change, no test edits, gate
  green throughout. Scoped to *data structs* — App-coupled `&mut self`
  methods stayed put (they're Phase 2 handler work; pulling them now
  would violate the one-way-dependency rule). Net ~700 LOC off
  `app/mod.rs` (11,757 now). Phase 2 (render / pager-key / command /
  key-dispatch handlers) is the next track; each is ~half-day and
  needs real interface design, so not "pick up any spare moment" work.
- **2026-05-30**: Phase 2 complete (PRs #187, #189, #190, #191). Four
  medium extractions, one PR each, no behavior change, no test edits.
  Established the **child-module `impl App`** pattern: a method moved to
  a private child module (`mod render;` etc.) reads App's private state
  via the descendant-module rule, so fields stay private and only the
  few cross-module entry points need `pub` (e.g. key_dispatch exposed 3
  of 8 methods). `mod.rs`: 10,672 → 8,427. The ≤4500 target wasn't met —
  it assumed a ~7.4k baseline; the real file was ~12.5k, and the run
  loop / App struct (which the criterion intends to keep) account for
  most of the remainder. Further shrinking (`apply_inner`, term-title,
  session save/restore, agent free-fns) is optional tidy-up, not blocked
  work and not required before Phase 3.
- **2026-05-30**: Two workflow fixes adopted mid-Phase-2, both now
  memories: (1) merge PRs with `bkt pr merge --close-source=false` and
  don't delete branches — the default deletes the source branch and
  Bitbucket Pipelines fails "branch not found" (the earlier #180–#186
  merges never had a passing pipeline because of this). (2) `make
  lint-linux` added (#188) and run for OS-gated changes — host clippy on
  macOS compiles out `cfg(target_os = "linux")` code, so a latent lint
  in clipboard.rs only failed on Linux CI.
- **2026-05-30**: Phase 2 tidy-up (the optional follow-on candidates from
  the done-criteria). Extracted `apply`/`apply_inner` → `actions.rs`
  (#193) and the four session methods → `session.rs` (#194, four
  non-contiguous cuts since they were interleaved with history-popup /
  worktree / sizing helpers). `mod.rs`: 8,427 → 7,689. **Deliberately
  did NOT extract** the agent-resume helpers (`command_without_*_resume`,
  `resolve_*_resume_target`): they're split across free-fn and `impl App`
  forms with tests far away and are `pub`/`pub(crate)` called from
  `src/agent/*.rs`, so moving them is cross-module path churn that partly
  undoes the agent-registry refactor's deliberate "keep strippers in
  `app`" decision. Stopping the mechanical tidy-up here; what's left in
  `mod.rs` is the `App` struct, `run` loop, and intentional glue.
- **2026-05-30**: Documented the decomposed layout (AGENTS.md module
  index + ARCHITECTURE.md) and added an anti-monolith guardrail
  (`app::guard_tests::mod_rs_stays_decomposed`, ceiling 8,500) so `mod.rs`
  can't silently creep back toward a monolith — the test directs
  contributors to extract a module rather than raise the ceiling.
- **2026-05-30**: Drew up the full MVU design ([`docs/MVU_PLAN.md`](docs/MVU_PLAN.md),
  PR #196) via a multi-agent design workflow — strangler-fig won a judge
  panel (46/50) over effect-first (45) and model-first (40); the borrow-
  checker / sync-only / scope-honesty reviewers surfaced the blockers
  (render reads the live PtyHost → PaneSnapshot; parkable reader vs
  foreground-child stdin contention; load-bearing pane poll floor;
  lost-wakeup-safe dirty-bit; SendToPane; MCP read-after-write ordering;
  chdir de-IO fork) which are folded into the plan.
- **2026-05-30 — GATE REVERSED (user decision)**: the MVU rewrite moves
  **pre-2.0**, into the road-to-2.0 track, reversing the "hold until 2.0 +
  ~2 weeks" trigger above. Rationale (user): "I want it done pre-2.0; I
  don't want to launch and then be faced with a big refactor" — 2.0 should
  ship *on* the cleaner foundation, not carry it as overhang. This is only
  safe because of the strangler-fig design: every phase is
  behavior-equivalent behind green CI and independently revertable, so it
  lands incrementally and interleaves with the other road-to-2.0 tracks
  rather than being a block-out-a-week big-bang. **Honest trade-off
  accepted:** deep loop/concurrency surgery before a public launch carries
  timing/focus/stdin regression risk that behavior-equivalence tests don't
  fully catch; mitigated by the test harness landing first, per-phase green
  CI + manual smoke, Phase 0 (lowest-risk) first / the single-channel loop
  rewrite last, and isolation-revertability per phase. The "When to start"
  / "Why we're not doing this right now" sections above are now historical
  context, not live gates.
