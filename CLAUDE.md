# CLAUDE.md — spyc architectural contract

spyc is a vi-keyboard-driven terminal file / process / agent-pane manager
(Rust, ratatui + crossterm). This file is the **architectural contract**: the
goals to design toward and the rules that keep the codebase in good shape.

- For the per-module map and day-to-day conventions → **[`AGENTS.md`](AGENTS.md)**.
- For the deep, stable design decisions (concurrency, persistence, MCP, the MVU
  mechanics) → **[`ARCHITECTURE.md`](ARCHITECTURE.md)**.

This file states the *why* and the *invariants*; it should not duplicate the
module index. Keep it short.

## The architecture we're committed to (MVU / Elm)

spyc is Model-View-Update. These invariants are what make the system
reason-about-able — preserve them; don't quietly erode them:

- **Three disjoint state types on `App`.** `state: AppState` is the **Model** —
  pure domain (listing, cursor, picks, marks, filter, mode, focus, git display);
  it holds *no* OS handles. `runtime: Runtime` holds OS handles / channels /
  `PtyHost`s and is never seen by domain logic. `view: ViewState` holds render
  ephemerals + caches. Don't smuggle OS handles into the Model, or domain state
  into Runtime.
- **One update entry.** User input flows through a single `App::update(msg)`.
  The pure transitions (`AppState::apply` / `dispatch_command` /
  `dispatch_prompt`) take the Model and **return effects as data** — no
  terminal/OS access, unit-testable without a TUI.
- **Effects are data; `run_effects` is the only executor.** Side effects are
  `Effect` variants (`src/app/effect.rs`); handlers return `Vec<Effect>` and
  never touch the OS directly. This is what makes "forgot to clear `pending_X`"
  and inline-IO bug classes structurally hard. Need a side effect? Add an
  `Effect` — don't reach for the OS inside a handler or the render pass.
- **Render is pure (`&self`).** The draw pass reads Model / ViewState / live
  grids and mutates nothing; any pre-frame state settling happens in
  `prepare_frame` *before* the draw. It's covered by a `TestBackend` + `insta`
  snapshot net — keep both true.
- **One message channel, event-driven.** Every source (input reader, `notify`
  watcher, pane parsers, capture/task readers, MCP, git worker, finder/grep)
  pushes `Message`s into one `mpsc::Receiver`; the loop blocks on `recv`
  (0 wakes at idle). Don't reintroduce `event::poll` / busy-polling.
- **Dependency direction is one-way.** `app` → `agent` profiles, never the
  reverse; the Model never depends on the `App` aggregate. Inside `src/app/`,
  child modules read `App`'s private fields via the descendant-module rule, so
  fields stay private — only the handful of cross-module entry points are `pub`.

## Keeping a good shape (maintenance rules)

- **No `.rs` over ~800 lines without a solid reason.** Oversized files make
  diffs impossible to reason about. When a file grows, extract a cohesive
  child/sibling module (verbatim relocation, behavior-identical) rather than
  letting it sprawl. A module root holding its own core *type definitions* is a
  legitimate "solid reason"; a pile of helpers is not. (`app/mod.rs` has a
  ceiling-guard test — extract a module if you hit it, don't bump the number.)
- **`app/mod.rs` is the module root, not a junk drawer.** It holds the core type
  defs (`App` / `Runtime` / `ViewState` / `Message`) and a little glue; `run`
  lives in `run.rs`, `App::new` in `bootstrap.rs`, leaf helpers in `util.rs`,
  process I/O in `proc.rs`. New render / key / command / action logic goes in
  the matching child module (or a new one) — never appended to `mod.rs`.
- **Glue stays with its types; leaves move out.** A helper that builds the
  module's own types (e.g. an `Effect` or `RowData`) is glue — keep it near
  them. A leaf helper with no `App` dependency (time/byte/text formatting, a
  path/host string, a subprocess shell-out) belongs in a `util`-style module.
- **`:command`s are compile-checked.** Every `:`-command is a `COMMAND_TABLE`
  entry (`src/app/command_table.rs`) carrying its handler; a registered command
  with no handler is a **build error**, not a runtime "unknown command". Add the
  entry and its handler together.
- **New behavior = an `Action`.** A user-observable feature gets an `Action`
  variant, a keymap binding, and a handler in the right `src/app/` module (or
  the pure half in `AppState::apply`) — not a special case wired into `mod.rs`.
- **Pure decisions get extracted and tested.** Branchy decisions (key routing,
  focus selection) become a `Copy` snapshot + a pure `fn` + unit tests (the
  `route.rs` / `focus.rs` template) instead of inline guards buried in a method.
- **Refactors are behavior-preserving.** Relocations don't edit test assertions;
  the full gate (`make check` / `make lint` / `make test`, plus `make
  lint-linux` for OS-gated code) stays green on every change.

## Docs

Update every affected doc *in the same commit* as the change — never as a
follow-up. The full list lives in AGENTS.md ("Keep docs in sync") and
ARCHITECTURE.md ("Documentation contract").
