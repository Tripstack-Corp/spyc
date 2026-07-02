# Agent awareness & orchestration — implementation plan (herdr-informed)

A prioritized plan for the ideas worth picking up from the June-2026 competitive
deep dive on **herdr** (see `docs/COMPETITIVE_REVIEW_2026-06.md` §1a). herdr is
spyc's Rust-TUI twin; this plan steals the *good ideas* while turning herdr's
biggest *weakness* — fragile, screen-scraped agent status — into a spyc strength.

> **AGPL caveat.** herdr is AGPL-3.0-or-later. Everything below is an
> independently-reimplementable **approach**, derived from reading herdr's public
> source for understanding. **No herdr code may be copied or adapted into spyc.**
> Where a herdr file/mechanism is named, it is a factual reference for *what to
> build cleanly from scratch*, not a thing to lift.

---

## Status at a glance

**Shipped** — folded into the codebase and documented in full in AGENTS.md
("Agent-activity dots"); kept here only as one-line pointers so this charter
stays focused on what's left.

- **P0 — status vocabulary + per-tab dot + `:why-status`.** `AgentActivity`
  vocabulary + a per-agent-tab activity dot in the divider (output-timing
  heat-pulse `●` / quiet `·`) + the `:why-status` / `:activity dump` debug
  surface. *Code:* `src/app/agent_status.rs`, `src/app/render/chrome.rs`,
  `src/app/commands.rs`.
- **P1-1 — `report_status` MCP tool + auto-hooks (claude / codex / agy).**
  Semantic self-report over the PID-scoped socket with a report-beats-timing
  authority model (`ttl_ms` backstop, fresh output supersedes a stale report),
  plus consent-gated per-repo lifecycle-hook install driven by
  `AgentProfile::status_hooks()` (`:hooks on|on!|off`). *Code:* `src/mcp/hooks.rs`,
  `src/app/mcp.rs`, `src/agent/`.
- **P3-1 — notification / bell / visual bell on Blocked / Done** (#638, v1.90.0).
  `Effect::Notify` fires on the real state transition (pure decision
  `notification_for_transition`); `[notify]` config, desktop ping on by default.
  *Code:* `src/notifications.rs`, `src/app/effect.rs`, `src/config/`.
- **P3-2 — crash-sufficient autosave** (v1.92.0). Debounced periodic session
  save (`Deadline::Autosave` + `settle_autosave` over the pure `autosave_action`)
  on a stable per-process session id, written atomically (`fs::write_atomic`), so
  a `SIGKILL` loses at most the ~2s debounce window instead of everything since
  launch. 0-dps-at-idle preserved (armed only while dirty; skips empty sessions).
  *Code:* `src/app/session.rs`, `src/app/scheduler.rs`, `src/state/sessions/`.
- **P1-3 — live session-identity capture** (v1.93.0). The status-hook reporter
  piggybacks the agent's live `session_id` (from Claude's hook stdin) onto its
  `report_status` call; spyc pins it to the pane's `claude_session_id`, so
  `save_session` / `-r` resume the **exact** conversation instead of guessing by
  spawn-proximity (fixes the #607 crossed-conversations class). Claude-only today
  (only its hooks carry the id in stdin). *Code:* `src/mcp/mod.rs`,
  `src/mcp/protocol.rs`, `src/app/mcp.rs`, `src/mcp_cmd.rs`.
- **P1-2 — data-driven detection manifest (the scrape fallback)** (v1.94.0). A
  declarative per-`AgentProfile` ruleset (`agent::detect_rules`: `Region` +
  `Matcher` + priority-ordered `DetectionRule`) scanned on each output event for
  an agent with rules **and no live report** — the third detection tier, strictly
  below self-report and above output timing (`effective_activity`). Event-driven
  hysteresis (`scrape_candidate_after`, `SCRAPE_CONFIRM_COUNT` consecutive
  agreeing scans) kills flicker; scans the live visible screen only (no stale
  scrollback); `:why-status` / `:activity dump` label the `scrape-fallback`
  source. Ships one verified rule — gemini's `Allow execution of:` approval
  prompt → `Blocked` (gemini has no `status_hooks`). *Code:*
  `src/agent/detect_rules.rs`, `src/agent/mod.rs`, `src/app/agent_status.rs`,
  `src/app/streaming.rs`, `src/pane/tabs.rs`.

**Remaining** — the live scope this charter still tracks:

- **P2** — merge/scope coordination. **Registry + verbs shipped** (v1.95.0):
  `register_scope` / `list_scopes` / `release_scope`, in-memory + session-
  persisted + auto-released on tab close. **Remaining:** `wait_for_scope_clear`
  (the blocking verb) + the orchestration screen. Full design in Phase 2 below.

---

## The thesis

The category's founding pain is **"which agent needs me?"** herdr proved the
demand (its agent-status sidebar is its #1 loved feature) — and proved the trap:
herdr detects status by **screen-scraping** the agent's terminal UI (OSC titles +
Braille spinner glyphs + body text), which is its #1 bug source (46+ detection
issues; broke when Claude moved its spinner; invisible to nix/Happy wrappers; a
no-op on Linux until v0.7.0).

**spyc inverts the model.** spyc already runs an **MCP server** and already
**lazily writes per-agent config** into each project (`ensure_agent_mcp_config` →
`.mcp.json` / `.codex/config.toml`). That same machinery is the perfect vehicle
to drop a tiny **status hook** so a cooperative agent *self-reports* its state
over a semantic channel — with a tunable **scrape fallback** for agents that
won't. Scrape-as-fallback, not scrape-as-primary. That dodges herdr's entire
fragility class and is how spyc wins a feature herdr struggles to keep working.
*(The self-report path is shipped — P1-1 above; the scrape fallback is P1-2,
still to build.)*

Three more ideas ride alongside: an **orchestration capability** (let agents
*coordinate merges* — claim a file scope, wait on a conflicting one — the genuine
gap vs MCP today; P2), live **session-identity capture** (more reliable than
spyc's exit-banner sniffing; shipped — P1-3), and a **notification/bell** when a
backgrounded agent needs you *(shipped — P3-1)*.

> **Field-validated (2026-07).** Each remaining (un-shipped) item below carries
> a **Use cases** line — the concrete workflow it unlocks, with a real-world source.
> These are grounded in a web scan of how herdr, cmux, Claude Squad, and Claude
> Code's own *agent-teams* are actually used (herdr deep dive:
> `docs/COMPETITIVE_REVIEW_2026-06.md` §1a). Recurring theme: the field keeps
> arriving at the same design spyc already chose — self-report over scrape, state-
> driven over timers, native-resume over tab-only restore — and keeps hitting the
> same Claude-hook gaps spyc hit. Where a feature carries real risk (P2), the
> skeptic's counterpoints are folded in as guardrails, not hand-waved.

### Invariants this plan must preserve (spyc MVU)

- **Single process. No daemon.** Detection stays off-thread
  (`src/app/agent_status.rs`), waking the loop with a `Message`. No `tokio`.
- **Effects are data.** Notifications, hook writes, etc. go through `Effect` +
  `run_effects` — never inline OS in a handler or the draw pass.
- **One message channel.** The event hub for orchestration fans out *within* the
  existing `mpsc` loop; no second runtime.
- **Render is pure.** Status dots/rollup read settled `ViewState`; any settling
  happens in the `prepare_*` steps.

---

## Explicitly OUT of scope (and why)

- **A detach/reattach daemon + live binary handoff (SCM_RIGHTS FD-passing).**
  herdr's persistence advantage is real, but it's a *re-architecture* (a 7.8k-LoC
  headless server, dual sockets, per-client render diffing) that fights spyc's
  single-process MVU/sync core — and herdr pays for it (macOS responsible-process
  / TCC loss on handoff, their #808). spyc's answer is **resilient autosave**
  (recovery-sufficient against `SIGKILL`) + richer `-r` content — the 80% win
  without the daemon. (Tracked as item P3-2 below.)
- **A many-agent grid multiplexer.** herdr *is* a multiplexer; spyc is a file
  manager with pane tabs + a file-commander vsplit. We nail the **2–4 agent**
  case, not a 50-pane wall (see review §4 objections).
- **Scrape-only detection.** We keep a scrape *fallback*, but never make it the
  primary path — that's the herdr trap we're avoiding.

---

## Phase 1 — Reliable detection *(shipped)*

All three detection tiers shipped: P1-1 self-report (the primary), P1-3 live
session-id, and P1-2 the scrape fallback below.

**P1-2 · Data-driven detection manifest (the fallback). — SHIPPED (v1.94.0).**
A declarative per-`AgentProfile` ruleset (`agent::detect_rules`: a `Region`, a
`Matcher`, and priority-ordered `DetectionRule`s each with a `visible_blocker`
hint), scanned on each output event for an agent that has rules **and** no live
report — the third tier of `effective_activity`, strictly below self-report and
above output timing. Event-driven hysteresis (`scrape_candidate_after`:
`SCRAPE_CONFIRM_COUNT` consecutive agreeing scans — herdr's timer debounce
adapted to spyc's output-event drain) kills flicker; it scans the **live visible
screen only** (never scrollback, so an answered prompt that scrolled off can't
linger as a false `Blocked`), and `:why-status` / `:activity dump` label the
`scrape-fallback` source. Ships one **verified** rule: gemini's `Allow execution
of:` approval prompt → `Blocked` (gemini has no `status_hooks`); every other
agent defaults to an empty ruleset — no guessing at UI text spyc hasn't observed
(worse than no fallback). Deliberately **second-class**: a live report always
wins (`settle_agent_activity` clears any scrape guess the instant one exists), so
spyc never re-enters herdr's fragility class as the *primary* path.
*Use cases:* an agent spyc **can't** auto-hook still gets a live dot instead of
going dark — a `gemini` pane (no `status_hooks()`), or a **wrapper-hidden**
Claude/codex (`nix`, Bubblewrap, Happy) whose hook env didn't propagate — exactly
the gap cmux shipped blind into (*"sidebar agent status is hook-only with no
fallback … codex agents go silent when hooks miss"*,
[cmux #3749](https://github.com/manaflow-ai/cmux/issues/3749)).

**P1-3 · Live session-identity capture. — SHIPPED (v1.93.0).**
The status-hook reporter (`spyc --report-status`) already reads Claude's hook
stdin; it now also lifts the `session_id` Claude includes there and piggybacks it
onto the same `report_status` socket call (no new tool — `session_id` rides the
existing args; pure `session_id_from_hook_payload`). spyc routes it to the pane's
`claude_session_id` (in the `McpCommand::ReportStatus` apply), and `save_session`
already prefers that id when its JSONL exists — so `-r` replays the **right**
native `--resume <id>` live, instead of guessing by spawn-proximity (fixes the
#607 crossed-conversations class; the failure Zed #58001 / CC #18311 also hit).
**Claude-only** in practice: codex keeps `app::codex_pin` (its id lives in the
rollout file, not hook stdin) and agy has no id channel — so this generalizes
self-report where the agent *can*, not literally all three. transcript_path is
unneeded (derived from cwd + id via `claude_jsonl_path`). *Code:* `src/mcp/mod.rs`,
`src/mcp/protocol.rs`, `src/app/mcp.rs`, `src/mcp_cmd.rs`.

---

## Phase 2 — Merge / scope coordination *(the capability gap, reframed)*

The last unshipped item — reframed from generic herdr-style "read/send/wait
primitives" to the coordination layer that solves spyc's **real** pain: the
**merge train**. Multiple agents work concurrently and collide on merges —
overlapping files, the `Cargo.toml` version line, racing PRs (this charter's own
build hit it repeatedly). The `spyc-semver` merge driver already auto-resolves the
*mechanical* version-line clash; what's missing is coordination for the *semantic*
overlap. So P2 is: each agent **registers the scope it's touching + its intent**
(editing vs merging), any agent can read the registry and **wait** on a
conflicting claim, and a **TUI orchestration screen** shows the whole picture and
lets you intervene. **Advisory, not enforced** — spyc informs and offers a wait
primitive; it never blocks a merge and **never auto-spawns**. Realizes the
merge-autopilot campaign's parked "scope-conductor" (ROADMAP → Tooling) with a
concrete driver.

> **Field-validated (the coordination angle).** The multi-agent field keeps
> hitting the wall spyc hit: concurrent agents need to *coordinate*, not just run.
> Claude's own agent-teams steer the lead to *"wait for your teammates to complete
> their tasks before proceeding"*
> ([agent-teams docs](https://code.claude.com/docs/en/agent-teams)); herdr ships a
> socket with `wait agent-status` + `pane.read`/`send_keys` and AWS Labs'
> [`cli-agent-orchestrator`](https://github.com/awslabs/cli-agent-orchestrator/blob/main/docs/herdr.md)
> scripts against it, so the coordination *shape* is proven. spyc's twist: make the
> unit of coordination the **scope claim** (what files, what intent), not a raw
> pane run-state — a far better fit for the merge-train problem than a status wait,
> and it composes with spyc's worktree MCP tools (claim per `create_worktree`).
>
> **Guardrails (the skeptic's counterpoints are real — bake them in).** Auto-
> coordination can collapse into agents *"generating huge status reports for each
> other"* instead of doing work ([HN 47245373](https://news.ycombinator.com/item?id=47245373));
> runaway loops burn tokens with no circuit-breaker; and Anthropic itself notes that
> for sequential / same-file / heavy-dependency work a single session beats a team.
> So spyc ships the **primitives** (register / list / wait + the screen) and stays
> a *primitive* — the human or a deliberately-authored orchestrator composes them;
> **spyc never auto-spawns** and never blocks a merge. `timeout` is mandatory on
> every wait (no unbounded blocks), and the PID-scoped socket + takeover prompt
> keep a stale waiter from wedging the loop.

### How it maps onto spyc's architecture (no daemon, no second runtime)

Every MCP tool already runs on a **per-connection socket thread**
(`handle_socket_connection`) that forwards an `McpCommand` to the main loop over
`mpsc` and **blocks on a one-shot reply channel** (`McpRequest { command, reply }`)
until the loop answers. Two consequences shape the whole P2 design — and let it
land without a daemon, a second runtime, or `tokio`:

- **Registry reads + mutations fit the one-shot model unchanged.**
  `register_scope` / `list_scopes` / `release_scope` are new `McpCommand` variants
  the loop answers *immediately* — mutate or read the in-memory registry — exactly
  like `report_status` / `navigate_to` do today.
- **`wait_for_scope_clear` just answers *later*.** Instead of replying at once,
  the loop **parks the reply sender** and fires it when the blocking claim
  releases or a deadline passes. The socket thread stays blocked on its `recv()`
  the whole time — no polling, no extra thread: a `Runtime`-owned
  `Vec<PendingWait>`, resolved in a `settle_scope_waiters` step alongside the
  `settle_*` chain that already runs each pre-recv tick.

So every invariant this plan opened with holds: single process, one message
channel, effects-as-data, render stays pure, 0 dps at idle.

### P2-1 · The scope registry + its MCP verbs — SHIPPED (v1.95.0)

An **in-memory, Model-owned** registry: a `Vec<ScopeClaim>` on `AppState`, each
`ScopeClaim { owner, paths, intent, pr, note, claimed_at }` — `intent` is
`Editing` | `Merging`, `paths` a set of path globs. `owner` is keyed by the tab's
**stable** id (its `claude_session_id` / a per-tab key that survives `-r`), not
the ephemeral pane uuid, so a claim re-binds to the right agent after a restore.
Every agent pane in one spyc shares that spyc's socket, so one registry
coordinates them all — no files, no locking, no staleness reaping (textbook MVU).

The verbs (one-shot MCP tools; generalize the existing `claim_worktree` lease
from a whole worktree to a file set + intent):
- **`register_scope(paths, intent, pr?, note?)`** → a claim id. *"I'm touching
  these files; I'm about to merge PR #N."*
- **`list_scopes()`** → the whole registry (owners, paths, intents, PRs, plus the
  wait graph) — what an agent checks *before* it merges, and what the screen
  renders.
- **`release_scope(id)`** — plus an automatic release when a tab closes / its
  agent exits (a dead agent never holds a stale claim).

**Persistence — the registry is session state, not ephemeral (rides P3-2).** It
serializes into the session snapshot next to tabs + vsplit, so:
- P3-2's debounced autosave persists it — `session_fingerprint` gains the
  registry, so registering/releasing a claim arms a save; a `SIGKILL` mid-train
  loses at most the ~2s window.
- **`spyc -r` restores it**, re-binding each claim to its restored tab by the
  stable `owner` key — coordination survives a crash/restart *exactly* when you'd
  most want it (mid-train). This is why the registry is **Model** state, not a
  Runtime scratch field.

*Touches:* `src/app/state/` (registry + `session_fingerprint`),
`src/state/sessions/` (serialize + restore), `src/mcp/{protocol,readers}.rs` +
`src/mcp_cmd.rs` + `src/app/mcp.rs` (the verbs). *Effort: med.*

### P2-2 · `wait_for_scope_clear` — the coordination verb

**`wait_for_scope_clear(paths, timeout_ms)`** blocks the caller until no
`Merging` claim overlaps `paths` (or the timeout fires), returning which happened.
"Another agent chooses to wait" becomes one call: before merging, an agent waits
until whoever's mid-merge on the overlapping files releases.

Mechanics (the loop-held-waiter shape above):
- The socket thread sends `McpCommand::WaitForScopeClear { paths, deadline,
  reply }` and blocks on `reply.recv()`.
- The loop parks a `PendingWait { paths, deadline, reply }`; if nothing conflicts
  *now*, it answers immediately (fast path).
- `settle_scope_waiters` (runs whenever the registry can change — any
  `release_scope` / tab close / claim edit) walks the parked waiters: none
  overlapping a `Merging` claim → send `cleared` + drop; past `deadline` → send
  `timed_out` + drop. A `Deadline::ScopeWait` at the earliest pending deadline
  honors timeouts with no other activity — armed only while a waiter is parked, so
  **0-dps-idle holds**.
- **Mandatory** `timeout_ms` (no unbounded blocks — a hard guardrail); the
  overlap check is pure + unit-tested (given claims + query paths → conflict?),
  per the `route.rs` / `focus.rs` template.

*Use cases (the merge train, solved):*
- **Serialize the merge train** — before merging, `register_scope(intent:
  Merging)` your PR's files, `wait_for_scope_clear` on them, merge, `release_scope`.
  Concurrent agents queue on overlap instead of colliding + rebasing (the exact
  grind this charter's build kept hitting).
- **Fan-out → gather** — N agents in N worktrees each claim `Editing` scope; the
  lead `list_scopes()` to see who overlaps and waits out the risky pairs before
  landing them.

### P2-3 · The orchestration screen

A TUI panel (`:orchestrate`, or a chord) rendering the live registry: per agent
tab — its claimed paths, `intent`, PR, and the **wait graph** (who's blocked on
whom). The "open a screen and see/poke the coordination" you want. Built in two
steps: **observability first** (a pure `&self` render over the Model registry —
the `ui/` renderer pattern), then **interactive** (release a stuck claim, wave a
merge through) via the same registry ops the MCP verbs use, routed through `apply`
like every other action. New render surface + a light modal; reads the Model,
mutates through the existing action path.

*Touches:* `src/ui/` (renderer), `src/app/render/` + a `Modal` arm, `src/keymap/`
(a binding / `:orchestrate` command). *Effort: med.*

### Deliberately not building (considered, set aside)

- **`read_pane` / `send_pane_keys` (one agent driving another's terminal).** The
  coordination use-case doesn't need it — agents coordinate through the *registry*,
  not by reading/typing into each other's panes — and it's the sharpest safety
  edge (agent-drives-agent). Left out unless a real handoff need appears; the
  pane-read/​send plumbing already exists (`gf` / `SendToPane`) if so.
- **Streaming `subscribe_events`.** `wait_for_scope_clear` + the screen cover the
  need; a continuous agent-consumed event feed is the speculative surface the
  guardrails warn about (agents narrating at each other). P3-1 already gives the
  *human* the live signal.

### Sequencing

Three PRs, smallest blast radius first (each its own worktree + version bump +
owner test):

1. **Registry + verbs + persistence** — ✅ **SHIPPED (v1.95.0).**
   `register_scope` / `list_scopes` / `release_scope`, the Model-owned
   `Vec<ScopeClaim>`, serialize/restore through the session snapshot +
   `session_fingerprint` (rides P3-2), auto-release on tab close. Useful on its
   own: `list_scopes` already lets agents see overlaps. *Code:*
   `src/state/scope_registry.rs`, `src/app/state/`, `src/state/sessions/`,
   `src/mcp/protocol.rs`, `src/mcp_cmd.rs`, `src/app/mcp.rs`, `src/app/pane_tabs.rs`.
2. **`wait_for_scope_clear`** — the loop-held `PendingWait` + `Deadline::ScopeWait`
   + `settle_scope_waiters` + the pure conflict check. *(next)*
3. **The orchestration screen** — observability render first, then the
   interactive ops.

`read_pane` / `send_pane_keys` and streaming `subscribe_events` stay **out of
scope** (documented above). So "P2 complete" = **registry + wait + screen** — the
merge-coordination layer, not a generic agent-drives-agent surface.

*Keep spyc's edge throughout:* one PID-scoped socket + the takeover prompt; the
registry is advisory (spyc never blocks a merge or auto-spawns); every wait
bounded by a mandatory timeout.

---

## Phase 3 — Polish *(shipped)*

Both polish items shipped. P3-1 = the blocked/done notification (#638, v1.90.0).

**P3-2 · Crash-sufficient autosave (the persistence answer). — SHIPPED (v1.92.0).**
`save_session` was **quit-only** (`request_quit`), so a `SIGKILL` lost everything
since launch. Now a debounced periodic autosave — `Deadline::Autosave`, armed by
`settle_autosave` over the pure `autosave_action`, on any session-relevant change
(tab / cwd / vsplit / project-home / geometry, detected via a cheap structural
fingerprint) — fires ~2s after the last change, so a hard kill loses at most that
window. Every save reuses a **stable per-process session id** (overwrite-in-place,
no file churn that would evict other projects' sessions) and goes through
`fs::write_atomic` (temp + rename, no torn file on kill). Armed only while dirty
and skips empty sessions, so 0-dps-at-idle holds. The split/pane **layout tree** +
per-pane **cwd** were already captured (`SavedVsplit` + `SavedTab.cwd`); this closes
the durability half. Restored panes come back as **fresh shells + resumed
conversations, not PID-preserved live processes** — the detach/reattach daemon
stays out of scope (above); codex #11852's "reconnect shows *working* but nothing
is running" ghost is the trap we avoid by not pretending. This is the *"Recovery
Snapshot"* Claude Code declined upstream (consolidating 100+ issues,
[CC #26729](https://github.com/anthropics/claude-code/issues/26729)) — spyc just
has it. *Code:* `src/app/session.rs`, `src/app/scheduler.rs`, `src/state/sessions/`.

---

## Sequencing & rationale

```
P0 ✅  ──►  P1-1 ✅  P1-3 ✅  P1-2 ✅  ──►  P2 (scope registry + wait_for_scope_clear + screen)
                              │
                              ▼
                         P3-1 ✅        P3-2 ✅ crash-sufficient autosave
```

- **P0 (shipped)** — you can't ship or debug status without a vocabulary, a
  render surface, and `:why-status`.
- **P1 is the moat (shipped)** — semantic-first detection (P1-1) + live session
  id (P1-3) + the scrape fallback (P1-2) are the reliability win over herdr;
  P1-2 rounds it out below the orchestration layer.
- **P2 is the capability gap (the only remainder)** — highest ceiling; the
  merge/scope registry builds *on* the shipped work — P1-3's stable per-tab id
  (restore-safe claim owners) and P3-2's autosave (persistence) — rather than on
  the live status signal.
- **P3 is independent polish (both shipped)** — P3-1 landed after P0; P3-2 (the
  persistence stance) stood alone; both done.

Each remaining item is a candidate for its own worktree + PR (per the project's
one-shape-per-PR norm), version-bumped, gated, and owner-tested before merge.

## What this does NOT change about spyc's identity

The wedge stays **file manager + in-process review + MCP** (review §4). Agent
awareness makes spyc a *better* agent host, but spyc's reason to exist over herdr
remains the file/navigation/review surface herdr doesn't have. Don't let the
agent-status work pull spyc toward being "just a better multiplexer."
