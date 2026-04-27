# spyc Architecture

This document captures the **stable architectural decisions** behind
spyc: the choices that should not drift without deliberate revisit.
For a per-module file index, see `CLAUDE.md`. For forward plans, see
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
  (file reads, libc `proc_pidinfo`, `lsof`, git shell-outs) to be
  re-plumbed or wrapped with `spawn_blocking` indirection — pure
  cost, zero benefit at our scale.
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

Future work (background directory loading, etc.) will follow the
same pattern: spawn a worker, push a typed message into a channel,
drop stale messages by generation. See `ROADMAP.md`'s "Background
directory loading" entry.

## Update model: Elm-architecture, in progress

The Update half of MVU is essentially done: `AppState::apply(action)`
returns an `ApplyResult` enum (`Handled`, `OpenPager`, `Post(PostAction)`)
with no terminal access. State transitions are pure-ish and unit-
testable without a TUI.

The View and event-loop halves are still fused into `app/mod.rs`.
Target shape:

1. **View** — pure functions in `src/ui/` taking `&AppState`. The
   render path collapses to `ui::render(terminal, &state)`. Snapshot
   tests extend mechanically.
2. **Single message channel** — one `mpsc::Receiver<Message>` for
   the loop. The crossterm event reader, file watcher, pane capture
   readers, MCP thread, and timer ticks all push `Message` variants
   into the same receiver. The loop blocks on `recv` instead of
   open-coding `event::poll` with manual timeout math.
3. **`App::run`** reduces to ~100 lines: `loop { recv → update →
   render }`.

Done incrementally alongside feature work — not a standalone rewrite.

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

## State persistence (XDG)

All persistent state lives under XDG paths (`$XDG_STATE_HOME` or
`~/.local/state/spyc/`):

- `inventory.json` — file-backed yank cache + graveyard.
- `marks.json` — vi-style `m{a-z}` marks.
- `history.json` — prompt history (shell + spyc commands).
- `sessions/<epoch-ms>.json` — workspace snapshots from quit.
- `mcp-<pid>.sock` — PID-scoped MCP socket.
- `debug.log` — `spyc_debug!` output when `--debug` is set.

Configuration lives at `~/.spycrc.toml` (user) and
`<cwd>/.spycrc.toml` (project, wins). Both are watched for live
reload. `spyc --print-config` emits a fully-commented default
template suitable for `>` redirect.

Startup runs a health check that validates inventory / marks /
sessions / graveyard, cleans up orphaned files, and warns on
corrupt JSON.

## MCP server

`src/mcp.rs` runs a JSON-RPC server on a PID-scoped Unix domain
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
- **`CLAUDE.md`** — slim, always loaded into Claude's context.
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
