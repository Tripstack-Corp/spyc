# Mouse

Part of spyc's [feature reference](../../FEATURES.md). What the mouse does in spyc. Its settings, and what text selection costs in each terminal, are on the [mouse configuration page](../config/mouse.md).

With mouse reporting on, the wheel scrolls whatever is under the pointer,
left-click focuses that region (and clicks through to a mouse-aware agent),
middle-click pastes (a clipboard tool that hangs reports itself instead of
freezing spyc), and right-click opens the menu of what it lands on. **Costs
native click-drag text selection while on** (hold Shift to select anyway —
Option/Fn on iTerm2);
`:mouse off` gives it back immediately, no restart, and survives a config
reload (`:mouse auto` returns to following the config). Default **on**
(`[mouse] capture`; `capture = false` turns it off permanently). `^a u`
quick-select and `y` in the pager are the mouse-free yank paths.

## Right-click: the menu of what's under it

- **On a file row**, the cursor moves there and that entry's menu opens: open,
  edit, pick, copy, move, remove, its git views, send it to the pane, and the
  rest, as `.` opens it from the keyboard. A Makefile's menu (`GNUmakefile`,
  `makefile` or `Makefile`) starts with `make target …`, which opens the make
  menu of its targets; a click on a target runs it ([Make
  targets](shell.md#make-targets)).
- **On the pane**, while its program runs, the pane takes the keyboard and
  the active tab's menu opens: restart, suspend, fork, rename, close, zoom, scroll back, and the
  rest, as `^a .` opens it.
- **On a pane tab's label**, spyc switches to that tab, as a left click does,
  and opens its menu.
- **On the pane divider, off the tab labels and the task tags** (past the
  last tab, or on the cwd), the pane takes the keyboard and the `pane tabs` menu opens: new tab,
  next and previous tab once there are two, zoom, grow and shrink.
- **On the background tasks' tags** (`[1●]` on the divider, or the status
  bar's `bg:1●` with no pane open), the `tasks` menu opens: the task viewer
  (`gB`), then each task by its command, with the `:fg N` that brings it
  back. A click on a task's row brings it back as the live capture, as
  `:fg N` does. While a capture is live only the task viewer is offered,
  since `:fg` would refuse.
- **On the split's divider**, the `split` menu opens: flip its height, focus
  the left or right column, and dim the inactive one, then close the split
  beside a preview, or close the second commander beside one. The keyboard
  stays where it was.
- **On an open pager**, the pager takes the keyboard and its menu opens,
  titled with the pager's title. A running `!` capture's menu, opened
  anywhere on screen, offers background (`^Z`), interrupt (`^C`) and kill
  (`^\`). The task viewer (`gB`) offers what applies to its task: interrupt,
  bring it to the foreground or move it to a pane tab (`:fg N` and
  `:task-to-pane N`, typed from the file list once the pager is closed,
  since in a pager `:` jumps to a line), pause or continue (`S`, `C`), and
  step to the next or previous task (`]t`, `[t`). Any other pager, a
  `^a v` scrollback or a `D` file among them, offers save (`s`) where its
  text can be saved, and close (`q`). Other full-screen views (the finder,
  quick select, harpoon, the image views) keep the right-click to
  themselves.
- **On the status bar:**
  - **A project's label** goes to that project, as a left click does, and
    opens its menu: close it (`Space x`, while another is open), jump to its
    home (`Space p`, once it has one), make the current directory its home
    (`Space P`), then the open projects' rows: the list (`Space j`), a new
    project (`Space n`), the next and previous (`Space ]`, `Space [`, while
    several are open) and the agent that needs you (`Space !`, while one
    waits). With one project open, its segment opens the same menu, less
    close and stepping.
  - **The collapsed count** (`2/4`, where the bar is too short for every
    project) opens the open projects' rows alone.
  - **The git segment** offers the worktree list, a new worktree and, at the
    root of a clean linked worktree no session has claimed, deleting it
    (`Space w l`, `n`, `d`), and `gd` while the repo has changes.
  - **The agent segment** gives the pane the keyboard and opens its active
    tab's menu, as `^a .` does.
  - **The session segment** offers the session's info (`Space s`).
- **Anywhere else** (blank list space, the rest of the status bar, the prompt
  row, the agent strip, or the divider while the prompt row has its place),
  the leader menu opens.

Each row shows the key that does the same, so the menu teaches it. A row
shown with `(list)` works from the file list, since in the pane the key
would be typed into the agent. `↑` and `↓` highlight a row and `Enter` fires
it; `Esc` closes the menu. Any other key closes the menu and then does what it
always does, so typing a row's key does what the row says, and from the pane
a letter still reaches the agent, while `?` opens the list of every key, as the
title says. In a pager's menu every key is the pager's, `?` too: in a running
capture it is typed into the command. A row appears only where its action applies:
no `e` on a directory, no harpoon outside a repo or a project home, no suspend
on a shell tab, no fork before an agent tab has a conversation, no pause on a
paused task, and inside an archive no row the archive refuses.

iTerm2 keeps a plain right-click for its own menu, so hold **⌃** while you
right-click there; spyc reads a right-click with any modifier the same.
[Which right-click reaches spyc](../config/mouse.md#which-right-click-reaches-spyc)
has each terminal measured.

## Open menus

While a which-key menu is open (the leader from a right-click, or a chord
you are part-way through), a click anywhere outside it closes it and does
nothing else, so the next key you type is an ordinary key again. A click on
the menu reaches nothing beneath it: not the list, and not the agent in the
pane.

The same holds for an entry's or a tab's menu, and the wheel beside one
closes it too, rather than move the cursor off the entry it names.

Click a row to do what its key does. An action row fires and the menu closes;
a submenu row, such as the leader's `worktree`, opens that menu; a range row,
such as `1-9`, stands for several keys, so it names them and stays open for
you to type one. A count typed first (`3`, then `d`) reaches the row you click,
as it would the key.

Once a row fires by a click or by `Enter`, the prompt row names its key, such
as `gg  top of column` or `^a n  next pane tab`, so it can be
typed next time. A row fired by its own key names nothing, and when the action
says something itself, its key goes in front of the message, which keeps its
place: `^a z · zoom: pane (^a z to exit)`, or an error as an error.

The hint fades as you learn the key: once you have fired an action by its key
three times, a click on its row no longer names it. Only the key counts, a key
typed while the menu is open included, and a click never does, so clicking a
row can't silence what it teaches. What you have learned is yours, kept across
runs and projects: `:hints` lists the actions whose hint has faded, and `:hints
reset` brings every hint back. The menus themselves never fade; each row keeps
its key. Until you first open an object's menu, the status bar ends with
`. menu`, the key that opens one. `[ui] menu_hints` sets how many presses it
takes, or keeps the hints for good, or turns them off
([configuration](../../CONFIGURATION.md)).

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
- **Click a project's label on the status bar to go to that project**, as
  `Space N` does, or the collapsed count (`2/4`) to list the projects, as
  `Space j` does; the prompt row names that key, as a menu row's click does,
  until you have learned it. A click on the agent segment gives the pane the
  keyboard.
  The rest of the bar is text to select.
- **Click the status bar's `⚠N` to open `:stderr`**, the log of what spyc's
  stderr received.
- **Click the `y` or the `n` of a prompt's `[y/N]` to answer it**, as
  typing the letter does: restarting or closing a running tab, closing a
  project, removing files, mounting a large archive, and the other
  single-key confirms. A consent's pop-up takes a click on its `[y]` or
  `[n]`, and a startup-tab list's on `[Esc]`. A click anywhere else leaves
  the prompt up. A prompt you finish with `Enter`, such as removing a
  worktree, still wants the keyboard.
- **Click the background tasks' tags to open the task viewer**, as `gB`
  does, on the newest task: the `[N●]` tags on the divider, or the status
  bar's `bg:` with no pane open. The viewer names `gB` until you have
  learned it.
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
