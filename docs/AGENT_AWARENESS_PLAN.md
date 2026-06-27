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

Three more ideas ride alongside: an **orchestration capability** (let one agent
*wait on / subscribe to* another's status — the genuine gap vs MCP today), live
**session-identity capture** (more reliable than spyc's exit-banner sniffing),
and a **notification/bell** when a backgrounded agent needs you.

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
  without the daemon. (Tracked as item P3-2, not here.)
- **A many-agent grid multiplexer.** herdr *is* a multiplexer; spyc is a file
  manager with pane tabs + a file-commander vsplit. We nail the **2–4 agent**
  case, not a 50-pane wall (see review §4 objections).
- **Scrape-only detection.** We keep a scrape *fallback*, but never make it the
  primary path — that's the herdr trap we're avoiding.

---

## Phase 0 — Status vocabulary + a place to show it *(low effort, no new deps)*

Foundation the rest builds on. spyc already computes a coarse agent status
off-thread; this formalizes it and gives it a render surface.

**P0-1 · Explicit status vocabulary.**
A small enum — `{ Idle, Working, Blocked, Unknown }` — plus a derived `Done`
(a finished turn the user hasn't looked at yet; derived, never reported). Lives
on the agent/pane state, computed in `src/app/agent_status.rs`.
*Touches:* `src/agent/`, `src/app/agent_status.rs`. *Effort: low.*
*Better-than-herdr:* same vocabulary herdr settled on (validates it), but sourced
from a semantic channel in P1, not glyphs.

**P0-2 · Per-pane-tab status dot.**
Render a colored dot per pane tab in the divider (spyc already renders the pane
tab bar + task markers in `src/app/render/chrome.rs`). **Worst-state-wins**
rollup when summarizing (Blocked > Done-unseen > Working > Idle-seen > Unknown),
matching herdr's aggregation. Honor the review's warning: **stable pane indices**
— never reorder tabs by attention (herdr users complain about exactly that).
*Touches:* `src/app/render/chrome.rs`, pane-tab state. *Effort: low.*

**P0-3 · `:why-status` debug surface.**
A command (and/or an `A`-overlay line) that explains *why* a pane has its current
status — which rule/region/signal fired. herdr's `agent.explain` is invaluable
for a fuzzy system; spyc's status work is on the backlog precisely because it's
hard to debug. Build this early.
*Touches:* `src/app/command_table.rs` + a small report in `agent_status.rs`.
*Effort: low.*

---

## Phase 1 — Reliable detection (the differentiated core) *(med effort)*

The heart of the plan: semantic self-report first, tunable scrape fallback.

**P1-1 · `report_status` MCP tool + a self-report agent hook.**
Add an MCP tool the agent can call to set its own pane status
(`working`/`blocked`/`idle`, optional message). Then — using the **existing**
`ensure_agent_mcp_config` lazy-write path — also drop a tiny status hook into the
agent's own config dir (Claude `Stop`/`Notification`/`SessionStart` hooks; codex
equivalent) that fires on the agent's native lifecycle and reports over the
socket. Carry an **authority model**: a monotonic `seq` (ordering/skew guard) and
a `ttl_ms` (a crashed agent's stale "working" auto-expires) — herdr's design,
reimplemented clean.
*Touches:* `src/mcp/` (new tool + protocol), `src/app/mcp.rs`,
`ensure_agent_mcp_config`, hook assets. *Effort: med.*
*Better-than-herdr:* herdr's Tier-1 agents (claude, codex — spyc's primaries)
*still* scrape for live status; the hook only buys session identity. spyc makes
the **primary** path semantic for those exact agents.

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

**P1-3 · Live session-identity capture (`report_agent_session`).**
Have the same hook report the agent's session UUID + transcript path *as it runs*
(herdr's Tier-1 model), instead of spyc's current **exit-banner sniffing**. More
reliable `-r` restore; fewer missed resumes. spyc already pins codex session ids
(`app::codex_pin`) — this generalizes that to a live, agent-driven signal.
*Touches:* `src/agent/` profiles, `src/state/sessions/`, `src/mcp/`, hook assets.
*Effort: low–med.*

---

## Phase 2 — Orchestration: wait / subscribe *(med–high effort — the capability gap)*

The single most valuable steal: today spyc's MCP is query+mutate; herdr's socket
lets an agent **block until another agent changes state**. That's "agents
orchestrate agents," and it's beyond what MCP does for spyc now.

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

## Phase 3 — Polish *(low effort)*

**P3-1 · Notification + bell on blocked/done.**
A new `Effect` that fires a desktop notification (and optional terminal bell) when
a backgrounded agent pane goes `Blocked` or `Done`. Cheap, high-value for the
dog-fooding loop: you alt-tab away, the agent needs you, you get pinged. herdr
does this (`AgentStatus::Done → sound`).
*Touches:* `src/app/agent_status.rs`, a new `Effect` in `src/app/effect.rs`.
*Effort: low.*

**P3-2 · Richer session-restore content (the persistence answer).**
Capture the split/pane **layout tree** + per-pane **cwd** in the session snapshot
so `-r` rebuilds geometry faithfully, and make periodic autosaves
**recovery-sufficient against `SIGKILL`** (never a thin fallback to a quit-time
flush). This is spyc's deliberate, daemon-free answer to the persistence pain
herdr solves with a server (review §3 #4).
*Touches:* `src/app/session.rs`, `src/state/sessions/`. *Effort: low–med.*

---

## Sequencing & rationale

```
P0 (vocabulary + dot + :why)  ──►  P1 (semantic report + fallback + session id)
                                        │
                                        ▼
                                   P2 (event hub + wait/subscribe + read/send)
                                        │
                                        ▼
                                   P3 (notify/bell, richer -r)
```

- **P0 first** — you can't ship or debug status without a vocabulary, a render
  surface, and `:why-status`.
- **P1 is the moat** — semantic-first detection is the reliability win over herdr;
  do it before the orchestration layer leans on it.
- **P2 is the capability gap** — highest ceiling, but it depends on P1's status
  signal being trustworthy.
- **P3 is independent polish** — P3-1 can land any time after P0; P3-2 is the
  persistence stance and stands alone.

Each item is a candidate for its own worktree + PR (per the project's one-shape-
per-PR norm), version-bumped, gated, and owner-tested before merge.

## What this does NOT change about spyc's identity

The wedge stays **file manager + in-process review + MCP** (review §4). Agent
awareness makes spyc a *better* agent host, but spyc's reason to exist over herdr
remains the file/navigation/review surface herdr doesn't have. Don't let the
agent-status work pull spyc toward being "just a better multiplexer."
