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
   (or its lifecycle hook does): `working` / `blocked` / `done` / `waiting` /
   `idle`.
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
| calm **teal** square `■` | **done** — finished a turn; one that landed while its tab was out of view stays teal until the tab is in view |
| steady **blue** hollow circle `○` | **waiting** — turn over, but background work the agent started will wake it |
| `💤` | `^z`-suspended |

Semantically reported `blocked` is **latched**: it stays red until you settle the prompt in that pane —
**Enter** to answer it, **Esc** or **`^c`** to dismiss it — or the agent files a
newer report. No timer or stray output bounces it off. The dismissal keys count
because no hook reports one: Claude Code ends a declined turn as a user
interrupt, and `Stop` doesn't fire on an interrupt, so the keystroke is the only
signal spyc gets.

**`waiting`** is neither the user's turn nor finished: the agent ended its turn
while work it started (a CI watch, a background build, a monitor, a scheduled
wakeup) is still pending, and it resumes by itself when that work wakes it. Two
sources set it:

- **Claude Code says so itself.** Its `Stop` hook payload lists in-flight
  `background_tasks` and pending `session_crons`, so the reporter turns that
  turn end's `done` into `waiting` when either list has an entry. The reporter
  keeps only how many entries each list had, never their descriptions or
  commands, and the note reads "1 background task". When the work finishes,
  Claude starts a turn by itself, its `UserPromptSubmit` hook reports `working`,
  and the next `Stop` decides again.
- **The agent reports it**: `report_status(waiting, note)`, for an agent whose
  hooks can't say (codex, agy) or to name what it waits on ("CI on #641"). The
  agent's note outranks the hook's count.

`waiting` is how a turn ends, so an agent's own `waiting` sent while its turn is
still open (starting a background watch, then carrying on) is held: the dot
keeps showing the turn, and the wait takes over when the turn's end arrives,
meeting it exactly as below. The hooks say when a turn opens (`working`) and
ends (`done`, or Claude's `waiting`). A question or Claude's idle nudge changes
neither, and an agent whose hooks report no turn start never has one open, so
its `waiting` applies at once. Any newer report from the agent, or **Enter**,
drops a held wait. `:why-status` shows it as `held`, with its note.

A live `waiting` holds back only a hook's `done`. Claude's idle nudge is never a
turn end. When a hook can't say whether work is pending, as with codex's and
agy's `Stop`, `waiting` absorbs the first `done`, which ends the turn that
reported it. The next one means the agent woke and finished without reporting
again, so it applies. A Claude `Stop` that lists no pending work applies too:
nothing will wake the agent. Every other report replaces `waiting`, as does
**Enter** in the pane, which starts the user's own turn. Esc and `^c` don't
retire it: interrupting an idle agent stops nothing running in the background.
Entering `waiting` fires no notification; the `done` or `blocked` ping fires
when the work actually ends.

**Auto-reporting.** So it works without the agent choosing to call the tool, spyc
installs lifecycle hooks that run `spyc --report-status` (prompt-submit→working,
needs-permission→blocked, turn-end→done, or `waiting` when Claude's `Stop` lists
pending work), once, in each agent's own user
configuration: **claude** (`~/.claude/settings.json`), **codex**
(`$CODEX_HOME/hooks.json`), **agy** (`~/.gemini/config/hooks.json` — working/done
only; agy has no approval event, so its `blocked` comes from spyc reading the
approval prompt off the pane).

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
It asks first — a `[Y/n]` on the first launch of each agent, saved for every
project; change it later with **`:hooks on|on!|off|ask <agent>`**. Only `y`/`n`
answers it: the prompt is raised
after the pane has focus, so any other key defers (saving nothing, asked again
next launch) and goes to the agent, rather than being eaten by a dialogue the
user was not typing at.

**Debug:** **`:why-status`** opens a pager with the active tab's state + source,
then its full derivation; **`:activity dump`** opens one with every pane's, in
every open project. While several are open, each project's tabs are under its
heading (`project web`, the one in view marked `(in view)`), and each MCP
connection names the project it acts on, `(closed)` once that has closed. A
live report's note shows in both, so a `waiting` tab says what it waits on.

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

`request_user_input_async` posts a question and returns immediately; its tool
completion does not establish that the user answered. The
[`0.162.1` async handler](https://github.com/openai/codex/blob/rust-v0.162.1/codex-rs/core/src/tools/handlers/request_user_input_async.rs)
and [question editor](https://github.com/openai/codex/blob/rust-v0.162.1/codex-rs/tui/src/bottom_pane/async_questions/state.rs)
keep posting, local answer delivery, skipping and native resolution separate.

spyc recognizes the default native question editor's complete submit/skip/navigation
footer, or its collapsed positive question count and answer hint, at the current
viewport bottom. The dot reads `blocked` with `awaiting async question answer`;
this means the tab needs you, even while Codex continues working. `:why-status`
and `:activity dump` label the source `scrape-fallback`. The latest semantic report
remains stored and resumes when the native view clears. An independent semantic
block keeps precedence and is not retired by a disappearing async view. Expired
reports are not revived, and another pane's view cannot supply attention.

This follows visible attention, not semantic proof of an answer. Native skipping,
queued answers, follow-up input, turn completion or auto-resolution can clear the
view. Quoted history above the composer, clipped views and customized shortcut
labels produce no guess. The existing dirty-viewport scan supplies the bounded
delay; no new hook, question-content store or idle polling is added. Exact-version
[fixture provenance](../tests/fixtures/codex-questions/README.md) and controlled
PTY tests are separate from native CLI acceptance.

### Duplicate Codex hook sources

Codex loads the hooks of every layer it reads — user and project, `config.toml`
and `hooks.json` — and runs them all, merging no duplicates. spyc's hooks live in
the user `$CODEX_HOME/hooks.json`, so a project `.codex/config.toml` or
`.codex/hooks.json` that a spyc before 2.2 wrote reporters into reports each
event a second time. 2.2 took those copies out; one left behind is the user's to
delete (`INSTALL.md`).

Codex identifies an owned handler by a direct `spyc` invocation (or the
currently resolved reporter executable), a known wire status and the generated
command shape, including its optional trace flag and fail-soft tail. Bare
invocations from older versions are also recognized. Quoted flags, other programs, wrappers, unknown
statuses and extra shell actions are preserved. A non-command handler is never
pruned. This check does not change generated hook commands or trust records.

A running CLI keeps the hook definitions it started with, so a change to them
needs a codex restart, which `:why-status` reports; then review `/hooks`.
Removing a duplicate does not establish single delivery before that restart,
and nothing spyc does approves a hook or infers that a pending question or
permission request was answered.

### How the hooks get written

`src/wiring/` writes them once, into each agent's own user configuration, when
the user says yes to that agent's first-launch `[Y/n]` (or runs `:hooks on
<agent>` / `spyc --install-agents`). All three agents share three properties:

- **Byte-idempotent merge** — spyc splices its entry into the user's existing
  config, keeping their settings and key order. An install that would change
  nothing writes nothing.
- **Refusal over loss** — a file that won't parse is left untouched, a symlink
  is written through, and removal never deletes a file.
- **Reporter ownership** — Claude and agy use the `--report-status` marker.
  Codex additionally requires the direct reporter command shape described above;
  mentioning the flag in another hook does not permit its removal.

For claude the same consent also wraps its `statusLine`, the only place Claude
Code shows its plan's rate limits and context window: `spyc --statusline`
forwards those numbers to the pane's spyc for the agent HUD, then runs the
command the user had, on the same input, so their line is unchanged. With no
statusline configured, spyc's prints nothing. `:hooks off claude` puts the
user's command back exactly; a later `/claude-hud:setup` or similar is wrapped
again at the next startup refresh.

The commands never vary, so claude's collapsing of identical handlers and
agy's of named sets keep a leftover project copy from reporting twice; codex
merges nothing, so a codex copy left behind does report twice. Agents that read
their hooks once at startup (codex, agy) are flagged for a restart when an
install changes them.

### Who owns them

Nobody, in the per-instance sense: the reporter targets whichever socket the
*pane's* env names, so one user-scope file serves every spyc on the machine and
every later session. Nothing is removed at exit, and there is nothing to
refcount. At startup spyc refreshes every wired agent's files, so a moved or
renamed spyc command is caught up; `:hooks off <agent>` is how they come out.

When they *are* missing, `:why-status` and `:activity dump` say so outright
rather than reporting the output-timing fallback as if it were the answer.

---

## 2. Notifications — the ping when an agent needs you

On a tab's transition to **blocked** or **done**, spyc fires a desktop
notification naming the tab, and its project when several are open — so you can
alt-tab away and get pulled back to the *right* pane. Configured under `[notify]` (see
[`CONFIGURATION.md`](../CONFIGURATION.md)):

- **Desktop ping** — on by default, on both blocked/done. Over SSH it routes as
  an OSC-9 terminal escape (reaches your *client* terminal); locally it uses the
  OS notifier.
- **Visual bell** (a spice-heat border pulse) — on by default; **terminal bell**
  — off by default. Rung for another project, it flashes where.

- `suppress_focused_tab` mutes the tab you're already watching (off by default —
  spyc-focused ≠ eyes-on-terminal).

**Seen and unseen (#99).** A tab is in view when it is the active tab of the
project in view and its pane is drawn: not hidden, not zoomed away, not under a
full-frame pager or image. A done that lands on a tab out of view is *unseen*
(`TabInfo::unseen_done`, kept from the first done) until the tab comes into
view, even after the done report decays to idle or the agent exits, and its dot
stays teal meanwhile (`:activity dump` shows `unseen_done=`). A blocked agent's
wait is timed from when it began (`TabInfo::blocked_since`). Neither is saved.

**Each project's roll-up (#99).** While several projects are open, each
project's chip in the status bar and its `Space j` row show its most urgent
agent in the tab dots' glyphs: blocked (red `■`), then a done not yet seen
(teal `■`), then working (`●`), then waiting on its own work (`○`, which never
pulses). A project whose agents are idle, or whose done is seen, shows none,
and a `^z`-suspended agent counts for nothing. The terminal title counts the
tabs blocked in the projects out of view: `🌶️: spyc (■2) · SAFFRON_CUMIN`.

---

## 3. Session persistence & resume

spyc auto-saves your workspace **on quit** *and* re-saves a couple of seconds
after any change (a debounced, crash-sufficient autosave written atomically), so
a `SIGKILL` / crash / laptop-sleep loses at most that window — not the session.

**`spyc -r`** restores: pane tabs (each agent respawned + its *exact* conversation
resumed — claude and agy report their live conversation id as they run, so restore
replays the right one instead of guessing by spawn time, which collides when
several panes come up together), the vertical split, **and the scope registry**
(§4) — coordination survives a restart mid-merge-train. A session another spyc
is still running restores as a copy whose agents start fresh, without that
spyc's scope claims, which still belong to its live agents.

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
  in a wait. While several projects are open, each claim names the project of
  the tab that owns it.
- **`:agent list`** — each agent tab with its activity dot + the scope it owns,
  in every open project, under each project's heading while several are open
  (`(no agents)` under one running only shells).

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
checks every edit to the user `hooks.json` first: a removal that would move a
user handler is refused with the file untouched (`no_codex_hook_ever_moves`),
and spyc's own handler is rewritten where it stands. Removing trailing
reporters is still allowed. spyc does not move or create hook trust records; a
manual edit must include reviewing the affected user hooks in `/hooks` after
restarting Codex.

### Interruption and process exit

A working report survives redraws and silent tool waits until its backstop.
Accepted Escape or `Ctrl-C` input retires that report so cancelled work falls
back to output timing and then idle. Enter, ordinary typing, arrows and pasted
control bytes preserve working reports. An input rejected by the PTY queue
cannot change report authority. Identified pending Codex questions retain their
existing correlated completion/new-report recovery rules.

A closed pane retires live reports, scrape attention and pending question
recovery, and stops using output timing to drive activity. Diagnostics show
`process-exit` as the source and retain the final output timestamp. Its last received report remains in diagnostics as no longer
authoritative, and late reports are rejected rather than reviving the dot.
Non-blocked report TTLs default to five minutes and are capped at five minutes
at dispatch, including direct callers outside the MCP schema. `waiting` defaults
to, and is capped at, an hour: a CI run outlasts five minutes, and the agent
re-reports each time its work wakes it. Blocked remains latched until explicit
recovery while the pane is alive.
