# spyc roadmap

The strategy layer — thesis, current state, the 2.2 and 2.3 arcs, non-goals, and
the decisions log. The per-item **backlog lives in [GitHub Issues](https://github.com/Tripstack-Corp/spyc/issues)**
(organized on the [roadmap board](https://github.com/orgs/Tripstack-Corp/projects/1));
`CHANGELOG.md` is the shipped history. Detailed designs for not-yet-started work
live in `docs/drafts/*_PLAN.md`; shipped or parked plans are archived in
`docs/archive/`.

## Thesis

spyc is a vi-keyboard-driven file commander that exposes itself to an AI coding
agent as a queryable context source. The target user is a developer who already
thinks in vi motions and wants Claude Code living in the same workspace -- not
one window over, not in a browser tab, in the same session, sharing context.

The MCP server shifted the tool's nature: spyc isn't just "a file manager with
Claude in a pane." It's a file manager that Claude can query -- current
directory, cursor, picks, inventory, filter, git branch -- via a standard
protocol. That bidirectional awareness is the positioning that differentiates
spyc from `tmux` + `claude`.

Every other feature -- picks, inventory, pager, status bar, sessions -- is
supporting infrastructure that makes the split-pane workflow fast and
comfortable. The roadmap is organized accordingly: the pane-and-agent
integration is the defining work track, not the trailing milestone.

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
`src/app/mod.rs` alone. Files over the line exist; re-derive the list from the
tree before acting on it rather than from a number written down here.

The thesis work shipped over 1.x and 2.0 — agent-awareness dots and
notifications, the worktree MCP suite, the merge/scope registry, the in-process
review loop, the vertical split, Lua scripting, and the chord/leader overhaul.
Those are the differentiators the competitive review
([`docs/COMPETITIVE_REVIEW.md`](docs/COMPETITIVE_REVIEW.md)) named as spyc's
wedge, and they are all real; the mechanics live in
[`docs/AGENT_ORCHESTRATION.md`](docs/AGENT_ORCHESTRATION.md), AGENTS.md and
ARCHITECTURE.md.

2.0 was the distribution pass — public repo, signed binaries, brew / apt /
crates.io, Show HN. Its plan is archived at
[`docs/archive/LAUNCH_PLAN_2_0.md`](docs/archive/LAUNCH_PLAN_2_0.md), with the
done-criteria confirmed and the parts that never shipped named. **2.1 was the
release where the tool got used**, and its arc is what daily-driving turned up:

- **An installable agent skill.** `spyc --install-skill` writes an embedded
  usage guide into Claude Code's, codex's and agy's personal-skills dirs, with
  a startup `[Y/n]` offer when spyc's copy moves ahead and local edits never
  clobbered unprompted (#187). It is the depth beneath the MCP `initialize`
  handshake, which has to stay short.
- **agy became first-class and gemini went away** (#194), with a scrape
  fallback for the `Blocked` state its hooks can't report, and session pinning
  from its own hook payload rather than spawn proximity (#202, #285, #287).
- **The mouse works, and it ships on by default** (#212–#234, plus twenty
  follow-ups through #387). The wheel scrolls whatever is under the pointer,
  three button gestures, drag-select in four surfaces. Non-goals narrowed to
  match: mouse *support* is no longer a non-goal, mouse-*first* design still is.
- **Archive browsing, end to end** (#301–#334, closing
  [#149](https://github.com/Tripstack-Corp/spyc/issues/149)). Walk into a zip
  or a tarball, change what's inside, `:archive write` repacks and verifies. A
  mount is an index, not a directory, so entering a huge zip extracts nothing.
- **Images stopped being opaque tokens** — capture at paste time, full-screen
  preview, and the `^a g` gallery of what the agent actually received (#300,
  #302, #304).
- **Per-character intraline diff highlight**, on by default (#351, #364).
- **A security and correctness pass**, then a review of it. The MCP `root`
  override is validated rather than merely documented (F1, #237), every
  persistent write goes through `write_atomic` (F3, #238), there is one
  state-root resolver (F6, #239), and the fuzz targets build for the target
  that can run them and go weekly in CI (F5, #242). The pre-2.1 review that
  followed put six reviewers plus a referee over `v2.0.0..HEAD` and closed 58
  findings across eight passes; the reports are archived under
  [`docs/archive/review-2.1/`](docs/archive/review-2.1/).

v2.1.1 is one packaging fix on top: `docs/ABOUT.md` is compiled into the binary
via `include_str!` and was missing from the published crate (#426).

Next is **2.2** — projects-prep plus daily-driver credibility — and then the
2.3 headline, Projects. Both below.

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

2.2 has two jobs and no third. It lands the prerequisites Projects needs, so
2.3 starts on an approved design instead of a refactor swamp; and it closes the
bugs a daily driver hits weekly, because the release that made the tool
credible is the one that has to keep it that way. Every item below is one or
the other. Detailed scope and sequencing:
[`docs/drafts/V2_2_PLAN.md`](docs/drafts/V2_2_PLAN.md).

- **Pane-identity transport** — option B of
  [`docs/drafts/pane-identity-transport-proposal.md`](docs/drafts/pane-identity-transport-proposal.md):
  the `spyc --mcp` proxy sends its `$SPYC_PANE_ID` in the `initialize`
  handshake and the server binds it to that connection. Closes the target
  design the F1 decisions-log entry names, and makes `get_spyc_context` answer
  for the caller instead of for whichever column the user is browsing.
  Attribution, not authorization — SECURITY.md says which.
- **[#40](https://github.com/Tripstack-Corp/spyc/issues/40) — one spyc per
  agent.** Abstract away the hardcoded `left`/`right` column references so what
  a column holds can change. Projects prep, and standalone cleanup value.
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
  [#326](https://github.com/Tripstack-Corp/spyc/issues/326) (a fresh pane eats
  the head of your first message),
  [#327](https://github.com/Tripstack-Corp/spyc/issues/327) (a partially-failed
  `remove_worktree` strands the worktree),
  [#34](https://github.com/Tripstack-Corp/spyc/issues/34) (Claude PTY
  scrollback artifacts), and
  [#22](https://github.com/Tripstack-Corp/spyc/issues/22) +
  [#11](https://github.com/Tripstack-Corp/spyc/issues/11) (the MCP takeover
  prompt, and an integration test that exercises multi-instance coexistence).
- **`docs/drafts/PROJECTS_PLAN.md`** — authored in 2.2, design only. It is a
  tracked deliverable, not a nice-to-have: 2.3's scope depends on it existing
  and being approved before any code lands.

## The 2.3 horizon: Projects

The goal is stated the way the user states it: **stop managing multiple
terminal windows.** A project is a wrapper around spyc sessions — a project
switcher, a `projects` segment in the status bar, several agents per project,
one attention signal that reaches you no matter which project raised it, and
recovery that restores every project rather than one. All of it **in one
process**.

That is the route CounterTop rejected. `docs/archive/V1_60_PLAN.md` lists
"lift App state into `Vec<Workspace>` with an active index" among three
candidates and rules it out — too much state to lift, complicates persistence
and the process model — choosing **siblings + mirror** instead: independent
peer spycs, frame mirroring over the MCP socket, input forwarding, headless
`--detached` instances. That design was parked on 2026-07-02 for fighting
spyc's single-process sync core, and it stays parked. The parking rationale
was about the *mirror*, though, not about the goal. Eliminating windows makes
mirroring, forwarding and headless peers moot rather than hard: there is one
process, one render state, one keyboard.

Four things are true now that were not when CounterTop was written. The MVU
migration is complete and guard-enforced, so "lift App state" is a bounded
question about which fields move into a project struct rather than an open one.
Vsplit Stage 2 already put a second full `Commander` — its own cwd, git,
harpoon, worktree-scoped MCP — in one process, which is the same lift at
smaller scale. Agent-awareness dots and desktop notifications already run
per-process, so global attention aggregation is nearly free once the panes
belong to projects. And the pane-identity transport (2.2) gives every MCP
connection an identity that extends naturally to project attribution.

Explicitly out of scope, in 2.3 and after: no frame mirroring, no input
forwarding, no headless or `--detached` peers, no cross-process discovery, no
CounterTop revival. `docs/drafts/PROJECTS_PLAN.md` — a 2.2 deliverable — is
where the design gets argued; this section only records the direction.
Tracked as [#99](https://github.com/Tripstack-Corp/spyc/issues/99).

## Backlog & roadmap

The live, actionable work — features, fixes, tooling, and the speculative
icebox — is tracked in **[GitHub Issues](https://github.com/Tripstack-Corp/spyc/issues)**,
labeled by `area:*` / `type:*` and organized on the **[roadmap board](https://github.com/orgs/Tripstack-Corp/projects/1)**. Signposts:

- **`2.2` milestone** — the scoped 2.2 work (see "Road to 2.2" above).
- **`2.3` milestone** — Projects (see "The 2.3 horizon" above).
- **`icebox`** — speculative / nice-to-have ideas.
- **`needs-design`** — items with a design doc in `docs/drafts/` or needing a spike.
- **`needs-repro`** — reported, not yet reproducible; evidence wanted before design.
- **`good first issue`** — small, self-contained entry points.

This file is the *strategy* layer — thesis, current state, the 2.2 and 2.3
arcs, non-goals, and the decisions log. The per-item backlog lives in Issues;
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
  `codex resume <UUID>` directly; agy uses `--conversation <UUID>`;
  zot uses `--continue`.
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
- **CounterTop stays parked as an *architecture*; the multi-project
  goal is reopened for 2.3 on the monolith route** (2026-08-19). What
  fights the single-process core is siblings + mirror — peer discovery,
  frame mirroring, input forwarding, headless `--detached` spycs — and
  that stays archived. Eliminating windows instead of aggregating them
  makes all of it moot, so 2.3 takes the route V1_60 rejected: one
  process, many projects. Four things changed since the 2026-07-02
  parking. MVU is complete and guard-enforced, so "lift App state" is a
  bounded question. Vsplit Stage 2 already runs a second full
  `Commander` in-process. Agent-status dots and notifications are
  already per-process, so global attention costs almost nothing.
  And #40 exists as the prep refactor. It depends on the pane-identity
  transport (2.2) for project attribution. `docs/drafts/PROJECTS_PLAN.md`,
  a 2.2 deliverable, is where it gets argued.

## Doc map

| Doc | Role |
|---|---|
| `ROADMAP.md` | This file — strategy, the 2.2 and 2.3 arcs, non-goals, decisions log. |
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
| `docs/drafts/PATH_HANDOFF_PLAN.md` | Pending design, unscheduled ([#9](https://github.com/Tripstack-Corp/spyc/issues/9), [#59](https://github.com/Tripstack-Corp/spyc/issues/59)). |
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
| `docs/COMPETITIVE_REVIEW.md` | Consolidated competitive review + GTM: the AI coding-agent-manager category (§1–§1c: herdr, psmux, claude-code-ide.el) plus the TUI file-manager lane (§1d: Yazi, folded 2026-07-02). Refresh on a competitor's next major. (Standalone Yazi original archived at `docs/archive/YAZI_COMPETITIVE_REVIEW.md`.) |
| `docs/archive/` | Shipped plans, kept as historical record. |

> **Note on pending plans:** `AUTO_APPROVAL_PLAN`, `PANE_STARTUP_TABS_PLAN`
> and `PATH_HANDOFF_PLAN` predate the MVU decomposition — their designs hold,
> but `src/app/mod.rs:NNNN`-style file pointers resolve nowhere; re-resolve
> against the current module layout when picking one up, and each carries that
> warning in its own header. `V2_2_PLAN` and
> `pane-identity-transport-proposal` are written against the current layout.
