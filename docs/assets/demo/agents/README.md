# Agent dots demo

The README's "See which agent needs you" clip (`docs/assets/demo-agents.gif`):
four panes in four states, and the one that needs you.

| Tab | Dot | What it is |
| --- | --- | --- |
| `[1]` | pulsing `●` | an agent working the whole clip |
| `[2]` | `●`, then red `■` | an agent that blocks on a question mid-take |
| `[3]` | teal `■` | an agent that finished its turn |
| `[4]` | `💤` | rmatrix, suspended with `^z` |

The take opens on the rain zoomed, suspends it, and comes back to the list.
Tab 2 then blocks: its dot turns red and the border pulses. Answering it lets
the agent finish and move the file list to `guides/` over MCP. The rain wakes to
close, so the loop restarts on it.

## Running it

```sh
make demos TAPE=agents      # films the release build into docs/assets/demo-agents.gif
python3 demo.py --binary /abs/path/to/spyc            # a take, outside the repo
python3 demo.py --binary /abs/path/to/spyc --setup    # arrange the panes, leave them running
```

Needs `tui-test` (`brew tap microsoft/tui-test`), `agg`, and `rmatrix`
(`cargo install rmatrix-reloaded`). Each take gets a new directory under
`/tmp/spyc-demo/` holding the cast, the GIF, a step log, and the screen at every
check, so a failed run can be read back.

## How it works

`demo.py` runs spyc in its own `tui-test` session, with a private `HOME` and
state directory under `/tmp/spyc-demo-agents`, recreated every run. Your config,
sessions and consent answers are never read or written, and the status bar reads
`~/aurora-docs`. The fixture is the markdown harness's `aurora-docs/`.

The agents are `agent.py`, installed as `claude` on the pane's `PATH` so spyc
treats each tab as a Claude pane. Each one opens a `spyc --mcp` session and calls
`report_status` and `navigate_to` exactly as a real agent does. Only the work is
scripted. Tab 2 blocks when the driver creates a trigger file, so the
transition happens on camera rather than during setup.

Setup is not filmed. The driver arranges the four tabs, verifies them through
`:activity dump`, then starts recording. A cast started mid-session opens on a
full-screen snapshot, so nothing before the first beat leaks into it. Every beat
waits on spyc's own state, and the dump is checked again once recording stops.
`agg` renders the cast in Catppuccin Macchiato, the palette the vhs-recorded
README GIFs use.

## Traps, confirmed

- **A real agent shows the recorder's account.** Run under the recorder's
  `HOME`, Claude Code shows that account's status line (usage and quota) and any
  notice from the organization's managed settings in its header. Stand-ins keep
  both out of a public GIF, need no login, and play identically every run.
- **`blocked` and `done` are the same glyph.** Both render `■` and differ only in
  colour, red against teal. The driver reads the dot's cell through
  `tui-test cells` and compares its colour; a text match alone cannot tell
  "needs you" from "finished".
- **`[N]` can match more than the divider.** The dot is read from the match on
  the row that starts with `─`.
- **The status-hooks consent popup is modal and only `y`/`n` closes it.** Every
  other key is swallowed while it stays up. A fresh state directory raises it on
  the first agent pane.
- **`^a c` prefills the command box** with `$SPYC_PANE_CMD`. Typing on top of it
  spawns `claudeclaude`; `^u` clears it first.
- **Four panes overflow one screen of `:activity dump`.** The driver pages
  through it until the pager reads `Bot` or `All`.
- **A non-blocked report lives five minutes,** then the dot falls back to output
  timing. Setup and take together run well under a minute.
- **Blank `SPYC_MCP_SOCK` and `SPYC_PANE_ID` for the session.** Run from inside a
  spyc pane, the demo would otherwise inherit them, and its reports would reach
  the spyc you are using.
- **Never `tui-test close --all`.** It closes every session on the machine,
  including other agents' smoke runs. The driver closes only its own.
- **The rain is what makes the GIF large.** Its frames redraw most of the screen,
  so the live rain shots are short, `rmatrix` draws at 15fps, and `agg` keeps at
  most 12. That brought a 1.7 MB take down to about 900 KB.
