# Shell and the command line

Part of spyc's [feature reference](../../FEATURES.md). Running commands from spyc, the `:` command line, and commands left running in the background.

## Shell integration

Three modes of running commands, each for a different use case:

- **!** captured — run a command and stream output into the pager in
  real-time with an hourglass timer. Stderr is merged so build
  progress, errors, and output all appear together. `%` expands to
  the current selection, relative to the column's directory where the
  command runs, so `./%` runs the selected script; anything outside
  stays absolute, and a name starting with `-` gets a `./`. `^C`
  interrupts; `^\` hard-kills; `^Z` sends the task to the background,
  where it keeps capturing its output (resume with `:fg` -- see
  Background tasks).
- **!!** — repeat the last captured command.
- **!?** — history editor popup. Opens instantly (no Enter needed).
  Also reachable mid-prompt: after `Esc` puts the line editor in
  Normal mode (e.g. you pressed `Esc k` to browse history), `?` (or
  `Space`) opens the same viewer — not just `!?` from a fresh prompt.
  Defaults to Normal mode — `j`/`k`/`G`/`gg` navigate, `/` search
  with `n`/`N` to jump between matches, `:N` jumps to entry N.
  Press `i` to vi-edit the highlighted command in-place.
  `Enter` executes the (possibly edited) command, `Ctrl+D` deletes
  an entry, `Esc`/`q` closes.
- **;** foreground — run an interactive command (top, vim, htop, less)
  in a top-overlay pty that replaces the file listing while the bottom
  pane stays visible. **`^a-j` / `^a-k` flip focus between the overlay
  and the bottom pane** so you can `;less docs/architecture.md`, pop
  down to claude, do work, and pop back up to scroll the doc — all
  without quitting the subprocess. The unfocused side dims; the focus
  flash says `focus: overlay` or `focus: <tab label>`.
- **$** shell — drop into `$SHELL` in the current directory. It opens with a
  reverse-video `spyc subshell — exit or ^D to return` line, sets the window
  title, and exports `SPYC_SUBSHELL=1`. The line scrolls away with the
  shell's output; for a marker on every prompt, add one line to your shell's
  rc file:

  ```sh
  [[ -n $SPYC_SUBSHELL ]] && PROMPT="[spyc] $PROMPT"   # zsh
  [[ -n $SPYC_SUBSHELL ]] && PS1="[spyc] $PS1"         # bash
  ```

  spyc doesn't keep a screen row for a marker, because every program the
  subshell runs (`less`, `man`, `clear`) would then draw inside what's left.
  A spyc launched with `SPYC_SUBSHELL` already set flashes a warning instead
  of letting you get lost two subshells deep.

All three modes run the command in your `$SHELL` as an interactive
shell would, so aliases, functions, and rc-file PATH entries from
`.zshrc` / `.bashrc` work the same way they do in a regular terminal
tab (zsh, bash, fish and ksh; a POSIX `sh` or `dash` reads no rc file
that way, so it just runs the command).

The shell prompt uses a vi-mode line editor with persistent history
(shared across sessions), so you get `h/l/w/b/0/$` motion, `x/D/C`
editing, operator+motion (`dw`, `cw`, `db`, `d$`, `dd`, `cc`, etc.),
and `i/a/I/A` mode switching — all within the one-line prompt.
`j`/`k` in normal mode cycle through history without leaving normal
mode. Ctrl+J inserts a newline in the pane (for Claude CLI
multi-line input).

Pane command prompts (`^a c`) have their own dedicated history,
separate from shell commands — so Up/Down shows `claude`, `zsh`,
`bash` instead of mixed shell commands. The follow-up "pane cwd:"
prompt keeps a *separate* history again, so Up/Down there recalls
previously-used working directories without mixing them into the
command list. History is de-duplicated (most recent use moves to the
end).

## Command line

**`:`** opens a vim-style command prompt with vi editing and history:

- **`:cd <path>`** — change directory (`~` and `$VAR` expanded, bare `:cd` goes home)
- **`:sort <mode>`** — sort listing by `name`, `size`, `mtime`, or `ext`
  (persists across chdir); **`:sort reverse`** toggles reverse order
- **`:marks`** — show all marks in a pager popup
- **`:set key=value`** — runtime settings: `:set sort=mtime`, and
  `:set flags=auto|short|full` for how the status bar writes its state
- **`:bprev`** / **`:bnext`** — navigate pager buffer history (also `[b`/`]b` in pager)
- **`:mouse on|off|auto`** — real mouse reporting: the wheel scrolls whatever is under
  the pointer, left-click focuses that region (and clicks through to a mouse-aware
  agent), middle-click pastes (a clipboard tool that hangs reports itself
  instead of freezing spyc), right-click opens
  the leader menu. **Costs native click-drag text
  selection while on** (hold Shift to select anyway — Option/Fn on iTerm2);
  `:mouse off` gives it back immediately, no restart, and survives a config
  reload (`:mouse auto` returns to following the config). Default **on**
  (`[mouse] capture`; `capture = false` turns it off permanently). `^a u`
  quick-select and `y` in the pager are the mouse-free yank paths.
  **Drag in a pane whose child ignores the mouse to select its text**; release
  copies it. **Ctrl+drag selects through spyc in any live pane**, including Codex's
  queued questions and other surfaces whose child cannot select text. Hold Ctrl
  when pressing; spyc owns the whole gesture even if you release Ctrl first.
  The pane holds still while you drag, with its program running behind it, and
  catches up when you release and the text is copied. An ordinary drag in a
  mouse-aware child keeps reaching the child. The highlight stays only while the
  pane still shows what you copied; new output or input clears it.
  Ctrl+click behaviour and host-terminal forwarding preferences are documented
  in [the selection caveats](../config/mouse.md#selecting-text).
  **Drag in the file list to select rows; release copies their names** — hold
  **Ctrl** while pressing to copy absolute paths instead. The highlight stays up
  after the copy. Distinct from picks: a drag never changes what the next file
  operation acts on.
  **Click a pane tab to switch to it.**
  **Drag across any chrome row to select part of it** — the status line, the
  divider/tab line, the prompt row (a flash, the `:` command line, an armed
  chord) and the activity HUD. Release copies exactly those columns, so you can
  take just a branch name, an agent session id, a custom tab name or an error
  message without the rest of the line. A click that doesn't move copies nothing,
  and copying part of a flash leaves the message intact.
  **Double-click a chrome row to take the whole line** — a flashed error with its
  whole `source()` chain, the status line, a divider. That's the copy you want
  when the line is one thing rather than a list of them, and it doesn't ask you to
  drag a full terminal width accurately. Copying the same row twice gives the same
  text both times: the ` (copied)` confirmation is spyc's own marker and never
  reaches the clipboard.
  **Drag in a pager to select text; release copies it** and the pager title
  reports the line count. Works the same full-screen or popped-up, and copies the
  content only — never the line-number gutter, the whitespace/line-break markers,
  or the border. In a Markdown view `m` chooses what you get: rendered prose, or
  the raw source. A click that doesn't move stays a click.
  Over an agent pane the wheel scrolls in whatever way that agent can:
  **claude** and **agy** scroll their own output; **codex**'s main chat view
  doesn't scroll, so the wheel drives its `^T` transcript overlay instead — opening
  it after a few consecutive ticks UP (`[mouse] pane_scroll_view`, configurable to
  stay closed or to use spyc's own `^a v` history instead), then scrolling by line,
  escalating to a page jump under a sustained gesture. Scrolling DOWN never opens
  it — that gesture is trying to reach the live buffer, not leave it. Scrolling
  down while already at the bottom exits the transcript (`q`). Outside the transcript,
  the same keys only move the draft cursor (codex keeps history recall on `^R`,
  so the wheel can't recall a past prompt).
- **`:limit <glob>`** — temporary filter (e.g. `:limit *.rs`)
- **`:limit !`** — show only picked files
- **`:limit git`** / **`:limit g`** — show only files in `git status`
- **`:limit h`** / **`:limit harpoon`** — show only harpoon entries
- **`:limit`** — clear filter
- **`:!<cmd>`** — captured shell command (same as `!`)
- **`:!!`** — repeat last captured command
- **`:;<cmd>`** — foreground shell command (same as `;`)
- **`:fg`** / **`:fg N`** — resume a backgrounded task (see Background tasks)
- **`:pause`** / **`:pause N`** — pause a backgrounded task (`SIGSTOP`)
- **`:resume`** / **`:resume N`** — resume a paused task (`SIGCONT`)
- **`:grep <pattern>`** — project-wide content search with ripgrep's
  matcher, built in. Walks the
  focused commander's worktree root (its repo root, else
  `PROJECT_HOME`, else current dir) honouring `.gitignore`, smart-case
  by default. Results stream into a pager as `path:line:col: text`,
  so `gf`/`gF` jump from a hit to the file in-place. Capped at 5000
  matches; refine the pattern if you hit it. Power users: `! rg foo`
  still works for ripgrep's full flag surface.
- **`:q`** / **`:quit`** — quit

The `:` prompt shares history with other shell prompts, so Up/Down
cycles through previous commands.

**`spyc -c <cmd>`** runs a `:` command from the shell once spyc is up:
after `init.lua` loads, a `-r` session is picked, and any startup question is
answered. Repeatable, run in order, the `:` optional
(`spyc -c "sort mtime" -c "limit *.rs"`).

## Background tasks

Long-running captured commands (`!cargo test`, `!find ...`) don't have
to lock you out of spyc.

- **^Z** while a `!` capture pager is open sends the task to the
  background, where spyc keeps capturing its output (the latest 1 MB of
  it). Tasks render in the pane divider,
  right-aligned, in a distinct colour family from pane tabs:
  `[N+]` running with new output (teal), `[N●]` running quiescent
  (blue), `[N✓]` exited cleanly (green), `[N✗]` non-zero / killed
  (red). When the pane is hidden, falls back to a status-bar
  `bg:N●M✓` suffix.
- **`:fg`** resumes the most-recently-backgrounded task; **`:fg N`**
  targets a specific task id. Still-running tasks come back as a
  streaming pager seeded with everything captured so far; already-
  exited tasks come back as a static pager titled with the final
  exit code and elapsed time.
- **Task viewer (peek mode).** `gB` from the file list opens the
  most-recent task in a peek view. `[t`/`]t` while in any pager
  cycles through bg tasks by id; `:task N` jumps to a specific task.
  While the task is running, the viewer auto-refreshes from the live
  buffer. On close, *exited* tasks are promoted: snapshot pushed to
  buffer history, task dropped from the bg list. Running tasks stay
  put -- you can come back via `gB` / `[t`.
- **`:pause`** / **`:pause N`** sends `SIGSTOP` to the task's
  process group, halting the whole subprocess tree (`make → cc →
  ld` all stop together). **`:resume`** / **`:resume N`** sends
  `SIGCONT`. Useful when switching networks, freeing CPU, or
  pausing an over-eager build to focus on something else. Inside
  the task viewer, **`S`** and **`C`** are the shorthand
  equivalents. Paused tasks render as `[N⏸]` in the divider;
  `:fg` on a paused task auto-resumes before re-attaching.
- A task that completes while in the background fires a flash:
  `task #N: cmd — exit 0 (43s)`.
- **`:task-to-pane`** / **`:task-to-pane N`** promotes a
  backgrounded task to a new pane tab. The process keeps running
  through the move, the tab opens showing what the task viewer
  showed, and a paused task resumes. Useful
  when an `!` task you started turns out to need persistent
  attention (a long-running `npm run dev`, a `cargo watch`, a
  `tail -F`) — promote it next to claude instead of shuttling
  through `:fg` / `^z`. A task that has exited can't be
  promoted; `:fg` shows its output instead. The tab keeps the
  task's `TERM=dumb`, so plain shells and colour output work but a
  full-screen program still won't.
- **`:pane-to-task`** is the symmetric inverse — moves the active
  pane tab into the background-task list without killing the
  child, which carries on as the same process. The task's buffer
  starts empty, as after vim's `^z`: what was on screen isn't
  carried over. Bring it back via `:fg` or `:task-to-pane`.
- The quit confirmation (`Q`/`^D`) counts backgrounded running tasks
  alongside pane-tab processes.
