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
- **`Space s`** — session info.
- **`Space ?`** — open this help.
- **`Space a`** — open the about page.

This is the line between *global* commands (worktree, project — they make
sense from any focus) and *frame* commands (git, picks, sort — they act on
the file view and stay on the `g` / letter chords). `gh` (old project-home
jump) is gone in favour of `Space p`; `gw` (jump to the worktree root) stays.

## Projects

One spyc holds several projects, each with its own columns, pane tabs, config
and `PROJECT_HOME`. **`Space n`** asks for a directory (pre-filled with the
focused column's worktree root; `Tab` completes, `~` and `$VAR` expand, and a
relative path is read against the column) and opens it as a project: its `.spycrc.toml` is read as a launch there would read it, and its
`[pane] tabs` start the same way, a repo's own list asking first. A directory
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

## Project home & session name

Each project has a **`PROJECT_HOME`** (a sticky project root), and each
spyc run a **session name** (a spice-themed label like `SAFFRON_CUMIN`).
Both are shown on the top bar and persist across `spyc -r`.

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
  `:project clear` manage the value. `:startdir` manages start dir.
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

When restoring a saved session, an unsupported Codex tab is left unopened while
supported tabs resume. Its command and conversation metadata stay saved through
autosave and quit. Session info (`Space s`, or `^a Space s` from a pane) lists the
unopened tabs and reasons. If every saved tab is refused, the current session
stays intact.

spyc auto-saves your workspace on quit and can restore it on startup.

- **Crash-sufficient autosave** — beyond the quit-time save, spyc
  re-saves the session ~2s after any change (new tab, `cd`, split
  resize, …) to a stable per-session file written atomically, so a hard
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
- **Running sessions** — a session another spyc is still writing is
  marked `running` in the picker. Restoring it makes a copy with its own
  file and name, whose agents start fresh rather than resuming the
  conversations that spyc still has open, and which leaves that spyc's
  scope claims alone.
- **j/k navigation** — browse sessions with highlighted cursor row.
  Enter to restore, n for a new session, 1-9 for direct selection.
- Capped at the 20 most recently saved sessions. A session you restore
  keeps its place by when it was last saved, not when it was created.
- **Agent session resume** — for tabs running `claude`, `codex`,
  `agy`, or `zot`, quitting spyc and launching with `spyc -r`
  will restore. Claude tabs spawn a fresh `claude` and type
  `/resume <id>` once it's settled (the CLI flag has a regression),
  then verify the submit landed — re-sending Enter while the command
  is still visibly unsubmitted, since Claude's async startup can eat
  a lone `\r`. A tab saved before its conversation's first message
  restores as a fresh `claude`, since there is nothing to resume yet.
  Codex tabs spawn `codex resume <UUID>` directly — the UUID being the
  rollout spyc pinned to that tab while it ran, so a tab still working at
  quit resumes exactly rather than guessing. When no UUID was ever pinned,
  codex falls back to `codex resume --last`, which uses codex's native
  cwd-filtered picker. Agy tabs spawn `agy --conversation <id>`, the
  conversation its status hook reported or the one the tab was launched
  on. A tab with none, or one agy never wrote, restores as a fresh `agy`
  rather than `--continue`, which would resume whichever conversation in
  the workspace was newest, possibly another tab's. `zot` tabs
  restore with `zot --continue` (zot's resume-most-recent-for-cwd);
  specific-session resume and transcript scrollback are a follow-up
  pending its on-disk session-file format.
