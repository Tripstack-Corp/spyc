# spyc v1.60 — CounterTop

> **Archived (2026-07-02) — considered & parked.** Design-history, not an active
> plan. A multi-instance hub with frame-mirroring + headless `--detached` spycs
> fights spyc's single-process MVU/sync core (the same reason a detach/reattach
> daemon is out of scope). The attention pain it targets — "which agent needs
> me, across windows?" — was met the single-process way instead: in-instance
> agent-status dots + notifications, and the MCP scope registry for merge
> coordination. Live summary: `ROADMAP.md` → "Post-2.0 (2.x)". Named for a
> version (1.60) the project shipped past (now 1.97+).

**Status:** archived design (parked — see banner).
**Predecessor:** [`V1_5_PLAN.md`](V1_5_PLAN.md) (six-phase pager / task-viewer
unification, shipped at v1.50.0).

## Thesis

A single spyc instance is one *project home*. Across a workday, the
user opens several — `tripstack/spyc`, `tripstack/platform`, a
worktree, a scratch repo, each in its own terminal window. Today the
user loses track of which agent is running where, which one is blocked
on input, and which one finished an hour ago.

v1.60 introduces **CounterTop**: a hub view that connects to every
running spyc the user has open and aggregates their state. It can also
spawn new headless spyc instances and "take control" of any peer —
forwarding your keystrokes to it and mirroring its screen.

The architectural choice is **siblings + mirror**, not recursive
composition. Each spyc owns its own pty and lives in its own terminal
window (or runs headless under the hub). They don't have parent/child
relationships. The hub is a peer that happens to be a client of every
other peer's MCP socket. The "cursor moves in two places" effect when
you take control is a deliberate property of having two clients
attached to the same render state.

A keyboard can only focus one terminal window at a time anyway, so the
risk of "both sides typing at once" is structural — the OS already
serializes intent. Last-keystroke-wins is the right semantic.

## Kitchen vocabulary

| Term         | Refers to                                                       |
| ------------ | --------------------------------------------------------------- |
| CounterTop   | The hub view — where the user sees every running spyc.          |
| Burner       | An active spyc instance. One is "on the heat" if its active pane has an agent currently working. |
| Pass         | Aggregate "ready for you" state — agent blocked on input, task finished, PR mergeable. Where finished plates wait for the runner. |
| Spice drawer | Saved bundles of spyc-instance configurations. Restoring a drawer entry spawns the right detached spycs at the right project homes. |
| Ticket       | A dispatched agent prompt inside a spyc instance (`claude --bg`-style). Lives at the workspace level, surfaced by CounterTop but managed inside each spyc. |
| 👁 badge     | Status-bar indicator on a spyc instance showing N clients are currently mirroring its frames. |

UI labels use the kitchen vocabulary. Code structs keep neutral names
(`Workspace`, `MirrorClient`, etc.).

## Architectural choice (decided)

Three routes were considered during planning:

- **Monolith refactor**: lift App state into `Vec<Workspace>` with an
  active index. One spyc process, many workspaces. Rejected — too much
  state to lift, complicates persistence and process model.
- **Recursive composition**: each workspace is a child spyc running in
  a pane tab of the master. Considered first; rejected after design
  discussion. The user model is "I open spycs in real terminals, then
  optionally bring up a hub" — not "I start a hub and launch
  everything from it."
- **Siblings + mirror (chosen)**: every spyc runs independently. The
  hub discovers and connects to them as peer clients. Frame mirroring
  and input forwarding ride over the MCP socket the spyc already
  exposes.

### Why siblings + mirror

1. **Spycs are launched the way you launch them today.** No change to
   the standalone-launch workflow. `spyc` in a terminal still opens a
   spyc; the hub is purely additive.
2. **Hub lifecycle is independent.** Quit the hub → every spyc keeps
   running in its own terminal. Launch the hub later → it rediscovers
   everything.
3. **Reuses the MCP socket.** Each spyc already listens on a
   PID-scoped Unix socket and speaks JSON-RPC. We extend the
   vocabulary with `subscribe_frames` + `send_input`. No new daemon,
   no new transport.
4. **Two clients = two viewers of the same render state.** When the
   hub attaches, the original terminal AND the hub both show the same
   frames. The OS guarantees single-focus, so input is naturally
   serialized — whichever window has keyboard focus gets to type.
5. **"Take control" is just "mirror + forward input."** A remote spyc
   in the hub becomes a widget that renders the published frame stream
   and forwards keystrokes back through the socket. Same widget shape
   as a pty pane (vt100 + cell grid), different source.

## Compatibility

Spycs running on the same machine won't all be the same version —
the user updates spyc periodically and any number of older instances
can be alive when a newer hub launches. Two layers handle this:

- **`spyc_version`** (semver string) — informational. Shown in the
  CounterTop row so the user can tell what's running where.
- **`capabilities`** (flat string array) — the negotiation layer.
  The hub only invokes operations whose capability the peer
  advertises. New capability names are added as new features land;
  removed names are reserved.

The minimum capability set for v1.60 is:

| Capability      | Means                                                          |
| --------------- | -------------------------------------------------------------- |
| `status`        | Peer responds to `get_workspace_status` (Phase 1).             |
| `frame_mirror`  | Peer responds to `subscribe_frames` (Phase 2).                 |
| `input_forward` | Peer responds to `send_input` (Phase 3).                       |

CounterTop behavior is **always-visible, gracefully-degraded** for
older peers:

| Peer caps                                    | Hub behavior                                                                  |
| -------------------------------------------- | ----------------------------------------------------------------------------- |
| Discovery file only (pre-1.60, no `status`)  | Row shows name + version + `pre-1.60 · status unavailable`. No live state.    |
| Has `status`, no `frame_mirror`              | Live state visible. Peek shows a last-known-text snapshot (cheap fallback).   |
| Has `frame_mirror`, no `input_forward`       | Mirror works; attach is read-only; status bar shows `👁 read-only`.          |
| Full set                                     | Full UX.                                                                      |
| Hub older than peer                          | Hub treats unknown capabilities as missing; never tries to call them.         |

Principle: **an older peer is visible but degraded, never invisible.**
Mid-rollout, "I can see all my work" still holds even if some
workspaces only show their project home + session name.

A `schema_version` field on the discovery file itself is the escape
valve for backwards-incompatible file-format changes. The hub
silently skips entries whose `schema_version` it doesn't understand.

## Phases

Each phase is one PR, one version bump (v1.50.26-ish during
development, then a v1.60.0 marker once shipped + dogfooded).

### Phase 0 — Discovery surface

**Goal:** every running spyc publishes its presence in a well-known
location so the hub (and external tools) can enumerate, and changes
become visible quickly without polling.

**File shape** (`~/.local/state/spyc/instances/<pid>.json`):

```json
{
  "schema_version": 1,
  "pid": 12345,
  "spyc_version": "1.60.0",
  "capabilities": ["status", "frame_mirror", "input_forward"],
  "project_home": "/Users/x/src/spyc",
  "session_name": "SAFFRON_CUMIN",
  "mcp_socket": "/Users/x/.local/state/spyc/mcp-12345.sock",
  "started_at_secs": 1762605451,
  "label": "spyc",
  "mode": "regular"
}
```

- `schema_version` — bumped only when the file shape itself
  changes incompatibly. Hub silently skips entries whose
  `schema_version` it doesn't understand.
- `capabilities` — the negotiation surface (see Compatibility).
- `mode` — one of `"regular"` / `"headless"` / `"hub"`.

**Publish (peer side):**
- Written atomically on startup: write to `<pid>.json.tmp`, then
  `rename`. POSIX guarantees rename is atomic, so readers either
  see the old file or the new file, never a half-written one.
- Re-written atomically on any durable state change worth
  surfacing — `:project` updates `project_home`, `:name` updates
  `session_name`, etc.
- Unlinked on clean quit. Wrapped in the same panic handler that
  restores the terminal so a panic still tries to clean up.

**Discover (hub side):**
- On launch, scan `instances/*.json`; deserialize each; skip
  unparseable or unknown-`schema_version` entries.
- Continuously, watch the directory with `notify` (already a
  runtime dep — same crate used for `.spycrc.toml` live reload):
  - `Create` event → read the new file, add a row.
  - `Modify` event → re-read, update the row.
  - `Remove` event → drop the row immediately.
- New peer changes appear in the hub within tens of ms, no
  polling on either side.

**Stale detection:**
- Background tick every ~5s: for each known peer, `kill(pid, 0)`.
  `ESRCH` → unlink the orphan file and drop the row.
- Faster path: any MCP RPC returning `ECONNREFUSED` immediately
  marks the peer stale and triggers cleanup.

**Files (code):**
- New `src/state/instances.rs` — write/remove/enumerate/prune,
  the notify watcher, the kill(pid, 0) sweeper.

**Exit:** `ls ~/.local/state/spyc/instances/` shows one file per
running spyc; orphans from killed processes are cleaned up at next
launch *or* within ~5s of the next sweep, whichever comes first.
`:project /new/path` in one peer updates its row in another peer's
hub within ~tens of ms.

### Phase 1 — CounterTop view (read-only)

**Goal:** a new pager-mounted view that lists every running spyc with
metadata pulled from each peer's MCP socket. No mirror yet, no
control — informational.

**Surface (sketch):**
```
🍳 CounterTop                                              5 burners
  ✻ spyc            ~/src/spyc           claude:76422c62  working   2m
  ⚠ tripstack       ~/src/tripstack-pl…  codex:4a7cd126   needs you 5s
  ∙ platform-tests  ~/src/platform-tes…  bash             idle      28m
  ✓ release-train   ~/src/spyc           gemini:4c130f82  done      14m
  ✻ scratch         ~/Desktop/scratch    claude:1b22f0a3  working   45s
  · old-checkout    ~/src/legacy         (1.59.0)         pre-1.60  3h
```

The last row above is what an older peer looks like: rendered from
the discovery file alone (no MCP `status` capability), with a quiet
`·` indicator and a `pre-1.60` tag. Project home and session name
still visible. See [Compatibility](#compatibility) for the full
behavior matrix.

**Files:**
- `src/ui/counter_top.rs` — render the table.
- `src/mcp.rs` — new tool `get_workspace_status` (sibling of
  `get_spyc_context`). Returns project_home, session_name, active
  agent + short session id, agent activity state derived from
  `~/.claude/jobs/<id>/state.json` and the codex / gemini analogues.
  Add `status` to every fresh spyc's advertised capabilities.
- `src/app/mod.rs` — `Action::OpenCounterTop`, keymap binding.
- `src/keymap/action.rs` — new variant.
- `src/ui/help.rs` — new entry.

**Exit:** `:counter` from any spyc opens the HUD with rows for every
running spyc; state indicator updates within ~2s of an agent state
change in any peer. A peer with no `status` capability still renders
as a row with the metadata from its discovery file plus a quiet
`pre-1.60` tag.

### Phase 2 — Frame mirroring

**Goal:** the hub can *see* what a remote spyc is rendering. New tee
ratatui backend in every spyc publishes its current frame to
subscribers via MCP. The hub renders the stream.

**Mechanism:**
- New `TeeBackend` wraps the standard `CrosstermBackend`. On each
  flush it computes the encoded ANSI delta and pushes it onto a
  publish channel (a small ring buffer + condvar).
- New MCP method `subscribe_frames` — returns a streaming subscription.
  Subscribers get full snapshot on attach + deltas on each render.
  When no subscribers, the tee is a passthrough (zero cost).
- Hub uses the existing `vt100::Parser` + the same `widget.rs` that
  renders ptys to render the mirrored stream.
- **Observer badge** lit on the remote spyc's status bar
  (`👁 N`) when subscriber count > 0.

**Files:**
- `src/ui/tee_backend.rs` — new ratatui backend wrapper.
- `src/mcp.rs` — `subscribe_frames` method + subscriber tracking.
  Add `frame_mirror` to every fresh spyc's advertised capabilities.
- `src/ui/status.rs` — observer-badge segment.
- `src/app/mod.rs` — wire TeeBackend at startup; expose subscriber
  count to the status bar.

**Exit:** open two spyc instances. From one, `:counter` then `Space`
on the other's row → a peek panel renders the other's full screen
(scrolling, mode changes, etc.). The other spyc shows `👁 1` in its
status bar.

### Phase 3 — Input forwarding ("take control")

**Goal:** keystrokes from the hub reach the remote spyc as if typed
locally.

**Mechanism:**
- New MCP method `send_input(key_event)` — accepts a serialized
  crossterm `KeyEvent`. Pushed into the remote spyc's event loop
  through a new internal channel so the dispatch path treats it
  identically to local input.
- When the hub attaches to a row (e.g. `^a-1`), CounterTop replaces
  its UI with the mirrored peer view, and local key events are routed
  to `send_input` against that peer's socket instead of being
  consumed by the hub itself.
- Detach with `^a-h` returns to CounterTop. The remote spyc keeps
  running.
- Multi-client: last-keystroke-wins, no lock. The OS's single-focus
  guarantee means in practice only one keyboard is producing events
  at a time.

**Files:**
- `src/mcp.rs` — `send_input` method. Add `input_forward` to every
  fresh spyc's advertised capabilities.
- `src/pane/input.rs` — extend if needed to ferry remote keys into
  the dispatch.
- `src/app/mod.rs` — attach / detach state, key forwarding wire-up.

**Exit:** from the hub, attach to a remote spyc, type `:q` — the
remote spyc quits, and its row disappears from CounterTop.

### Phase 4 — Headless dispatch (`--detached`)

**Goal:** the hub can launch new spyc instances that have no local
terminal — they exist purely as discoverable peers visible through
the hub mirror.

**Mechanism:**
- New flag `spyc --detached /path` — spawn a spyc with a pseudo-pty
  backing its terminal output, no foreground terminal attached.
  Registers in discovery with `headless: true`.
- Hub `^a-c` (new dispatch) opens a prompt: `path: <enter>`. Launches
  `spyc --detached <path>` as a child of the hub (or via a tiny
  supervisor) and immediately subscribes to its frames.
- Headless spycs auto-quit when their hub-side mirror disconnects
  AND no other client is subscribed AND no active pane subprocess
  is running — same idle-cleanup shape Agent View uses for its
  supervisor.

**Files:**
- `src/main.rs` — `--detached` flag handling.
- `src/app/mod.rs` — headless mode (skip claiming the host terminal,
  render to a fixed-size virtual viewport).
- `src/ui/counter_top.rs` — dispatch prompt UX.

**Exit:** `^a-c spyc /tmp/foo` from the hub creates a new row, opens
its mirror, lets you work in it; close the hub → spyc keeps running
detached; reopen the hub → row reappears.

### Phase 5 — Agent View bridge

**Goal:** surface Claude Code's background sessions
(`claude --bg`, `/bg`-detached) inside CounterTop alongside spyc
peers. One HUD covering both spyc-managed and supervisor-managed
work.

**Files:**
- `src/state/agent_view.rs` — reader for `~/.claude/daemon/roster.json`
  and `~/.claude/jobs/<id>/state.json`.
- `src/ui/counter_top.rs` — render a second section labelled
  `── background ──` below the spyc peers, listing supervisor
  sessions with the same shape (working / needs input / done).

**Exit:** `claude --bg "investigate X"` from any shell → that
session shows in CounterTop's background section. Pressing `Enter`
on the row spawns a new detached spyc that opens a pane running
`claude attach <id>`, adopting the background session into a
workspace under spyc.

### Phase 6 — Spice drawer + Pass polish

**Goal:** persistence + at-a-glance aggregate.

**Spice drawer:**
- `src/state/spice_drawer.rs` — save / restore named bundles of
  `(project_home, session_id_or_none, headless_flag)` triples to
  `~/.local/state/spyc/spice_drawer/<name>.json`.
- `:drawer save morning`, `:drawer open morning`, `:drawer list`.
- `spyc --hub --drawer morning` spawns all entries on launch
  (as `--detached` or `--bg` depending on the entry).

**Pass:**
- `src/ui/status.rs` — add a `pass: N` segment counting peers in
  "needs input" / "done" state. Visible in the hub's status bar AND
  optionally in standalone-spyc status bars when more than one peer
  is running (configurable).
- Term-title integration: `🌶️ CounterTop · 3 burners · 1 on the pass`.

**Exit:** glance at the terminal title bar alone → know how many
peers are working and how many want you.

## Out of scope (v1.60)

- **No re-implementing `claude agents`.** Agent View runs as itself
  if you want it. CounterTop only *reads* its state files.
- **No process supervisor.** Headless spycs are owned by whoever
  spawned them (hub or shell), not by a separate daemon. If the
  hub spawns a detached spyc and the hub crashes, the detached
  spyc keeps running until it idles out or the user kills it.
- **No cross-machine.** Discovery is per-user local state. Two
  machines = two CounterTops.
- **No write-locking on input.** Last-keystroke-wins is the chosen
  semantic; the OS does the gating in practice.

## Open questions

- **Resolver chord for CounterTop.** `^a-W` collides with worktree
  (`W l/n/d`). Candidates: `^a-O` (overview), `:counter`, `^a-^a`.
  Decide in Phase 1.
- **Frame encoding bandwidth.** A full 200×60 buffer is ~12000 cells
  × ~16 bytes serialized ≈ 200 KB. At even 10 fps that's 2 MB/s
  per subscriber. Mitigation: tee already writes encoded ANSI deltas,
  not full buffers — delta sizes are usually tiny. If this becomes a
  problem, add a "snapshot every N seconds + delta in between"
  protocol.
- **Hub identity / session name.** Does the hub get a spice-pair
  name like other spycs? Lean **no** — the hub's identity is the
  set of peers, not a project home. Status bar shows
  `🍳 CounterTop · 3 burners`, no session name segment.
- **Reattach after hub quits mid-control.** If the hub crashes while
  attached to a peer, the peer keeps running but loses the badge
  count one tick late. Plan: subscribers send a heartbeat every 2s;
  remote spyc drops stale subscribers after 5s of silence.
- **`--hub` mode quit semantics.** `:q` in the hub should not quit
  any peers — only close the hub. `:Q` reserved for "tell every
  peer to quit too" (rare).

## Naming reference (code ↔ UI)

| UI surface     | Code                | What it is                                           |
| -------------- | ------------------- | ---------------------------------------------------- |
| CounterTop     | `CounterTopView`    | The hub HUD pager / hub-mode main view.              |
| Burner         | row in CounterTop   | One row = one peer spyc instance.                    |
| Pass           | (derived state)     | Aggregate "ready for you" count.                     |
| Spice drawer   | `SpiceDrawer`       | Saved-bundle store.                                  |
| Ticket         | (no struct)         | UI label for a dispatched agent prompt.              |
| 👁 badge       | `MirrorClientCount` | Status-bar subscriber count on the remote spyc.      |
| TeeBackend     | `TeeBackend`        | ratatui backend wrapper that publishes frames.       |
| Subscribe      | `subscribe_frames`  | MCP method to receive a peer's render stream.        |
| Forward input  | `send_input`        | MCP method to inject a key event into a peer.        |
