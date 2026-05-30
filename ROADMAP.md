# spyc roadmap

## Thesis

spyc is a vi-keyboard-driven file commander that exposes itself to an
AI coding agent as a queryable context source. The target user is a
developer who already thinks in vi motions and wants Claude Code
living in the same workspace -- not one window over, not in a browser
tab, in the same session, sharing context.

The MCP server (M14) shifted the tool's nature: spyc isn't just "a
file manager with Claude in a pane." It's a file manager that Claude
can query -- current directory, cursor, picks, inventory, filter, git
branch -- via a standard protocol. That bidirectional awareness is the
positioning that differentiates spyc from `tmux` + `claude`.

Every other feature -- picks, inventory, pager, status bar, sessions --
is supporting infrastructure that makes the split-pane workflow fast
and comfortable. The roadmap is organized accordingly: the
pane-and-agent integration is the defining work track, not the
trailing milestone.

## Working tracks

Work proceeds along three parallel tracks. They're not strictly
sequential; distribution work can land while thesis work is still in
flight, and foundations work continues throughout.

- **Foundations** -- testing, hardening, build hygiene. The minimum to
  not embarrass ourselves and to make every other change safer.
- **Thesis** -- deepening the agent integration until the split-pane
  workflow is measurably better than `tmux` + `claude` for the target
  audience. This is where the tool earns its reason for being.
- **Distribution** -- release automation, signing, packaging, docs.
  Turns a repo into a tool people can install, trust, and find.

Each track has its own priority ordering below. Specific items migrate
from these lists into `Done (recent)` as they ship.

## Foundations

Testing infrastructure, CI pipeline, handler extraction, unicode-width,
startup health check, and performance refactor are all shipped. The one
*active* foundations item is the **`app/mod.rs` decomposition** — it has
grown to ~12k lines and is now the road-to-2.0's next track (see
[`REFACTOR_PLAN.md`](REFACTOR_PLAN.md) and the Releases section). The
other remaining items below are lower priority — nice to have, mostly
not blocking 2.0.

### Done

- Panic hook that restores the terminal -- restores raw mode + alt
  screen on panic, writes backtrace to debug log.
- CI fixes -- `rust:1.85-slim` matches MSRV, `cargo-audit` in
  pipeline (RUSTSEC-2026-0009 ignored -- needs Rust 1.88),
  `cargo-llvm-cov` with 35% ratcheting floor.
- Testing strategy execution -- 358 tests. Keymap resolver (77 tests),
  state modules (picks/inventory/cursor/ignore/history/sessions),
  DSL->resolver round-trips, `tests/` integration directory, snapshot
  tests via `insta` + `TestBackend`, handler extraction (Phases 0-4
  complete: `AppState` with domain logic cleanly separated from
  terminal state), health check tests (7).
- 71 clippy errors fixed -- clean `cargo clippy -D warnings` build.
- Unicode width in the list view -- `unicode-width` crate,
  `display_width()`/`display_truncate()` helpers. All UI width sites
  fixed: list_view, status bar, help, pager, `truncate_middle()`.
- CHANGELOG.md seeded in Keep-a-Changelog format with entries from
  v0.11.0 through v1.7.0.
- `spyc --version --verbose` -- `build.rs` embeds git SHA, build
  timestamp, rustc version. Dumps version, git, build time, rustc,
  TERM, COLORTERM, os/arch.
- Startup health check -- scans `~/.local/state/spyc/` on startup.
  Validates inventory, marks, sessions, graveyard. Cleans up orphaned
  files, warns about corrupt state. 7 tests.
- `spyc --print-config` -- emits a fully-commented default
  `.spycrc.toml` to stdout. Self-documentation for every option
  (layout, colors, ignore masks, keymap DSL) and a user starting
  point: `spyc --print-config > ~/.spycrc.toml`.
- Configurable status bar position (`[layout] status_position`) --
  `"top"` (default) or `"bottom"` (vim/tmux convention; useful when
  running spyc inside tmux to avoid double status bars). Prompt
  follows status (cmdline-above-statusline ordering).
- Performance refactor -- idle CPU dropped from ~12.5% to ~2.5%
  (vim-competitive). Context file watcher exclusion, context write
  caching, DEC 2026 synchronized output, build_rows/grid caching,
  active-tab-only draw, has_pending atomic guard, adaptive poll.
  Activity monitor (`A` toggle) for ongoing diagnosis.

### Remaining

- **Elm Architecture refactor (Model-View-Update).** `app/mod.rs` is
  ~12k lines with entangled concerns: domain state, TUI state,
  process lifecycle, rendering caches, file watching. The handler
  extraction (Phases 0-4) separated `AppState` domain logic
  (`AppState::apply` already returns an `ApplyResult` enum — the
  Update half is essentially done). The event loop and render path
  are still fused. **Staged in
  [`REFACTOR_PLAN.md`](REFACTOR_PLAN.md), detailed in
  [`docs/MVU_PLAN.md`](docs/MVU_PLAN.md): Phases 1–2 (the module
  extractions) shipped; the full MVU rewrite (Phase 3) is now a
  road-to-2.0 track too** (2026-05-30 decision — landed pre-2.0 so the
  launch ships on the cleaner foundation, via the strangler-fig plan that
  makes each phase behavior-equivalent behind green CI). Target shape:
  1. **View** — pure functions in `src/ui/` that take `&AppState` and
     render. Replace the inline rendering in `app/mod.rs::render` with
     a single `ui::render(terminal, &self.state)` call. Snapshot tests
     extend cleanly because every widget renders from `&AppState`.
  2. **Single message channel** — one `mpsc::Receiver<Message>` for
     the event loop. The crossterm event reader, file watcher (already
     a thread), pane capture readers (already threads), MCP socket
     thread, and timer ticks all push their events as a `Message`
     variant into the same receiver. The loop blocks on `recv` instead
     of polling.
  3. **`App::run` reduces to ~100 lines:** `loop { recv → update →
     render }`. No more open-coded watcher polling, no more inline
     `output_rx.try_recv()` per tick, no more `event::poll` with
     manual timeout math.
  Stick with `std::thread + mpsc` — spyc is sync end-to-end, tokio
  would be a regression here. Subsumes the deferred handler
  extraction Phases 5-6. Big lift — do incrementally alongside
  feature work, not as a standalone rewrite. The mechanical Phase 1
  extractions (per `REFACTOR_PLAN.md`) are the natural first slices:
  no behavior change, one module per commit, shrinking `app/mod.rs`
  immediately.
- **Expand snapshot tests.** `insta` + `TestBackend` infra is wired.
  Status bar snapshots done (4). Remaining: `list_view`, `pager`
  (ANSI, hex, line numbers, search highlight), `line_edit` modes.
  Incremental -- add as widgets are touched.
- **One pty integration test.** Spawn `cat` via `portable-pty`, write
  bytes, parse `vt100::Screen`, assert rendered output. `#[cfg(unix)]`.
  One test, not a suite.
- **Property tests (narrow).** `proptest!` blocks for: shell-arg
  quoting round-trip, limit-filter glob matching, resolver count
  invariants. One block per site.
- **Background directory loading.** Large directories (100K+ entries)
  and slow filesystems (NFS, external drives) block the event loop
  because `Listing::read()` and `git_file_statuses()` run
  synchronously on the main thread. Target flow:
  1. `chdir()` clears the current rows and sets a "loading" sentinel
     in `AppState` so the View can render a spinner / dimmed list.
  2. A worker thread (one per chdir; supersedes any in-flight one)
     runs `Listing::read` + `git_file_statuses` and pushes
     `Message::ListingReady(Listing)` into the main channel (lands
     for free once the Elm refactor's single-channel shape is in).
  3. Main loop receives, swaps in the new listing, redraws.
  4. **Cancellation:** if the user `chdir`s again before the worker
     finishes, the new chdir bumps a generation counter; stale
     `ListingReady` messages whose generation doesn't match are
     dropped (don't bother killing the worker — the read is bounded
     and its result just gets discarded).
  Scoped conservatively -- the common case (local NVMe, <1K entries)
  stays fast; the spinner only appears when the read actually takes
  long enough to notice (~50ms threshold).
- **Cwd export on quit** (Yazi-inspired; promoted from Additional
  Ideas based on BUGS.md). Yazi's `q` writes the cursor's cwd to a
  path the parent shell wrapper sources, so the shell follows. Add
  a `--cwd-file <path>` flag (or `$SPYC_CWD_FILE` env var); on
  quit, write the file-list cwd to it. Document a tiny zsh/bash
  function in INSTALL.md that wraps `spyc` and `cd`s the parent
  shell on exit. ~30 lines of code + doc snippet. Day-driver
  fundamentals -- `q` becomes "go here in my shell" instead of
  "back to where I started" for users who use spyc as their
  primary navigator. `Q` keeps the no-export semantics so users
  who *don't* want this can opt out per-quit.
- **Keymap DSL completeness** (promoted from BUGS; external
  contributor 2026-05-15). Two paired shortcomings in
  `src/config/dsl.rs`:
  1. **Many `Action` variants are unbindable.** `parse_action`
     doesn't accept `HarpoonAppend`, `SetMark(_)`, `JumpMark(_)`,
     `PaneTabByIndex(_)`, the `Yank*` family, `Goto*`,
     `WorktreeList`, `GitDiff*`. They exist as actions but the
     parser has no string form. Pick one: grow `parse_action` to
     cover them, or explicitly document which actions are
     user-bindable and which aren't.
  2. **`unmap` is a no-op.** `parse_dsl_line` returns `Ok(None)`
     for `unmap KEY` with a `// TODO: represent unbind` comment.
     Users can't currently remove built-in bindings cleanly. Wire
     it through so `unmap <KEY>` actually unbinds.
  Tackle as a pair -- shared parser, shared documentation.
- **PgUp/PgDn discoverability in panes** (promoted from BUGS;
  external contributor 2026-05-15). Two paired UX items:
  1. **PgUp/PgDn in a focused pane auto-enters `^a-v` scrollback
     mode** with a single page move applied, so users without
     `^a` in their fingertips get a discoverable scroll
     affordance. Guard with `!is_alternate_screen()` (scroll-mode
     entry already does this) and with a modifier (Shift-PgUp) so
     the child's own PgUp handling isn't stolen.
  2. **First-time pane-focus hint.** On the first time the user
     focuses a pane this session, flash `^a-v scrolls history`
     for ~2s. Pure discoverability, no behaviour change.
  Both small; pair them because the hint explains the binding.
- **Mouse forwarding to the pane** (promoted from BUGS). spyc never
  calls `EnableMouseCapture` on the host terminal
  (`src/main.rs::setup_terminal`), and `src/pane/input.rs` has no
  encoder for `Event::Mouse(_)`. `vt100` already tracks the mouse
  protocols when the child enables them (1000/1002/1003/1006), but
  no events ever reach the pty. Apps that default to mouse-on
  (lazygit, htop, broot) look half-broken -- clicks on panel
  headers / commit list / footer keybindings, scroll-wheel on
  diffs, all silently no-op.

  Two-layer fix: enable mouse capture on the host terminal *and*
  add the `Event::Mouse` arm in `pane::input::encode_key` to
  encode SGR mouse reports the child expects. Design carefully
  because spyc itself doesn't want mouse events outside the pane
  -- the right shape is "forward to pane only when pane is
  focused;" the file list keeps its existing minimal mouse
  semantics. Tension with the keyboard-first thesis is OK here:
  the pane is a real terminal whose contents (lazygit, htop) are
  third-party and mouse-aware, so denying the forward is
  effectively breaking those tools.

## Thesis -- deepening the agent integration

The pty pane is already the core of the tool (M8-M12 done). The work
that remains is making the integration genuinely novel, not just "a
terminal inside a terminal." In priority order:

### Done

- **Codex MCP discovery via `.codex/config.toml`** (v1.41.8).
  `ensure_codex_config_toml` mirrors `ensure_mcp_json` for codex's
  TOML config. Splice is shape-safe (malformed file → clean
  rewrite). Takeover detection now covers both `.mcp.json` and
  `.codex/config.toml` so a stale codex-only entry triggers the
  same prompt. Closes the codex parity series (PR-C).
- **Alt-screen scroll-mode hint + `[pane] default_command` config
  + `gd`-vs-HEAD** (v1.41.7). PR-B of the codex parity series. UX
  fixes for the "running an agent in the pane" workflow.
- **Codex session save/restore parity with claude** (v1.41.6).
  `spyc -r` resurrects codex panes with conversation intact. UUID
  sniffed from codex's exit banner; restore spawns
  `codex resume <UUID>` directly (CLI flag works for codex, unlike
  claude). `AgentKind` enum + `agent_session_id` field added; older
  saves migrate via serde aliases. PR-A of the codex parity series.
- **Writable MCP actions** (v1.8.0). Five new tools: `navigate_to`,
  `set_filter`, `pick_files`, `clear_picks`, `get_file_content`.
  Command channel (mpsc) from HTTP server threads to event loop with
  one-shot reply channels and 5s timeout. Claude can mutate the TUI
  workspace directly. Flash messages inform the user.

### Remaining

- **Background tasks (`^Z` / `:fg`).** Day-driver fundamentals --
  running `cargo test` or a long `find` shouldn't lock you out of
  spyc. The plumbing is mostly there: `spawn_capture` already returns
  `(child, writer, mpsc::Receiver<bytes>)` and `PendingCapture` owns
  exactly that shape; backgrounding is fundamentally "move
  `PendingCapture` from a singular field into a collection, detach
  the pager, keep the reader thread draining into a per-task
  buffer." Phasing:
  1. **M1 -- `^Z` to background, `:fg` to resume.** ✅ shipped in
     v1.20.0. `^Z` in the streaming capture pager moves the task into
     a `BackgroundTasks` collection on `App`; pager closes; flash
     `task #N backgrounded`. `:fg` (no arg) re-attaches the
     most-recent task; `:fg N` targets a specific id. Still-running
     tasks come back as a streaming pager seeded with the buffered
     output so far; already-exited tasks come back as a static pager
     with the final `exited <code> (Xs)` title. Buffer head-truncates
     at 1 MB. Original task id is preserved across `^Z` → `:fg` → `^Z`
     cycles via `PendingCapture.original_id`. v1.20.2 moved the
     status indicator from the status bar into the pane divider:
     right-aligned, distinct color from pane tabs, glyphs `[N+]`/
     `[N●]`/`[N✓]`/`[N✗]` keyed off a per-task `has_unread_output`
     flag.
  2. **M2 -- task viewer.** ✅ shipped in v1.21.0 (different shape
     from the original `:bg` picker overlay sketch). `gB` from the
     file list opens the most-recent task in a peek pager; `[t`/`]t`
     chord (or `:task N`) cycles by id. Live-refreshes from the task
     buffer while running. On close of a viewed-and-exited task, its
     rendered view is promoted into buffer-history (`[b` walks back
     to it) and the task is dropped from the bg list. v1.21.1 added
     `gp` to reopen the most-recent closed pager buffer from the
     file list. **Still TODO** from the original M2 sketch:
     `R`-to-kill (SIGTERM/SIGKILL on running task), `r`-to-re-run.
     The auto-promote-on-view-of-exited semantic replaced the
     explicit dismiss step.
  3. **M3 -- `!&cmd` direct-launch.** Skip the foreground pager
     entirely; task starts in background. Symmetric `:!&cmd` and
     `:bg cmd` command-line variants.
  4. **M4 -- Polish.** Optional bell / OS notify on completion
     behind a config flag (`[notify] on_task_complete = "bell"`),
     off by default. Pane-tab integration: treat exited tabs as
     background-task-style records for post-mortem viewing. MCP
     exposure: a `get_running_tasks` tool so Claude can ask "what's
     running?" and tail recent output.
  Open decisions: `^Z` is the right vim/shell muscle memory but
  overrides the literal terminal-suspend semantics some users may
  expect (we already trap most ctrl- combos in the file list, so
  this is consistent). Backgrounded tasks **don't** survive
  `spyc -r` -- running children are tied to the spyc PID and
  reattach is a rabbit hole; quit-time prompt covers cleanup.
- **Context enrichment.** `get_spyc_context` currently returns file
  paths and metadata. Could additionally expose: file contents (or
  snippets) for picked files, recent compiler errors from `cargo
  check`, unstaged diffs. Makes Claude's context richer without the
  user needing to pipe explicitly. Scope carefully -- large payloads
  would need truncation or pagination.
- **Generalized "beam" -- send content to any stdin sink.** Extends
  the existing `^a s` (paths), `^a P` (pick contents), `^a i`
  (inventory) family along three axes:
  1. **Region beam from the pager.** Beam visually-selected or
     `:N,M`-specified line ranges of the open file, wrapped with a
     `path:N-M` header and a fenced code block. Lets the user quote
     exact sections into the receiver without leaving the keyboard.
  2. **Configurable sink targets.** Today the destination is always
     the active pane tab. Make it pluggable: active tab (default),
     a specific tab by index, system clipboard (OSC 52), or an
     arbitrary shell command (`:beam !pbcopy`, `:beam !nc host port`,
     `:beam !jq .`). Named sinks live in `.spycrc.toml`. This is
     what generalizes the feature beyond Claude -- any tool that
     reads stdin is a target.
  3. **Format wrappers.** Per-target toggle: raw, paths-only,
     fenced-with-path-header, diff-style. So beaming picks to a
     "claude" sink dumps a path header + a fenced block; beaming
     paths to an `!xargs` sink stays raw newline-separated.
  Lower-level primitive that "Prompt templates" (below) sits on top
  of. Implementation reuses `pane.send_bytes()` plus a sink dispatch
  table.
- **Image paste (`^v`) to the agent pane** (promoted from BUGS).
  Natural first user of the DnD drop-action picker design (see
  `Drag and drop` under Additional Ideas): when the clipboard
  carries an image, pressing `^v` while the file list is focused
  changes focus to the lower pane and sends the image as an
  attachment (when the agent supports attachments -- Claude does
  today). Different from the OSC 72 native DnD path;
  complementary -- the routing logic (image vs. text, target
  agent supports attachment vs. not) is the same as the picker's
  "send to lower pane as image" arm. Implement the routing once,
  expose via both DnD and `^v`. Today `^v` is a no-op outside of
  prompts so the binding is free.
- **Project-wide search (`F` finder, `:grep`, MCP exposure).** Today
  `/` matches filenames in the current listing only and content
  search means shelling out to `! rg foo`. Two distinct gaps:
  filename search across the project, and content search across
  files. Both worth filling; both have a spyc-shaped MCP angle that
  generic Claude tools (Glob/Grep via Bash) don't.
  1. ~~**M1 -- `F` filename finder.**~~ Shipped v1.22.0 (worker-
     thread streaming in v1.22.1, multi-repo descent in v1.22.2).
  2. ~~**M2 -- `:grep <pattern>` content search.**~~ Shipped
     v1.23.0. Embedded ripgrep matcher (`grep-searcher` +
     `grep-regex`), gitignore-aware, smart-case, multi-repo
     walker. Results stream into a pager as `path:line:col: text`
     so `gf`/`gF` jump for free. Capped at 5000 matches; refine to
     narrow.
  3. ~~**M3 -- MCP exposure.**~~ Shipped v1.24.0. Four tools:
     `search_paths` (fuzzy filename), `search_content` (ripgrep-
     matcher content search), plus the two *spyc-shaped* tools
     that justify the MCP-thesis -- `search_picks(pattern)` (only
     within the user's currently-picked files; picks are spyc
     state Claude can't see otherwise) and
     `search_inventory(pattern)` (across the persistent inventory
     cache, so Claude can grep accumulated interesting files
     without re-explaining context).
  No persistent index (no tantivy, no ctags). Maintenance burden
  isn't worth it -- ripgrep on a 100K-file repo is sub-second
  cold and instant from page cache on repeat. Let dedicated tools
  be dedicated; spyc is the keyboard surface and the MCP bridge.
- **Session forking** (already in old roadmap as `^a f`). Duplicate a
  pane tab with scrollback replayed, so a Claude conversation can
  branch without losing the prior line of inquiry. High-value for
  "let me try a different approach." Implementable with current
  plumbing.
- **Prompt templates in `.spycrc.toml`.** User-defined macros that
  send a pre-composed prompt to the pane with picks/inventory
  substituted in -- e.g., `map "<space>cr" claude-template review`
  where `review` is defined in config. Turns spyc into a
  keyboard-driven Claude launcher for repeated workflows.
- **Session cost telemetry (replaces the older "status bar agent
  segment" entry).** Read the active Claude session's JSONL at
  `~/.claude/projects/<slug>/<session>.jsonl` (the same file
  `find_claude_session` already locates for resume), sum input /
  output / cache tokens, multiply by a small built-in pricing table
  keyed on model name. Surface in two places:
  1. `I` info overlay -- a `session: $0.42 (37k in / 12k out)` line
     so the user can see their spend without leaving spyc.
     Optionally a tiny top-bar segment when the pane is running
     Claude, behind a config flag (off by default; the bar is
     already busy).
  2. New MCP tool `get_session_cost` -- lets Claude self-report
     ("you've cost $0.42 in this conversation"). This is the
     *spyc-shaped* angle: only spyc can see Claude's own JSONL via
     MCP, so the tool is novel-positioned rather than duplicative
     of standalone cost trackers.
  Scope: pricing table is hardcoded constants for the Claude 4.x
  family, updated when new models ship. Multi-provider / currency
  conversion / historical dashboards / one-shot success classifiers
  stay out of scope -- standalone tools handle those better and
  spyc has no business being one of them.
- **Autocommands** per the old roadmap, but scoped to the agent
  workflow -- `autocmd "*.test.ts" "claude-template test-review"`
  etc. Defer until the template feature lands and the shape is
  clear.
- **MCP peer credential checking.** Socket permissions and path
  containment are shipped (v1.10.1). Remaining hardening:
  `SO_PEERCRED` (Linux) / `LOCAL_PEERPID` (macOS) to verify the
  connecting process UID matches the server. Defense-in-depth --
  low priority since socket file permissions already enforce
  user-only access.
- **Structured event stream (subscriber socket).** Inspired by
  Yazi's `--local-events` / `--remote-events` flags. Today MCP
  consumers poll the context snapshot file (`.spyc-context-<pid>.json`),
  which is fine for "what is the user looking at right now?" but
  forces clients to diff snapshots if they want to react to
  *changes*. Add a subscribe verb to the existing PID-scoped Unix
  socket: a subscriber connects, registers an interest set
  (`cd`, `cursor`, `pick`, `filter`, `task_state`, `quit`, etc.),
  and receives a JSON-line stream of structured events as they
  happen. Same socket, same auth model -- just a new RPC method.
  - Plays directly into the bridge thesis: opens spyc's state to
    *non-Claude* tools (a tmux status segment showing the active
    spyc's PROJECT_HOME, a Neovim plugin that follows the spyc
    cursor's parent directory, a desktop notifier on long-task
    completion) without any of them needing the MCP stdio
    handshake.
  - Generic primitive, but the consumer ecosystem we care about
    is still keyboard/agent-flavored -- not a generic
    "automation bus." We don't add the message-publishing side
    (Yazi's `ya pub`); that's a different feature (autocommand
    targets) and adds a wider attack surface.
  - Shape: `subscribe(events: ["cd", "pick"])` returns a stream;
    each event is `{ts, type, payload}`. Unsubscribe by closing
    the socket. Backpressure: drop events past a small per-
    subscriber buffer (events are advisory; replay isn't
    expected).
  - Implementation hook: every place that currently bumps
    `last_context_json` or flips a `needs_draw` reason is
    already a good event boundary. Centralize emission behind a
    single `emit_event(EventKind)` so we don't have to hunt
    call-sites later.

## Distribution

Most of this is one-time setup work. Worth doing properly and then
forgetting about.

### Done

- **README rewrite.** Leads with the MCP-server thesis. Screenshot
  embedded. Claude-specific framing, softened first-mover claim,
  ^a keybindings, expanded config example. Revised across three
  rounds of review.
- **Makefile install target.** Build, release, cross-compile, install,
  deploy targets. Verbose install with codesign step.

### Remaining

- **Release automation in Bitbucket Pipelines.** Tag push
  (`v[0-9]+.*`) triggers: cross-compile matrix (Linux x86_64/aarch64
  musl, macOS universal), build artifacts uploaded to a release
  bucket, release notes generated from CHANGELOG.md, Homebrew tap
  formula bumped, crates.io publish. Zero-manual-step release.
- **macOS code signing and notarization.** Developer ID certificate,
  `codesign --deep --sign`, `xcrun notarytool submit`, stapled.
  Without this, Gatekeeper blocks the binary on fresh macOS
  installations and the first user report will be "it says spyc is
  damaged." (Ad-hoc signing via `codesign -s -` is in the Makefile
  for local dev builds.)
- **Linux signing.** Minisign or cosign on release artifacts,
  public key committed to the repo and published in release notes.
  Cheaper than Sigstore/SLSA attestation and sufficient for this
  audience.
- **Reproducible build verification.** Lock the toolchain
  (`rust-toolchain.toml` already pins), `SOURCE_DATE_EPOCH` honored,
  `cargo-auditable` to embed build metadata. A second CI job rebuilds
  from the tag and diffs against the released artifact. Not strictly
  required but cheap insurance and a nice signal.
- **SBOM.** `cargo-sbom` or `cargo-auditable` generates SPDX/CycloneDX
  at release time. Uploaded alongside binaries.
- **Package registries.** `cargo publish` to crates.io (binary-only
  crate, acceptable). Homebrew tap at `tripstack/homebrew-spyc`.
  Arch via AUR `spyc-bin`. Skip nixpkgs, Debian, Fedora packaging
  unless volunteers emerge -- not worth the maintenance tail for this
  tier.
- **GitHub mirror.** Read-only mirror at `github.com/tripstack/spyc`,
  synced from Bitbucket on every push. Bitbucket's public-repo
  discoverability is worse than GitHub's, and the target audience
  expects a GitHub URL. Mirror, don't migrate.
- **Docs site.** `mdbook` rendered to Bitbucket/GitHub Pages.
  Getting started, keymap reference, `.spycrc.toml` DSL reference,
  agent workflow guide. Auto-built from the `docs/` directory on
  release.
- **Built-in `:tutor` (vimtutor-style).** Interactive walkthrough on a
  pre-baked scratch directory that teaches motions, marks, picks, the
  `=` filter, the pager, the pane (`^a` family), MCP context, and
  sessions. Each lesson sets a concrete goal ("pick three files
  matching `*.rs`", "open the pager and search for `foo`", "beam
  picks to the pane"), watches for the action, and advances; user can
  quit at any step with `:q`. Invoked as `spyc --tutor` or `:tutor`
  from inside a session. Goal: give a Show-HN / blog-post reader one
  command that demonstrates the power of the vi-keyboard terminal
  workflow without forcing them to internalise a keymap reference
  first -- complements the docs site rather than replacing it.
  Maintenance: tutor content tracks bindings, so add it to the
  "Keep docs in sync" checklist in AGENTS.md when keybindings move.
- **First-run hint flash.** On the very first launch in a new
  `$HOME` (detected by absence of `state_root()/first_run_done`
  marker file — write it after the flash fires), display a short
  status-bar flash that calls out the two highest-friction
  things: (1) `^a` and `^w` are reserved chord prefixes and
  won't reach a shell running in the pane, rebind in
  `.spycrc.toml` if needed; (2) `?` opens the help overlay.
  Single line, dismisses automatically after ~8 s or on the
  first keystroke. Reported by Justin (in tmux, hit ^a/^w
  conflicts immediately) — saves the next 100 shell-heavy
  users the same surprise. Implementation: ~30 lines + a tiny
  state file. Pairs with the docs note we added to the README.

- **Repo hygiene.** `SECURITY.md` (how to report vulnerabilities),
  `CODE_OF_CONDUCT.md` (one of the standard ones, link only),
  PR template, issue templates for bug reports and feature
  requests. Low effort, sets the tone.
- **`spyc --generate-completion {bash,zsh,fish}`.** Shell
  completions for the (small) CLI surface. Trivial with `clap`
  derive, worth it for the polish signal.
- **Asciinema demo cast.** 30-second cast showing the MCP thesis in
  action: user picks files, asks Claude a question, Claude lists the
  picked files back by name via `get_spyc_context`. Embed in README.

## Non-goals

These are things someone will inevitably ask for. The answer is no, and
the roadmap committing to that saves a lot of drift.

- **Native Windows support.** WSL is the supported story. `portable-pty`
  technically works on Windows but debugging the failure modes is a tax
  we're not paying.
- **Plugin system.** A decade of maintenance debt for a feature 3% of
  users will touch. The `.spycrc` DSL and keymap extensibility are the
  customization surface.
- **Localization.** English only. The target audience reads English
  docs.
- **Telemetry.** Not even anonymized opt-in. The greybeard half of
  the audience will not forgive it and the vibe-coder half won't
  notice it's missing.
- **SLSA L3 / supply-chain theatre.** Minisign signatures + SBOM + a
  reproducible build job are proportionate. Full SLSA attestation
  infrastructure is not.
- **Mouse support beyond what already exists.** Old roadmap mentions
  it; deprioritize indefinitely. The tool is keyboard-first by
  thesis.

## Releases

Semver per `CONTRIBUTING.md`. Version bumps in `Cargo.toml` as part of
the PR that ships the change; the `CHANGELOG.md` entry lands in the
same commit.

**Current: v1.55.2.** The 1.5x line shipped the agent integration
maturing fast — agent-aware scrollback (codex/agy transcripts), session
resume across claude/codex/gemini/agy/zot, an `AgentProfile` registry
that collapsed ~10 per-agent dispatch sites, untracked/worktree git
marker fixes, the activity-monitor process line, and the `install-debug`
profile. See `CHANGELOG.md` for the full per-version history; this
section tracks only the milestone sequence ahead.

### Road to 2.0 (lean: refactor → launch)

2.0 is a public-distribution + signaling bump. The path there is
deliberately lean: make the codebase reviewable, land the MVU foundation,
fix the daily-driver papercuts, finish distribution, launch. The deep
structural arc that *remains* 2.x is the typed protocol + crate split +
multi-instance hub — see "Post-2.0" below. **The MVU rewrite moved into
this road-to-2.0 list (2026-05-30 decision)** so 2.0 ships on the clean
foundation instead of carrying it as overhang; it's safe to land pre-2.0
because the strangler-fig plan ([`docs/MVU_PLAN.md`](docs/MVU_PLAN.md))
makes every phase behavior-equivalent behind green CI.

1. **Decomposition (the next track).** Break `app/mod.rs` (now ~12k
   lines, single crate) into reviewable modules per
   [`REFACTOR_PLAN.md`](REFACTOR_PLAN.md) **Phases 1–2** (mechanical
   extractions → render/key-dispatch/command extractions). No behavior
   change; interleavable with feature work. This is both the
   maintainability win that lets outside contributors land PRs *and*
   the prerequisite that makes the 2.x crate split possible (you can't
   split a 12k-line monolith into `spyc-proto`/`spyc-pty`/`spyc-os`
   first).
1b. **MVU rewrite (now pre-2.0).** The full Model-View-Update migration
   per [`docs/MVU_PLAN.md`](docs/MVU_PLAN.md) — strangler-fig, 8 phases,
   each behavior-equivalent behind green CI. **Phase 0 (Focus-as-one-value)
   lands first as a daily-driver bug fix** (the recurring focus-confusion
   class); Phases 1–6 land incrementally, sequenced *after* the test
   de-risking (item 3) so the loop/effect surgery is caught by the harness.
   Interleaves with items 2/4/5; not one unbroken push.
2. **Daily-driver fixes (interleave with the decomposition).** Small,
   high-value, mostly standalone:
   - `^a s` path handoff Option A — anchor sent paths on the pane's
     *live* cwd, not `PROJECT_HOME` (live bug;
     [`docs/PATH_HANDOFF_PLAN.md`](docs/PATH_HANDOFF_PLAN.md)).
   - Configurable startup pane tabs — `[pane] tabs = [...]`
     ([`docs/PANE_STARTUP_TABS_PLAN.md`](docs/PANE_STARTUP_TABS_PLAN.md)).
   - Pane recovery **Phase 0** — cosmetic vt100-snapshot backdrop on a
     just-respawned pane
     ([`docs/PANE_RECOVERY_PLAN.md`](docs/PANE_RECOVERY_PLAN.md)).
   - Cwd-export-on-quit (`--cwd-file`), keymap-DSL completeness +
     `unmap`, PgUp/PgDn pane discoverability — all from the
     Foundations > Remaining list above.
3. **Test de-risking.** The `App` workflow-test harness +
   regression tests for the high-bug-density flows (routing/focus,
   pane↔task transitions, session restore)
   ([`docs/TEST_IMPROVEMENT_PLAN.md`](docs/TEST_IMPROVEMENT_PLAN.md)).
   Raises confidence for the 2.0 push; pairs naturally with the
   decomposition (extracted modules get focused tests).
4. **Thesis features that gate 2.0.** Session forking (`^a f`) and
   prompt templates in `.spycrc.toml` — the two agent-workflow
   features the old roadmap named as 2.0 gates. Both implementable
   on current plumbing.
5. **Distribution / launch hygiene.** The full Distribution >
   Remaining list: release automation (tag → cross-compile →
   artifacts → Homebrew bump), macOS notarization, Linux signing,
   GitHub mirror, docs site (`mdbook`), `:tutor`, shell completions,
   first-run hint, repo hygiene, asciinema demo.

**v2.0 — Public distribution launch.** Cut once: decomposition
Phases 1–2 landed, the **MVU rewrite ([`docs/MVU_PLAN.md`](docs/MVU_PLAN.md))
landed**, the daily-driver fixes + session-forking + prompt-templates
shipped, and the Distribution track is complete.
External announcement (TripStack engineering blog, optional Show HN).
The major bump is a signaling choice as much as a semver one — the MCP
positioning shift + public distribution mark the transition.

### Post-2.0 (2.x) — the structural arc

Held until 2.0 has shipped and stabilized (~2 weeks). These build on
each other in order. (The **MVU rewrite moved out of this list into the
road-to-2.0 track** — 2026-05-30 decision; see above and
[`docs/MVU_PLAN.md`](docs/MVU_PLAN.md). The crate split below still
hard-depends on it being done, which it now is, pre-2.0.)

- **Mise en Place — typed addressability + crate split**
  ([`docs/V1_70_PLAN.md`](docs/V1_70_PLAN.md)). Stations (stable pane
  handles), Plates (structured snapshots), the typed Order-rail
  protocol, and Bell primitives that retire timer hacks like
  `RESTORE_BANNER_SETTLE`. One protocol, three clients (MCP server,
  `spyc-sdk` crate, `spyc` CLI subcommands). Includes the
  single-responsibility crate split (`spyc-proto`/`spyc-pty`/`spyc-os`
  [unsafe isolation]/…) — **hard-depends on the decomposition above.**
- **CounterTop — multi-instance hub**
  ([`docs/V1_60_PLAN.md`](docs/V1_60_PLAN.md)). Peer-spyc discovery, a
  HUD aggregating per-workspace agent state, frame mirroring +
  take-control, `--hub` mode. Rides on Mise en Place's Order rail
  rather than inventing ad-hoc MCP shapes — so it follows V1_70.
- **Auto-approval & action log**
  ([`docs/AUTO_APPROVAL_PLAN.md`](docs/AUTO_APPROVAL_PLAN.md)). Curate
  each agent's *native* permission system + a `:approvals` view.
  Large and partly blocked on codex/gemini permission-schema
  verification.
- **Trailing thesis + QoL.** Session cost telemetry, autocommands,
  generalized beam/sinks, structured event-subscriber socket, pane
  recovery tmux/recipes, path-handoff Options B/C, and the Additional
  Ideas backlog. Community-driven contributions.

> **Doc status:** `docs/V1_5_PLAN.md` is **shipped** (v1.5.0, the
> pager/task-viewer unification) and kept only as historical record.
> The remaining `docs/*_PLAN.md` files are the live, not-yet-started
> designs referenced above.

## Additional Ideas

Lower-priority items retained from the prior roadmap. Will graduate to
one of the tracks above when picked up.

- **Tree-sitter syntax highlighting.** v1.50.61 shipped a
  user-syntax dir so people can drop `.sublime-syntax` files into
  `~/.config/spyc/syntaxes/` and have spyc pick them up — but the
  underlying engine is still syntect (Sublime-Text-format grammars,
  regex-based). Tree-sitter is the modern alternative: incremental,
  more accurate, per-language compiled parsers. It's what
  Neovim / telescope / Helix use under the hood. Switching is a
  real refactor of `src/ui/syntax.rs`: tree-sitter grammars come
  as separate crates (`tree-sitter-typescript` etc.) and either
  ship statically or as user-loadable `.so` plugins. Pairs
  naturally with the `spyc-render-core` crate split in
  [V1_70 Mise en Place](docs/V1_70_PLAN.md). Reported by Spencer:
  "look into telescope for syntax highlighting solutions".

- **Configurable startup pane tabs.** Let `.spycrc.toml` declare K
  tabs that open in the bottom pane at launch, instead of just one.
  Compact array form (`[pane] tabs = ["claude", "bash"]`) plus a
  table form for per-tab cwd/label. No splits, no layout refactor —
  uses the existing tab system. Plan in
  [`docs/PANE_STARTUP_TABS_PLAN.md`](docs/PANE_STARTUP_TABS_PLAN.md),
  which also captures the larger "real splits" ask (horizontal /
  tree / grid options) as a deferred future direction with rationale.
  No urgent driver; opportunistic land.

- ~~**Markdown viewer with source/rendered toggle.**~~ Shipped
  v1.26.0. `pulldown-cmark` + a small custom renderer in
  `src/ui/markdown.rs`. `.md` / `.markdown` files open in
  rendered mode by default; `m` toggles to syntect-highlighted
  source and back. Yank/save always emit the source via
  `source_text()`. Scroll resets to top on toggle (line counts
  differ between views; preserving an absolute index would land
  somewhere arbitrary). Tables, images, and embedded HTML stay
  out of scope as planned.
- **History popup kind routing.** v1.31.0 wired double-Esc in
  vi prompts to open `show_history_popup`, but that helper is
  hardcoded to `state.history` (shell bucket). For `:` (command
  line, which has its own `command_history` since v1.28.0), the
  popup currently shows the wrong bucket. Need to parameterize
  `show_history_popup` with a kind, route the popup's submit
  back to the right history (and the right dispatch:
  `dispatch_command` for `:`, `dispatch_prompt` with
  `ShellCmdCaptured` for `!`/`;`). Same generalization unlocks
  per-bucket `^D` deletes, sync_editor, etc. Estimated ~150
  LOC of careful refactor inside the !? popup machinery.
- **Drag and drop** -- files between spyc and other apps. Native path
  is the OSC 72 DnD protocol (Yazi shipped this in PR #4005, 2026-05-28);
  kitty 0.47.0+ is the only terminal that implements it today, so defer
  the native impl until ≥1 more terminal (iTerm2/WezTerm/Ghostty) adopts
  OSC 72. The path-paste fallback (paste a filesystem path into the
  prompt or `J` and have spyc resolve it) is cheap and independent --
  ship that first.

  On receive, present a drop-action picker rather than a single
  hardcoded behaviour. Candidate actions: (a) send to lower pane as
  raw bytes or as an image attachment when the agent supports it,
  (b) create a new file in cwd with auto-type-detect from the
  payload, (c) add to picks or inventory, (d) open in the pager.
  The "send to lower pane as image" arm is the spyc-shaped one Yazi
  doesn't have -- DnD becomes a routing step into the agent
  conversation rather than just a file copy. See
  `docs/YAZI_COMPETITIVE_REVIEW.md` for the broader framing.
- **Page scroll overlap** in the pager -- keep 2-3 lines of previous
  page visible (`_scroll_skip_page_fraction`).
- **Auto-scroll reading mode** -- continuous scroll at configurable
  speed for hands-free reading.
- **Jump-back in pager** (`''`) -- return to the pre-search/jump
  position, matching the file-list behavior.
- **J prompt live directory preview** -- Show contents of the target
  directory as the user types in the J prompt (like fzf's preview
  window). Builds on the frecency database to make path navigation
  even faster.
- **Macro recording** (`qa` ... `q` ... `@a`) -- vim-style action
  recording and replay. The lowercase `q` key is already reserved for
  this in v1.19.1 (it flashes a hint instead of doing anything else)
  so the binding is free to wire up when the feature lands.
- **Startup/exit command flags** -- `spyc -c "sort mtime"` runs
  commands at launch, `-F` for exit hooks.
- **Stdout on exit** -- emit picks/inventory paths on quit so spyc
  composes with shell pipelines (`spyc | xargs rm`).
- **Conditional status bar expandos** -- `%?git?%branch?` shows a
  segment only when its condition holds. Requires a format-string
  parser; worth it only if the status bar gains more segments.
- **Per-file tags/metadata** -- key-value pairs attached to files,
  usable in filters and autocommands.
- **Bulk rename via `$EDITOR`** (Yazi-inspired). `:rename` (or `R`
  on the file list) opens the current pick-set in `$EDITOR` as a
  newline-delimited list of paths; on save, the buffer is parsed
  as a rename plan (line N of the original maps to line N of the
  edit) and applied as a sequence of `mv` calls. Same model as
  `vidir` / `massren`. Edge cases: blank line = delete (with
  confirm), reordering ignored (rename-by-position only), conflicts
  (target exists, source missing) abort the whole batch with a
  diff-style error pager. No persistent UI surface -- just an
  editor round-trip -- so it doesn't bloat the keymap. Pairs
  naturally with our existing pick model (`t`/`T`).
- **Visual-mode range-pick** (`v`) (Yazi-inspired). Today picks
  are toggle-per-row (`t`) or by glob (`T`). Add a vi-flavored
  visual mode: `v` starts a range from the cursor, motion keys
  extend it (`j`/`k`/`G`/`gg`/`5j` etc.), `Space` or Enter
  commits the highlighted range as picks (additive), Esc cancels.
  Builds on the existing `Mode::VisualSelect` shape that vim
  users already know. Different axis from `T` (glob-filtered)
  and `^T` (all/none) -- this is "select these N adjacent rows"
  which is the natural shape for "pick the four files I just
  scrolled past."
- **Generalized pager picker** (lazygit-inspired). Adapt
  lazygit's `Menu` popup pattern into spyc's existing
  `pager.picker_cursor` machinery so any list-of-options surface
  (project chooser, `W l` worktree picker, branch checkout) is
  a pager mode rather than a fifth overlay. Stays inside
  DESIGN.md's "render *into* the pager" rule. Highest-leverage
  of the lazygit borrows because the scoped-help item below
  builds on it.
- **Context-sensitive prompt-row hint** (lazygit-inspired).
  Paint the most-relevant keys for the active overlay or mode
  into the prompt row using the DIM modifier — only when keys
  differ from list-mode (pager `?/n/s/:N`, finder, `!?`
  history editor, picker). DESIGN.md is explicit that a third
  status row is forbidden, so the prompt row is the only legal
  transient surface for this. Directly addresses the "I know it
  exists but forgot the key" failure mode without a help-overlay
  context switch.
- **Scoped `?` help** (lazygit-inspired). Restructure the
  existing `src/ui/help.rs` dump to lead with the active
  surface's keys, then a collapsed "global / other surfaces"
  tail. Content reorganization, not a new feature; cost is
  near-zero once the generalized pager picker lands and `?`
  can render its scoped section as a picker.

## Done (recent)

Items shipped in the current development cycle, newest first.

- **v1.26.0** -- Markdown viewer (rendered ↔ source toggle, `m`
  in the pager). Headings styled, lists with bullets, fenced code
  blocks syntect-highlighted by language, blockquotes with a left
  rule, links rendered with the destination URL, inline emphasis
  preserved. Yank/save always emit the source.
- **v1.25.0** -- Pager line wrap returns (`W` to toggle, default
  on for content pagers). Wrap done by spyc instead of ratatui's
  Paragraph::wrap so per-span styling and the gutter stay aligned
  on continuation rows -- no recurrence of the v1.21.6
  "Builde$.cs" misalignment.
- **v1.24.0** -- Project-wide-search MCP exposure (M3, completing
  the search track). Four tools: `search_paths` (fuzzy filename),
  `search_content` (ripgrep-matcher content search), plus the
  spyc-shaped `search_picks` and `search_inventory` -- search
  scoped to the user's TUI multi-select state and persistent
  cache, neither of which Claude can see otherwise.
- **v1.23.0** -- `:grep <pattern>` project-wide content search (M2 of
  the project-wide-search track). Embedded ripgrep matcher
  (`grep-regex` + `grep-searcher`, no subprocess), gitignore-aware,
  smart-case, multi-repo walker that descends into sibling-clone
  subrepos the outer `.gitignore` excluded (same shape as the
  v1.22.2 finder fix). Matches stream into a pager as
  `path:line:col: text` so `gf`/`gF` jump from a hit for free.
  Capped at 5000 matches per session.
- **v1.22.x** -- `F` project-wide fuzzy filename finder (M1 of the
  project-wide-search track). Streams candidates from a worker
  thread on a 100K-cap; nucleo-matcher for fzf-style ranking;
  multi-repo descent into sibling-clone subrepos.
- **v1.11.0** -- `PROJECT_HOME`, named sessions, editable start dir,
  top-bar redesign.
  - `PROJECT_HOME` is a sticky per-session project root, auto-set at
    startup when the launch dir contains `.git`. `gh` jumps to it,
    `gP` sets it to cwd. `:project` command manages it from the
    command line. New pane tabs default their cwd to `PROJECT_HOME`.
    Exposed via MCP context.
  - Named sessions: every session now carries a spice-themed display
    name like `SAFFRON_CUMIN`, generated from a list of ~30 spices on
    session creation. Rename with `:name <NEW>`. Shown as the primary
    column in the `-r` session picker and in the top bar (all caps).
  - Start dir is now editable at runtime: `gS` sets it to the current
    directory, `:startdir` manages it from the command line.
    Previously only settable at spyc launch or on session restore.
  - Top bar redesigned: dropped `user@host` (flash with `gU` / use
    `:whoami`, or see it in the `I` overlay). New order:
    `🌶️ | PROJECT_HOME | SESSION_NAME | path | git | suffix`.
- **v1.8.0** -- Writable MCP actions. Five new tools: navigate_to,
  set_filter, pick_files, clear_picks, get_file_content. Command
  channel from MCP server threads to event loop with one-shot reply
  channels. AGENTS.md updated to instruct Claude to use tools
  proactively. Debounce fix for git status refresh.
- **v1.7.0** -- Performance refactor, ^a pane prefix, yank commands,
  activity monitor, pager improvements, exit summary, startup health
  check, README rewrite.
  - Idle CPU dropped from ~12.5% to ~2.5% (context watcher exclusion,
    DEC 2026 synchronized output, build_rows/grid caching,
    active-tab-only draw, adaptive poll).
  - Pane prefix switched from ^w to ^a (screen-style); ^w still works
    as alias. Bindings: ^a c new, ^a n/] next, ^a p/[ prev,
    ^a K/x close, ^a P pipe, ^a r rename, ^a s send, ^a v scroll.
  - Yank prefix: yy=take, yp=yank pane output, yP=yank last prompt.
  - Activity monitor (A toggle): draws/sec, cells/sec, draw reason
    breakdown (pane/event/other), poll rate.
  - Pager: l/w split (line numbers vs whitespace), ? help overlay,
    flash messages in title bar (not behind overlay).
  - Exit summary printed to stdout on quit.
  - Startup health check validates inventory/marks/sessions/graveyard.
  - README rewrite with MCP-first positioning and screenshot.
  - Cursor blink removed (was causing phantom redraws).
  - `git status -unormal` replaces `-uall`.
  - Makefile install shows verbose progress.
- **v1.6.0** -- Unicode-width, CHANGELOG.md, `--version --verbose`,
  inventory rewrite as file cache, `V` edit-in-pane, `:version`/`gV`,
  focus naming ("focus: spyc" / "focus: claude"), hidden file count.
- **v1.5.0** -- MCP context handoff (M14). HTTP MCP server on
  background thread. `get_spyc_context` tool. Conversation-aware
  session restore (`--resume <sessionId>`). Pane tabs stay open with
  `[exited]` label. Session dedup ignores ephemeral `--mcp-config`
  ports.
- **v1.4.0** -- Bidirectional path references (M13). `gf`/`gF` jump to
  file paths in pane output. Path extraction handles bare paths,
  `path:line:col`, backticks, quotes, Claude CLI patterns, diff
  headers, ANSI stripping. Bottom-up scan, dual cwd resolution.
  35 extraction tests.
- **v1.3.x** -- `:cd`, `:sort`, `:marks`, `:set`, `:bprev`/`:bnext`
  buffer history. `.git/index` watch for live git status updates.
- **Earlier** -- Embedded pty pane (M8), multi-tab (M9), context piping
  (M10, ^a s/P/i), git worktrees (M11, W l/n/d), git diff (M12,
  g d/D), `.spycrc.toml` config, keymap DSL, live reload, pager with
  syntax highlighting and streaming, shell integration (!/?/;/$/:),
  vi line editor, marks, picks, inventory, powerline status bar.
