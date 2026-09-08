# Agent orchestration demo — WORK IN PROGRESS, NOT YET SHOOTING

The second harness: a ~90s clip answering one question, *which agent needs me?*
It opens on four panes already in four states and lets the dots carry it.

| Tab | Dot | What it is |
| --- | --- | --- |
| `[1]` | heat-pulse `●` | Haiku, mid-task |
| `[2]` | steady red `■` | Haiku, waiting on you |
| `[3]` | calm teal `■` | Haiku, finished its turn |
| `[4]` | `💤` | rmatrix, `^z`-suspended |

The agents are real (`claude --model haiku`). The fixture is borrowed from the
markdown harness rather than copied — `fixtureRel` is the one path both the
locate-check and the staging copy read.

```sh
osascript spyc-agents-demo.applescript setup   # arrange four panes, stop, no recording
osascript spyc-agents-demo.applescript dry     # arrange + run the beats, no recording
osascript spyc-agents-demo.applescript         # film it
```

## Where it got to

The arrangement runs: four panes spawn, the consent popup is answered, and three
Haiku agents take their prompts. **It has never reached a take.** Verification of
the four states through `:activity dump` is where it stops, and the last two runs
both died on an iTerm error (`Can't get tab 1 of window id N`) that means the
window lost its session mid-run. Both times a person was also driving that
window, so it is **not yet established** whether that is a harness bug or an
interrupted run. Establish that first, from an untouched window.

## Traps, confirmed

- **`blocked` and `done` are the same glyph.** Both render `■` (U+25A0) and
  differ only in colour — hot red against teal — and `get text` carries no
  colour. Counting `■` cannot tell "needs me" from "finished". The arrangement
  is verified through `:activity dump`, which names each state in words. This is
  the mermaid-beat bug waiting to happen again, and the reason the dump exists.
- **The status-hooks consent popup is modal and only `y`/`n` closes it.** Esc and
  every other key are swallowed while it stays up. Miss it and the entire
  arrangement types itself into a prompt that never closes: no hooks are
  installed, so no agent can self-report, and every dot silently degrades to
  output timing. Nothing errors — the run just produces meaningless dots. It is
  raised on the first agent pane per project root and remembered afterwards, so
  it reproduces only on a machine that has never consented for that path.
- **`^a c` prefills the command box** with the default (`claude`). Typing on top
  of it yields `claudeclaude` and a pane that exits 127. `^u` clears the buffer.
- **Only `working` is unstable.** `blocked` latches until settled, `done` and
  `idle` persist, `💤` is a sticky toggle — but `●` lasts only while a turn is
  running, so tab 1 needs a long read-only task and the take has to be shot while
  it is still going. Read is not gated by default, which is what keeps that pane
  `working` rather than stopping on a prompt like tab 2 deliberately does.
- **Never assert on agent prose.** Real agents are non-deterministic; spyc's own
  state is not. The beats wait on the dots, the `💤`, and the file list moving
  under a `navigate_to` — never on what an agent said.
- **Do not zoom to watch a dot.** The dots live in the divider, so `^a z` on a
  pane hides the thing the beat is about. And `:activity dump` is a setup
  instrument only — it throws a pager over the frame, which is fine while
  arranging and wrong on camera.

## Next

1. Reproduce the `tab 1` error in a window nobody touches; decide bug or interrupt.
2. Confirm tab 2 actually reaches `blocked` — a permission prompt depends on the
   local Claude config, and an allowlisted Bash would leave it `working` instead.
3. Then a `dry` run, then a take.
