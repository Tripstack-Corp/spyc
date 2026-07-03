# spyc v1.70 — Mise en Place

> **Archived (2026-07-02).** Design-history, not an active plan — named for a
> version (1.70) the project shipped past (now 1.97+) via other work. Live
> summary: `ROADMAP.md` → "Post-2.0 (2.x) — the structural arc". The MCP socket
> already delivers this thesis's *value* informally (`SPYC_PANE_ID` stable
> handles, `get_spyc_context` read+drive, `wait_for_scope_clear` observed wait);
> the unbuilt part is the *typed* protocol + crate split + SDK — revisit only if
> external drive (an SDK / CLI harness) becomes a real ask.

**Status:** archived design (superseded in spirit by the MCP socket; see banner).
**Predecessor:** [`V1_60_PLAN.md`](V1_60_PLAN.md) (CounterTop hub —
peer spycs discover each other, hub aggregates state).
**Inspiration:** [rmux](https://github.com/helvesec/rmux) and its
[HN launch](https://news.ycombinator.com/item?id=48219918). rmux is a
Rust-rewrite of tmux pitched at agentic terminal use; the
differentiators we want to absorb are the typed daemon protocol +
embeddable widget + structured snapshots, not the tmux compatibility.

## Thesis

Today an external process can read spyc's state via the MCP socket
(`get_spyc_context`, `navigate_to`, `pick_files`, etc.), but it can't
*drive* spyc the way a Playwright script drives a browser. Claude in
the lower pane navigates by typing text into a pty and hoping the
right keystrokes land in the right window. Skills and external
harnesses fall back to scraping the visible terminal because they have
no structured handle on what spyc is showing.

v1.70 makes spyc **programmatically addressable**. Every pane,
selection, and pager view becomes a named target with a structured
snapshot. External clients — the SDK, the CLI, a skill, a sibling
spyc — issue typed *orders* against those targets and wait on typed
*bells* ("the prompt is ready", "the search returned matches",
"exit code 0") instead of timer-based heuristics. The MCP socket
that V1_60 used informally for peer discovery becomes a formal typed
surface in V1_70; an in-process Rust SDK and a `spyc` CLI subcommand
both consume the same protocol.

The architectural choice is **one protocol, three clients**. Same
protocol the in-process SDK uses (for embedding spyc in other TUIs
and for spyc-to-spyc), the external CLI uses (for shell scripts and
coding harnesses), and the MCP server already exposes (for Claude /
Codex). Renaming and typing what's already there, not building a
parallel system.

## Kitchen vocabulary

| Term         | Refers to                                                       |
| ------------ | --------------------------------------------------------------- |
| Mise en Place | The overall plan — structured, addressable, ready state.      |
| Station      | A stable handle to one pane, picker, or pager view. Survives layout changes, tab moves, and re-opens within a session. |
| Plate        | The structured snapshot of a station: cwd, selection, mode, scroll position, last command exit, agent kind, prompt-ready flag. Inspection primitive — clients read plates, not raw terminal text. |
| Order        | An incoming typed command from a client (CLI, SDK, MCP). `chdir`, `send-keys`, `subscribe-plates`, etc. |
| Bell         | An async "ready" signal a client waits on: `wait_for_text`, `wait_for_state(prompt_ready)`, `wait_for_exit`. Replaces timer-based heuristics. |
| Tasting      | The embeddable Ratatui widget — another TUI hosts a spyc station, sampling its plate stream. |
| Recipe path  | Predictable config search order so per-host and per-project overrides are obvious. |

UI labels stay neutral where they already exist; new surfaces use the
kitchen names. Code structs use neutral names (`StationId`,
`PlateSnapshot`, `Bell`, etc.) — kitchen vocabulary is a UI / docs
convention, not a typing scheme.

## Architectural choice (decided)

Three public surfaces share one daemon protocol:

- **CLI** — `spyc send-keys -t main:1 'cargo test\r'`, `spyc plate -t main:1`.
  Subcommands of the existing `spyc` binary, shell-scriptable.
- **SDK** — `spyc-sdk` crate. Typed async Rust API; same wire format
  as the CLI; embeddable in other Rust TUIs and in test harnesses.
- **MCP** — the existing socket server. Becomes a thin wrapper over
  the daemon protocol; tool names stay the same for compatibility.

All three speak to the *spyc daemon*, which is just the running spyc
process itself — there's no separate `spycd`. The existing MCP socket
server is renamed to the **Order rail** and extended to handle all
order types, not just MCP-flavored reads. Peer subscription
(V1_60's `subscribe_frames`) becomes one order shape among many.

Crate split happens *before* the protocol work, not after — it's the
seam that lets the SDK consume the same types the daemon does without
pulling the whole binary in. Same single-responsibility shape rmux
uses: `spyc-proto` (wire types), `spyc-os` (platform), `spyc-pty`
(pty handling), `spyc-ipc` (socket transport), `spyc-render-core`
(ratatui widget bits), `spyc-core` (state machine), `spyc-server`
(the daemon glue), `spyc-cli`, `spyc-sdk`.

## Phases

### Phase 1 — Stations: stable handles for panes, pickers, pagers

Today `pane_tabs` indexes tabs by position. Closing tab 0 shifts the
others. A station ID is a `u32` minted on creation and never reused
within the process lifetime; it lives on `TabEntry` and on the active
pager view. The status-bar / hub already shows tab labels; stations
add a parallel addressing scheme that external clients use.

- Add `StationId(u32)` with monotonic minting on `PaneTabs::push`,
  `App::set_pager`, etc.
- Add `App::station(id) -> Option<StationRef>` returning a borrow
  over (tab | pager | scroll view).
- Keep tab indexes working for keyboard UI; stations are the
  external-client identity.

### Phase 2 — Plates: structured snapshots

A `PlateSnapshot` for each station kind. For pane tabs:
`{ cwd, label, command, agent_kind, prompt_ready, last_exit,
scroll_offset, alt_screen, vt100_cursor_visible }`. For pagers:
`{ source_path, scroll, lines_total, mode, search_query }`. For the
file list: `{ listing_dir, cursor_path, picks, inventory_count,
filter, git_branch, git_dirty }`.

Plates are computed on demand from existing state; no new
event-source machinery. The `last_exit` field comes from the existing
`TaskStatus`; `prompt_ready` is a new heuristic on top of the
agent-pane snapshot (look for the `>` prompt glyph or vt100 cursor at
column 1 of the prompt row — same logic the resume-Enter fix
inferred indirectly via timing).

### Phase 3 — Crate split

Pre-requisite for the SDK. Move types and helpers into focused crates
behind the same `spyc` binary build:

- `spyc-proto` — `Order`, `Plate`, `Bell`, `StationId`, wire codec.
- `spyc-os` — platform abstraction (signal handling, terminal modes,
  cwd lookup). Isolates `unsafe`/platform code so the
  `feedback_avoid_unsafe` memory keeps its meaning.
- `spyc-pty` — pty host, reader thread, drain loop.
- `spyc-ipc` — Unix socket / Named Pipe transport.
- `spyc-render-core` — ratatui widget primitives (the pane widget
  for use in Phase 6).
- `spyc-core` — state machine (current `app::state`, listing, etc.).
- `spyc-server` — current `app::mod.rs` glue.
- `spyc-cli` — clap entry, subcommands.

Workspace builds the same `spyc` binary. Crate boundaries are
single-responsibility; no public API surface yet beyond `spyc-proto`
+ `spyc-sdk`.

### Phase 4 — Order rail: typed daemon protocol

Rename the MCP socket server to the **Order rail**; keep the existing
endpoint path (`~/.local/state/spyc/mcp-PID.sock`) so client
compatibility stays. Define `Order` as a Rust enum encoded as
length-prefixed JSON (matches existing MCP tool-call shape):

- `Order::ListStations` → `Vec<PlateSnapshot>`
- `Order::Plate(StationId)` → `PlateSnapshot`
- `Order::SubscribePlates(StationId)` → stream of `PlateDelta`
- `Order::Navigate(path)` → ack
- `Order::SendKeys(StationId, Vec<KeyEvent>)` → ack
- `Order::SendBytes(StationId, Vec<u8>)` → ack
- `Order::Pick { path, ... }` → existing MCP semantics, typed
- `Order::WaitForBell(StationId, BellSpec)` → blocks until matched

Existing MCP tool names alias onto orders. The MCP server becomes
~150 lines of "translate JSON-RPC method name to `Order` variant".

### Phase 5 — Bells: async waits

The replacement for "spawn process, sleep 1500 ms, send Enter, hope
it landed" (literally the v1.50.54 fix). Bells are typed and
matched server-side:

- `BellSpec::TextAppears { station, regex, timeout }` — pane scrollback contains the pattern.
- `BellSpec::PromptReady { station, timeout }` — plate's `prompt_ready` transitions to true.
- `BellSpec::Exited { station, expected_code, timeout }` — task / capture finishes.
- `BellSpec::CursorAt { station, row, col, timeout }` — vt100 cursor lands at a position (terminal-app tests).

Client APIs:

```rust
let sid = client.list_stations().await?[0].id;
client.send_keys(sid, "/resume abc123").await?;
client.wait_for(sid, BellSpec::PromptReady { timeout: 3s }).await?;
client.send_keys(sid, "\r").await?;
```

This is the same shape as the `pending_resume_send` two-phase fix,
but the wait condition is *observed* rather than timer-based. Once
this lands, retrofit the restore path to use a bell internally —
deletes the `RESTORE_BANNER_SETTLE` / `RESTORE_RESUME_ENTER_DELAY`
constants.

### Phase 6 — CLI subcommands + SDK + Tasting widget

Three deliverables off the same protocol:

- **CLI subcommands** — `spyc send-keys`, `spyc plate`, `spyc wait`,
  `spyc list-stations`, `spyc subscribe-plates`. Auto-attach to the
  spyc daemon for the current `$SPYC_SESSION` (env var set in each
  pane child) or accept `-S <session-name>`. Coding-harness facing:
  this is what HN repeatedly asked for as the missing piece in
  competitors.

- **`spyc-sdk` crate** — typed async client. Used by skills, by
  spyc-in-spyc, by integration tests, and by V1_60's hub for peer
  control. Same `Order` / `Plate` / `Bell` types as the daemon.

- **Tasting widget** — `spyc_render_core::PaneStationWidget`. Other
  ratatui apps host a live spyc station; ticks subscribe to plate
  deltas and re-render. Lets the CounterTop hub render burner peers
  inline instead of mirroring frames (which is what V1_60 Phase 4
  reaches for). Lower bandwidth than the frame-tee approach.

### Phase 7 — Recipe path + Windows prep

Smaller hygiene work that fits the v1.70 envelope.

- **Recipe path.** Adopt rmux's config search: `$XDG_CONFIG_HOME/spyc/spyc.conf` →
  `~/.config/spyc/spyc.conf` → `~/.spycrc.toml` (existing) →
  `/etc/spyc.conf`. Predictable host-level overrides.
- **Windows prep.** `spyc-os` and `spyc-pty` get a Windows
  implementation behind `#[cfg]`. ConPTY + Named Pipes per rmux's
  approach. Not a v1.70 ship — the crate split + `unsafe`
  isolation makes a v1.7x follow-up feasible.

## Out of scope

- Native Windows GA. v1.70 isolates platform code; native Windows
  build follows in a v1.7x point release once we have someone with
  a Windows box to test against.
- `tmux` command compatibility. We're not chasing rmux's 90-command
  tmux surface; we have our own bindings.
- Headless / daemon-only mode (no TUI). The spyc daemon is the
  running spyc process; running spyc without a TUI is not a v1.70
  use case. If it becomes one, it's a v1.8x feature.

## Risks

- **Scope creep.** The crate split alone is a couple weeks of
  mechanical refactor. The temptation will be to "fix things while
  we're in there" — resist; v1.70 is structural reorganization, not
  feature work. Bugs caught during the split land as separate PRs.
- **MCP protocol churn.** External agents (Claude Code) already
  consume the MCP tool surface. The Order rail must keep the
  existing tool names and shapes working through v1.70; new orders
  are additive. Don't rename `get_spyc_context` etc.
- **Plate cost.** Computing a full plate on every Order is fine;
  computing one every tick for every subscribed client is not.
  `SubscribePlates` returns deltas, not full plates; the server
  caches the last full plate per station.
- **Stability of `StationId`.** Once external scripts reference
  station IDs, they need to survive. Within a session: monotonic,
  never reused. Across sessions: not stable — session save records
  *labels*, not station IDs. Tools that need cross-session identity
  use `cwd + command` (same key `load_sessions` already dedups on).

## Naming reference (code ↔ UI)

| UI surface     | Code                    | What it is                                             |
| -------------- | ----------------------- | ------------------------------------------------------ |
| Mise en Place  | (no struct)             | The plan name; UI doesn't surface it.                  |
| Station        | `StationId(u32)`        | Stable handle for one addressable target.              |
| Plate          | `PlateSnapshot`         | Structured state snapshot returned by `Order::Plate`.  |
| Order          | `Order` enum            | Wire-typed command on the daemon protocol.             |
| Bell           | `BellSpec` enum         | Async ready-condition matched server-side.             |
| Tasting widget | `PaneStationWidget`     | Embeddable ratatui widget over a live station.         |
| Order rail     | `OrderRail` / socket    | The local-socket server. Renamed from MCP socket.      |
| Recipe path    | (no struct)             | The config search order convention.                    |

## Relation to v1.60

V1_60 introduced peer-spyc discovery and the hub. It used the MCP
socket informally for `subscribe_frames` and `send_input`. V1_70
formalizes that socket as the **Order rail** with a typed protocol —
so the hub's peer control becomes one client of the same surface that
skills, the CLI, and external scripts use. If both ship, V1_60's hub
*should* be refactored onto the Order rail during V1_70 Phase 4.

If they ship out of order: V1_70 can land before V1_60, in which
case V1_60's hub builds directly on the typed protocol. V1_60 before
V1_70: the hub uses ad-hoc MCP shapes initially; V1_70 reworks them.
The smaller delta is **V1_70 first** — the typed protocol is the
foundation for everything else and absorbs less rework if it lands
ahead.
