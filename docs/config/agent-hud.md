# Agent HUD — `[pane] agent_hud`

Part of [Configuring spyc](../../CONFIGURATION.md).

The native agent strip, for Codex, Claude Code, and Antigravity tabs, is **off
by default**. Set `agent_hud = "compact"` or `"full"` in `~/.spycrc.toml`, or
for one project in its own file under `~/.config/spyc/projects/`; a repo's
`.spycrc.toml` values are ignored. `:agent hud off|compact|full` chooses for the
session: the choice outranks the file, `spyc -r` brings it back, and a config
reload keeps it unless the reload changes `agent_hud` itself. `:agent hud info`
(or `dump`, or a double-click on the strip) names which one is in force and
opens saveable source diagnostics, with every tool's tally.

Compact uses two rows below the child: the activity row and the usage row. Full
adds the tools row and the mode row, for four. Every agent shares one layout in
aligned columns, described in [`DESIGN.md`](../../DESIGN.md) under "Native agent
strip". The child keeps at least six rows: full collapses to compact below ten
available pane rows, and the strip hides below eight. It follows the active live
Codex, Claude Code, or Antigravity tab, including pane zoom, vertical splits and
scrollback. Hidden panes, closed tabs, other agents and a fullscreen image have
no strip or metric subscription. The native composer and status line stay inside
the resized child.

The activity row is the tab's own dot and state, the session's UUID suffix, the
model and effort, the repository and known branch, and elapsed time, with what a
`waiting` agent waits on at its end. For a Codex tab, `req`, `turn` and `thread`
are separately reported token totals; legacy totals remain labelled and are
never added to current totals. A reported zero is different from `--`. A
`context` of `--` means current occupancy is unknown; a reported model window
and lifetime usage cannot establish a percentage. Limits may be not reported or
unsupported. A Codex tab's tools row reads `recent`: its observations are
partial, never a count of active tools or agents, and none is shown as running.
Its mode row is the approval policy and the sandbox. `hooks ✓` means events
arrived; otherwise it reads `unverified`, `restart` or `off`. `mcp ✓` counts
observed bound connections.

Collection reads the pinned rollout under the pane's captured Codex home.
Startup filename lookup is bounded and requires one matching file; missing,
ambiguous or out-of-budget lookups do not retry at idle. Toggle the strip to
retry. An exact-file watch then incrementally reads bounded complete records.
No account request or additional Codex session is opened. Samples older than
60 seconds are labelled stale; finite expiry edges can wake once. Elapsed time
updates on other events and existing activity animation, so an idle strip has
no recurring clock. Diagnostics contain source, identity, freshness and bounded
metric observations, with no prompts, reasoning text, tool arguments or output.

For a Claude Code tab, `context` is what its latest request sent, in tokens:
input plus prompt-cache reads and writes; after a compaction it reads `--` until
the next request. The transcript has no window size, so a share of the window
comes only from Claude Code itself: with spyc's statusline wrapper in place
(below), `context` is followed by Claude Code's own percentage for this
session, then `5h` and `7d`, the plan's five-hour and seven-day rate-limit
windows. Those are the account's, so every Claude tab shows the newest any tab
reported. Each window is followed by when it resets, in your local time:
`↻3:40pm` today, `↻Thu 9am` later this week, `↻Oct 22 9am` beyond that.
`agent_hud_resets = "countdown"` shows the time left instead (`↻2h05m`,
`↻3d4h`), at the cost of a redraw each time the countdown ticks over: once a
minute within a day of the reset, once an hour before that. Clock time needs
none, beyond one when a window resets. A window whose reset has passed reads
`--`, since its share belongs to the window before, until Claude Code reports
again. A share is green below 50%,
amber below 80% and red from there; nothing is shown before a report, or for an
API-key session, which has no plan windows. `out` is the output of every distinct message since the read
began, `hit` the share of input read from the prompt cache, and `compacted` the
compactions seen; all three survive a compaction. When a long transcript was
read from part-way through, these counts carry `≥` and the tools row reads
`recent`. The tools row counts every call and every failure, names what is
running with `◐`, and lists the three most called, each marked `✓` or `✗<n>` by
its own failures; the dump lists every tool, then the most recent 64 calls with
their times and outcomes. The mode row counts subagent (`Agent`) and background
calls, then, once `TodoWrite` or `TaskCreate`/`TaskUpdate` has reported one,
`todo`: completed over the total, with how many are in progress in brackets
when any are. The two tools share one three-state list; nothing is shown until
one has been used. Claude Code's own footer already shows its permission mode.
Only spyc's own MCP server keeps its name in a tool label, as `spyc:`; other
servers' tools read `mcp`.

Claude Code hands those figures only to its `statusLine` command, so when claude
is wired (the first-launch `[Y/n]`, or `:hooks on claude`), spyc wraps that
command: `spyc --statusline` forwards the window size, the shares and the reset
times to its pane's spyc, then runs the command you had on the same input, so
your statusline looks as it did. With no statusline configured, spyc's prints
nothing. `:hooks off claude` puts your command back exactly, and a statusline
set later is wrapped at the next startup. Each run costs a few milliseconds and
happens only when Claude Code redraws its line, never at idle unless your
`statusLine` sets a `refreshInterval`.

The strip reads `projects/*/<session>.jsonl` in Claude Code's configuration
directory, `$CLAUDE_CONFIG_DIR` or else `~/.claude`, as spyc's own environment
names it at startup. The session is the one the tab's status hooks reported, so
the strip waits for the first report. Claude Code files a session under the
directory it started in, so every project is asked for that exact name, and a
session found in two projects is not guessed between. Claude Code reports its
session at startup but writes the transcript only with the first prompt, so a
transcript not found yet is looked up again on the tab's next status report.

For an Antigravity (`agy`) tab, `context` is the input plus prompt-cache-read
tokens from the latest request, or `--` after a compaction until the next
request; `out` is the output of every distinct planner response since the read
began, `hit` the share of input read from the prompt cache, and `compacted` the
compactions seen. When a long transcript was read from part-way through, these
counts carry `≥` and the tools row reads `recent`. The tools row counts every
call and failure, names what is running with `◐`, and lists the three most
called, each marked `✓` or `✗<n>` by its own failures; spyc's own MCP calls
keep `spyc:`, while other `call_mcp_tool` servers read `mcp`. The mode row
counts subagent (`invoke_subagent`) calls as `agents`, followed by `hooks` and
`mcp`. While the agent is `working` and a planner response in the current
turn has recorded reasoning, that row reads `thinking` and the reasoning's
latest line instead, cut from the left with `…` so the newest words stay in
view. The mode row comes back when the turn settles; a new prompt or a
compaction clears the line. The strip reads
`~/.gemini/antigravity-cli/brain/<session>/.system_generated/logs/transcript.jsonl`
under `$HOME` as spyc's environment names it at startup, using the conversation
UUID reported by the tab's status hooks or named by `--conversation`.
