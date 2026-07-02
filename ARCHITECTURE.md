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
- File watcher (`notify`) — pushes change events into the main loop. A
  dedicated watch-control worker (`app/watch.rs`) owns the
  `RecommendedWatcher` and applies (un)watch commands off the loop, because
  notify's recursive `watch()` does a blocking per-subdir `inotify_add_watch`
  walk on Linux.
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

## No live cursor reads (the SSH rule)

<!-- SPYC-TRAP: cursor-read-ssh -->
Nothing on a live, post-startup code path may read the cursor position
(`ESC[6n` / `get_cursor_position()`). The reply can exceed crossterm's
~2 s timeout over SSH, *and* the unparked input-reader thread races to
read the same bytes off stdin — either way the read fails and tears the
whole session down. This bit us through ratatui 0.30's `Terminal::clear()`
(closing a pager / any `needs_full_repaint` over SSH crashed the session,
#444); `force_full_repaint` (`src/lib.rs`) is the cursor-read-free
replacement — `Terminal::resize()` to the current size has the same
clear-and-full-repaint effect but takes the no-cursor-read branch. Any
detection that *does* need a probe (the graphics-protocol query feeding
`detect_image_picker`) runs **once at startup, before the input reader
spawns**, so nothing races it. Fixed 1.58.8.

## Mermaid / image rendering

The pager renders ` ```mermaid ` blocks as real images, all pure-Rust
(no Node/Chromium/C deps): `mermaid-rs-renderer` (mermaid → SVG) →
`resvg` (SVG → raster) → `ratatui-image` (terminal graphics:
Kitty/iTerm2/Sixel/halfblocks). The render is far too heavy for the
loop (parse → layout → SVG → raster → font load), so it runs on a
detached worker like the graveyard ops (`src/app/mermaid_ops.rs`):
`Effect::RenderMermaid` → worker → `runtime.mermaid_results` →
`Message::MermaidDone` → `apply_mermaid_outcomes` (pre-recv drain)
installs a `ViewState.image_view: Option<ImageView>` overlay or opens
the PNG in the OS viewer. Two modes: `Open` (`o`, temp PNG +
`open::that_detached` — any local terminal) and `View` (`i`, a
terminal-sized `Protocol` for a full-screen in-spyc overlay — graphics
terminals only; the `Picker` is detected once at startup and `None`
disables the in-terminal path). The overlay is modal with its own
verbs (save / copy-image / copy-source / light-dark / base64); image
copy uses `arboard` (spyc's text clipboard stays shell-based).

Graphics gotchas, each of which cost real debugging time:

<!-- SPYC-TRAP: iterm-osc1337 -->
- **iTerm2 (3.5+) answers the Kitty graphics probe**, so
  `Picker::from_query_stdio` detects it as Kitty — but only iTerm2's
  *native* OSC 1337 actually paints. `detect_image_picker` forces the
  Iterm2 protocol when `TERM_PROGRAM`/`LC_TERMINAL` says iTerm. Detect
  once at startup, before the input reader spawns (the cursor-read SSH
  rule).
- **DEC 2026 synchronized update swallows inline-image escapes** —
  iTerm2 drops OSC 1337 emitted inside `\x1b[?2026h…l`. The per-frame
  sync wrap (above) is therefore skipped while `image_view.is_some()`.
- **`ratatui-image` renders nothing if the protocol is larger than the
  draw area** (silent), and **`Resize::Fit` only downscales** — so the
  vector SVG is rasterized at the terminal's pixel size and centered,
  rather than fitting a small natural raster into a large area.
- **Footer-only overlay verbs must not `needs_full_repaint`** — a full
  repaint clears the screen and re-blits the image (a visible flash);
  the input-arm diff draw updates just the footer cells and leaves the
  image untouched. Only verbs that change/remove the image repaint.

## Vertical (left/right) split

`^a |` opens a second column on the right hosting a **live-reloading
preview** of the cursor file. The split's *shape* is pure Model
(`AppState.vsplit: Option<VSplit { width_pct, mode: TopOnly | FullHeight,
focus: Side }>`); the preview's *content* is a `ViewState.right_pager:
Option<PagerView>` slot (`Mount::RightPane`), parallel to the top/scrollback
pagers; the reload's *worker* lives in `Runtime`. The three-disjoint-state
split holds — nothing about the split leaks an OS handle into the Model.

**Geometry is a pure post-pass.** `compute_layout` builds the single-column
frame unchanged; when a split is open, `carve_vsplit` (unit-tested, no TUI)
splits it into `left | vdivider | right`. *TopOnly* carves only the file-list
region (the PTY pane stays full-width below both columns); *FullHeight* runs the
divider the whole height and clamps the left chrome — including the pane — to the
left width, so the pane sits under the left column only. Both modes narrow
`top_unit` to the left column, which scopes the `V`/`;cmd` editor overlay to its
column for free. Zoom takes precedence: the carve is skipped while
`pane.zoom != None`. `vsplit_column_widths` (clamped to `[20,80]`, floored at a
20-col minimum, `None` when too narrow) is the single source of truth for both
the carve geometry and the markdown wrap width, so they can't drift.

**Focus is two axes, no `Focus` explosion.** `Focus::FileList` still means "the
commander region owns input"; the left/right axis rides `VSplit.focus: Side`.
`route_input` gains one bit — `right_column_focused` — that sends non-meta keys
to `right_pager` while meta chords (`^a …`) still escape to the resolver. The two
columns are addressed `a` (left) / `b` (right): **letters for file panes, numbers
for PTY tabs.**

**Live reload is off-thread** (`app/preview_ops.rs`), the graveyard/mermaid
pattern. The fs-event ingest matches the previewed file (`config::is_preview_path`,
exempt from the gitignore drop) and `kick_preview_reload` spawns a detached worker
that re-runs the pure `pager_handler::build_pager_view` (markdown render + syntect
— far too heavy for the loop) → `runtime.preview_results` → `Message::PreviewReloadDone`
→ `apply_preview_reloads` (pre-recv drain) installs the rebuilt view, preserving
scroll. The watch worker (`watch.rs`) adds the preview's *parent* dir
non-recursively — a file-level watch follows the old inode through an editor's
atomic rename and goes deaf — skipped when that parent already lies under the
recursive listing watch. A terminal resize re-kicks to re-wrap at the new width;
an in-flight guard plus a `preview_dirty` flag collapse a save/resize burst to a
single trailing re-render. A deleted preview file flashes in the footer and keeps
the last-good render.

Stage 1 shipped the preview-on-the-right; Stage 2 — a second *full
file-commander* on the right — has since landed: a second cwd + per-column git
(each `Commander` owns its `GitState`/`GitCache`), per-column tools
(grep/find/MCP-search/harpoon follow the focused column's worktree via
`tool_root`/`harpoon_root`), focus-aware MCP context, and **dual fs-watch** —
the watch worker (`watch.rs`) re-points a second recursive-tree + non-recursive
gitdir watch onto column `b`, and the fs-event predicates (`config::
is_listing_path` / `is_gitdir_status_path`) accept either column, so `b`'s
markers refresh on edits, not just the 1 Hz poll. Session restore reopens both
columns: the saved split persists `b`'s cwd (`SavedVsplit::right_cwd`) and `-r`
reopens the second commander there.

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

> Before reworking this refresh/cache machinery, read
> `docs/archive/VSCODE_GIT_STUDY.md` — a comparative study of VSCode's git
> extension (no freshness cache, purely event-driven, re-walk-and-replace
> + a changed-file cap). It's left as a *consideration* that strengthens
> the deferred git-status-owner-thread consolidation, not a committed
> plan.

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
- `mcp-<pid>.root` — trusted-root sidecar: the directory that spyc
  is rooted at. Stdio discovery cross-checks a project's
  `.spyc-context-<pid>.json` marker against this (owner-private,
  attacker can't forge it) so a planted marker can't redirect MCP
  attachment to another project. Written at socket start, removed on
  cleanup.

The debug log is the exception: `spyc_debug!` output goes to
`/tmp/spyc-debug-<ts>.log` (timestamped per run, not under XDG) so
a log can be attached to a bug report without digging in state dirs.

Configuration lives at `~/.spycrc.toml` (user) and
`<cwd>/.spycrc.toml` (project, wins). Both are watched for live
reload. `spyc --print-config` emits a fully-commented default
template suitable for `>` redirect.

The project file is **untrusted** (spyc is routinely pointed at
hostile content): its cosmetic/behavioral settings and plain
rebindings are honored, but *executing* keymap bindings (`unix`
shell commands, `jump`) are dropped — those take effect only from
`~/.spycrc.toml`. `^R` reload re-reads the project file from the
**startup** cwd, never the browsed directory, so browsing into a
hostile tree can't load its rc.

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

`.mcp.json` (claude) and `.codex/config.toml` (codex) carry
`SPYC_MCP_SOCK` in the `env` block so the proxy connects to the
right instance. spyc writes the client config **when an agent pane
launches** (`open_pane_tab_in` → `ensure_agent_mcp_config`), not at
startup — so a directory where no agent is ever run doesn't get a
stray `.mcp.json` / `.codex/` written into it. On startup, if another
live spyc already owns the entry, spyc prompts on stderr (`PID N
already owns MCP here. Take over? [Y/n]`); decline keeps the old
instance as MCP owner and the launch-time write leaves its entry in
place. Non-tty stdin auto-takes-over so CI isn't blocked.

On exit, teardown (`cleanup_written_mcp_configs`, run from
`run_teardown` after the terminal is restored) removes the entries
*we* wrote — our socket is about to die, so a lingering registration
would point at nothing. It only touches an entry whose `SPYC_MCP_SOCK`
is still ours (a successor that took over is left alone), deletes a
file/`.codex/` dir left empty, preserves any other servers/config the
user has, and refuses to modify a **git-tracked** config (warning on
stderr instead) — we never dirty something the user committed.

Enterprise managed-settings.json policies
(`deniedMcpServers`/`allowedMcpServers`) are honored.

**Two execution lanes** keep the event loop responsive. *Read* tools
(`get_spyc_context`, `search_*`, `list_worktrees`, `git_status`/`git_log`,
`claim_worktree`/`release_worktree`) run **on the socket thread** off a snapshot
of the context file + the pure `git::`/`fs::` facades — no `App` access, so the
loop never stalls. *Light writable* tools (`navigate_to`, `pick_files`, …) send
an `McpCommand` and run on the **main thread**, replying synchronously. *Heavy
writable* worktree mutations (`create`/`remove`/`clean_worktree`) take a third
path (`src/app/worktree_ops.rs`): validate on the loop, run the gix/graveyard IO
on a **detached worker**, then a tiny main-thread reconcile refreshes the
listing + writes the context before replying — so a multi-worktree archive can't
block render/input. Worktree removal is **safe-by-default** (untracked +
uncommitted content goes to the graveyard; a branch is deleted only if merged),
and a worktree can be **leased** (`claim_worktree` writes git's native lock) so a
second agent's cleanup refuses it.

## Documentation contract

Architecture decisions land in:

- **This file** — stable principles. Edit when a *decision* changes,
  not on every feature.
- **`AGENTS.md`** — slim, always loaded into Claude's context.
  Module index, conventions, "what spyc does" summary, MCP usage
  hints. Don't grow it past what's worth paying context tokens for.
- **`ROADMAP.md`** — forward plan; move shipped items to Done.
- **`CHANGELOG.md`** — release notes (Keep-a-Changelog).
- **`BACKLOG_DRAFT_NOTES.md`** — owner's raw backlog / draft notes (open +
  fixed bugs, ideas, reports). Move from open buckets to FIXED on commit.
- **`FEATURES.md`** — user-facing feature reference.
- **`README.md`** — landing page, install, positioning.
- **`src/ui/help.rs`** — in-app `?` help; user-visible keybindings.

When a commit changes user-visible behavior, update every doc
that's affected in the same commit — not as a follow-up.

### Load-bearing trap anchors

A handful of invariants fail *silently* when a later edit undoes them —
the session crashes only over SSH, or queries quietly return wrong rows.
Those get a **trap anchor**: a terse, grep-unique `SPYC-TRAP(<slug>)`
comment at each code site, dereferencing to the full rationale here,
keyed by `<slug>`. This file is the rationale store; the marker is an
invisible, render-agnostic HTML comment placed at the head of the
section:

```
<!-- SPYC-TRAP: <slug> -->
```

The **slug is the join key**, deliberately not the heading text — so the
section can be reworded without dangling the references. A guard test
(`app::mod_tests::guard_tests::traps_resolve_against_architecture_anchors`,
in `make check`) pins both ends: every `SPYC-TRAP(<slug>)` in `src/`
resolves to a marker here, and every marker has a code referrer. This is
a sparse discoverability signal for agents and humans — **not** a
comment-style change; ordinary "why" comments stay inline. See AGENTS.md
→ "Load-bearing trap anchors" for the authoring protocol.
