# Refactor plan — `app/mod.rs` decomposition

Working doc for the staged decomposition of `app/mod.rs` (currently
~7400 lines, ~120 fns) into smaller, more reviewable units. ROADMAP
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

## Phase 1 — Cheap mechanical extractions

Self-contained types + impls that landed in `app/mod.rs` by inertia,
not coupling. Each is a verbatim move + a `pub mod ...; use ...::*;`
import — no behavior change, no API change. Each is one commit.

Order doesn't matter; pick the one whose itch is loudest the day
you sit down.

| # | Extract | Target file | LOC est | Risk |
|---|---------|-------------|---------|------|
| 1 | `BackgroundTasks` + `BackgroundTask` + `TaskStatus` + helpers | `src/app/tasks.rs` | ~250 | trivial |
| 2 | `PagerHistory` + `MAX_PAGER_HISTORY` | `src/app/pager_history.rs` | ~80 | trivial |
| 3 | `FindPicker` + `drain_walk` | `src/app/find_picker.rs` | ~150 | trivial |
| 4 | `GrepSession` + drain logic | `src/app/grep_session.rs` | ~150 | trivial |
| 5 | `Prompt` + `PromptKind` + `simple()`/`shell()` ctors | `src/app/prompt.rs` | ~150 | trivial |
| 6 | `PendingCapture` + `spawn_capture` + `strip_crlf` | `src/app/capture.rs` | ~250 | low — touches the live capture path; verify a `!cargo build` round-trips after the move |

Cumulative: ~1000 LOC off `app/mod.rs` → ~6400. Each item shippable
on its own; failure mode is "compile error caught immediately,"
nothing user-visible.

### Phase 1 done-criteria

- `wc -l src/app/mod.rs` ≤ 6500
- Every extracted module compiles standalone with no imports back
  into `app/mod.rs` (one-way dependency only).
- All 470 tests still pass.
- `cargo clippy --all-targets` clean.

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
