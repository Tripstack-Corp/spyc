# spyc roadmap

The single living roadmap. Strategy, backlog, launch plan, and the
post-2.0 arc all live here; `BACKLOG_DRAFT_NOTES.md` is the owner's raw intake
backlog, and `CHANGELOG.md` is the shipped history. Detailed designs
for not-yet-started work live in `docs/*_PLAN.md` (indexed at the
bottom); shipped plans are archived in `docs/archive/`.

## Thesis

spyc is a vi-keyboard-driven file commander that exposes itself to an
AI coding agent as a queryable context source. The target user is a
developer who already thinks in vi motions and wants Claude Code
living in the same workspace -- not one window over, not in a browser
tab, in the same session, sharing context.

The MCP server shifted the tool's nature: spyc isn't just "a file
manager with Claude in a pane." It's a file manager that Claude can
query -- current directory, cursor, picks, inventory, filter, git
branch -- via a standard protocol. That bidirectional awareness is the
positioning that differentiates spyc from `tmux` + `claude`.

Every other feature -- picks, inventory, pager, status bar, sessions --
is supporting infrastructure that makes the split-pane workflow fast
and comfortable. The roadmap is organized accordingly: the
pane-and-agent integration is the defining work track, not the
trailing milestone.

## Where we are (v1.57.0)

The structural foundation is **done**: the full MVU/Elm migration
(Model/Runtime/ViewState split, effects-as-data, single message
channel, pure render), the `app/mod.rs` decomposition (12.4k → ~1k
lines, ceiling-guard-enforced), the 800-LoC file rule, the complete
git→gix migration (100% in-process, guard-enforced, with in-house
side-by-side diff/show/blame views), off-thread PagerStream (grep /
git-view / agent transcripts on one seam), and unified input routing
(`route_input`/`InputSink`, `Focus` as the routing authority).
AGENTS.md is the architectural contract + conventions; ARCHITECTURE.md
holds the deep design decisions. What remains before 2.0 is daily-driver
papercuts, two gating thesis features, and the distribution/launch
pass — see "Road to 2.0."

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

## Road to 2.0

2.0 is a public-distribution + signaling bump. The path is
deliberately lean: fix the daily-driver papercuts, finish the gating
thesis features, finish distribution, launch. (The structural items
that used to lead this list — decomposition and the MVU rewrite —
shipped; the deep structural arc that *remains* is post-2.0.)

1. **Daily-driver fixes.** Small, high-value, mostly standalone:
   - `^a s` path handoff Option A — anchor sent paths on the pane's
     *live* cwd, not `PROJECT_HOME` (live bug;
     [`docs/PATH_HANDOFF_PLAN.md`](docs/PATH_HANDOFF_PLAN.md)).
   - Configurable startup pane tabs — `[pane] tabs = [...]`
     ([`docs/PANE_STARTUP_TABS_PLAN.md`](docs/PANE_STARTUP_TABS_PLAN.md)).
   - Pane recovery **Phase 0** — cosmetic vt100-snapshot backdrop on a
     just-respawned pane
     ([`docs/PANE_RECOVERY_PLAN.md`](docs/PANE_RECOVERY_PLAN.md)).
   - Cwd-export-on-quit (`--cwd-file`), keymap-DSL completeness +
     `unmap`, PgUp/PgDn pane discoverability — all under
     "Foundations backlog" below.
2. **Test de-risking (remainder).** The `App` workflow harness and
   render snapshots shipped
   ([`docs/TEST_IMPROVEMENT_PLAN.md`](docs/TEST_IMPROVEMENT_PLAN.md)
   Phase 1); remaining: workflow tests for pane/pty lifecycle
   (zoom, `^a v`, tab lifecycle), Quick Select end-to-end,
   background-task flows (`^Z`/`:fg`), and session restore.
3. **Thesis features that gate 2.0.** Session forking (`^a f`) and
   prompt templates in `.spycrc.toml` — both described under "Thesis
   backlog," both implementable on current plumbing.
4. **Distribution / launch.** The launch plan below, end to end.

**v2.0 — public distribution launch.** Cut once the daily-driver
fixes, session forking, prompt templates, and the launch plan are
done. External announcement (TripStack engineering blog, optional
Show HN). The major bump is a signaling choice as much as a semver
one — the MCP positioning shift + public distribution mark the
transition.

## Launch plan (2.0)

Benchmarked against Yazi (github.com/sxyazi/yazi, ~37k stars) as the
gold-standard reputable TUI tool. The MCP / Claude-Code pairing
remains spyc's differentiator — Yazi has nothing like it; keep it
front and centre. Goal: a release that someone reading the repo cold
can trust enough to make their daily file manager. Not a promotion
blitz — just enough signal to feel "this is real, maintained, and
works for me."

### Open decisions

- [ ] **Repo home: GitHub move vs mirror.** The two positions on
  record, unresolved — **decide before any github-side work**:
  - *Move* (the launch-prep recommendation): create
    `github.com/<org>/spyc` as canonical, push full history, retire
    Bitbucket — single source of truth. Org choice (Etraveli vs
    Tripstack vs personal) keys everything downstream: `Cargo.toml
    repository =`, `.github/` workflows, Homebrew tap namespace.
  - *Mirror* (the earlier roadmap position): read-only GitHub mirror
    synced from Bitbucket on every push; Bitbucket stays canonical.
    Cheaper, but splits attention and GitHub-native flows (issues,
    releases, Actions) stay second-class.
  - Operational reality to weigh: the team currently runs on
    Bitbucket (bkt, Pipelines, branch conventions).
- [ ] **License footer.** Already BSD-3-Clause in `Cargo.toml`;
  confirm for public release and that LICENSE is at repo root.
- [ ] **Status statement wording.** Default proposal: *"Public beta,
  daily-driver-ready. macOS and Linux."*

### Required for 2.0

1. **Repo move/mirror execution** (per the decision above): public
   repo, history + tags pushed, `Cargo.toml` repository field,
   README/INSTALL link updates, branch protection on `main`.
2. **Demo capture at top of README.** 30–60s asciinema or MP4 of the
   full Claude pairing loop: launch → `F` fuzzy-find → `:grep` →
   `^\` to Claude → "what files am I picking?" answered via
   `get_spyc_context` → `gf` jump on a path Claude mentions → quit.
   Place as the first media element after the value prop.
3. **Release pipeline + binaries.** Tag push triggers cross-compile
   matrix — macOS arm64 + x86_64, Linux x86_64 + arm64 (musl,
   static) — with artifacts attached to Releases. Homebrew tap
   (`brew tap <org>/spyc && brew install spyc`) auto-bumped from the
   release workflow. crates.io publish (binary-only crate,
   acceptable). AUR `spyc-bin` deferred post-2.0 unless a volunteer
   emerges.

### Cheap wins — batch with the launch pass

- **README hygiene**: stale status line replaced with the agreed
  status statement; headline sells the Claude angle in one sentence;
  spot-check keybinding tables.
- **Repo scaffolding**: issue templates (bug: repro/version/OS/
  terminal; feature: what/why/would-you-use-it), PR template,
  CODE_OF_CONDUCT (Contributor Covenant, link only). SECURITY.md ✅
  exists.
- **`MIGRATION.md`**: three small keybind tables (ranger → spyc,
  lf → spyc, Yazi → spyc, ~10 binds each) plus one paragraph on what
  spyc has that they don't (the MCP integration). Unblocks the two
  remaining Yazi-review recommendations
  ([`docs/YAZI_COMPETITIVE_REVIEW.md`](docs/YAZI_COMPETITIVE_REVIEW.md)).
- **Signing & supply chain**: macOS Developer ID signing +
  notarization (without it the first user report is "macOS says spyc
  is damaged"); Linux minisign signatures with the public key in the
  repo; SBOM via `cargo-sbom`/`cargo-auditable`; reproducible-build
  verification job (toolchain already pinned, `SOURCE_DATE_EPOCH`,
  rebuild-and-diff). Proportionate — no SLSA theater (see Non-goals).
- **Shell completions**: `spyc --generate-completion {bash,zsh,fish}`
  via clap derive; ship in release artifacts.
- **First-run hint flash**: on first launch (no
  `state_root()/first_run_done` marker), flash that (1) `^a`/`^w` are
  reserved chord prefixes (rebindable) and (2) `?` opens help.
  ~30 lines; saves every tmux/shell-heavy user the same surprise.
- **`:tutor` (vimtutor-style)**: interactive walkthrough on a
  pre-baked scratch directory — motions, marks, picks, `=` filter,
  pager, `^a` family, MCP context, sessions. Each lesson sets a goal,
  watches for the action, advances. The one-command demo for a
  Show-HN reader. Tutor content tracks bindings — add to the AGENTS.md
  doc-sync checklist when it lands.

### Explicitly deferred (not 2.0)

- Dedicated docs site (mdbook/Starlight). The Markdown reads fine on
  GitHub; revisit if docs outgrow single files.
- Blog/marketing posts beyond one Show HN at 2.0. CHANGELOG is enough.
- Windows support (see Non-goals — WSL is the story).
- Discord/Matrix/forum. GitHub Discussions post-launch if traffic
  warrants; a chat channel is a maintenance commitment.
- Sponsorship buttons, until traction warrants.

### Done-criteria for the 2.0 launch

A user landing on the repo cold should be able to:

1. Watch a 30-second demo in the README and understand what spyc does
   and why it's different.
2. Install via Homebrew *or* a pre-built Release binary — no Rust
   toolchain required.
3. Read FEATURES.md and INSTALL.md without broken links or stale
   version numbers.
4. File a bug or feature request via templated issues.
5. See a recent release (within ~30 days) and a current CHANGELOG.
6. Read a clear 2.0 CHANGELOG entry: what changed since 1.x, what
   stability we promise going forward.

Sequencing: repo decision first (blocks everything) → README hygiene
→ demo capture → scaffolding → release pipeline + first 2.0 binaries
→ Homebrew → migration page. The 2.0 CHANGELOG entry is written last,
once we've daily-driven our own builds for a few days.

## Foundations backlog

- **Background directory loading.** Large directories (100K+ entries)
  and slow filesystems (NFS, external drives) block the event loop
  because `Listing::read()` and git status run synchronously. Target
  flow: (1) `chdir()` clears rows and sets a "loading" sentinel so the
  View renders a spinner/dimmed list; (2) a worker thread runs the
  read and pushes `Message::ListingReady(Listing)` into the main
  channel; (3) the loop swaps it in; (4) cancellation via a generation
  counter — stale `ListingReady` messages are dropped (don't kill the
  worker; the read is bounded and its result just gets discarded).
  Scoped conservatively: the common case (local NVMe, <1K entries)
  stays synchronous-fast; the spinner only appears past ~50ms. (Only
  the 50k-entry cap mitigation exists today.)
- **Incremental / lazy row rebuild.** `rebuild_rows()` re-clones
  `entry.path` (and reformats `display`) for every visible row on every
  chdir/sort/filter/view change; a wide-open listing near the 50k cap
  clones tens of thousands of `PathBuf`s per rebuild. Two independent
  wins: (1) diff the new listing against the old and skip the rebuild
  (or rebuild only changed rows) when nothing moved, and (2) compute
  `RowData.display` lazily for visible rows only. **Profile first** —
  the `list_generation`-gated render cache already avoids re-cloning
  *across frames*, so the cost is confined to the rebuild itself;
  confirm it's a real hotspot before optimizing.
- **Cursor-by-path across rebuilds.** The file-list cursor is a flat
  `usize` index into `rows`, clamped after every rebuild — so on a
  listing change the selection can jump to a different file than the
  one it was on. Remember the cursor's *path* and re-find it after a
  rebuild (fall back to the clamped index when it's gone). A
  UX/correctness win, not a perf one; pairs with the incremental-rebuild
  item above. (Both items fell out of a 2026-06 investigation into
  arena/slotmap/ECS adoption, which concluded none are warranted —
  spyc's state is flat, path-keyed, and cycle-free; these are the
  domain-level wins that remained.)
- **Cwd export on quit** (Yazi-inspired). `--cwd-file <path>` flag (or
  `$SPYC_CWD_FILE`); on quit, write the file-list cwd; document a tiny
  zsh/bash wrapper that `cd`s the parent shell on exit. `q` becomes
  "go here in my shell"; `Q` keeps no-export semantics as the opt-out.
- **Keymap DSL completeness** (external contributor 2026-05-15). Two
  paired shortcomings in `src/config/dsl.rs`: (1) many `Action`
  variants are unbindable — `parse_action` lacks `HarpoonAppend`,
  `SetMark`/`JumpMark`, `PaneTabByIndex`, the `Yank*` family, `Goto*`,
  `WorktreeList`, `GitDiff*`; either grow the parser or document the
  bindable set explicitly. (2) `unmap` is a no-op (`Ok(None)` with a
  TODO) — wire it through so users can remove built-in bindings.
  Tackle as a pair: shared parser, shared documentation.
- **PgUp/PgDn discoverability in panes** (external contributor
  2026-05-15). (1) Shift-PgUp in a focused pane auto-enters `^a v`
  scrollback with one page applied (guard `!is_alternate_screen()`;
  use a modifier so the child's own PgUp isn't stolen). (2) First
  pane-focus this session flashes `^a-v scrolls history` for ~2s.
  Pair them — the hint explains the binding.
- **Mouse forwarding to the pane.** spyc never calls
  `EnableMouseCapture` and the pane input path has no `Event::Mouse`
  encoder, so mouse-first TUIs in the pane (lazygit, htop, broot)
  look half-broken. Two-layer fix: enable capture on the host
  terminal *and* encode SGR mouse reports to the pty — but only when
  the pane is focused; the file list keeps its minimal mouse
  semantics. The keyboard-first thesis tension is acceptable here:
  the pane's contents are third-party and mouse-aware, so refusing to
  forward effectively breaks those tools.

## Thesis backlog

- **Background tasks M3/M4** (M1 `^Z`/`:fg`, M2 task viewer, kill,
  and pause/resume all shipped — see CHANGELOG v1.20–v1.37).
  Remaining: **M3 — `!&cmd` direct-launch** (skip the foreground
  pager; symmetric `:!&cmd` / `:bg cmd`), `r`-to-re-run on an exited
  task, and **M4 — polish**: optional completion notify
  (`[notify] on_task_complete = "bell"`, off by default), exited
  pane-tabs as post-mortem task records, and a `get_running_tasks`
  MCP tool so Claude can ask "what's running?" and tail output.
- **Vertical (left/right) split.** **Stage 1 shipped (v1.59.0):** `^a |`
  cycles a right-hand column off → top-only → full-height, hosting a
  **live-reloading** preview of the cursor file (markdown rendered); `^a a`/`^a h`
  and `^a b`/`^a l` focus the a/b columns, `^a +`/`^a -` resize the width, `^a z`
  zooms the active column, `^a d` toggles the focus-dim. The preview re-renders
  off-thread on save (parent-dir watch survives replace-on-save) and re-wraps on
  resize. **Stage 2 shipped:** `^s n` opens a second *full file-commander* in
  column b (`^s x` / `^d` close it) — its own process cwd + listing, per-column
  git cache + status map (dual fs-watch), per-column harpoon, and search/MCP
  tools scoped to the focused column's worktree root (`tool_root` /
  `harpoon_root`). The focused column drives MCP context, and both cwds are
  saved/restored across sessions (`SavedVsplit.right_cwd`); the `open_worktree` /
  `clean_worktree` MCP tools operate on column b. See ARCHITECTURE.md.
- **Context enrichment.** `get_spyc_context` returns paths and
  metadata; could add pick snippets, recent `cargo check` errors,
  unstaged diffs. Makes Claude's context richer without explicit
  piping. Scope carefully — large payloads need truncation/pagination.
- **Generalized "beam" — send content to any stdin sink.** Extends
  `^a s` / `^a P` / `^a i` along three axes: (1) region beam from the
  pager — visually-selected or `:N,M` ranges, wrapped with a
  `path:N-M` header and fenced code block; (2) configurable sinks —
  active tab (default), tab by index, OSC 52 clipboard, arbitrary
  shell command (`:beam !pbcopy`, `:beam !jq .`), named sinks in
  `.spycrc.toml`; (3) per-target format wrappers — raw, paths-only,
  fenced-with-header, diff-style. The lower-level primitive prompt
  templates sit on top of; reuses `pane.send_bytes()` + a dispatch
  table.
- **Image paste (`^v`) to the agent pane.** When the clipboard holds
  an image, `^v` from the file list focuses the pane and sends it as
  an attachment (Claude supports this today). Shares routing logic
  with the DnD drop-action picker (Additional Ideas) — implement the
  routing once, expose via both. `^v` is currently a no-op outside
  prompts, so the binding is free.
- **Session forking (`^a f`)** — 2.0 gate. Duplicate a pane tab with
  scrollback replayed so a Claude conversation can branch without
  losing the prior line of inquiry. Implementable with current
  plumbing.
- **Prompt templates in `.spycrc.toml`** — 2.0 gate. User-defined
  macros that send a pre-composed prompt with picks/inventory
  substituted — e.g. `map "<space>cr" claude-template review`. Turns
  spyc into a keyboard-driven Claude launcher for repeated workflows.
- **Session cost telemetry.** Read the active Claude session's JSONL
  (the same file resume already locates), sum input/output/cache
  tokens against a small built-in pricing table. Surface in the `I`
  overlay (`session: $0.42 (37k in / 12k out)`), optionally a top-bar
  segment behind a config flag, and a `get_session_cost` MCP tool so
  Claude can self-report — the spyc-shaped angle: only spyc sees
  Claude's own JSONL via MCP. Pricing table is hardcoded constants
  for current Claude models; multi-provider/currency/dashboards stay
  out of scope.
- **Autocommands**, scoped to the agent workflow — `autocmd
  "*.test.ts" "claude-template test-review"`. Defer until templates
  land and the shape is clear.
- **MCP peer credential checking.** Socket permissions + path
  containment shipped; remaining defense-in-depth: `SO_PEERCRED`
  (Linux) / `LOCAL_PEERPID` (macOS) UID verification. Low priority.
- **Structured event stream (subscriber socket)** (Yazi-inspired).
  Add a subscribe verb to the existing PID-scoped Unix socket: a
  subscriber registers an interest set (`cd`, `cursor`, `pick`,
  `filter`, `task_state`, `quit`, …) and receives a JSON-line stream
  of `{ts, type, payload}` events. Opens spyc state to *non-Claude*
  tools (tmux status segment, Neovim cursor-follow, desktop notifier)
  without the MCP stdio handshake. No publish side (that's
  autocommand territory and a wider attack surface). Backpressure:
  drop past a small per-subscriber buffer; events are advisory.
  Implementation hook: centralize emission behind `emit_event(Kind)`
  at the sites that already bump `last_context_json` / `needs_draw`.

## Tooling

- **macOS step in CI.** Linux-only CI lets
  `#[cfg(target_os = "linux")]`-related clippy errors through
  (surfaced by PR #87); local `make check` on macOS catches them too
  late for external contributors. Cheapest path: a parallel
  `macos-quality` step on main + PRs. **Deferred until after the OSS
  launch** (macOS CI minutes are scarce/pricey); the agreed interim
  workaround: the PR template asks cross-platform contributors to
  confirm `make check` passes locally. (`make lint-linux` covers the
  reverse direction from macOS dev machines.)
- **Renovate bot (post-OSS launch).** Today: weekly `cargo deny check
  advisories` + `cargo outdated` via the `weekly-deps` scheduled
  pipeline with Slack notification. Once the repo is public (Renovate
  free tier requires it), wire `renovate.json` with
  `config:recommended`, `rangeStrategy: bump`. **Ratified decision
  (May 2026): auto-merge patch bumps when CI passes** — the suite
  gives enough confidence and the noise reduction beats the rare
  bad-patch risk; minor bumps grouped weekly; majors individual,
  labeled `needs-review`.
- **sccache for CI (GCS-backed).** Warm-cache `make check` is ~2m13s;
  the remainder is proc-macro/build-script crates cargo won't trust
  across runners. sccache caches at the rustc-invocation level.
  Wiring: GCS bucket in the Tripstack GCP project + scoped service
  account (key as a masked repo variable; later OIDC/WIF),
  `RUSTC_WRAPPER=sccache` in the quality and coverage steps. Do it
  when PR friction warrants; current caching is good enough.
- **Merge autopilot — `:land` (PARKED, needs more thought).** Kill the
  merge-train traffic-copping: push → CI → main moved → rebase → repeat,
  done by hand. Diagnosis: the recurring conflict is *always* the
  per-PR `Cargo.toml` version line + `Cargo.lock` spyc entry — never
  scope. So the win is mechanical, not a multi-agent "conductor"
  (scope-negotiation is over-fit; the remote is the real serialization
  authority and spyc coordination is per-instance/one-socket anyway —
  explicitly **not** building that).
  - **Tier 1 — SHIPPED #608 (v1.77.0).** `spyc-semver` git merge driver
    (`src/merge_driver.rs`): `.gitattributes` routes `Cargo.toml` through
    `spyc --merge-driver`, a 3-way merge in-process (diffy, no git
    subprocess) that keeps the higher semver for a version-only conflict;
    `Cargo.lock` uses `merge=ours` + a `cargo` regen. Installed into the
    repo's git config on launch. Every PR *after* it auto-resolves the
    version line on rebase (it couldn't help its own introducing PR —
    `.gitattributes` wasn't on main yet).
  - **Tier 2 — `:land` autopilot (the parked piece).** A command that
    runs the loop off the main thread (rebase onto fresh main → driver
    auto-resolves → force-push → `bkt pr merge` → conflict-lap → repeat),
    reported as a background task. **Open design fork:** the loop's core
    moves — `git rebase` and especially `git push` — can't be done via
    gix (networking/credentials features are off by design; gix push is
    nascent; no rebase API), so it *must* invoke the `git` CLI, which the
    `no_subprocess_git_in_production` guard forbids in spyc's Rust. Two
    ways through, undecided: (a) ship `scripts/land.sh` and launch it via
    the existing background-task machinery (spyc's Rust never spawns git;
    guard intact; logic in bash); (b) a Rust off-thread worker with a
    documented guard carve-out for the autopilot module. Lean (a), but
    parked to think it through.
  - **Tier 3 (only if real scope collisions ever appear).** A single
    *merge mutex* on the worktree-lease substrate (`claim_worktree`) so
    concurrent `:land`s queue rather than race — plus an MCP `land_pr`
    tool. The useful 5% of "the conductor"; not scope declaration.

## Post-2.0 (2.x) — the structural arc

Held until 2.0 has shipped and stabilized. These build on each other
in order; the MVU prerequisite they used to wait on is done.

- **Mise en Place — typed addressability + crate split**
  ([`docs/V1_70_PLAN.md`](docs/V1_70_PLAN.md)). Stations (stable pane
  handles), Plates (structured snapshots), the typed Order-rail
  protocol, and Bell primitives (observed waits) that retire timer
  hacks like `RESTORE_BANNER_SETTLE` and the resume verify-retry
  loop. One protocol, three clients: MCP server, `spyc-sdk` crate,
  `spyc` CLI subcommands. Includes the single-responsibility crate
  split (`spyc-proto`/`spyc-pty`/`spyc-os` for unsafe isolation/…).
  Lands *before* CounterTop so the hub rides a real protocol.
- **CounterTop — multi-instance hub**
  ([`docs/V1_60_PLAN.md`](docs/V1_60_PLAN.md)). Peer-spyc discovery,
  a HUD aggregating per-workspace agent state, frame mirroring +
  take-control, `--hub` mode. Rides on the Order rail.
- **Auto-approval & action log**
  ([`docs/AUTO_APPROVAL_PLAN.md`](docs/AUTO_APPROVAL_PLAN.md)).
  Curate each agent's *native* permission system + a `:approvals`
  view. Large; partly blocked on codex/gemini permission-schema
  verification (the plan's open questions 1–2).
- **Trailing thesis + QoL.** Cost telemetry, autocommands,
  generalized beam, the event-subscriber socket, pane-recovery
  tmux/recipes phases, path-handoff Options B/C, and the Additional
  Ideas backlog. Community-driven contributions.

## Needs investigation

Items where the *approach* is unknown — they need a discovery spike
before graduating to a track.

- **Ollama harness scrollback (`^a v`).** Goal: scrollback for an
  ollama-backed agent pane, the way Claude's full-screen mode gets it.
  **Findings so far (2026-06-09):**
  - Plain `ollama run <model>` (and `… --experimental`, the agent
    loop) is an **inline readline REPL — NOT alternate-screen**
    (probed: no `\e[?1049h`; just `\e[2K`/`\e[1G` line redraws +
    `\e[?2026` synchronized output). So vt100 scrollback already
    applies to the plain REPL. A plain-`ollama` `AgentProfile`
    (detection + `AgentKind::Ollama` + restore-as-fresh) is
    **complete but PARKED, unmerged, on branch
    `feat/ollama-agent-profile`** — merge it if plain-REPL
    recognition is wanted on its own.
  - The user's actual "ollama harness" is a **full-screen (alt-screen)
    wrapper / third-party tool** backed by ollama (not plain
    `ollama run`). So `^a v` dead-ends with "scroll: alt-screen app —
    use its own scrollback" — the exact wall Claude's full-screen
    mode hit.
  - **ollama itself persists no conversation:** no per-session
    transcript file (unlike `~/.claude/projects/<slug>/<id>.jsonl`),
    `~/.ollama/logs` are operational only, and the HTTP API is
    stateless (the client passes full history each call). So there is
    **no clean-transcript source** to reconstruct from.
  - **To proceed, two unknowns must be resolved:** (1) the wrapper's
    exact command/binary — needed for
    `AgentProfile::matches_command` detection; (2) whether/where that
    wrapper persists the conversation on disk. If it does, the fix is
    a `TranscriptSpec` (`resolve` + `render`) exactly like the Claude
    path (`state::claude_transcript`, auto-engaged on the alt screen
    via `pane_scroll::decide_scroll_source`). If it keeps history
    only in memory, alt-screen scrollback needs a different capture
    mechanism (no source exists).
  - **Template:** the Claude full-screen scrollback (PR #309) is the
    working pattern to copy once the wrapper is identified.

## Additional Ideas

Lower-priority items. Will graduate to a track when picked up.

- **Tree-sitter syntax highlighting.** v1.50.61 shipped a user-syntax
  dir (`~/.config/spyc/syntaxes/`, `.sublime-syntax`), but the engine
  is still syntect (regex-based). Tree-sitter is incremental, more
  accurate, and what Neovim/Helix use — a real refactor of
  `src/ui/syntax.rs` (grammars as crates, static or `.so`-loadable).
  Pairs with the `spyc-render-core` crate split in V1_70. (Reported
  by Spencer.)
- **History popup kind routing.** Double-Esc opens
  `show_history_popup`, but it's hardcoded to the shell bucket; for
  `:` (which has its own `command_history`) it shows the wrong one.
  Parameterize by kind, route submit to the right history and
  dispatch. Same generalization unlocks per-bucket `^D` deletes.
  ~150 LoC inside the `!?` popup machinery. (Still live — KNOWN
  LIMITATION comment in `src/app/key_dispatch/prompts.rs`.)
- **Drag and drop.** Native path is OSC 72 (Yazi PR #4005); only
  kitty implements it today, so **defer the native impl until a
  second terminal adopts it**. The path-paste fallback (paste a path
  into a prompt/`J` and resolve it) is cheap and independent — ship
  first. On receive, present a drop-action picker: send to pane as
  raw bytes or image attachment (the spyc-shaped arm Yazi doesn't
  have), create file in cwd, add to picks/inventory, open in pager.
  See [`docs/YAZI_COMPETITIVE_REVIEW.md`](docs/YAZI_COMPETITIVE_REVIEW.md).
- **Page scroll overlap** in the pager — keep 2–3 lines of the
  previous page visible.
- **Auto-scroll reading mode** — continuous scroll at configurable
  speed.
- **Jump-back in pager (`''`)** — return to the pre-search/jump
  position, matching the file list.
- **J-prompt live directory preview** — fzf-style preview of the
  target dir while typing; builds on frecency.
- **Macro recording** (`qa` … `q` … `@a`) — `q` is already reserved
  (flashes a hint), so the binding is free.
- **Startup/exit command flags** — `spyc -c "sort mtime"`, `-F` exit
  hooks.
- **Stdout on exit** — emit picks/inventory on quit so spyc composes
  with pipelines (`spyc | xargs rm`).
- **Conditional status-bar expandos** — `%?git?%branch?`; needs a
  format-string parser; only worth it if the bar gains segments.
- **Per-file tags/metadata** — key-value pairs usable in filters and
  autocommands.
- **Bulk rename via `$EDITOR`** (Yazi-inspired, vidir-style). Picks
  open in `$EDITOR` as a path list; on save, parsed as a
  rename-by-position plan applied as `mv`s. Blank line = delete (with
  confirm); conflicts abort the batch with a diff-style error pager.
- **Visual-mode range-pick (`v`)** (Yazi-inspired). `v` starts a
  range, motions extend, Space/Enter commits as additive picks, Esc
  cancels. The "pick the four files I just scrolled past" shape that
  `t`/`T`/`^T` don't cover.
- **Generalized pager picker** (lazygit-inspired). Adapt the Menu
  popup pattern into the existing `pager.picker_cursor` machinery so
  any list-of-options surface (project chooser, worktree picker,
  branch checkout) is a pager mode, not a fifth overlay. Stays
  inside DESIGN.md's "render *into* the pager" rule.
- **Context-sensitive prompt-row hint** (lazygit-inspired). Paint the
  most-relevant keys for the active overlay into the prompt row (DIM)
  only when they differ from list mode. DESIGN.md forbids a third
  status row; the prompt row is the legal transient surface.
- **Scoped `?` help** (lazygit-inspired). Lead with the active
  surface's keys, collapse the rest. Near-free once the generalized
  picker lands.

## Non-goals

These are things someone will inevitably ask for. The answer is no,
and the roadmap committing to that saves a lot of drift.

- **Native Windows support.** WSL is the supported story.
  `portable-pty` technically works on Windows but debugging the
  failure modes is a tax we're not paying. (V1_70's crate split
  isolates platform code so a future volunteer *could* — that's the
  extent of the commitment.)
- **Plugin system.** A decade of maintenance debt for a feature 3% of
  users will touch. The `.spycrc` DSL and keymap extensibility are
  the customization surface.
- **Localization.** English only.
- **Telemetry.** Not even anonymized opt-in. The greybeard half of
  the audience will not forgive it and the vibe-coder half won't
  notice it's missing.
- **SLSA L3 / supply-chain theatre.** Minisign + SBOM + a
  reproducible-build job are proportionate. Full SLSA attestation is
  not.
- **Mouse support beyond the pane forward.** Keyboard-first by
  thesis; the pane forward (Foundations backlog) exists only because
  the pane hosts third-party mouse-aware tools.
- **tmux command compatibility.** We have our own bindings.
- **Persistent search index** (tantivy/ctags). Ripgrep on a 100K-file
  repo is sub-second cold; the maintenance burden isn't worth it.

## Decisions log

Condensed record of the choices that shaped current behavior — kept
so we don't re-litigate them. Full history in CHANGELOG.md.

- **Sync end-to-end, no tokio.** `std::thread` + one mpsc channel.
  Revisit never; async would be a regression for this workload.
- **MVU landed pre-2.0** (2026-05-30) so the launch ships on the
  clean foundation; strangler-fig, every phase behavior-equivalent
  behind green CI. Shipped.
- **`^Z` backgrounds tasks** despite overriding terminal-suspend
  muscle memory — consistent with spyc trapping most ctrl-combos.
  Backgrounded tasks don't survive `spyc -r` (children tied to the
  spyc PID; reattach is a rabbit hole; quit-time prompt covers it).
- **Task-viewer shape**: exited tasks auto-promote to buffer history
  on view-close instead of an explicit dismiss step.
- **No persistent search index** — see Non-goals.
- **Claude restore types `/resume <sid>`** into a fresh spawn (the
  `--resume` CLI flag has a mount-crash regression) with
  verify-and-retry on the Enter; codex restores via
  `codex resume <UUID>` directly; gemini recomputes its unstable
  resume index from the saved UUID at restore time; zot uses
  `--continue`.
- **OSC 72 DnD deferred** until a second terminal (beyond kitty)
  implements it.
- **Renovate auto-merges patch bumps** once public (May 2026); minors
  grouped weekly; majors labeled.
- **macOS CI deferred to post-launch**; PR template asks
  cross-platform contributors to run `make check` locally.
- **git is 100% in-process gix** in production, guard-enforced; no
  subprocess git, no gix repo open on the 1 Hz poll.
- **Crate-over-handroll**: prefer a small focused crate (features
  trimmed) over shelling out or reimplementing (libproc over
  ps/lsof). "Lightweight" means small runtime + few transitive deps,
  not "avoid crates."
- **No `unsafe` going forward** — DI / rustix / signal-hook over raw
  libc; unsafe is exceptional and isolated (V1_70 gives it a crate).

## Doc map

| Doc | Role |
|---|---|
| `ROADMAP.md` | This file — strategy, backlog, launch, decisions. |
| `BACKLOG_DRAFT_NOTES.md` | Owner's raw intake backlog (small fixes, ideas, reports). |
| `CHANGELOG.md` | Shipped history (git-cliff, conventional commits). |
| `AGENTS.md` | The canonical agent guide: architectural contract (MVU invariants), module map, conventions. |
| `CLAUDE.md` | One-line `@AGENTS.md` import (Claude Code entrypoint). |
| `ARCHITECTURE.md` | Deep stable design decisions. |
| `DESIGN.md` | UI design language (theme, components, glyphs). |
| `FEATURES.md` | User-facing feature reference. |
| `docs/AUTO_APPROVAL_PLAN.md` | Pending design (post-2.0). |
| `docs/PANE_RECOVERY_PLAN.md` | Pending design (Phase 0 is road-to-2.0). |
| `docs/PANE_STARTUP_TABS_PLAN.md` | Pending design (road-to-2.0). |
| `docs/PATH_HANDOFF_PLAN.md` | Pending design (Option A is road-to-2.0). |
| `docs/TEST_IMPROVEMENT_PLAN.md` | Testing campaign charter (coverage + anti-"test theater"). Phase 1 (App harness) shipped; remainder is road-to-2.0. |
| `docs/V1_60_PLAN.md` | Pending design (post-2.0, after V1_70). |
| `docs/V1_70_PLAN.md` | Pending design (post-2.0, first in the arc). |
| `docs/YAZI_COMPETITIVE_REVIEW.md` | Competitive snapshot (2026-05-28); refresh on Yazi's next major. |
| `docs/archive/` | Shipped plans, kept as historical record. |

> **Note on pending plans:** the four feature plans predate the MVU
> decomposition — their designs hold, but `src/app/mod.rs:NNNN`-style
> file pointers are stale; re-resolve against the current module
> layout when picking one up.
