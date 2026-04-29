# spyc — features

spyc is a vi-keyboard-driven terminal file manager written in Rust. It's
built for developers who live in the terminal and want a fast, modal
interface for navigating files, running commands, and — critically —
working alongside AI coding assistants like Claude Code.

Inspired by SideFX's `spy` (a file manager from the Houdini VFX
ecosystem), spyc brings that same "always-open workspace" philosophy to
modern terminal workflows. The split-pane design lets you browse your
project in the top half while Claude runs in the bottom half, so file
context and AI conversation stay in the same window.

## Vi-style navigation

Everything is keyboard-driven with vi motions as the foundation.

- **h/j/k/l** movement across a multi-column file listing (h/l clamp at edges, no wrap)
- **gg / G** to jump to top or bottom
- **^B / ^F** page up and down
- **Count prefix** — `5j`, `10k`, etc. with visual display in the prompt area
- **/ search** with incremental filtering (prefix match, or glob with `*`, `?`, `[`)
- **n / N** to repeat search forward / backward

## Directory browsing

- **d / Enter** descend into a directory, or view a text file in the pager
- **e / v** descend into a directory, or open a file in `$EDITOR` (suspends TUI)
- **V** open `$EDITOR` in the top pane — Claude pane below stays visible
- **u / -** climb to the parent directory (cursor returns to the dir you came from)
- **H / ~** jump to home
- **J** jump to any path (with `~` and `$VAR` expansion, frecency-ranked
  suggestions from visit history)
- **F** project-wide fuzzy filename finder. Walks `PROJECT_HOME` (or
  the current dir as fallback) honoring `.gitignore`, ranks
  candidates against typed input via nucleo-matcher (basename hits
  outrank parent-dir hits, fzf-style). Up/Down move selection,
  Enter chdirs to the matched file's parent and places the cursor
  on it; Esc cancels. No persistent index — walks lazily on open.
- Multi-column layout that adapts to terminal width
- Color-coded entries: directories, executables, symlinks, files
- **Git status colors** — modified files show amber, untracked/added
  show green, deleted show dim, renamed show lavender, conflicted
  show bold red. Directories containing changes are tinted too.

## Picks and inventory

Two levels of selection for flexible file management.

**Picks** are per-directory multi-select:
- **t** toggle pick on the cursor entry
- **T** pick by glob pattern
- **^T** pick all / clear all

**Inventory** is a file cache — yanked files are copied to a local cache
(`~/.local/state/spyc/inventory/`), persisted across sessions:
- **yy** yank file(s) into inventory cache (regular files only)
- **yp** yank visible pane output to the system clipboard
- **yP** yank the last prompt you typed into the pane to the clipboard
- **Y** remove cursor file from inventory
- **p** put all inventory files to the current directory
- **i** toggle the inventory view (replaces the file listing)
- **z** clear inventory (moves files to graveyard)

Inside the **inventory view** (`i`):
- **t / Space** tag items for partial put
- **p** put tagged items (or all) to cwd — removes from inventory
- **x / d** remove item to graveyard
- **ESC** return to directory view

Picks and inventory feed into file operations and shell commands — they
become `%` in shell expansion.

## File operations

- **c** copy selection to a destination
- **M** move selection to a destination
- **R** remove selection (with confirmation)
- **+** create a new directory
- **L** long listing -- aligned table with inode, mode, octal,
  links, owner, group, size, bytes, blocks, mtime/atime/ctime/birth,
  name (symlinks as `name -> target`). Pager height fits to content.
- **f** run `file(1)` on the selection
- **^X** chmod +x

## Split pane with multi-tab pty

The bottom half of the terminal hosts a fully independent pty — by
default, it runs `claude` (the Claude Code CLI). This is the core of
spyc's workflow: browse files above, talk to Claude below.

- **^\\ / F10** toggle the pane open/closed
- **F9** open pane with `claude --resume`
- **^a j / ^a k** switch focus between the file list and the pane
  (`^w` also works as an alias for `^a`)
- **^a s** send the current selection (file paths) to the pane as stdin
- **^a P** pipe file contents of the selection to the pane
- **^a i** pipe inventory file contents to the pane
- **^a + / ^a -** grow or shrink the pane
- **^a v** enter scroll mode — browse up to 10K lines of scrollback
  without interrupting the child process; **s** saves to a file
- **Ctrl+J** newline in pane (multi-line input for Claude CLI)
- **gf** jump to a file path referenced in pane output; **gF** also
  opens the pager at the referenced line. Scans the last 200 lines of
  output (including scrollback) so paths in large diffs are still found.

### Multi-tab

Multiple tabs, each running an independent pty:

- **^a c** new tab (prompts for command and working directory)
- **^a K / ^a x** close the active tab
- **^a 1..9** switch to tab N
- **^a p / ^a [** prev tab
- **^a n / ^a ]** next tab
- **^a r** rename the active tab
- Activity indicator (**+**) on background tabs that have new output
- Set `SPYC_PANE_CMD` to change the default pane command from `claude`

## In-app pager

A built-in pager for viewing files and command output without leaving
spyc.

- **Syntax highlighting** via syntect — source files are highlighted
  with language-aware coloring (hundreds of languages supported)
- ANSI color preservation — captured command output looks exactly right
- **Streaming output** — `!` commands show output live with an
  hourglass timer, stderr merged so build progress appears in real-time
- **/ search** within pager content, with **n / N** navigation
- **:N** jump to line N
- **l** toggle line numbers (on by default)
- **w** toggle whitespace markers (·, ↲, $)
- **W** toggle line wrap (default on for content; off for picker UIs)
- **m** toggle Markdown rendered ↔ source view (`.md`/`.markdown` only;
  flashes "not a markdown file" otherwise)
- **f** toggle full-width mode vs. centered overlay
- **v** open pager content in `$EDITOR`
- **s** save pager content to a file
- **y** yank pager content to the system clipboard (always operates
  on the *source* — yanking a rendered Markdown view gives you back
  the markdown text, not the styled rendering)
- **x** toggle hex-dump view for binary files
- **Markdown viewer** — `.md`/`.markdown` files open in rendered
  mode by default: headings styled, lists with bullets, fenced
  code blocks syntect-highlighted by language, blockquotes with a
  left rule, links shown with the destination URL, inline
  bold/italic/strikethrough preserved. Press `m` to toggle to the
  raw source view (with full syntect highlighting of the
  markdown source itself); press `m` again to flip back. Yank
  and save always emit the source.
- Page-up/down, half-page, and vi-style scrolling

## Shell integration

Three modes of running commands, each for a different use case:

- **!** captured — run a command and stream output into the pager in
  real-time with an hourglass timer. Stderr is merged so build
  progress, errors, and output all appear together. `%` expands to
  the current selection. `^C` interrupts; `^\` hard-kills; `^Z`
  sends the task to the background (reader thread keeps draining
  output into a buffer; resume with `:fg` -- see Background tasks).
- **!!** — repeat the last captured command.
- **!?** — history editor popup. Opens instantly (no Enter needed).
  Defaults to Normal mode — `j`/`k`/`G`/`gg` navigate, `/` search
  with `n`/`N` to jump between matches, `:N` jumps to entry N.
  Press `i` to vi-edit the highlighted command in-place.
  `Enter` executes the (possibly edited) command, `Ctrl+D` deletes
  an entry, `Esc`/`q` closes.
- **;** foreground — run an interactive command (top, vim, htop) in a
  top-overlay pty that replaces the file listing while the bottom pane
  stays untouched.
- **$** shell — drop into `$SHELL` in the current directory.

The shell prompt uses a vi-mode line editor with persistent history
(shared across sessions), so you get `h/l/w/b/0/$` motion, `x/D/C`
editing, operator+motion (`dw`, `cw`, `db`, `d$`, `dd`, `cc`, etc.),
and `i/a/I/A` mode switching — all within the one-line prompt.
`j`/`k` in normal mode cycle through history without leaving normal
mode. Ctrl+J inserts a newline in the pane (for Claude CLI
multi-line input).

Pane command prompts (`^W n`) have their own dedicated history,
separate from shell commands — so Up/Down shows `claude`, `zsh`,
`bash` instead of mixed shell commands. History is de-duplicated
(most recent use moves to the end).

## Command line

**`:`** opens a vim-style command prompt with vi editing and history:

- **`:cd <path>`** — change directory (`~` and `$VAR` expanded, bare `:cd` goes home)
- **`:sort <mode>`** — sort listing by `name`, `size`, `mtime`, or `ext` (persists across chdir)
- **`:marks`** — show all marks in a pager popup
- **`:set key=value`** — runtime settings (e.g. `:set sort=mtime`)
- **`:bprev`** / **`:bnext`** — navigate pager buffer history (also `[b`/`]b` in pager)
- **`:limit <glob>`** — temporary filter (e.g. `:limit *.rs`)
- **`:limit !`** — show only picked files
- **`:limit`** — clear filter
- **`:!<cmd>`** — captured shell command (same as `!`)
- **`:!!`** — repeat last captured command
- **`:;<cmd>`** — foreground shell command (same as `;`)
- **`:fg`** / **`:fg N`** — resume a backgrounded task (see Background tasks)
- **`:pause`** / **`:pause N`** — pause a backgrounded task (`SIGSTOP`)
- **`:resume`** / **`:resume N`** — resume a paused task (`SIGCONT`)
- **`:grep <pattern>`** — project-wide content search via embedded
  ripgrep matcher (`grep-regex` + `grep-searcher`). Walks
  `PROJECT_HOME` (or current dir) honoring `.gitignore`, smart-case
  by default. Results stream into a pager as `path:line:col: text`,
  so `gf`/`gF` jump from a hit to the file in-place. Capped at 5000
  matches; refine the pattern if you hit it. Power users: `! rg foo`
  still works for ripgrep's full flag surface.
- **`:q`** — quit

The `:` prompt shares history with other shell prompts, so Up/Down
cycles through previous commands.

## Background tasks

Long-running captured commands (`!cargo test`, `!find ...`) don't have
to lock you out of spyc.

- **^Z** while a `!` capture pager is open sends the task to the
  background. The reader thread keeps draining output into a per-task
  buffer (head-truncated at 1 MB). Tasks render in the pane divider,
  right-aligned, in a distinct color family from pane tabs:
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
- The quit confirmation (`Q`/`^D`) counts backgrounded running tasks
  alongside pane-tab processes.

## Pager buffer history

Closed pager views are saved to a history stack (up to 10). Navigate
with `:bprev`/`:bnext` from the main prompt, `[b`/`]b` (chord)
while in the pager, or **`gp`** to reopen the most-recent closed
buffer in one keystroke. The help overlay is excluded from the
stack. Walking off the end keeps the current pager open with a
flash instead of silently closing. Scroll positions are preserved.

## Marks

Vi-style named bookmarks for fast navigation:

- **m{a-z}** set a mark at the current directory + cursor position
- **'{a-z}** jump to a mark
- **''** jump back to the previous directory (like `cd -`)
- **\`** jump to the start directory (editable via `gS` or `:startdir`)

## Project home & session name

Each spyc run has a **`PROJECT_HOME`** (a sticky project root) and a
**session name** (a spice-themed label like `SAFFRON_CUMIN`). Both are
shown on the top bar and persist across `spyc -r`.

- **Auto-detect** — `PROJECT_HOME` is set to the launch directory
  automatically when that directory contains a `.git` entry. No
  upward walk — the concept is explicit and predictable.
- **Keys** — `gh` jumps to `PROJECT_HOME`; `gP` sets it to the current
  directory; `gS` re-points the start directory (target of `` ` ``).
- **Commands** — `:project` prints; `:project .`, `:project <path>`,
  `:project clear` manage the value. `:startdir` manages start dir.
  `:name <NEW>` renames the session (normalized to
  `[A-Z0-9_]`). `:whoami` / `gU` flashes `user@host`.
- **New pane tabs** default their cwd to `PROJECT_HOME` when set,
  otherwise to the current listing dir.
- **Session names** are generated at session creation from ~30
  spices, joined pairwise: `CUMIN_SAFFRON`, `HARISSA_SUMAC`, etc.
  Shown as the primary column in the `-r` session picker so you can
  pick by memory instead of by timestamp.

## Ignore masks & filtering

Two toggle-able filter masks to hide clutter:

- **a** toggle mask 1 (dotfiles by default)
- **o** toggle mask 2 (build artifacts by default)

Masks are configurable in `.spycrc.toml` — you can define custom glob
patterns for each group.

**Temporary filter** (`=`): type a glob pattern to temporarily hide
non-matching files. `=!` shows only picked files. `=` with an empty
pattern clears the filter. The active filter is shown in the status bar.
Cleared automatically when changing directories.

## Powerline status bar

The status bar uses powerline-style segments in this order:

- Pepper emoji (logo)
- `PROJECT_HOME` basename (hidden when unset)
- Session name in all caps (hidden when empty)
- Current path (intelligently truncated)
- Git branch with dirty flag (`main*`)
- Active state: pick counts, inventory counts, mask status, hidden
  file count, active filter

Under width pressure, segments are dropped in reverse priority:
suffix → path becomes basename → git branch. `PROJECT_HOME` and
session name are retained as the primary workspace identifiers.

`user@host` is no longer in the top bar — press `gU` (or run
`:whoami`) to flash it in the status line, or open the `I` info
overlay where it appears alongside the session name, project home,
and start directory.

Falls back to a plain text layout in mono mode.

## Focus indicators

When switching between the file list and the pane, focus is
unambiguous:

- **File list cursor** dims to a muted color when the pane has focus
- **Pane cursor** shows as a bright reverse-video block when focused,
  dim block when unfocused
- The divider rule brightens when the pane is focused
- The divider also shows the active tab's *live* cwd (polled from
  `/proc/<pid>/cwd` on Linux, `lsof` on macOS). If the subprocess
  has `cd`'d away from where spyc launched it, the cwd is prefixed
  with `↪` and rendered in the active-tab color so it's easy to
  spot — useful when a `bash` tab has wandered.

## Configuration

`.spycrc.toml` supports per-user (`~/.spycrc.toml`) and per-project
(`.spycrc.toml` in the working directory) configuration:

- **Keymap DSL** — `map KEY action [args]` syntax to rebind any key to
  any action. Chord bindings (e.g., `^W n`) are supported.
- **Color overrides** — customize the palette for directories, cursors,
  picks, status bar segments, etc.
- **Ignore mask patterns** — define what each mask group hides.
- **Layout** — `[layout] status_position = "top" | "bottom"`. Default
  is `"top"`. `"bottom"` matches vim/tmux convention and avoids a
  double status bar when running spyc inside tmux. With `"bottom"` the
  prompt sits one row above the status bar (vim cmdline ordering).
- **Live reload** — config changes are picked up automatically without
  restarting spyc. Manual reload with **^R**.

### Bootstrapping

```sh
spyc --print-config > ~/.spycrc.toml
```

emits a fully-commented template with every option at its default —
self-documenting starting point.

## Session management

spyc auto-saves your workspace on quit and can restore it on startup.

- **Auto-save** — on quit, spyc saves the current directory, all pane
  tabs (command, label, cwd), active tab, pane height, focus state,
  the spice-themed session name, and `PROJECT_HOME`.
- **`spyc --resume`** (or `-r`) — opens a session picker showing
  the session name (primary column), a human-readable timestamp
  ("just now", "2 hours ago", "3 days ago"), and the cwd.
- **j/k navigation** — browse sessions with highlighted cursor row.
  Enter to restore, n for a new session, 1-9 for direct selection.
- Sessions are de-duplicated by cwd + tab commands (most recent kept).
- Capped at 20 most recent sessions.

## MCP server (Claude integration)

spyc runs a background MCP server on a PID-scoped Unix domain socket
(`~/.local/state/spyc/mcp-<PID>.sock`). On startup it writes
`.mcp.json` with a stdio transport entry so Claude Code discovers
spyc automatically — no `--mcp-config` flag needed. Multiple spyc
instances coexist safely; when a new instance opens in a directory
already owned by a live spyc, it prompts on stderr before taking over
(`PID N already owns MCP here. Take over? [Y/n]`, default Y). On
takeover it sends a `spyc/disconnected` notification to the old
instance and rewrites `.mcp.json`; on decline (`n`), the old instance
keeps ownership and the new spyc starts without MCP. Non-tty stdin
(scripts/CI) auto-takes-over. Enterprise managed-settings.json
policies (`deniedMcpServers`/`allowedMcpServers`) are respected.

Claude can query and control the workspace through these tools:

**Read tools:**
- **`get_spyc_context`** -- returns cwd, cursor file, picks, inventory,
  active filter, git branch, `project_home`, and `session_name`
- **`get_file_content`** -- reads a file's text content (up to 100KB)

**Write tools (Claude can mutate the TUI):**
- **`navigate_to`** -- change directory or focus cursor on a file
- **`set_filter`** -- set or clear the file listing filter (glob)
- **`pick_files`** -- pick files matching glob patterns (additive)
- **`clear_picks`** -- clear all picks

**Search tools (gitignore-aware, PROJECT_HOME-scoped):**
- **`search_paths(query, [limit])`** -- fuzzy filename search
  (same `ignore` walker + nucleo ranking as the `F` picker).
  Returns repo-relative paths, fzf-style ranked.
- **`search_content(pattern, [limit])`** -- regex content search
  via the embedded ripgrep matcher (same as `:grep`). Returns
  `{path, line, col, text}` objects.
- **`search_picks(pattern, [limit])`** -- *spyc-shaped*: content
  search restricted to the user's currently-picked files. Picks
  are TUI multi-select state Claude can't see otherwise, so this
  is the only way to grep the user's intended subset.
- **`search_inventory(pattern, [limit])`** -- *spyc-shaped*:
  content search over the persistent inventory cache (yanked
  files surviving across sessions). Lets Claude grep accumulated
  "interesting files" without re-establishing context.

Write actions execute on the main thread via a command channel.
Flash messages (`[mcp] navigated to src/`) inform the user when
Claude changes the workspace. The `gf`/`gF` keys complete the loop:
jump from Claude's output back to the file list.

## Info and diagnostics

- **D** show date and time (UTC)
- **gV** show spyc version (also `:version`)
- **I** session info: PID, RSS memory usage, entry counts
- **A** activity monitor: live draws/sec, cells/sec, draw reason
  breakdown (pane/event/other), and poll interval
- **C** toggle between color and mono themes
- **s** set an environment variable (`NAME=VALUE`)

## Building

```sh
cargo build            # dev build
cargo build --release  # release build
make                   # see Makefile for build, release, cross-compile, install, deploy
```

Cross-compilation targets are available via the Makefile for deployment
to remote hosts.
