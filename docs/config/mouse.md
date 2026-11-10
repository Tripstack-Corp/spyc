# Mouse — `[mouse]`

Part of [Configuring spyc](../../CONFIGURATION.md).

```toml
[mouse]
capture = true       # real mouse reporting (wheel + buttons). Default ON.
scroll_lines = 1     # lines per wheel tick, for surfaces spyc scrolls itself
pane_scroll_lines = 3 # lines per tick for a pane spyc drives with synthesized keys
pane_scroll_view = "native" # native | off | spyc_history
invert_scroll = false # true flips the wheel direction everywhere
```

`pane_scroll_view` decides what a wheel gesture does when it hits an agent pane
whose own scrollback view spyc can drive but that view isn't open yet — today,
codex's `^T` transcript. `native` (default) opens it and scrolls; `off` leaves it
closed and only scrolls if it happens to already be open; `spyc_history` opens
spyc's own `^a v` scrollback pager instead. Doesn't affect an already-open view
(always scrolled) or an agent with no such view (agy scrolls its live content
directly via Shift+Arrow, with nothing to open).

Only a sustained scroll **up** — three consecutive ticks, reset by a pause, a
reversal or a tab switch — opens a closed view. A downward gesture never does:
it's aimed at the live buffer, and opening a transcript there lands at its
bottom, which the next tick reads as "at the bottom" and closes again.

A sustained same-direction wheel gesture — past ~1 second — escalates from the
per-line step to a page-sized one, for an agent with a verified fast key (codex's
`^T` documents `pgup/pgdn to page` in its own footer). `pane_scroll_lines` is the
line-scroll floor under that escalation, and the only knob agy uses (it has no
page-sized equivalent).

`capture` asks the terminal for real mouse reporting so spyc can scroll whatever
is under the pointer, instead of DEC 1007's trick of translating the wheel into
arrow keys (which a focused pane receives as history navigation).

`invert_scroll` flips the wheel direction. Left at its default (`false`), a
downward tick moves the file-list cursor to a later row and the pager further
into the content — the "scroll down = toward the end" mapping shared by browsers
and `less`. Set it to `true` if that reads backwards for you.

It's named for the mechanism rather than for a convention on purpose: whether
"down" *should* move the content or the cursor depends on your OS trackpad
setting and your terminal, so a `natural_scroll`-style flag would be ambiguous
in precisely the situation you'd reach for it. The flip is applied once, to the
event itself, before anything branches on it — so the file list, the pager, an
agent pane's synthesized scroll keys **and the wheel report forwarded to a
mouse-aware child** (claude, vim, htop) all move together. Half-inverting spyc
would be worse than either setting: the pane scrolling one way and the column
beside it the other.

> [!WARNING]
> **Capture takes native click-drag text selection away from your terminal.**
> That is inherent to mouse reporting, not a spyc choice. Three ways to get it
> back:
>
> | | |
> |---|---|
> | **Bypass modifier** (per-drag) | Hold **Shift** — Ghostty, WezTerm, kitty, Alacritty, most others. **Option** or **Fn** on iTerm2. |
> | **`:mouse off`** (per-session) | Immediate, no restart, no file edit. `:mouse on` to re-enable, `:mouse auto` to follow the config again. Survives a config reload — it is scoped to the session, not written to a file. |
> | **`capture = false`** (permanent) | Turn it off entirely — the pre-#226 behavior. |
>
> spyc's mouse-free yank paths: **`^a u`** quick-select (URLs / paths / SHAs) and
> **`y`** in the pager.

## What the buttons do

| Button | Action |
|---|---|
| **Left** | Focus the region under the pointer. Over a mouse-aware child (claude, vim, htop) the click also reaches it — the pane is live, so swallowing the first click just to focus would read as broken. **Dragging** is forwarded too, so the child's own text selection works. |
| **Middle** | Paste the system clipboard wherever a paste would land (pane, `:` line, shell prompt). |
| **Right** | Open the menu of what it lands on (a file row, the pane, a tab label, a divider, an open pager, a status-bar segment), else the leader menu. `.` opens the entry's menu from the keyboard, and `^a .` the active tab's from the pane. On iTerm2, hold ⌃: see [which right-click reaches spyc](#which-right-click-reaches-spyc). |

Middle and right are **always spyc's** — never forwarded, even over a mouse-aware
child, since otherwise the gesture would be unavailable in exactly the region where
the pane holds focus. If you need a child's own right-click menu, `:mouse off`.
What each menu offers: [Mouse](../features/mouse.md#right-click-the-menu-of-whats-under-it).

## Which right-click reaches spyc

spyc reads a right-click the same with any modifier held, but some terminals
keep a right-click for their own menu, so it never reaches spyc. The rule:
**⌃-right-click on iTerm2, a plain right-click elsewhere.** ⌃ is the one
modifier that reached spyc in every terminal measured, so it is the one to try
in a terminal not listed here.

Measured on macOS 26.7.1, with the mouse modes spyc turns on, by
`scripts/rightclick-probe.py` (2026-10-26). ✓ means the click reached spyc:

| Terminal | Plain | ⇧ | ⌥ | ⌃ | ⌘ |
|---|---|---|---|---|---|
| iTerm2 3.7.6 | its own menu | ✓ | its own menu | ✓ | ✓ |
| Terminal.app 470.2 | ✓ | ✓ | ✓ | ✓ | a middle click (pastes) |
| Ghostty 1.3.1 | ✓ | its selection bypass | ✓ | ✓ | ✓ |
| kitty, WezTerm | not measured | | | | |
| Windows Terminal (WSL) | not measured | | | | |

Where no right-click reaches spyc, `.` opens the entry's menu and `^a .` the
active tab's, and every menu row names the key or command that does the same.

## Selecting text

Capture takes the terminal's own click-drag selection away. Where that leaves you:

| | |
|---|---|
| **Inside an agent tab** (claude, Codex, agy) | A drag selects through spyc, holding the pane still while the agent keeps streaming. A click that doesn't move still reaches the agent, when you release it. |
| **Inside another mouse-aware child** (vim, htop) | An ordinary drag reaches the child. **Ctrl+drag** selects through spyc, including child surfaces that cannot select text. |
| **spyc surfaces and non-mouse panes** | Drag normally; release copies the selected text or file names. Hold Ctrl in the file list to copy absolute paths. |
| **Host terminal selection** | Hold **Shift** (Option/Fn on iTerm2), or use `:mouse off`. spyc never sees this drag, so it can't hold the pane still for it: streaming output can move the text under it. |
| **Copying a diagnostic** | `:activity dump`, `:grep`, `:why-git` etc. open a pager — **`y`** yanks the source to the clipboard, **`Y`** the visible text. No mouse needed. |

spyc holds a pane's visible grid during its own drag, while the child keeps running.
Release copies from that held grid and resumes live output. Hold Ctrl when pressing
to claim a mouse-aware pane; the drag remains spyc's if Ctrl is released first.
A Ctrl+click without movement leaves the clipboard alone and is consumed by spyc:
the child receives neither press nor release. Child Ctrl+click actions, such as
Vim's tag jump, are unavailable through this gesture while capture is on.

While the drag lasts, nothing moves the pane: the agent HUD strip, a flash, the
divider and the pane's size wait for the release, then catch up. A key, paste,
wheel gesture, tab switch or terminal resize cancels the held pane drag, and so
do 30 seconds with no mouse event at all, so a release your terminal never
reported (you switched windows mid-drag) can't leave the pane frozen.
Turning mouse capture off also cancels it.

The terminal must report Ctrl+left mouse events to spyc. In iTerm2, enable
**Settings → Pointer → “Ctrl-click reported to apps, does not open menu”**;
[iTerm2's pointer reference](https://iterm2.com/documentation-preferences-pointer.html)
describes this forwarding preference. Terminal.app uses Control-click for its
[shortcut menu](https://support.apple.com/guide/terminal/trml135fbc26/mac).
If your terminal opens a menu instead of starting a selection, spyc never receives
the gesture. Use `:mouse off` with the terminal's own drag selection, or a terminal
that forwards Ctrl+left events.

A left click on a *row* in the file list deliberately doesn't move the cursor:
clicking a region is coarse and hard to get wrong, while aiming at a one-cell-tall
line invites near-misses that silently move the cursor. `j`/`k`, `F`, and frecency
jumps are all faster and more precise. A right-click on a row does move the cursor
there, since the menu it opens is titled with that entry, so a near-miss shows
before anything acts.

`scroll_lines` defaults to **1** because trackpads and most modern terminals
already emit one wheel event per notional line — a multiplier there makes the list
fly. Raise it for a detented wheel that sends one event per physical click.

`scroll_lines` applies to the surfaces spyc scrolls itself. A pane forwarding to a
mouse-aware child is unaffected — the child receives one event per tick and picks
its own step. Clamped to at least 1.
