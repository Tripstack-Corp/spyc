# MVU migration plan — spyc → full Elm architecture

> **Status (2026-05-30): APPROVED — pre-2.0 / road-to-2.0 track.**
> This is the detailed design for `REFACTOR_PLAN.md`'s Phase 3 (the
> Model-View-Update rewrite). It is a **strangler-fig** migration: the MVU
> machinery grows *alongside* the existing `App::run` busy-poll loop and
> never replaces it in one step.
>
> **Sequencing decision (2026-05-30): this lands pre-2.0**, reversing the
> earlier "hold the MVU rewrite until 2.0 + ~2 weeks" gate. Rationale: 2.0
> should ship *on* the cleaner foundation, not carry a big-bang refactor as
> launch overhang. This is only safe because of the strangler-fig design —
> every phase is **behavior-equivalent behind green CI** (all 786 tests, no
> assertion edits), so the work lands incrementally and interleaves with the
> other road-to-2.0 tracks rather than being a block-out-a-week rewrite.
> **Phase 0 (Focus-as-one-value) lands first** as a standalone daily-driver
> bug fix (highest bug-leverage, zero loop change). **Phases 1–6 land
> incrementally before launch, sequenced after the test-harness de-risking**
> (`docs/archive/TESTING_STRATEGY.md`) so regressions in the loop/effect surgery
> are caught.
>
> **Trade-off (honest):** doing deep loop/concurrency surgery before a public
> launch carries regression risk that behavior-equivalence tests don't fully
> catch (timing, focus, stdin) — see Risks. Mitigated by per-phase green CI +
> manual daily-driver smoke + the test harness landing first, and by
> sequencing the lowest-risk, highest-value phase (0) first and the scariest
> (the single-channel loop rewrite) last.

Target pattern: the canonical ratatui Elm architecture
(<https://ratatui.rs/concepts/application-patterns/the-elm-architecture/>) —
a `Model`, a `Message` enum, `update(model, msg) → effects`, `view(model)`,
one message channel, and side-effects represented as data and run by the
runtime. spyc already has the *bones* of this (`AppState::apply → ApplyResult`
is a partial Update; `render.rs` is the View; `PostAction` is effect-as-data;
`route.rs` is a pure router). The work is to finish and unify them.

---

## Why — the bug classes a cleaner design prevents

This migration is motivated by recurring, design-rooted bug classes (grounded
in `BUGS.md`), not by aesthetics. Scope is stated honestly — where a class is
only *partially* closed, that's called out.

| Bug class | Evidence | How MVU prevents it | Honest scope |
|---|---|---|---|
| **Focus-model confusion** — paste reaching the wrong surface; `V`-editor `^a-c` launching a pane | `BUGS.md` literally asks "do we need a better model for what activity is in focus"; ~8 scattered booleans, ~10 copy-pasted `pane_focused = false` sites | **Phase 0**: one `Focus` enum field with a single writer path; `route.rs`'s focus-axis inputs, the render DIM cue, and `^C` signal-delivery all derive from it, so on the focus axis the dimmed half, input target, and paste target can't disagree | Closes the focus/overlay/pane axis (paste-to-wrong-surface, V-editor). The scrollback/exited/mid-chord axes are **not** subsumed. |
| **Key-routing shape bugs** | route.rs's five-in-one-week history (#75 paste leak, #78/#80 chord swallowed, #81 exited-tab dropped, the V-key bug) | route.rs is already pure; Phase 0 makes its focus-axis inputs derive from `model.focus`; Phase 1's strict Press-filter-for-Key-only protects paste/resize pass-through | `RouteSnapshot` has 8 inputs; `Focus` subsumes ~3. Chord-swallowed (#80) and exited-tab (#81) are **not** claimed structurally closed — route.rs's 20+ regression tests stay as the guard. |
| **`^C` / signal routing to wrong target** | task-pager `^C` cancelling the lower pane | Phase 0 gives one `Focus` owner of the keystroke; Phase 4 maps the **signal-delivery** `^C` sites to `SignalGroup` effects | 5+ `^C` sites, only 2 are signal delivery; prompt-cancel/buffer-clear/flash-hint stay pure transitions. `^\` SIGQUIT / `^t` are **not** closed (byte-forwarded today; a spyc-level escape-hatch is a separate behavior-change PR). |
| **Two/three-site `:command` punt-list footgun** | silent "unknown command" flash; bitten on `:undo` (v1.41.1) and `:limit` | Phase 6 replaces the prose split (NotHandled walls + unknown-command fallthrough + hand-synced `SPYC_COMMANDS`) with **one command table**; forgetting registration becomes an obvious missing entry, not a runtime flash | — |
| **State-out-of-sync** across App/AppState + derived caches | `git_files` updated but not `git_info`; basename-collision markers; `last_grid` read back as state | Phase 5 collapses duplicated facts to one source of truth (one `git` value feeds top-bar + per-file markers); reunites the torn git channel; removes `last_grid` | The `[EOF]`/`:fg`-empty bugs have a derived-view sub-case (closed by Phase 6's per-frame pager recompute) **and** a timing sub-case (closed *iff* Phase 2's timing-equivalence harness passes) |
| **PostAction / inline side-effect anemia** | only `None`+`Spawn` modeled; 5 inline `clipboard::copy` sites that do IO while returning `PostAction::None` | Phase 4 widens `PostAction` into the full `Effect` vocabulary; the run loop becomes the sole executor | — |
| **Forgot-to-clear `pending_X`** | recurring v1.27.x–v1.32.x | Phase 5's `update(&mut Model, Message, now) → Vec<Effect>` isolates transitions; pending fields get one owner/lifecycle | — |
| **Self-refresh feedback loop** | `WriteContext` writes inside the watched dir → FsEvent → refresh → rewrite | Phase 4 models context writes as effects; the `.spyc-context-*` self-write filter lives in the Runtime subscription | — |

---

## Current state (the snapshot we're migrating from)

> Line numbers below are a **snapshot** and will drift; done-criteria use
> grep **patterns**, not line anchors.

`App::run` (`src/app/mod.rs`, ~920 lines) is a **busy-poll loop**:
`while !should_quit`, each iteration non-blocking-drains 7+ independent
sources, computes an adaptive `poll_ms` (16/100/500ms), then blocks **only**
on `event::poll(poll_ms)` before `event::read()`. **There is no single
message channel.**

Sources today:
- **crossterm input** — the only thing `poll()` blocks on.
- **notify watcher** — already an mpsc channel, drained via `try_recv` with a
  hand-rolled trailing-debounce gated by the pure, unit-tested
  `should_fire_refresh`.
- **per-Pane PTY output** — **not** a channel: an `AtomicU64 parser_gen`
  bumped by a parser-worker thread, **polled** by diffing `last_seen_gen`.
  (Verified: the worker does *not* send wakeups today.)
- **background `!` captures / `:fg` tasks** — `PtyHost` byte-channel drains,
  `child.wait()` done in-handler.
- **MCP command channel** — `mcp_cmd_rx` with a per-request one-shot reply
  `Sender` (already MVU-shaped).
- **git-status worker** — request/result mpsc with generation-drop (already
  MVU-shaped, but the channel is **torn** across `App.git_result_rx` and
  `AppState.git_worker_tx`).
- **finder / grep** session channels.

On top sit pure-timer checks (1Hz git poll, 1s activity rollover, 150ms
context-write debounce, 300ms resume-enter delay) that only advance because
`poll()` periodically wakes the loop. The **16ms typing-burst hack** exists
only because `poll` can't wake on pane output; the **100ms idle-pane poll** is
the *only* thing that makes idle-pane output appear before the next keystroke
(load-bearing for streaming visibility).

Partial MVU already present: `AppState::apply → ApplyResult{Handled, OpenPager,
Post(PostAction), NotHandled}` (pane/pager/git/theme/yank arms fall to
`NotHandled` and run inline impurely). `PostAction` is effect-as-data but
executed only at the loop's PostAction site, with one meaningful variant
(`Spawn`, no cwd / no completion message). Side effects bypass `PostAction`
and run inline: 5 `clipboard::copy` sites (3 of which read the **Runtime-owned
`PtyHost`** for yank/`gf` text), `kill_pg`/SIGSTOP/SIGCONT, `term_title::set`
from the draw branch, `write_context` (synchronous-by-design for the MCP
read-after-write contract), `save_session`, MCP-config writes, and
`send_pending_resumes` (a two-phase pane write).

Focus is **not modeled** — reconstructed from ~8 booleans at every site, with
~10 copy-pasted `pane_focused = false` overlay-open mutations and 5+
semantically distinct `^C` sites. `AppState` is **not pure** — it holds
`git_worker_tx` (a `Sender`) and does blocking IO (`chdir` does `canonicalize`
+ `Listing::read` consumed in the same call). `render.rs` reads the **live
vt100 screen** out of the `PtyHost` (via `with_screen` + `active_mut().resize`).

---

## Target state (where we're going)

`App::run` is a ~100-line loop:

```rust
loop {
    let now = Instant::now();
    let msg = rx.recv_timeout(min(next_deadline, pane_floor) - now);
    let effects = update(&mut model, &mut view, msg, now);
    for e in effects {
        if let Some(m) = runtime.run_effect(e) { /* re-enter via channel */ }
    }
    if dirty { ui::render(frame, &model, &view, &runtime) }
}
```

**One `mpsc::Receiver<Message>`** is fed by every source: a **parkable**
crossterm reader thread (`Message::Input`, Press-filtered for `Key` events
only; Paste/Resize/Focus pass through), the notify watcher (`FsEvent`),
per-pane parser-workers (coalesced `PaneOutput{tab}` **wakeups**, not bytes —
parsing stays off the main thread), capture/task readers, the MCP channel
(`Mcp`, reply via an `McpReply` effect), the git worker (`GitResult`),
finder/grep (`FindBatch`/`GrepBatch`), and a timer/deadline scheduler
(`Tick`). The adaptive `poll_ms` math, the 16ms typing-burst hack, and the
100ms idle-pane floor are deleted **together** once real wakeups land (Phase
3b), not before.

State is **three types** under `src/app/`, with `App = { model, runtime, view }`.

---

## The three types — Model / Runtime / ViewState

Bucketed field-by-field against the real struct defs; realized as three Rust
types in Phase 5 but treated as such from Phase 0.

**`Model`** (`src/app/model.rs`, grown from `AppState`) — pure domain;
performs no unbounded/async-shaped blocking IO; serializable-ish.
Holds: listing, picks, inventory, marks, masks, filter, sort, view, cursor,
mode, project/session/dirs, search/captured-cmd, frecency, histories, config,
the **pure** resolver, keymap, quit flags, flash, `pending_*`, graveyard,
layout-intent (pane focus/height/zoom/hidden, pager positions), and the
input-gating `focus_chord_completed` (read via the `now` param). **New
fields**: `focus: Focus` (Phase 0) and a unified `git: GitState`. **Moves in**:
`harpoon` (currently misplaced on `App`). **Moves out**: `git_worker_tx` (→
Runtime).

> **PaneSnapshot** (the fix for the borrow-checker + yank/`gf` blocker): the
> Model holds `HashMap<SinkId, PaneSnapshot>` where `PaneSnapshot { visible_lines,
> scrollback_tail, pickable_text, is_scrolling, is_closed, cursor }` is
> populated by the Runtime on each `PaneOutput`/`PaneExited` tick (one
> bounded copy-out per wakeup). This makes "yank reads `model.pane_snapshot[id]`"
> buildable in pure `update()`, and lets `RouteSnapshot`'s `pane_scrolling`/
> `pane_closed` read from the Model. **render still reads the live vt100 grid
> through `&runtime`** — the full grid is *not* snapshotted (too large
> per-frame); the snapshot carries only the small derived facts.

**`Runtime`** (`src/app/runtime.rs`) — OS handles + threads + channels; never
serialized; never seen by `update()`. Owns: the `PtyHost` registry keyed by a
`SinkId` newtype (Model holds only ids + snapshots), the `notify::Watcher`, the
MCP listener + threads + rx, the git worker thread + **both reunited channel
ends**, finder/grep threads, the **parkable crossterm reader**, the timer
scheduler, and the `Tui` handle. render reads the live vt100 screens through a
shared borrow here.

**`ViewState`** (`src/app/view_state.rs`) — render ephemerals + caches: pager
group, overlay metadata, picker view state, scroll/history pending, tab state,
`needs_full_repaint`, theme, `cached_rows`/grid keys, activity/proc counters,
agent-status cache, context-write bookkeeping. **`last_grid` is eliminated** —
render computes layout in-frame.

**Borrow-checker contract** (verified against the review): `update()` takes
`&mut Model` (+ `&mut ViewState`, + `now: Instant`) and returns `Vec<Effect>` —
**not** `(Model, Vec<Effect>)`. Effects carry owned data, so the `Vec<Effect>`
outlives the `&mut` borrow and the Runtime executes them after the borrow ends.
`render` takes `(&model, &view, &runtime)` with `App` owning all three as
disjoint fields so a `&mut runtime` resize and `&model`/`&view` reads coexist.
The naïve "render never touches Runtime" idea is **dropped** — `render.rs`
demonstrably calls `tabs.active_mut().resize` and `with_screen` on the live
`PtyHost`.

---

## Message

One `enum Message`, grown variant-by-variant as each source migrates. Keys are
**not** pre-translated to semantic Messages (the chord suppressor reads
wall-clock `elapsed()`, so a pure `key → Message` fn is infeasible) — `route.rs`
stays the router and consumes `Message::Input(Event)` unchanged.

```rust
enum Message {
    Input(crossterm::event::Event),     // Phase 1; Press-filtered for Key only; Paste/Resize/Focus pass through
    FsEvent(notify::Event),             // Phase 3a; debounce in the Phase-2 timer layer
    PaneOutput { tab: SinkId },         // Phase 3b; WAKEUP via lost-wakeup-safe dirty-bit, never bytes
    PaneExited { tab: SinkId, status: ExitStatus },
    CaptureOutput, CaptureExit { status: ExitStatus },          // Phase 3c
    TaskOutput { id: SinkId }, TaskExited { id: SinkId, status: ExitStatus },
    GitResult(GitWorkerResult),         // Phase 3a; generation-drop preserved
    FindBatch { session: FinderId, matches: Vec<FindMatch> },   // Phase 3d
    GrepBatch { session: GrepId, matches: Vec<GrepMatch> },
    Mcp(McpRequest),                    // Phase 3d; owns its one-shot reply Sender
    Tick(Deadline),                     // Phase 2 (DONE); GitPoll/ActivityRollover/RefreshQuiet/ContextWrite/RestoreSettle/ResumeEnter. (Not ScrollThrottle — it's an in-arm event-gap dedup, not a wakeup timer.)
    ForegroundDone { on_done: PostWork },                       // DEFERRED past Phase 4 (the spawn after-work runs inline in run_effects's ForegroundExec arm); revisit in Phase 5. PostWork is a testable enum
    ClipboardResult(Result<usize, String>),
    ListingLoaded { dir: PathBuf, listing: Listing, gen: u64 }, // Phase 5, only if chdir is made async
}
```

The loop blocks on `recv_timeout(min(min_armed_deadline - now, pane_idle_floor))`.
Generation/id matching tolerates messages for closed/stale sessions exactly as
`drain_grep_session` already discards on id mismatch.

**Lost-wakeup-safe pane coalescing** (verified hazard): keep the monotonic
`AtomicU64 parser_gen` as source of truth. The worker bumps it **and**, only on
a `wake_pending` `AtomicBool` 0→1 CAS, sends one `PaneOutput{tab}`. The main
loop, on `PaneOutput{tab}`, **clears `wake_pending` first, then** calls
`drain_output()` (Acquire-loads the latest gen). Because clear precedes the
read, any bump racing the read re-arms and re-sends — at worst one redundant
wakeup, never a lost tail/final echo.

---

## Effect

Widen the existing `PostAction` into a `#[non_exhaustive] enum Effect` (the loop
already executes `PostAction` and only `PostAction` — this generalizes a proven
seam). Four classes by execution discipline:

- **(A) Fire-and-forget** (no TUI teardown, optional result Message):
  `SignalGroup{pgid,sig}` (rustix; scoped to signal-**delivery** `^C` only),
  `CopyToClipboard{text}` (the 3 pane-reading sites build text from
  `model.pane_snapshot[id]`), `SetTerminalTitle`, `WriteContext` (debounced,
  atomic tmp+rename; self-write filter survives in the subscription),
  `WriteSession`, `WriteMcpConfig`, `McpReply{reply_tx,resp}`,
  `RequestGitStatus`, `SyncWatch`, and the **new `SendToPane{sink,bytes}`** —
  the highest-frequency effect (resume injection, `^C`/`^\` forwarding to a
  capture child, paste-to-pane, chord forwarding). Without it, "update holds
  only `SinkId`s, Runtime owns `PtyHost`" is unsatisfiable.
- **(B) Blocking / TUI-tearing** (executor-only; the *only* effect that tears
  down the renderer): `ForegroundExec{argv,cwd,env,pause_after,on_done}` —
  today's `PostAction::Spawn` made complete. **Precondition**: the executor
  must **park the crossterm reader** across the takeover (reader + foreground
  child would otherwise both read the controlling-tty stdin and steal
  keystrokes). Sequence: park+**ack**+drain reader → `suspend_tui` → spawn FG
  process-group → `tcsetpgrp` → wait → restore → optional read-key →
  `resume_tui` → unpark → feed back `ForegroundDone`. This parking machinery
  is a **Phase-1** precondition. **Mechanism (resolved when Phase 1 shipped):
  NOT a self-pipe.** crossterm 0.28 has no public mid-read interrupt (the mio
  `Waker` is `event-stream`-only, not in our feature set), and a bare
  `event::read()` pins crossterm's process-global reader mutex via an
  infinite-timeout poll. So the reader loops on `event::poll(10ms)` (finite —
  uses `try_lock`, so a parked reader holds no lock and issues no tty read)
  and checks a park flag between polls; on park it drains crossterm's buffered
  events to empty (dropping them) before a synchronous, bounded ack. Park
  lands within ~one poll interval. (Done — `spawn_input_reader`/`ForegroundExec`
  in `src/app/mod.rs`.)
- **(C) Synchronous-ordered** (executor-only, blocking, run inline before the
  next Message): `WriteContextSync` for the MCP read-after-write contract — the
  MCP path emits an ordered `[WriteContextSync, McpReply]` so the atomic write
  completes before the reply travels down the Sender. Per-command audit decides
  reply-in-update vs reply-in-effect (`ChdirThenReply` for ops whose success is
  Runtime-known).
- **(D) Subscriptions** (long-lived; Runtime owns thread+handle; Model holds
  only ids): `SpawnPane`, `ShutdownPane` (SIGTERM-250ms-then-SIGKILL),
  `ResizePane`, `ReassignSink` (atomic remove+`take_host`+reinsert — `take_host`
  consumes the `Pane` by value, so demote-to-task is a worker-lifecycle swap),
  `StartFileWalk`, `StartGrep`, `StartMcpServer`.

`Runtime::run_effect(&mut self, e: Effect) -> Option<Message>`. Every effect
field is owned; `ForegroundExec` blocks the single thread (correct — the TUI is
torn down); subscriptions are `std::thread` workers pushing Messages. **No async.**

---

## Focus

```rust
enum Focus { FileList, Pane(SinkId), Overlay, Pager(Mount) }
```

One `focus: Focus` field on the Model. `route.rs`'s focus-axis inputs, the
render DIM cue, and signal-delivery routing **derive** from it. **`RouteSnapshot`
remains a projection** over `(focus, pager_mount, resolver_pending, is_prompting,
pane-flags)` — *not* over `focus` alone (it has 8 inputs; `Focus` subsumes ~3).

---

## View

Target: `ui::render(frame, &model, &view, &runtime)` — **zero clipboard/title/fs
IO** (the achievable purity goal), `&mut` only for ratatui `StatefulWidget`s.
render reads the live vt100 cells through the shared `&runtime` borrow exactly
as today (the grid is non-Clone and lock-guarded; there is no per-frame full
copy-out). Three smells die: `last_grid` (eliminated — layout computed
in-frame), the per-cell DIM cue (derives from `model.focus` for the focus axis),
and `update_term_title` firing from the draw branch (→ `SetTerminalTitle`
effect). `needs_draw`/`draw_reason` collapse into `update()`'s
`dirty: Option<RedrawReason>` return. The DEC 2026 synchronized-output wrap and
full-repaint clear stay as thin render-side glue.

---

## Phases

Each phase ships **independently behind green CI** (`make check` / `make lint`
/ `make test`, plus `make lint-linux` for OS-gated signal/clipboard paths) with
all **786 tests passing**, under the corrected invariant: **no test
assertion/expected-value edits; mechanical fixture/constructor churn for added
or relocated fields is permitted** (the absolutist "zero test edits" was
falsified against `test_state()` — an exhaustive `AppState` literal with no
`Default` tail).

All phases are **pre-2.0** (the road-to-2.0 MVU track). "When" below is the
*ordering within* that track, not a post-launch gate.

| # | Phase | When | Ships indep. |
|---|-------|------|--------------|
| **-1** | Re-baseline test fixtures for additive field growth | prereq | ✅ |
| **0** | **Focus as one Model value** (no loop change) | now (daily-driver fix, ahead of the rest) | ✅ |
| 1 | Single channel for **Input** + **parkable reader** + `ForegroundExec` rerouting | after test-harness de-risking | ✅ |
| 2 | Timer/deadline layer with **pane-presence floor** | road-to-2.0 | ✅ |
| 3 | Migrate each non-input source onto the channel (sub-phases 3a–3d) | road-to-2.0 | ✅ |
| 4 | Widen `PostAction` into the full **Effect** vocabulary; loop is sole executor | road-to-2.0 | ✅ |
| 5 | Physically split **Model/Runtime/ViewState** + de-IO audit (chdir fork) | road-to-2.0 | ✅ |
| 6 | **Pure-of-IO View** + command table; loop reaches ~100 lines | road-to-2.0 (last) | ✅ |

### Phase -1 — Re-baseline test fixtures (prereq)
`test_state()` and its derivatives become a single base + struct-update builder
(`AppState { focus: …, ..base() }`), mirroring `route.rs`'s `..idle()` pattern,
so future field additions touch **one** fixture line. No assertion changes.

### Phase 0 — Focus as one Model value (lands first)
Highest bug-leverage, lowest risk, zero loop change. **Spec first**: extend the
route.rs test matrix additively for coexisting-slot pager cases. Add `Focus` +
`focus` field; replace the ~10 copy-pasted `pane_focused = false` sites with one
transition point; reimplement `route_snapshot()` to project the focus-axis
inputs from `model.focus` (keeping `pane_scrolling`/`pane_closed`/`resolver_pending`/
`is_prompting` as explicit inputs); derive the render DIM cue from `focus`. Make
`pane_focused` a thin accessor so the compiler flags every writer.
**Done**: `grep 'self.state.pane_focused\s*='` shows zero matches outside the
single transition fn; focus dim / `^C` routing / paste target unchanged in smoke.

### Phase 1 — Single channel for Input + parkable reader (pre-2.0)
Introduce the one `mpsc::Receiver<Message>`; move crossterm input onto it via a
**parkable** reader thread (blocking `event::read`, Press-filtered for Key only),
**and** reroute the still-inline `run_child_in_foreground` through a
parking-aware minimal `ForegroundExec` executor (lands early because the
always-on reader otherwise races vim/less for stdin). Loop calls
`recv_timeout(poll_ms)` preserving the same adaptive cadence as the timeout.
**Done (hard)**: no keystroke leakage to/from a foreground `$EDITOR`/`$PAGER`
(round-trip smoke + park-gate test); reader forwards Paste/Resize, drops only
non-Press Key events. Revertable by deleting the reader + park gate + FG reroute
together.

### Phase 2 — Timer/deadline layer with pane floor (pre-2.0)
Replace `elapsed()`-vs-poll-cadence timers with `Message::Tick(Deadline)`; loop
blocks on `recv_timeout(min(next_deadline - now, pane_idle_floor))`. **Keep the
pane floor** (the 16/100/500 cadence as a floor when a pane/overlay/capture is
present) — deleting the idle-pane poll before Phase 3b would regress streaming
visibility. Thread `now` so timing logic is a pure fn of inputs.
**Done**: idle agent pane streams ≤100ms unchanged; idle CPU at 0 draws/sec
preserved; a **timing-equivalence harness** asserts a Message arriving 5ms into a
150ms debounce does *not* prematurely fire it.

### Phase 3 — Migrate each non-input source (sub-phases, one source per PR)
- **3a** fs watcher → `FsEvent`; git worker → `GitResult` (generation-drop kept).
- **3b** pane PTY: introduce `SinkId`; parser-worker emits coalesced
  `PaneOutput{tab}` via the lost-wakeup-safe clear-then-read dirty-bit (the
  consumer clears `wake_pending` in the pre-recv scan, NOT in `drain_output`,
  which render's `drain_all` also calls). Split **PR1 (add, both paths live)**
  / **PR2 (delete the pane component of the floor)** so the deletion is
  independently revertable. **`PaneSnapshot` is DEFERRED to Phase 5** (the
  borrow conflict it solves — render vs the yank/`gf`/quick-select text arms —
  doesn't exist in 3b: those arms run under exclusive `&mut self`, never
  concurrent with render's borrow, and an eager per-wakeup grid copy would
  regress the streaming path 3b protects; populate it lazily, yank-gated, when
  Phase 5 splits Model/Runtime). **PR2 deletes the *pane* floor only** — the
  16ms smooth-streaming floor narrows to cover `pending_capture` **and** a
  streaming `:task N` viewer of a running background task (both still polled
  until **3c**); `MAX_IDLE_CAP` stays (MCP/finder until 3d). Reader-death
  detection widens ~100ms→≤500ms after PR2 (fatal path, bounded — accepted).
- **3c** (DONE) capture/tasks → `SinkOutput` wake via a runtime-swappable
  `PtyHost` wake slot (the demote/promote/`:fg`/`^Z` swap needs a slot, not a
  spawn-time closure); exit observed via `newly_closed` on the main loop (the
  reader can't call `child.wait()` — `portable_pty` needs `&mut self`), so the
  on-channel exit message is deferred to Phase 4. PR2-of-3c deleted the pane
  floor; PR3 deleted the streaming floor.
- **3d** (DONE) MCP → `Mcp(McpRequest)` via a git-style forwarder (reply +
  synchronous `write_context` stay adjacent on the main loop = single-connection
  read-after-write); finder/grep → payloadless `FindOutput`/`GrepOutput` wakes
  via a `WakingSender` (data stays on the per-source channels — the literal
  `FindBatch/GrepBatch{matches}` payload is deferred to Phase 5's ReassignSink).
  Removed `MAX_IDLE_CAP`: the loop now blocks on `recv()` (reader-death via a
  `ReaderExited` death-wake + loop-top check; a 1Hz `CaptureTick` ticks the
  streaming elapsed-timer the cap used to).
**Phase 3 DONE**: every event source wakes the unified channel; the run loop is
fully event-driven (0 idle wakes when no deadline is armed). Each sub-phase was
spec'd via an adversarial workflow + shipped behavior-equivalent behind green
CI, one source per PR (3b/3c/3d split add-the-wake / delete-the-floor so the
floor backstopped every wake migration before its poll was removed).

### Phase 4 — Widen PostAction into Effect; loop is sole executor (pre-2.0)
Move every inline side effect into Effects returned by handlers. **Keep** the
three result enums (`ApplyResult`/`CommandResult`/`PromptResult`) — widen their
`Post(PostAction)` payload to `Post(Vec<Effect>)`; do **not** collapse the
signature here (that's Phase 5). Land `ForegroundExec` via a `From<PostAction>`
shim (call site byte-identical, parking executor already exists from Phase 1).
Add `SendToPane` and route the inline `send_bytes` sites through it. Convert the
5 clipboard sites, `kill_pg`/SIGSTOP/SIGCONT, and `update_term_title`.
Scope `^C`: only signal-**delivery** sites become `SignalGroup`; prompt-cancel/
buffer-clear/flash-hint stay pure transitions.

**Phase 4 DONE** (PRs #213–#216, each behavior-equivalent behind green CI +
a per-PR adversarial verification workflow): `run_effects` (in the new
`src/app/effect.rs`) is the **sole side-effect executor** for clipboard /
signal / send-to-pane / terminal-title. Vocabulary: `ForegroundExec` (via a
`From<PostAction> for Vec<Effect>` shim), `CopyToClipboard` + `ClipMsg`,
`#[cfg(unix)] SignalGroup` + `SigOk`, `SendToPane` + `PaneTarget` + `PaneInput`,
`SetTerminalTitle`. Only `ApplyResult::Post` was widened to `Post(Vec<Effect>)`
— the other two result enums and the `update`-signature collapse stay in
Phase 5. **Corrected Done metrics** (the original cross-file grep was vacuous —
those handler modules were already at 0 because the IO lived in `mod.rs`):
`clipboard::copy` now appears exactly **5×** repo-wide (1 in `run_effects` + 3
`pager.rs` footer yanks + 1 `yank_quick_select` — the latter four documented
inline exceptions, footer-routing / flash-ordering); `kill_pg(STOP/CONT)` for
pause/resume only in `run_effects`; `term_title::set` only in `run_effects`; no
`send_key`/`send_bytes` in the 4 converted send sites.
**Deliberately left inline** (loop-intrinsic — the producer *is* the loop, so
threading through a non-existent handler return buys nothing): the paste
bracketed-write, the resume-injection two-phase write (needs a `Tab`/`SinkId`
target — Phase 5), and the context-write debounce. **Re-scoped to later work**:
`interrupt_task`'s SIGINT stays inline (it flashes the *pager footer*, not the
status bar) and `write_context` stays inline (protects the MCP read-after-write
contract). The typed `ForegroundDone`/`PostWork` message was **not** added — the
spawn after-work runs inline in `run_effects`'s `ForegroundExec` arm (Phase 5).

### Phase 5 — Physically split Model/Runtime/ViewState + de-IO audit (pre-2.0)
Introduce `Runtime` and `Model`/`ViewState`; move `git_worker_tx` into Runtime
(channel reunited), `harpoon` into Model, eliminate `last_grid`. Provide
backward-compat accessors during the phase (same trick as Phase-0's
`pane_focused`). Collapse the three update entry pairs into one
`update(&mut Model, &mut ViewState, Message, now) -> Vec<Effect>`.
**chdir fork (explicit, not mechanical)**: chdir does `canonicalize` +
`Listing::read` consumed same-call by `rebuild_rows` from ~10 sites — pick and
document **(a)** model chdir as a *synchronous blocking effect* run inline before
the next Message (preferred — same-frame visibility, no async listing), or
**(b)** split into pure-decision + `LoadListing` effect + `ListingLoaded`
feedback, auditing every same-handler `self.listing` read. Soften the criterion
to "no *unbounded* blocking IO; bounded synchronous reads consumed in the same
transition may stay as a documented exception". **Done**: mark-jump / `..` /
`:cd` produce identical cursor+listing+git-markers on the first post-action
frame; one `git` value feeds top-bar + markers.

### Phase 6 — Pure-of-IO View + command table (pre-2.0)
Convert `render.rs` to `ui::render(frame, &model, &view, &runtime)` (zero
clipboard/title/fs IO; `&mut` only for `StatefulWidget`s; decide resize-in-render
vs a `ResizePane` effect). Replace the three-way `:command` punt-list with **one
command table** `{name, completion_visible, handler: PureDomain|TerminalTouching}`;
`SPYC_COMMANDS` derived from it. **Done**: adding an App-handled `:command` can no
longer flash "unknown command" (regression-tests the `:undo`/`:limit` footgun);
`App::run` is ~100 lines.

---

## Risks & mitigations

The plan survived four adversarial review lenses (Rust-feasibility, sync-only,
incrementalism, scope-honesty); all returned viable after these were folded in.

- **Pre-launch timing (the deliberate trade-off).** Landing deep loop/
  concurrency surgery before a public 2.0 launch risks subtle regressions
  (timing, focus, stdin) that behavior-equivalence tests don't fully catch, at
  the worst possible moment. → Accepted to avoid a post-launch refactor
  overhang. Mitigated by: every phase behavior-equivalent behind green CI +
  manual daily-driver smoke; the test harness (`docs/archive/TESTING_STRATEGY.md`)
  lands *before* Phases 1–6; Phase 0 (lowest-risk, highest-value) lands first
  and the single-channel loop rewrite lands last; each phase is independently
  revertable. If a phase destabilizes the daily drivers, it reverts in
  isolation without blocking the launch.
- **Borrow checker.** render + the yank/`gf` arms read the live `PtyHost`. →
  render takes `&runtime`; the 3 pane-reading arms source text from the Model
  `PaneSnapshot`. The "render never touches Runtime" claim is dropped.
- **Stdin contention.** The always-on reader + `ForegroundExec` would both read
  the tty. → Parkable reader with a synchronous bounded ack + buffer drain
  (finite-`poll(10ms)` + park-flag, **not** a self-pipe — crossterm 0.28 has no
  mid-read interrupt); parking is a Phase-1 precondition, not deferred. **Done
  in Phase 1.**
- **Pane-visibility regression.** The 100ms idle poll is load-bearing. → Phase 2
  keeps a `PANE_IDLE_FLOOR`; the floor + 16ms hack are deleted together only in
  3b-PR2 after the wakeup tests soak.
- **Lost-wakeup race (3b).** Edge-triggered dirty-bits can drop the final echo.
  → Monotonic `parser_gen` stays source of truth; clear-before-read protocol +
  deterministic test.
- **Missing `SendToPane`.** The keystroke-to-pane path is the highest-frequency
  effect. → Added to class A.
- **MCP read-after-write.** Fire-and-forget context writes could let the reply
  outrun the disk. → `WriteContextSync` in an ordered `[WriteContextSync, McpReply]`.
- **chdir de-IO is not mechanical.** → Explicit Phase-5 fork + chdir-equivalence
  test; "no blocking IO" softened to "no *unbounded* blocking IO".
- **Phase 4/5 signature ordering.** `Vec<Effect>` return would force collapsing
  the three enums early. → Phase 4 widens `Post(PostAction)` → `Post(Vec<Effect>)`;
  the single-`update()` collapse is deferred to Phase 5.
- **Test-edit invariant.** `test_state()` breaks on the first field add. →
  Phase -1 re-baselines fixtures; invariant corrected to "no assertion edits".
- **`ReassignSink` ownership.** `take_host` consumes the `Pane`. → Atomic
  remove+take+reinsert + worker-lifecycle swap; stale-drop covers the window.
- **Anti-monolith guard.** `mod_rs_stays_decomposed` (ceiling 8,500) — confirm it
  passes / update in the same commit per the doc contract after each phase.

---

## Open questions (decide before the relevant phase)

- **chdir de-IO fork** (Phase 5): commit to synchronous-effect (a) vs async
  `LoadListing` (b). Preserve the `huge_tree_anchor`/git-status-cache invariants.
- **PaneSnapshot sizing/cost**: confirm the per-tick `scrollback_tail` copy-out
  (yank-scrollback uses `recent_lines(10_000)`) isn't too costly; possibly
  populate scrollback lazily only when a yank-scrollback is pending.
- **recv_timeout precision** vs the timing-equivalence harness on macOS (the dev
  platform) — confirm before Phase 2.
- **McpReply per-command audit**: which commands reply-in-update vs reply-in-effect.
- **resize-in-render**: render holds `&mut runtime` disjoint, or resize moves to a
  `ResizePane` effect on Resize Messages (Phase 6).
- **Dropped (out of scope)**: a pure `handle_key(key, &Model) -> Option<Message>`
  (the chord suppressor reads wall-clock `elapsed()`); folding `^\` SIGQUIT / `^t`
  into Phase 4 (a behavior change, not a free byproduct).

---

## Done-criteria (whole migration)

- Sequencing: Phase 0 shipped first (as a daily-driver fix); Phases 1–6 landed
  pre-2.0, after the test-harness de-risking, each interleaved with the other
  road-to-2.0 tracks. 2.0 ships *on* the MVU foundation, not carrying it as
  overhang.
- All phases merged, each behind green CI, all 786 tests passing under the
  corrected invariant.
- `App::run` is a ~100-line `recv_timeout → update → run_effects → render` loop
  over one channel; no `event::poll`, no adaptive `poll_ms`, no 16ms hack, no
  100ms floor, no busy-poll drain.
- State is three types; `update(&mut Model, &mut ViewState, Message, now) ->
  Vec<Effect>` is the single Update entry; no Runtime handle reachable from
  `update`; no Model method does unbounded blocking IO.
- `Effect` is a `#[non_exhaustive]` enum (four classes) executed only by the
  Runtime; grep patterns confirm zero inline side-effects in the handler modules.
- `Focus` is one field driving the focus axis; `:command` is one table; render is
  `ui::render(frame, &model, &view, &runtime)` doing zero clipboard/title/fs IO.
- The recurring bug classes are structurally prevented **within their honest
  scope** (and the un-closed cases are documented, not over-claimed).

---

*Provenance: this plan was produced by a multi-agent design workflow
(understand → 3 competing designs → judge → synthesize → 4 adversarial
stress-test lenses → revise). The strangler-fig approach won the judge panel
(46/50) over effect-first (45) and model-first (40); the blocker fixes above
were surfaced by the Rust-feasibility, sync-only, and scope-honesty reviewers
and folded into this revision.*
