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

**Remaining** — the live scope this charter still tracks:

- **P1-2** — data-driven detection manifest (the scrape *fallback* for agents
  spyc can't auto-hook).
- **P1-3** — live session-identity capture (`report_agent_session`).
- **P2** — orchestration: event hub + wait/subscribe + read/send (the capability
  gap; the highest-ceiling item).

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

Three more ideas ride alongside: an **orchestration capability** (let one agent
*wait on / subscribe to* another's status — the genuine gap vs MCP today), live
**session-identity capture** (more reliable than spyc's exit-banner sniffing),
and a **notification/bell** when a backgrounded agent needs you *(shipped —
P3-1)*.

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

## Phase 1 (remaining) — Reliable detection: the fallback + live identity

The self-report core (P1-1) shipped; what's left is the graceful-degradation
fallback for agents spyc can't hook, and a live (rather than exit-banner-sniffed)
session-identity signal.

**P1-2 · Data-driven detection manifest (the fallback).**
For agents that don't self-report, replace the imperative per-profile scrape with
a declarative ruleset on `AgentProfile`: priority-ordered rules, each with a
`region` (`bottom_non_empty_lines(N)` / `after_last_horizontal_rule` /
`osc_title` / `osc_progress`), a matcher (`contains` / `regex`, composed
`any`/`all`/`not`), a target state, and a `visible_blocker` hint. Add the
debounce/hysteresis herdr tuned (≈3 idle confirmations @ 100ms, cap ~700ms) to
kill flicker. Region-/OSC-scoped scanning beats spyc's current fixed-slice scan.
*Touches:* `src/agent/` (profile gains an optional ruleset), `agent_status.rs`
(one engine drives all agents). *Effort: med.*
*Better-than-herdr:* a fallback, not the primary — so an agent UI change degrades
gracefully instead of breaking the headline feature.
*Use cases:* an agent spyc **can't** auto-hook still gets a live dot instead of
going dark — a `gemini` / `zot` / `aider` pane (no `status_hooks()`), or a
**wrapper-hidden** Claude/codex (`nix`, Bubblewrap, Happy) whose hook env didn't
propagate. This is exactly the gap cmux shipped blind into — *"sidebar agent status
is hook-only with no fallback … codex agents go silent when hooks miss"*
([cmux #3749](https://github.com/manaflow-ai/cmux/issues/3749)) — and the same
wrapper problem herdr band-aids with a manual `HERDR_AGENT=claude` env hint. The
declarative ruleset is spyc's safety net: a missed hook degrades to a scrape guess,
never to nothing. Keep it clearly second-class (a report always wins) so we never
re-enter herdr's fragility class as the *primary* path.

**P1-3 · Live session-identity capture (`report_agent_session`).**
Have the same hook report the agent's session UUID + transcript path *as it runs*
(herdr's Tier-1 model), instead of spyc's current **exit-banner sniffing**. More
reliable `-r` restore; fewer missed resumes. spyc already pins codex session ids
(`app::codex_pin`) — this generalizes that to a live, agent-driven signal.
*Touches:* `src/agent/` profiles, `src/state/sessions/`, `src/mcp/`, hook assets.
*Effort: low–med.*
*Use cases:* reliable `spyc -r` — the agent reports its own session UUID + transcript
path *live*, so restore replays the **right** native `--resume <id>` instead of
guessing by spawn-proximity (spyc's own #607: two same-cwd Claude panes crossed
conversations on restore — exactly this failure). The field keeps arriving here:
*"preserving the agent session ID and calling the native resume command is the key
part"* ([Zed #58001](https://github.com/zed-industries/zed/discussions/58001)), and
Claude Code's own `--resume` throws *"No conversation found"* when its index desyncs
from the on-disk `.jsonl` ([CC #18311](https://github.com/anthropics/claude-code/issues/18311)).
A live, agent-driven signal sidesteps that whole class — and generalizes the codex
spawn-order pin (`app::codex_pin`) into one agent-agnostic mechanism.

---

## Phase 2 — Orchestration: wait / subscribe *(med–high effort — the capability gap)*

The single most valuable steal: today spyc's MCP is query+mutate; herdr's socket
lets an agent **block until another agent changes state**. That's "agents
orchestrate agents," and it's beyond what MCP does for spyc now.

> **Field-validated workflows (what wait/subscribe actually unlocks).** herdr's
> socket already ships this exact verb set — `wait agent-status … --status done`,
> `events.subscribe`, `pane.read`, `pane.send_keys` — and AWS Labs'
> [`cli-agent-orchestrator`](https://github.com/awslabs/cli-agent-orchestrator/blob/main/docs/herdr.md)
> scripts against it, so the *shape* is proven, not hypothetical. The concrete jobs
> spyc's tools would serve (each composing with spyc's worktree MCP tools):
> - **Fan-out → gather.** Spin up N agents across N worktrees (`create_worktree`),
>   `wait_for_pane_status` until each is `blocked`/`done`, then review the batch. The
>   dominant multi-agent pattern — Claude's own agent-teams steer the lead with
>   *"wait for your teammates to complete their tasks before proceeding"*
>   ([agent-teams docs](https://code.claude.com/docs/en/agent-teams)).
> - **Parallel review by lens.** Three teammates review a PR for security / perf /
>   tests concurrently; the lead synthesizes once all report `done` (Anthropic's own
>   headline example, ibid). Pairs naturally with spyc's in-process diff/review loop.
> - **Watch-for-blocked → escalate.** `subscribe_events` for the first `blocked`
>   transition and surface "who needs you" — the supervisor case (feeds P3-1's ping).
> - **Sequential handoff.** A finishes → `read_pane` its output → `send_pane_keys` to
>   brief B. The case in-process subagents *can't* cover: they *"only report results
>   back to the parent and never talk to each other,"* and can't span heterogeneous
>   agents (claude + codex + aider) the way a terminal socket can.
>
> **Guardrails (the skeptic's counterpoints are real — bake them in).** Auto-
> coordination can collapse into agents *"generating huge status reports for each
> other"* instead of doing work ([HN 47245373](https://news.ycombinator.com/item?id=47245373));
> runaway loops burn tokens with no circuit-breaker; and Anthropic itself notes that
> for sequential / same-file / heavy-dependency work a single session beats a team.
> So spyc ships the **primitives** (wait/subscribe/read/send) and stays a *primitive*
> — the human or a deliberately-authored orchestrator composes them; **spyc never
> auto-spawns**. `timeout` is mandatory on every wait (no unbounded blocks), and the
> PID-scoped socket + takeover prompt keep a stale waiter from wedging the loop.

**P2-1 · MCP event hub.**
A sequenced ring buffer of `(seq, Event)` + a subscriber list, fanned out from
spyc's single `mpsc` `Message` loop (drained in the pre-recv scan, like every
other source). **Edge-triggered** (emit only on transition) with `events_after(seq)`
catch-up. Event kinds: `pane.agent_status_changed`, `pane.output_matched`,
`pane.exited`, `pane.agent_detected`.
*Touches:* `src/mcp/`, the event loop (`src/app/sources.rs` / `loop_steps.rs`),
`agent_status.rs`. *Effort: med.*

**P2-2 · `wait_for_pane_status` + `subscribe_events` MCP tools.**
`wait_for_pane_status(pane, status, timeout)` (long-poll or event-backed) and a
streaming `subscribe_events`. Now a top-level Claude in spyc can "spin up codex in
worktree `b`, wait until it's blocked or done, then read its output" — composing
with spyc's worktree MCP tools.
*Touches:* `src/mcp/protocol.rs`, the hub. *Effort: med.*

**P2-3 · `read_pane` + `send_pane_keys` MCP tools.**
Let an agent read what a *sibling* pane shows (spyc already does pane text reads
internally for `gf` / quick-select) and send it input. Pairs with P2-1/2 for real
cross-pane orchestration.
*Touches:* `src/mcp/`, `src/pane/`. *Effort: med.*
*Keep spyc's edge:* one PID-scoped socket + the takeover prompt (better-scoped
than herdr's no-auth, first-to-bind sockets); reuse the `crate::VERSION` staleness
check spyc already does.

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
P0 ✅  ──►  P1-1 ✅  ──►  P1-2 fallback / P1-3 session-id (remaining)
                              │
                              ▼
                         P2 (event hub + wait/subscribe + read/send)
                              │
                              ▼
                         P3-1 ✅        P3-2 ✅ crash-sufficient autosave
```

- **P0 (shipped)** — you can't ship or debug status without a vocabulary, a
  render surface, and `:why-status`.
- **P1 is the moat** — semantic-first detection (P1-1, shipped) is the
  reliability win over herdr; the fallback (P1-2) and live session id (P1-3)
  round it out before the orchestration layer leans on it.
- **P2 is the capability gap** — highest ceiling, but it depends on P1's status
  signal being trustworthy.
- **P3 is independent polish (both shipped)** — P3-1 landed after P0; P3-2 (the
  persistence stance) stood alone and is now done, leaving P1-2/P1-3/P2 as the
  live remainder.

Each remaining item is a candidate for its own worktree + PR (per the project's
one-shape-per-PR norm), version-bumped, gated, and owner-tested before merge.

## What this does NOT change about spyc's identity

The wedge stays **file manager + in-process review + MCP** (review §4). Agent
awareness makes spyc a *better* agent host, but spyc's reason to exist over herdr
remains the file/navigation/review surface herdr doesn't have. Don't let the
agent-status work pull spyc toward being "just a better multiplexer."
