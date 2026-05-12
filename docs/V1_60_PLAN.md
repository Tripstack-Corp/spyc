# spyc v1.60 — CounterTop

**Status:** plan, not yet implemented.
**Predecessor:** [`V1_5_PLAN.md`](V1_5_PLAN.md) (six-phase pager / task-viewer
unification, shipped at v1.50.0).

## Thesis

A single spyc instance is one *project home*. Across a workday, the user
opens several — `tripstack/spyc`, `tripstack/platform`, a worktree, a
scratch repo. Today each lives in its own terminal window, and the user
loses track of which agent is running where, which one is blocked on
input, and which one finished an hour ago.

v1.60 introduces **CounterTop**: a hub view that sits above any one
spyc instance and aggregates state across every running spyc the user
has open, plus background agents tracked by Claude Code's `claude
agents` supervisor. One spyc to launch on terminal open, every
workspace and every agent visible from one place.

The thesis is recursive composition: spyc panes already host any
program; spyc happens to be a program; therefore spyc panes already
host spyc. The only missing pieces are a discovery surface so
instances can find each other, and a HUD that aggregates their state.

## Kitchen vocabulary

Naming the surfaces in keeping with spyc's existing spice + pepper
motif:

| Term         | Refers to                                                       |
| ------------ | --------------------------------------------------------------- |
| CounterTop   | The master / hub view. Where the user sees every workspace.     |
| Burner       | An active workspace. One is "on the heat" if it has an agent currently working. |
| Pass         | Workspace state "ready for the user" — agent is blocked on input, task is finished, PR is mergeable. The kitchen pass is where finished plates wait for the runner to take them. |
| Spice drawer | Persisted bundles of workspaces. "Today's stack" / "platform + spyc" / "release-train cleanup". Restored via a picker. |
| Ticket       | A dispatched agent prompt (`claude --bg`-style). Lives at the workspace level, not in CounterTop. |

These are public surface names. Code structs may keep neutral names
(`Workspace`, `WorkspaceSet`); the kitchen vocabulary is for UI labels,
help text, and command names.

## Architectural choice (decided)

Two routes were considered:

- **Monolith**: lift App-level state into `Vec<Workspace>` with an
  active index. One spyc process, many workspaces.
- **Recursive (chosen)**: each workspace is a child spyc process
  running in a pane tab of the master. Discovery + introspection via
  the MCP socket each child already exposes.

The recursive route wins on three axes:

1. **No state lift.** App / AppState / pane_tabs stay where they are.
   The new code is a discovery layer + a HUD pager that reads it.
2. **Composable.** A workspace can have nested workspaces if a user
   really wants. Bounded by terminal size, not by code.
3. **Uses what's there.** Each spyc already writes a context file and
   listens on a PID-scoped MCP socket. The master can query those
   sockets with the same JSON-RPC vocabulary the agents use.

The cost: more processes. A user with five workspaces has six spyc
processes (master + five). Memory is negligible (each ~10 MB
resident), and the OS keeps them cheap.

## Phases

Each phase is one PR, one version bump (v1.50.23 → v1.50.28-ish during
development, then a v1.60.0 release marker once the set is shipped and
dogfooded).

### Phase 0 — Discovery surface

**Goal:** every running spyc registers itself in a well-known location
so peer instances (and external tools) can enumerate them.

**Files:**
- `~/.local/state/spyc/instances/<pid>.json` — per-instance discovery
  record. Written on startup, unlinked on clean exit. Includes:
  ```json
  {
    "pid": 12345,
    "project_home": "/Users/derek/src/spyc",
    "session_name": "SAFFRON_CUMIN",
    "mcp_socket": "/Users/derek/.local/state/spyc/mcp-12345.sock",
    "started_at_secs": 1762605451,
    "label": "spyc"
  }
  ```
- New module: `src/state/instances.rs` — write/remove on
  startup/shutdown, enumerate, prune stale entries (PID not alive).

**Exit:** `ls ~/.local/state/spyc/instances/` shows one file per
running spyc; orphans from killed processes are cleaned up at next
launch.

### Phase 1 — CounterTop pager (read-only)

**Goal:** new `^a-W` (or `:counter`) opens the CounterTop view. A
pager-mount pulling discovery records + reading each instance's
context file. Read-only — informational.

**Surface:**
```
🍳 CounterTop                                              5 burners
  ✻ spyc            ~/src/spyc           claude:76422c62  working   2m
  ⚠ tripstack       ~/src/tripstack-pl…  codex:4a7cd126   needs you 5s
  ∙ platform-tests  ~/src/platform-tes…  bash             idle      28m
  ✓ release-train   ~/src/spyc           gemini:4c130f82  done      14m
  ✻ scratch         ~/Desktop/scratch    claude:1b22f0a3  working   45s
```

**Files:**
- `src/ui/counter_top.rs` — render the table as a pager.
- `src/app/mod.rs` — wire `Action::OpenCounterTop`, keymap binding
  (`^a-W` chord — repurpose; `W` chord today is worktrees, so use
  `^a-O` or `:counter`).
- `src/keymap/action.rs` — new variant.
- `src/ui/help.rs` — new entry.

**Exit:** open two spycs in different cwds, hit `:counter` in one,
see both rows. State indicator updates within 2s of an agent state
change.

### Phase 2 — `--hub` startup mode

**Goal:** `spyc --hub` launches with no project, no listing — just
the CounterTop maximized. Panes opened from hub spawn child spyc
processes, not bash / claude.

**Files:**
- `src/main.rs` — `--hub` flag.
- `src/app/mod.rs` — `AppMode::Hub` (existing modes plus this one).
  Hub mode suppresses the file list and centers the CounterTop.
  `^a-c` (new tab) defaults to `spyc` instead of the user's default
  pane command.
- README + docs/presentation.html — short mention.

**Exit:** `spyc --hub` opens an empty hub. `^a-c` spawns `spyc`,
which registers in discovery, which appears in the CounterTop. User
arrows down to it, `Enter` attaches to that child spyc (focuses its
pane tab).

### Phase 3 — Cross-workspace MCP introspection

**Goal:** the CounterTop's per-row state is live, pulled by querying
each child's MCP socket from the master.

**Files:**
- `src/mcp.rs` — new tool `get_workspace_status` (sibling of
  `get_spyc_context`). Returns project_home, session_name, active
  agent + short session id, agent activity state derived from
  `~/.claude/jobs/<id>/state.json` and the codex / gemini analogues.
- `src/ui/counter_top.rs` — per-tick query of each peer's socket. A
  short timeout (250 ms) per call; failed sockets render dimmed and
  drop after three failures.

**Exit:** a Claude pane in a child spyc enters "needs input" state →
the master's row shows ⚠ within ~2s. Killing the child process →
the row disappears after the stale check kicks in.

### Phase 4 — Agent View bridge

**Goal:** surface Claude Code's background sessions (`claude --bg`,
`/bg`-detached, things visible in `claude agents`) inside CounterTop
alongside spyc workspaces. The user sees one HUD covering both
spyc-managed and supervisor-managed work.

**Files:**
- `src/state/agent_view.rs` — reader for
  `~/.claude/daemon/roster.json` + `~/.claude/jobs/<id>/state.json`.
- `src/ui/counter_top.rs` — render a second section labelled
  `── background ──` below the spyc workspaces, listing supervisor
  sessions with the same shape (working / needs input / done).

**Exit:** `claude --bg "investigate X"` from any shell → that
session shows in CounterTop's background section. Pressing `Enter` on
that row spawns a new pane tab running `claude attach <id>` (i.e.
adopts the background session into a workspace under spyc).

### Phase 5 — Spice drawer

**Goal:** save and restore bundles of workspaces ("today's stack").
Session save is already per-workspace; this is a layer above —
"start these three spyc instances at these three cwds with these
saved sessions."

**Files:**
- `src/state/spice_drawer.rs` — read / write
  `~/.local/state/spyc/spice_drawer/<name>.json`. Each entry: a list
  of project_home + session_id pairs.
- `src/app/mod.rs` — `:drawer save <name>`, `:drawer open <name>`,
  `:drawer list`. From hub mode, `^a-D` opens a picker.

**Exit:** running 3 workspaces, `:drawer save morning` →
`spyc --hub --drawer morning` (or `:drawer open morning` from inside
hub) spawns the same 3 workspaces with their saved sessions.

### Phase 6 — Pass + polish

**Goal:** make the HUD scannable at a glance. "Pass" is a derived
aggregate: how many workspaces / sessions are blocked on the user.

**Files:**
- `src/ui/status.rs` — when running with multiple peers visible, add
  a `pass: 2` segment showing the count of rows in the "needs input"
  / "done" buckets.
- `src/ui/counter_top.rs` — group rows: `Burners` (working), `Pass`
  (needs input / done), `Idle`. Match Agent View's grouping ergonomics
  without copying its keys.
- Term-title integration: `🌶️ CounterTop · 3 burners · 1 on the pass`.

**Exit:** opening spyc in the morning, glancing at the title bar of
the terminal alone tells you which workspace wants you.

## Out of scope (v1.60)

- **No re-implementing `claude agents`.** Agent View runs in its own
  pty if you want it; CounterTop only *reads* its state files.
- **No master-controls-children writes.** The master can't (yet) send
  commands to children beyond opening their MCP tools. Bidirectional
  control is a v1.61+ question.
- **No nested CounterTops.** A child spyc opening its own hub is
  technically allowed (recursive composition), but the master doesn't
  flatten the tree. Mental model: leaves of the tree are agents,
  branches are workspaces, root is the hub the user launches.
- **No automatic background-on-quit.** If you quit a workspace its
  agents stop (modulo claude's existing supervisor behaviour). v1.60
  doesn't reach into `/bg` semantics.

## Open questions

- **Where does `:counter` live in the resolver?** `^a-W` collides
  with the worktree chord (`W l/n/d`). Candidates: `^a-O` (overview),
  `:counter`, `^a-^a` (double prefix). Decide in Phase 1.
- **Does the master inherit the spice-pair session name?** Or is the
  hub itself nameless? Lean nameless — its identity is the set of
  burners.
- **Per-socket query budget.** A user with 10 workspaces means 10
  MCP roundtrips per CounterTop render. Plan: cache state per peer
  with a 2s TTL; render on whatever's cached.
- **What does session save look like in hub mode?** The hub itself
  is stateless; quitting it shouldn't kill children unless the user
  asks. Probably `:q` becomes a no-op in hub mode and `:Q` is the
  "stop everything" command.

## Naming reference

This document uses kitchen vocabulary for UI surfaces but neutral
names in code. Map for future cross-reference:

| UI surface      | Code              | What it is                                       |
| --------------- | ----------------- | ------------------------------------------------ |
| CounterTop      | `CounterTopView`  | The master HUD pager / hub-mode main view.       |
| Burner          | `Workspace` row   | One row in CounterTop = one workspace.           |
| Pass            | (derived state)   | Aggregate "ready for you" count.                 |
| Spice drawer    | `SpiceDrawer`     | Saved-workspace-bundle store.                    |
| Ticket          | (no struct)       | UI-side label for a dispatched agent prompt.     |
