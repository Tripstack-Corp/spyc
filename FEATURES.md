# cspy — features

cspy is a vi-keyboard-driven terminal file manager written in Rust. It's
built for developers who live in the terminal and want a fast, modal
interface for navigating files, running commands, and — critically —
working alongside AI coding assistants like Claude Code.

Inspired by SideFX's `spy` (a file manager from the Houdini VFX
ecosystem), cspy brings that same "always-open workspace" philosophy to
modern terminal workflows. The split-pane design lets you browse your
project in the top half while Claude runs in the bottom half, so file
context and AI conversation stay in the same window.

## Vi-style navigation

Everything is keyboard-driven with vi motions as the foundation.

- **h/j/k/l** movement across a multi-column file listing (h/l clamp at edges, no wrap)
- **gg / G** to jump to top or bottom
- **^B / ^F** page up and down
- **Count prefix** — `5j`, `10k`, etc.
- **/ search** with incremental filtering (prefix match, or glob with `*`, `?`, `[`)
- **n / N** to repeat search forward / backward

## Directory browsing

- **d / Enter** descend into a directory, or view a text file in the pager
- **e / v** descend into a directory, or open a file in `$EDITOR`
- **u / -** climb to the parent directory (cursor returns to the dir you came from)
- **H / ~** jump to home
- **J** jump to any path (with `~` and `$VAR` expansion)
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

**Inventory** is a global, cross-directory clipboard:
- **y / Y** take picked files (or cursor entry) into inventory
- **p** drop an item from inventory
- **i** toggle the inventory view (replaces the file listing)
- **z** empty the entire inventory

Picks and inventory feed into file operations and shell commands — they
become `%` in shell expansion.

## File operations

- **c** copy selection to a destination
- **M** move selection to a destination
- **R** remove selection (with confirmation)
- **+** create a new directory
- **L** long listing (`ls -lh`) piped through `$PAGER`
- **f** run `file(1)` on the selection
- **^X** chmod +x

## Split pane with multi-tab pty

The bottom half of the terminal hosts a fully independent pty — by
default, it runs `claude` (the Claude Code CLI). This is the core of
cspy's workflow: browse files above, talk to Claude below.

- **^\\ / F10** toggle the pane open/closed
- **F9** open pane with `claude --resume`
- **^W j / ^W k** switch focus between the file list and the pane
- **^W s** send the current selection (file paths) to the pane as stdin
- **^W + / ^W -** grow or shrink the pane
- **^W v** enter scroll mode — browse up to 10K lines of scrollback
  without interrupting the child process; **s** saves to a file

### Multi-tab

Multiple tabs, each running an independent pty:

- **^W n** new tab (prompts for command and working directory)
- **^W x** close the active tab
- **^W 1..9** switch to tab N
- **^W [ / ^W ]** prev / next tab
- **^W r** rename the active tab
- Activity indicator (**+**) on background tabs that have new output
- Set `CSPY_PANE_CMD` to change the default pane command from `claude`

## In-app pager

A built-in pager for viewing files and command output without leaving
cspy.

- ANSI color preservation — captured command output looks exactly right
- **/ search** within pager content, with **n / N** navigation
- **l** toggle line numbers and whitespace markers
- **f** toggle full-width mode vs. centered overlay
- **v** open pager content in `$EDITOR`
- **s** save pager content to a file
- **y** yank pager content to the system clipboard
- **x** toggle hex-dump view for binary files
- Line wrapping for long lines
- Page-up/down, half-page, and vi-style scrolling

## Shell integration

Three modes of running commands, each for a different use case:

- **!** captured — run a command and stream output into the pager in
  real-time with an hourglass timer. Stderr is merged so build
  progress, errors, and output all appear together. `%` expands to
  the current selection. `^C` interrupts.
- **;** foreground — run an interactive command (top, vim, htop) in a
  top-overlay pty that replaces the file listing while the bottom pane
  stays untouched.
- **$** shell — drop into `$SHELL` in the current directory.

The shell prompt uses a vi-mode line editor with persistent history
(shared across sessions), so you get `h/l/w/b/0/$` motion, `x/D/C`
editing, operator+motion (`dw`, `cw`, `db`, `d$`, `dd`, `cc`, etc.),
and `i/a/I/A` mode switching — all within the one-line prompt.
`j`/`k` in normal mode cycle through history without leaving normal
mode. Alt+Enter inserts a newline in the pane (for Claude CLI
multi-line input).

Pane command prompts (`^W n`) have their own dedicated history,
separate from shell commands — so Up/Down shows `claude`, `zsh`,
`bash` instead of mixed shell commands. History is de-duplicated
(most recent use moves to the end).

## Marks

Vi-style named bookmarks for fast navigation:

- **m{a-z}** set a mark at the current directory + cursor position
- **'{a-z}** jump to a mark
- **''** jump back to the previous directory (like `cd -`)
- **\`** jump to the directory where cspy was launched

## Ignore masks

Two toggle-able filter masks to hide clutter:

- **a** toggle mask 1 (dotfiles by default)
- **o** toggle mask 2 (build artifacts by default)

Masks are configurable in `.cspyrc.toml` — you can define custom glob
patterns for each group.

## Powerline status bar

The status bar uses powerline-style segments showing:

- User and hostname
- Current path (intelligently truncated)
- Git branch with dirty flag (`main*`)
- Active state: pick counts, inventory counts, mask status

Falls back to a plain text layout in mono mode.

## Focus indicators

When switching between the file list and the pane, focus is
unambiguous:

- **File list cursor** dims to a muted color when the pane has focus
- **Pane cursor** blinks when focused, shows as a static block when not
- The divider rule brightens when the pane is focused

## Configuration

`.cspyrc.toml` supports per-user (`~/.cspyrc.toml`) and per-project
(`.cspyrc.toml` in the working directory) configuration:

- **Keymap DSL** — `map KEY action [args]` syntax to rebind any key to
  any action. Chord bindings (e.g., `^W n`) are supported.
- **Color overrides** — customize the palette for directories, cursors,
  picks, status bar segments, etc.
- **Ignore mask patterns** — define what each mask group hides.
- **Live reload** — config changes are picked up automatically without
  restarting cspy. Manual reload with **^R**.

## Session management

cspy auto-saves your workspace on quit and can restore it on startup.

- **Auto-save** — on quit, cspy saves the current directory, all pane
  tabs (command, label, cwd), active tab, pane height, and focus state.
- **`cspy --resume`** (or `-r`) — opens a session picker with
  human-readable timestamps ("just now", "2 hours ago", "3 days ago").
- **j/k navigation** — browse sessions with highlighted cursor row.
  Enter to restore, n for a new session, 1-9 for direct selection.
- Sessions are de-duplicated by cwd + tab commands (most recent kept).
- Capped at 20 most recent sessions.

## Info and diagnostics

- **D** show date and time (UTC)
- **V** show cspy version
- **I** session info: PID, RSS memory usage, entry counts
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
