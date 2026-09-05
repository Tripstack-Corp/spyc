# spyc roadmap

The strategy layer — thesis, current state, the 2.2, 2.3 and 3.0 arcs,
non-goals, and the decisions log. The per-item **backlog lives in [GitHub Issues](https://github.com/Tripstack-Corp/spyc/issues)**
(organized on the [roadmap board](https://github.com/orgs/Tripstack-Corp/projects/1));
`CHANGELOG.md` is the shipped history. Detailed designs for not-yet-started work
live in `docs/drafts/*_PLAN.md`; shipped or parked plans are archived in
`docs/archive/`.

## Thesis

spyc is the working set, shared. A vi-keyboard-driven file commander is the
human's view of it; MCP is the agent's. A multiplexer shares a *screen* with an
agent — cells, bytes, scrollback to scrape. spyc shares state that already
means something: cursor, picks, inventory, filter, branch, worktree. The target
user is a developer who thinks in vi motions and wants their agents in the same
session, reading the same context — not one window over, not in a browser tab.

The arc follows from that sentence. 2.2 gives every participant an identity
(pane-identity transport). 2.3 grows the session to everything you're working
on (Projects: one process, many working sets, one attention signal). 3.0 makes
the session outlive the terminal that started it.

The benchmark stays `tmux` + `claude` — better on context today, better on
durability after 3.0, at which point the target user's stack simply no longer
contains tmux. That is the full extent of the tmux claim: we don't replace it
for anyone else; we remove the reason our user runs it.

## Where we are (v2.1.1)

The structural foundation has been **done** for a while: the full MVU/Elm
migration (Model/Runtime/ViewState split, effects-as-data, single message
channel, pure render), the `app/mod.rs` decomposition (12.4k → 425 lines,
ceiling-guard-enforced at 600), the complete git→gix migration
(100% in-process, guard-enforced, with in-house side-by-side diff/show/blame
views), off-thread PagerStream (grep / git-view / agent transcripts on one
seam), and unified input routing (`route_input`/`InputSink`, `Focus` as the
routing authority).

The **800-LoC file rule is a convention with an explicit escape hatch**, stated
in AGENTS.md and enforced nowhere except `mod_rs_stays_decomposed`, which caps
`src/app/mod.rs` alone. Some files are over it. Re-derive the list from the
tree rather than from a number written down here.

The thesis work shipped over 1.x and 2.0: agent-awareness dots and
notifications, the worktree MCP suite, the merge/scope registry, the in-process
review loop, the vertical split, Lua scripting, and the chord/leader overhaul.
The competitive review
([`docs/COMPETITIVE_REVIEW.md`](docs/COMPETITIVE_REVIEW.md)) named these as
spyc's wedge; the mechanics are documented in
[`docs/AGENT_ORCHESTRATION.md`](docs/AGENT_ORCHESTRATION.md), AGENTS.md and
ARCHITECTURE.md.

2.0 was the distribution pass — public repo, signed binaries, brew / apt /
crates.io, Show HN. Its plan is archived at
[`docs/archive/LAUNCH_PLAN_2_0.md`](docs/archive/LAUNCH_PLAN_2_0.md), with the
done-criteria confirmed and the parts that never shipped named. 2.1 shipped:

- **An installable agent skill** (#187). `spyc --install-skill` writes an
  embedded usage guide into Claude Code's, codex's and agy's personal-skills
  dirs, offers a `[Y/n]` update when spyc's copy moves ahead, and never
  clobbers local edits unprompted. The MCP `initialize` handshake has to stay
  short, so the depth ships here instead.
- **agy support, and gemini removed** (#194), with a scrape fallback for the
  `Blocked` state agy's hooks can't report and session pinning from its hook
  payload rather than spawn proximity (#202, #285, #287).
- **The mouse suite, on by default** (#212–#234, plus follow-ups through #387):
  the wheel scrolls whatever is under the pointer, three button gestures,
  drag-select in four surfaces. The Non-goals entry narrowed to match — mouse
  support is no longer a non-goal; mouse-first design still is.
- **Archive browsing** (#301–#334, closing
  [#149](https://github.com/Tripstack-Corp/spyc/issues/149)). Walk into a zip
  or tarball, change what's inside, `:archive write` repacks and verifies. A
  mount is an index, not a directory, so entering a huge zip extracts nothing.
- **Image capture, preview and the `^a g` gallery** (#300, #302, #304) — what
  the agent received, plus anything pasted and not yet sent.
- **Per-character intraline diff highlight**, on by default (#351, #364).
- **A security and correctness pass.** The MCP `root` override is validated
  rather than documented (F1, #237), persistent writes go through
  `write_atomic` (F3, #238), there is one state-root resolver (F6, #239), and
  the fuzz targets build for a target that can run them and go weekly in CI
  (F5, #242). The pre-2.1 review that followed put six reviewers and a referee
  over `v2.0.0..HEAD` and closed 58 findings across eight passes; the reports
  are archived under
  [`docs/archive/review-2.1/`](docs/archive/review-2.1/).

v2.1.1 adds one packaging fix: `docs/ABOUT.md` is compiled into the binary via
`include_str!` and was missing from the published crate (#426).

## Working tracks

Work proceeds along three parallel tracks. They're not strictly sequential;
distribution work can land while thesis work is still in flight, and
foundations work continues throughout.

- **Foundations** -- testing, hardening, build hygiene. The minimum to not
  embarrass ourselves and to make every other change safer.
- **Thesis** -- deepening the agent integration until the split-pane workflow
  is measurably better than `tmux` + `claude` for the target
  audience. This is where the tool earns its reason for being.
- **Distribution** -- release automation, signing, packaging, docs.  Turns a
  repo into a tool people can install, trust, and find.

## Road to 2.2

2.2 lands the prerequisites Projects needs and closes the bugs a daily driver
hits weekly. Every item is one or the other. Scope and sequencing:
[`docs/drafts/V2_2_PLAN.md`](docs/drafts/V2_2_PLAN.md).

- **Pane-identity transport** — option B of
  [`docs/drafts/pane-identity-transport-proposal.md`](docs/drafts/pane-identity-transport-proposal.md):
  the `spyc --mcp` proxy sends its `$SPYC_PANE_ID` in the `initialize`
  handshake and the server binds it to that connection. Closes the target
  design the F1 decisions-log entry names, and lets `get_spyc_context` answer
  for the calling pane rather than for whichever column the user is browsing.
  Attribution, not authorization — SECURITY.md says which.
- **[#40](https://github.com/Tripstack-Corp/spyc/issues/40) — one spyc per
  agent.** Abstract the hardcoded `left`/`right` column references so what a
  column holds can change. Projects prep, with standalone cleanup value.
- **[#58](https://github.com/Tripstack-Corp/spyc/issues/58) — configurable
  startup pane tabs**, per
  [`docs/drafts/PANE_STARTUP_TABS_PLAN.md`](docs/drafts/PANE_STARTUP_TABS_PLAN.md).
  A declarative tab set is the config half of a 2.3 project definition.
- **[#8](https://github.com/Tripstack-Corp/spyc/issues/8) — session forking
  (`^a f`)**, so an agent conversation can branch without losing the prior line
  of inquiry.
- **[#71](https://github.com/Tripstack-Corp/spyc/issues/71) — prompt templates
  in `.spycrc.toml`**, with picks and inventory substituted.
- **The daily-driver bug set** —
  [#326](https://github.com/Tripstack-Corp/spyc/issues/326) (the first
  keystrokes into a fresh pane are dropped),
  [#327](https://github.com/Tripstack-Corp/spyc/issues/327) (a partially-failed
  `remove_worktree` strands the worktree),
  [#9](https://github.com/Tripstack-Corp/spyc/issues/9) (`^a s` anchors paths
  on PROJECT_HOME, so an agent in a worktree can't resolve them),
  [#34](https://github.com/Tripstack-Corp/spyc/issues/34) (Claude PTY
  scrollback artifacts),
  [#22](https://github.com/Tripstack-Corp/spyc/issues/22) +
  [#11](https://github.com/Tripstack-Corp/spyc/issues/11) (the MCP takeover
  prompt, and an integration test for multi-instance coexistence).
- **`docs/drafts/PROJECTS_PLAN.md`** — authored in 2.2, design only. 2.3's
  scope depends on it being written and approved before code lands.

## The 2.3 horizon: Projects

The goal is to stop managing multiple terminal windows. A project wraps spyc
sessions: a project switcher, a `projects` status-bar segment, several agents
per project, one attention signal across all of them, and recovery that
restores every project rather than one. All in one process.

CounterTop rejected that route. `docs/archive/V1_60_PLAN.md` lists "lift App
state into `Vec<Workspace>` with an active index" as one of three candidates
and rules it out — too much state to lift, complicates persistence and the
process model — choosing siblings + mirror instead: independent peer spycs,
frame mirroring over the MCP socket, input forwarding, headless `--detached`
instances. That design was parked on 2026-07-02 for fighting spyc's
single-process sync core, and it stays parked. The rationale applied to the
mirror rather than to the goal: with one process there is nothing to mirror,
forward, or run headless.

Four things changed since. MVU is complete and guard-enforced, so lifting App
state is a bounded question about which fields move. Vsplit Stage 2 already
runs a second full `Commander` — own cwd, git, harpoon, worktree-scoped MCP —
in one process. Agent-awareness dots and desktop notifications already run
per-process, so aggregating attention across projects costs little. And the
pane-identity transport (2.2) gives every MCP connection an identity that
extends to project attribution.

Out of scope permanently: frame mirroring, input forwarding, cross-process
discovery, and any CounterTop revival. Headless needs the finer distinction.
Headless *peers* are dead with the rest of that list — a second spyc that
another spyc discovers, mirrors or forwards to. The daemonized *monolith*
returns in 3.0, and it is not a peer: one process, nothing mirrored, and the
client is a renderer rather than an instance. See "The 3.0 horizon" below.
`docs/drafts/PROJECTS_PLAN.md` — a 2.2 deliverable — is where the design gets
argued. Tracked as [#99](https://github.com/Tripstack-Corp/spyc/issues/99).

## The 3.0 horizon: Slow Cooker (durable sessions)

The goal is that spyc stops needing tmux underneath it. Detach, close the
laptop, let the agents keep working; `spyc -a` restores the client and
everything is where you left it.

The shape is the daemonized monolith. One headless spyc owns the PTYs, the
agent children and the Model; a thin client attaches over the existing unix
socket and renders. One attached client at a time. Explicitly not a
general-purpose multiplexer, not multi-client, no cross-machine protocols, and
no binary state-replication ambitions — that is Superlogical's fight and
zellij's category, and neither is ours.

The architecture already affords it. MVU's single message channel means every
inter-message tick is a quiescent, mutation-safe snapshot boundary, so state
capture falls out of the pre-2.0 migration rather than needing new machinery.
The 2.3 Projects work supplies the rest: the per-project state inventory and
the recovery manifest are the same inventory an attach snapshot needs, which is
why `PROJECTS_PLAN.md` is asked to answer for each field whether it serializes
or is rebuilt client-side.

The user-facing words are **detach** and **attach**. "Session" stays an
agent-conversation term — session forking, `/resume`, session pinning — and the
durable thing gets no noun of its own: it is just spyc, still running.

The VT-engine spike has reported (`docs/drafts/VT_ENGINE_SPIKE.md`), and its
recommendation moved forward rather than back: **the engine lands in 2.2**, so
it gets a full release of daily dogfood soak before a reattach depends on it.
That closes the engine question this horizon was waiting on — screen
reconstruction fidelity is load-bearing the moment a client reattaches, and the
incumbent reconstructs 0% of scrollback. It does **not** commit 3.0's own scope:
detach/attach, the daemon lifecycle and the client protocol are still
uncommitted, and the known-hard bug class the spike priced only in part —
reattaching into a *different* terminal emulator, where cell size, kitty/sixel
capability and colour depth all change mid-session — is where that scope has to
start. The spike settled the text half (the engine's emit is parameterized by
target capability, 13 independent toggles) and explicitly left in-pane graphics
reconstruction unpriced, flagging it for `PROJECTS_PLAN.md` to answer as a
"serializes or is rebuilt client-side" question. Tracked as a `3.0` milestone
once issues exist; none are created yet.


## Backlog & roadmap

The live, actionable work — features, fixes, tooling, and the speculative
icebox — is tracked in **[GitHub Issues](https://github.com/Tripstack-Corp/spyc/issues)**,
labelled by `area:*` / `type:*` and organized on the **[roadmap board](https://github.com/orgs/Tripstack-Corp/projects/1)**. Signposts:

- **`2.2` milestone** — the scoped 2.2 work (see "Road to 2.2" above).
- **`2.3` milestone** — Projects (see "The 2.3 horizon" above).
- **`icebox`** — speculative / nice-to-have ideas.
- **`needs-design`** — items with a design doc in `docs/drafts/` or needing a spike.
- **`needs-repro`** — reported, not yet reproducible; evidence wanted before design.
- **`good first issue`** — small, self-contained entry points.

This file is the *strategy* layer — thesis, current state, the 2.2, 2.3 and
3.0 arcs, non-goals, and the decisions log. The per-item backlog lives in Issues;
detailed designs for not-yet-started work are in `docs/drafts/*_PLAN.md`;
shipped or parked designs are archived under `docs/archive/`.

## Non-goals

These are things someone will inevitably ask for. The answer is no,
and the roadmap committing to that saves a lot of drift.

- **Native Windows support.** WSL is the supported story.
  `portable-pty` technically works on Windows but debugging the
  failure modes is a tax we're not paying. (A future crate split — the
  archived Mise en Place design — would isolate platform code so a
  volunteer *could*; that's the extent of the commitment.)
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
- **A mouse-*first* UI.** Note the narrowing: real mouse reporting
  **shipped and is on by default** (`[mouse] capture` — wheel scrolls
  whatever is under the pointer, left/middle/right buttons, drag-select
  in four surfaces, `:mouse on|off|auto`), so "mouse support" is no
  longer a non-goal. What stays out of scope is mouse-first design —
  every action keeps a keybinding, and no affordance is reachable only
  by pointer. Keys remain the API.
- **A general-purpose multiplexer.** Multi-client attach, cross-machine
  session protocols, multiplayer, production-ops surfaces — the funded
  players can have that category. The durability work in 3.0 is scoped to
  one user, one machine, one attached client, and stops there.
- **tmux command compatibility.** We have our own bindings.
- **Persistent search index** (tantivy/ctags). Ripgrep on a 100K-file
  repo is sub-second cold; the maintenance burden isn't worth it.

## Decisions log

Condensed record of the choices that shaped current behaviour — kept
so we don't re-litigate them. Full history in CHANGELOG.md.

- **Sync end-to-end, no tokio.** `std::thread` + one mpsc channel.
  Revisit never; async would be a regression for this workload.
- **MVU landed pre-2.0** (2026-05-30) so the launch ships on the
  clean foundation; strangler-fig, every phase behaviour-equivalent
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
  `codex resume <UUID>` directly; agy uses `--conversation <UUID>`;
  zot uses `--continue`.
- **OSC 72 DnD deferred** until a second terminal (beyond kitty)
  implements it.
- **Renovate auto-merges patch bumps** once public (May 2026); minors
  grouped weekly; majors labelled.
- **macOS CI deferred to post-launch**; PR template asks
  cross-platform contributors to run `make check` locally.
- **git is 100% in-process gix** in production, guard-enforced; no
  subprocess git, no gix repo open on the 1 Hz poll.
- **Crate-over-handroll**: prefer a small focused crate (features
  trimmed) over shelling out or reimplementing (libproc over
  ps/lsof). "Lightweight" means small runtime + few transitive deps,
  not "avoid crates."
- **No `unsafe` going forward** — DI / rustix / signal-hook over raw
  libc; unsafe is exceptional and isolated (a future crate split would
  give it a dedicated crate).
- **The MCP `root` override is validated, not merely documented.**
  `get_file_content`'s traversal check anchors on a caller-supplied
  root, so `root: "/"` made it decorative. Enforcement beats a
  SECURITY.md caveat because harnesses auto-approve MCP tool calls
  while gating shell execution behind per-command prompts — there,
  `search_content(root: "/")` bypasses a boundary the *user* believes
  exists. ("The agent has Bash anyway" only holds where it does.)
  Three constraints, in order of how easily they're lost:
  1. The allowed set is **cursor-independent**. Anchoring on
     `search_root`/`project_home` alone rejects the agent's own
     worktree the moment the user browses elsewhere — and a rejected
     call doesn't stop the agent, it sends it to unscoped `Bash rg`.
     Over-tight scoping produces bypass, not safety.
  2. It reuses the **trusted-root sidecar** (`write_root_marker`),
     already spyc's boundary for marker discovery. One root concept,
     not two — and `root_matches` already does the canonical compare
     that keeps symlinked worktrees from false-rejecting.
  3. Per-pane attribution — validating against the *calling* pane's
     cwd — is the target design, blocked on transport: `SPYC_PANE_ID`
     reaches the pane's env, but read-tool dispatch resolves through
     the context file, which carries no pane identity.
- **`^a s` anchors on the pane's live cwd, not on `PROJECT_HOME`**
  (2026-08-19). `PATH_HANDOFF_PLAN`'s Option A: a path under the target
  pane's live cwd goes out relative, everything else absolute, and the
  absolute tier is never `~`-collapsed (claude's `Read` won't reliably
  expand it). The old anchor hands an agent working in a worktree a path
  that resolves against the wrong directory. An unknown `live_cwd` falls
  through to absolute, so the failure mode is verbose rather than wrong.
  Prompt templates ([#71](https://github.com/Tripstack-Corp/spyc/issues/71))
  use the same anchor. The general handoff problem — terse tokens,
  submit hooks, consumer-aware `^a s` — stays exploration under
  [#59](https://github.com/Tripstack-Corp/spyc/issues/59).
- **CounterTop stays parked as an *architecture*; the multi-project
  goal is reopened for 2.3 on the monolith route** (2026-08-19). What
  fought the single-process core was siblings + mirror — peer discovery,
  frame mirroring, input forwarding, headless `--detached` spycs — and
  that stays archived. With one process there is nothing to mirror, so
  2.3 takes the route V1_60 rejected: one process, many projects. Four
  things changed since the 2026-07-02 parking. MVU is complete and
  guard-enforced, so lifting App state is a bounded question. Vsplit
  Stage 2 already runs a second full `Commander` in-process.
  Agent-status dots and notifications are already per-process. And #40
  exists as the prep refactor. Depends on the pane-identity transport
  (2.2) for project attribution; `docs/drafts/PROJECTS_PLAN.md`, a 2.2
  deliverable, is where it gets argued.
- **The 3.0 horizon opens on durable sessions, via the daemonized
  monolith** (2026-09-04). A headless spyc owns the PTYs, agent children and
  Model; a thin client attaches over the existing unix socket and renders.
  One user, one machine, one attached client — not a multiplexer. What
  changed is the premise, not the appetite: the `^Z` entry above reasons
  about children tied to a dying spyc PID under a dying TTY, and a daemon's
  children hang off a process that doesn't die. That entry stays as written;
  the log records what was decided when, and doesn't rewrite it. This is the
  CounterTop pattern a second time — the parking rationale named mechanisms
  (mirroring, forwarding, discovery), not the goal, and those mechanisms stay
  archived, while a monolith needs none of them. Positioning never says "tmux
  replacement": the claim is only that our user's stack stops containing
  tmux, and the tmux-command-compatibility non-goal is unchanged. Vocabulary
  is fixed to detach/attach, with "session" reserved for agent
  conversations. Scope does not commit until the VT-engine spike reports —
  reattaching into a different terminal emulator is the bug class to price
  first.
- **The VT engine becomes libghostty-vt, and it lands in 2.2 — not 3.0**
  (2026-09-04). A three-way differential spike
  (`docs/drafts/VT_ENGINE_SPIKE.md`, harness `spikes/vt-engine/`) measured
  vt100 0.16.2, wezterm-term and libghostty-vt over a 26-case corpus, 50,000
  fuzz iterations and a rehydration round trip. **wezterm-term is excluded
  structurally:** unpublished on crates.io, so reachable only by a git
  dependency, and `cargo package` refuses one. **vt100 is excluded on
  rehydration:** it reconstructs the visible screen perfectly and **0%** of
  scrollback, and `all_contents_formatted` has been an open upstream PR since
  2021-01-30 on a project with bus factor 1, 14 months silent and 15 open PRs
  (three of them panic fixes). **libghostty-vt is adopted**: 99.5% scrollback
  reconstruction, capability-parameterized emission (13 independent toggles —
  the thing that prices reattaching into a different emulator), 2.0× the
  incumbent's throughput at the shipped build (see the gate entry below —
  the 3.7× first recorded was a `ReleaseFast` archive that does not fit
  crates.io's size cap), zero panics in 50k adversarial iterations against
  vt100's 1,437 (2.87%), +1.0 MiB binary and +5 crates. Distribution is
  **vendored prebuilt static archives at a spyc-owned pinned ghostty commit**,
  measured at 0.59 MiB gzipped per target — ~2.4 MiB for the four release
  targets against crates.io's 10 MiB cap, so `cargo install spyc` keeps working
  with no Zig on the user's machine. FFI is isolated in a new `spyc-vt-sys`
  crate, per this log's existing scope for unsafe. **Every figure in this entry
  was measured at a ghostty commit that cannot ship** — see the next entry.
  MSRV is *not* expected to move: the spike's "1.88 → 1.90" was the published
  `libghostty-vt` crates' own `rust-version`, and spyc does not inherit it
  because it writes its own bindings (the published ones are ABI-incompatible
  with the shipping constructor, which is the reason for writing them).
  `spyc-vt-sys` declares spyc's 1.88 and the CI MSRV job proves it; if the FFI
  turns out to need more, that is a decision recorded here, not a silent bump.
  **Why 2.2 and not 3.0, which is what the spike recommended:** #34's engine
  half is already 2.2 scope, the incumbent cannot be fixed on its own
  timeline, and landing the engine now buys a full release of daily dogfood
  soak *before* 3.0 makes screen reconstruction load-bearing. Adoption is
  staged across seven PRs; vt100 stays selectable for 2.2 as the fallback and
  its removal is filed for 2.3 triage.
- **The engine adoption is contingent on a measurement gate, because the
  measured figures are not from the shipping configuration** (2026-09-04).
  Every ghostty figure in the spike was taken at ghostty `f4c68d65`, chosen
  because it is ABI-compatible with the published bindings — and at that
  commit `max_scrollback` is **inert**, so the retained history saturated at
  ~840 rows regardless of the budget. That commit cannot ship. The shipping
  pin postdates the scrollback-limits refactor (`03d5fa26` moved limits to
  `terminal_set`; `main` splits them into `SCROLLBACK_MAX_BYTES` /
  `SCROLLBACK_MAX_LINES`), and at that pin the spike's numbers are unverified.
  The gate: the harness is re-run at the shipping pin and must show a
  functional scrollback budget, zero panics over ≥50k `fuzz_diff` iterations,
  re-graded rehydration, the two known ghostty emit bugs re-checked, and
  throughput and memory re-measured — appended to the spike report as a dated
  addendum, never a rewrite. **If the re-run materially degrades rehydration
  fidelity or the panic count, adoption does not proceed** and the series stops
  after the profile-comment correction. Feasibility was probed before the
  series opened and holds: at `main` the two-scalar constructor round-trips
  geometry, and `OPT_SCROLLBACK_MAX_LINES` retains 9,883 of a configured
  10,000 rows (98.8%) — **but only with the byte limit removed**, since a
  default byte cap otherwise binds first and truncates history to ~840 rows
  irrespective of the line limit. Which fixes the budget mapping as a decision
  rather than a conversion: **spyc budgets in rows, so it sets the line limit
  to its row budget and removes the byte limit.** Page-granularity pruning
  makes the limit an estimate, so the contract is "approximately respected",
  never equality.
- **The engine gate passed at the shipping pin, with two figures corrected and
  one improved** (2026-09-04). The harness was re-run at ghostty
  `1f5bb5769fbb5e717546073d33d3985604a315b2` through `spyc-vt-sys` — the same
  bindings and vendored archive production links — and the results are appended
  to `docs/drafts/VT_ENGINE_SPIKE.md` as a dated addendum. Zero panics over
  50,000 fuzz iterations (vt100: 1,437). Fidelity unchanged at 20/26 exact
  parity. The scrollback budget works at the shipped configuration: realistic
  content retains 9,658 of 10,000 rows against a floor of budget-minus-one-page,
  and a pathological stream demonstrably binds the byte valve.
  **Improved:** the pin carries a snapshot API that did not exist at the
  measured commit, and it beats the VT formatter on every fidelity axis —
  100% rows, cells, attributes, cursor, alt-screen and **100% of scrollback**,
  against the formatter's 98%-at-one-row-shift; the continuation round trip
  survives a snapshot cut mid-escape-sequence. So 3.0 attach consumes the
  snapshot API, and the formatter's remaining one-row viewport offset is off the
  critical path. Snapshots are **transport-only — same binary, same pin**:
  version 1 carries no binary-compatibility guarantee, though the versioned
  envelope and per-record CRC32C make a stale or corrupt snapshot detectable and
  discardable rather than misparsed (measured: truncated, CRC-corrupted and
  unknown-version streams all refuse to decode). `PROJECTS_PLAN.md` question 7
  inherits that sentence. **Corrected:** throughput is **2.0×** the incumbent,
  not the 3.7× first recorded — the shipped archives are `ReleaseSmall` because
  five `ReleaseFast` archives gzip to ~16.1 MiB against crates.io's 10 MiB cap,
  so ~1.5× throughput is the priced cost of `cargo install` needing no Zig.
  And memory is **not** parity, in ghostty's favour: at a full 10,000-row budget
  vt100 costs 26.62 MiB/pane against ghostty's 6.94, because vt100 allocates its
  grid eagerly at 32 B/cell regardless of content. The earlier parity reading
  compared ghostty's 9.64 MiB *ceiling* against vt100's *usage at 3,988 rows* —
  a cap against a measurement, at different row counts. The adoption case never
  rested on memory, but the figure now points the way it actually points.
- **The engine's threading resolution is deliberately still open** (2026-09-04).
  The bindings' `Terminal` is `!Send` — a conservative binding choice, not a
  C-library constraint — while spyc's `Pane` holds `Arc<Mutex<vt100::Parser>>`
  and parses on a dedicated worker thread. Two options: confine the terminal to
  its worker and pass snapshots or dirty regions out (the actor shape, which
  ghostty's `begin_update`/`end` plus `Dirty::{Clean,Partial,Full}` may make a
  restructure with a payoff, since those map onto `needs_draw`'s reason codes),
  or `unsafe impl Send` in `spyc-vt-sys` justified line-by-line against the C
  API's documented threading contract at the pin, behind a `SPYC-TRAP` anchor
  because its failure mode is silent UB. Both get priced before either is
  coded, the actor shape is preferred if costs are comparable, and the choice
  is recorded here when made rather than inferred from the diff.

- **The threading resolution: `unsafe impl Send`, with the render path moved
  onto ghostty's render state** (2026-09-05). Priced, as the entry above
  required, and the measurement collapsed the question rather than answering
  it. Reading a frame by coordinate costs 80.0 us at 24x80 (two `grid_ref`
  lookups per cell, since style and text resolve separately); reading it
  through `ghostty_render_state_*` costs 19.3 us, **4.15x cheaper**, with both
  sides computing the same fields — an earlier comparison that let the render
  state skip `wide` reported a flattering 7x and was wrong.

  What decided it is not the speed but where the lock sits. `begin_update` is
  the only call that touches the terminal, which the C API documents as its
  purpose: "callers that synchronize access to the terminal state (e.g. with a
  lock shared with an IO thread)" hold the lock for that call alone. The pane's
  lock window therefore stops being a whole frame walk and becomes one call.
  With it that small, the actor shape buys nothing measurable and costs a
  message protocol and a copy — **and it would not even remove the `unsafe`**,
  because the main thread still calls `begin_update` on the terminal, so the
  handle crosses threads either way. The actor was preferred if costs were
  comparable; they are not comparable, they are the same cost plus machinery.

  Soundness is argued against the C API's own words — it asks for
  serialization, never for thread affinity — under the `ghostty-terminal-send`
  trap anchor in ARCHITECTURE.md, which also records what would break it.

- **`[pane] engine` is not shipped with the flip** (2026-09-05). The staging
  table listed a config key selecting the engine; it is deferred to
  [#453](https://github.com/Tripstack-Corp/spyc/issues/453), which was already
  the triage on whether vt100 stays selectable. Two reasons. Runtime selection
  means an enum over both engines with delegating impls and the choice plumbed
  through `Pane::spawn_with_env`, so every future change to the seam pays for
  two engines. And the evidence for needing the hatch got weaker: the flip is
  green across the whole suite — 2,483 tests including the `insta` snapshots
  and the widget attribute tests — which is what the strangler-fig seam was
  built to make possible. The escape hatch today is a one-line revert of the
  `PaneEngine` alias, which is what the seam was for.

## Doc map

| Doc | Role |
|---|---|
| `ROADMAP.md` | This file — strategy, the 2.2, 2.3 and 3.0 arcs, non-goals, decisions log. |
| [GitHub Issues](https://github.com/Tripstack-Corp/spyc/issues) + [roadmap board](https://github.com/orgs/Tripstack-Corp/projects/1) | The live backlog — features, fixes, ideas; labeled + milestoned. |
| `docs/archive/BACKLOG_DRAFT_NOTES.md` | Archived raw intake backlog — open items migrated to Issues (2026-07); kept as history. |
| `CHANGELOG.md` | Shipped history (git-cliff, conventional commits). |
| `AGENTS.md` | The canonical agent guide: architectural contract (MVU invariants), module map, conventions. |
| `CLAUDE.md` | One-line `@AGENTS.md` import (Claude Code entrypoint). |
| `ARCHITECTURE.md` | Deep stable design decisions. |
| `DESIGN.md` | UI design language (theme, components, glyphs). |
| `FEATURES.md` | User-facing feature reference. |
| `CONFIGURATION.md` | Config reference (`.spycrc.toml`, notifications, keymap DSL, Lua). |
| `docs/RELEASE_ENGINEERING.md` | The launch operating manual — release streams, CI, signing, Homebrew, org setup. |
| `docs/BRAND.md` | Brand & identity — the name story, palette, voice. |
| `docs/AGENT_ORCHESTRATION.md` | How the agent activity-dots / notifications / session-resume / scope registry fit together (living reference). |
| `docs/drafts/V2_2_PLAN.md` | The 2.2 scope, sequencing and exit criteria — the plan behind "Road to 2.2". |
| `docs/drafts/pane-identity-transport-proposal.md` | Accepted for 2.2 — pane id in the MCP `initialize` handshake (option B). Also the attribution mechanism Projects extends. |
| `docs/drafts/PANE_STARTUP_TABS_PLAN.md` | Pending design, 2.2 scope ([#58](https://github.com/Tripstack-Corp/spyc/issues/58)). |
| `docs/drafts/AUTO_APPROVAL_PLAN.md` | Pending design, unscheduled ([#57](https://github.com/Tripstack-Corp/spyc/issues/57)). |
| `docs/drafts/PATH_HANDOFF_PLAN.md` | Split — Option A is 2.2 scope ([#9](https://github.com/Tripstack-Corp/spyc/issues/9)); the rest stays exploration ([#59](https://github.com/Tripstack-Corp/spyc/issues/59)). |
| `docs/drafts/multi-question-bug-investigation.md` | Open bug, parked without a repro — an agent pane going deaf to input. Not in the tracker; this is the record. |
| `docs/archive/LAUNCH_PLAN_2_0.md` | Archived — the 2.0 distribution/launch plan, every gate closed, plus what never shipped. |
| `docs/archive/ARCHIVE_BROWSING_PLAN.md` | Archived design — navigate into zip/tarballs with full editing; shipped v2.1.0, [#149](https://github.com/Tripstack-Corp/spyc/issues/149) closed. |
| `docs/archive/native_scroll_plan.md` · `docs/archive/mouse_selection_plan.md` | Archived designs — the mouse suite, shipped v2.1.0. Neither was amended as it was built; the banners name what diverged. |
| `docs/archive/pasted-image-preview-plan.md` | Archived design — paste capture, image preview, the `^a g` gallery; shipped v2.1.0. |
| `docs/archive/2.1-release-notes.md` | Archived — the human-facing 2.1 notes. |
| `docs/archive/TESTING_STRATEGY.md` | Testing strategy & guidelines (coverage, anti-"test theater", proptest/cargo-fuzz, AI-testing rules). Campaign complete — 8 clusters, June 2026; kept as the how-we-test reference. |
| `docs/archive/V1_60_PLAN.md` | Archived design — CounterTop multi-instance hub. Parked as an architecture; the multi-project goal it targeted is reopened for 2.3 on the monolith route (see the decisions log). |
| `docs/archive/V1_70_PLAN.md` | Archived design — Mise en Place typed addressability + crate split. Speculative; MCP already covers the basics. |
| `docs/archive/engagements/` | Engagement briefs and deliverables (the review-remediation and cleanup rounds) — which PR closed which finding. |
| `docs/COMPETITIVE_REVIEW.md` | Consolidated competitive review + GTM: the AI coding-agent-manager category (§1–§1c: herdr, psmux, claude-code-ide.el), the TUI file-manager lane (§1d: Yazi, folded 2026-07-02; §1e: lf), and the session layer beneath both (§1f: Superlogical, zmx). Refresh on a competitor's next major. (Standalone Yazi original archived at `docs/archive/YAZI_COMPETITIVE_REVIEW.md`.) |
| `docs/archive/` | Shipped plans, kept as historical record. |

> **Note on pending plans:** `AUTO_APPROVAL_PLAN`, `PANE_STARTUP_TABS_PLAN`
> and `PATH_HANDOFF_PLAN` predate the MVU decomposition — their designs hold,
> but `src/app/mod.rs:NNNN`-style file pointers resolve nowhere; re-resolve
> against the current module layout when picking one up, and each carries that
> warning in its own header. `V2_2_PLAN` and
> `pane-identity-transport-proposal` are written against the current layout.
