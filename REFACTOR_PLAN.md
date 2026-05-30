# Refactor plan — `app/mod.rs` decomposition

> **Status (2026-05-30): Phase 1 COMPLETE; Phase 2 next.** All six
> Phase 1 struct extractions have landed (PRs #180–#185); `app/mod.rs`
> is down to ~11.8k lines. Per the lean road-to-2.0 sequencing in
> `ROADMAP.md`, **Phase 2 is now the active decomposition track on the
> way to 2.0**; the **MVU rewrite (Phase 3) stays post-2.0**. Original
> "hold until after 2.0" reasoning is kept below as historical context
> — it applied to the *whole* plan; the decision now is to take the
> low-risk decomposition early and hold only the deep rewrite.

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
- **Phase 3**: multi-day commitment. **Don't start unless you can
  block out a full week of unbroken focus.** The code's shape will
  invert (state struct + reducer + effects), and partial conversions
  leave both styles coexisting which is worse than either alone.

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

## Phase 2 — Medium extractions

Once Phase 1 is done, the *shape* of what's left in `app/mod.rs`
becomes clearer and these become more obvious. Each is a
~half-day commit:

| # | Extract | Target file | LOC est | Notes |
|---|---------|-------------|---------|-------|
| 7 | `fn render()` + `compute_layout` + layout helpers | `src/app/render.rs` | ~600 | View half of MVU; biggest standalone win |
| 8 | `fn handle_pager_key()` + sub-handlers | `src/app/pager_handler.rs` | ~700 | Key dispatch surface — design the interface to App carefully |
| 9 | `dispatch_command()` + `:` arms | `src/app/commands.rs` | ~500 | Move the colon-command dispatch table out |
| 10 | `fn handle_key()` top-level + capture / pane / picker routing | `src/app/key_dispatch.rs` | ~400 | Skim what's left and decide if this is worth pulling out |

Cumulative: ~2200 more LOC off `app/mod.rs` → ~4200. At that point
`app/mod.rs` is *just* the App struct, the run loop, and the
`apply(action)` shim — close to the natural MVU shape.

### Phase 2 done-criteria

- `wc -l src/app/mod.rs` ≤ 4500
- `app/mod.rs` no longer contains any rendering code or any pager-key
  handling. Only the run loop, the App struct, and small glue.
- Tests still pass; clippy clean.

---

## Phase 3 — MVU rewrite

The architectural change. **Don't start until Phase 2 is done and
the project has stabilized post-2.0.** Multi-day work; don't
context-switch.

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
  (`docs/V1_70_PLAN.md`) — can't split a 12k-line monolith. Phase 3
  (MVU) still held until 2.0 has shipped + stabilized ~2 weeks.
- **2026-05-30**: Phase 1 complete (PRs #180–#185). Six struct
  extractions, one PR each, no behavior change, no test edits, gate
  green throughout. Scoped to *data structs* — App-coupled `&mut self`
  methods stayed put (they're Phase 2 handler work; pulling them now
  would violate the one-way-dependency rule). Net ~700 LOC off
  `app/mod.rs` (11,757 now). Phase 2 (render / pager-key / command /
  key-dispatch handlers) is the next track; each is ~half-day and
  needs real interface design, so not "pick up any spare moment" work.
