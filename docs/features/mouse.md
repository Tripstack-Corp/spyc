# Mouse

Part of spyc's [feature reference](../../FEATURES.md). What the mouse does in spyc. Its settings, and what text selection costs in each terminal, are on the [mouse configuration page](../config/mouse.md).

With mouse reporting on, the wheel scrolls whatever is under the pointer,
left-click focuses that region (and clicks through to a mouse-aware agent),
middle-click pastes (a clipboard tool that hangs reports itself instead of
freezing spyc), right-click opens the leader menu. **Costs native click-drag
text selection while on** (hold Shift to select anyway — Option/Fn on iTerm2);
`:mouse off` gives it back immediately, no restart, and survives a config
reload (`:mouse auto` returns to following the config). Default **on**
(`[mouse] capture`; `capture = false` turns it off permanently). `^a u`
quick-select and `y` in the pager are the mouse-free yank paths.

While a which-key menu is open (the leader from a right-click, or a chord
you are part-way through), a click anywhere outside it closes it and does
nothing else, so the next key you type is an ordinary key again. A click on
the menu reaches nothing beneath it: not the list, and not the agent in the
pane.

Click a row to do what its key does. An action row fires and the menu closes;
a submenu row, such as the leader's `worktree`, opens that menu; a range row,
such as `1-9`, stands for several keys, so it names them and stays open for
you to type one. A count typed first (`3`, then `d`) reaches the row you click,
as it would the key.

## Selecting and copying

- **Drag in a pane whose child ignores the mouse to select its text**; release
  copies it. **Ctrl+drag selects through spyc in any live pane**, including
  Codex's queued questions and other surfaces whose child cannot select text.
  Hold Ctrl when pressing; spyc owns the whole gesture even if you release Ctrl
  first. The pane holds still while you drag, with its program running behind
  it, and catches up when you release and the text is copied. An ordinary drag
  in a mouse-aware child keeps reaching the child. The highlight stays only
  while the pane still shows what you copied; new output or input clears it.
  Ctrl+click behaviour and host-terminal forwarding preferences are documented
  in [the selection caveats](../config/mouse.md#selecting-text).
- **Drag in the file list to select rows; release copies their names** — hold
  **Ctrl** while pressing to copy absolute paths instead. The highlight stays
  up after the copy. Distinct from picks: a drag never changes what the next
  file operation acts on.
- **Drag across any chrome row to select part of it** — the status line, the
  divider/tab line, the prompt row (a flash, the `:` command line, an armed
  chord) and the activity HUD. Release copies exactly those columns, so you can
  take just a branch name, an agent session id, a custom tab name or an error
  message without the rest of the line. A click that doesn't move copies
  nothing, and copying part of a flash leaves the message intact.
- **Double-click a chrome row to take the whole line** — a flashed error with
  its whole `source()` chain, the status line, a divider. That's the copy you
  want when the line is one thing rather than a list of them, and it doesn't
  ask you to drag a full terminal width accurately. Copying the same row twice
  gives the same text both times: the ` (copied)` confirmation is spyc's own
  marker and never reaches the clipboard.
- **Drag in a pager to select text; release copies it** and the pager title
  reports the line count. Works the same full-screen or popped-up, and copies
  the content only — never the line-number gutter, the whitespace/line-break
  markers, or the border. In a Markdown view `m` chooses what you get: rendered
  prose, or the raw source. A click that doesn't move stays a click.

## Clicking and scrolling

- **Click a pane tab to switch to it.**
- **Click the status bar's `⚠N` to open `:stderr`**, the log of what spyc's
  stderr received.
- Over an agent pane the wheel scrolls in whatever way that agent can:
  **claude** and **agy** scroll their own output; **codex**'s main chat view
  doesn't scroll, so the wheel drives its `^T` transcript overlay instead —
  opening it after a few consecutive ticks UP (`[mouse] pane_scroll_view`,
  configurable to stay closed or to use spyc's own `^a v` history instead),
  then scrolling by line, escalating to a page jump under a sustained gesture.
  Scrolling DOWN never opens it — that gesture is trying to reach the live
  buffer, not leave it. Scrolling down while already at the bottom exits the
  transcript (`q`). Outside the transcript, the same keys only move the draft
  cursor (codex keeps history recall on `^R`, so the wheel can't recall a past
  prompt).
