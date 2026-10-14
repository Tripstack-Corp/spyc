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

Codex's complete default async-question editor or collapsed positive question
summary supplies the same temporary attention, through `codex_questions`.
Its posting tool returns immediately, so neither that completion nor Enter alone
establishes an answer. The current native view is the signal: answering, skipping,
queuing, follow-up input or native resolution can clear it. A semantic block keeps
precedence, but the visible wait stays cached behind it because a later working or
done report need not repaint the child. Clearing the view retires only this fallback;
the latest live report resumes, and expiry still applies. Unknown/customized or
clipped footers produce no guess. No question content or answer ids are retained,
and no new hook, subscription or idle timer is introduced.

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

## Codex telemetry is consumer-owned and bounded

`agent::codex_records::metrics` projects numeric usage and bounded mode/tool
metadata through the shared rollout identity rule. It does not retain transcript
content. `agent::codex_metrics` owns the pure reducer and the read-only collector;
the native agent HUD is its application consumer. The library's narrow collector
export lets the standalone acceptance example drive the same code.

The caller supplies the pane's captured home, pinned thread, exact rollout and
agent generation. Results match that whole binding; a closed or rebound pane
cannot accept an old snapshot. The reader checks the header and home, holds only
complete bounded records, caps initial/backlog reads, and clears state across
truncation, replacement and gaps. Cumulative token families replace samples on
their own clocks; they are never summed. Context percentage stays unknown and
unsupported limit windows remain explicit.

Collection starts only for an explicit consumer. Runtime owners hold the worker
subscription, offsets and watcher; dropping the subscription cancels it. A
bounded signal and one-slot snapshot mailbox coalesce bursts, waking a consumer
only for changed metrics/source state. Blocking receives have no idle timeout.
macOS watches exactly the file and parent through kernel vnode descriptors, so
replacement can re-arm before the first read of the new inode. Linux watches the
parent non-recursively and filters read/open events. There is no sessions-tree
scan, account access or idle polling.

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

### Agent image gallery

`AgentProfile::transcript_images` selects the resolver and reader format. The
gallery snapshots the pane's home/session query and resolves its path on the
image worker. A tab id and request epoch reject results from a closed or reopened
popup. It indexes metadata there, then reads and decodes one selected
attachment on that same worker pipeline. Claude entries locate a physical line
and image block. Codex entries retain their source rollout, byte offset and a
record hash, plus an API-image fallback when a local attachment has a durable
inline twin. The hash rejects a rewritten source record. A fork's entries keep
parent-file provenance and stop at each inherited branch offset.

`src/state/codex_images/` bounds the whole index by bytes and row count, in addition
to the shared per-record cap. Local files must be regular and fit the encoded
byte budget; the display worker separately limits raster allocation and dimensions.
Clipboard capture carries the tab id and draft epoch alongside the forwarded
paste key. Accepted Enter advances that epoch, and late results for submitted or
closed tabs are discarded. Pending image bytes remain in memory only.

## Native agent HUD

The optional strip is a visible consumer of `codex_metrics` for a Codex tab and
`claude_metrics` for a Claude Code tab, not an activity authority.
`AppState.pane.agent_hud` holds one bounded snapshot keyed by its feed, stable
pane id, session (Codex's pinned thread, or the session Claude Code's hooks
reported), the agent's home and a monotonic worker generation. The feed is part
of the key because a tab that changes agents keeps its pane id, and one id and
home could name either agent's files. `Runtime.agent_hud` owns the cancellable
startup worker and the exact-file subscription. `Effect::AgentHud` starts/stops it only through the
executor; `Wake::AgentHud` lands its coalesced outcome in the pre-receive drain.
Both the current visible target and full generation binding reject stale work.
Changing tabs, hiding the pane, closing it or disabling the strip clears the
Model and cancels the Runtime consumer.

Startup walks at most 65,536 directory entries through three date levels, with
bounded iterator depth and no followed directory symlinks. It requires a unique
filename match under the captured home; the metric reader verifies containment
and the header. A Claude Code transcript has no header and lives under the
directory its session started in, which the pane's cwd may have left: the lookup
reads `<home>/projects` once, within the same entry budget, and asks each project
for the exact `<session>.jsonl`, refusing a name found twice. The reader then
checks containment and every record's session. Claude Code's home comes from
spyc's environment, read once at startup, because the Model reads no environment.
No missing-source polling is armed. The existing bounded reader
handles complete records, partial writes, file replacement and truncation. HUD
state is memory-only and diagnostics expose its metric projection.

`frame_layout` carves the strip after the vertical split and before prompt
wrapping. Its child rectangle is shared by PTY resize, draw, mouse routing,
scrollback and held-grid selection. A geometry change cancels a held pane drag
without forwarding its remaining events. Fullscreen images disable the strip.
`ui::agent_hud` budgets grapheme widths and escapes control characters in labels.
Drawing reads cached facts and timestamps. Metric landings compare the same
visible projection the renderer uses, so hidden fields cause no repaint. One
finite freshness edge can mark stale data; no recurring elapsed-time timer is
added. Semantic dots, hook trust and child input retain their existing paths.

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

Agent panes capture only `HOME` and `CODEX_HOME` immediately before the original
`exec`, after that single rc pass. Explicit per-pane home overrides are reapplied
at the same point so restore, restart and fork retain their recorded identity.
`pane::launch_env` bounds reads to 64 KiB and owns a 0600 record inside a 0700
private directory for the pane's lifetime; dropping the directory also prevents
a late-starting shell from recreating a closed pane's record. The Codex discovery
worker reads this metadata and scopes claims by home plus stable pane id. Missing
metadata stays unbound. New saves record unresolved homes explicitly and refuse
those tabs on restore; legacy saves without home fields retain their old launch
behaviour. No second interactive shell or environment dump is introduced.

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

A child that asks with `CSI ? 2027 $ p` is told the mode is set (see "Answering
the child's queries" below), though the common producers emit and assume rather
than ask, which is what made the bug visible. vt100 cannot satisfy this (flag as
two narrow cells, ZWJ family across six columns, VS16 heart in one), so the
contract test is scoped to the ghostty engine rather than the `E: Engine`
conformance suite.

## Answering the child's queries

The pane answers a child's terminal queries, but only with answers that are true
of spyc's pane ([#486](https://github.com/Tripstack-Corp/spyc/issues/486)).
libghostty-vt answers once a `write_pty` callback is installed. spyc installs
one in `pane::engine_ghostty::replies` and passes what the engine produces
through an allowlist:

| Answered | Why it's true |
|---|---|
| DECRQM mode reports | the engine's own modes, so 2026 and 2027 report what spyc does |
| DSR operating status, cursor position | engine state |
| DA1 `?62;22c`, DA2 `>1;10;0c` | Ghostty's own answers less clipboard access (52) |
| XTVERSION `spyc <version>` | not `ghostty` or `kitty`, the names children key image support on |
| DECRQSS | engine state |

Left unanswered: the kitty keyboard flags (spyc's key encoder is legacy-only,
so claiming the protocol would switch a child to an encoding spyc never sends),
XTGETTCAP (answered from Ghostty's terminfo, which advertises OSC 52 clipboard
spyc lacks), colour reports (Ghostty's palette, not the host's), size reports
(the engine's cell pixel size is a placeholder), the title report (an injection
path), and DA3, which Ghostty doesn't answer either. Kitty graphics and the
glyph protocol are switched off in the engine, so it doesn't answer their queries
at all. Left on, the engine replies `OK` to a graphics query, and image tools
then send pictures spyc never draws.

An allowlist rather than a denylist, so an answer a pin bump teaches the engine
stays unsent until someone judges it.

The callback fires inside `ghostty_terminal_vt_write`, on the parser worker,
with the engine lock held, so it only buffers (64 KiB at most). The worker takes
the buffer, releases the lock, and queues it on the pty's input writer as a
second producer beside the keyboard. That queue never blocks and shares input's
8 MiB limit, so a child that never reads its input can't stall the worker.
Replies go out even while a synchronized update holds the output: a child may
wait on one before it closes the update.

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
  the live spyc instance via the socket. An agent spyc launched falls back
  to read-only tools in place once its spyc is gone; any other agent gets a
  server with no tools (`mcp::route`).
- **In-process socket listener** — the running spyc accepts
  connections from the stdio proxy.

The agents' MCP `spyc` entry lives in each agent's own user configuration
(see "Agent wiring in user scope" below) and names **no instance**, just
`spyc --mcp`. An agent pane's env carries its spyc's `SPYC_MCP_SOCK` and its
own `SPYC_PANE_ID`, and the proxy connects to that socket, so an agent always
reaches the spyc that launched it, and every spyc on the machine keeps its own
agents (see "The agents' MCP entry names no instance" below). A shell pane
carries both too, so an agent typed into the shell inherits them. An agent
started outside spyc has no pane env and gets a server with no tools and no
instructions: the entry starts in every session of that agent, and a spyc
rooted at `$HOME` would otherwise answer every one. Project-scoped discovery
serves only an agent whose spyc restarted under it and a session codex's
shared daemon started. A report reaching a shell tab that isn't running an
agent is recorded but moves no dot.

**A shell tab's agent is the one in its foreground.** A moment after a shell
tab's output, `shell_agents` looks at the pty's foreground process group: at
most once a second while output streams, and never while the tab is idle. It
lists the group's members (`proc_group`: `libproc` on macOS, `/proc` on Linux)
and matches each one's argv[0], kernel name, or interpreted script against the
agents' binaries; the earliest-started match wins, and printed text plays no
part. `TabInfo::running` records what it found and `TabInfo::agent()` follows
it, so the dot, reports, `^a v` and `^a F` follow the live agent. When the
agent exits, everything the tab learned about it (reports, scrape, session ids,
codex home) is dropped. `^Z` leaves it `backgrounded`, the tab reading as its
shell until `fg`; it is never `suspended`, which would swallow the keys the
shell needs. Restore and restart still bring back the launch command
(`TabInfo::launched_agent()`), so such a tab is saved as its shell, and lookups
that match a session by start time use `agent_epoch_secs()`, the agent's start
rather than the tab's spawn.

Enterprise policies are honoured: a `managed-mcp.json` has exclusive control
of claude's MCP servers, and `managed-settings.json`'s
`deniedMcpServers`/`allowedMcpServers` can forbid spyc's entry.

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
don't depend on attribution at all. A proxy or status hook that codex's shared
daemon spawned (`codex app-server … --managed-daemon` among its ancestors,
`proc_group::has_ancestor`) inherited its `SPYC_PANE_ID` from whichever codex
started that daemon, so it names that tab, not its own session's: the reporter
stays silent, and the proxy connects unattributed. The proxy's inherited
`SPYC_MCP_SOCK` and `SPYC_CONTEXT` belong to that tab's spyc too, which may be
another instance, so it ignores them and discovers its spyc from its cwd, the
session's directory, where the daemon starts it. It is attribution, not authorization —
SECURITY.md says why.

**A restored tab keeps its pane id.** An agent can outlive the spyc that
launched it: Claude Code can host a session in its own background process, so
after `spyc -r` the tab shows the same conversation while the agent, its MCP
proxy and its hooks keep the env they started with. The session records each
tab's id (`SavedTab::pane_id`) and restore gives it back, unless a live tab
already holds it, so that env still names the tab. Its `SPYC_MCP_SOCK` is dead
by then. The proxy already falls back to project-scoped discovery, and the
status-hook reporter does the same, but only with a pane id to report for:
without one, discovery would deliver the report to whichever tab is focused.

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

## Agent wiring in user scope

`src/wiring/` installs an agent's `spyc` MCP entry and its status hooks once,
into that agent's own user configuration (#619); nothing is written into the
directories agents run in. The first time spyc launches an agent it asks, once
for that agent, whether to wire it (`app::wiring::offer_wiring`), and records
the answer in `state::wiring_consent`; `:hooks on|off|ask <agent>` and
`spyc --install-agents` / `--remove-agents` change it later. The install runs
on a worker, because claude's MCP entry goes through its CLI, and wakes the
loop with `Wake::Wiring`. Agents that read their hooks only at startup (codex,
agy) are flagged for a restart when an install changes their hooks. At startup
spyc refreshes every wired agent's files, so a moved or renamed spyc command is
caught up. Nothing is removed at exit: the wiring serves every spyc and every
later session. The files:

| Agent | Hooks | MCP entry |
|---|---|---|
| claude | `settings.json` in `$CLAUDE_CONFIG_DIR`, else `~/.claude` | `.claude.json` beside it, through `claude mcp add-json -s user` |
| codex | `hooks.json` in `$CODEX_HOME`, else `~/.codex` | `[mcp_servers.spyc]` in `config.toml` beside it |
| agy | `~/.gemini/config/hooks.json`, set `spyc-status` | `~/.gemini/config/mcp_config.json` |

These files hold the user's own settings, so spyc owns only content it can
recognize: a hook whose command runs `--report-status`, agy's `spyc-status`
set, and an MCP server named `spyc` whose command runs spyc. An MCP server named
`spyc` that runs anything else is someone else's, and spyc neither replaces nor
removes it. An install that would change nothing writes nothing, a file that
won't parse is refused untouched, and removal never deletes a file. Claude's
`.claude.json` is claude's own state file, rewritten constantly, so spyc reads it
but leaves the edit to `claude mcp`. An organization's `managed-mcp.json` takes
exclusive control of claude's MCP servers: spyc then reports the entry as
provided, or forbidden, and installs only the hooks.

codex's hooks go into `hooks.json` rather than `config.toml`, because codex
rewrites `config.toml` itself (project trust, and the `[hooks.state]` records of
which hooks the user trusted) and spyc's edits there should be as few as
possible. codex records that trust against a hook's file, event and indices,
and runs every layer's hooks without merging duplicates. So spyc rewrites its
own handler where it stands, appends one that's missing, and refuses a removal
that would shift one of the user's handlers (`no_codex_hook_ever_moves`). The
MCP entry is spliced into `config.toml` with `toml_edit`, so comments and layout
survive, and only the keys spyc needs change; `[pane] codex_mcp = false`, read
from `$HOME`'s rc alone because the entry it governs is global, keeps the entry
out. A codex pane whose shell sets its own `CODEX_HOME` reads its hooks there,
so once codex reports that home, spyc wires it too.

A project copy an older spyc wrote runs beside the user one, so spyc takes it
out (`wiring::legacy`): at launch from the agent's own directory, and at startup,
on a worker, from every directory its per-project consent and ownership records
name, for every agent the user has answered for. A directory another live spyc
relies on is left for that spyc, a file git tracks is left alone, and a project
file that is itself a user-scope file (`$HOME` as a project) is never touched.
Startup also turns every old per-project answer into a no, because an older
spyc still running reads that store before writing a project its hooks.

Claude Code hands its plan's rate-limit windows and the context window to one
place only: the `statusLine` command's input. So claude's wiring also wraps that
command (`wiring::statusline`). The wrapper runs `spyc --statusline`, which
forwards a bounded projection (`agent::claude_statusline`: the window size,
Claude Code's own share of it, and the five-hour and seven-day percentages with
their reset times) to its pane's spyc, then runs the user's command on the same
input, so the line they see is unchanged. The user's command lives inside the
wrapper, quoted, rather than in a file of spyc's: removal restores it byte for
byte. A `settings.json` synced to a machine without spyc still runs it through
the wrapper's `command -v` guard, and one with a spyc that predates
`--statusline` runs it through the `||` fallback: that spyc fails before reading
its input, and a current one exits 0 once it has run the command, so the
command never runs twice. The forward is an internal JSON-RPC
method, `spyc/statusline`, never an MCP tool, and the server re-checks the
projection's bounds, trusting the wire no further.

<!-- SPYC-TRAP: statusline-wrapper-format -->
`wiring::statusline::unwrap` must keep recognizing every format `wrap` ever
wrote. A wrapper it can't recognize reads as the user's own command, so the
next install wraps it again, and the startup refresh adds a layer on every
launch, each one forwarding and then running the next. Change the format only
alongside a match arm for the old one, and keep `--statusline -- COMMAND`
working: a wrapper written by one spyc may be run by another; `a_wrapped_command_comes_back_exactly`
and `settings_gain_the_wrapper_once_and_lose_it_exactly` hold the current one.

<!-- SPYC-TRAP: user-config-write-through-symlink -->
A user-configuration file is written through a symlink and keeps its mode
(`wiring::file::write`). Dotfiles managers keep files like `settings.json` as
links into their repository, and spyc's usual `fs::write_atomic` renames a new
file over the path, which would replace the link with a plain file and quietly
cut the user's configuration off from that repository. It would also reset the
file to the temp file's mode. `a_linked_settings_file_stays_a_link` and
`an_edited_file_keeps_its_mode_and_a_new_one_is_readable` hold both.

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
drag; an unclaimed press leaves child-forwarding untouched, which is why an
ordinary drag inside a mouse-aware child still reaches the child. Ctrl+left press
claims the pane for spyc even when the child requested mouse reporting.
[Selection caveats](CONFIGURATION.md#selecting-text) define Ctrl+click behaviour.

Pane selection owns a bounded `pane::snapshot::ScreenSnapshot` captured under the
parser lock at press. The draw and clipboard extraction use that same immutable
grid throughout the gesture; the parser worker keeps consuming live output. The
host terminal cursor is suppressed during the drag; the widget's main-screen block
comes from the held grid. Render reuses its pane rect to validate the snapshot and
only compares copied bytes with the live grid after release.
Release resumes the live grid and copies from the held grid. A released highlight
is compared with the live text under the draw's parser lock, so output racing
release cannot put it over different text. The snapshot preserves the engine's
soft-wrap boundaries, Unicode cells and empty-cell copy policy. Capture is capped
at 262,144 cells and 16 MiB of text. Keys, paste, wheel and other button gestures
retire the held selection; frame preparation also retires it when its tab id or
pane geometry changes or mouse capture is disabled.

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
handler. The user-scope `hooks.json` install, the cleanup of older spycs'
per-directory copies and its positional-trust preflight share a conservative
command matcher: a direct `spyc` invocation
(or the caller's resolved reporter executable), a known wire status, and the
exact generated or bare legacy command shape. Quoted flags, other programs,
wrappers, extra shell actions and unknown statuses remain user content. The
matcher does not execute a command or inspect hook trust. Existing generated
reporter commands retain their bytes; changing cleanup recognition must not
force a new native hook trust decision.
