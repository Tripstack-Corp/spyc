# Projects and sessions

Part of spyc's [feature reference](../../FEATURES.md). Workspace-level commands, several open projects, and saving and restoring a session.

## Global menu (leader)

Workspace-level commands live behind a **leader** so they're reachable
from anywhere — including while you're typing to the agent in the bottom
pane. The leader is **`Space`** in the file list; from the pane it's
**`^a Space`** (a bare `Space` is literal text to the child, so `^a`
wakes spyc first, then `Space` enters the menu). Hold it and the
which-key popup lists the options:

- **`Space w l` / `w n` / `w d`** — worktree list / new / delete (the same
  submenu as the `W` prefix, which still works in the list).
- **`Space p`** — jump to `PROJECT_HOME`.
- **`Space P`** — set `PROJECT_HOME` to the current directory.
- **`Space n`** — open a directory as a project, and go to it.
- **`Space x`** — close the project in view.
- **`Space j`** — list the open projects, to go to one.
- **`Space Space`** — go to the project you were in last.
- **`Space 1`..`Space 9`** — go to open project N.
- **`Space ]` / `Space [`** — go to the next / previous project, wrapping.
- **`Space !`** — go to the agent that needs you, in any project (`^a !` from
  the pane, two keys).
- **`Space s`** — session info.
- **`Space ?`** — open this help.
- **`Space a`** — open the about page.

The leader holds the workspace commands, the ones that make sense from
anywhere; commands that act on the file view (git, picks, sort) stay on the
`g` and letter keys, and `gw` still jumps to the worktree root.

## Projects

One spyc holds several projects, each with its own columns, pane tabs, config
and `PROJECT_HOME`. **`Space n`** asks for a directory (pre-filled with the
focused column's worktree root; `Tab` completes, `~` and `$VAR` expand, and a
relative path is read against the column) and opens it as a project: its `.spycrc.toml` is read as a launch there would read it, and its
`[pane] tabs` start the same way, a repo's own list asking first. Your own
settings for a project go in a file under `~/.config/spyc/projects/` whose
`home` names it: it wins over the repo's file, its tabs open unasked, and its
file name becomes the project's name
([per-project files](../config/projects.md)). A directory
an open project is anchored at is that project's, so `Space n` onto it goes
there instead, and says so when it is the one you're in. **`Space x`** closes
the project in view and takes you to the one you were in last, else a
neighbour. While a tab or an editor in it still runs it asks first, since
closing ends them; its agents' scope claims go, and an agent waiting on one is
answered. The only project open isn't closed this way (`:q` quits), and
from inside an editor it says to quit the editor first, since closing would
end it.
**`Space j`** lists the open projects, numbered, each with how many tabs it
has and its home, the one you're in highlighted: `j`/`k` and `Enter`, or
`1`-`9`, go to one, `/` searches, and `q` closes it. Opened from inside an
editor, it has the keyboard until it closes, as help does.
**`Space Space`** goes back to the project you were in last,
**`Space 1`..`Space 9`** to the Nth, and **`Space ]` / `Space [`** step
through them in the order they were opened, wrapping. With one project open
these do nothing at all.

**`Space !`**, or **`^a !`** from inside an agent's pane, goes to the agent
that needs you, in whichever project it is: the one blocked on you longest,
then one that finished while you weren't looking. It switches project and tab
and puts the keyboard in that agent's pane, showing the pane if it was hidden
or zoomed away. Press it again for the next one; with nothing waiting, it says
"nothing needs you". It works with one project open too, across its tabs.

**The status bar shows every open project** while there are several: a chip
for each, in the order they were opened, the one you're in bold and the others
dim, each with the glyph of what its agents most need, as their tab dots show
it: a red `■` when one is blocked on you, a teal `■` for a done you haven't
seen, `●` while one works and `○` while one waits on its own background work,
and nothing while they're idle (`spyc ■ · web ● · api`). The chips take the
session name's place, which stays in the terminal title and `Space s`, and
get a third of the bar. In a narrow terminal the quiet projects go first,
counted as `+N` (`spyc ■ · web ● +2`); then the others collapse to where you
are among them and their most urgent glyph (`spyc 2/4 ■`, project 2 of 4);
then the project you're in stands alone, with a red `■` while another is
blocked, which is never dropped: the label is cut first. Each `Space j` row
shows its project's glyph too, as it was when the list opened. The terminal
title counts the tabs blocked in the other projects, for when you're in
another window: `🌶️: spyc (■2) · SAFFRON_CUMIN`. A `^z`-suspended agent counts
for nothing in any of these, as its tab shows 💤. With one project open, the
bar, the title and `Space j` read as they always have. The coordination dumps
(`:activity dump`, `:agent list`, `:agent registry`) cover every open project
too, each project's agents under its name.

## Project home & session name

Each project has a **`PROJECT_HOME`** (a sticky project root), and each
spyc run a **session name** (a spice-themed label like `SAFFRON_CUMIN`).
Both are shown on the top bar (while several projects are open, their chips
take the place of both) and persist across `spyc -r`.

- **Auto-detect** — `PROJECT_HOME` is set to the launch directory
  automatically when that directory contains a `.git` entry. No
  upward walk — the concept is explicit and predictable.
- **Keys** — `Space p` jumps to `PROJECT_HOME`; `gP` (or `Space P`) sets it
  to the current directory; `gS` re-points the start directory (target of `` ` ``).
  A directory is one project's home: setting another open project's home (or
  clearing a home onto one, or moving a homeless project's start directory
  onto one) is refused, naming that project. A start directory is one
  project's too, so moving one onto another project's is refused.
- **Commands** — `:project` prints; `:project .`, `:project <path>`,
  `:project clear` manage the value. `:project config` opens the project's
  own config file in the editor, making it first if it has none
  ([per-project files](../config/projects.md)); `./config` reaches a
  directory named `config`. `:startdir` manages start dir.
  `:name <NEW>` renames the session (normalized to
  `[A-Z0-9_]`). `:whoami` flashes `user@host`.
- **New pane tabs** default their cwd to the focused column's
  worktree/repo root (`gw`'s target), falling back to the current
  listing dir when there's no repo. Set `[pane] new_tab_cwd` in
  `.spycrc.toml` to `"project_home"` (pin to `PROJECT_HOME`) or
  `"browse_dir"` (open in the current listing dir, "open here").
- **Session names** are generated at session creation from ~30
  spices, joined pairwise: `CUMIN_SAFFRON`, `HARISSA_SUMAC`, etc.
  Shown as the primary column in the `-r` session picker so you can
  pick by memory instead of by timestamp.

## Session management

spyc auto-saves your workspace on quit and can restore it on startup.

- **Crash-sufficient autosave** — beyond the quit-time save, spyc
  re-saves the session ~2s after any change (new tab, `cd`, split
  resize, …), so a hard
  kill (`SIGKILL`, crash, laptop sleep) loses at most that couple of
  seconds rather than everything since launch. It only writes when
  something actually changed, so an idle spyc does no disk work.
- **Auto-save** — on quit, spyc saves column `a`'s directory, all pane
  tabs (command, label, cwd), active tab, pane height, focus state,
  the spice-themed session name, `PROJECT_HOME`, the start directory, and
  the vertical split (its shape plus the second commander's cwd, or the
  preview file) — restored on `-r`, reopening columns `a` and `b` where you
  left them. A column `a` whose directory has gone opens at `PROJECT_HOME`.
  Sessions saved by an older spyc still restore. A quit with
  nothing to restore (no tabs, split or scope claims) saves nothing, so it
  doesn't take a picker slot; a restored session is still updated.
- **`spyc --resume`** (or `-r`) — opens a session picker showing
  the session name (primary column), a human-readable timestamp
  ("just now", "2 hours ago", "3 days ago"), and the cwd, or each
  project with its tab count when the session had several open.
- **Every project comes back** — restoring a session opens every project
  it had open, each with its own tabs, columns and split, in the order
  they were opened, and puts you in the one you were in. A project whose
  directory has gone (a worktree removed after its merge, say), or whose
  home another project of the session already has, stays shut: the rest
  open as a copy, with their own file, and the session they came from
  keeps every project. If that was the project you were in, you land in
  the first that opened. A project out of view that was inside an archive
  comes back beside it, cursor on it, one `Enter` from where it was.
- **Tabs that can't come back** — an unsupported Codex tab is left unopened
  while the other tabs resume, and its command and conversation stay saved
  for next time. Session info (`Space s`, or `^a Space s` from a pane) lists
  the unopened tabs and why. If every saved tab is refused, the current
  session stays as it was.
- **Running sessions** — a session another spyc is still writing is
  marked `running` in the picker. Restoring it makes a copy with its own
  file and name, whose agents start fresh rather than resuming the
  conversations that spyc still has open, and which leaves that spyc's
  scope claims alone.
- **Conversations another spyc is in** — whichever session you restore,
  a tab that would resume a conversation another running spyc's agent is
  in starts its agent fresh instead, leaving that spyc its scope claims,
  and the restore's message counts those tabs. So restoring a session
  twice, once as a copy and then again, doesn't put two spycs in one
  conversation. A codex tab that never learned its conversation, and a
  zot tab, resume the newest one in their directory, so each starts fresh
  where another spyc runs that agent there. The other spyc's save is a
  couple of seconds behind it, and names an agent's conversation only once
  spyc has matched it, so a restore at the very moment it starts can miss
  it; an agent started inside a shell tab isn't seen at all.
- **Tabs that can't be rebuilt safely** — a tab the restore would refuse,
  such as a codex started with a prompt, stays unopened even when it would
  start fresh, so its prompt is never sent twice. A copy of a running
  session keeps those tabs unopened too, as restoring the session itself
  would.
- **j/k navigation** — browse sessions with highlighted cursor row.
  Enter to restore, n for a new session, 1-9 for direct selection.
- Capped at the 20 most recently saved sessions. A session you restore
  keeps its place by when it was last saved, not when it was created.
- **Agent session resume** — for tabs running `claude`, `codex`,
  `agy`, or `zot`, quitting spyc and launching with `spyc -r`
  will restore. A claude tab resumes its own conversation (spyc types
  `/resume <id>` into a fresh `claude`); one saved before its first
  message restores as a fresh `claude`, since there is nothing to resume
  yet. A codex tab resumes its own session (`codex resume <id>`) even if
  it was still working at quit, or, when spyc never learned which session
  it was, falls back to `codex resume --last`, codex's own picker for the
  directory. An agy tab resumes its conversation (`agy --conversation
  <id>`); one with none restores as a fresh `agy`, since agy's
  `--continue` would take whichever conversation was newest, possibly
  another tab's. A `zot` tab restores with `zot --continue`, zot's most
  recent session for the directory. How each agent is resumed:
  [`docs/HARNESS.md`](../HARNESS.md).
