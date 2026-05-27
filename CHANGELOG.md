# Changelog

All notable changes to spyc are documented here.
Format: [Keep a Changelog](https://keepachangelog.com/).

## [Unreleased]

### Fixed
- **A panicking vt100 parser no longer crashes the whole session.**
  The pane parser runs on a worker thread inside `catch_unwind`; the
  recovery path locked the parser mutex *inside* the guarded closure,
  so a panic unwound through the held guard and poisoned the mutex.
  The recovery re-lock then silently failed and the next render hit
  `with_screen`'s `.expect(...)`, taking down spyc. The worker now
  recovers the poisoned guard, installs a fresh parser, and clears
  the poison; all parser lock sites tolerate poison. The intended
  "screen blanks for a frame, the child repaints" behavior now works.
- **The pane parser worker thread is always stopped and joined.** It
  was only joined on the demotion path (`take_host`); tab close,
  restart, and app exit dropped the `Pane` without joining. A new
  `ParserWorker` RAII type owns the stop-flag + join handle and joins
  on `Drop`, covering every teardown path. (`Pane` can't implement
  `Drop` directly — it moves `host` out in `take_host`.)
- **Removed the dead `has_pending` fast-path.** Since vt100 parsing
  moved to the worker thread, `drain_output` is itself a cheap
  Acquire-load on a generation counter, but the pane render loop
  still gated it behind `has_pending`, which was never cleared for
  panes (only the unused main-thread `drain` cleared it) — so the
  gate always fell through. Dropped the flag and its plumbing.
- **`^a v` no longer hangs (or panics) on a very short terminal.**
  `lines_from_scrollback` paged the buffer in `rows`-sized chunks; a
  0-row screen made `chunk = remaining.min(0)` never decrement, so the
  event loop spun forever. It now early-returns on a 0-row screen.
  Separately, `Pane::resize` floors its dimensions at 1×1 so the vt100
  emulator (whose `set_size` does an unconditional `rows - 1`) never
  underflows when the layout produces a 0-height pane rect.
- **MCP clients no longer read stale picks/filter after a mutation.**
  `navigate_to` / `set_filter` / `pick_files` / `clear_picks` marked
  the context file dirty and let the debounced, event-driven writer
  flush it — but that writer is suppressed during a typing burst, so a
  follow-up `get_spyc_context` (which reads the on-disk file) could see
  pre-mutation state for as long as the user kept typing. These
  agent-initiated mutations now write the context file synchronously.
  Ordinary edits (cursor moves, browsing) stay debounced.

### Added
- **Opt-in Claude transcript scrollback.** `[pane]
  claude_transcript_scrollback = true` makes `^a v` on a Claude
  pane render Claude's on-disk conversation JSONL
  (`~/.claude/projects/<slug>/<session-id>.jsonl`) — structured
  user/assistant/tool turns — instead of the default
  terminal-scrollback capture. Default **false**: Claude's
  terminal output scrolls into the main buffer, so the existing
  vt100 capture works and stays the default; the transcript view
  is the cleaner alternative for those who want it (no
  grid/repaint artifacts). Same machinery as the codex transcript
  view, resolving the active pane's session by cwd + spawn time.
  If the toggle is on but no JSONL resolves, falls back to vt100
  capture.

- **Agent-aware scrollback: `^a v` on a codex pane shows the real
  conversation.** Codex keeps its history in a DECSTBM scroll
  region (both alt-screen and `--no-alt-screen`), so it can never
  be screen-scraped from the terminal. Instead, `^a v` now reads
  codex's on-disk rollout transcript
  (`~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` — the source of
  truth, flushed per turn), resolves the active pane's session by
  cwd + spawn time, and renders the actual conversation (user
  turns `❯`, agent replies, tool calls `⚙`) in the pager. Full
  pager features apply — search, jump, visual-yank. Titled
  `(transcript)`. This works in *both* codex modes since it
  doesn't depend on terminal capture at all. Brand-new sessions
  with no rollout yet flash a hint.
  
  Claude Code writes an equivalent JSONL transcript; wiring the
  same view for Claude is a noted follow-up (it works via
  terminal capture today, but the transcript would be cleaner).

### Fixed
- **Git status no longer gets stuck stale on a busy tree.** The
  background git-status worker captured its cache key (`.git/index`
  + `HEAD` mtimes) *after* running `git status`, so an index write
  racing that window stored old status under a new mtime — and the
  1 Hz safety poll then matched the same mtime and short-circuited
  forever, hiding staged/working changes until an unrelated later
  write happened to move the mtime again. The worker now stats the
  mtimes before reading status (validate-key-first), so a racing
  write forces one redundant refresh instead of a permanent stale
  snapshot.
- **`^a v` transcript scrollback no longer freezes on large
  conversations.** Both the codex and (opt-in) Claude transcript
  renderers read the whole JSONL into memory synchronously on the
  render thread — and real Claude conversation logs reach 100+ MB,
  so a single `^a v` could hang the TUI for seconds and allocate
  hundreds of MB. Scrollback only needs recent history, so the
  renderers now read just the tail (last 4 MB) via
  `state::read_tail_lossy`, dropping the leading partial line.
  Small transcripts (all codex rollouts today) are unaffected.
- **`^a v` on codex now explains why there's no scrollback (and
  the `--no-alt-screen` advice was wrong).** Codex confines its
  conversation history to a DECSTBM scroll region above its
  viewport — in *both* alt-screen and `--no-alt-screen` (inline)
  modes — so completed lines never scroll into the terminal's
  main buffer where vt100 (and therefore spyc) could capture
  them. The alt-screen guard didn't catch the inline case, so
  `^a v` previously opened a silent, empty-looking pager with no
  explanation. Now spyc detects an empty scrollback and flashes
  "no scrollback captured — codex & inline TUIs keep their own
  history (use the app's scroll keys)". An earlier release
  recommended launching codex with `--no-alt-screen` as a fix;
  that was incorrect and is now corrected in the flash and
  FEATURES.md. There is no spyc-side capture for codex history —
  use codex's own scroll keys.

### Performance
- **Cache the status-line agent short-id (was 65% of main-thread
  CPU on long-running sessions).** `App::render` called
  `active_agent_status` every frame, which walked every
  `~/.claude/sessions/*.json` with a full `serde_json::Value`
  parse and could then scan a session's `*.jsonl` looking for
  `custom-title`. For a long-running user with hundreds of
  accumulated session files, a symbolicated `sample` showed
  `serde_json::value::de::deserialize` consumed about two-thirds
  of the main thread.
  
  Cache the resolved status string with a 30 s TTL, keyed on the
  active pane's `(kind, cwd, spawn_epoch_secs)`. Switching tabs
  or chdir'ing inside a pane invalidates correctly; the status
  string itself changes ~never within a session, so a coarse TTL
  is fine. The resolver function's own doc comment had even
  flagged this as the right shape ("caller should add a per-pane
  TTL cache if it shows up as a hotspot") — now it has.

### Fixed
- **Remove v1.50.82/83 throttles — they were vestigial under the
  worker-thread parser.** With v1.50.84 moving vt100 parsing off
  the main thread, the two mitigations the previous PRs added
  (defer the active-pane drain during typing, cap pane-driven
  renders to ~30 dps during typing) stopped avoiding any work —
  they were just *delaying* the moment the main thread checked
  the worker's generation counter. That delay manifested as an
  off-by-one between keystroke and visible echo: pressing space
  or `^d` (claude vi mode delete-to-end-of-line) didn't update
  the cursor / line content until the typing-burst window
  expired. Removed both. Pane output now triggers a redraw the
  same tick the worker bumps the gen counter.

- **Pane render + cursor placement now share a single parser
  lock acquisition.** Follow-up to the v1.50.84 worker-thread
  refactor: the pane content was drawn under one `with_screen`
  lock, then `place_pty_cursor` re-acquired the lock to read the
  cursor position. Between the two, the worker thread could
  parse a new chunk — so the cursor reflected a *newer* grid
  state than what was just rendered. Visible as off-by-one
  tearing in claude's input region during fast typing /
  backspace. Each pane render path now folds the cursor
  placement into the same `with_screen` closure as the widget
  render, so the grid + cursor share a single mutex window.

### Performance
- **Move pane vt100 parsing to a worker thread.** v1.50.82/83's
  defer + cap mitigations helped but didn't eliminate the
  long-running-claude input lag. The structural problem:
  parsing pane bytes was synchronous on the main thread, so a
  chatty pane stretched every iteration body proportional to how
  many bytes claude had emitted since the previous iteration.
  
  Now each `Pane` owns a parser worker thread that consumes
  bytes from the PTY reader-thread channel and parses them into
  the (mutex-wrapped) vt100 grid concurrently with the main
  loop. Main thread reads grid state via `with_screen` /
  `with_screen_mut` closures that briefly lock the mutex.
  `Pane::drain_output()` becomes a non-locking generation-counter
  read.
  
  Effect: per-iteration cost on the main thread is now bounded
  by render + input dispatch only, regardless of how many bytes
  claude emits. Holding `j` while claude churns: cursor tracks
  every keystroke at key-repeat rate; no event backlog after
  release.
  
  The defer + cap mitigations from v1.50.82/83 are kept as
  cheap belt-and-braces — they cost nothing when not triggered
  and add a small render-throttle ceiling.
  
  Demote/promote path (`^a-z` / `:fg`): `Pane::take_host()` now
  stops the parser worker and restores the byte receiver to the
  host before handing it to `BackgroundTask`, so raw-byte
  buffering continues without interruption.

- **Defer active-pane vt100 parsing during typing burst.** The
  v1.50.82 render cap helped pane-only renders, but every
  *event*-driven render still included a fresh vt100 grid update
  for the pane — so each keystroke produced a ratatui frame diff
  that emitted all the cells claude had changed since the last
  iteration. On a chatty pane (long-running claude session), the
  emission cost was high enough that the loop iterated only ~8
  times/sec while holding `j`, and events queued up in
  crossterm's buffer for seconds after the user let go.

  Now: in the 300 ms typing-burst window, the active pane's
  reader-thread channel drain is skipped if we drained within
  the last 100 ms. Bytes still queue in the (unbounded) channel
  — no back-pressure to claude — and the vt100 grid stays
  frozen at the last-drained state. The next ratatui render
  therefore finds the pane region unchanged, and the diff
  emission is empty. After the user stops typing (or after the
  100 ms gap), the drain catches up and the pane redraws to
  current state.

  Background tabs are always drained — their `has_activity`
  flag must flip promptly. Only the *active* pane is deferred.

- **Cap pane-driven renders to ~30 dps while typing.** Reported:
  in a session where claude had been running for a while
  (growing chat history → growing per-redraw cost on claude's
  side), holding `j` to scroll the file list spiked spyc CPU to
  90%+. The activity monitor showed all channel drains at 0 — so
  the per-iteration cost wasn't channel work, it was the vt100
  parse + ratatui render of the chatty pane. Each iteration was
  doing a full render for whatever fresh bytes claude had
  emitted, and the cost stacked enough to make the loop iterate
  only ~9 times/sec instead of 30+.
  
  Now: when claude (or any pane) emits bytes *and* the user is
  inside the 300 ms typing-burst window, we skip the
  pane-driven render if the previous render was <33 ms ago.
  Bytes are still drained from the PTY (no back-pressure to
  claude), and key-event renders escape the cap entirely so the
  file list cursor tracks every keystroke. When the user stops
  typing, the cap lifts and the pane resumes redrawing on every
  fresh chunk. Fresh-session claude (no lag) is unaffected.

- **`.spyc-context.json` writes are now event-driven, not 1 Hz
  polling.** Reported: input echo in the claude pane feels laggy
  under spyc but not standalone — confirmed via the `A` monitor
  that spyc itself was idle (`mcp:0/s fs:0/s git:0/s p:0`), so the
  culprit had to be something *outside* spyc that claude was
  reacting to. Likely cause: claude's HUD plugin watches
  `.spyc-context.json` and re-renders on every mtime change. The
  old 1 Hz polling write kept yanking claude's main loop ~once a
  second, even when state hadn't changed enough to matter — and
  during that yank, claude couldn't service its user-space input
  echo. zsh felt snappy by contrast because zsh uses kernel echo.

  Now the write is gated on a `context_dirty` flag set by event
  sources that can actually affect context (keypresses, MCP
  commands, fs-driven refresh_listing, git worker results). At
  end of iteration we write only if dirty AND ≥150 ms since the
  last write AND not in the 300 ms typing-burst window. The
  serialize-then-skip dedup still suppresses no-op writes (e.g.
  chord-prefix keys that don't change state). Net effect: claude
  sees the file change exactly when something meaningful
  happened, and never while you're mid-keystroke.

- **Throttle git-worker re-spawns from `refresh_listing`.** On a
  huge tree (e.g. a 112K-file monorepo), running spyc next to a
  busy agent that writes files (claude saving findings, build
  outputs, IDE auto-saves) drove sustained ~48% CPU. Root cause:
  `refresh_listing` unconditionally invalidated the raw git status
  cache, causing every debounced file-system event in the listing
  dir to re-spawn `git status --porcelain -uno` (200-500 ms each
  on huge trees). Now throttled — at most once per 10 s on huge
  trees (1 s on small). The 1 Hz / 10 s safety poll still catches
  real `.git/index` changes immediately; trade-off is at-most-
  10 s lag in working-tree ` M` markers for edits within the
  throttle window.

### Added
- **Activity monitor (`A`) now has a second internals line.**
  Below the existing throughput line (dps / cells/s / poll / draw
  reasons), a second line on teal background surfaces background
  tasks, git worker, FS watcher, MCP, listing, and pager state:
  
  `bg:R●D✓ [P⏸]  git:N/s last:Nms  fs:N/s  mcp:N/s  list:N  pager:none|overlay|top|lower`
  
  Rates are per-second in the same 1 s window as the existing
  line. `git last:Nms` is the roundtrip duration of the most
  recent worker request (would have surfaced the CPU bug above
  on first glance — high spawn rate jumps out). Activity-only
  redraws still don't count toward `dps` so the readout doesn't
  self-oscillate.

### Changed
- **Scrollback mode now uses blue (not amber) as its color signal.**
  Reported repeatedly that scroll-mode is "confusing to remember" —
  the rule, the `[SCROLL]` tag, and the active-tab REVERSED fill
  all painted amber (`theme.pick`), which is the same color used
  for the focused / active state in *live* mode. Three signals
  but all the wrong color: amber kept reading as "this is your
  focused element," not "you've left live view."
  
  Now: the divider rule, the `[SCROLL]` tag, and the active-tab
  background fill all switch to `theme.dir` (the dir-color soft
  blue) when in scrollback. Blue is unused as a UI signal color
  elsewhere, so it unambiguously means "scrollback." Amber stays
  the "focus" color for live mode. The uppercased active-tab
  label stays as a shape cue independent of color.

### Performance
- **Typing-burst poll cadence for low first-echo latency.**
  Reported: "I seem to be able to type faster than the input."
  Cause: `crossterm::event::poll` doesn't wake on PTY output —
  only on host-terminal events — so after a keystroke is sent to
  the child, the event loop could sit at the 100 ms idle poll
  for up to one full window before draining the echo. Sustained
  typing was fine (`pane_had_output` already drives 16 ms), but
  the *first* character after an idle gap had a worst-case
  ~100 ms latency.
  
  Now: any keypress arms a 250 ms typing-burst window that
  tightens the poll cadence to 16 ms when a pane is open, so the
  first echo lands within ~16 ms instead of ~100 ms. Bounded so
  idle CPU stays low. Longer-term fix (let pane output wake the
  main loop directly, rather than timeout polling) is the proper
  solution but a larger refactor.

### Fixed
- **Background-tab activity is now detected promptly.** Repro:
  `sleep 10 && echo "hello"` in a zsh tab, switch tabs, wait —
  the `+` activity indicator never appeared until you pressed a
  key in the foreground. Cause: the event-loop pre-drain
  consumed the background tab's bytes (correctly, to avoid
  pile-up) but didn't flip `has_activity`. The render-path
  `drain_all` was the only setter, and by the time it ran the
  queue was already empty. Pre-drain now flips `has_activity` +
  requests a redraw when background bytes arrive, so the
  indicator shows up within one poll tick (16-100 ms depending
  on pane state).

- **Active tab is now visually distinct from a background tab
  with activity.** Reported by Spencer: "tab name highlighting
  buggy and highlights don't get cleared from tabs sometimes so
  it's hard to tell where you are." Cause: the active-tab style
  and the activity style were both `theme.pick` (amber) + BOLD,
  and `pick` shares its RGB with `prompt_prefix` — so the two
  states rendered *identically*, with only the small `*`/`+`
  glyph distinguishing them. Active tabs now also get a
  background-fill (REVERSED modifier), so "you are here"
  registers in peripheral vision before glyph parsing. Activity
  stays amber-bold; inactive stays gray. Three clearly distinct
  states.

  (Separate but related: background-tab output not being detected
  without foreground input — filed in BUGS, follow-up PR.)

### Changed
- **`^a v` alt-screen flash hints at inline-mode workarounds.**
  Reported by Spencer: `^w-v` on a codex pane was dead. Cause is
  fundamental — alt-screen TUIs do virtual scrolling inside a
  fixed grid, so terminal-side scrollback has nothing to show.
  The flash now names the workaround: launch the app inline
  (codex: `--no-alt-screen`, or `[tui] alternate_screen = "never"`
  in `~/.codex/config.toml`; Claude Code is already inline by
  default). FEATURES.md updated with the same guidance.

### Fixed
- **Pager `v` from the top-pane mount (`D`) now mirrors file-list
  `V` instead of taking over the screen.** When the user opened a
  file with `D` (top-pane embedded pager, bottom pane visible) and
  pressed `v` to drop into `$EDITOR`, the editor opened
  full-screen via the `Spawn` path — same as the centered-overlay
  pager — which defeated the whole point of staying in the
  bottom-pane-visible workflow. Now, when `view.mount ==
  Mount::TopPane` and a real source path is known, `v` spawns the
  editor as a top-overlay `Pane` (the same flow `V` from the file
  list uses), so the bottom pane stays visible for the duration of
  the editor session. Other mounts (centered overlay, lower-pane
  scrollback) and the temp-file edit case keep the full-screen
  flow.

- **Syntax highlighting for `Makefile` (and other bare-filename
  formats).** `highlight_to_lines` only consulted `path.extension()`,
  which is `None` for `Makefile` / `GNUmakefile` / `OCamlMakefile`
  — so the pager rendered them as plain text even though syntect's
  bundled `Makefile.sublime-syntax` lists the filename in its
  `file_extensions` table. Now we also probe by bare filename
  before falling through to first-line shebang detection. Any
  user-supplied `.sublime-syntax` keyed on a bare filename will
  also resolve.

### Added
- **JSON pretty-print pager.** `.json` files now open with
  canonical 2-space-indented JSON in the pager, with syntect
  highlighting on top. Minified-on-disk files become readable on
  open; already-pretty files render unchanged.
  - `m` toggles **pretty ↔ raw on-disk bytes** (mirrors the
    markdown `m` toggle). Toggle is suppressed when the two views
    would be identical.
  - Parse failures (json5-with-comments, malformed input) fall
    through to the existing plain-text pager — no behavior
    regression for non-strict JSON files.
  - `.jsonl` (line-delimited JSON) is intentionally NOT
    pretty-printed: reflowing would destroy the one-record-per-line
    affordance. Source still gets syntect highlighting.

  Folding, path indicator, search-within-structure, and `:jq`
  filtering are queued for v1.50.73+. No new crate deps (uses the
  existing `serde_json` + `syntect`).

### Fixed
- **`[EOF — exit N]` marker stays visible at the bottom of finished
  `!` captures and task viewers, even on long output.** Previously
  the marker was only painted in unused viewport rows below content
  — so short captures got the marker, but anything taller than the
  pane silently lost it after `scroll_to_bottom`. Now the marker is
  appended as a real content line on capture-finish / task-status
  transition, anchored to the bottom of the stream regardless of
  content length. Tilde-fill below short content still works.

- **Enter follows symlinks-to-directories.** Reported in the wild
  on a pnpm `node_modules` tree where `prettier ->
  .pnpm/prettier@3.8.3/node_modules/prettier` couldn't be entered.
  `DirEntry::metadata` uses lstat semantics, so symlinks land as
  `EntryKind::Symlink` regardless of target, and the Enter handler
  only descended on `EntryKind::Dir`. Now Enter (and the `D` / `v`
  guards) follow through to the target kind and descend when the
  target is a directory. Other ops (`R`, picks, etc.) still
  intentionally operate on the symlink itself.

- **Markdown re-renders after `v` (edit in $EDITOR) returns.**
  Reported by JRob: open a `.md` (rendered), press `v` to edit
  it in $EDITOR, quit the editor — the file came back as plain
  text and `m` had nothing to toggle to. The `PagerReturn::SourceFile`
  restore path built the view inline via `PagerView::new_plain`,
  skipping the markdown / alt_lines branch entirely. Reuse the
  full `build_pager_view_for_file` builder on return so markdown
  files go through the same rendering path as on first open;
  override the position with the scroll we stashed before
  launching $EDITOR.

### Added
- **`S` cycles sort; `gs` reverses; status bar shows the current
  mode.** Requested in BUGS: there was no quick way to swap sort
  order without typing `:sort <mode>`, and no persistent
  indicator of which mode was active.

  - `S` cycles name → size → mtime → ext → name (flashes the new
    mode for confirmation).
  - `gs` toggles a reverse flag on the current mode. Reversed
    reverts each mode's natural direction (Name/Ext ascending →
    descending; Size/Mtime descending → ascending). Dirs-first
    grouping is always preserved.
  - `:sort reverse` (or `:sort -`) is the colon-cmd equivalent
    of `gs`. `:sort <mode>` still sets the mode explicitly.
  - The status bar gains a `sort:<mode>` segment, with an `↑`
    marker when reversed.

- **`dd` / `Ndd` to delete; `ZZ` to quit.** Vim-style bindings
  for the two most common ops we'd been bare-shortcut-only on.

  - `dd` (no count) — confirms then removes the current selection
    (picks if any, else cursor entry). Same shape as the existing
    `R`.
  - `Ndd` (count prefix) — confirms then removes the cursor + N-1
    entries below, clamped at end of list (no wrap). The explicit
    count *ignores picks* — the count is the user being explicit
    about scope.
  - `ZZ` — alias for `Quit` (auto-saves session, same as
    `Q` / `^D` / `:q`).
  - `0dd` is a no-op (vim: `0` is line-start motion; never reaches
    `dd`).

  `Enter` is now the sole primary for "open/descend" — previously
  `d` and `Enter` shared the binding. Bare `d` is now a chord-
  arming key (vim parity: `d` is always operator-pending).

  Tested: bare `dd`, `4dd`, `dd` cancel-on-non-d-followup,
  `ZZ` → Quit, `Enter` still opens after the `d` split.

- **Warning-color row highlight while a delete confirm prompt is
  up.** The prompt text counts files, but doesn't tell the user
  *which* files — especially load-bearing for `Ndd` where the
  affected rows depend on cursor position at the moment the chord
  fired. Now the targeted rows render with a strong crimson
  background (overrideable as `delete_warning` in `[colors]`) so
  the consequence of `y` is visible at a glance. Cleared on
  confirm or cancel.

- **`[delete] confirm = false` opt-in for yolo mode.** Skips the
  `y/N` prompt and the warning highlight; deletions fire
  immediately on `R` / `dd` / `Ndd`. The graveyard is still the
  destination either way, so `gy` recovers. Off by default;
  documented in `spyc --print-config`.

### Fixed
- **Graveyard view: `?` now opens the help overlay, and the
  view flashes a hint on entry.** Reported by a user dropping
  into the graveyard for the first time: the restore (`p` /
  `P`), purge (`dd` / `x` / `Z`), and navigation bindings exist
  but were undiscoverable — `?` fell through to the catch-all
  arm in `handle_graveyard_view_key` and got swallowed. Two
  fixes: `?` / F1 in the graveyard view now opens the standard
  help overlay (returns to the same cursor on dismiss), and
  entering the view flashes `graveyard: p restore here · P
  original · dd/x purge · ? help` so the headline bindings are
  visible without having to look.

- **F9 (`ResumePane`) spawns in the current listing dir, not
  `PROJECT_HOME`.** Reported by Justin: after navigating to a
  different project folder in the file pane, pressing F9 opened
  `claude --resume` in the dir where he'd originally launched
  spyc instead of the dir he was browsing. `^a-c` (which prompts
  for cwd) already pre-fills with `listing.dir`; the bare-spawn
  path that F9 uses now follows the same default. Users who
  want a specific anchor can edit the prompt in `^a-c` or move
  `PROJECT_HOME` via `:project`.

- **`^a-\` / F10 is a pure hide/show toggle.** When no pane
  exists, `TogglePane` silently spawned the default command
  (`$SPYC_PANE_CMD` or `claude`) — surprising users who pressed
  the binding expecting a no-op (reported by Justin: "I see
  `^a-c` defaults to claude, POLA"). Now `^a-\` with no pane
  flashes `no pane — ^a-c to create one` and returns. Pane
  creation stays explicit via `^a-c`.

### Added
- **TX-side timestamps in the key trace.** `--key-trace` /
  `SPYC_KEY_TRACE=1` now logs `TX` lines whenever a pane write
  fires (`send_key` or `send_bytes`), annotated with the elapsed
  time since the most recent RX event:
  `[12345ms] TX send_key code=Backspace mods=... bytes="^?" [+3ms since RX]`.
  Lets a user reporting input lag (Justin's report:
  pane-shell keystrokes feel slow inside tmux) tell at a glance
  whether spyc itself is adding latency vs. something upstream
  (terminal repeat rate, tmux's `escape-time`, OS keyboard
  pacing). New `key_trace::note_rx_event` / `log_tx` helpers; the
  RX path stamps after logging so the next TX has a reference.
- **README note on `^a` / `^w` chord-prefix shell conflicts.**
  Calls out that an interactive shell running in the pane won't
  see those readline bindings, and points at `.spycrc.toml` for
  the rebind. Adds the tmux `escape-time 0` hint while we're at
  it. A more permanent fix (first-run flash) is on the roadmap.

### Fixed
- **Stale git markers after worktree switches.** Reported by
  Spencer: switching between worktrees occasionally surfaced
  markers (top-bar dirty flag, per-file status) from the
  previous worktree. Two leak paths fixed:

  1. `compute_git_info_fast` derived its dirty marker from the
     raw porcelain cache without checking the cache's
     `repo_root` matched the current one. After a worktree
     switch, the cache still held the old worktree's data until
     the async git worker finished refilling — so for that
     window the branch line showed the old dirty state. Filter
     the cache by `repo_root` before reading.
  2. `update_huge_tree` no longer cleared the raw cache when
     crossing a repo boundary (removed in v1.50.44's "cache
     survives leave-and-return" optimization). Add a targeted
     clear: only when the *new* anchor is a different repo
     than the cache holds — preserves the leave-and-return
     win (going from a repo to a non-repo dir doesn't satisfy
     the condition, so the cache lives on for re-entry).

### Added
- **User-supplied syntax grammars.** The pager's syntax
  highlighting (via `syntect`) ships with ~90 languages but has
  notable gaps — TypeScript is the canonical missing one.
  Instead of bundling every grammar we could ever want, spyc now
  merges `.sublime-syntax` files dropped in
  `$XDG_CONFIG_HOME/spyc/syntaxes/` (or
  `~/.config/spyc/syntaxes/` when `XDG_CONFIG_HOME` is unset).
  Grammars from Sublime's package store, `bat`'s assets, or
  `syntect/syntaxes` work as-is. Reported by Spencer:
  "typescript not highlighted but this should be customizable
  rather than pre-installed". Loading is best-effort; a malformed
  grammar fails open (log + skip) so a bad file doesn't break
  startup. Tree-sitter as the longer-term engine swap is on the
  roadmap.

### Fixed
- **`^a-k` / `^a-j` from inside a `^a-v` scrollback pager now
  actually transfer key focus, *without closing the pager.***
  v1.50.56 added a chord handler inside `handle_pager_key` to
  make the focus-switch work from scrollback. That handler was
  never reachable — `^a` is a meta key, so `route_key` always
  sends it to the resolver, not to the pager. The chord *did*
  dispatch `PaneFocusUp` correctly, but `set_pane_focus(false)`
  only flipped the flag; the `LowerPane` pager kept owning every
  subsequent non-meta key, so j/k went right back into the
  scrollback view even though `pane_focused == false`. Worst
  when visual-block was active in the pager because the user
  expected post-`^a-k` motions to navigate the file list, not
  extend a stale selection.

  The right fix lives in `route_key`: when a `LowerPane` pager
  is open and focus is on the top, non-meta keys flow to the
  file list — symmetric to the existing `TopPane`-pager-with-
  bottom-focus rule. The pager stays visible; only key
  ownership changes. `^a-j` flips back. Remove the unreachable
  chord handler and its `pager_pending_w_chord` field.

### Changed
- **Pager `^v` enters a placement state before visual block.**
  Previously `^v` immediately anchored at the top visible line,
  col 0 — awkward when the user wanted the anchor anywhere else.
  Now `^v` enters a "placement" cursor that moves with vi motions
  (`hjkl`, `w` / `b`, `0` / `$`, `g` / `G`); a second `^v`
  commits the anchor at the cursor for a visual block selection,
  or `V` commits to Line visual at the cursor's row. `Esc`
  cancels. The placement cursor renders as a reverse-video cell;
  a flash on entry lists the motions for discoverability.

- **Dropped the redundant `⏳ running` indicator in the prompt
  bar during `!` captures.** The pager title already shows
  `⏳ ! cmd — running... (Ns)`; the prompt-bar duplicate
  (`⏳ running: <cmd>  (keys → child, ^C interrupt, ^\ kill)`)
  said the same thing one row away. The keybinding hint is
  documented in `?` (the in-app help) for anyone who needs it.

- **`copy to:` / `move to:` / `mkdir:` prompts get vi editing.**
  These prompts used `Prompt::simple` (plain char append + backspace
  only); now they use `Prompt::shell` so vi bindings — `w`/`b`,
  `0`/`$`, `cw`/`dd`, etc. — work for editing the destination path.
  Tab completion already worked and still does. Up/Down history nav
  is intentionally disabled for these prompts: they share the
  shell-command history slot, which has nothing useful for a path
  prompt and was surfacing `!`-typed commands on Up arrow.

### Fixed
- **`^a-k` / `^a-j` work inside `^a-v` scrollback mode.** The
  top-level keymap resolver is bypassed when a pager is active
  (the pager owns the keys), so the focus-switch chord was
  silently dropped: `^a` consumed by the pager as a no-op, then
  `k` interpreted as scroll-up. The pager handler now tracks the
  `^a` chord locally when the active view is a `pane_scroll`
  pager; `^a-k` exits scroll mode and focuses the top, `^a-j`
  exits scroll mode and stays on the pane. Other pager modes
  (file viewer, help, task viewer) are unaffected.

- **Markdown prose reflows at the pager body width.** Two
  earlier choices combined badly on source files authored with
  80-col wrap (typical README / design-doc style): (a) v1.50.52
  made every source `\n` a hard line break, so each ~70-char
  source row became its own ~70-char rendered row; (b) prose
  wrap was hard-capped at the 80-col `CONTENT_WIDTH` even on
  wide terminals. Result: a paragraph that ought to flow across
  a 200-cell pager broke into a dozen short, mid-word rows.

  Now: soft breaks are CommonMark-default whitespace (paragraph
  reflows), and the wrap target tracks the pager body width
  passed in by `display_in_pane` (the same hint tables already
  use). Source-wrapped-at-80 paragraphs flow naturally to fill
  whatever width is available.

  The `**Key:** value` metadata stack still renders as separate
  rows: a new `force_hard_breaks_before_keyed_lines`
  preprocessor inserts markdown's two-space hard-break marker
  before each line that starts with `**Word(s):**`, so the
  metadata case keeps working without forcing every other
  paragraph into short-line mode.

- **Intermittent "Claude restore stops at the prompt" race fixed.**
  Restoring a session with a Claude pane sometimes left the
  `/resume <sid>` command typed but unsubmitted — the user had
  to press Enter once to finish the restore. Cause: a single
  combined write of `/resume <sid>\r` could arrive at Claude's
  TUI mid-render, the prompt absorbed the chars but the trailing
  `\r` got dropped. Now the keystroke injection is two-phase:
  text goes in after a 2 s banner-settle (was 1.5 s), then
  300 ms later a separate `\r` write submits it. The pause
  between writes lets the prompt finish reacting to the typed
  chars before we tell it to submit.

- **Wide markdown tables no longer overflow the pager's right
  edge when line numbers are on.** v1.50.48 hinted the markdown
  renderer at the actual pager body width so wide tables would
  expand instead of wrap. That width didn't account for the
  line-number gutter the pager draws on top of the rendered
  lines (`ilog10(lines) + 2` cells), so a table sized to fill
  the body width still got pushed off the right edge by the
  gutter columns. Now we estimate the gutter from the source
  line count (with a ~1-digit safety margin to cover soft-break-
  as-hard-break and table expansion) and subtract it from the
  hint. Tables fit cleanly under the right border.

### Changed
- **Markdown soft line breaks render as hard breaks.** CommonMark
  spec joins consecutive non-blank lines into one reflowed
  paragraph, which collapsed common patterns like
  `**To:** Alice\n**From:** Bob\n**Status:** Draft` into a single
  wrapped line. Now each source line renders on its own row —
  matches Discord / Slack / chat-style rendering and the way
  technical docs with `**Key:**` metadata expect to look.
  Prose authored at 80-col source wrap shows as several short
  lines instead of one reflowed paragraph; small trade for the
  metadata case actually working.

- **Command-capture pager title shows a status glyph.** The `!`
  capture pager already used ⏳ while a command was running, but
  the final title (after the child exits) was a bare
  `! cmd — exit N` with no visual cue for success vs failure. Now
  the title gains `✓` for clean exits, `✗` for non-zero / killed
  / crashed / `^\` interrupts — matching the glyphs the
  bottom-status-bar already uses for backgrounded tasks. Task
  viewer (`gB`) titles get the same treatment (⏳ running,
  ✓ exit 0, ✗ otherwise).

### Fixed
- **Pager position memory now actually restores.** Shipped in
  v1.50.49 but didn't work for two reasons; this fix addresses
  both:

  1. **Key mismatch.** `record()` canonicalized the path before
     insert while `get()` looked up the raw path, so any caller
     form that differed from the canonical form (`.`-segments,
     trailing slash, `/private/tmp` vs `/tmp` on macOS) silently
     missed. Added a single `key()` helper used by both sides
     plus a regression test.
  2. **Esc/q close bypassed save.** The pager Esc/q handler took
     the active pager and pushed it to buffer history *before*
     the trailing `clear_pager()` could run — by then
     `self.pager` was already `None`, so the disk save was a
     no-op. File pagers closed normally (the vast majority of
     close events) never landed on disk, just in
     `pager_history`. Now persistence runs before the take so
     scroll survives the next spyc launch too.

### Added
- **Pager remembers your last scroll position per file.** Open
  `foo.md`, scroll to line 200, close (or quit spyc entirely),
  reopen later — you land back at line 200 instead of the top.
  Stored as `pager_positions.json` under the spyc state dir,
  LRU-capped at 500 entries, keyed by canonical path. Scope is
  intentionally narrow: only files (`source_path: Some(_)`) — not
  command-output buffers, help overlays, pickers, etc. (those
  views' "start at top" is the expected interaction).
  Out-of-range saves (file shrank) clamp to the new last line.
  Saves on close, buffer swap, and process exit.

### Changed
- **Markdown tables expand to the pager body width.** Tables used
  to be hard-capped at the 80-column prose budget with a 24-cell
  per-column ceiling, so on a wide terminal a two-column reference
  table looked cramped (~30-cell columns) with the right half of
  the pager empty. Now the renderer takes an optional width hint,
  the file-open path passes the actual pager body width
  (90% of terminal, minus borders), and the per-column cap is
  computed proportionally — clamped between 24 (the old minimum,
  preserves existing tight behavior on small terminals) and 60
  (avoids 200-cell-wide single columns on ultrawides). Prose
  intentionally still wraps at 80 columns regardless of the
  hint — long prose lines are unpleasant to read.

### Fixed
- **`^W s` (PaneSendSelection) sends paths relative to
  `project_home`.** The selection-to-pane paste was emitting
  absolute paths, so a Claude (or shell) session running inside
  the project received `/Users/.../src/foo.rs` when the agent
  would expect `src/foo.rs`. Paths inside `project_home` now
  strip the project prefix; the project root itself becomes `.`;
  paths *outside* `project_home` stay absolute rather than walking
  up with `../../..` (rare and rarely intended). No change to
  other `selection_paths()` consumers — file commands, shell `%`
  expansion, `:grep`, etc.

- **Session save anchors on `project_home`, which now always
  defaults to the launch dir.** Two changes:

  1. `project_home` initialization at startup no longer requires a
     literal `.git` in the cwd. Previously, launching spyc one
     level above the actual repo (e.g. from `~/src/workspace`
     containing a clone at `~/src/workspace/inner-repo`) left
     `project_home` None — and downstream code (session save,
     harpoon, MCP context) had no project anchor. Now
     `project_home` defaults to the launch dir unconditionally;
     override with `:project <path>` / `gP`, clear with
     `:project clear`.
  2. Session save uses `project_home` (→ `start_dir` →
     `listing.dir` as defense-in-depth fallbacks) instead of the
     current listing dir. Quitting from a deep subdir was saving
     the session at that path, producing a noisy `session saved
     — …` flash and (because `load_sessions` dedups on cwd + tab
     commands) a fresh session entry for every drill-in rather
     than one entry per project.

  `spyc -r` accordingly drops you at the project root — navigate
  from there using frecency / harpoon / history.

### Performance
- **`git status` runs on a background worker thread.** Cache hits
  on chdir are already sub-ms, but cache *misses* (first entry into
  a project, mtime moved, huge flag flipped) still blocked the UI
  for the 200-500 ms `git status` index walk on a ~110k-file repo.
  Now the chdir returns immediately:

  - Listing renders right away with the branch from `.git/HEAD`
    (still sync — single small file read).
  - `git_files` is empty for the brief window between chdir and
    worker reply; markers fade in on the next paint.
  - The worker is a single long-running thread holding an `mpsc`
    request queue; results return on another channel that the
    event loop drains each tick.
  - A `git_generation` counter bumps on every cache-miss send.
    Results carry the generation they were spawned for; if the
    user has navigated past that point, the result is discarded
    silently (no thread cancellation needed).
  - Startup itself is async: `App::new` no longer blocks on
    `git_status` / `git_file_statuses`; the first paint shows the
    branch, markers arrive a frame or two later.

  `compute_git_info_fast` is unchanged — branch reads from
  `.git/HEAD`; dirty marker derives from the raw cache, which is
  empty during the in-flight window. Effect: a project the user
  has never entered before still takes ~300 ms for `git status` to
  walk the index, but spyc no longer freezes for it.

- **Cache survives "leave the project and return".** Two
  single-slot caches (`huge_tree_anchor` deciding `is_huge_tree`,
  and `git_status_raw_cache` holding the parsed porcelain) were
  wiped on every anchor change in `update_huge_tree`. Going from
  the active project up to its parent (no `.git`) and back in
  invalidated both, repaying:

  1. A `count_subdirs_capped` walk (10-100 ms on the parent dir
     before bailing at the 256 cap).
  2. A `git status --porcelain -uno` spawn that walks the
     ~110k-entry index (200-500 ms).

  Two fixes: (a) added a multi-slot
  `huge_tree_decisions: HashMap<PathBuf, bool>` so previously-
  classified anchors are reused without re-walking; (b) removed
  the explicit `git_status_raw_cache = None` reset — the cache
  is keyed on (repo_root, mtimes, huge) and self-invalidates in
  `git_file_statuses_cached`. Excursions out of a repo set
  `current_repo_root = None`, which already short-circuits the
  cache lookup without touching the stored entry.

- **Zero-subprocess chdir on a cached repo.** Following on from the
  huge-tree adaptive backoff, drilling into a deeply-nested package
  layout (`com/example/foo/bar/baz/...`) was still observed taking
  hundreds of ms per step on a ~110k-file Java monorepo. Each chdir
  was spawning **four** `git` subprocesses:

  1. `git rev-parse --abbrev-ref HEAD` (branch for the top-bar)
  2. `git status --porcelain -unormal` (dirty flag — full untracked
     walk even on huge trees, because the top-bar string was
     computed by `sysinfo::git_status`, which ignored `is_huge_tree`)
  3. `git status --porcelain -uno` (per-file markers)
  4. `git rev-parse --show-toplevel` (prefix for parsing #3) — and
     later `git rev-parse --git-dir` for the mtime cache key

  Index walks dominate the cost on a ~110k-entry repo (200-500 ms
  each). On a small project these are all sub-10ms — invisible —
  but they compound on a huge tree.

  After this change, every chdir on a *cached* repo (i.e. one we've
  already seen the raw status for, and whose `.git/index` and
  `.git/HEAD` mtimes haven't moved) does the following instead:

  - **No subprocess at all.** `git_info` (branch + dirty marker) is
    computed by reading `<repo>/.git/HEAD` directly (new
    `sysinfo::read_head_branch` — handles attached refs and
    detached HEADs) and using the cached raw porcelain's emptiness
    as the dirty signal.
  - **Per-file markers** are re-parsed from the cached raw
    porcelain using a locally-computed prefix
    (`canonical.strip_prefix(repo_root)`), no `git rev-parse`
    needed.
  - **Mtime cache key** is `stat <repo>/.git/index` + `stat
    <repo>/.git/HEAD` directly (new `sysinfo::resolve_gitdir`
    handles `.git` dirs *and* worktree/submodule gitfiles).

  New `AppState::git_status_raw_cache` holds the raw porcelain
  keyed on (repo root, `.git/index` mtime, `.git/HEAD` mtime, huge
  flag). Cache is invalidated when (a) crossing into a different
  project (in `update_huge_tree`), (b) the 1 Hz safety poll
  detects an index/HEAD mtime move, or (c) the event-driven
  `refresh_listing` runs (working-tree edits don't change
  `.git/index` but should still surface ` M filename` markers).

  Net: per-chdir cost on a cached repo drops from "hundreds of ms,
  user-visible" to "single-digit ms on the worst dir read; sub-ms
  for git work."

- **Huge-tree adaptive backoff.** At chdir, count the new dir's
  subdirs with the bounded-DFS `count_subdirs_capped` helper
  (previously Linux-only, now platform-agnostic). When the count
  exceeds `HUGE_TREE_SUBDIR_THRESHOLD` (256, same as the
  recursive-watch downgrade cap), set `AppState::is_huge_tree`
  and shift three behaviors:

  - `REFRESH_QUIET` (event-driven debounce): 500 ms → 3 s.
    Bursty fsevent storms no longer trigger `git status` on
    every 500 ms quiescent window.
  - `GIT_POLL_INTERVAL` (1 Hz safety net): 1 s → 10 s. Combined
    with the Tier 0 mtime cache, the idle poll is now stat-only
    *and* 10× less frequent.
  - `git_file_statuses` untracked mode: `-unormal` → `-uno`.
    Skips per-untracked-file enumeration entirely — the dominant
    cost on a working tree where most files aren't in the index
    (build artifacts, generated code, vendored deps). Trade:
    untracked files no longer get a `?` marker on huge trees.

  Flash on first entry: `large tree (256+ subdirs) — git poll
  throttled, untracked markers off` so the trade is visible.
  Reset on chdir so a brief excursion into a small subtree
  doesn't permanently downgrade. Reported on a ~110k-file
  freshly-cloned monorepo where Spotlight (`mdworker_shared`)
  and the spyc git poll were stacking up. Caleb's tests for
  `count_subdirs_capped` now run on macOS too (test module
  un-cfg-gated from Linux-only).

- **1 Hz git poll short-circuits when `.git/index` and `.git/HEAD`
  haven't moved.** The poll exists as a safety net for the rare
  case where FSEvents drops the `.git/index.lock` →
  `.git/index` atomic-rename notification (the macOS FSEvents
  inode-replacement edge case). On a quiet repo, it was firing
  `git status --porcelain -unormal` every second regardless —
  cheap on a small tree, real CPU on a ~110k-file working tree
  (reported with a Spotlight-indexing-overlap on a freshly-cloned
  monorepo). Cache the mtime pair of `.git/index` + `.git/HEAD`
  from the last successful poll; on the next tick stat both files
  and bail before any subprocess spawns if the mtimes match.
  Cache is scoped to the poll path only — the event-driven
  refresh (`refresh_listing` after fsevents debounce) still
  recomputes unconditionally because working-tree edits change
  file mtimes but NOT `.git/index`/`HEAD`, and a cache hit there
  would silently miss ` M filename` markers. Cache is reset on
  chdir.

### Fixed
- **`:q` / `:quit` now save the session and warn about running
  processes, matching the `Q` keybinding.** Previously the two
  quit surfaces diverged: `Action::Quit` (bound to `Q` / `^D`) ran
  the full lifecycle — double-tap confirm, running-process count,
  `save_session()` on the second press — while the `:q` colon
  command in `AppState::dispatch_command` just flipped
  `should_quit = true`, skipping persistence *and* the
  "N running processes — press again to quit" warning. That
  contradicted the user-facing contract documented in the
  reserve-`q` flash (`"q reserved for future macro recording — Q
  or :q to quit"` at `src/app/state.rs:1120`) and explained why
  long-time `:q` users could quit thousands of times and still
  see "no saved sessions" from `spyc -r`. Fix: pure-domain
  dispatch returns a typed `CommandResult::Quit` variant for
  `:q` / `:quit`; the App-side exhaustive match routes it
  through a new `App::request_quit()` helper shared with
  `Action::Quit`. The typed variant means a future refactor
  that drops the App arm is a compile error rather than a
  silent regression to "unknown command".

- **Yank to clipboard now works on Linux.** Previously `yf`, `yp`,
  `yP`, `ya`, and every pager-side yank failed with
  `yank failed: No such file or directory (os error 2)` because both
  clipboard sites unconditionally invoked `pbcopy` (macOS-only).
  Extracted a single `src/clipboard.rs` helper with platform branches:
  macOS keeps `pbcopy`; Linux tries `wl-copy` (when `$WAYLAND_DISPLAY`
  is set), then `xclip -selection clipboard`, then `xsel -ib`, and
  flashes `no clipboard helper available — install xclip, xsel, or
  wl-copy` when none is on PATH. Deduped two identical bodies
  (`src/app/mod.rs` and `src/ui/pager.rs`) into the new module. No
  new crate dependency — mirrors the `cfg(target_os)` pattern from
  `src/sysinfo.rs`. Closes Tripstack-Corp/spyc#2.

- **Markdown pager: loose-list bullets stay attached to their item
  text.** Viewing a `.md` file with blank-line-separated bullets
  (a *loose* list per CommonMark — what BUGS.md happens to be)
  rendered as `•` alone on one line, then the item text on the
  next line. Cause: pulldown-cmark wraps each loose-list item in a
  `Paragraph`, and the paragraph-start handler unconditionally
  flushed `current` to start a new line — dumping the bullet glyph
  that `Tag::Item` had just pushed. Fix: new `just_started_item`
  flag set by `Tag::Item`, honored by `Tag::Paragraph` to suppress
  exactly that flush. Tight lists were already fine because
  pulldown-cmark skips the Paragraph wrapper for them; added a
  loose-list regression test to pin the new behavior.

- **`J ?` opens the jump-history popup (spy-parity).** The popup
  already existed at `App::show_jump_history_popup` but was only
  reachable via `J <Esc> <Space>` — two prerequisites a spy user
  has no reason to know. The `?`-on-empty-buffer pattern already
  worked for shell history (`!?`); extended the same handler in
  `handle_vi_prompt_key` to recognize `PromptKind::Jump` and
  route to the jump-history popup instead. Also fixed two stale
  docstrings (`src/app/mod.rs:555-560` and `:7972-7976`) that
  described an Esc-on-empty trigger that didn't exist in the
  code. Help-overlay J line updated to advertise the `?`
  affordance.

- **`F10` / `^a-\` hides the pane instead of killing it.** Previous
  behavior was destructive: `toggle_pane` set `pane_tabs = None`,
  which dropped every `TabEntry`, which dropped every `PtyHost`,
  which SIGKILLed the claude (or whatever) child via the
  `Drop for PtyHost` chain. Daily-drivers lost their conversation
  every time they wanted the full screen for a few seconds.
  Replaced with a visibility flag (`state.pane_hidden: bool`):
  toggle flips the flag; child ptys keep running in
  `pane_tabs`; layout treats hidden as "no pane" so the file
  list reclaims the middle region; re-toggle restores everything
  unchanged. Discoverability: flash on hide is
  `pane hidden — F10/^a-\ to show`. Explicit kill of a tab
  remains via `^a-x` (`PaneCloseTab`); killing the *whole* pane
  container is now an intentional multi-step act (close each tab
  via `^a-x`), not a one-keystroke side effect. Sourced from
  external-contributor analysis (Caleb Howard,
  `feature-pane-toggle-preserve-context` thread).

- **Worktree entry / exit re-anchors `PROJECT_HOME`.** Selecting
  an existing worktree via `W l` → digit, creating one via `W n`,
  or deleting one via `W d` now updates `state.project_home` to
  the right anchor and triggers harpoon reload. Pre-fix behavior
  left `PROJECT_HOME` pointing at the parent repo (entry) or at
  the just-deleted dir (delete), so harpoon stayed bound to the
  wrong project, MCP context (and therefore claude's `:grep` /
  `search_paths` / `search_content`) searched the wrong tree,
  and `gh` (jump-home) jumped out of the worktree. Reported by a
  daily-driver after a confusing afternoon working out of a
  worktree.

  Mechanics:
  - **`W l` picker**: sets `state.project_home = listing.dir`
    after chdir and calls `reconcile_harpoon()` directly.
  - **`W n` new worktree**: pure-domain handler sets
    `state.project_home` after the chdir into the new worktree;
    the App-side `dispatch_prompt` wrapper now always calls
    `reconcile_harpoon` after `PromptResult::Handled` so the
    harpoon list reloads whenever a prompt shifts the project
    root.
  - **`W d` delete worktree**: captures the main repo path
    *before* the delete (it's the first entry of
    `git worktree list --porcelain` — and unreachable after the
    worktree's directory is gone), chdir's to parent as before,
    sets `state.project_home` to the main repo. `listing.dir`
    and `project_home` legitimately differ here — the user lands
    in the parent so they can keep browsing sibling worktrees;
    harpoon / MCP context bind to the main repo, which is the
    sensible "home" after the just-used worktree is gone.

- **Tab labels no longer carry a stale `[exited N]` suffix across
  session restore.** When a pane's child exited at any point during
  a session, `mark_exited` appended ` [exited N]` to the tab label
  for display. Session save serialized the label verbatim; on
  `spyc -r` the tab was respawned alive but the label was restored
  with the dead-process suffix still glued on — the user saw their
  freshly-running `htop` tagged "exited 0" until they manually
  renamed it. New `pane::tabs::strip_exit_suffix` helper strips
  the suffix; applied at both save time (so new session JSONs
  land clean) and restore time (defensive, so older session files
  heal automatically the next time they load). 6 new unit tests
  cover numeric / `?` codes, no-suffix passthrough, idempotence,
  the require-trailing-`]` guard, and the "middle of label" weird
  case.

- **`^a-v` scrollback pager now travels with the tab.** Previously
  the pager was App-level state: opening it on tab 1, then `^a-n`
  to tab 2, left the pager rendering tab 1's history while the
  active-tab pointer was on tab 2 — a stale view of the wrong
  tab's pty. Now each tab has its own `stashed_scrollback_pager`
  slot. Tab-switch action handlers (`PaneNextTab`, `PanePrevTab`,
  `PaneTabByIndex`) stash the current pager onto the just-departed
  tab before flipping, then restore the destination tab's stashed
  pager (if any) after. The pager visually disappears as you tab
  away from a scrolled-back tab; it reappears, exactly as you left
  it (scroll, search, selection), when you tab back.

  No flash on swap — the visible appear / disappear is its own
  signal. Esc / `q` close the pager normally as before; only
  tab-switch triggers the stash. Content-bound pagers (Overlay
  file viewer, TopPane Markdown) are App-level and persist
  across tab switches unchanged — they're tied to content, not to
  a tab.

### Added
- **`[markdown] open_as_rendered` preference.** New `.spycrc.toml`
  knob controlling which view a `.md` / `.markdown` file lands in
  when first opened in the pager. Default `true` (rendered first,
  `m` toggles to source — current behavior). Set to `false` to
  open in source view (`m` then toggles to rendered). Pairs
  naturally with the scroll-preservation fix below — whichever
  side you start on, per-side memory keeps your place across
  toggles.

### Fixed
- **`m` (Markdown toggle) preserves scroll position.** Pre-fix
  behavior was a hard reset to line 0 every time — the comment
  said "preserving an absolute index would land somewhere
  arbitrary" because the rendered and source views have different
  line counts. Replaced with per-side memory: each side's scroll
  is stashed in a `saved_alt_scroll` slot on toggle, and restored
  when the user comes back. First-ever visit to a side has no
  memory yet, so we fall back to a proportional projection
  (`old_scroll * (new_total - 1) / (old_total - 1)`) which lands
  in the right neighborhood. Round-tripping rendered ↔ source ↔
  rendered now restores the original position exactly; checking
  the source for a specific section and bouncing back is the
  motivating use case. Four new unit tests pin the rule
  (proportional first-time, exact round-trip, clamp to new
  bounds, no-alt no-op).

### Added
- **Pager yanks prepend a `# <title>` header by default.** `y` / `Y`
  (full and visible) and visual-mode `y` (line + block) now include
  the pager's title — `!cargo build`, `task #3: cargo test`, the
  filename, etc. — as a single comment-style line followed by a
  blank separator, then the content. The point: when you paste a
  capture into chat / a code review / a note, the "what command was
  this from" context goes with it. Empty titles still skip the
  header (no `# \n\n` noise). Toggle off in `.spycrc.toml`:
  ```toml
  [yank]
  include_pager_title = false
  ```
  Default `true` — covered by config round-trip tests and a unit
  test on the header helper itself.

### Internal
- **`unsafe` reduction: 36 → 2 sites.** Remaining unsafe: the
  `:setenv` prompt command (user-driven, intentional) and the
  `install_signal_handlers` block at startup (one isolated call —
  see below). Four changes got us there. (1) Test-time
  `XDG_STATE_HOME` mutation replaced by a per-thread
  `with_state_root` override on a new `crate::state::state_root()`
  helper; every state module (`harpoon`, `marks`, `inventory`,
  `sessions`, `graveyard`, `history`, `frecency`, `health`) now
  delegates to it, and the `env_test_lock` mutex is gone. (2)
  `libc::kill(-pid, sig)`, `libc::tcsetpgrp`, `libc::getpgrp`,
  `libc::umask` migrated to `rustix` safe wrappers
  (`process::kill_process_group`, `termios::tcsetpgrp`,
  `process::getpgrp`, `process::umask`); the `tcsetpgrp` fd comes
  from `std::io::stdin().as_fd()` so no hand-built `BorrowedFd`.
  (3) `SHELL` and `TMUX` reads in `shell` and `term_title` accept
  their values as parameters, so tests pass them directly (no env
  mutation, no shared lock). (4) `paths::expand` factored into
  `expand_with(input, home, lookup)` so substitution tests pass a
  hashmap-like closure instead of mutating the process env.
  Net: 36 `unsafe` blocks across 11 files → 2, 669 tests still
  green, one new dep (`rustix`, already in our transitive tree).

  **Signal handlers stay on raw `libc`.** An experimental migration
  to `signal-hook` for SIGINT/SIGQUIT/SIGTTOU broke the
  `tcsetpgrp` restore path after `v` / `p` / `;` foreground
  commands: with SIGTTOU set to a custom Rust handler instead of
  kernel-level `SIG_IGN`, POSIX `tcsetpgrp` from outside the FG
  group returns `EINTR` rather than succeeding (the kernel checks
  for "ignored" specifically, not "handled"). Symptom: spyc would
  exit (status 146 / SIGTSTP, via the host PTY reaping a stopped
  child as exit) the first time the user closed an editor /
  pager. Reverted that piece — the one remaining `unsafe` block
  in `install_signal_handlers` is well-isolated to startup and
  the function doc spells out why no safe wrapper substitutes for
  `SIG_IGN` inherit-across-exec semantics on SIGTTOU.

- **Centralized key-event routing into `route_key`.** Closes the
  routing-refactor TODO filed in v1.50.25 after five inline-routing
  bugs in a week (#75 paste leak, #78 TopPane pager chord, #80
  LowerPane pager chord, #81 exited-tab race, plus the original
  V-overlay key bug). The five inline guards in `handle_key`
  collapse into a single `match` on a `KeyDestination` enum;
  decision logic lives in a pure function `route::route_key(snap,
  key) -> KeyDestination` over a small `RouteSnapshot`.
  Behavior preserved (657 tests, including 19 new unit tests in
  `src/app/route.rs` — one per known regression case plus happy
  paths). The route function takes no `&self` and does no I/O, so
  every routing decision is unit-testable without instantiating
  the TUI; future routing bugs land as a failing test row rather
  than a sixth inline patch. Sets up v1.60 Phase 3 (input
  forwarding) to add a `RemotePeer(socket)` destination without
  touching the rest of `handle_key`.

### Fixed
- **Exited tabs only dismiss on explicit `^a-x` (or `^a-R` to
  restart).** Reported: pressing `^a-R` to restart an exited
  pane tab raced the dismiss — the bare `^a` keystroke fired
  the old "any key closes the exited tab" branch, dropping the
  tab before `R` could land. Equally surprising as the BUGS.md
  entry that flagged "ESC dismisses too eagerly." Fix: on an
  exited focused tab, non-meta keys flash a hint (`pane exited
  — \`^a-R\` to restart, \`^a-x\` to close`) and do nothing.
  Meta chords (`^a` / `^w` / `^\` / F10, plus any
  resolver-pending continuation) fall through to the resolver
  so `^a-R`, `^a-x`, `^a-j`, etc. all work as documented.

- **`^a-k` / `^w-k` reach the resolver from a `^a-v` scroll pager.**
  Same routing-shape bug as v1.50.25, different mount. The
  earlier fix covered `Mount::TopPane` (the `D` in-app pager)
  but not `Mount::LowerPane` (the `^a-v` pane-scrollback pager).
  Meta chords from inside the lower pager were swallowed by
  `handle_pager_key`. Generalized the routing guard: any
  slot-mounted pager (TopPane *or* LowerPane) falls through to
  the resolver on meta chords regardless of focus.
  Non-meta-when-bottom-focused fall-through stays scoped to
  `TopPane` only, since `LowerPane` visually *replaces* the
  bottom pty (no separate slot to type into). Documents the
  fourth instance of the same pattern in the routing-refactor
  TODO; the refactor still wants to happen before v1.60 Phase 3
  so input forwarding doesn't thread through this ad-hoc shape.

### Documentation
- **v1.60 plan: Compatibility section + Phase 0 details.** Two
  questions surfaced during plan review: (1) what happens when a
  hub talks to a peer running an older spyc version, and (2) how
  exactly does the hub discover peers and keep its list fresh?
  Added a `Compatibility` section to `docs/V1_60_PLAN.md` with
  the per-capability behavior matrix — older peers are always
  visible in CounterTop with metadata from their discovery file,
  but features (frame mirror, input forwarding) are gated on
  capability advertisement. Discovery file shape grew
  `schema_version` / `spyc_version` / `capabilities` / `mode`
  fields. Phase 0 is now more precise: peers publish atomically
  (write `.tmp` + rename), the hub watches the directory with
  `notify` (already a runtime dep) for live updates, and stale
  entries are detected via `kill(pid, 0)` on a ~5s tick.
  Phases 1, 2, 3 each note which capability they add to a fresh
  spyc's advertised set when the phase ships. Principle stated
  in plain English: *an older peer is visible but degraded,
  never invisible.*

### Fixed
- **`^a-j` / `^w-j` reach the resolver from a `D`-opened pager.**
  Reported in BUGS.md: with `D` (in-app pager mounted in the top
  pane) open and a bottom agent pane visible, pressing the
  pane-focus chord while the pager has focus did nothing — the
  pager's key handler swallowed it. Cause: same shape as the V
  paste bug and the original `top_overlay` key bug — the routing
  guard in front of `handle_pager_key` checked focus ownership
  but not "is this a meta chord regardless of focus." Fix:
  when the pager coexists with a bottom pane, meta chords
  (`^a` / `^w` / `^\` / F10) ALWAYS fall through to the
  resolver, regardless of which side currently has focus. The
  rest of the existing behavior (non-meta keys go to the pager
  if the pager has focus, to the bottom pane if it has focus)
  is unchanged.

### Documentation
- **v1.60 plan rewritten — siblings + mirror, not recursion.**
  Design discussion with the user reframed the architecture: each
  spyc instance lives in its own terminal and owns its own pty;
  the hub is a peer client that discovers and connects to siblings
  via the MCP socket each spyc already exposes. "Take control" =
  mirror the remote's render stream and forward keystrokes back,
  with both terminals (original + hub) showing the same frames
  because they're both clients of the same render state. The
  recursive-composition route from yesterday's plan is recorded as
  considered-and-rejected. Six phases revised: Discovery →
  CounterTop read-only → Frame mirroring (new TeeBackend, new
  `subscribe_frames` MCP method, `👁 N` observer badge) → Input
  forwarding (new `send_input` MCP method, last-keystroke-wins
  semantics gated by OS single-focus) → Headless dispatch
  (`spyc --detached /path`) → Agent View bridge + Spice drawer +
  Pass polish.

- **v1.60 plan filed.** New `docs/V1_60_PLAN.md` lays out a
  six-phase plan for **CounterTop** — a hub view above any one spyc
  instance, with discovery so peer spycs find each other, a HUD
  aggregating per-workspace agent state, and a `--hub` startup
  mode for "launch one spyc on terminal open, run every project
  from there." Mirrors `V1_5_PLAN.md`'s shape. Kitchen vocabulary
  for UI surfaces: CounterTop (the hub view), Burner (an active
  workspace), Pass (workspaces ready for the user), Spice drawer
  (saved workspace bundles). Architectural decision recorded:
  recursive composition (each workspace = a child spyc process in
  a pane tab of the master, introspected via the MCP socket the
  child already exposes) chosen over a monolith refactor that
  lifts App state into a `Vec<Workspace>`. ROADMAP.md cross-links
  the plan as a pre-v2.0 release.

### Fixed
- **Paste into a `V`-opened top-overlay editor stays in the editor.**
  Reported in BUGS.md: with `V` open (editor running as a top
  overlay) and a bottom pane visible (e.g. claude), pasting text
  routed the paste to the bottom pane *and* yanked focus to the
  bottom pane — losing both the paste and the editor session.
  Cause: `Event::Paste`'s router had three branches (prompt /
  `pane_tabs` / ignored) but no `top_overlay` arm, so the
  `pane_tabs.is_some()` branch fired indiscriminately. Fix: a new
  arm before the `pane_tabs` one routes paste to `top_overlay`
  when it exists, *unless* the user has explicitly focused the
  bottom pane (`^a-j`) — in which case the bottom-pane arm still
  wins. No focus-stealing in the overlay arm (you're typing into
  the editor, not signalling intent to switch).

### Documentation
- **Presentation deck refreshed.** `docs/presentation.html` was
  written around M14 (HTTP MCP era) and had drifted: slide 1
  tagline now matches the README's MCP-from-the-pane framing;
  slide 3 corrects HTTP → PID-scoped Unix domain socket with
  agent auto-discovery via `.mcp.json` / `.codex/config.toml` /
  `settings.json`; slide 6's `src/mcp.rs` row drops the HTTP
  label; slide 7's MCP design pattern says "MCP socket threads"
  instead of "HTTP threads"; slide 9's M14 timeline entry
  reflects the multi-agent reach; added a v1.5 phase entry;
  slide 5 / 10 / 11 stats bumped (19K → 35K LOC, 358 → 638
  tests, 121 → 397 commits, 44 → 60 source files, 6 → 10 MCP
  tools); roadmap flips `--print-config` and "expanded snapshot
  tests" to shipped. Slide 1 date moved April → May. Cargo.toml's
  `description` field also updated to the new framing — it had
  drifted to "pairs with Claude Code" wording that pre-dated
  codex/gemini support and the MCP-as-the-thesis positioning.

- **AGENTS.md: new "Commits, merges, and CHANGELOG" sub-section.**
  Three guidelines for future agents working on the repo, derived
  from watercooler's catalogue review of the project's commit
  history: (1) commit subjects state actual scope rather than just
  the headline feature; (2) merge with `--strategy squash` so
  `main`'s log is one commit per shipped shape; (3) CHANGELOG bucket
  follows the user-observable nature of a change, not where the
  diff landed in the tree. No code change.

- **README rewrite leading with the MCP-from-the-pane thesis.**
  Replaced the lede + "Why spyc?" with a tighter framing surfaced
  by an external review of the project's catalogue: spyc as the
  *noun the agent operates on*, not a file manager that happens
  to host a chat window. Added an explicit "What it is" section
  with the two-pane / chord-prefix / Unix-domain-MCP-socket
  one-pager, called out Codex + Gemini as first-class (matching
  the actual agent support), and dropped a stale v1.21.1 footer.
  Body of the doc (keybindings, configuration, etc.) is
  unchanged.

### Added
- **Status bar agent segment.** Active pane's agent identity now
  appears as its own status-bar segment between the git and suffix
  bands: `claude:<8-hex>` / `gemini:<8-hex>` / bare `codex`.
  Short-id resolves at render time from each agent's on-disk
  session records (`~/.claude/sessions/`,
  `~/.gemini/tmp/<proj>/chats/`) using the pane's `spawn_epoch_secs`
  to pick the matching entry; the matching is the same
  closest-by-startTime logic save_session uses. Codex's UUID lives
  in its rollout filename (`rollout-<TS>-<UUID>.jsonl`); parsing
  that is a future follow-up — Codex panes currently show just
  `codex` in the segment. Hidden when no pane is open or the
  active pane isn't a known agent. Token usage from each CLI is
  skipped — none of the three surface it natively.

### Internal
- **V1.5 plan archived to `docs/V1_5_PLAN.md`.** Six-phase plan
  for the v1.5 pager/task-viewer unification; long since shipped
  (closed at v1.50.0). Keeping the historical doc but out of the
  repo root.

- **CI: `CARGO_INCREMENTAL=0` + cache bust.** Pipeline #380's
  warm-cache run still showed cargo printing `Compiling X` for
  ~30 dep crates per step. Cache restoration was working
  correctly (target/ downloaded at 512MB), but cargo's per-crate
  incremental metadata files include build paths and timestamps
  that go stale across runners, so the fingerprint check
  invalidated every artifact and forced re-verification (and
  partial re-build for proc-macro / build-script crates).
  Disabling incremental compilation in CI is the standard
  big-Rust-shop fix — target/ becomes smaller and deterministic
  across runs. Local dev keeps incremental on (the override is
  inline on `make check` / `cargo llvm-cov`, not a Cargo config
  change). Bumped `.ci-cache-version` to 2 so the next run
  uploads a fresh, smaller target/ without the incremental
  metadata.

### Added
- **Gemini CLI as a third agent kind alongside Claude and Codex.**
  `gemini` (and path-qualified variants) are now detected as
  `AgentKind::Gemini` and tracked through the save/restore
  pipeline:
  - **Save**: walks `~/.gemini/tmp/<project>/chats/*.jsonl` —
    each chat's first line is JSON metadata with `sessionId` +
    `startTime` — and picks the unclaimed UUID whose start time
    is closest to that pane's spawn time. Same multi-pane
    discipline as Claude/Codex (a `claimed` set prevents two
    panes from collapsing onto one conversation).
  - **Restore**: Gemini's `--resume` consumes an *index* into
    `--list-sessions`, not a UUID. spyc shells out to `gemini
    --list-sessions` synchronously, parses the
    `<n>. <title> (...) [<uuid>]` lines, and spawns
    `gemini --resume <n>` for the matched UUID. Falls back to
    bare `gemini` if the lookup fails (binary missing, session
    pruned, output format drift) so the user can still pick.
  - New plumbing: `AgentKind::Gemini`, `is_gemini_command`,
    `command_without_gemini_resume`,
    `find_gemini_sessions`, `gemini_project_name`,
    `parse_iso8601_to_epoch_secs`,
    `parse_gemini_list_sessions_for_uuid`, plus a generalized
    `pick_closest_unclaimed_session` (now over a
    `SessionCandidate` trait so Claude and Gemini share the
    picker).
  - 18 new unit tests (ISO-8601 parser, command stripping,
    list-sessions parser, picker generality).

### Fixed
- **Multiple Claude/Codex panes now resume to distinct sessions.**
  Reported in BUGS.md: with several Claude/Codex tabs alive in the
  same cwd at quit time, restoring the saved session pulled all of
  them into a single conversation. Cause: `save_session`'s resolver
  fell back to `most_recent_jsonl_for_cwd` when the pane hadn't yet
  printed an exit banner (the common case — Claude is usually still
  alive when the user quits spyc), and that fallback returned the
  same JSONL for every pane in the cwd. Fix: `save_session` now
  walks tabs in order tracking a `claimed` set of already-assigned
  session IDs. The Claude resolver scans
  `~/.claude/sessions/*.json`, picks the unclaimed record whose
  `startedAt` is closest to *that* pane's spawn time (a new
  `spawn_epoch_secs` on `TabInfo`), and verifies the JSONL exists
  before saving. Codex's banner-derived ID is also gated on
  `claimed`. Picker is a pure helper
  (`pick_closest_unclaimed_session`) with five unit tests covering
  the closest-match-with-claim-skip semantics.

### Internal
- **`.ci-cache-version` for explicit cache busting.** New file at the
  repo root, included in all four pipeline cache key file lists
  (`cargo`, `target`, `target-cov`, `rustup`). Bitbucket caches are
  immutable per key — `Skipping upload for existing cache` in the
  logs means once populated they never auto-update for newly-added
  crates or target artifacts. Result: caches were frozen at whatever
  was in `$CARGO_HOME` / `target/` on the first cold run after a
  key change, and proptest (added in v1.50.8) was being re-downloaded
  every PR. Bumping the integer in `.ci-cache-version` changes the
  derived key and forces a fresh upload on the next run. This
  Unreleased entry triggers an initial bump (1) so v1.50.14's first
  pipeline picks up everything currently in Cargo.lock.

- **Coverage step: `CARGO_TARGET_DIR` instead of `--target-dir`.**
  Hot-fix for v1.50.12: `cargo llvm-cov` doesn't accept the
  `--target-dir` flag (it's a wrapper that doesn't pass that arg
  through to the inner cargo invocation), so the v1.50.12 ship
  failed on the Coverage step with `error: invalid option
  '--target-dir'`. The repo doesn't gate merges on green builds
  so it landed anyway and broke main's Coverage. Switched to the
  documented escape hatch — set `CARGO_TARGET_DIR=target-cov`
  inline on the cargo llvm-cov line. Same intent, working syntax.

- **Coverage step gets its own `target-cov/` cache.** The Quality
  and Coverage steps were sharing the `target` cache and racing
  each other: Quality writes un-instrumented artifacts, Coverage
  writes coverage-instrumented ones (`-C instrument-coverage`
  RUSTFLAGS), and whichever step uploaded last clobbered the
  cache for the next run — so the loser pulled down artifacts it
  couldn't reuse and rebuilt the whole graph (~18s+ in the
  v1.50.11 PR run). Fix: a separate `target-cov` cache (same
  key, separate path) plus `--target-dir target-cov` on the
  `cargo llvm-cov` invocation. Each step now owns its cache and
  warms incrementally across runs.

### Added
- **`:` command tab-completion.** Hit Tab while typing the spyc
  command name (`pa<Tab>` → cycle through `pane-to-task` /
  `pause`, `lim<Tab>` → `limit `, `ver<Tab>` → `version`) and the
  prompt fills in from the canonical `SPYC_COMMANDS` list. Single
  match completes with a trailing space; common prefix advances
  and shows the candidates with "— Tab to cycle"; ambiguous
  prefix stages a cycle through the matching names.

  Once the buffer contains whitespace (`cd <Tab>`, `grep foo<Tab>`)
  Tab falls through to the existing filesystem completion. No
  change to the `J`/`!` prompts.

  Two unit tests guard the contract: `SPYC_COMMANDS` must be
  sorted + deduped, and every entry must round-trip through
  `dispatch_command` without falling into the "unknown command"
  branch (so adding a new `:foo` to the list without wiring up
  the dispatch arm fails CI). One drive-by fix: bare `:set` now
  flashes `usage: :set key=value` instead of "unknown command".

### Internal
- **CI: relaxed `target` cache key + dropped the `-slim` image.**
  Two more cuts on top of v1.50.6 / v1.50.7. (1) The `target` cache
  was keyed on `Cargo.lock + rust-toolchain.toml`; that sounded
  conservative but in practice meant every patch version bump
  busted the cache and forced a ~3 min recompile of the whole dep
  graph, even though `Cargo.lock` only changed because the
  lockfile records `name = "spyc" version = ...`. The cache is now
  keyed on `rust-toolchain.toml` only — cargo's per-crate
  fingerprint hashes each crate's actual inputs (source,
  build-script outputs, feature flags), so restoring a stale
  target/ against a different lockfile is safe: changed deps
  recompile fresh, unchanged deps reuse. (2) Switched base image
  from `rust:1.85-slim` to `rust:1.85` (non-slim). Bakes in
  `make`, `git`, `curl`, `ca-certificates`, `tar`, etc., so the
  ~13s `apt-get install` step disappears. The image pull is
  bigger but Bitbucket caches images per-runner.

- **One pty roundtrip integration test.** New
  `tests/pane_roundtrip.rs` (`#[cfg(unix)]`, single test) spawns
  `cat` via `portable-pty`, writes a line plus `^D`, drains the
  master in a thread, parses the bytes through `vt100`, and
  asserts row 0 of the rendered screen contains the input. This
  validates the integration contract spyc relies on
  (`portable-pty` pty plumbing + `vt100` parser) without going
  through any spyc-internal wiring; if a future portable-pty
  release stops delivering CRLF-translated bytes, or `vt100`
  changes how it lays out cells, this test trips. Closes the
  v1.5-era "[L] One pty integration test" TODO.

- **Property tests added for three core invariants.** New
  `proptest` dev-dep + one `proptest!` block per site, narrowly
  scoped:
  - `shell::expand::shell_quote` — round-trip property: parsing
    the output back through a small POSIX single-quoted-string
    decoder always returns the original input. Includes the
    decoder as test-only code so the property is real (not a
    tautology against the encoder itself).
  - `state::ignore::Mask` — `Mask::matches(name)` is the union
    over the patterns: a multi-pattern `Mask` matches iff any
    single-pattern `Mask` matches. Plus a literal-self-match
    invariant for names without glob meta-chars.
  - `keymap::resolver::Resolver` — feeding N digits then a motion
    produces the action with `count == parsed integer`, and the
    count is consumed (next motion defaults to 1). Plus a
    leading-zero invariant: any number of bare `0`s is ignored
    and leaves no pending state. Bounded to 1-4 digits so values
    stay well below `u32::MAX` (the underlying multiply isn't
    checked; out of scope for this test).

  Closes the v1.5-era "[S] Property tests (narrow)" TODO. All
  five properties pass with `proptest`'s default 256 cases per
  property.

- **Pipeline now caches `$RUSTUP_HOME` too.** Follow-up to v1.50.6:
  `rustup component add rustfmt clippy` (quality step) and
  `rustup component add llvm-tools-preview` (coverage step) were
  the next-largest CI cost after cargo-deny — about 44s combined
  on a cold image because every run re-downloaded and re-extracted
  the components. Added a third cache (`rustup` → `$RUSTUP_HOME`,
  keyed on `rust-toolchain.toml` so a toolchain bump is the only
  invalidator) and wired both steps to use it. `rustup component
  add` is idempotent; on a warm cache the step becomes a near-no-op.

- **Bitbucket pipeline cache restructured + cargo-deny prebuilt.**
  Two changes that together cut cold-cache CI from ~6 minutes
  toward ~1.5: (1) the `cargo` cache (covering `$CARGO_HOME` —
  registry, downloaded crates, installed bin tools) now keys on
  `rust-toolchain.toml` only; previously a Cargo.lock change (which
  every patch version bump triggers, since the lockfile carries
  `name = "spyc"` + version) busted the whole cache and forced a
  fresh registry fetch + cargo-deny rebuild every PR. The `target`
  cache still keys on Cargo.lock since compile state must follow
  the dep graph. (2) `cargo install cargo-deny --locked` (~3 min
  on a cold cache) is replaced by a pinned prebuilt binary download
  from the upstream EmbarkStudios GitHub release, verified against
  the project's published `.sha256`. Pinned to 0.19.4; bump the
  VERSION + SHA256 pair together at the same call-site.

- **Widget snapshot test coverage extended.** Added 10 `insta`
  snapshots (on top of the existing 4 status-bar snaps): `list_view`
  (basic / picks-and-takes / empty), `pager` (ANSI input, hex dump,
  line-number gutter widening from 1- to 2-digit, search highlight
  bar), and the vi-mode prompt (simple / insert / normal). Glyph-
  level only — same trade-off as the status-bar suite — but enough
  to catch layout, gutter, and search-bar regressions before they
  ship. Closes the v1.5-era "M: snapshot tests on widgets" TODO.

### Fixed
- **Spawned panes now advertise `COLORTERM=truecolor`.** Reported by
  Gemini code review: `TERM=xterm-256color` alone doesn't tell apps
  that negotiate truecolor (bat, fzf, delta, lazygit, …) that the
  surrounding terminal can render 24-bit color, so they silently
  downgrade their palette to 256. portable-pty inherits the parent
  env so it usually leaks through, but "usually" depends on which
  terminal launched spyc. Set it explicitly in `pane::spawn_with_env`
  alongside `SPYC_CONTEXT` so panes are consistent regardless of host
  terminal. Background-task capture (TERM=dumb) is unaffected — it
  builds its own env and is non-interactive by design.

- **Switching pane tabs (`^a-n` / `^a-p` / `^a-1..9`) now pulls
  focus into the pane.** Reported: switching tabs from the
  file-list-focused state changed the active tab but kept focus
  on the file list — the next keystroke went to spyc, not the
  newly-active tab. Matches the existing behavior of `^a c`
  (new tab), which has always pulled focus.

- **Opening `?` no longer flickers the underlying pane back to
  live-pty / file-list rendering.** Reported as the polish
  follow-up to v1.50.1: the help overlay opens correctly and
  dismisses correctly, but for the lifetime of the help the
  underlying TopPane / LowerPane pager would visibly redraw
  with the *non-pager* content (live pty in the lower slot,
  file list in the top slot). The user saw text "jump" while
  help was up, even though dismissing landed back in the right
  place. Cause: `top_pager` / `bottom_is_pager` checks read
  `self.pager.mount`, which is `Mount::Overlay` while help is
  active — so the slots reverted to default rendering for those
  frames. Fix: when help is open, peek into `pager_help_stash`
  for the slot mount so the underlying pager keeps drawing in
  its slot, and the centered help overlay paints on top.

- **`?` from a non-Overlay pager now dismisses back to the same
  pager.** Reported: `D` opens a file in the top pane → `?` opens
  pager help → dismissing help dropped the user into a stale
  file-viewer overlay (or nothing), not back into the TopPane
  pager. Same root-cause regression for `^a-v` lower-pane
  scrollback. The `?` handler was pushing the pre-help pager
  onto `pager_history`, which silently filters out
  `no_history=true` views — and both v1.5 mounts set that flag
  intentionally (so `[b`/`]b` doesn't surface them). Fix: a
  dedicated `pager_help_stash` slot, exempt from the
  `no_history` filter, restores the pre-help pager verbatim
  (same content, same mount, same `pane_scroll` flag).

## [1.50.0] - 2026-05-08

The pager / task-viewer unification. The pager grew from "overlay
you summon" into "renderer you mount anywhere" — `^a-v` is a real
pager, `D` opens files in-pager, `:task-to-pane` and
`:pane-to-task` move pty hosts between display containers, MCP
socket discovery is project-scoped (no more cross-project
attachment), and a long tail of daily-driver UX fixes from
internal usage. See the [Unreleased] section above for the
collected changelog entries since v1.41.1.

### Fixed
- **`^a-v` scroll now keeps the bottom of the snapshot reachable.**
  Reported: pressing `k` after entering scrollback view collapsed
  the HUD off-screen and the view jumped; `gg` then `G` left the
  HUD missing. Root cause: `handle_pager_key` computed the
  viewport from `term_h * 92 / 100 - 2`, which is correct for the
  centered overlay but wrong for `Mount::LowerPane` (the lower
  pane slot is ~40 % of terminal height, not 92 %). The
  inflated viewport made `scroll_by`'s `scroll_max` clamp return
  a value smaller than the real maximum, so the pager refused
  to scroll into the snapshot's last lines (where the HUD lives).
  Fix: prefer the renderer's cached `last_viewport_h` — it's the
  real body-area row count from the most recent frame and is
  correct for every mount. Falls back to the heuristic only on
  the very first key event before the renderer has run.

- **`^a-v` snapshot now mirrors the live screen geometry.**
  Reported: opening pane scrollback via `^a-v` made text "jump"
  vertically and the latest pty output (e.g. claude HUD plugin
  paint) was missing from the snapshot. Two fixes:
  1. Drain pending bytes from the reader thread into the vt100
     parser *before* snapshotting, so output that arrived between
     the last render and the user pressing `^a-v` is captured.
  2. Stop trimming trailing blank live rows from the snapshot.
     The live screen often has the cursor mid-grid with empty
     rows below (a shell prompt at row 5 of 24, blank rows
     6..23). Trimming them anchored the pager at the cursor row
     and shifted content up vs. what was just on screen.
     Mirroring the screen verbatim makes `^a-v` feel like a
     frozen copy of the live pty.

- **`v` (edit) from a `LowerPane`/`TopPane` pager returns to
  the same slot, not a centered overlay.** Reported: editing
  from the lower-pane scrollback view, then quitting `$EDITOR`,
  re-opened the buffer as a centered popup (regression, the
  `mount` field was reset to default `Overlay`). `PagerReturn`
  now carries `mount` and `pane_scroll` across the round-trip,
  so the post-edit pager lands back in the original slot with
  the original Esc semantics.

- **`:pane-to-task N` (numeric arg form) now works.** Reported
  as "unknown command: pane-to-task 2". Phase 6c shipped only
  the no-arg form (active tab); the numeric arg form lets you
  demote a specific tab by 1-indexed number, matching the
  divider's `[1]` `[2]` labels. Out-of-range numbers flash a
  clear error.

### Added
- **`:pane-to-task` — demote the active pane tab to a background
  task.** v1.5 Phase 6c, the symmetric inverse of `:task-to-pane`.
  Same `PtyHost` moves between containers, the pty keeps running,
  no quit-and-respawn. Useful when a tab you opened is fine to
  let run quietly — push it to the background list with
  `:pane-to-task`, bring it back later via `:fg` or
  `:task-to-pane`. Round-trips with Phase 6b: same child PID
  through both transitions.
  - **Buffer recovery is empty start.** vim's `^z` parity:
    fresh output accumulates from the demote point, prior
    visual context is gone. Seeding the task buffer from the
    vt100 grid would erase color (grid is cells; task buffer
    is ANSI bytes). Acceptable — most uses are "I don't need
    to look at this for a while" not "I need a screenshot."
  - In-app `?` help row added under "Background tasks";
    FEATURES.md mention added.

- **`:task-to-pane [N]` — promote a backgrounded `!` task to a
  new pane tab.** v1.5 Phase 6b. Useful when an `!` task you
  started turns out to need persistent attention (a long-running
  `npm run dev`, a `cargo watch`, a `tail -F`) — promote it next
  to claude instead of shuttling through `:fg` / `^z`.
  - The pty keeps running through the transition. The host is
    moved from the task into a fresh `Pane`; we resize to the
    bottom-pane geometry, replay the captured buffer through a
    new vt100 parser so the tab opens with the same content the
    task viewer was showing, and SIGCONT the child if it was
    paused.
  - No-arg form promotes the most-recent task; numeric arg
    targets a specific id. Already-exited tasks don't promote
    (would just create a dead tab); flashes `task #N already
    exited; :fg to view its output instead` and leaves the
    task in the bg list.
  - The promoted tab inherits the task's TERM (`dumb`, set when
    the `!` capture spawned). Plain shells and SGR-color output
    render fine; alt-screen TUIs (vim, htop, lazygit) won't
    suddenly start working in the new tab — that's a property
    of the spawned process, not the wrapper.
  - If the task viewer was open on this id, it closes
    automatically (the task no longer exists in
    `background_tasks`).
  - `:fg` continues to work for the inverse "I just want to look
    at the buffer" case; `:task-to-pane` is the "I want this to
    keep going as a tab" case.

### Internal
- **v1.5 Phase 6a: shared `PtyHost`.** Pulls the pty kernel
  (master + writer + child + reader thread + event channel +
  `closed` / `exit_status` / `last_size` / `debug_dump`) out of
  `Pane`, `PendingCapture`, and `BackgroundTask` into a new
  `pane::pty_host` module. All three consumers shrink to a thin
  wrapper plus their own state (vt100 parser for `Pane`, flat
  byte buffer + lifecycle metadata for `PendingCapture` /
  `BackgroundTask`).

  Pure refactor — strict no-behavior-change goal, 594 tests
  still pass. The reader-thread protocol, debug-byte-dump, has-
  pending flag, exit-status harvesting, and SIGTERM-then-SIGKILL
  shutdown all match the pre-refactor paths exactly.

  **Side benefit:** `spawn_capture` now retains the master in
  the host, so backgrounded captures can be resized when the
  terminal resizes — pre-v1.5 they couldn't, because the master
  was dropped after extracting reader/writer. This was the
  blocker that made Phase 6b (`:task-to-pane`) impossible.

  3 new unit tests on `PtyHost`: `spawn_and_drain_echo`
  (round-trip a real subprocess), `resize_updates_last_size_and_coalesces`
  (geometry + coalescing), `process_id_is_some_after_spawn`.

### Fixed
- **Pane scrollback view (`^a-v`) opens cleanly — no jump, wrap
  on, borderless.** Three issues reported against the v1.41.29
  Phase-3 ship, fixed in one PR:
  - **No-jump initialization.** Used to set `scroll = lines - 1`,
    which puts the *last* line at the *top* of the viewport (so
    `[EOF]` showed at row 0 and the user had to scroll up). Now
    `pending_scroll_to_bottom` is set on open, and the renderer
    — which has the actual rect — calls
    `scroll_to_bottom(rect.height)` before drawing the first
    frame. Lands in the bottom window immediately.
  - **Wrap on by default.** Long lines (compiler errors, diff
    rows, log entries) now fold instead of truncating, since
    horizontal scroll isn't a thing in the pager. The pager's
    continuation-row blank-gutter behavior keeps alignment
    intact when line numbers (`l`) are toggled on.
  - **Borderless in `Mount::LowerPane`.** The pty has no border;
    the pager replacing it shouldn't either. Drawing the
    `Borders::ALL` block was eating two rows of usable content
    and visually disrupting the layout the user just had on
    screen. `full_width` mode already drew without a border;
    extended that to `LowerPane` mount.
  3 new unit tests cover the flag default, scroll-to-bottom
  with explicit viewport, and that LowerPane mount uses the
  rect verbatim.

### Added
- **Visual block (columnar) selection in the pager.** v1.5
  Phase 4 — vi's `^v` rectangle. From normal pager mode `^v`
  enters block visual; from line visual (`V`) `^v` upgrades
  in place, preserving anchor / cursor lines. `j` / `k` extend
  rows, `h` / `l` extend columns, all the existing pager
  motions (`g` / `G` / `^d` / `^u` / `^f` / `^b` / Page* /
  Space) still extend the row axis. The selection paints as a
  rectangle (cursor cell brighter; rest of the rect dimmer);
  `y` yanks the slice — each row contributes
  `chars[lo_col..=hi_col]` and rows shorter than the column
  range simply contribute fewer chars. The footer reads
  `-- VISUAL BLOCK --  L{lo}-L{hi} C{lo}-C{hi}  ({rows}×{cols})`
  so the dimensions are unambiguous before commit.
  `Esc` cancels; `^v` toggles back off; `V` from inside block
  drops down to line mode (vim parity). Wrap is forced off
  while block mode is active so the rectangle aligns to
  on-screen rows. `?` help updated.

  Caveats: column units are character-based (Unicode scalars),
  not display-width — so a wide CJK / emoji glyph counts as 1
  in the rectangle even though it paints as 2 cells. Vim does
  the same; full display-width-aware block selection is future
  work.

### Changed
- **`D` opens files in the in-app pager (top-pane mount), not
  `\$PAGER` as a pty overlay.** v1.5 Phase 5 — the in-app pager
  has been the more capable viewer for a while now (search, jump,
  visual range yank, syntax highlighting, markdown render, hex
  dump for binaries) and Phase 1's `Mount::TopPane` is the rail
  for landing it in the same screen slot the old overlay used.
  Bottom pane stays visible alongside; `^a-j` / `^a-k` flips
  focus between the in-app pager and the pty just like it did
  for the old overlay. `Esc` / `q` closes the pager and returns
  focus to the file list.
  - **Huge-file fallback:** files past `MAX_PAGER_BYTES` (5 MB)
    are still handed to `\$PAGER` as a top-overlay pty, because
    `less` streams from disk while the in-app pager loads the
    (already truncated) buffer into memory. Streaming wins for
    multi-GB logs.
  - Binary files use the existing hex-dump pager view (same as
    `Enter` / `d` does), not `\$PAGER`'s raw-byte spew.
  - The file-loading body of `Enter` / `d` and `D` is now
    shared via a new `App::build_pager_view_for_file` helper —
    truncation banner, syntax highlighting, markdown rendering,
    hex dump all behave identically across the two openers.

- **`^a-v` (pane scrollback view) is now a real pager.** First
  user-visible piece of the v1.5 unification. The old scroll
  mode was a flat byte-buffer view: `j` / `k` / `g` / `G` only,
  no search, no jump, no yank-by-range. Replace it with a
  `PagerView` mounted in the lower pane slot, fed by the new
  scrollback adapter (Phase 2). All the pager features come
  along for free: `/` search with `n` / `N`, `:N` jump,
  `V` visual line mode + `y` range yank to clipboard, `l`
  toggle line numbers (off by default — opening the pager would
  otherwise jump existing content rightward), `w` whitespace
  markers, `W` wrap toggle. `Esc` / `q` snaps the pty back to
  live and clears the divider's `[SCROLL]` indicator. The pty
  keeps running off-screen while the pager is up; output you
  miss while reading lands in scrollback for the next view.
  Alt-screen apps (codex, vim, htop, lazygit) still flash the
  "no scrollback" hint and skip opening — there's nothing to
  scroll back through and the app's own history viewer is the
  right tool.

### Internal
- **v1.5 Phase 2: scrollback adapter
  (`src/ui/scrollback.rs`).** New module bridges a pane's
  `vt100::Screen` (cell grid + bounded scrollback) into the
  pager's data model (styled `Vec<Line<'static>>`), so the
  Phase 3 `^a-v` rewrite can use the in-app pager — search,
  jump, visual-mode range yank, line numbers — over pane
  history. Walks the visible window backwards through scrollback
  by mutating `scrollback_offset` (clamped by `set_scrollback`),
  reading one page at a time. Original offset is restored
  before the function returns. Adjacent same-style cells merge
  into one span; trailing whitespace on each row trimmed; trailing
  blank lines dropped. 10 unit tests cover empty buffer, live-only,
  scrollback-then-live ordering, padding trim, offset restoration,
  page-walk chunking with non-multiple sizes, color preservation,
  same-style merging, sub-page scrollback, and zero-capacity
  scrollback. Made `pane::widget::cell_style` / `convert_color`
  `pub` so the adapter can reuse the existing vt100→ratatui style
  mapping.

  `#[allow(dead_code)]` on the module until Phase 3 wires the
  consumer; tests still exercise it.

- **v1.5 Phase 1: `PagerView::mount`.** The pager gets an explicit
  `Mount` field (`Overlay` / `TopPane` / `LowerPane`) so v1.5 can
  embed the same renderer into different slots (top pane for an
  in-app `D`, lower pane for `^a-v` scrollback) instead of always
  drawing a centered popup. Pure plumbing — every existing caller
  defaults to `Mount::Overlay` so nothing visible changes today;
  rect dispatch lives in a new `pager_inner_area` helper covered
  by 6 new unit tests. `TopPane` / `LowerPane` variants are
  `#[allow(dead_code)]` until Phase 3 retargets callers.

### Fixed
- **nvim / less / htop / lazygit cursor visible again inside spyc's
  pty panes.** Reported by Spencer: opening nvim via `V` (top
  overlay) or `^a-c` → `nvim` (new tab) showed an invisible cursor;
  `v` (full TTY suspend) was fine because the OS terminal owned
  the cursor. Root cause: the v1.41.18 alt-screen guard correctly
  stopped us from painting a reverse-block over nvim's cursor
  shape — but spyc hides the host cursor at startup
  (`main.rs::setup_terminal`) and nothing was telling ratatui to
  put it back at the child's vt100 cursor position, so alt-screen
  TUIs ended up with neither a spyc-painted block *nor* a host
  cursor. Now `App::render` calls `frame.set_cursor_position` for
  the focused pty pane (overlay if `!pane_focused`, bottom pane
  otherwise) at its vt100 cursor coordinates, gated on
  `!screen.hide_cursor()` so DEC ?25l still hides the cursor.
  Non-alt-screen panes keep the existing reverse-block as a
  high-contrast cue (the host cursor sits on the same cell, no
  conflict). Forwarding the child's *cursor shape* (beam vs.
  block) to the host terminal is a separate piece of work.

- **`^C` inside the pager is now contextual instead of leaking to
  the spyc-list status bar.** Reported with a screenshot: a `! find /`
  capture had finished (correct exit 130 from the original ^C), but
  every subsequent ^C while the user was still looking at the
  result printed `^C is not a quit binding` on the *background*
  spyc-list flash row — wrong screen for the notice. Now ^C in the
  pager dispatches contextually:
    - Task viewer + task running → `SIGINT` to the process group,
      flash `task #N: sent SIGINT` inside the pager (mirrors what
      ^C does in a normal terminal; child decides exit vs. trap).
    - Task viewer + task finished → flash `process already
      stopped` inside the pager.
    - Other pager views (file viewer, help, etc.) → flash `press
      Esc or q to close pager` inside the pager (^C-as-quit is
      muscle memory from `less`).
  The top-level `^C is not a quit binding` flash now also excludes
  the pager-open case, so it can never fire while a pager is up.

- **MCP socket discovery is now project-scoped — no more
  cross-project attachment.** Previously, when `$SPYC_MCP_SOCK`
  wasn't set (claude launched outside spyc's pane, env didn't
  propagate, or enterprise managed-mcp.json suppressed the local
  `.mcp.json`), `discover_live_socket` scanned every
  `~/.local/state/spyc/mcp-*.sock` on the host and returned the
  first one that connected — a claude in project A could silently
  attach to a spyc running in project B (or, with `$HOME` unset,
  even another user's spyc on a shared host). Wrong-context tools
  and file paths flowed through, with no log line saying so.
  Discovery now walks the caller's cwd toward the filesystem
  root, looking for `.spyc-context-<pid>.json` markers (each
  written by a running spyc rooted at that directory). The first
  ancestor with at least one marker is the project boundary; only
  those PIDs become socket candidates. A parent-dir spyc never
  shadows a child-dir spyc — locality wins. With no project
  match, discovery returns None and the stdio proxy falls back to
  read-only direct mode instead of attaching to the wrong host.
  Also tightened stale-socket cleanup: only delete on
  ConnectionRefused / NotFound, not on every connect error,
  so a transient EAGAIN/EMFILE doesn't race-delete a healthy
  peer's socket. 8 unit tests cover same-dir, walk-up, locality,
  multi-instance, no-match, and end-to-end no-attach behaviors.

### Changed
- **`/` and `=` are now case-insensitive substring matchers (was:
  case-insensitive prefix).** Reported as `env` not finding `.env`
  even though it's right there — the matcher anchored at the start
  of the name, so dot-prefixed files were unreachable without
  typing the leading dot. Substring fixes that:
  `/env` → `.env`, `.envrc`, `environment.toml`. Globs (queries
  containing `*`, `?`, `[`) are unchanged and still anchored, so
  `/env*` re-anchors at the start when that's what you want, and
  `*env*` is the explicit substring form. Same behavior change for
  the `=` filter prompt (it shares the matcher), so `=test`
  shrinks the listing to anything containing `test` in its name.
  `?` help and FEATURES.md updated.

- **`D` now opens the cursor file in `$PAGER` as a top overlay**
  (was: flash the date/time). Mirror of `V` for $EDITOR and a
  natural use of the focus-sharing overlay landed in v1.41.21.
  Workflow this enables: `D` on `docs/architecture.md`, `^a-j`
  into claude, work, `^a-k` to scroll the doc — without quitting
  less. The old date utility is still reachable via the typed
  command `:date`. `D` flashes an error on directories ("D: cannot
  page a directory") and when `$PAGER` is unset. Updated `?` help
  and FEATURES.md.

### Fixed
- **`;cmd` overlay no longer traps focus when a bottom pane is open.**
  Used to be: a `;`-style interactive command (`;less docs/foo.md`,
  `;vim`, `;htop`) was an unconditional key takeover — every key
  went to the overlay subprocess until it exited. With a bottom
  pane already running claude / zsh, that meant the user had to
  quit `less` just to glance at the lower pane. Now spyc meta keys
  (`^a`, `^w`, `^\`, F10) fall through to the chord resolver while
  the overlay is up, so `^a-j` / `^a-k` flip focus between the
  overlay and the bottom pane and `^a-c` / `^a-n` etc. still manage
  bottom-pane tabs. The overlay rendering tracks the focus state —
  unfocused overlays dim like any other unfocused pane — and the
  focus-switch flash says `focus: overlay` when the overlay holds
  the slot. New overlays steal focus on spawn so `;less` lands you
  in the pager directly. The user-visible workflow this enables:
  `;less docs/architecture.md`, `^a-j` into claude, do work, `^a-k`
  back to scroll the doc, repeat.

### Added
- **Pager visual line mode for range yank.** `V` in any pager view
  enters vi-style visual line mode: the anchor is set at the top
  visible line and `j` / `k` / `^d` / `^u` / `^f` / `^b` /
  `PageDown` / `PageUp` / `Space` / `g` / `G` / `Home` / `End`
  extend the cursor end (auto-scrolling when the cursor leaves the
  viewport). The selection is highlighted with the muted indigo
  cursor-bg-dim across the range and the active cursor row gets
  the brighter cursor-bg, so it reads like vi's visual cursor. The
  status footer shows `-- VISUAL --  L{lo}-L{hi}  ({n} lines)` so
  the range is unambiguous before you commit. `y` / `Y` yanks the
  inclusive range to the system clipboard via `pbcopy` and exits;
  `Esc` or `V` cancels without yanking. While the mode is active
  unrelated keys (`/`, `:`, `f`, `l`, `w`, etc.) are swallowed —
  exit visual mode first to use them, so a stray `/` doesn't
  silently reinterpret your selection mid-flight. Top-level `y`
  (yank source) and `Y` (yank visible) are unchanged outside
  visual mode. Also surfaced in the pager `?` help.

### Fixed
- **Built-in chord prefixes now beat user keybindings on the second
  key.** A user reported `^a-n` / `^a-p` flashing the pending
  indicator and then doing nothing — they had `n` / `p` bound
  elsewhere in `.spycrc`, and the resolver was consulting user
  bindings *before* checking whether a chord was already in flight.
  Same root cause for `]g` / `[g` (anyone with `g` user-bound),
  `H1`..`H9`, `yp` / `yf` / etc., `ma`..`mz`, `'a`..`'z`, `Wl` /
  `Wn` / `Wd`. The fix flips the precedence: when an explicit chord
  prefix (`^a`, `[`, `]`, `H`, `W`, `m`, `'`, `y`) is pending, the
  next key resolves the chord. The `g` chord keeps its previous
  behavior — bare `g` is also a vi motion fragment users may want to
  remap (`gd` / `gf` / etc. remain user-overridable). Top-level user
  bindings are unaffected.

### Changed
- **Upgraded vt100 0.15 → 0.16, ratatui 0.29 → 0.30, ansi-to-tui
  7 → 8.** The vt100 bump is the proper fix for the
  `screen.rs:934.unwrap()` panic that previously crashed spyc when
  closing nvim from inside zsh (caught defensively in v1.41.17;
  now resolved upstream). The transitive `unicode-width` pin
  forced the ratatui major bump along with it; ansi-to-tui needed
  to follow to a ratatui-0.30-compatible release. Net code change
  was small: vt100 0.16 moved `set_size` and `set_scrollback` from
  `Parser` to `Screen` (call sites adjusted via `screen_mut()`),
  and `Cell::contents` now returns `&str` directly instead of
  needing a borrow. The `catch_unwind` safety net from v1.41.17
  stays — any third-party parser can hit edge cases on rare
  escape sequences, and the cost is zero on the happy path.

### Fixed
- **vt100 parser panics no longer take spyc down.** A user reported
  nvim crashing the whole spyc process when closing it inside a zsh
  pane: `panicked at vt100/src/screen.rs:934: Option::unwrap() on a
  None value`. We're on vt100 0.15.2 (upstream is at 0.16.2 — an
  upgrade may resolve this and is worth doing separately), and 0.15
  has a known `unwrap()` deep in `screen.rs` for certain valid
  escape sequences (this one fires while parsing the exit-from-
  alt-screen byte stream after a specific scroll/cursor state). The
  defensive fix is a `catch_unwind` around the parser hot path: on
  panic we log via the debug log and replace the parser with a
  fresh one at the same dimensions and 10k-line scrollback. The
  user loses the in-pane screen state at the moment of recovery
  (the next render from the child repaints anyway), but spyc and
  every other pane stay alive. Even after a vt100 upgrade this
  safety net is worth keeping — any third-party parser can hit
  edge cases.

  Also flipped the release profile from `panic = "abort"` to
  `panic = "unwind"` — `catch_unwind` is a no-op under `abort`,
  so the recovery code only worked in dev builds before. Slight
  binary-size cost, fine trade for not crashing the user's session.
- **Pane cursor block no longer clobbers nvim's own cursor.** Spyc
  used to paint a reverse-block at the pty cursor position
  unconditionally (modulo `?25l`-hidden), which fought with TUI
  apps that draw their own cursor — most visibly nvim's beam in
  insert mode, where users saw a block when the app was clearly
  asking for a beam. The block is now skipped when (a) the pane
  isn't focused (user's eye is on the file list anyway, the block
  is just clutter), or (b) the child has switched to the alternate
  screen (any full-screen TUI: nvim / vim / less / htop / lazygit
  / claude in TUI mode renders its own cursor in its own shape).
  Plain shells / REPLs on the main screen still get the block as
  before — that visibility cue genuinely helps when no native
  cursor is rendered.
- **Huge directories no longer hang spyc.** A user reported entering
  a stale `/tmp/...` directory and having to kill the terminal to
  recover — every entry costs a `stat()` call plus a sort
  comparison, so a 1M-entry directory could spend minutes blocking
  the event loop. `Listing::read` now caps at 50,000 entries (new
  `MAX_ENTRIES` const) and flashes
  `listing capped at 50000 entries — directory has more` on chdir
  when the cap was hit. The cap is generous enough that real-world
  navigation directories (build trees, monorepos, even chubby
  `node_modules`) read in full; only pathological directories
  (message queues, log spools, runaway tmp) trip it. Also extracted
  `read_capped(dir, cap)` for unit testing without burning real
  time on 50k file stats.

### Changed
- **Two-character git markers distinguish staged from unstaged.**
  The left gutter now shows the full porcelain XY pair (column 0 =
  staged side, column 1 = unstaged side), mirroring `git status -s`.
  `M ` is staged-only, ` M` is unstaged-only, `MM` is partially
  staged + further edits, `R~` is staged rename + further unstaged
  edits, ` ?` is untracked. Each char carries its own color so the
  staged/unstaged halves are independently legible at a glance.
  Previously a single marker collapsed all three cases ("staged",
  "unstaged", "both") to one glyph, making the staged-vs-unstaged
  distinction invisible. Marker column was already 2 cells wide
  (was `~` + space) — no layout shift. Internally `GitFileStatus`
  is now a struct (`staged: Option<GitChange>`, `unstaged:`,
  `untracked: bool`) instead of a flat enum; new `GitChange`
  carries the per-side kind. 3 new parser tests cover the
  staged-only / partially-staged / conflict shapes.
- **Unfocused side dims so focus is obvious at a glance.** When the
  pane has focus, the file list above renders with `Modifier::DIM`
  on every non-cursor row; when the list has focus, the pty pane
  below renders with DIM on every cell. The cursor row's existing
  `cursor_bg_dim` treatment stacks on top so the highlighted row
  stays distinguishable in either state. SGR 2 lands as ~50%
  lightness on every supported terminal — no theme work or layout
  shift, just a clean visual cue for "input goes here vs. there."

### Fixed
- **Input dispatch hardening for fast typing.** Two defensive guards
  on the user-reported "switching panes input doesn't work when done
  too quickly" symptom:
  1. **Post-chord bounce-suppression** — a focus-switch chord
     (`^a-j` / `^a-k`) now stamps the chord-completing key. The
     next dispatch drops a same-key Press/Repeat within 60 ms,
     so a fast `^a-j` no longer leaks a stray `j` byte into the
     just-focused pane child (the `j` Press completes the chord,
     but the OS-level Repeat or a too-quick second Press would
     otherwise arrive with the new focus already active).
  2. **Stranded paste flash** — `Event::Paste` outside Prompting
     mode and without a pane open now flashes "paste ignored
     (N chars) — open `:` or `^\` to paste" instead of silently
     dropping. Some terminals wrap rapid keystrokes in bracketed
     paste, which would previously vanish; the flash makes it
     obvious. The Prompting and pane-open paths are unchanged.

### Added
- **`--key-trace` / `SPYC_KEY_TRACE` diagnostic switch.** Writes
  every key event + dispatch decision to
  `/tmp/spyc-key-trace-<ts>.log` with elapsed-since-start
  timestamps. Off by default; mirrors the `--debug` /
  `SPYC_DEBUG` pattern. Useful for users hitting hard-to-reproduce
  input bugs — flip it on, reproduce, ship the log.
- **`]g` / `[g` — cursor to next / previous git-changed entry.**
  Vim-style "next hunk" muscle memory for the file list. Walks the
  current directory's listing in either direction looking for the
  next file or dir whose git status is anything other than clean
  (`~` modified / `+` added / `?` untracked / `-` deleted /
  `>` renamed). Wraps around end-of-list so the chord can be held
  without thinking about direction. Flashes "no git changes in
  this directory" when there's nothing to jump to. Implementation
  is pure-domain (lives on `AppState`); 5 new unit tests pin the
  forward, backward, wrap, advance-off-current, and empty-listing
  cases. Reuses the same `git_files` map the listing markers
  consume, so detection is consistent with what the user sees.
- **`yf` — yank cursor file path (or all picks) to clipboard.** New
  binding in the `y`-prefix family. Yields absolute paths so the
  receiving shell resolves them correctly regardless of where the
  user pastes them. With picks active, joins them newline-separated
  for one-per-line consumption (`xargs`, `git restore $(pbpaste)`,
  etc.). Came from a real-world ask — "easy way to copy a file path
  for a one-off `git restore`" — that previously had to route
  through `!git restore %`.

### Changed
- **`?` help text discoverability.** Added the long-missing `%`
  substitution under the `!` row (`%` = cursor file or all picks,
  shell-quoted; `%%` = literal percent), so a new user looking at
  the help can find the substitution mechanism without having to
  read source or remember the spy heritage. Updated the pane
  default-command row to reflect the v1.41.7 precedence chain
  (`$SPYC_PANE_CMD` env > `[pane] default_command` config >
  `"claude"` fallback) — the prior text only mentioned the env var.

### Fixed
- **Pane child trees now exit cleanly on tab close and spyc quit.**
  `^a x` / `^a K` (close tab) and `Q` / `:q` / `^D` (quit spyc) used
  to drop the pane without signalling its child, leaving processes
  orphaned — most painfully `npm run dev` / `vite` / etc., where the
  whole `node` → `esbuild` → workers tree kept running and stayed
  bound to its dev-server port. New `Pane::shutdown(grace)` sends
  SIGTERM to the child's process group (negative PID — reaches the
  whole subprocess tree), waits up to 250 ms for voluntary exit,
  then escalates to SIGKILL and reaps. Wired into both
  `tabs.remove_at` (close-tab path) and the end of `App::run` (quit
  path). A backstop `Drop for Pane` SIGKILLs the process group on
  any path that bypasses the orderly shutdown (panic unwind,
  `?`-propagated error), so children never leak.

### Added
- **Codex MCP discovery via `.codex/config.toml`.** spyc now writes
  the codex equivalent of its `.mcp.json` to
  `<project>/.codex/config.toml` on startup, registering itself as
  a stdio MCP server in the `[mcp_servers.spyc]` section. The
  registration re-execs `spyc --mcp` and shares the same socket as
  the claude side, so a single server backs both agents. Same
  takeover semantics as claude — startup detection now checks both
  `.mcp.json` and `.codex/config.toml`, so a stale codex-only entry
  also triggers the takeover prompt. TOML splice is shape-safe (a
  malformed or invalid `.codex/config.toml` falls back to a clean
  rewrite rather than panicking).
- **`[pane] default_command` config key.** `^a c` (new pane tab)
  pre-fills its prompt with this command instead of the hardcoded
  `"claude"`. Precedence: `$SPYC_PANE_CMD` env var > config >
  `"claude"` fallback. The env var still wins so users can
  experiment per-shell without editing config; the new key just
  fixes the default for users who've switched to codex (or anything
  else) as their daily driver.
- **`gd` now matches what the `~` marker says.** `gd` was running
  bare `git diff` (working-tree-vs-index) and flashing
  "no unstaged changes" on rows the listing had marked dirty with
  `~`, because once you `git add` a file the diff lives in the
  index and unstaged is empty. `~` flags anything different from
  HEAD, so `gd` is now `git diff HEAD` — covers staged + unstaged
  + still folds in untracked-as-new — and the empty-flash now says
  "no uncommitted changes". `gD` (`--cached`) is unchanged for the
  "what would commit" view.
- **Alt-screen scroll-mode hint.** `^a v` against a full-screen TUI
  (codex, claude post-startup, vim, htop, lazygit) now flashes
  `scroll: on — alt-screen app, no scrollback (use the app's own
  history)` instead of the generic `(j/k nav, s save, Esc exit)`
  message. Alt-screen apps don't write to main-screen scrollback,
  so there's nothing for `^a v` to surface — the hint redirects the
  user to the app's built-in history viewer rather than letting them
  think scroll-back is broken. Detection via vt100's
  `Screen::alternate_screen()`. Single-screen apps (bash, plain
  shells) keep the old flash.
- **Codex session save/restore parity with claude.** `spyc -r` now
  resurrects codex panes with their conversation intact, the same
  way it has long done for claude. On quit, spyc sniffs the codex
  exit banner (`To continue this session, run codex resume <UUID>`)
  out of pane scrollback and stashes the UUID in the saved tab. On
  restore, codex tabs spawn directly as `codex resume <UUID>` —
  cleaner than claude's path because the CLI flag works for codex
  (no `/resume`-over-stdin dance). When no UUID was captured (e.g.
  the user killed the pane before exit), restore falls back to
  `codex resume --last`, which uses codex's own cwd-filtered
  most-recent picker.

  Plumbing: introduced `AgentKind` (Claude/Codex/Other) and renamed
  `SavedTab.claude_session_id`/`name` → `agent_session_id`/`name`.
  Older saves load via serde aliases and `effective_kind()` infers
  Claude when the legacy fields were set, so existing `spyc -r`
  flows continue working without migration. Session-picker tooltips
  now group by agent kind (`claude:foo (12345678), codex:abcdef12`).

### Changed
- **`CLAUDE.md` → `AGENTS.md`.** Renamed the project instructions
  file to the cross-tool standard. Recent Claude Code reads both
  names so behavior is unchanged. All references in repo docs
  (`ARCHITECTURE.md`, `CONTRIBUTING.md`, `DESIGN.md`, `ROADMAP.md`,
  `LAUNCH_PREP.md`, `BUGS.md`, `CHANGELOG.md`) and source comments
  updated.

### Fixed
- **`.mcp.json` with an unexpected shape panics startup.**
  `ensure_mcp_json` parsed the file, then unwrapped
  `.as_object_mut()` on both the top-level value and the
  `mcpServers` key. A file that was valid JSON but had the wrong
  shape (top-level array, top-level string, `mcpServers: []`) would
  panic instead of being safely overwritten. Now shape-checks each
  layer; falls back to a clean rewrite when splice isn't safe.
- **Pane `SPYC_CONTEXT` pointed at a path App never writes.**
  App writes one canonical `<start_dir>/.spyc-context-<pid>.json`,
  but `Pane::spawn` was recomputing the path from the pane's `cwd`
  via `context_path(cwd)`. When a pane spawned outside `start_dir`
  (e.g. in `PROJECT_HOME` or a subdir), Claude Code's direct-mode
  MCP fallback read a path nobody writes. `Pane::spawn` /
  `spawn_with_env` now require an explicit `context_path` parameter
  threaded through from `App`; all five call sites updated.
- **`TMUX` env race in `term_title` tests.** The two
  `wrap_*_tmux` tests both mutated process-global `TMUX` and could
  race under parallel execution (same flake family as the
  state-module tests). Both now hold `crate::state::env_test_lock()`.
- **`/...` then `n n n` in the help pager left the view stuck at the
  bottom.** The help overlay renders in two columns when the terminal
  is wide enough; in multi-column mode `scroll` is interpreted
  per-column (each column applies the same offset within its own
  chunk), but `scroll_to_match` was treating it as a global line
  offset. A match in column 2 produced a `scroll` value larger than
  `scroll_max` (= longest-chunk - viewport_h), got clamped to the
  bottom, and pinned every column at the end of its chunk — hiding
  the actual match. Now translates the match's global line index to
  a chunk-local offset before assigning to `self.scroll`. Single-
  column pagers behave unchanged. Pinned by a regression test.
- **`:fg` opened the pager scrolled to the top with the live tail
  off-screen.** Resuming a backgrounded `cargo build` (or any
  chatty task) showed an empty pager, or — once the next chunk
  landed — content scrolled to row 0 with the latest output
  invisible, so it looked like nothing was running. Root cause:
  `foreground_task`'s Running branch built the `PagerView` with
  `lines: Vec::new()` and only the streaming-tick repopulated it
  on the next chunk. Now seeded the same way `:task N`'s peek
  viewer is — render `task.buffer` into the pager and call
  `scroll_to_bottom_auto()` before handing the buffer to
  `pending_capture`.
- **Flaky test suite under parallel execution.** Several state-module
  tests (graveyard / harpoon / inventory / marks / sessions) and the
  shell-module tests mutate process-global env vars
  (`XDG_STATE_HOME`, `SHELL`) and raced when run in parallel,
  surfacing as random `NotFound` errors deep inside graveyard
  restores or wrong-shell-path assertions. `make check` was
  papered over with `--test-threads=1`; the CI Coverage step ran
  parallel and was failing intermittently. Added a single shared
  `crate::state::env_test_lock()` mutex; each affected test holds
  it for its full body. 15 consecutive parallel runs now pass.
- **`^C` swallowed when pane is focused.** The "^C is not a quit
  binding" footgun-guard fired before the pane-forward path, so
  pressing `^C` while focused on a child process (zsh, a long-running
  command, etc.) flashed the hint instead of delivering `0x03` to the
  child. The guard now skips when the pane has focus, so `^C` reaches
  the running process as it does in any normal terminal. Other control
  codes (`^T`, `^D`, …) were already forwarded; only the `^C` case
  carried the extra guard.
- **Git markers leaked across same-name files.** A clean root-level
  file rendered with a `~` (modified) marker when a sibling-named
  file in a subdirectory was actually the dirty one (e.g. root
  `AGENTS.md` clean, `content-acquisition/AGENTS.md` modified →
  both rows showed `~`). `git_file_statuses` collapsed every
  porcelain entry to its basename and indexed the map by that
  basename, so the deep file's status overwrote the root row.
  The basename now only goes into the map for files actually in
  the listing directory; deeper entries still mark the parent
  directory. The parsing logic is now split out as
  `parse_porcelain_statuses` with unit tests pinning the rule.
- **`:undo` and `:graveyard` returned "unknown command".** State's
  command dispatcher routes a fixed list of names to App's
  terminal-touching arms; `undo` and `graveyard` weren't on it,
  so they hit the unknown-command fallthrough before App's
  handler could see them. Added both to the punt list.

### Added
- **Graveyard — soft-delete recovery for `R` and undo support.**
  Files removed with `R` (and items expelled from inventory) now
  go to a per-user **graveyard** at `$XDG_STATE_HOME/spyc/graveyard/`
  instead of being hard-deleted. Each entry is a `<uuid>.json` +
  `<uuid>.tar.zst` pair — single regular files and directory trees
  use the same shape. zstd compression keeps the payload small;
  tar's `HeaderMode::Complete` preserves mode bits (executable,
  group-write), mtime, and best-effort UID/GID; `set_overwrite(false)`
  on restore refuses to clobber existing files. xattrs / ACLs /
  macOS resource forks are NOT preserved (out of scope for v1).

  Recover via:
  - **`gy`** — open the graveyard view (newest first)
  - **`:undo`** — restore most-recent entry to its original path
    (one-shot escape hatch)
  - Inside the view: **`p`** restore-to-cwd, **`P`**
    restore-to-original, **`dd`**/**`x`** purge entry to system
    trash, **`Z`** purge ALL (single-key confirm),
    **`Esc`**/**`gy`** close.

  When the graveyard exceeds 500 MB at startup, the **oldest
  entries cascade to the system trash** (FIFO) until under the
  cap, with a flash reporting the count. Net pipeline:
  `R` → graveyard (compressed, undo-able from spyc) → system
  trash (uncompressed, browsable from the OS) when the cap is
  hit. New deps: `tar`, `zstd`, `trash`.

### Changed
- **`R` confirm prompt now surfaces directory blast radius.**
  Previously: "remove N file(s)?". Now pre-walks any selected
  directory to count files inside and shows
  "remove DIR (recursive, N file(s)) + M file(s)? (y/N)" so a
  reflexive `y` doesn't accidentally drop a build tree. Cost is
  microseconds on any sane subtree.
- **Inventory's `move_to_graveyard` now uses the new tar.zst
  schema** so the graveyard is uniform — one read/write code
  path. Pre-v1.41.0 paired `<uuid>.json` + `<uuid>.dat` graveyard
  entries are silently ignored by the new reader (the graveyard
  is a transient soft-delete cache; major version bumps may lose
  recovery state).

### Fixed
- **Pager: trailing logical lines were unreachable when long lines
  wrapped.** A file with N logical lines (some long enough to wrap
  to multiple visual rows) showed "Bot" before all content was
  visible — `scroll_max` capped the scroll using logical-line count,
  so wrapped portions of earlier lines consumed the visual budget
  and pushed the last few lines off-screen. Reported on
  `docs/spyc-logo.svg` (154 lines, several path elements wrap; lines
  151-154 never appeared at "Bot"). Fix: `scroll_max` now walks
  lines from the end summing visual rows when wrap is on, using a
  `last_body_w` cache the renderer updates each frame. Wrap-off
  pagers and multi-column pickers keep the original logical-line
  bound. Two regression tests in `pager::tests`.

### Added
- **Quick Select — labeled overlay picker (`^a u`).** Borrowed from
  WezTerm's mode of the same name. Press `^a u` to scan the visible
  pane for URLs, file paths, git SHAs, IPv4 addresses, and any
  user-defined regex patterns; each match is overlaid with a 1- or
  2-letter label. Lowercase label → yank to clipboard. **Uppercase
  label → "open" intent**, dispatched per match kind:
  - URLs → system handler (`open` / `xdg-open`)
  - Paths → cursor-jump in spyc
  - Git SHAs → `git show <sha>` in the in-app pager
  - Custom patterns with a `url = "https://.../{}"` template →
    fill `{}` with the match, then `open`/`xdg-open`
  - Other kinds → fall back to yank with a flash hint

  Scroll mode "just works": the picker scans exactly the user's
  visible viewport, so scrolling up to a Claude reply and pressing
  `^a u` labels the URLs in *that* reply.

  Custom patterns in `.spycrc.toml`:
  ```toml
  [[scan.patterns]]
  name = "jira"
  regex = '[A-Z]+-\\d+'
  url = "https://tripstack.atlassian.net/browse/{}"
  ```
  Bad regexes are dropped at config load with a debug-log note;
  one typo never blocks startup.

### Fixed
- **`gf` / `gF` now honor scroll mode.** Previously
  `goto_file_from_pane` temporarily forced scrollback to its
  deepest position, so a path the user had scrolled up to was
  ignored — the scanner read a different region of history. Now
  routes through a new `Pane::pickable_text()` helper: when
  scrolling, scans exactly the visible viewport; when live, the
  prior 200-line behavior is preserved so paths in large diffs
  that just scrolled past the bottom are still findable.

### Added
- **Harpoon — per-project pinned working set.** Inspired by
  ThePrimeagen's neovim plugin: a small (max 9), hand-curated,
  ordered list of file or directory pointers for muscle-memory
  navigation. `Ha` appends the cursor file/dir, `Hx` removes,
  `H1`..`H9` jumps to slot N (chdirs to the parent and places the
  cursor on the file; chdirs *into* the slot if it's a directory).
  `Hh` opens a modal menu where `K`/`J` reorder, `dd` deletes
  (vim-style two-key arming), `Enter`/`1`-`9` jumps. `=h` (or
  `:limit h`) filters the listing to harpoon entries plus all
  their ancestor directories — so `foo/` shows up when viewing
  `src/` and `src/foo/bar/hello.c` is harpooned, letting you drill
  in. Persisted at `$XDG_STATE_HOME/spyc/harpoon/<basename>.<hash>.toml`
  per `PROJECT_HOME`; auto-saved on every mutation. Two PROJECT_HOMEs
  with the same basename can't collide (filename is keyed by an
  absolute-path hash).

### Changed
- **`H` is no longer an alias for "jump to `$HOME`".** It's now the
  harpoon chord prefix. The `~` key and the Home key still jump
  to `$HOME`; `gh` still jumps to `PROJECT_HOME`. This frees the
  natural `H1`..`H9` muscle-memory bindings without three-keystroke
  chord overhead.

### Added
- **`=git` / `=g` limit filter.** Shows only entries appearing in
  `git status` (modified, staged, untracked, deleted, renamed,
  conflicted) plus parent directories that contain such files
  (so changed subtrees stay navigable). The filter stays live as
  the 1Hz git poll updates `git_files`. `=` clears it like any
  other filter. Outside a git repo (or with no changes), applying
  `=git` flashes "not in a git repo (or no changes)" instead of
  silently showing an empty list. Requested via BUGS.md; the
  harpoon-style pinned-set part of that request is split out for
  a deeper design pass.
- **Pane zoom (fullscreen toggle).** `^a z` (and `^w z`) zooms the
  bottom pane: the file list collapses to 0 rows and the pane fills
  the middle region between status and prompt. Tmux-style — the
  status bar and prompt row stay visible, focus is forced into the
  pane on zoom-on and the prior focus is restored on un-zoom. The
  user's preferred `pane_height_pct` is preserved untouched so the
  prior split returns exactly on un-zoom. A `[ZOOM]` tag renders in
  the divider while active. `^a +` / `^a -` are no-ops while
  zoomed (with a status flash). Closing the pane (`^a \`) clears
  the zoom flag. Requested by a daily user.

## [1.37.2] - 2026-04-30

### Fixed
- **Shell aliases and rc-file PATH entries now work in `:!cmd`,
  `;cmd`, and pane prompts.** Previously, spyc spawned `sh -c <cmd>`
  regardless of the user's `$SHELL`, and even setting `$SHELL` would
  not have helped: aliases / functions live in interactive rc files
  (`.zshrc`, `.bashrc`) which non-interactive shells don't load.
  A user running `:!gemma` (where `gemma` is an alias for a local
  `llama.cpp` invocation) got `sh: gemma: command not found`. Now
  `spawn_capture` and `Pane::spawn` resolve `$SHELL` and pass `-i`
  to shells that source rc files in interactive mode (`zsh`, `bash`,
  `fish`, `ksh`, `mksh`); POSIX `sh` / `dash` get plain `-c` since
  they don't read rc files in `-i` mode anyway. Helper lives at
  `shell::user_shell_invocation`. FEATURES.md updated to describe
  the new behavior. Tradeoff: heavy `.zshrc` / `.bashrc` setups
  (oh-my-zsh banners, p10k init) may now print init noise into
  capture pagers; well-behaved rc files gate that behind
  `[[ -t 1 ]]` / `[[ $- == *i* ]]` and stay quiet.

### Changed
- **`make install` now defaults to `~/.local/bin` (no sudo).** The
  Makefile's `PREFIX` defaults to `$HOME/.local`. To install
  system-wide, override: `sudo make install PREFIX=/usr/local`. The
  install target prints a hint if `~/.local/bin` is not on `$PATH`.
  README, INSTALL.md, and AGENTS.md updated to reflect the new
  recommended flow.

### CI / Tooling
- **`bitbucket-pipelines.yml` now calls `make check`** instead of
  inlining its own cargo commands. The Makefile's `test` target
  runs with `--test-threads=1` to serialize XDG_STATE_HOME-mutating
  state-module tests; CI was inlining `cargo test --all-targets`
  without that flag and hitting the race, leaving CI red on `main`.
  Calling `make check` keeps CI and local on the same exact gate.
- **Pipeline `target/` cache** added alongside the existing cargo
  cache, both keyed on `Cargo.lock` + `rust-toolchain.toml`.
  Should drop pipeline compile time materially on cache hits.
- **Code-tree `cargo fmt --all` sweep** to clear pre-existing
  formatting drift in `pager.rs`, `markdown.rs`, `fs/ops.rs`,
  `line_edit.rs`, and `app/mod.rs`. No behavior changes.
- **Pre-commit hook** in `scripts/git-hooks/pre-commit`. Install
  with `make install-hooks` — runs `make check` on every commit so
  fmt / clippy / test failures surface locally instead of ~10 min
  later in CI. Bypass with `git commit --no-verify` if you must.

### Security
- **`SECURITY.md`** — honest posture doc covering threat model,
  supply-chain controls, build/install trust chain, and known
  caveats. Avoids signing/SBOM theater for an internal tool with
  no published binary distribution channel.
- **`cargo deny check`** replaces `cargo audit` in CI. Same advisory
  coverage, plus license allow-listing (only the SPDX identifiers
  present in the actual dep graph), source allow-listing
  (crates.io only — no `git = ...` deps), and bans (yanked /
  multiple-major-versions). Configuration in `deny.toml`; ignored
  advisories list a documented reason each.
- **`--locked` on every `cargo` invocation** in the Makefile and
  pipelines (test, lint, all release builds, coverage). Prevents a
  CI-time `Cargo.lock` drift from silently pulling fresh transitive
  deps; failures are loud.
- **`make dist-sign`** scaffolding for GPG-signed checksum files.
  Not used today (we don't ship prebuilt binaries); SECURITY.md
  documents the intentional gap so a future signing rollout has a
  ready landing spot.

## [1.37.3] - 2026-04-30

### Fixed
- **Stray reverse-video cell when running TUI apps in the lower pane.**
  spyc's pane renderer was unconditionally painting a reverse-video
  block at `vt100`'s cursor position, even when the child had hidden
  the cursor with DEC `?25l`. Apps like lazygit, less, and vim hide
  the cursor and draw their own selection highlight, so the overlay
  showed up as a stray "glitch" cell sitting on top of the child's
  UI -- typically wherever the child had last left its (now-hidden)
  cursor. Now suppressed when `screen.hide_cursor()` is true.
  Reported via lazygit dog-fooding in the lower pane
  (`src/pane/widget.rs`).

## [1.37.1] - 2026-04-30

### Fixed
- **Stale `+` (or any) git marker after commit/push now self-heals
  within ~1s.** The `notify`-driven FSEvents watch on `.git/` would
  occasionally miss the `.git/index.lock` → `.git/index` atomic
  rename that happens on every commit -- macOS FSEvents has a known
  soft spot for inode replacement, so the listing dir's `+` / `~` /
  `?` markers (and the top-bar branch/dirty string) could stay
  stale until the user changed directories. Added a 1Hz safety-net
  poll: when `git_info` is set (we're in a repo), the run loop
  re-runs `git_status` and `git_file_statuses` once per second and
  diffs the results against the live state. Diff-aware -- only
  bumps `list_generation` and requests a repaint when something
  actually changed, so idle dps stays at 0. Watcher path is
  unchanged; this is a backstop, not a replacement.

## [1.37.0] - 2026-04-29

### Added
- **`:pause [N]` / `:resume [N]` for backgrounded tasks.** The
  top BIGGER-pile request: pause/resume execution so you can
  swap networks, free CPU, or just stop an over-eager build to
  focus on something else. Implementation sends `SIGSTOP` /
  `SIGCONT` to the task's *process group* (negative pid via
  `libc::kill`), so subprocess trees (e.g. `make → cc → ld`)
  all halt together rather than just the direct child. No-arg
  forms target the most-recent task; numeric arg targets a
  specific id. Same UX shape as `:fg [N]` / `:task [N]`.
- **`S` / `C` keybindings inside the task viewer** (`gB` /
  `:task N`) — Stop and Continue, the hand-on-keyboard
  shorthand for `:pause` / `:resume`.
- Divider glyph `[N⏸]` for paused tasks (mixed in with the
  existing `[N+]` / `[N●]` / `[N✓]` / `[N✗]`).
- `:fg` on a paused task **auto-resumes** before re-attaching
  the streaming capture, so the user doesn't get a frozen
  foreground pager.
- `paused: bool` field added to `BackgroundTask`.

### Fixed
- Cleared the "pause and resume execution of backgrounded
  tasks" entry from BUGS.md BIGGER pile (it's the feature this
  release adds).

## [1.36.0] - 2026-04-28

### Changed
- **Markdown table cells now wrap to multiple visual rows**
  instead of truncating with `…`. v1.35.0 capped each column at
  ≤24 chars and `…`-truncated overlong content; the result was
  unreadable on tables like the README key-binding tables where
  the `Action` column has full sentences. Now: each cell is
  wrapped at its column width via the same `word_wrap_ranges`
  routine the paragraph renderer uses (par-style word boundaries
  with hard-break fallback for unbreakable tokens). The visual
  height of a row is the max wrap-rows across cells; cells that
  wrap to fewer rows are padded with blank lines so the column
  borders stay aligned. Per-span styling (`**bold**`, `*italic*`,
  `code`) preserved across wrap boundaries via `slice_spans`.
  `truncate_spans_to_width` is gone -- nothing called it after
  the cell renderer switched to wrap.

## [1.35.2] - 2026-04-28

### Fixed
- **Streaming `!cmd` capture pager auto-tail uses real viewport
  height** instead of a hardcoded 40 rows. Repro: run a long
  capture (`!cargo build` or similar) on a tall terminal; the
  pager would render ~63 rows tall but the auto-tail would only
  scroll enough to show the last 40 lines -- the bottom of the
  pager filled with `~` markers while content sat in the upper
  half. The "go to top + bottom" workaround that fixed it
  manually was just `G` reading the actual viewport height.
  Same bug affected `:fg` resume of backgrounded tasks. Fix:
  cache the rendered viewport height on `PagerView.last_viewport_h`
  (a `Cell<u16>`) during render; tick-loop auto-tail reads it
  via the new `scroll_to_bottom_auto()`. Falls back to 40 on
  the very first frame before any render has run -- harmless
  since the next frame replaces it.

## [1.35.1] - 2026-04-28

### Fixed
- **`w` / `b` / `e` / `dw` / `cw` / `^W` now respect punctuation
  boundaries**, matching vim's default `iskeyword`. Previously
  the line editor's word motions split only on whitespace, so
  `foo-bar` was treated as a single word and `dw` from position 0
  deleted the whole thing. Now `dw` on `foo-bar` deletes only
  `foo` -- the same behavior any vim user expects when editing
  paths, kebab-case identifiers, flag values, URLs, etc.
  Implemented via a `CharClass` helper (`Word` / `Punct` /
  `Space`); word motion stops at any class transition. 4 new
  unit tests cover `w`, `dw`, `cw`, `^W` against `foo-bar`.

## [1.35.0] - 2026-04-28

### Added
- **Markdown tables now render with proper borders.** v1.26.0
  punted on tables ("tables fall through unstyled — out of scope
  for v1"); the result was cell text getting mashed together as
  inline text. Now: tables get a real renderer with box-drawing
  borders (`┌┐└┘├┤┬┴┼─│`), bold headers, dim slate borders.
  Column widths computed from natural cell content, capped at
  24 chars per column, then proportionally trimmed so the whole
  table stays inside the 80-col content budget. Cells longer
  than their allotted width truncate with `…`. Inline emphasis
  inside cells (`**bold**`, `*italic*`, `code`) is preserved
  thanks to the same span-styling pipeline the rest of the
  renderer uses. 2 new tests cover border rendering and
  overlong-cell truncation.

## [1.34.1] - 2026-04-28

### Fixed
- **`/` "no matches" flash now renders inside the pager**, not on
  the spyc file-list status bar underneath. The pager search
  routed its empty-result feedback through `state.flash_error`
  which lives on the file-list pane; the message would appear
  *behind* the pager overlay where the user wasn't looking. Now
  it's set on `view.flash` so the pager's own title-bar flash
  (teal-on-amber, per v1.27.4) carries it.

## [1.34.0] - 2026-04-28

### Changed
- **History popup is now opened by `Esc Space`** (vi prompts) and
  `Esc <Space>` for `J` (also a vi prompt as of v1.33.0), not
  the v1.31.0/v1.32.0 double-Esc. The user found double-Esc
  fights Esc's "back out of something" muscle memory; Space in
  Normal mode reads more naturally as "expand into the bigger
  view." Space is unused in our line editor's Normal mode, so
  no binding conflict.
- Sequence on every prompt with history: type → Esc (enters
  Normal) → Space (opens kind-specific popup). Single Esc no
  longer escalates -- it just toggles Insert↔Normal as standard
  vi.

## [1.33.0] - 2026-04-28

### Changed
- **`J` is now a vi-line-editor prompt** (was a "simple prompt"
  with append-only buffer editing). User feedback: after pulling
  up a history entry with j/k or Up/Down, you should be able to
  *tweak* it before submitting -- e.g. recall `~/src/spyc` and
  append `/src` before Enter. The simple prompt only supported
  end-of-buffer typing + Backspace, so cursor positioning, word
  motion, mid-buffer delete etc. were all unavailable.
- Promoting J to vi-line-editor unifies its key handling with
  `!` / `;` / `:`. All four prompts now share the same model:
  - First Esc: Insert → Normal mode
  - Normal-mode `j`/`k` (or Up/Down anywhere): walk history
  - Second Esc (in Normal): open the kind-specific popup
    (`show_jump_history_popup` for J, `show_history_popup` for
    the others)
  - Full vi line editing: h/l motion, w/b/e word motion, x/D/C
    delete operators, A/I/0/$ position, etc.
- `browse_mode` field removed from `Prompt` (was added in v1.32.0
  to fake a vi-mode for the simple prompt; redundant now that J
  has the real thing).
- All four history-push routings already worked from v1.28.0;
  removed the duplicate Submit-push for Jump from the
  simple-prompt path that v1.29.3 added (handle_vi_prompt_key
  picks it up via history_for_prompt).

## [1.32.0] - 2026-04-28

### Added
- **`J` now matches the vi-prompt double-Esc pattern.** First Esc
  on a `J` prompt enters "browse mode" (no popup yet); j/k
  walks history inline like vi Normal-mode j/k. Second Esc
  (already in browse mode) opens the full jump-history popup.
  Typing any non-nav character drops out of browse mode and
  resumes normal text editing. Backspace-on-empty and `^C`
  unconditionally cancel.
  - Reverses v1.29.0/v1.29.2's behavior where Esc on an empty J
    buffer opened the popup directly. Now Esc always enters
    browse mode first; the popup is the second tap. Consistent
    with the `!`/`;`/`:` model shipped in v1.31.0.
  - `browse_mode: bool` field added to the `Prompt` struct so
    simple prompts can carry the same kind of mode-state vi
    line editors track internally.

## [1.31.0] - 2026-04-28

### Added
- **Double-Esc opens the history popup in vi prompts** (`!`,
  `;`, `:`). First Esc puts the line editor in vi Normal mode
  (existing behavior); second Esc (when already in Normal)
  opens the `!?` popup. j/k inside the popup browse, Enter
  submits, ^D deletes, q/Esc closes. Mirrors J's Esc-on-empty
  popup, generalized to any vi prompt.

### Known limitation
- The popup currently shows shell history regardless of which
  vi prompt opened it. For `!`/`;` that's correct; for `:`
  (command line, which has its own `command_history` since
  v1.28.0) the popup will show the wrong bucket -- mostly
  empty for users who don't also use `!`. Fixing requires
  parameterizing the popup helper to take a kind and routing
  the popup's submit back to the matching dispatch
  (`dispatch_command` for `:`, etc.). Tracked in ROADMAP.

## [1.30.0] - 2026-04-28

### Added
- **`Up` / `Down` in the `J` prompt cycle through jump history
  inline** (replaces the buffer with the prev/next entry, just
  like `:` and `!` already do). v1.28.0's changelog claimed this
  worked but the wiring was wrong twice over: history-push lived
  in the vi-prompt branch (which `J` doesn't use), and Up/Down
  was never registered in the simple-prompt branch at all. Now
  the simple-prompt path has its own Up/Down handler that walks
  `jump_history.prev` / `next`, with `reset_nav` on cancel /
  submit so the next `J` opens fresh at the most-recent entry.

### Fixed
- **`j` / `k` work in the jump-history popup.** v1.29.0's popup
  set `picker_cursor` but never wired the j/k → picker_move
  arms; the pager dispatch doesn't have a generic picker-nav
  fallback, each popup type has to wire its own. Added them to
  the `pending_jump_history` block so j/k navigate as expected
  (matches the session picker's pattern).

## [1.29.3] - 2026-04-28

### Fixed
- **`J` submissions actually push to `jump_history` now.** Same
  bucket of bug as v1.29.2: v1.28.0's history-push lived in
  `handle_vi_prompt_key`'s Submit arm, but `J` is a simple
  prompt that submits via `handle_prompt_key`'s Enter branch
  and never reached the editor flow. Result: every J jump
  silently *didn't* persist, the popup forever flashed "jump
  history is empty," Up/Down had nothing to recall. Push moved
  into the simple-prompt Enter handler, gated on
  `PromptKind::Jump`. New jumps now persist; the v1.29.0
  popup is finally reachable with content.

## [1.29.2] - 2026-04-28

### Fixed
- **`Esc` on empty `J` prompt actually opens the jump-history
  popup now.** v1.29.0 put the check in `handle_vi_prompt_key`,
  but `J` is a "simple prompt" (no line editor) and dispatches
  through `handle_prompt_key`'s simple-prompt branch — never
  reaching the vi-editor path where my check lived. Moved the
  check into the simple-prompt branch ahead of the generic
  Esc-cancel arm.
- **`^C` in `J` (and other simple prompts) cancels** — same
  fix shape as v1.29.0's `^C` handling, but at the
  `handle_prompt_key` simple-branch level so it actually
  reaches J / search / pattern-pick etc.

## [1.29.1] - 2026-04-28

### Changed
- **Jump-history popup uses `x` to delete** instead of `^D`,
  matching the inventory view's `x` for "remove this item."
  Consistency with the rest of the spyc surface. The `!?` shell-
  history popup keeps `^D` because it has a vi line-editor
  active, where `x` is already taken by the editor; the jump
  popup has no editor so `x` is unambiguously "delete entry."

## [1.29.0] - 2026-04-28

### Added
- **`Esc` on an empty `J` prompt opens a jump-history popup.**
  Scrollable list of every jumped-to path, newest first. `j`/`k`
  navigate, `Enter` chdirs to the cursored path (and pushes it
  to the top of MRU so the next browse surfaces it), `^D` deletes
  the entry from history, `q`/`Esc` closes. Esc on a *non-empty*
  J buffer still cancels normally -- only the empty-buffer case
  switches to the popup, since there's nothing to throw away.
- **Option+Enter sends a newline to the pane on terminals that
  support the kitty keyboard protocol.** `setup_terminal` now
  pushes `KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES`
  before entering the TUI; modern terminals (Ghostty, Kitty,
  WezTerm, foot, recent Alacritty, iTerm2 with the experimental
  flag) report `Option+Enter` as an unambiguous `Alt+Enter`
  KeyEvent. Old Terminal.app silently ignores the request -- on
  that one, users still need "Use Option as Meta key" in their
  profile preferences. Also broadened `pane::input::encode_key`:
  *any* modified Enter (Alt, Ctrl, Shift, Super/Meta/Hyper) now
  folds to `\n` so weird per-terminal modifier reports all
  produce the multi-line newline Claude expects.

### Fixed
- **`^C` in a `:` / `J` / `!` / `;` prompt cancels** instead of
  flashing the "use Q to quit" hint. v1.27.1's hint was the right
  thing in normal mode but wrong in prompts where vi muscle
  memory wants `^C` ≡ `Esc`. The hint now skips Prompting mode;
  `handle_vi_prompt_key` intercepts `^C` and routes to
  `cancel_prompt`. Capture mode still forwards `^C` to the child
  as 0x03 (sudo / ssh prompts unaffected).

## [1.28.0] - 2026-04-28

### Added
- **`J` (jump to path) gets its own persistent history.** Up /
  Down in the J prompt now cycle through previously-jumped
  destinations, persisted to
  `$XDG_STATE_HOME/spyc/jump_history`. Tab-completion + frecency
  hits still work as before; this is a parallel surface for
  "take me back to that thing I jumped to yesterday."

### Fixed
- **`:` and `!` no longer share a history bucket.** Real-world
  repro: type `!make sync-all` (a shell command), later type `:`
  and press Up to recall something — the buffer surfaces
  `make sync-all`, you submit it, spyc fires "unknown command:
  make sync-all" because `:` is the vim-style command line, not
  a shell. Now `:` has its own `command_history` file
  (`$XDG_STATE_HOME/spyc/command_history`) and the four buckets
  stay fully isolated: shell (`!`/`;`), pane-tab cmd/cwd, jump,
  and command-line.

## [1.27.4] - 2026-04-28

### Changed
- **Pager flash messages now render in teal-on-default** instead
  of inheriting the amber title color. Real-world miss: the
  `truncated at 5000 lines · press p for full file in $PAGER`
  notice on a capped file rendered in the same amber as the
  filename, looking like part of the title; users (me) didn't
  read it as a separate help notice. Now the title stays amber,
  the flash segment renders in teal + BOLD with a thin space
  buffer on each side -- visually clear that "this part is a
  notice, not part of the filename." Same treatment applies to
  every flash (yank confirmations, save confirmations, "no
  source file" warnings, etc.).

## [1.27.3] - 2026-04-28

### Fixed
- **`^C` in `p` → less now interrupts less cleanly** (rather than
  appearing to be ignored). v1.27.2 stopped spyc from dying on
  Ctrl+C, but the no-op-handler approach left spyc and the child
  sharing a process group, so signals went to both. less *did*
  receive SIGINT but interactions between two processes seeing
  the same FG-group signal led to "less seems to miss the
  signal" symptoms (race-y disposition handling, signal mask
  ambiguity, etc.). Fix: proper Unix job control around
  `run_child_in_foreground`:
  - Child spawned with `process_group(0)` ⇒ becomes leader of a
    new process group (PGID == child PID).
  - After spawn, `tcsetpgrp(stdin, child_pid)` makes the child's
    group the foreground group of the controlling tty. Now `^C`
    / `^\` from the kernel go to the child *only*.
  - On wait completion, `tcsetpgrp(stdin, our_pgid)` restores
    spyc as the FG group; SIGTTOU (raised on a non-FG-group
    process calling tcsetpgrp) is now ignored permanently in
    `install_signal_handlers` so the restore call doesn't
    suspend spyc itself.
  - Same shape that bash/zsh use to launch foreground commands.
    Less, vim, and any other child now get clean signal
    delivery and behave exactly as they would in a normal
    terminal.

## [1.27.2] - 2026-04-28

### Fixed
- **`^C` in a `p`/`v`/`;` takeover no longer kills spyc.**
  Real-world repro: `p` opens `less` on a huge file; `G` jumps to
  end and triggers a long line-count; user hits `^C` to abort the
  count → less *quits entirely* AND spyc exits. In a normal
  terminal less would just stop counting and stay open.
  - Root cause: spyc runs in raw mode (kernel `ISIG` disabled,
    `^C` arrives as a key event). When suspending for the
    `p`/`v`/`;` takeover, raw mode is restored to canonical, and
    `^C` from the tty driver is delivered as `SIGINT` to the
    *whole foreground process group* — which is spyc's process
    group, since the child inherited it. Both processes get the
    signal: less handles it gracefully (interrupt the count, stay
    open), spyc dies on the default disposition. The tty session
    leader exits → kernel `SIGHUP`s remaining processes → less +
    sh die too. From the user: "spyc died on ^C in less."
  - Fix: install no-op handlers for SIGINT and SIGQUIT in spyc at
    startup. spyc receives the signal, ignores it. Per POSIX
    `execve(2)`, custom handlers are reset to `SIG_DFL` in the
    child, so less / vim / etc. receive the signal with normal
    disposition and handle it themselves. (Pure `SIG_IGN` would
    inherit across exec, breaking the child's signal handling --
    that's why a custom no-op handler is the right shape.)

## [1.27.1] - 2026-04-28

### Fixed
- **Truncation now flashes the `p` hint immediately on open.**
  v1.27.0 added a banner row at the *end* of the truncated content,
  but if the file's the first 5000 lines and the user doesn't scroll
  to the bottom, they'd never see the escape hatch. Now: a flash
  message ("truncated at N lines · press p for full file in
  $PAGER") appears in the title bar the moment a truncated view
  opens, alongside the existing footer banner.
- **Pager-help (`?`) `Esc` now dismisses just the help, not the
  underlying pager.** Before: pressing `?` pushed the active pager
  into history, opened help; pressing `Esc` then closed the help
  *and* dropped you back to the file list, requiring `[b` or `gp`
  to reopen what you were viewing. Now: `Esc` / `q` on the
  help overlay pops the previous pager from history and restores
  it as active. Help is also flagged `no_history = true` so it
  can't accidentally land in the buffer-history stack.
- **^C in spyc-normal mode flashes an explicit hint** instead of
  silently doing nothing. Real-world repro: `p` opens `$PAGER`,
  user hits ^C to abort a struggling `less`, comes back to spyc
  thinking ^C may have been "captured." Now the flash makes the
  contract explicit: `^C is not a quit binding — use Q (or :q) to
  quit, Esc to cancel modes`. Capture mode still forwards ^C to
  the running child (sudo/ssh prompts behave normally).

## [1.27.0] - 2026-04-28

### Added
- **`p` (in pager) — open in `$PAGER` (full-screen takeover).**
  Mirrors `v` / `$EDITOR`: resolves `$PAGER` (default `less`),
  suspends the TUI, runs the external pager on the current
  file, resumes spyc on quit. The right tool for full traversal
  of huge files, interactive `less`-style search, or piping
  through marks. Buffer pagers without a source path (`!cmd`
  output, `:grep` results) flash "no source file (try `s` to
  save first)" instead.

### Fixed
- **Pager no longer OOMs on huge files.** Previous behavior was
  `read_to_string(path)` + syntect over the whole content, which
  built a `Vec<Line<'static>>` with millions of styled spans on
  multi-MB CSVs/logs -- pager state ballooned to ~50× file size
  in worst cases. Now: files above 5 MB load only the first 5000
  lines (plain text, syntect skipped — that's the dominant memory
  amplifier). Title gets a `⚠ truncated · X MB` suffix; a banner
  row at the end of the truncated content points at the new `p`
  binding for full-file viewing. Markdown rendered-mode also
  skips for truncated files since rendering half a doc looks
  weird (broken refs, half-closed code fences).
- 3 new `read_truncated` tests cover under-cap, over-cap, and
  exact-cap-boundary cases.

## [1.26.3] - 2026-04-28

### Fixed
- **`!cmd` captures now advertise `TERM=dumb` instead of
  `xterm-256color`.** The capture pager only renders ANSI SGR
  colors and CR/LF intelligently; cursor positioning, alt-screen,
  and mouse codes get stripped or render as garbage. Lying about
  vt100 capability meant `!less foo`, `!vim foo`, `!htop` etc.
  would switch into alt-screen TUI mode and either freeze the
  capture or write unrenderable cursor games into the pager.
  `TERM=dumb` is the canonical "nothing fancy" signal:
  TUI programs refuse to run as TUIs (they dump to stdout or
  print a friendly error and exit cleanly), which is exactly
  what we want for capture mode. `;cmd` (foreground in the top
  pane) remains the path for genuine TUI programs.
  `FORCE_COLOR`, `CLICOLOR_FORCE`, and `COLORTERM=truecolor` are
  kept so tools that respect those (cargo, eza, bat, ripgrep)
  keep producing colored output despite `TERM=dumb`.

## [1.26.2] - 2026-04-28

### Added
- **`Y` (capital) yanks the *visible* pager content** to the
  clipboard. Lowercase `y` still yanks the source (the POLA
  default). Most useful with the Markdown viewer in rendered
  mode: `Y` gives you back the styled-but-plain rendering
  (headings with `#`, bullets, blockquote rules, 80-col wrap)
  that you can paste into chat or a doc, without having to
  toggle to source first. In all other contexts (regular files,
  capture pagers, `:grep` results) `y` and `Y` are identical
  because the visible text *is* the source. Flash text
  distinguishes the two ("yanked source" vs "yanked visible")
  so you know which one fired.

## [1.26.1] - 2026-04-28

### Changed
- **Markdown content wraps at 80 cols (par-style).** The renderer
  now word-wraps paragraphs and list items at 80 visual columns
  inside `src/ui/markdown.rs` itself (not via the pager-level wrap),
  preserving per-span styles across break points and dropping
  trailing whitespace. List-item continuation rows get a hanging
  indent that matches the bullet width, so wrapped text aligns
  under the item content rather than under outer-level bullets.
  Code blocks pass through unwrapped (their formatting matters).
  Blockquote content wraps inside the rule prefix (78 col content
  + 2 col `┃ `). The pager pane stays full-width as before; only
  the content body is bounded.
- **Line-number gutter and inline `code` are no longer washed out.**
  Both used `status_suffix + DIM` which left them barely legible
  against dark backgrounds. Line numbers drop the DIM modifier
  (`status_suffix` alone is plenty subtle); inline code switches to
  `theme.take` (teal) — semantically reads as "code" and contrasts
  cleanly with body text.

### Added
- 4 new markdown tests: long-paragraph wrap, list-item continuation
  indent, word-wrap range breaks, hard-break fallback.

## [1.26.0] - 2026-04-28

### Added
- **Markdown viewer with source ↔ rendered toggle.** `.md` /
  `.markdown` files now open in *rendered* mode by default --
  headings styled, lists with bullets, fenced code blocks
  syntect-highlighted by language, blockquotes with a left rule,
  links rendered with the destination URL appended, inline
  bold/italic/strikethrough preserved. Press `m` in the pager to
  toggle to the syntect-highlighted source view, `m` again to
  flip back. Non-Markdown files flash "not a markdown file" on
  `m`. Both views are pre-computed at file-open so the toggle is
  instant.
  - Yank (`y`) and save (`s`) always operate on the *source*
    regardless of view mode -- POLA: yanking a README should give
    you back the markdown text, not the styled rendering.
  - Search / `n` / `N` match the *active* rendering: what you see
    is what you find. Toggle to source first if you need to grep
    for raw markdown syntax.
  - Scroll resets to top on toggle (rendered/source line counts
    differ; preserving an absolute index would land somewhere
    arbitrary).
  - Tables, images, and embedded HTML are out of scope by design;
    tables fall through unstyled, images render as `[image: url]`
    placeholders, raw HTML renders as dim text.
  - Built on `pulldown-cmark` + a small `src/ui/markdown.rs`
    renderer (~370 LOC). 7 unit tests cover heading prefix,
    paragraph flow, bullet list, blockquote rule, fenced code
    block fences, link rendering, and extension detection.

## [1.25.0] - 2026-04-28

### Added
- **Pager line wrap is back, done properly this time.** v1.21.6
  removed `Paragraph::wrap` because ratatui's wrap hard-breaks
  unbreakable tokens (paths, log lines) mid-character and the
  line-number gutter didn't carry across continuation rows --
  visible misalignment like `Builde$.cs` on long paths. New impl
  pre-computes visual-width chunks ourselves with per-span style
  preservation: long lines wrap cleanly at viewport width, wide
  CJK characters and emoji count as 2 cols (same as ratatui's
  layout), continuation rows get a blank gutter so wrapped pieces
  visually align with the source line's indent, and the `$`
  end-of-line whitespace marker stays on the actual end of the
  source line (last wrapped piece). Default ON for content
  pagers (file viewers, `:grep`, `!cmd` capture, task viewer);
  explicitly OFF for the `F` finder picker where each source line
  must map 1:1 to a selectable row. Toggle: `W` (capital) in the
  pager. 5 unit tests cover hard-break, span splitting, wide
  chars, and zero-width edge case.

## [1.24.2] - 2026-04-28

### Changed
- **Custom-code reduction sweep.** Continuing the v1.24.1 jiff
  swap, replaced four more hand-rolled implementations with
  established crates after a code survey:
  - **`fs/ops.rs` — uid/gid/localtime via `uzers` + `jiff`**.
    Deleted ~70 lines of `unsafe` `getpwuid_r` / `getgrgid_r` /
    `localtime_r` libc FFI plus a duplicated date-formatter.
    `format_local_time_from_unix` now uses
    `jiff::Timestamp::from_second(..).to_zoned(system).strftime`.
  - **`state/inventory.rs` — `make_id` via `uuid::Uuid::now_v7`**.
    The previous "simple UUID-like" hex-of-nanos generator could
    collide on rapid yanks; UUIDv7 is time-ordered with random
    suffix, no collision risk.
  - **`app/mod.rs` — ANSI stripping via `strip-ansi-escapes`**.
    Replaced ~40 lines of hand-rolled CSI/OSC parsing with the
    BurntSushi-adjacent crate. Kept the spyc-specific
    `strip_crlf` 3-pass normalizer (taking the *last* CR-frame
    on a line is a deliberate UX choice).
  - **`sysinfo::epoch_secs` / `epoch_nanos`**. Six files were
    each spelling out
    `SystemTime::now().duration_since(UNIX_EPOCH)...` -- now they
    call shared helpers backed by `jiff::Timestamp::now()`.

  Net: -~110 LOC, two new tiny deps (`uzers`, `strip-ansi-escapes`,
  `uuid`), one less `unsafe` block, one less custom date algorithm.
  All 456 tests pass serially.

## [1.24.1] - 2026-04-28

### Changed
- **Date formatting moved to `jiff`.** Replaced the hand-rolled
  Howard Hinnant `civil_from_days` algorithm in `sysinfo.rs` with
  `jiff::Timestamp::now().to_zoned(UTC).strftime(...)`. Same
  output (`YYYY-MM-DD HH:MM:SS UTC`), one less algorithm we have
  to maintain. `jiff` joins the existing BurntSushi crates we
  already depend on (`grep-regex`, `grep-searcher`, `ignore`).

## [1.24.0] - 2026-04-28

### Added
- **Project-wide search MCP exposure (M3 of project-wide search).**
  Four new tools, all gitignore-aware where applicable, all
  PROJECT_HOME-scoped (cwd fallback if no project root):
  - `search_paths(query, [limit])` — fuzzy filename search via the
    same `ignore` walker + `nucleo-matcher` ranking the `F` picker
    uses. Returns a JSON array of repo-relative paths, fzf-style
    ranked. Default limit 100, max 1000.
  - `search_content(pattern, [limit])` — content search via the
    same embedded ripgrep matcher `:grep` uses (smart-case, binary
    files skipped). Returns a JSON array of `{path, line, col,
    text}`. Default limit 200, max 5000.
  - `search_picks(pattern, [limit])` — content search restricted
    to the user's currently-picked files. **Uniquely spyc-shaped**:
    picks are TUI multi-select state Claude can't see otherwise,
    so this is the only way to grep the user's intended subset.
  - `search_inventory(pattern, [limit])` — content search across
    the persistent inventory cache (yanked-into-cache files that
    survive sessions). Lets Claude grep accumulated "interesting
    files" without leaving the conversation.
- 3 new MCP roundtrip tests (search_paths, search_content,
  search_picks). 3 new fs::grep tests (search_to_vec cap,
  search_files explicit-set scoping, invalid-regex error).

## [1.23.3] - 2026-04-28

### Fixed
- **`:grep` no longer scrambles tab-separated content.** Real-world
  repro: searching `tarzan` in tripstack_platform turned hits in
  `postcodes.txt` (a TSV file) into garbled overlapping text --
  `Tarzana    California` rendered as `rzCliforn aorniarnCA`. Cause:
  ratatui counts `\t` as zero-width via `unicode-width`, but
  terminals expand it to ~8 columns, so ratatui's position
  tracking drifts from the terminal's actual cursor and content
  visibly overlaps. Fixed in `sanitize_line`: tabs now expand to
  the next 4-column boundary (chosen over 8 to keep result lines
  compact, since most paths are already deep).

## [1.23.2] - 2026-04-28

### Fixed
- **`:grep` pager gutter no longer jitters mid-scan.** The line-
  number gutter width is computed each frame from
  `ilog10(view.lines.len())`, so as results streamed in the gutter
  widened from 1→2→3→4 chars at every power-of-10 boundary -- and
  every existing visible row shifted right by one column at each
  step. Visible content also realigned weirdly when the user
  toggled `l` mid-stream. Fixed by adding `line_count_hint` to
  PagerView; streaming views (currently `:grep`) seed it with the
  result-count cap so the gutter is sized for the worst case from
  the start. Also: `:grep` now defaults to **line numbers on**
  (was off) -- the row index is the most useful column for
  navigating result lists.

## [1.23.1] - 2026-04-28

### Fixed
- **`:grep` no longer corrupts the terminal on binary files.**
  Real-world repro: running `:grep test` in a workspace with
  tracked `.docx`, `.dll`, `.jar`, `.pdf` files dumped raw bytes
  (NULs, ESCs, backspaces) into the pager, scrambling colors and
  cursor positioning. Two fixes:
  - Searcher now uses `BinaryDetection::quit(0)` -- ripgrep's
    default. The first NUL byte in a file aborts the search of
    that file, so binary blobs are skipped.
  - Matched-line text is sanitized before display: control bytes
    (everything < 0x20 except tab, plus DEL) are replaced with
    `·`, CR/LF trimmed, and lines wider than 400 chars truncated
    with `…`. Catches sourcemap blobs, base64-inlined assets, and
    text files that happen to contain ANSI escapes.
- Also added `:grep` to the AppState command passthrough list so
  the prompt parser routes it to the terminal-touching arm; without
  it, `:grep test` flashed "unknown command".

### Added
- 2 new tests: binary-file skip behavior and `sanitize_line` length
  cap + control-byte filter.

## [1.23.0] - 2026-04-28

### Added
- **`:grep <pattern>` — project-wide content search (M2 of project-
  wide search).** Embedded ripgrep matcher (`grep-regex` +
  `grep-searcher`, the BurntSushi crates ripgrep itself uses), no
  subprocess. Walks `PROJECT_HOME` (or the listing dir as fallback)
  honoring `.gitignore`, smart-case by default. Matches stream into
  a pager as `path:line:col: text` -- the same shape `gf`/`gF`
  already understand from pane output, so jumping from a hit into
  the file is free. Same multi-repo-aware walker as the `F` finder:
  pass 2 picks up sibling-clone subrepos the outer `.gitignore`
  excluded. Capped at 5000 matches; refine the pattern if you hit
  it. Pattern errors flash inline before opening an empty pager.
  Power users with custom `~/.ripgreprc` or fancy flag combinations
  can still drop down to `! rg foo` for ripgrep's full surface.
- 8 unit tests cover smart case, gitignore honored, sibling-clone
  descent, invalid-regex error, and receiver-drop cancellation.

## [1.22.2] - 2026-04-28

### Fixed
- **`F` finder descends into sibling-clone subdirs that the parent
  repo's `.gitignore` excludes.** Real-world repro:
  `~/src/tripstack_platform` is a git repo whose `.gitignore` has
  entries like `book-org/`, `content-acquisition/`, etc. -- not
  because the user doesn't want to see those files, but because
  those subdirs are *separate clones* (each with its own `.git`)
  living inside the parent dir. Pass 1 of the walker (gitignore-
  aware from the parent) correctly skipped them, but the user
  expects `F` to find files anywhere checked out under the
  workspace. Now the walker runs a second pass: when the start
  root is itself a git repo, it scans for nested `.git/`
  directories that pass 1 missed and walks each as its own
  ignore root (with `parents(false)` so the outer repo's
  gitignore doesn't bleed in). Each subrepo's own gitignore is
  still honored within its tree.

## [1.22.1] - 2026-04-28

### Changed
- **`F` finder walks on a worker thread.** v1.22.0 walked
  synchronously on F-press, blocking the picker open for ~100-200ms
  on large monorepos. The walker now runs on a background thread
  and streams candidate batches (256 paths each) into the picker
  via an `mpsc::channel`. The picker is interactive immediately
  (the user can start typing before the walk finishes), and the
  candidate count + ranked results live-update as batches arrive.
  Title shows "scanning…" while the walk is in progress; flips to
  the final count when done. Closing the picker drops the receiver,
  which makes the walker exit cleanly on its next `tx.send`.

## [1.22.0] - 2026-04-28

### Added
- **`F` project-wide fuzzy filename finder.** First milestone of
  the project-wide-search ROADMAP entry. New key in the file list
  walks `PROJECT_HOME` (or the listing dir as fallback) honoring
  `.gitignore` via the `ignore` crate, ranks candidates against
  typed input with `nucleo-matcher` (basename hits outrank
  parent-dir hits, fzf-style). Up/Down move selection, Enter
  chdirs to the matched file's parent and places the cursor on
  it; Esc cancels. Walk runs lazily on open (no persistent
  index); 100K-file cap so a monorepo doesn't load the whole
  kernel into RAM. Subsequent milestones (`:grep` content search,
  MCP `search_paths` / `search_content` / `search_picks` /
  `search_inventory` tools) remain on the ROADMAP.

## [1.21.7] - 2026-04-27

### Fixed
- **Git status markers on parent-directory rows update on
  subtree changes.** Previously, adding/modifying a file in a
  subdirectory (e.g. `docs/foo.md` while sitting at the repo
  root) didn't update the `docs/` row's git marker -- you had to
  navigate into the subdirectory before the change registered.
  Two pieces: (1) the `notify` listing watch was `NonRecursive`,
  so subtree events never reached the loop; (2) `is_listing_path`
  only matched the dir itself or direct children, so even a
  recursive watch's events would have been rejected. Now: watch
  is `Recursive`, and `is_listing_path` accepts anywhere under
  the listing dir while keeping `.git/` carved out (only `index`
  and `HEAD` direct children count, so background gc / pack /
  refs activity doesn't cascade into needless `git status`
  subprocesses). The 500ms trailing debounce already in place
  bounds the cost on noisy subtrees. macOS FSEvents handles
  recursive watches at the OS level (cheap); Linux inotify
  needs a watch per subdir, which can hit
  `fs.inotify.max_user_watches` on enormous monorepos.

## [1.21.6] - 2026-04-27

### Fixed
- **Single-column pager truncates long lines instead of wrapping.**
  The `!cmd` / task-viewer / file-view pager used
  `Paragraph::new(...).wrap(Wrap { trim: false })`, which made
  ratatui hard-break long unbreakable words (paths, log lines)
  mid-character; continuation rows don't carry their own line-
  number gutter, so the `$` whitespace marker landed mid-row and
  the gutter accounting drifted on subsequent rows ("Builde$.cs"-
  style mismatches in long `git log` output, especially with
  `w` toggled on). Behavior now matches the multi-column path
  and `less -S`: clip at the right edge. Yank / save / search
  operate on `view.lines`, so the full untruncated content
  remains available regardless of how the visual rendering
  clips.

## [1.21.5] - 2026-04-27

### Fixed
- **`!cmd` capture pager strips stray ASCII control bytes** (NUL,
  SOH, backspace, vertical tab, form feed, etc.) that ansi-to-tui
  used to pass through to ratatui. Real-world repro: a long
  `git log` whose commit-message rendering emits `\x01` (SOH)
  before each conflict-list line. The host terminal swallowed the
  byte but ratatui's width accounting didn't, drifting the rest of
  the rendered line (`Buil$er.cs`-style misalignment with `w` on).
  `strip_crlf` gained a third pass that filters 0x00-0x08,
  0x0b-0x0c, 0x0e-0x1a, 0x1c-0x1f, 0x7f while keeping `\t`, `\n`,
  and `\x1b` (ESC for ANSI sequences). Same fix path covers the
  task viewer.

## [1.21.4] - 2026-04-27

### Fixed
- **`!` captures no longer launch a sub-pager.** `git log`, `man`, and
  any tool that probes `isatty(stdout)` and defers to `$PAGER` would
  detect our slave PTY as a real TTY and invoke `less`, which then
  took the PTY hostage waiting for keystrokes *inside* spyc's
  pager. `spawn_capture` now sets `PAGER=cat`, `GIT_PAGER=cat`,
  `MANPAGER=cat` in the child env so the tools dump directly and
  spyc's pager wraps the whole result. Foreground (`;`) commands
  and pane tabs are unaffected -- they should keep paginating
  since the user owns the TTY there.

## [1.21.3] - 2026-04-27

### Fixed
- **Pasting into `!` / `;` / `:` prompts now splices at the cursor**
  instead of appending to the end of the line. The bracketed-paste
  handler used to `push_str` to the prompt buffer regardless of
  where the cursor was; now, when the prompt has a vi line editor
  attached, it calls a new `LineEditor::insert_str(&str)` that
  inserts each char at `cursor` and advances. The canonical
  `Prompt.buffer` is then synced from the editor's text. Simple
  prompts (search, mkdir, file/dir name) still append since they
  have no cursor concept. Lets you `!` `git ` ⏎-paste-back-from-`!?`
  history-Esc-`b` (move back a word)-paste-mid-cursor without
  having to retype.

## [1.21.2] - 2026-04-26

### Fixed
- **`!` capture and task viewer collapse bare `\r` progress-bar
  updates to the last frame.** `git pull` / `npm install` / `cargo
  build` use bare carriage return (no newline) to overwrite
  progress on the same line; `ansi-to-tui` doesn't process `\r`,
  so we were rendering every frame side-by-side as one super-wide
  line. `strip_crlf` gained a second pass: for each `\n`-delimited
  segment, keep only the bytes after the *last* `\r`. Live
  streaming reads the latest frame each tick, and the saved view
  shows the final clean line. ANSI sequences never embed bare
  `\r`, so the byte-level pass is safe. Five new tests cover the
  passes individually and combined.
- **Task viewer no longer shows `[EOF]` while the task is still
  running.** `build_task_viewer_for` sets `view.streaming` based
  on `TaskStatus::Running`, and the per-tick refresh now fires on
  Running → Exited transitions (not just on new bytes), so the
  title and `[EOF]` marker keep up with reality when a task
  quietly finishes mid-view.

## [1.21.1] - 2026-04-26

### Added
- **`gp` reopens the most-recently-closed pager buffer** from the
  file list. Pairs with `gB` for "go to bg-task viewer" -- both pop
  the most-recent thing of their kind. When no buffers are in
  history, flashes "no buffers in history" instead of doing nothing.

### Changed
- **New `Background tasks & buffer history` help section** groups
  `^Z`, `:fg`, `g B`, `:task N`, `[t]t`, `g p`, `:bprev`, `:bnext`,
  `[b]b`, plus the divider-glyph legend (`[N+]`/`[N●]`/`[N✓]`/`[N✗]`)
  in one place. The `g B` and `:task N` bindings used to be tucked
  inside `Shell-out & commands` next to `:fg` -- easy to miss.

## [1.21.0] - 2026-04-26

### Added
- **Task viewer (`gB`, `[t`/`]t`, `:task N`).** A peek view into a
  backgrounded shell task's buffer that doesn't take ownership the way
  `:fg` does. From the file list, `gB` opens the most-recent task in
  the viewer; from inside any pager `[t`/`]t` cycles through bg tasks
  by id (wraps around). `:task N` jumps to a specific task. While the
  task is running, the viewer's content auto-refreshes from the live
  buffer; the title shows `running ({Xs})` / `exit 0 ({Xs})` etc.
- **Task viewer → buffer history promotion.** When you close
  (Esc / `q`) a task viewer for a task that has *exited* and that
  you've actually viewed, spyc snapshots the current rendered view
  into the buffer-history stack and removes the task from the bg
  list. `[b` from any subsequent pager walks back to the snapshot.
  Running tasks never auto-promote -- they stay in the bg list until
  exit + view.

### Changed
- **Help overlay no longer pollutes buffer history.** Hitting `[b`
  after closing the help could surface stale help content; help is
  now flagged `no_history` so it's skipped on close.
- **`[b`/`]b` at the edge of history keeps the current pager open.**
  Previously, hitting `[b` at the start (or `]b` at the end) silently
  closed the pager because the current view was consumed before the
  empty-stack case was checked. Now the pager stays put with a flash
  ("no older buffers" / "no newer buffers"); same fix for
  `:bprev` / `:bnext`.

## [1.20.2] - 2026-04-26

### Changed
- **Background tasks render in the pane divider, not the status-bar
  suffix.** Right-aligned, growing leftward, with a distinct color
  family (blue/teal/green/red) so the numbering doesn't visually
  collide with pane tabs (yellow/amber, left-aligned). Glyphs:
  - `[N+]` running, output arrived since you last `:fg`'d (teal)
  - `[N\u{25cf}]` running, quiescent (blue)
  - `[N\u{2713}]` exited cleanly (green)
  - `[N\u{2717}]` non-zero exit / killed / crashed (red)
  Per-task `has_unread_output` flag flips true when bytes arrive
  during the bg drain, false on `:fg` -- so `+` is a real "go look
  at this" cue, not just "still alive". When the pane is hidden
  (no divider rendered), the old `bg:N\u{25cf}` status-bar suffix
  is the fallback. If too many tasks to fit on the divider,
  oldest are dropped first; newest stay visible at the right.

## [1.20.1] - 2026-04-26

### Fixed
- **`:fg` no longer flashes "unknown command: fg".**
  `AppState::dispatch_command` whitelists which colon-commands fall
  through to App's terminal-touching arms (where the v1.20.0 `:fg`
  implementation lives); `fg` wasn't on the list, so the command was
  rejected inside AppState before App ever saw it. Added `fg` and
  `fg <N>` to the passthrough list. `^Z` to background was unaffected.

## [1.20.0] - 2026-04-26

### Added
- **Background tasks (M1) -- `^Z` to background, `:fg` to resume.**
  Long captured commands (`!cargo test`, `!find ...`) no longer lock
  you out of spyc. Press `^Z` while a streaming `!` capture pager is
  open to send the task to the background; reader thread keeps
  draining output into a per-task buffer (head-truncated at 1 MB).
  `:fg` (no arg) resumes the most-recently-backgrounded task; `:fg N`
  targets a specific id. Round-trip semantics:
  - Still-running tasks resume as a streaming pager seeded with
    everything captured so far; the original task id is preserved
    across `^Z` -> `:fg` -> `^Z` cycles.
  - Already-exited tasks resume as a static pager titled
    `! cmd — exit 0 (43s)` and are removed from the background list
    on view (one-shot).
  - A task that completes while in the background fires a flash
    `task #N: cmd — exit 0 (43s)`.
  - Status-bar suffix shows `bg:N●M✓` (N running, M completed).
  - Quit confirmation counts backgrounded running tasks alongside
    pane-tab processes.
  - Already in a foreground task and `:fg` is hit? Error-flash
    `already in a foreground task — ^Z to send to background first`
    (no silent swap).

  M2-M4 (`:bg` overlay, `!&cmd` direct-launch, polish) remain on the
  ROADMAP.

## [1.19.1] - 2026-04-26

### Changed
- **`q` no longer quits** -- it's now reserved for a future vim-style
  macro recording feature (already on the roadmap as `qa ... q ... @a`).
  Pressing `q` flashes a hint ("q reserved for future macro recording
  -- Q or :q to quit") instead. Quit is still bound to `Q`, `^D`, and
  `:q`. Motivation: an accidental `q` was easy to fat-finger when
  switching from vim contexts and produced the most destructive
  possible outcome (silent quit). Help overlay updated accordingly.

## [1.19.0] - 2026-04-26

### Changed
- **`L` long listing rewritten as an aligned table.** One header row
  + one data row per file with columns: inode, mode (symbolic),
  octal, links, owner, group (resolved via `getpwuid_r` / `getgrgid_r`),
  size (human), bytes, 512B blocks, mtime, atime, ctime, birth, name.
  Symlinks render as `name -> target` in the NAME column. Column
  widths are computed once across the whole selection so everything
  aligns. Renders inside the standard centered pager (90% width, top
  edge in the usual place), not full-screen — UX consistency with
  every other pager surface.
- **Pager `fit_to_content` mode.** New flag on `PagerView` /
  `PagerRequest` that shrinks the box from the bottom: same x, y, and
  width as the standard centered pager, but height = lines + borders
  + status row (capped at 92% of the terminal). Line-number gutter is
  suppressed since it's noise for short summaries. Long listing opts
  in so a single-file table (or even a 5-row directory listing)
  doesn't sit inside a 92%-tall frame.

## [1.18.6] - 2026-04-26

### Fixed
- **Captured shell (`!cmd`) pager no longer bleeds tail of
  longer lines through shorter ones.** spawn_capture runs the
  child under a pty whose slave has `ONLCR` on by default, so
  the child's `\n` becomes `\r\n` on the master side. The
  literal `\r` survived into our buffer; when ratatui rendered
  a line followed by a shorter line, the terminal interpreted
  the `\r` as carriage-return and the new line overlaid only
  as far as it was long, leaving the tail of the prior line
  visible. (`make help` in `~/src/system_setup` was a great
  repro — short lines following a long URL line.) Now we
  normalize `\r\n` → `\n` before feeding the buffer to
  `ansi_to_tui`. Standalone `\r` is preserved so in-place
  progress-bar updates still work.

## [1.18.5] - 2026-04-26

### Fixed
- **Trailing debounce on watcher refresh.** The previous
  debounce fired 500ms after the *first* event in a burst, which
  meant chained git operations (`git add && git commit && git
  push`) would have the refresh subprocess run during a
  transient mid-burst state — sometimes returning `M  BUGS.md`
  (staged but not committed) instead of clean. Once that
  transient sample landed, no further `.git/index` rename event
  fired (the commit's later side-effects only touched lockfiles
  we filter out), so the refresh never re-ran and the top bar
  stayed stale forever. Refresh now fires 500ms after the *last*
  listing event — wait for the storm to pass, then sample. Also
  rate-limits to 500ms between refreshes regardless.

## [1.18.4] - 2026-04-26

### Changed
- **Refresh debug log now includes the dirty file list** and
  the raw `git status --porcelain` output. We saw `git_files: 1`
  after a commit that should leave 0, but the prior logging
  didn't tell us *which* file was dirty — too many possible
  explanations (race with `.spyc-context-*.tmp`, stale BUGS.md,
  some other transient). Now `refresh_listing` logs the sorted
  dirty file basenames, and `git_status` logs both branch and
  the raw porcelain string. Run with `-d`, reproduce, and we'll
  know exactly what git was reporting at refresh time.

## [1.18.3] - 2026-04-26

### Changed
- **Debug-log diagnostics in `refresh_listing`.** Logs the
  before/after `git_info` and `git_files.len()` on every refresh
  (or the `Listing::read` error if it fails). Run with `-d` to
  diagnose when a watcher event fires but the display doesn't
  appear to update.

## [1.18.2] - 2026-04-26

### Fixed
- **Git status refresh on commit, take two.** 1.18.1's directory
  watch on `.git/` was right but the path filter still missed the
  case where macOS FSEvents *coalesces* multiple intra-directory
  changes into a single event whose path is `.git/` itself rather
  than `.git/index`. `is_listing_path` now also accepts `path ==
  .git/` (treating it as "something changed in there, refresh");
  the existing `index`/`HEAD` filter still applies to file-level
  events for backends/cases that deliver them.
- **Debug log now records every watcher event** (path, listing /
  config classification, event kind) — run spyc with `-d` to send
  events to `$XDG_STATE_HOME/spyc/debug.log` for diagnosis.

## [1.18.1] - 2026-04-26

### Fixed
- **Git status now actually updates after a commit.** 1.17.6
  taught `refresh_listing` to refresh `git_info` too, but the
  watch itself was unreliable: we were watching `.git/index`
  *as a file*, and `git commit` writes
  `.git/index.lock` then atomically renames it to `.git/index`.
  The rename replaces the inode, but our watcher kept its
  handle on the old (now-deleted) inode and stopped delivering
  events. Result: top-bar `main*` and the per-file dirty
  markers stayed stale until you switched directories, even
  though `refresh_listing` was correct. spyc now watches the
  `.git/` directory non-recursively and filters events to
  `index` (status / staging) and `HEAD` (branch switch); the
  rename lands as a directory event and the refresh fires.

## [1.18.0] - 2026-04-26

### Changed
- **Pane scroll-mode indicator is much harder to miss.** The
  divider rule line and the active tab label both retint to
  `theme.pick` (typically yellow) while in scroll mode, the
  active tab label is bold-uppercased (`[1*] CLAUDE`), and the
  right-side `[SCROLL]` tag picks up the same color. Three
  redundant signals across the divider — eye lands on at least
  one no matter where you're looking.
- **Entering scroll mode no longer jumps the pane.** Previously
  `enter_scroll_mode` shifted the viewport up by one line so
  there was *some* visual cue you'd left live view; with the
  new divider treatment the shift was just noise. The flag is
  now decoupled from `scroll_offset`, so entry is purely modal
  with no content motion. Also: `j` past the live position no
  longer auto-exits scroll mode — the mode is now purely modal,
  exit explicitly with Esc / q.

## [1.17.9] - 2026-04-25

### Changed
- **Session restore stops using `claude --resume`; types
  `/resume <sid>` after launch instead.** The CLI flag triggers
  a Claude regression where the mount-time
  `useEffect(...,[],g9H(K))` reads `onSessionRestored` from
  `FXK({enabled:false})`'s return value, gets `undefined`, and
  throws `g9H is not a function` — which wedges React while
  bun keeps the pty alive. Same effect doesn't fire on a fresh
  start (initialMessages is empty), so we now spawn fresh
  `claude` and, after a 1.5s settle delay, write
  `/resume <sid>\r` to the pane. The slash-command goes through
  `tM_` (a different code path that doesn't hit the bug). The
  crash-recovery prompt from 1.17.1 stays as a safety net for
  any path we missed.

## [1.17.8] - 2026-04-25

### Fixed
- **Claude crash-recovery prompt fires reliably again.** The
  1.17.5 simplify pass added an `output_dirty` gate to the
  crash-detection scan as a hot-path optimization, but
  `output_dirty` is cleared on every render. Claude prints its
  whole crash dump in well under a second and then sits
  quiescent, so by the time `dump_grace` (3s) elapses the flag
  is `false` — we'd skip the scan forever and the prompt would
  never fire. With 1.17.7's slug fix landing, restore now
  successfully spawns `claude --resume <sid>`, which trips the
  g9H regression and crashes — and *that's* exactly the case
  the silenced prompt was supposed to catch. Reverted the gate;
  the scan is bounded to the 30-second restore window and to
  tabs with `restore_fallback` armed, so it's not a meaningful
  cost.

## [1.17.7] - 2026-04-25

### Fixed
- **Session restore for projects with `_` (or any non-alphanumeric)
  in the path.** spyc's `project_slug` only rewrote `/` to `-`, but
  Claude rewrites *any* non-alphanumeric/hyphen char (so
  `tripstack_platform` lands at
  `~/.claude/projects/-Users-…-tripstack-platform/`, not
  `…-tripstack_platform/`). spyc was looking in the wrong directory,
  finding zero JSONLs, returning `None` from
  `resolve_claude_resume_target`, and saving sessions with no
  `claude_session_id` — so `spyc -r` always spawned a fresh
  `claude` for these projects regardless of how recent the
  conversation was. `project_slug` now matches Claude's
  normalization (any non-alphanumeric char → `-`); tests cover
  underscore, dot, and space.

## [1.17.6] - 2026-04-25

### Fixed
- **Top-bar git status now updates on file changes.** The
  watcher-triggered `refresh_listing()` only refreshed
  `git_files` (per-file dirty markers next to filenames); it
  never refreshed `git_info` (the branch + dirty string in the
  top bar — e.g. `main` vs `main*`). So after editing a tracked
  file, the per-row markers updated but the top bar stayed on
  `main`; switching directories forced a `chdir` which did
  refresh `git_info`. `refresh_listing()` now also calls
  `git_status()` so the top bar tracks repo state in place.

## [1.17.5] - 2026-04-25

### Changed
- **`make install` now depends on `make release`.** No more
  separate two-step dance — `make install` builds the optimized
  binary and copies it to `$(PREFIX)/bin` in one shot. README
  and INSTALL.md updated to drop the redundant `make release`
  line. The standalone `make release` target is unchanged for
  anyone who just wants a binary in `target/release/`.

## [1.17.4] - 2026-04-25

### Fixed
- **`!` (captured shell) now runs in spyc's listing dir.** The
  `!cmd` path went through `spawn_capture`, which built its
  `CommandBuilder` without setting a `cwd` — so the child
  inherited spyc's process cwd, which can drift from the
  navigated `state.listing.dir` (and only happens to match
  because `chdir()` also calls `set_current_dir`, which is
  best-effort and silently ignored on failure). `;cmd` worked
  fine because it explicitly passed `&self.state.listing.dir`
  to `Pane::spawn`. `spawn_capture` now takes a `cwd: &Path`
  and all four callers (`!`/`:!`/`:!!`/the `!?` history
  re-execute) pass `&self.state.listing.dir`. `make` from
  the project root now finds the Makefile.

## [1.17.3] - 2026-04-25

### Changed
- **Don't write `.mcp.json` under enterprise control.** When
  `/Library/Application Support/ClaudeCode/managed-mcp.json` (macOS)
  or `/etc/claude-code/managed-mcp.json` (Linux) defines a server
  named `spyc`, Claude already knows how to reach us through the
  org config. The per-project `.mcp.json` we used to write at every
  startup just collided on the server name (Claude resolves the org
  definition; the local file is dead weight). spyc now detects the
  managed definition, skips the write entirely, and removes any
  prior `spyc` entry from an existing `.mcp.json` (preserving any
  other servers the user has added; deleting the file if it only
  contained spyc). Status flashes `MCP: enterprise-managed (skipped
  local .mcp.json)` so it's visible. The takeover prompt is
  suppressed under enterprise control too — there's nothing to
  take over.
  Note: this *only* skips the local `.mcp.json` write. The Unix
  socket server (`mcp-<pid>.sock`) still runs so the org-defined
  `spyc --mcp` proxy can connect.

## [1.17.2] - 2026-04-25

### Fixed
- **Session restore no longer corrupts itself across cycles.** A
  saved tab's `command` was captured verbatim from the spawn
  string, so a tab spawned by restore as `claude --resume <sid>`
  would on the next save persist `command =
  "claude --resume <sid>"` instead of the user's original
  `"claude"`. When `resolve_claude_resume_target` later returned
  `None` (Claude had no fresh JSONL — e.g. a wedged or never-used
  conversation), the next restore fell back to `tab.command` and
  ran `claude --resume <stale-sid>` → fail → crash dump → tab
  closed → save again with same polluted command → infinite
  degradation. Save now strips `--resume <token>` from
  `tab.command` when it's a `claude` invocation, and the restore
  path applies the same strip defensively so already-corrupted
  session files heal on first reload.

## [1.17.1] - 2026-04-25

### Changed
- **Claude crash on resume now prompts before recovering.** The
  prior auto-respawn (1.16.2) only caught the case where
  `claude --resume <sid>` exited non-zero — but Claude has a
  regression where the resume path throws an unhandled
  `g9H is not a function`, leaving bun's pty alive while React is
  wedged. spyc now also detects "alive but printed a crash dump"
  by scanning the last ~200 lines of pane scrollback for stable
  markers (`/$bunfs/root/`, `is not a function`,
  `Error: sandbox required but unavailable`) at least 3 seconds
  after spawn. On detection it pops a one-key prompt:
  `claude crash detected — start fresh and recover with /resume?
  [Y/n]`. `y/Y/Enter` kills the child and spawns a fresh `claude`
  in the same slot; anything else kills it and removes the tab so
  the wall of minified JS is gone. The prompt is gated on
  `Mode::Normal` so it doesn't preempt other UI work — if you're
  busy with another prompt or pager, detection retries next loop.

## [1.17.0] - 2026-04-25

### Added
- **Host terminal title.** spyc now sets the outer terminal's window
  title to `🌶️: <project> · <session>` (e.g. `🌶️: spyc ·
  SAFFRON_CUMIN`). `<project>` is the basename of `PROJECT_HOME` when
  set, otherwise the basename of the cwd. Session is omitted when
  there's no `SESSION_NAME`. The pre-spyc title is pushed onto the
  terminal's title stack (xterm CSI 22;0t) on startup and popped on
  quit, including from the panic handler. Inside tmux, OSC 2 is
  wrapped in tmux's DCS passthrough so the outer terminal (iTerm2,
  etc.) sees it — requires `set -g set-titles on` in tmux. Updates
  are change-only (no redundant emits per draw); after a foregrounded
  child (vim, less) returns we force a re-emit in case it clobbered
  the title.

## [1.16.2] - 2026-04-25

### Fixed
- **Session restore now recovers from a failed `claude --resume`.** If
  a tab spawned by `spyc -r` as `claude --resume <sid>` exits non-zero
  within 10 seconds of spawn (bad/missing session id, sandbox crash,
  binary mismatch, …), spyc replaces the dead tab in place with a
  fresh `claude` and flashes `automatic session restore failed. try
  with /restore`. Previously the user was left staring at whatever
  Claude dumped on its way out — for sandbox crashes that's a wall of
  minified JS. The fallback preserves any extra flags from the
  original command (e.g. `--dangerously-skip-permissions`) and only
  strips the `--resume <token>` pair, so the replacement isn't
  doomed to fail the same way.

## [1.16.1] - 2026-04-25

### Fixed
- **Claude session resume no longer saves ghost UUIDs.** The
  resolver's last-ditch fallback (`find_claude_session`, which
  reads `~/.claude/sessions/<pid>.json`) trusted the PID-scoped
  index without checking that a JSONL actually existed. Claude
  writes the index entry as soon as `claude` starts, but the
  conversation JSONL only appears on the first turn — quitting
  spyc *before that first turn* produced a saved session ID with
  no file behind it, leading to "No conversation found with
  session ID …" on `spyc -r`. `resolve_claude_resume_target` now
  applies a final `claude_jsonl_exists` guard regardless of which
  branch produced the ID; if the file isn't there, we save no ID
  and restore opens a fresh `claude`. `claude_jsonl_exists` also
  checks the canonical (symlink-resolved) cwd, so macOS
  `/var` → `/private/var` paths don't slip through. A debug-log
  line records the dropped ID for future diagnosis.

## [1.16.0] - 2026-04-24

### Added
- **Live pane cwd in the divider line.** The pane status line now
  polls the active subprocess's actual cwd via `/proc/<pid>/cwd` on
  Linux and `lsof -Fn` on macOS (1-second TTL cache, render-path
  cost is negligible). When the live cwd differs from where spyc
  launched the tab — e.g. a `bash` tab where the user `cd`'d
  somewhere — the path gets a `↪` prefix and is rendered in the
  active-tab color so it's easy to spot. The previous tilde-collapse
  for `$HOME` is preserved via `paths::display_tilde`.
- **AGENTS.md note on shell-continuity loops.** Claude Code doesn't
  have shell continuity between Bash tool calls — `cd /foo` in one
  call doesn't persist to the next, which is a real source of
  Claude getting stuck on `make`/`cargo`/test loops. Added an
  explicit instruction in `AGENTS.md` covering compound `cd && cmd`,
  absolute paths, and the "run `pwd && ls` first when stuck" habit.
  The live-cwd indicator helps the *user* spot drift; this note
  helps Claude avoid the trap in the first place.

## [1.15.0] - 2026-04-24

### Added
- **`g b` — git blame on the cursor file.** Runs
  `git blame --color-lines -- <file>` and shows the colored output
  in the pager. Blame is single-file by design; the selection is
  ignored. Flashes a clear error if the cursor is on a directory or
  the file isn't tracked.

### Changed
- **`g d` now includes new (untracked) files.** Previously, sitting
  on an untracked file (`?` flag) and pressing `gd` produced empty
  output and looked broken — git diff doesn't know about untracked
  files. spyc now also runs
  `git ls-files --others --exclude-standard -- <selection>` to find
  untracked content under the selection, then synthesizes an "added"
  diff per file via `git diff --no-index /dev/null <file>`. The
  unstaged diff and the new-files diff are concatenated. `gD`
  (`--cached`) is unchanged — staging untracked files is a separate
  flow. Pager title is now `git diff (+ new)` to make the difference
  visible.

## [1.14.0] - 2026-04-24

### Changed
- **MCP takeover now prompts before clobbering another instance.**
  Previously, starting a second spyc in a directory already owned by
  a live spyc silently rewrote `.mcp.json` and notified the old
  instance to disconnect — easy to do accidentally and then wonder
  why your other session lost MCP. Now spyc detects the live
  instance before entering raw mode and prompts on stderr:
  `🌶️ spyc: PID 11935 already owns MCP here. Take over? [Y/n]`.
  Default Y on empty input. Decline ("n") and the old instance keeps
  ownership; the new spyc starts normally without MCP and flashes
  `MCP: kept PID 11935 as owner (Claude here will talk to it)`.
  Non-tty stdin (CI, piped input) keeps the historical auto-takeover
  behavior — there's no one to prompt.

## [1.13.0] - 2026-04-24

### Added
- **`spyc --print-config`** — emits a fully-commented default
  `.spycrc.toml` to stdout. Every option is shown commented out at
  its default value with a one-liner explaining what it does, grouped
  by section. Bootstrap a config with
  `spyc --print-config > ~/.spycrc.toml`. Round-trip parsed in tests
  so the dump always loads cleanly with the current schema.
- **Configurable status bar position.** New `[layout]
  status_position = "top" | "bottom"` option in `.spycrc.toml`.
  Default `"top"` (stock spyc). `"bottom"` matches the vim/tmux
  convention and is the right choice when running spyc inside tmux —
  the host status line typically owns the top row, so keeping spyc's
  bar there causes a double-bar. With `"bottom"` the prompt sits one
  row above the status bar (vim cmdline-above-statusline ordering),
  consistent with both pane-open and pane-closed layouts.

## [1.12.1] - 2026-04-24

### Fixed
- **Claude session resume — verify the banner token actually exists.**
  v1.11.2 trusted the `claude --resume <id>` banner unconditionally,
  but Claude sometimes prints the banner with a session ID it never
  persisted (the user `/clear`'d or `/resume`'d to a different
  session before exit). Restore would then fail with "No
  conversation found with session ID …". Now we verify the JSONL
  exists at `~/.claude/projects/<slug>/<id>.jsonl` before saving;
  if it doesn't, we fall back to the most-recently-modified JSONL
  in the project slug — the same file `claude --resume`'s no-arg
  picker would surface first. The PID-scoped scan is now only the
  last-ditch fallback.

## [1.12.0] - 2026-04-24

### Changed
- **`!` captured commands now run under a PTY and accept input.**
  Previously `!sudo …` (or anything else that opens `/dev/tty` for
  prompts: ssh, scp, gpg, passwd) wrote its password prompt straight
  to the real terminal — bleeding "Password:" / "Sorry, try again."
  text on top of the file list and into the pager body, with no way
  to actually answer because keystrokes went to spyc's normal key
  handling. Now `!` allocates a slave PTY for the child, so
  `/dev/tty` resolves to that slave and prompt bytes flow into the
  pager via the master like any other output. While the capture is
  live, every keystroke is encoded and written to the master, so
  the user can type a password (no echo — `sudo` controls termios
  on the slave) and press Enter. New control bindings inside a
  running `!`: **^C** sends SIGINT through the tty (cancels sudo's
  prompt, etc.); **^\\** hard-kills the child if it has detached
  from the tty. Status line updated accordingly.

## [1.11.3] - 2026-04-24

### Changed
- **Home directory shortens to `~` in displayed paths.** The status
  bar path, `I` info overlay (`start dir`, `cwd`, config sources),
  `:project` display, and the on-quit exit summary now collapse a
  leading `$HOME` to `~` (e.g. `~/src/spyc` instead of
  `/Users/derek/src/spyc`). Match is anchored at directory
  boundaries so unrelated paths sharing the home prefix as a
  substring are unaffected. MCP context output is unchanged —
  consumers continue to receive absolute paths.

## [1.11.2] - 2026-04-24

### Fixed
- **Claude session resume reliability.** `spyc -r` no longer fails
  with "No conversation found with session ID …" for sessions that
  resume cleanly via `claude --resume` by hand. Root cause: the old
  resolver scanned `~/.claude/sessions/*.json`, which is a PID-scoped
  index of *running* claude processes, not resumable conversations.
  After `/compact` or `/clear` rotates the session ID, that file
  still pointed at the original (now-orphan) ID. Fix: on session
  save we now read the `Resume this session with: claude --resume
  <token>` banner Claude prints on exit straight from the pane
  scrollback. The token is the authoritative resume target and works
  for both UUID and named sessions. Falls back to the old scan only
  when no banner is captured.

## [1.11.1] - 2026-04-23

### Fixed
- **Help pager multi-column layout.** Descriptions wider than a column
  now wrap onto continuation lines that align under the description
  column (no more silent truncation at the column edge). Section
  headers stay with their bodies — a section that wouldn't fit in the
  remaining column space moves as a unit to the next column.
- **Content-to-column mapping is now static.** `j`/`k` scrolls both
  columns in lockstep against a fixed partition instead of reshuffling
  lines between columns on every scroll. `G` and the `Top`/`Bot`/`NN%`
  position indicator all share the same "longest chunk" math, so
  pressing `k` from `Bot` no longer jumps back to 91%.
- **Responsive column count.** The 2-col / 1-col choice is made from
  the actual body width (90% of terminal × borders), not the raw
  terminal width, and is re-decided whenever the window is resized
  while help is open. Help rebuilds in place with the new wrap points.

## [1.11.0] - 2026-04-23

### Added
- **`PROJECT_HOME` concept.** A sticky per-session project root,
  distinct from `start_dir` (the backtick target). Auto-set on
  startup when the launch dir contains `.git`; otherwise unset.
  New bindings: `gh` (jump), `gP` (set to current dir). Command
  line: `:project`, `:project .`, `:project <path>`, `:project clear`.
  New pane tabs default their cwd to `PROJECT_HOME` when set.
  Persisted with the session (round-trips through `spyc -r`).
- **Named sessions (spice-themed).** Every session now has a
  display name like `SAFFRON_CUMIN` or `HARISSA_SUMAC`, generated
  on creation from ~30 spice words. Shown on the top bar in
  all-caps and as the primary column in the session picker.
  Rename with `:name <NEW>`.
- **Start dir is now editable at runtime.** `gS` sets it to
  current dir; `:startdir` prints; `:startdir .` / `:startdir <path>`
  sets it. Previously only settable at spyc launch or on session
  restore.
- **`gU` / `:whoami` to flash user@host** in the status line.
  User@host also appears in the `I` info overlay.
- **MCP context exposes `project_home` and `session_name`** so
  Claude can see the sticky project root and the session label.

### Changed
- **Top bar redesign.** Drops the user@host segment (rarely useful
  once you're inside spyc). New order:
  `🌶️ | PROJECT_HOME | SESSION_NAME | path | git | suffix`.
  Truncation priority under width pressure: suffix → path-basename
  → git branch. PROJECT_HOME and SESSION_NAME are retained as the
  primary identifiers for the workspace.

## [1.9.0] - 2026-04-21

### Added
- **Frecency-based path ranking for J prompt.** The J (jump) prompt
  now learns from your navigation history. Directories are scored by
  frequency x recency (zoxide-style tiered decay). When filesystem
  completion finds no matches, frecency suggests directories you've
  visited before — type a fragment, Tab completes the best match.
  Persisted to `~/.local/state/spyc/frecency.json`, capped at 500
  entries with LRU pruning. Health check validates on startup.
- **DEC 1007 alternate scroll mode** replaces `EnableMouseCapture`.
  Scroll wheel becomes arrow keys in the alternate screen — prevents
  scrollback interference while keeping text selection working.
- **Trackpad scroll throttle.** Rate-limits rapid-fire arrow keys
  from trackpad inertia to ~25/sec (40ms gap) for smooth two-finger
  scrolling.

### Fixed
- **Tab completion for remote directories.** `~/D<tab>` no longer
  filters the current listing when completing paths in a different
  directory. Now flashes match names directly (Desktop, Documents,
  Downloads).

## [1.8.0] - 2026-04-19

### Added
- **Writable MCP actions.** Claude can now mutate the TUI workspace
  via five new MCP tools: `navigate_to` (chdir or focus file),
  `set_filter` (set/clear glob filter), `pick_files` (pick by glob
  patterns), `clear_picks`, and `get_file_content` (read up to 100KB
  text). The MCP server uses a command channel to the main event loop
  with one-shot reply channels and 5-second timeout. Flash messages
  (`[mcp] navigated to src/`) keep the user informed when Claude
  changes the workspace.

## [1.7.0] - 2026-04-19

### Added
- **Performance refactor.** Idle CPU dropped from ~12.5% to near-vim
  levels (~2.5%). Root cause: context file writes were triggering
  file-watcher → refresh_listing → git subprocess → redraw cycles.
  Fixes: context file excluded from watcher, context writes skipped
  when unchanged, DEC 2026 synchronized output, build_rows/grid
  computation caching, active-tab-only draw triggering, has_pending
  atomic guard on drain, increased idle poll interval.
- **Activity monitor** (`A` toggle): live overlay showing draws/sec,
  cells/sec, draw reason breakdown (pane/event/other), and poll rate.
  Piggybacks on real draws — does not force its own redraws.
- **`y` prefix for yank commands.** `yy` yanks files into inventory
  (was bare `y`), `yp` yanks visible pane output to clipboard, `yP`
  yanks the last prompt you typed into the pane to clipboard.
- **Pager `?` help:** dedicated help overlay showing all pager keybindings.
- **Exit summary:** on quit, spyc prints a one-line session summary to
  stdout (cwd, tab count, Claude session name, restore hint).
- **Pager line numbers default to on.** `l` toggles line numbers, `w`
  toggles whitespace markers (previously `l` controlled both).
- `make install` now shows verbose progress (linking stage note,
  codesign step, version in final message).

### Changed
- **Pane prefix switched to `^a` (screen-style).** `^w` still works
  as an alias. Bindings: `^a n`/`]` next tab, `^a p`/`[` prev tab,
  `^a c` new tab, `^a K`/`x` close tab, `^a P` pipe content,
  `^a r` rename, `^a s` send selection, `^a v` scroll mode.
- Focus notice uses product naming: "focus: spyc" / "focus: claude"
  (active tab label) instead of generic "focus: list" / "focus: pane".
- `git status` uses `-unormal` instead of `-uall` to avoid expensive
  recursive enumeration of untracked directories.

### Removed
- Cursor blink in the pane — was causing phantom redraws and added no
  value. Unfocused cursor now shows as a static dim block.
- Periodic `^L` refresh to Claude pane tabs — cleared draft prompts
  when focus was elsewhere.

### Fixed
- Backtick (`` ` ``) now returns to the session's home directory, not
  where spyc was launched from.
- `gf`/`gF` scans the last 200 lines of scrollback (not just the
  visible viewport), so paths in large diffs are still found.

## [1.6.0] - 2026-04-18

### Added
- Unicode-width support: CJK filenames, flags, and emoji now render
  with correct column alignment in the file list, status bar, help
  screen, and pager. Uses `unicode-width` crate.
- `CHANGELOG.md` seeded in Keep-a-Changelog format.
- `--version --verbose` dumps git SHA, build timestamp, rustc version,
  TERM, COLORTERM, and os/arch. `build.rs` embeds build info.
- **Inventory rewritten as file cache.** `y` (yank) copies file
  content into `~/.local/state/spyc/inventory/`. `p` (put) copies
  cached files to the current directory and removes from inventory.
  Regular files only — directories and special files are rejected.
- Inventory view (`i`): `t`/`Space` to tag items for partial put,
  `p` to put tagged (or all) to cwd, `x`/`d` to remove to graveyard.
- `Y` (shift-y) removes cursor file from inventory in dir view.
- Inventory persists across sessions (file-backed cache with metadata).
- Graveyard: removed inventory items are preserved in
  `~/.local/state/spyc/graveyard/` for undo safety.
- ESC exits inventory view (returns to directory view).
- Status bar always shows hidden file count (even when 0).
- `V` opens `$EDITOR` in the top pane (overlay) — the Claude pane below
  stays visible so you can edit while watching Claude work. `e`/`v` still
  opens the editor full-screen (suspends TUI).
- `:version` command and `gV` keybinding show the spyc version
  (previously `V`, now reassigned to edit-in-pane).

### Changed
- `p` in dir view now means "put inventory to cwd" (was "drop from
  inventory").

## [1.5.0] - 2026-04-18

### Added
- **MCP context handoff (M14):** spyc runs an HTTP MCP server on a
  background thread (OS-assigned port). Claude CLI connects via
  `--mcp-config` injected at pane spawn. `get_spyc_context` tool
  returns cwd, cursor, picks, inventory, filter, and git branch.
- **Conversation-aware session restore:** session save captures
  Claude Code session ID and display name. Restore spawns
  `claude --resume <sessionId>` to resume the conversation.
  Session picker shows name + short ID.
- `SPYC_CONTEXT` environment variable set in pane environment,
  pointing to `.spyc-context-<PID>.json`.
- `--mcp` CLI flag for stdio MCP server (testing/future use).
- macOS `codesign -s -` in Makefile install target.

### Fixed
- Pane tabs now stay open with `[exited]` label when the child
  process exits, so error output is readable. Any keypress dismisses.
- Session dedup no longer broken by ephemeral `--mcp-config` port
  numbers in saved commands.

## [1.4.0] - 2026-04-18

### Added
- **Bidirectional path references (M13):** `gf` jumps the file list
  to a path reference in pane output; `gF` also opens the pager at
  the referenced line.
- Path extraction handles: bare paths, `path:line:col`, backticks,
  quotes, Claude CLI patterns (`Update(path)`, `Read path`, `⎿`,
  `→`), diff headers, ANSI stripping.
- Bottom-up scan (most recent output wins), dual cwd resolution
  (pane cwd + project root).
- Works in both live and scroll mode (`g` prefix: `gg`/`gf`/`gF`).
- 35 path extraction tests.

### Fixed
- `gf`/`gF` no longer matches bare slashes as paths.
- `gf`/`gF` exits scroll mode and unfocuses pane on successful jump.

## [1.3.1] - 2026-04-17

### Fixed
- Watch `.git/index` for live git status marker updates after
  `git add`, `commit`, `checkout`, etc.

## [1.3.0] - 2026-04-17

### Added
- `:cd` command to change directory from the command line.
- `:sort` with `name`, `size`, `mtime`, `ext` modes.
- `:marks` displays current marks in a pager.
- `:set key=value` for runtime settings.
- Pager buffer history: `:bprev`/`:bnext` or `[b`/`]b` navigate
  closed pagers (max 10 in back/forward stack).

## [1.2.0] - 2026-04-16

### Changed
- Git status markers moved to left gutter (was overriding file
  colors).

## [1.1.0] - 2026-04-16

### Fixed
- File type colors no longer overridden by git status colors.

## [1.0.0] - 2026-04-15

### Changed
- Renamed from `cspy` to `spyc`.

## [0.13.0] - 2026-04-14

### Added
- `:` command line: `:limit`, `:!cmd`, `:!!`, `:;cmd`, `:q`.
- `=` limit filter: `=*.rs`, `=!` for picks only, `=` clears.
- Numeric prefix display (typing `3j` shows "3" in prompt area).
- `:N` jump-to-line in pager and history editor.

## [0.11.0] - 2026-04-13

### Added
- `!?` history picker popup with vi-editable lines, `/search`,
  `n`/`N` navigation, `G`/`gg`, `Ctrl+D` delete, deduped history.

### Fixed
- Pager/pane repaint artifact on close.
