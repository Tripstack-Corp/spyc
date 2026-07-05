# spyc keybindings

The complete keymap. Press `?` inside spyc for the same reference as an
overlay; this file is the browsable version. The survival subset lives in the
[README](../README.md#keybindings).

Binding tiers (see [DESIGN.md](../DESIGN.md) → "Binding taxonomy"): **frame**
keys drive the file view (letters / `g` / `H` / `[`/`]`); **pane** keys use the
`^a` prefix; **global** workspace ops are on the `Space` leader.

## Navigation

| Key | Action |
|-----|--------|
| `h` `j` `k` `l` | Move (counts work: `5j`, `10k`) |
| `gg` / `G` | Top / bottom |
| `Enter` | Descend into dir or view file in pager |
| `e` / `v` | Descend into dir or open file in `$EDITOR` |
| `dd` / `Ndd` | Remove cursor entry (+ N-1 below) to the graveyard (confirm with `y`) |
| `V` | Open `$EDITOR` in top pane (bottom pane stays visible) |
| `D` | Open file in the in-app pager in top pane (bottom pane stays visible) |
| `u` / `-` | Climb to parent |
| `/` | Search current listing (incremental, glob-aware) |
| `~` / `Home` | Jump to home (`H` is the harpoon prefix) |
| `J` | Jump to any path |
| `F` | Project-wide fuzzy filename finder (gitignore-aware) |
| `:grep <pat>` | Project-wide content search (embedded ripgrep matcher) |

## Picks & inventory

**Picks** are per-directory multi-select. **Inventory** is a persistent
file cache that survives across sessions.

| Key | Action |
|-----|--------|
| `t` | Toggle pick |
| `T` | Pick by glob |
| `yy` | Yank to inventory (copies file to cache) |
| `yp` | Yank visible pane output to clipboard |
| `yP` | Yank last typed prompt to clipboard |
| `Y` | Remove cursor file from inventory |
| `p` | Put inventory files into cwd |
| `i` | Toggle inventory view |

> Yank-to-clipboard uses `pbcopy` on macOS and `wl-copy` / `xclip` /
> `xsel` on Linux (auto-detected). Install one of those on Linux —
> see [INSTALL.md](../INSTALL.md#clipboard-helper-linux-only).

## Graveyard (R-undo + soft-delete recovery)

Files removed with **`R`** (and items expelled from inventory) go
to a per-user **graveyard** as compressed `tar.zst` blobs (mode
bits + mtime preserved). Recover with `:graveyard` or `:undo`. When the
graveyard exceeds 500 MB the oldest entries cascade to the system
trash so OS-native recovery still works.

| Key | Action |
|-----|--------|
| `:graveyard` | Open graveyard view (newest first; bind a key with `map KEY command graveyard`) |
| `:undo` | Restore most-recent removal to its original path |
| `p` (in view) | Restore cursor entry to cwd |
| `P` (in view) | Restore cursor entry to original path |
| `dd` / `x` (in view) | Purge cursor entry to system trash |
| `Z` (in view) | Purge ALL entries to system trash (confirm) |

## Git views

In-house gix-backed diff / show / blame pager views (in-process, no
`git` subprocess) — syntax-highlighted, side-by-side or unified (`|`
toggles), word-level intra-line change highlighting.

| Key | Action |
|-----|--------|
| `gd` | Diff vs HEAD for the selection (staged + unstaged + new) |
| `gD` | Staged-only diff (`git diff --cached`) |
| `gu` | Unstaged diff (`git diff`) — what changed since you staged |
| `gb` | Blame the cursor file |
| `\|` (in view) | Toggle side-by-side ⇄ unified layout |

## Split pane

The pane is a real pty -- it runs `claude` by default, but any command
works. Prefix is `^a` (screen-style); `^w` also works.

| Key | Action |
|-----|--------|
| `^\` / `F10` | Toggle pane |
| `F9` | Open pane with `claude --resume` |
| `^a j` / `^a k` | Switch focus |
| `^a c` | New tab |
| `^a n` / `^a ]` | Next tab |
| `^a p` / `^a [` | Prev tab |
| `^a K` / `^a x` | Close tab |
| `^a 1`..`9` | Switch to tab N |
| `^a s` | Send selection paths to pane |
| `^a P` | Pipe file contents to pane |
| `^a z` | Zoom the active region — list or bottom pane (fullscreen toggle) |
| `^a \|` | Vertical split — cycle off / top-only / full-height (live-reloading preview of the cursor file) |
| `^a a` / `^a h` | Focus the left file pane (a) |
| `^a b` / `^a l` | Focus the right file pane (b) |
| `^a d` | Toggle dimming of the inactive split column / list |
| `^s n` | Open a second file-commander (column b, at PROJECT_HOME) |
| `^s x` | Close the second file-commander (`^d` quits, keeping `b` open for `-r`) |
| `^a u` | Quick Select — labeled picker for URL/path/SHA/IP |
| `^a v` | Pane scrollback in the in-app pager (search, jump, visual yank) |
| `Ctrl+J` | Newline in pane (multi-line input) |
| `gf` | Jump to file path in pane output |
| `gF` | Jump to file + open at referenced line |

## Pager

Press `Enter` on a file to view it in the built-in pager with
syntax highlighting, search (`/` forward, `?` backward; `n` / `N`
repeat), line numbers, hex dump, markdown rendering, and ANSI color
support. Press `H` (or `F1`) inside the pager for its own help overlay.

The pager isn't limited to a centered overlay. It can also mount
in place:

- **`D`** opens the cursor file in the **top pane** (bottom pane
  stays visible alongside).
- **`^a v`** mounts a frozen snapshot of pane scrollback in the
  **bottom pane** (line numbers on by default, so it reads as
  scrolled-back rather than live).

Inside the pager: `/` search with `n`/`N`, `:N` jump-to-line,
`V` arms visual line mode — first `V` places a line cursor you
move to the exact start line, a second `V` anchors the selection
(`y` yanks the line range); `^v` enters visual block mode for
rectangular selection.

## Shell

| Key | Action |
|-----|--------|
| `!` | Captured command -- streams into pager |
| `!!` | Repeat last command |
| `!?` | History editor (vi-editable, searchable) |
| `;` | Foreground command (top, vim, etc.) |
| `$` | Drop into `$SHELL` |
| `:` | Command line (`:cd`, `:sort`, `:limit`, `:grep`, `:fg`, `:task`, `:q`) |

`%` in any command expands to the current selection.

## Background tasks & buffer history

Long captured commands shouldn't lock you out of spyc.

| Key | Action |
|-----|--------|
| `^Z` | (in `!` pager) send the running task to the background |
| `:fg` / `:fg N` | resume the most-recent (or specific) backgrounded task |
| `gB` / `:task N` | open the *task viewer* -- a peek view without taking ownership |
| `[t` / `]t` | (in pager, chord) cycle the task viewer prev/next by id |
| `gp` | reopen the most-recently-closed pager buffer |
| `:bprev` / `:bnext` | walk pager buffer history back/forward |
| `[b` / `]b` | (in pager, chord) walk buffer history back/forward |

Backgrounded tasks render in the pane divider as `[N+]` (running, new
output), `[N●]` (running, quiescent), `[N✓]` (exit 0), `[N✗]`
(non-zero / killed / crashed), in a distinct color from pane tabs.
When a viewed task exits, closing the task viewer pushes its
final rendered view into the buffer-history stack so `[b` walks
back to it later.

## Marks & filters

| Key | Action |
|-----|--------|
| `m{a-z}` | Set a bookmark |
| `'{a-z}` | Jump to bookmark |
| `''` | Jump back (like `cd -`) |
| `` ` `` | Jump to start dir (set with `gS` or `:startdir`) |
| `a` | Toggle dotfile filter |
| `o` | Toggle build artifact filter |
| `=` | Temporary glob filter (`=*.rs`, `=!` picks, `=git` git, `=h` harpoon) |

## Harpoon (per-worktree pinned files)

A small ordered list (max 9 slots) of files / dirs you're cycling
between. Persists per worktree (the focused column's repo root,
else `PROJECT_HOME`), so a second column in another worktree keeps
its own list.

| Key | Action |
|-----|--------|
| `Ha` | Append cursor file/dir to harpoon |
| `Hx` | Remove cursor file/dir from harpoon |
| `H1`..`H9` | Jump to slot N (chdir + place cursor) |
| `Hh` | Open harpoon menu (j/k, K/J reorder, dd delete) |
| `=h` | Limit listing to harpoon entries (incl. ancestor dirs) |

## Project home & session

Each spyc run has a `PROJECT_HOME` (a sticky project root) and a
session name (a spice-themed label like `SAFFRON_CUMIN`). Both appear
on the top bar and persist across `spyc -r`.

| Key | Action |
|-----|--------|
| `Space p` | Jump to `PROJECT_HOME` (leader; `^a Space p` from the pane) |
| `gP` / `Space P` | Set `PROJECT_HOME` to current directory |
| `gS` | Set start dir (target of `` ` ``) to current directory |
| `:project [.\|<path>\|clear]` | Manage `PROJECT_HOME` |
| `:startdir [.\|<path>]` | Manage start dir |
| `:name <NEW>` | Rename the active session |
| `:whoami` | Show `user@host` in the status line |

The **leader** (`Space`, or `^a Space` from the agent pane) opens a
global/workspace menu: `Space w l\|n\|d` (worktree list/new/delete),
`Space p` (project home), `Space s` (session info). Hold it to see the
which-key popup.

`PROJECT_HOME` is auto-set on startup if the launch directory contains
`.git`. New pane tabs default their cwd to `PROJECT_HOME` when set
(set `[pane] new_tab_cwd = "browse_dir"` to open them in the current
listing dir instead).
