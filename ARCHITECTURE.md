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

## Files handed to an external viewer live in a private scratch dir

<!-- SPYC-TRAP: viewer-temp-symlink -->
`image_ops::viewer_scratch_path` is the only way spyc builds a path for a
file it hands to the OS image viewer (`o` on a diagram or an agent image).
It creates ONE per-process directory via `tempfile` — random name, `O_EXCL`,
mode 0700 — and returns paths inside it. Never join `env::temp_dir()`
directly and never use a predictable file name there.

`fs::write` follows symlinks. Both call sites previously wrote to a fully
predictable path (`spyc-agent-image-1.png`, `spyc-mermaid-<hash-of-source>.png`)
directly under `env::temp_dir()`. On macOS `$TMPDIR` is per-user and mode 700,
so nothing could be planted; on Linux it is the shared world-writable `/tmp`,
where any local user can pre-create that exact name as a symlink to a file of
the user's and turn the keypress into an arbitrary-file overwrite. The sticky
bit does not help — it prevents deleting or renaming *other people's* files,
not creating a name that does not exist yet. The 0700 directory also stops the
images (often screenshots) being world-readable while they sit there.

The directory deliberately outlives the process: the external viewer opens the
file after spyc has moved on. That is what lets the file *name* stay stable per
content, so re-opening the same diagram reuses its file rather than littering.

## Watcher event filtering (read-only access events)

<!-- SPYC-TRAP: fs-watch-readonly-access -->
The watch worker's notify callback drops read-only access events —
`Access(Open(_))`, `Access(Read)`, `Access(Close(Read))` — before they
reach the loop (`app/watch.rs`, `is_readonly_access`). Load-bearing on
Linux: notify's inotify mask includes `IN_OPEN` / `IN_CLOSE_NOWRITE`, so
every file *open* fires an event. spyc's own git-status + gitignore-excludes
machinery opens every `.gitignore` in the tree to build the ignore stack;
under the recursive listing watch those opens fire `Access(Open)` events,
which drive a refresh that opens them again — a self-sustaining storm
(~21 k events/s in a small repo, enough to starve the input path). macOS
FSEvents never reports opens, so it's invisible in local dev — the reason
this went unnoticed until a WSL user hit it. `Access(Close(Write))` (a real
write completion) and every Create / Modify / Remove event still pass
through. Removing the filter silently reintroduces the storm on Linux/WSL.

## Scrape fallback: the scan ignores a live report

<!-- SPYC-TRAP: scrape-scan-ignores-live-report -->
`settle_scrape_quiet` (`app/agent_status.rs`) must not skip a dirty tab's
screen scan on the grounds that `info.reported` is `Some`. It reads like a
free optimization — a live report outranks the scrape result in
`effective_activity`, so why pay for the screen read — and it is not, because
of *when* the two settles run. Both fire pre-recv in the same iteration:
`drain_pane_output` stamps `last_output_at` and sets `scrape_dirty`, then
`settle_scrape_quiet`, then `settle_agent_activity` — which drops that very
report through `report_superseded_by_output` (for agy, output newer than a
non-`Blocked` report supersedes it). Codex and Claude retain semantic reports
through output until expiry or a newer report.
So consuming the dirty flag on the
report's behalf discarded the scan for a report that no longer existed by the
end of the tick.

The flag is only re-set by fresh output, which is what makes the failure
permanent rather than merely late: an agent whose *last* output is the prompt
it is now blocked on never emits again, so the scan never runs. That is
exactly agy's tool-approval prompt (`PreInvocation` → `working`, draw the
prompt, go silent), and the scrape is agy's only source of `Blocked` — agy
has no hook event for "asking the user". The tier was therefore dead for the
single case it exists to serve, and the symptom is a dot that quietly decays
to Idle instead of going red, which nobody reports as a bug.

`scrape_step` takes `has_rules` rather than a report precisely so the pure
decision cannot express the skip. Other live reports discard scrape guesses
so they cannot resurface stale after expiry. A tab without detection rules
returns `Skip` before any screen read.

Codex's `PermissionRequest` precedes both automatic review and a human
approval dialogue, and supplies no call id. Its metadata-bearing report is
observational rather than a semantic user wait. A verified command, file-edit,
MCP-tool or network approval form at the viewport bottom temporarily overrides
a non-blocked report. The report remains stored and resumes when the dialogue
disappears. This
also requires scanning behind a live report. Native question blocks and
explicit agent blocks retain semantic precedence. `codex_approval::overrides_report`
is shared by activity settling and both status diagnostics, so the dot and
its explanation agree. The rules require known phrases and a complete default
footer at the viewport bottom, including native word wrapping. Missing required
text and other approval forms produce no guess. Codex's scan deadline starts
with the first pending repaint rather than moving with every output event;
continuous modal redraws therefore cannot postpone the scan indefinitely.
Other agents retain the trailing quiet-window debounce.

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
  live terminal grids through a shared `&runtime` borrow.

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
- Inbound, the pane honours a child's own DEC 2026: while its update is
  open the engine presents the last frame it finished and the parser
  worker publishes nothing, so a redraw that erases a line before
  rewriting it (brew's download list) never paints half-done. An update
  left open past a second is ended, Ghostty's own bound. Detail in the
  `pane::engine_ghostty` module doc.
- Per-frame: colour-depth downgrade. The theme is 24-bit `Color::Rgb`;
  terminals that can't parse `\x1b[38;2…m` (old GNU screen) drop all
  colour. `ui::color_depth::downgrade_buffer` rewrites the finished
  frame buffer's RGB cells to nearest-256 when the resolved
  `ColorDepth` isn't `TrueColor`. Resolution is CLI `--color` >
  `[layout] color_depth` > auto, where auto forces 256 inside GNU
  screen (`$STY` set, not tmux — screen inherits a `$COLORTERM`
  truecolor claim it can't honour) else keys off `$COLORTERM`. On the
  buffer, not the theme, so it catches syntect / diffs / ANSI
  passthrough too; a no-op at `TrueColor`.
- Per-frame: emoji width pinning. When a VS16 emoji's cell (`🌶️`,
  `❤️`) changes, ratatui's diff also rewrites the column it covers,
  and the crossterm backend sends that write with no cursor move. A
  terminal that draws the emoji two wide (Ghostty) is already past
  that column, so the rest of the run lands one column right and its
  last glyph survives the next frame's clear. That was a stray `)`
  after the `gV` version flash replaced the `g-` chord hint.
  `ui::emoji_diff::pin_emoji_widths` marks each such cell
  `ForcedWidth`, so the diff skips the covered column as it does for
  CJK, and the next write starts with a cursor move. It runs on the
  buffer because the emoji arrives from flashes, file names and agent
  output alike. Its tests replay ratatui's real diff and backend bytes
  through libghostty-vt and compare the screen to the buffer.
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
#444); `force_full_repaint` (`src/terminal.rs`) is the cursor-read-free
replacement — `Terminal::resize()` to the current size has the same
clear-and-full-repaint effect but takes the no-cursor-read branch. Any
detection that *does* need a probe (the graphics-protocol query feeding
`detect_image_picker`) runs **once at startup, before the input reader
spawns**, so nothing races it. Fixed 1.58.8.

Distinct from that: `setup_terminal` / `resume_tui` emit a one-shot
`Clear(ClearType::All)` (`\x1b[2J`) right after `EnterAlternateScreen`.
This is a plain erase-display *write* with no cursor read, so it's safe
over SSH — don't confuse it with the banned `Terminal::clear()`. It
exists for terminals with `altscreen off` (macOS's bundled GNU screen
4.00.03): there `?1049h` is ignored, spyc stays on the main buffer, and
ratatui never paints cells for regions it keeps blank, so pre-launch
shell content bleeds through below the list until the buffer is wiped.

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
  vector SVG is rasterized at the terminal's pixel size and centred,
  rather than fitting a small natural raster into a large area.
- **Footer-only overlay verbs must not `needs_full_repaint`** — a full
  repaint clears the screen and re-blits the image (a visible flash);
  the input-arm diff draw updates just the footer cells and leaves the
  image untouched. Only verbs that change/remove the image repaint.

## Vertical (left/right) split

`^s |` (aliased `^a |`) opens a second column on the right hosting a
**live-reloading preview** of the cursor file, and closes it again; `^s f` flips
its height. Splitting the two keys apart is deliberate: height is a layout
preference (`[layout] vsplit_mode`, full-height by default), not a step on the
way to closing, so repeating the open key is a plain show/hide. The split's
*shape* is pure Model
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
  `portable_pty`). The pane is a VT-emulated rectangle inside
  spyc's TUI; the child has a real tty, ours is unaffected.
- **Child input never waits for the child on the UI thread.** `PtyHost`
  queues keys, wheel commands and complete pastes to its private input worker.
  Ordinary input is bounded to 512 waiting batches and 8 MiB including the
  batch being written. A single larger paste is rejected with a size-limit
  error; queue pressure has a separate retryable error. File piping (`^a P`)
  above that limit asks for explicit confirmation of the payload size. A
  confirmed large pipe requires an empty queue and moves its existing allocation
  to the worker as one exclusive batch, preserving its bracketed-paste envelope.
  No other input is accepted until that batch finishes. Acceptance means queued,
  not consumed by the child; prompt tracking and attention settlement commit
  only after acceptance. A rejected batch sends no prefix. The worker preserves
  FIFO order and owns both the OS writer and its destructor (which may write
  EOF). Host close never joins it. Process-group teardown normally releases the
  blocked write, but a background job in another group can retain the slave and
  leave the detached writer blocked until that job closes it.
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

<!-- SPYC-TRAP: ghostty-terminal-send -->
`GhosttyEngine` carries a raw `GhosttyTerminal` from `spyc-vt-sys`, which the C
API makes neither `Send` nor `Sync`. `unsafe impl Send` is what lets `Pane`
keep its shape: the engine lives in an `Arc<Mutex<PaneEngine>>`, the parser
worker thread writes bytes into it, and the render pass locks it to read.

**Why this is sound, and what would break it.** The library never asks for
thread *affinity* — it asks for *serialization*, repeatedly and in writing:
`ghostty_terminal_compression_step` is documented as "not thread-safe with
other operations on the same terminal. The caller must serialize it with
writes, rendering, searches, and other terminal access." Serialized access from
several threads is exactly `Send` without `Sync`, and the mutex is what
provides it. The terminal is never shared unlocked and never reached from two
threads at once.

It would stop being sound if the library grew thread-local state, or if any
code path reached the terminal outside the mutex. The second is the one to
watch, because it is an ordinary-looking mistake: every read goes through
`Pane::with_screen` / `with_screen_mut`, and a new accessor that captures the
raw handle instead would compile.

**Why not an actor.** The alternative was to confine the terminal to the worker
thread and hand the renderer a copy. It was rejected on measurement, not taste.
The render path now reads through `ghostty_render_state_*`, whose two-phase
`begin_update` / `end_update` split exists (in its own documentation) for
"callers that synchronize access to the terminal state (e.g. with a lock shared
with an IO thread)": only `begin_update` touches the terminal, so the lock
window is one call rather than a whole frame walk. With the window that small
an actor buys nothing measurable and costs a message protocol and a copy — and
it would not even remove the `unsafe`, because the main thread still calls
`begin_update` on the terminal, so the handle still crosses threads either way.

<!-- SPYC-TRAP: pane-shell-rc-double-source -->
An interactive pane spawns as `$SHELL -i -c 'exec <command>'`
(`shell::pane_invocation`): `-i` so the wrapper loads the user's rc-file PATH
and aliases before resolving the command, `exec` so no job-control wrapper
survives to fight `^z` (see "Pane I/O" in AGENTS.md). When `<command>` is
*itself* a shell that sources an interactive startup file, `-i` is dropped and
only the target does an interactive rc pass.

Load-bearing because `exec` **preserves the pid** while resetting every shell
variable. Two rc passes therefore run under one pid with a fresh variable
space each time, which silently breaks any rc machinery that keys temp state
on pid plus a per-process counter. p10k's gitstatus is the live casualty: its
lock/fifo prefix is `$sysparams[pid].$EPOCHSECONDS.$((++_GITSTATUS_START_COUNTER))`,
so both passes compute the *same* path; the first pass's orphaned daemon then
hits its stale-client watchdog (`flock && sleep 5 && [[ -e lock ]]`), deletes
what is now the *second* instance's live lock, and the second client's
success-path `zf_rm` fails — surfacing as "gitstatus failed to initialize"
in a pane that works fine outside spyc. Restoring `-i` for a shell command
reintroduces it. The symptom is always third-party (spyc logs nothing), so
"pane-only rc weirdness + pid-keyed temp files" is the signature.

<!-- SPYC-TRAP: signal-teardown-precomputed -->
`signal_terminate` (SIGTERM / SIGHUP) restores the terminal from a string built
in `setup_terminal` — `RESTORE_SEQ` — and from a `termios` captured there before
`enable_raw_mode`. Neither may be produced inside the handler.

A signal handler may only call async-signal-safe functions. Building the restore
string runs crossterm's formatting (allocates) and reads `$TMUX` via `getenv`
(not on POSIX's safe list); `disable_raw_mode` takes a lock inside crossterm,
and `exit` runs atexit handlers and flushes stdio. Any of those in a handler is
a latent deadlock or corruption that shows up only when a signal lands at the
wrong instant — the hardest possible thing to reproduce. So the handler is
exactly `write` + `tcsetattr` + `_exit`, and everything it needs is computed
before the signal can arrive.

Load-bearing because the failure is *someone else's* terminal, after spyc is
gone: without the restore, `pkill spyc` or a closed terminal leaves the shell on
the alt screen, in raw mode, and — since `[mouse] capture` defaults on — with
`?1000h` armed, so every pointer move emits escape garbage for the rest of the
session. The plan that introduced default-on capture listed this teardown as
its prerequisite; the default shipped first, which is how the gap reached users.

## Grapheme clustering (DEC mode 2027)

Pane terminals run mode 2027 **on**, which is not libghostty's default, so a
cluster costs the columns `ui::display_width` budgets for it rather than one wide
cell per codepoint.

The choice is forced. spyc is both a terminal emulator and a client of the host,
so the engine's model has to match the host's — but ratatui is the only writer to
the host, and its `Buffer` stores one grapheme per cell and skips continuations
using `unicode-width` on that grapheme, the same rules `display_width` applies to
the chrome. Emitting a four-column flag through that path would mean
hand-splitting clusters in the render pass, so the alternative (summing
`ghostty_unicode_codepoint_width` to match the engine) could only move the
disagreement to the ratatui boundary. The engine was the one component modelling
the mode disabled ([#484](https://github.com/Tripstack-Corp/spyc/issues/484)).

The cost is real: that commitment was previously exercised only by the chrome,
whose sole cluster is the status-bar chilli, and now covers pane content, where
agents print emoji constantly. A host that does not cluster now disagrees over
far more cells. Accepted because the alternative is unavailable, not because the
risk is imaginary.

Set via `OPT_MODE_DEFAULT`, not `OPT_MODE`: it sets the current value *and* the
one RIS restores, so a child running `reset` keeps clustering.

Two limits. No `write_pty` callback is installed, so a child's `CSI ? 2027 $ p`
goes unanswered ([#486](https://github.com/Tripstack-Corp/spyc/issues/486)); the
common producers emit and assume, which is what made the bug visible. And vt100
cannot satisfy this (flag as two narrow cells, ZWJ family across six columns,
VS16 heart in one), so the contract test is scoped to the ghostty engine rather
than the `E: Engine` conformance suite.

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

## Git marker poll: one key function

<!-- SPYC-TRAP: git-poll-key-single-source -->
`git::status::poll_key` is the *only* place the 1 Hz marker poll's
freshness key is computed — `index`'s mtime plus the latest of
`head_ref_mtime` (the HEAD file **and** the resolved branch ref, since a
commit moves only the ref) and the shared `config` (which decides what
`status` reports — `core.bare = true` makes a dirty tree read clean —
while moving neither).

Both ends of the cache must use it: `refresh_git_state_for` compares the
live key against `git_poll_cache`, and the status walk that fills the
cache (`repo_status_stable`, off-thread) stamps its result with the key
it measured *before* walking. Two key functions that differ by any input
never compare equal, so the poll's short-circuit never fires and spyc
dispatches a full repo status walk **every second, forever** — silent
except for the CPU, since the markers it produces are correct.

That is not hypothetical: the config fold once lived only on the poll
side, so any repo whose `.git/config` was newer than its branch ref
(one `.git/config` write — spyc's own merge-driver install among them —
poisons every worktree sharing it) re-walked at 1 Hz for the life of the
session. A `:why-git` dump showed it as `generation: 3879` on the
affected column against `7` on a healthy one.

`:why-git`'s `on-disk` line calls the same fn, so the dump can't drift
from the mechanism it exists to explain.

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
- `.spyc-context-<pid>.json` — the MCP context: the focused column's
  state and the session's root, which the read tools validate a `root`
  argument against. It lives here, not in the working directory, so its
  path never moves with the root (#523).
- `mcp-<pid>.root` — the root sidecar: the directory that spyc is
  rooted at (`start_dir`, which `spyc -r` moves). It is stdio
  discovery's only record of where a spyc is rooted, and it is
  owner-private, so nothing an attacker can plant in a cloned repo takes
  part. Written at socket start, rewritten when the root moves, removed
  on cleanup; a crashed spyc's is swept at the next start.

The debug log is the exception: `spyc_debug!` output goes to
`/tmp/spyc-debug-<ts>.log` (timestamped per run, not under XDG) so
a log can be attached to a bug report without digging in state dirs.

Configuration lives at `~/.spycrc.toml` (user) and
`<cwd>/.spycrc.toml` (project, wins). Both are watched for live
reload. `spyc --print-config` emits a fully-commented default
template suitable for `>` redirect.

The project file is **untrusted** (spyc is routinely pointed at
hostile content): its cosmetic/behavioural settings and plain
rebindings are honoured, but *executing* keymap bindings (`unix`
shell commands, `jump`) are dropped — those take effect only from
`~/.spycrc.toml`. `[pane]` startup tabs spawn their commands at launch
with no keypress, so a project file's list is held apart
(`pane.project_tabs`) and opens only after the user approves that exact
list (`state::tab_consent`, `app::startup_tabs`); an edited list asks
again. A malformed project list is a warning, not an error, because an
error would discard the trusted user file too. `^R` reload re-reads the
project file from the **startup** cwd, never the browsed directory, so
browsing into a hostile tree can't load its rc.

Startup runs a health check that validates inventory / marks /
sessions / graveyard, cleans up orphaned files, and warns on
corrupt JSON.

## Startup-tab consent binds to content

<!-- SPYC-TRAP: startup-tab-consent-content -->
A project `.spycrc.toml`'s startup tabs run only once the user approves
them, and `state::tab_consent` records that approval against the list
itself, not against the project. It stores each tab's `command` and `cwd`,
in order (`Identity`, built by `identity`), and `consent_for` answers
`Allowed` only while the declared list still equals the recorded one. So a
`git pull` that edits the rc raises the prompt again instead of running
the new commands under an old yes, the way `direnv` re-blocks an edited
`.envrc`. `label` is left out on purpose: it changes only what the tab bar
shows.

Load-bearing because the failure is silent. Suppose a `PaneTabConfig`
field is added that changes what runs (an env map, a shell override,
arguments split out of `command`) without also being added to `Identity`.
An approval of one list then covers every list that differs only in that
field. Nothing crashes, and no test fails unless one pins the new field;
`any_change_to_what_runs_asks_again` is the test to extend.

## MCP server

`src/mcp/` runs a JSON-RPC server on a PID-scoped Unix domain
socket so multiple spyc instances coexist. Two transports share
the same dispatch:

- **`spyc --mcp`** (stdio) — what Claude Code spawns. Proxies to
  the live spyc instance via the socket. Falls back to read-only
  direct mode if no live instance is reachable.
- **In-process socket listener** — the running spyc accepts
  connections from the stdio proxy.

The agents' MCP `spyc` entry — `.mcp.json` (claude), `.codex/config.toml`
(codex), `.agents/mcp_config.json` (agy) — names **no instance**, just
`spyc --mcp`. An agent pane's env carries its spyc's `SPYC_MCP_SOCK` and its
own `SPYC_PANE_ID`, and the proxy connects to that socket, so an agent always
reaches the spyc that launched it and two spycs in one directory each keep
their own agents (see "The agents' MCP entry names no instance" below). An
agent started outside spyc has no socket in its env and falls back to
project-scoped discovery. spyc writes the entry **when an agent pane
launches** (`open_pane_tab_in` → `ensure_agent_mcp_config`), not at startup —
so a directory where no agent is ever run doesn't get a stray `.mcp.json` /
`.codex/` written into it.

Every spyc in a directory shares that one entry, so removing it is
refcounted like the status hooks (`state::dir_owners`): writing it claims the
directory, and teardown (`cleanup_written_mcp_configs`, run from
`run_teardown` after the terminal is restored) removes it only when no other
live spyc still claims it. It deletes a file/`.codex/`/`.agents/` dir left
empty, preserves any other servers/config the user has, leaves an entry an
older spyc pinned to its own live socket, and refuses to modify a
**git-tracked** config (warning on stderr instead) — we never dirty something
the user committed. The startup orphan sweep reaps an unclaimed entry that a
killed spyc left.

Enterprise managed-settings.json policies
(`deniedMcpServers`/`allowedMcpServers`) are honoured.

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

**A connection is attributed to the pane that opened it.** An agent pane's env
carries `SPYC_PANE_ID`; its `spyc --mcp` proxy reads it once and adds it to the
`initialize` it forwards, under `params._meta["spyc/paneId"]`. The connection
thread asks the loop whether a live tab carries that id
(`McpCommand::PaneContext`) and binds it only if one does: once, for the
connection's lifetime, and never from a tool argument, since a per-call id is
one an agent can forget to send. A bound connection's `get_spyc_context` adds
`pane`, that tab's live cwd, worktree root and branch, beside the fields that
describe the user's view; `report_status`, `register_scope` and
`wait_for_scope_clear` default to that tab instead of the focused one. An
unbound connection (an older proxy, the status hook, an id no live tab has) is
served exactly as before, and the read tools' default scope and allowed roots
don't depend on attribution at all. It is attribution, not authorization —
SECURITY.md says why.

## The agents' MCP entry names no instance

<!-- SPYC-TRAP: mcp-entry-names-no-socket -->
The `spyc` entry spyc writes into an agent's MCP config must never set
`SPYC_MCP_SOCK`, or any other per-instance value, in its `env`. One file per
directory serves every spyc there. It used to pin the writer's socket, and
claude and agy give an entry's `env` precedence over their own environment, so
whichever spyc wrote last received every agent launched in that directory —
including agents the other spyc launched, whose `SPYC_PANE_ID` then matched no
tab there and left them unattributed (#22). Nothing reported it: the agent's
tools worked, against the wrong instance, and a startup-only takeover prompt
couldn't see a directory that had no config yet.

The pane's env already names the right socket, so the entry only has to let it
through. Claude and agy pass their environment to an MCP server as it is;
codex clears it to a fixed allow-list, so its entry lists both names in
`env_vars`. An org-deployed `managed-mcp.json` has always had this shape
(`command` + `--mcp`, no `env`), which is why the bug never appeared under
one. `two_spycs_in_one_directory_each_keep_their_own_agents`
(`src/mcp/tests/coexistence.rs`) holds it, against a model of each agent's env
handling as probed on 2026-09-30.

## Mouse routing

`src/app/mouse/`. The layer is split pure/impure on the `route.rs` / `focus.rs`
template, which is what makes the interesting half testable without a terminal.

**`mouse/route.rs` — the pure half.** A `Copy` `MouseSnapshot` plus a pure
`route_mouse` → `MouseSink`, and `region_at` hit-testing the pointer against
`compute_layout`'s `FrameLayout`. It resolves against **the pointer, never
keyboard focus** — that single choice is what makes the wheel scroll the thing
you are looking at rather than the thing that happens to be focused.

**`selection.rs` — four drag-select clusters** (pager text, chrome, pane text,
list rows), each a `begin` + `extend` + `finish` triple. The press *claims* the
drag; an unclaimed press leaves child-forwarding untouched, which is why a drag
inside a mouse-aware child still reaches the child.

**`scroll.rs` — wheel for children that ignore mouse reports.** A tick becomes
the agent's own verified scroll keybinding, and a sustained streak escalates to
page keys. The agent is the rate limiter, not spyc. Also drives codex's `^T`
overlay.

**`forward.rs`** forwards to a mouse-aware child, pairing press and release to
the *same* child, and owns `mouse_report`'s translation into the pane's
coordinate space.

**`tab_hit.rs`** is the divider tab bar's geometry. `tab_widths` is the **single
source of truth** for per-tab widths, consumed by both `render/chrome.rs` and
the hit-test. Derive them in two places and a click lands on the neighbouring
tab — but only after whichever tab drifted, so it reads as "clicks are off by
one sometimes" rather than as a width bug.

**`mod.rs`** holds the selection/streak data types and `handle_mouse`, the one
dispatch entry. Every button rides the same pure fn via a `Gesture`:

- **left** — focus the region, clicking *through* to a mouse-aware child. Reuses
  `set_pane_focus` / `vsplit_focus`, so zoom-refusal and split-column restore
  come free rather than being reimplemented for the mouse.
- **middle** — `Effect::PasteFromClipboard`. The read runs on a worker because
  `xclip -o` blocks until the selection owner transfers, and a wedged one on the
  loop thread is a total freeze. `apply_clipboard_pastes` then routes it through
  `handle_paste`, so bracketed-paste gating is inherited and focus is read at
  *landing* time, not at click time.
- **right** — `resolver.enter_leader()` plus a due-now `chord_hint_due`, so the
  which-key popup appears without the keyboard path's debounce.

Middle and right are never forwarded to a child.

## Archive mounts

`src/archive/` is the pure core; `src/app/archive*.rs` is the app glue. A mount
is an **index, not a directory**: entering a multi-gigabyte zip writes zero
bytes, and a member is materialized only when something reads it.

**Containment is decided against the filesystem, never against the name.**
Names are only half the problem — `index` normalization makes a zip-slip *name*
structurally impossible by rejecting unsafe names on the way in, but a member is
also free to be a symlink, and a link relocates where a *later* member lands. No
per-name check can see that composition. So:

- **Extraction never traverses a symlink.** `contained_dest` walks the
  destination one component at a time and refuses if any existing component is a
  link.
- **A link target is judged the same way, and never folded on paper.**
  Cancelling `..` against the preceding component is a paper operation and is
  wrong the moment that component is itself a link: `d/link1/../x` reads as
  `d/x` lexically while `open()` follows `link1` first and climbs out of the
  mount. `link_target_contained` follows link components and applies `..` to the
  real directory reached.
- **A `..` behind a component that doesn't exist yet is refused outright**, since
  a later member could create it as a link. That is what makes the verdict
  independent of member order — the property the whole design rests on.

**Staging carries spyc's permissions, the index carries the archive's.** A
staged copy is owner-rw so spyc can read and rewrite its own cache; the mode a
repack writes back comes from the index, never off disk.

**Seekable vs streamed.** A zip or plain tar is indexed only and materialized
per entry. A compressed tar cannot be listed without decompressing it, so it
extracts as it streams under `[archive] extract_budget_mb` — the budget counts
bytes that *arrive*, not bytes a header declares.

**The write-back** (`write`) plans repack steps, writes a temp file beside the
original, carries untouched members across without recompressing, **verifies by
reading the result back**, then renames atomically — so a failed write leaves
the original byte-identical. The verification re-read of a compressed tar is
bounded by the same extract budget the mount used.

## Documentation contract

Architecture decisions land in:

- **This file** — stable principles. Edit when a *decision* changes,
  not on every feature.
- **`AGENTS.md`** — slim, always loaded into Claude's context.
  Module index, conventions, "what spyc does" summary, MCP usage
  hints. Don't grow it past what's worth paying context tokens for.
- **`ROADMAP.md`** — forward *strategy* (thesis, the 2.0 gate, non-goals, decisions).
- **GitHub Issues** — the live per-item backlog (features, fixes, ideas), labelled
  `area:*`/`type:*` on the roadmap board. (`docs/archive/BACKLOG_DRAFT_NOTES.md` is
  the archived raw-intake predecessor.)
- **`CHANGELOG.md`** — release notes (Keep-a-Changelog).
- **`FEATURES.md`** — user-facing feature reference.
- **`README.md`** — landing page, install, positioning.
- **`src/ui/help.rs`** — in-app `?` help; user-visible keybindings.

When a commit changes user-visible behaviour, update every doc
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

<!-- SPYC-TRAP: pty-input-never-waits -->
### Child input must not wait on the UI thread

A child that stops reading can fill its PTY. Blocking writes or a writer
whose destructor sends EOF must belong to the private `pty-input` worker,
including for captured commands. The UI enqueues complete batches with
`try_send`, never waits for capacity and never joins the writer on tab close.
A confirmed large file pipe changes only admission of that one allocation;
it cannot change the execution thread. A real raw-mode non-reading child
regression reaches the pane executor and `PtyHost::write_all`, with an owned-child
watchdog so a regression fails within a deadline. Rejected input must flash its
cause, preserve attention and leave prompt replay unchanged.


<!-- SPYC-TRAP: hook-cleanup-needs-managed-lease -->
### Hook cleanup requires a managed lease

An existing reporter marker can belong to a legacy import or a file whose
installation spyc refused. Borrowing that reporter protects a live pane from
sibling teardown, but grants no permission to remove it on exit. Hook leases
in `state::dir_owners::hooks` record the resolved directory, agent kind, pid and
whether installation succeeded. Teardown requires both the local and recorded
managed lease, with no live same-agent owner. A Claude-only or MCP-only session
cannot acquire Codex hook cleanup authority.

The separate lease registry uses a nonblocking advisory lock, held through
cleanup to prevent another instance registering between the owner check and
file removal. Corrupt/unreadable state or lock contention preserves hooks;
installation also skips writes when a lease cannot be registered. Legacy
untyped owners conservatively protect every agent's hooks in their directory.
An explicit `:hooks off` retains the user's authority to remove the active
project's reporters, subject to tracked-file and positional-trust guards.


<!-- SPYC-TRAP: partial-restore-keeps-saved-tabs -->
### Partial restore must keep unopened tabs in the next save

A session restore refuses unsupported agent commands per tab. The valid tabs
can resume, but autosave and quit overwrite the same session file. Saving only
the live panes would silently erase the refused records. The pure Model keeps
those records verbatim and the shared snapshot builder merges them back into
the saved list, mapping the selected live tab past inserted records. Spawn
failures receive the same protection. Session info lists the unopened tabs and
their reasons; none of their commands are executed. A wholly refused tab set
leaves the current session and its identity intact.


<!-- SPYC-TRAP: session-prune-by-last-save -->
### Session pruning ranks by last save, not by id

Each session lives in `<id>.json`, where the id is the session's creation time
in epoch millis, and `save_session` prunes the directory back to
`MAX_SESSIONS`. A restore keeps the id so later saves overwrite one file. The
filename therefore says nothing about recency: a session restored every day has
the lowest name in the directory while being the one saved most recently.
Ranking the prune by name deleted that file in the same call that wrote it,
whenever the directory already held `MAX_SESSIONS` newer-named files. Nothing
reported it, and the quit summary still said "session saved". `prune_victims`
ranks by file write time instead, and never selects the file the current save
just wrote, whatever its timestamp says.

The quit save skips a session with nothing to restore — no tabs, split or scope
claims — unless a file already exists under its id. Each file holds one of the
`MAX_SESSIONS` slots, so an empty quit would push out a real session. An
existing file is still overwritten, so tabs closed before quitting don't come
back on `-r`.


<!-- SPYC-TRAP: codex-hook-ownership-is-a-command -->
### Codex hook pruning requires a reporter invocation

A flag mentioned in an unrelated hook grants no permission to remove that
handler. Codex TOML installation, JSON migration, cleanup and positional-trust
preflight share a conservative command matcher: a direct `spyc` invocation
(or the caller's resolved reporter executable), a known wire status, and the
exact generated or bare legacy command shape. Quoted flags, other programs,
wrappers, extra shell actions and unknown statuses remain user content. The
matcher does not execute a command or inspect hook trust. Existing generated
reporter commands retain their bytes; changing cleanup recognition must not
force a new native hook trust decision.
