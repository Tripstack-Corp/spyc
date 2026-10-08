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
three tiers, with a narrow Codex approval exception:

1. **Semantic self-report** (best) — the agent calls the `report_status` MCP tool
   (or its lifecycle hook does): `working` / `blocked` / `done` / `idle`.
2. **Scrape fallback** — for a state an agent's hooks can't report, spyc reads
   its *visible screen* for a known prompt: agy's tool approval or Codex's
   complete command, file-edit, MCP-tool and network approval forms. Codex scans
   within 250 ms of the first pending repaint; agy waits for quiet.
   Codex's verified dialogue temporarily overrides a non-blocked report;
   semantic question/agent blocks retain precedence.
3. **Output timing** — with neither of the above, output flowing = `working`,
   silence = `idle`.

Codex and Claude retain semantic reports through output and footer redraws.
Silent tool waits do not erase `working`; the default five-minute TTL remains
a backstop, and newer reports replace older ones. Agy still yields non-blocked
reports to fresh output so their uncovered
approval prompts can be detected. Timing-only `idle` means quiet, not proof
that a turn stopped or finished.

**Glyphs** (shape = liveness, colour = urgency):

| Dot | Meaning |
|-----|---------|
| heat-pulse `●` (pepper→ember) | **working** — output flowing or a `working` report |
| quiet `·` | **idle** |
| steady **red** square `■` | **blocked** — waiting on you ("needs me") |
| calm **teal** square `■` | **done** — finished a turn |
| `💤` | `^z`-suspended |

Semantically reported `blocked` is **latched**: it stays red until you settle the prompt in that pane —
**Enter** to answer it, **Esc** or **`^c`** to dismiss it — or the agent files a
newer report. No timer or stray output bounces it off. The dismissal keys count
because no hook reports one: Claude Code ends a declined turn as a user
interrupt, and `Stop` doesn't fire on an interrupt, so the keystroke is the only
signal spyc gets.

**Auto-reporting.** So it works without the agent choosing to call the tool, spyc
installs lifecycle hooks that run `spyc --report-status` (prompt-submit→working,
needs-permission→blocked, turn-end→done): **claude** (`.claude/settings.json`),
**codex** (`.codex/config.toml`), **agy** (`.agents/hooks.json` — working/done only;
agy has no approval event, so its `blocked` comes from spyc reading the approval
prompt off the pane).

Codex also reports `idle` through its `Interrupt` hook when the main turn is
cancelled. No general tool-completion hook clears `blocked`: a concurrent tool
finishing is not proof that a separate permission/question prompt was answered.

agy's `done` needs **agy >= 1.1.10**: earlier versions evaluated `hooks.json` after
their own termination checks, so the `Stop` handler spyc installs was unreachable
and that half silently degraded to output timing.

agy waits on the user in **two** different ways, and they need different instruments:

| agy waits via | signal | why |
|---|---|---|
| its `ask_question` tool | `PreToolUse` hook, matcher `ask_question` | it's a real tool, so a hook sees it exactly when it's called |
| its tool-permission prompt (`Requesting permission for:`) | screen scrape | no event fires for it, in any agy version |

`PreToolUse` cannot be widened to cover the second. A handler must answer with a
`decision` (`allow`/`deny`/`ask`/`force_ask`), so matching every tool would put spyc
in charge of agy's permissions; scoped to `ask_question` the `allow` is a no-op,
because that tool was always going to wait for you. Omitting the answer is worse
than either: agy re-drives the tool, which ran a probe turn to 17 invocations before
it timed out, never reaching `Stop`. spyc's handler therefore prints its decision
unconditionally and discards the reporter's own output to keep stdout pure JSON.
It asks first — a `[Y/n]` on the first launch per repo, saved; change it later
with **`:hooks on|on!|off`**. Only `y`/`n` answers it: the prompt is raised
after the pane has focus, so any other key defers (saving nothing, asked again
next launch) and goes to the agent, rather than being eaten by a dialogue the
user was not typing at.

**Debug:** **`:why-status`** flashes the active tab's state + source; **`:activity
dump`** opens a pager with every pane's derivation.

The dump retains the last received semantic report after expiry or dismissal;
its source can be a hook or the agent itself. Hook diagnostics separate the
reporter marker found in a file, presence before launch, and known post-launch
config changes requiring a restart. These facts do **not** establish loading,
execution or trust. Codex needs project trust and review of new/changed hooks
in `/hooks`; spyc neither approves them nor edits trust records. See the
[official Codex hooks documentation](https://learn.chatgpt.com/docs/hooks).

`:activity dump` also retains up to eight bounded hook-event summaries per
pane, oldest first, after status expiry or dismissal. A restart clears them.
The reporter forwards only event/tool names, notification subtype and available
turn/call ids; it excludes arguments, prompts, response text and tool output.
Control characters, malformed identifiers and oversized fields are discarded.
`--status-trace` logs this sanitized metadata instead of the raw stdin payload.
The reporter streams the complete hook JSON and retains only a fixed set of
root metadata fields; a large patch, heredoc or answer does not erase later ids.
The 8 KiB limit applies to normalized metadata, not raw stdin. String capture is
bounded and nesting is limited to 128 levels; malformed documents produce no
partial metadata. Arguments and responses are skipped without being retained.
This is claimed metadata, not authenticated hook provenance. Missing metadata
can mean an older reporter or an absent/malformed payload, not a missing hook.

The current official hook contract supplies `tool_use_id` for `PreToolUse` and
`PostToolUse`, but does not list it for `PermissionRequest`. An absent id stays
absent. A generic tool completion cannot establish which permission/question
was answered. No broad tool-completion hook is added; narrowly correlated
question recovery is described below.

### Codex question-tool recovery

Codex's `request_user_input` tool has narrowly matched `PreToolUse` and
`PostToolUse` reporters. A start reports `blocked`; successful completion
restores `working` only when pane, session, turn and call match a pending
question. The first valid correlated start supersedes a preceding generic
agent block, such as the agent's announcement that it is about to ask. A generic
block received while any question is pending remains independent and retains
attention after completion; duplicate or additional starts cannot clear it.
Enter alone does not clear an identified question, and unrelated or late
completions are recorded as unapplied with a reason in `:activity dump`.

Correlation is bounded to eight pending questions per pane, with overflow
remaining blocked until a lifecycle or explicit agent report. It is reset for
replaced panes and never stores question arguments or answers. The hook wire
values require a capable host and metadata-bearing reporter; older components
cannot silently apply an uncorrelated completion. Restart Codex and review both
new hooks in `/hooks`; spyc never approves them or edits trust records.

This instruments the question tool's attempt/completion, not authenticated
proof that a dialogue opened. Invalid/cancelled calls may have no post event;
`Stop`, `Interrupt` or an explicit newer report retires the wait. Correlated semantic
permission completion cannot be correlated because `PermissionRequest` has
no call id. It runs before hook decisions, automatic review or a human dialogue,
as shown by the exact-version
[permission event](https://github.com/openai/codex/blob/rust-v0.160.1/codex-rs/hooks/src/events/permission_request.rs)
and [review path](https://github.com/openai/codex/blob/rust-v0.160.1/codex-rs/core/src/tools/approvals.rs).
Metadata-bearing permission reports are recorded as observational
(`not applied: PermissionRequest precedes review; human wait unverified`). They
cannot latch blocked or interfere with a later question. The existing hook
command remains unchanged, preserving its trust hash; the host and reporter
on `PATH` must both support lifecycle metadata.

Command, file-edit, MCP-tool and network approvals use a narrow fallback:
known required phrases and a complete default footer at the current viewport
bottom temporarily override a non-blocked report. Native word wrapping is
accepted; missing required text, changed forms and old dialogues above the
composer are not inferred. Codex scans within 250 ms of the first pending
repaint, so continuous redraws cannot postpone detection indefinitely.
`:why-status` and `:activity dump` identify `scrape-fallback` as the source.
The silent-work report remains stored and resumes after the dialogue closes.
This is UI detection, not semantic permission completion. Network coverage uses
an exact-version upstream UI fixture; a live native network approval remains
an acceptance case. Generic MCP elicitation, extra-permission and stdin-write
approval forms remain uncovered. Explicit agent blocks retain their ordinary input
recovery; identified questions require matching completion instead of Enter.
Real-CLI native question and quiet-after-answer checks passed through the
automated TUI harness with already trusted hooks. Fresh hook trust onboarding
remains a separate acceptance case; see `docs/HARNESS.md` for the driver.

`request_user_input_async` is a separate question path. Its immediate completion
acknowledges posting a question; it does not establish that the user answered.
These native question hooks do not instrument that path. The
[`0.160.1` async handler](https://github.com/openai/codex/blob/rust-v0.160.1/codex-rs/core/src/tools/handlers/request_user_input_async.rs)
posts an async message and immediately returns `accepted`; the
[TUI reply parser](https://github.com/openai/codex/blob/rust-v0.160.1/codex-rs/tui/src/async_question_reply.rs)
handles the answer as a later user message. A blocked latch tied to the tool's
completion would misrepresent an agent that continues working. Async-answer
lifecycle coverage remains open in A3.

### Duplicate Codex hook sources

Codex loads `.codex/hooks.json` alongside inline hooks in `.codex/config.toml`.
If both contain spyc reporters, each matching event can report twice. At
installation, spyc migrates its `--report-status` handlers out of the JSON file
and keeps its reporters in TOML. Other handlers, matcher fields and trust records
are preserved. An emptied legacy file is removed; user-only files are not
rewritten. `:hooks off` and last-owner cleanup also remove legacy reporters.

Legacy files containing tracked reporters, or malformed/unreadable files,
block migration; `:activity dump`
names the source and repair action. Use `:hooks on` to migrate a valid untracked
file, then restart Codex and review `/hooks`. A running CLI may retain its old
hook definitions, so removing a duplicate file does not establish single
delivery before restart. A JSON-only change also sets the restart diagnostic.
Migration does not approve hooks or infer that a pending question or permission
request was answered.

### Codex linked-worktree hook sources

Codex loads project hooks from the corresponding directory in the root checkout,
even when launched in a linked worktree. For example, `repo.worktrees/fix/src/`
uses `repo/src/.codex/config.toml` and `repo/src/.codex/hooks.json`. Ordinary
project config and spyc's MCP entry remain worktree-local. Worktree-local hook
declarations are ignored; their reporter marker cannot establish readiness.

spyc resolves that source for installation, consent, diagnostics, re-healing and
shared ownership. Consent belongs to the actual source's project root; a prior
worktree-only grant cannot authorize a write in the root checkout. The consent
popup names the file, and linked-worktree diagnostics show its full path. A
source change marks all affected Codex tabs as needing restart. Review `/hooks`
in the fresh CLI; definition presence still does not establish execution/trust.

### How the hooks get written

Each agent's hook writer/cleaner pair (`mcp::{ensure,cleanup}_{codex,agy}_status_hooks`)
mirrors the claude pair, and all three share the same three properties:

- **Byte-idempotent merge** — spyc splices its entry into the user's existing
  config rather than rewriting the file. Re-running produces identical bytes,
  so a repeated launch never churns the file.
- **Git-tracked guard** — spyc refuses to modify a config the repo tracks.
  These files are the user's (`.codex/config.toml` carries their `model` and
  `approval_policy`), and silently rewriting a committed one is data loss.
- **`--report-status` is the "ours" marker** — cleanup removes only entries
  invoking spyc's own reporter, so a hand-written hook of the user's survives
  teardown.

They all ride the same `mcp_config_dirs` teardown path. Non-live-reload agents
(codex, agy) additionally get their hooks written *pre-spawn* for an
already-consented repo via `maybe_preinstall_startup_hooks`, because they read
their config once at startup and would otherwise miss them until next launch.

### Who owns them (why one spyc's exit can't blind another)

The installed hooks are **shared, not per-instance**: the reporter they invoke
targets whichever socket the *pane's* env names, so one `.claude/settings.json`
serves every spyc on the machine. Installing is therefore safe to repeat — but
removing is not. Until this was refcounted, a second spyc quitting deleted the
hooks out from under the first one's live panes, and every dot there fell back
to output timing for the rest of that session, silently. Two things close it:

- **`state/hook_owners.rs`** — `{dir: [pid, ...]}` in the state dir, pruned by
  liveness on each write (a `SIGKILL`ed spyc never releases). Teardown removes
  the hooks only when `release` reports no live owner left. An explicit
  `:hooks off` still removes them regardless — that's the user revoking consent
  for the project, and consent is what every instance's re-heal consults.
- **`app/status_hooks.rs`** — `settle_status_hooks` re-installs, at loop bottom
  behind a 30s throttle, whenever a consented pane's config has lost the hooks
  by any other route (`git clean -xfd`, a hand edit, an older spyc). It arms no
  deadline, so idle stays 0 dps.

When they *are* missing, `:why-status` and `:activity dump` say so outright
rather than reporting the output-timing fallback as if it were the answer.

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
resumed — claude and agy report their live conversation id as they run, so restore
replays the right one instead of guessing by spawn time, which collides when
several panes come up together), the vertical split, **and the scope registry**
(§4) — coordination survives a restart mid-merge-train.

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

### Preserve Codex user hook positions

Codex's persisted hook key includes the source, event, matcher-group index and
handler index. Removing a reporter ahead of a user handler changes the trust
lookup even when that handler's command hash is identical. spyc therefore
preflights inline TOML and legacy JSON pruning. If a user handler would move,
installation and cleanup preserve both sources and explain the refusal in
`:activity dump`. Removing trailing reporters is still allowed. spyc does not
move or create hook trust records; a manual migration must include reviewing the
affected user hooks in `/hooks` after restarting Codex.
