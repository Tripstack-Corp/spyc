# spyc Architecture

This document captures the **stable architectural decisions** behind
spyc: the choices that should not drift without deliberate revisit.
For a per-module file index, see `AGENTS.md`. For forward plans, see
`ROADMAP.md`.

## Concurrency model: sync-only, `std::thread` + `mpsc`

**spyc does not use an async runtime.** No `tokio`, no `async-std`,
no `futures` crate. Concurrency is `std::thread::spawn` + `std::sync::mpsc`.

Why:
- The TUI is fundamentally a single event-driven loop with a few
  long-lived I/O sources (file watcher, pane PTY readers, MCP socket
  listener, optional shell-capture readers). Each is naturally a
  thread that pushes into a channel. There is no fan-out workload
  that would benefit from a task scheduler.
- An async runtime would force every blocking-stdlib call site
  (file reads, `proc_pidinfo` via the safe `libproc` crate, `lsof`)
  to be re-plumbed or wrapped with `spawn_blocking` indirection —
  pure cost, zero benefit at our scale.
- Build times and binary size matter for a CLI. tokio is large.
- Cancellation we need (background directory loads, etc.) is well
  served by a generation counter on the receiver side; we drop
  stale messages instead of cancelling workers.

Where threads exist today:
- File watcher (`notify`) — pushes change events into the main loop.
- Per-pane PTY reader threads — push bytes from the master into a
  per-pane channel.
- MCP socket listener — accepts stdio-proxy connections.
- `!` shell-capture reader thread — feeds bytes into the pager
  while the captured child runs.
- Git worker — a long-lived thread (spawned in `app/bootstrap.rs`)
  that runs gix status/branch work off the loop and pushes
  `GitWorkerResult` messages back.
- Agent-status worker — a short-lived thread per refresh
  (`app/agent_status.rs`) that resolves the pane agent's short-id
  off the loop and wakes it on completion.
- `F`-finder walker — a gitignore-aware streaming directory walk
  (`fs/finder.rs`) feeding the fuzzy filename picker.
- **Pager-stream workers** (`app/pager_stream.rs`) — the unified seam
  for "read/parse off the UI thread, stream styled lines into a pager."
  A worker resolves / reads / renders and pushes payloads through a
  `fs::WakingSender` (waking the loop with a payloadless
  `Message::PagerStreamOutput`); the main loop's `drain_pager_stream`
  id-gates the live pager via its `stream_id` and applies the result
  through the object-safe `PagerStream` trait (`DrainOutcome`). Stale
  output self-discards on the id mismatch — the generation-counter
  cancellation pattern above, specialized for pagers.

**Off-thread read/parse is the default architecture** for any feature
that fills a pager from disk or compute — it does not block the
keypress path. A 4 MB agent-transcript tail-read + JSON parse, a
streaming ripgrep search, and a gix diff/show/blame model all ride this
one `pager_stream` seam — the bespoke `grep_session` / `git_view_session`
skeletons collapsed onto `GrepStream` / `GitViewStream`, sharing the
single `stream_id` / `Message::PagerStreamOutput`. Adding a new such
feature = a `produce` closure (the worker body) + a small `PagerStream`
impl (the apply step); the channel, wake, id-gating, and mounting are
shared.

Future work (background directory loading, etc.) will follow the
same pattern: spawn a worker, push a typed message into a channel,
drop stale messages by generation. See `ROADMAP.md`'s "Background
directory loading" entry.

## Update model: Elm-architecture (MVU)

spyc follows the Elm/Model-View-Update pattern. The structural migration
**and** the last-mile purity pass have landed (decision logs in
`docs/archive/REFACTOR_PLAN.md` / `docs/archive/MVU_PLAN.md`). The shape today:

- **Three-type state split.** `App` owns three disjoint fields
  (`src/app/mod.rs`): `state: AppState` (the **Model** — pure domain:
  listing, cursor, picks, marks, filter, mode, config, `focus`, git display
  state; holds no OS handles), `runtime: Runtime` (OS handles + channels +
  worker endpoints + the `PtyHost` registry — never seen by domain logic),
  and `view: ViewState` (render ephemerals + caches: pager group, overlay
  metadata, dirty flags, theme, cached rows / grid keys).
- **Single message channel.** One `mpsc::Receiver<Message>` feeds the loop.
  A parkable crossterm reader, the `notify` watcher, the per-pane parser
  workers, capture / task readers, the MCP forwarder, the git worker, and
  finder / grep all push `Message` variants into the same receiver. `App::run`
  is **event-driven**: it blocks on `recv` / `recv_timeout` (0 wakes at idle
  when no deadline is armed) — there is no `event::poll`, no adaptive
  busy-poll. Timers are `Message::Tick(Deadline)`s armed against a scheduler.
- **Update.** All input funnels through a single `App::update(msg)` entry
  (`src/app/update.rs`). The pure-domain transitions it dispatches to —
  `AppState::apply` and the siblings `dispatch_command` / `dispatch_prompt` —
  take the Model, do no terminal access, are unit-testable without a TUI, and
  return effects as data.
- **Effects.** Side effects are a `#[non_exhaustive] enum Effect`
  (`src/app/effect.rs`) — `ForegroundExec`, `CopyToClipboard`, `SignalGroup`,
  `SendToPane`, `SetTerminalTitle`, `ReadPaneText`, `ChangeDir`. `run_effects`
  is the **sole** executor; handlers return `Vec<Effect>` and never touch the
  OS directly. This makes "forgot to clear `pending_X`" and inline-IO bug
  classes structurally hard.
- **View.** Rendering lives in `src/app/render/`. The draw pass is
  **mutation-free** (`&self`): any pre-frame state settling happens in
  `prepare_frame` *before* the draw, and the output is pinned by a ratatui
  `TestBackend` + `insta` snapshot net. It reads the Model / ViewState and the
  live vt100 grids through a shared `&runtime` borrow.

The last-mile purity pass is **done**: the single `App::update` entry above
(the former `ApplyResult` / `CommandResult` / `PromptResult` split collapsed into
one `Update`), the mutation-free render behind snapshots, the `:command` surface
compile-checked via `COMMAND_TABLE` handler fn-pointers, and a one-way
`app → agent` dependency. See `docs/archive/MVU_PLAN.md` for the decision logs.

## Repaint strategy: event-driven, dirty-frame

Goal: 0 draws-per-second at idle. Implementation:

- A `needs_draw` flag with reason codes (`pane=1`, `event=2`,
  `other=3`) — set by handlers that change visible state, cleared
  after the frame.
- `needs_full_repaint` for teardown transitions (pager close, overlay
  close) where partial damage can leave artifacts.
- Per-frame: DEC 2026 synchronized output (`\x1b[?2026h…l`) wraps
  every render so terminals that support it (iTerm2, kitty, WezTerm,
  Alacritty current) draw atomically — no flicker.
- Caching: `build_rows()` and grid stabilization keyed by a
  `list_generation` counter that increments on any listing /
  cursor / pick / mask change.

The activity overlay (`A` toggle) reports dps and bytes/sec for
ongoing tuning.

## Process & TTY ownership

- The TUI runs in raw mode + alt screen. `setup_terminal`
  (`src/main.rs`) enables raw mode, alt screen, bracketed paste,
  DEC 1007 alternate scroll, and hides the mouse pointer.
- For child processes that need the real tty (`$EDITOR`,
  interactive `$SHELL`, etc.), `suspend_tui` clears the alt screen
  and disables raw mode, then re-`enable`s after the child returns
  via `resume_tui`. Critically, `suspend_tui` does **not**
  `LeaveAlternateScreen` — that would flash main-buffer content;
  the child's own `smcup` reuses our blanked alt buffer.
- Pane subprocesses run under their own slave PTY (allocated via
  `portable_pty`). The pane is a vt100-emulated rectangle inside
  spyc's TUI; the child has a real tty, ours is unaffected.
- `!` captured commands also use a slave PTY now (since v1.12.0),
  so programs that open `/dev/tty` for prompts (sudo, ssh, gpg)
  flow through the master into the pager instead of bleeding onto
  spyc's screen. Typed keys are forwarded to the child via the
  master writer while the capture is live.
- **Background tasks** (since v1.20.0) reuse the captured-shell
  plumbing exactly: `^Z` from a streaming `!` pager moves the
  `(child, writer, output_rx, buffer)` tuple from `App.pending_capture`
  into a `BackgroundTasks` collection on `App`. The reader thread
  spawned by `spawn_capture` is unchanged — it keeps draining into
  the per-task buffer regardless of whether the pager is attached.
  No new threads. `:fg` reverses the move; the task viewer (`gB` /
  `[t]t`) reads the buffer non-destructively for live peek. Buffer
  is head-truncated at 1 MB (the tail of a `cargo build` is what
  the user wants).

## Git: 100% in-process gix

Production git is entirely in-process via `gix` (gitoxide) — status,
diff/show/blame models, worktrees, discovery. **No `git` subprocess
in production code**, and that's enforced: the
`no_subprocess_git_in_production` guard test in `src/git/mod.rs`
asserts zero `git`-binary spawns outside test fixtures. `src/git/` is
the single boundary owning every git operation (pure infra: paths in,
owned `Send` data out — no `App`, no ratatui); heavier model builds
run off-thread via the `pager_stream` seam.

**Hot-path rule:** the 1 Hz git mtime poll reads the cached
`current_gitdir` — no gix repo open on the poll. gix opens only on
chdir into a new repo and on HEAD change.

## State persistence (XDG)

All persistent state lives under XDG paths (`$XDG_STATE_HOME` or
`~/.local/state/spyc/`):

- `inventory/` — file-backed yank cache; one `<uuid>.json`
  (metadata) + `<uuid>.dat` (content) pair per entry.
- `graveyard/` — soft-delete cache; `<uuid>.json` +
  `<uuid>.tar.zst` pairs, FIFO-pruned at 500 MB.
- `harpoon/` — per-project pinned lists (`<basename>.<hash>.toml`).
- `marks.toml` — vi-style `m{a-z}` marks.
- `history` / `pane_history` — plain-text prompt history files
  (one entry per line).
- `frecency.json` — directory frecency scores for the `J` jump.
- `pager_positions.json` — persisted pager scroll offsets (LRU).
- `sessions/<epoch-ms>.json` — workspace snapshots from quit.
- `mcp-<pid>.sock` — PID-scoped MCP socket.

The debug log is the exception: `spyc_debug!` output goes to
`/tmp/spyc-debug-<ts>.log` (timestamped per run, not under XDG) so
a log can be attached to a bug report without digging in state dirs.

Configuration lives at `~/.spycrc.toml` (user) and
`<cwd>/.spycrc.toml` (project, wins). Both are watched for live
reload. `spyc --print-config` emits a fully-commented default
template suitable for `>` redirect.

Startup runs a health check that validates inventory / marks /
sessions / graveyard, cleans up orphaned files, and warns on
corrupt JSON.

## MCP server

`src/mcp/` runs a JSON-RPC server on a PID-scoped Unix domain
socket so multiple spyc instances coexist. Two transports share
the same dispatch:

- **`spyc --mcp`** (stdio) — what Claude Code spawns. Proxies to
  the live spyc instance via the socket. Falls back to read-only
  direct mode if no live instance is reachable.
- **In-process socket listener** — the running spyc accepts
  connections from the stdio proxy.

`.mcp.json` carries `SPYC_MCP_SOCK` in the `env` block so the
proxy connects to the right instance. On startup, if another live
spyc owns the entry, spyc prompts on stderr before taking over
(`PID N already owns MCP here. Take over? [Y/n]`); decline keeps
the old instance in charge. Non-tty stdin auto-takes-over so CI
isn't blocked.

Enterprise managed-settings.json policies
(`deniedMcpServers`/`allowedMcpServers`) are honored.

## Documentation contract

Architecture decisions land in:

- **This file** — stable principles. Edit when a *decision* changes,
  not on every feature.
- **`AGENTS.md`** — slim, always loaded into Claude's context.
  Module index, conventions, "what spyc does" summary, MCP usage
  hints. Don't grow it past what's worth paying context tokens for.
- **`ROADMAP.md`** — forward plan; move shipped items to Done.
- **`CHANGELOG.md`** — release notes (Keep-a-Changelog).
- **`BUGS.md`** — open + fixed bugs. Move from open buckets to
  FIXED on commit.
- **`FEATURES.md`** — user-facing feature reference.
- **`README.md`** — landing page, install, positioning.
- **`src/ui/help.rs`** — in-app `?` help; user-visible keybindings.

When a commit changes user-visible behavior, update every doc
that's affected in the same commit — not as a follow-up.
