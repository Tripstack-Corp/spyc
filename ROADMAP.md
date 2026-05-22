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

Foundations are ~85% complete. Testing infrastructure, CI pipeline,
handler extraction, unicode-width, startup health check, and
performance refactor are all shipped. Remaining items are lower
priority -- nice to have, not blocking v2.0.

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
  4000+ lines with entangled concerns: domain state, TUI state,
  process lifecycle, rendering caches, file watching. The handler
  extraction (Phases 0-4) separated `AppState` domain logic
  (`AppState::apply` already returns an `ApplyResult` enum — the
  Update half is essentially done). The event loop and render path
  are still fused. Target shape:
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
  extraction Phases 5-6. Big lift -- do incrementally alongside
  feature work, not as a standalone rewrite. The View extraction is
  the natural first slice (no behavior change, mechanical move).
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
the PR that ships the change. The `CHANGELOG.md` entry lands in the
same commit.

- **v1.8** (current) -- Writable MCP actions (navigate_to, set_filter,
  pick_files, clear_picks, get_file_content). Command channel
  architecture. AGENTS.md updated to instruct Claude to use tools
  proactively.
- **v1.7** -- Performance refactor (idle CPU ~2.5%), ^a pane prefix,
  yank commands (yy/yp/yP), activity monitor, pager improvements,
  exit summary, startup health check, README rewrite.
- **v1.9** -- Distribution track. Release automation, macOS
  notarization, Homebrew tap, asciinema demo.
- **v1.51 -- Auto-approval & action log.** Per-agent rule curation
  (claude `.claude/settings.json`, codex `.codex/config.toml`,
  gemini TBD) plus a `:approvals` pager view aggregating tool
  invocations across all three agents with auto/manual tags.
  Architecture: curate each agent's native permission system, no
  pty-level interception. Six-phase plan in
  [`docs/AUTO_APPROVAL_PLAN.md`](docs/AUTO_APPROVAL_PLAN.md).
- **v1.52 -- Pane state recovery.** Cross-restart continuity beyond
  what `Session` JSON already captures. Phase 0: cosmetic vt100-grid
  snapshot rendered as a faded backdrop on the just-respawned pane
  until the new process produces output. Phase 1: MCP-side Claude
  sid capture to eliminate the banner/JSONL race. Phase 2 (opt-in):
  `[pane] use_tmux` flag that wraps each pane in a hidden tmux
  session for real cross-restart process survival. Tiered design
  written up in [`docs/PANE_RECOVERY_PLAN.md`](docs/PANE_RECOVERY_PLAN.md).
- **v1.60 -- CounterTop.** A hub view above any one spyc instance.
  Discovery surface so peer spycs find each other, a HUD that
  aggregates per-workspace agent state, `--hub` mode for "launch one
  spyc on terminal open and run every project from there." Kitchen
  vocabulary for the UI surfaces (CounterTop, Burner, Pass, Spice
  Drawer). Six-phase plan in [`docs/V1_60_PLAN.md`](docs/V1_60_PLAN.md).
- **v1.70 -- Mise en Place.** Programmatic addressability: stable
  pane handles (stations), structured snapshots (plates), a typed
  daemon protocol (the Order rail), and async "ready" primitives
  (bells) that replace timer-based heuristics like
  `RESTORE_BANNER_SETTLE`. Three clients share one protocol: the
  existing MCP server (renamed), a new `spyc-sdk` crate, and `spyc`
  CLI subcommands aimed at coding harnesses (`spyc send-keys`,
  `spyc plate`, `spyc wait`). Rmux-inspired crate split into
  single-responsibility seams (`spyc-proto`, `spyc-pty`, `spyc-os`,
  …). Seven-phase plan in [`docs/V1_70_PLAN.md`](docs/V1_70_PLAN.md).
- **v2.0** -- Public distribution launch. Gated on: thesis-track items
  #1-#2 shipped (session forking, prompt templates), remaining
  Distribution track complete. External announcement: TripStack
  engineering blog post, optional Show HN. Target: mid-to-late May
  2026.
- **v2.x onward** -- Remaining thesis items (session cost telemetry,
  autocommands), feature work from Additional Ideas section,
  community-driven contributions.

The v2.0 version bump is a signaling choice as much as a semver one.
The tool has been shipping 1.x for a while, but the MCP positioning
shift + public distribution justifies a major bump to mark the
transition.

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
- **Drag and drop** -- files from the desktop into spyc via OSC 52 or
  path paste.
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
- **Cwd export on quit** (Yazi-inspired). Yazi's `q` writes the
  cursor's cwd to a path the parent shell wrapper sources, so the
  shell follows. Add a `--cwd-file <path>` flag (or
  `$SPYC_CWD_FILE` env var); on quit, write the file-list cwd to
  it. Document a tiny zsh/bash function in INSTALL.md that wraps
  `spyc` and `cd`s the parent shell on exit. ~30 lines of code +
  doc snippet. Nice UX win for users who use spyc as their primary
  navigator -- `q` becomes "go here in my shell" instead of "back
  to where I started." `Q` keeps the no-export semantics so users
  who *don't* want this can opt out per-quit.
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
