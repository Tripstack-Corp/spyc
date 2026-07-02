# Agent orchestration & awareness

How spyc helps you — and the coding agents in its panes — **know what each agent
is doing** and **coordinate so concurrent agents don't collide**. This is the
user- and agent-facing reference for the shipped system; the design history +
rationale live in the archived charter
([`docs/archive/AGENT_AWARENESS_PLAN.md`](archive/AGENT_AWARENESS_PLAN.md)).

Two halves:

- **Awareness** — a live status dot per agent tab ("which agent needs me?"), a
  desktop ping when one blocks/finishes, and session restore that brings each
  agent's conversation back.
- **Coordination** — agents declare the file scope they're touching and can
  *wait* on a conflicting merge, so a fleet of agents serializes instead of
  racing (the merge-train problem).

Everything is **single-process, in-memory, MCP-driven** — no daemon, no second
runtime. Every agent pane in one spyc shares that spyc's MCP socket, so one
in-memory view coordinates them all.

---

## 1. Activity dots — "which agent needs me?"

Each **agent** pane tab carries a live dot in the divider. Its state comes from
three tiers; a higher tier always wins:

1. **Semantic self-report** (best) — the agent calls the `report_status` MCP tool
   (or its lifecycle hook does): `working` / `blocked` / `done` / `idle`.
2. **Scrape fallback** — for an agent that can't self-report, spyc reads its
   *visible screen* for a known prompt (today: gemini's `Allow execution of:`
   approval → `blocked`). Second-class; a live report always overrides it.
3. **Output timing** — with neither of the above, output flowing = `working`,
   silence = `idle`.

**Glyphs** (shape = liveness, colour = urgency):

| Dot | Meaning |
|-----|---------|
| heat-pulse `●` (pepper→ember) | **working** — output flowing or a `working` report |
| quiet `·` | **idle** |
| steady **red** square `■` | **blocked** — waiting on you ("needs me") |
| calm **teal** square `■` | **done** — finished a turn |
| `💤` | `^z`-suspended |

`blocked` is **latched**: it stays red until you press **Enter** in that pane (or
the agent files a newer report) — no timer or stray output bounces it off.

**Auto-reporting.** So it works without the agent choosing to call the tool, spyc
installs lifecycle hooks that run `spyc --report-status` (prompt-submit→working,
needs-permission→blocked, turn-end→done): **claude** (`.claude/settings.json`),
**codex** (`.codex/config.toml`), **agy** (`.agents/hooks.json`, no `blocked`).
It asks first — a `[Y/n]` on the first launch per repo, saved; change it later
with **`:hooks on|on!|off`**.

**Debug:** **`:why-status`** flashes the active tab's state + source; **`:activity
dump`** opens a pager with every pane's derivation.

---

## 2. Notifications — the ping when an agent needs you

On a tab's transition to **blocked** or **done**, spyc fires a desktop
notification naming the tab — so you can alt-tab away and get pulled back to the
*right* pane. Configured under `[notify]` (see
[`CONFIGURATION.md`](../CONFIGURATION.md)):

- **Desktop ping** — on by default, on both blocked/done. Over SSH it routes as
  an OSC-9 terminal escape (reaches your *client* terminal); locally it uses the
  OS notifier.
- **Visual bell** (a spice-heat border pulse) — on by default; **terminal bell**
  — off by default.
- `suppress_focused_tab` mutes the tab you're already watching (off by default —
  spyc-focused ≠ eyes-on-terminal).

---

## 3. Session persistence & resume

spyc auto-saves your workspace **on quit** *and* re-saves a couple of seconds
after any change (a debounced, crash-sufficient autosave written atomically), so
a `SIGKILL` / crash / laptop-sleep loses at most that window — not the session.

**`spyc -r`** restores: pane tabs (each agent respawned + its *exact* conversation
resumed — Claude's live session id is captured as it runs, so restore replays the
right `--resume <id>` instead of guessing), the vertical split, **and the scope
registry** (§4) — coordination survives a restart mid-merge-train.

Restored panes are **fresh shells + resumed conversations**, not PID-preserved
live processes (spyc is single-process by design; no detach/reattach daemon).

---

## 4. Merge / scope coordination — the registry

**The problem it solves:** several agents working concurrently (across worktrees,
tabs) collide on merges — overlapping files, the `Cargo.toml` version line,
racing PRs. The `spyc-semver` merge driver already auto-resolves the *mechanical*
version-line clash; this handles the *semantic* overlap.

**The model:** each agent **declares the scope it's touching** and its **intent**;
another agent can see that and **wait** on a conflicting merge. spyc is
**advisory** — it informs and offers a wait primitive; it never blocks a merge
itself and never auto-spawns agents. The registry is **in-memory**, **session-
persisted** (survives `-r`), and a claim is **auto-released when its tab closes**.

### MCP verbs

| Verb | What it does |
|------|--------------|
| `register_scope(paths, intent, pr?, note?)` | Claim a file set. `intent` = `editing` (informational) or `merging` (blocks others' waits). Returns `{claim_id, conflicting_merges}`. |
| `list_scopes()` | The whole registry — who's touching what, intents, PRs. Check before you merge. |
| `wait_for_scope_clear(paths, timeout_ms?)` | **Block** until no *other* owner's `merging` claim overlaps `paths`, or the timeout fires (default 5 min, hard cap 10 min). Returns `{outcome: cleared|timed_out, conflicts}`. Your own claims never block you. |
| `release_scope(id)` | Drop a claim (also automatic on tab close). |

`paths` are literal paths or globs (`src/app/*.rs`); overlap is checked either
direction. Only **`merging`** claims block a wait — `editing` is purely
informational.

### The canonical merge-train workflow

```
register_scope(paths=[my PR's files], intent="merging", pr="#123")
wait_for_scope_clear(paths=[my PR's files])   # blocks until conflicts clear
<merge / rebase / push>
release_scope(id)
```

Concurrent agents doing this **serialize on overlap** instead of colliding and
rebasing. Non-overlapping merges proceed in parallel untouched.

### Inspecting it

- **`:agent registry`** — pager dump of every claim + how many agents are parked
  in a wait.
- **`:agent list`** — each agent tab with its activity dot + the scope it owns.

---

## 5. What's intentionally *not* here

- **No agent-drives-agent** (`read_pane` / `send_pane_keys`) — agents coordinate
  through the *registry*, not by typing into each other's terminals.
- **No streaming event feed** (`subscribe_events`) — `wait_for_scope_clear` + the
  `:agent` dumps cover the need; a continuous feed invites agents narrating at
  each other.
- **No cross-instance registry** — coordination is within one spyc (its shared
  socket). Agents in *separate* spyc processes don't see each other's claims.
- **No detach/reattach daemon, no many-pane grid** — spyc stays a single-process
  file+agent manager, not a multiplexer.

---

## Design history

The full P0–P3 design, alternatives, and competitive rationale (herdr/cmux/Claude
Squad) are in the archived charter:
[`docs/archive/AGENT_AWARENESS_PLAN.md`](archive/AGENT_AWARENESS_PLAN.md). Code
map: `src/app/agent_status.rs` (dots + notify + autosave settle),
`src/mcp/` + `src/mcp_cmd.rs` + `src/app/mcp.rs` (the MCP verbs + wait parking),
`src/state/scope_registry.rs` (the registry + conflict logic),
`src/agent/` (per-agent profiles + hooks + scrape rules).
