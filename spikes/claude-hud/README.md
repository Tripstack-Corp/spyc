# Claude Code HUD: source spike

The source spike for [#648](https://github.com/Tripstack-Corp/spyc/issues/648),
the Claude Code tab's agent HUD: which of the HUD's fields Claude Code's own
session data carries, observed rather than assumed. It is the counterpart of
the Codex spike in [`../codex-hud/`](../codex-hud/), and like it, it adds no
configuration, collection or strip.

`probe.py` reads one explicitly named transcript, at most a 16 MiB tail of it,
and prints only counts, token numbers and short labels. It never searches
`~/.claude`, never watches, and drops prompts, responses, reasoning, tool
arguments and results, paths, ids and the names of MCP servers other than
spyc's. [`observation.json`](observation.json) is its output for one transcript
from Claude Code `2.1.291`, observed 2026-10-13: 591 assistant records over
nine hours, one auto-compaction, four background shell commands. The hook
fields below come from the hook-input schema compiled into Claude Code
`2.1.296`.

```sh
python3 -I spikes/claude-hud/probe.py TRANSCRIPT --session SESSION_ID
python3 -m unittest discover -s spikes/claude-hud -p test_probe.py
```

## What each field comes from

| Field | Observed source | What the reducer must do |
|---|---|---|
| Model | `message.model` on every assistant record (`claude-opus-5-5`). `requestedModel` sits beside it from `2.1.296` but not in `2.1.291`. | Follow the latest assistant record, so a mid-session `/model` shows. Treat `requestedModel` as optional. |
| Effort | `effort` on every assistant record (`xhigh`), and `effort.level` in every tool-context hook payload. | Latest record. |
| Tokens | `message.usage`: `input_tokens`, `cache_creation_input_tokens`, `cache_read_input_tokens`, `output_tokens`. A message is written once per content block (`apiBlockIndex`): 192 of 246 messages appear more than once, always with identical usage. | Deduplicate by `message.id` before summing, or totals inflate by about 2.4×. Session totals are a sum over distinct messages. |
| Context occupancy | The latest message's input + cache creation + cache read: 441,725 tokens, just before the compaction, which no request followed. The boundary's `preTokens` (442,230) is that sum plus the message's 505 output tokens, so Claude Code's own count agrees with the arithmetic. The transcript has no window size, and a 1M window would not have compacted at 442k. | Forget the size at a compaction until the next request. Show `Context --` until a percentage is checked against `/context`: the window size lives in the statusline's input, which is the user's, not spyc's. |
| Permission mode | A `permission-mode` record per turn (`auto`), and `permission_mode` in every hook payload. | Latest record; display only. |
| Tools | `tool_use` blocks by name, with their results in the following user record. | Bounded counts. Collapse MCP tool names, which can name private servers. |
| Plan | `TodoWrite`'s `todos[].status`. `2.1.296` also ships `TaskCreate`, `TaskUpdate` and `TaskList`. Neither appears in this transcript. | Unverified: omit the row until a transcript with one of them is probed. |
| Subagents | The `Agent` tool's `subagent_type`, and `isSidechain` records. None in this transcript. | Count `Agent` calls, never parallel tool calls. |
| Background work | `run_in_background` on a `Bash` call (4), with `backgroundTaskId` in its result. At a turn end, the `Stop` hook's `background_tasks` and `session_crons` lists. | The tab's `waiting` state comes from the hook (#645). The transcript only adds counts. |
| Compaction | A `system` record of subtype `compact_boundary`, whose `compactMetadata` carries `trigger` (`auto`) and `preTokens`, then one `isCompactSummary` user record. | Invalidate occupancy at the boundary; keep lifetime totals. |
| Turn timing | `system` records of subtype `turn_duration` (9) and `stop_hook_summary` (9). | Possible source for elapsed time; not yet inspected. |

`src/agent/claude_metrics/` is the reducer these rules describe, with a reader
and watch that follow a live transcript. Its example driver folds the same
transcript to the same 246 messages and four totals as the probe, an independent
implementation, and reports the context as unknown, since the compaction came
last:

```sh
cargo run --example claude_metrics -- CLAUDE_HOME SESSION_ID TRANSCRIPT
```

Not HUD sources, though they look like it:

- `total_tokens_reminder` attachments carry the agent's own token budget, not
  context usage.
- `ai-title` records are the session's generated title: conversation content,
  so the HUD shows the session id instead.
- `cost-state` records exist; cost is out of scope (#72).

## Open before a percentage

- The context window for `claude-opus-5-5` in this harness. Auto-compaction near
  442k suggests the window, or the compaction threshold, is not the 1M the
  model documentation lists. Compare the reducer's occupancy with `/context` on
  a live session before showing a percentage.
- The plan and subagent rows, which need a transcript that has them.
