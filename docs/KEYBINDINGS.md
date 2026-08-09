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
| `0`-`9` `<motion>` | Count prefix (e.g. `5j`, `10k`, `5G`) |
| `gg` / `G` | Top / bottom |
| `^B` / `PageUp` | Previous page |
| `^F` / `PageDown` | Next page |
| `Enter` | Descend into dir or view file in pager |
| `e` / `v` | Descend into dir or open file in `$EDITOR` |
| `dd` / `Ndd` | Remove cursor entry (+ N-1 below) to the graveyard (confirm with `y`) |
| `R` | Remove selection (picks, else cursor) to the graveyard |
| `V` | Open `$EDITOR` in top pane (bottom pane stays visible) |
| `D` | Open file in the in-app pager in top pane (bottom pane stays visible) |
| `u` / `-` | Climb to parent |
| `/` | Search current listing (incremental; glob-aware, `^`/`$` anchors) |
| `~` / `Home` | Jump to home (`H` is the harpoon prefix) |
| `J` | Jump to any path |
| `F` | Project-wide fuzzy filename finder (gitignore-aware) |
| `:grep <pat>` | Project-wide content search (embedded ripgrep matcher) |

## File operations

`%` expands to the current selection in any prompt (e.g. `M` then `%.bak`).

| Key | Action |
|-----|--------|
| `c` | Copy selection to a destination (prompt) |
| `M` | Move / rename selection (prompt) |
| `+` | Make a new directory (prompt) |
| `O` | Create a new file in `$EDITOR` (prompt) |
| `L` | Long listing — wide aligned table (name, mode, size, mtime, …) |
| `S` | Cycle sort: name → size → mtime → ext |
| `:chmod <mode>` | Change mode on the selection |
| `:filetype` | Show the cursor file's detected type |

## Picks & inventory

**Picks** are per-directory multi-select. **Inventory** is a persistent
file cache that survives across sessions.

| Key | Action |
|-----|--------|
| `t` | Toggle pick |
| `T` | Pick by glob |
| `^T` | Pick all / clear all |
| `yy` | Yank to inventory (copies file to cache) |
| `yf` | Yank cursor file's absolute path (or picks) to clipboard |
| `yp` | Yank visible pane output to clipboard |
| `yP` | Yank last typed prompt to clipboard |
| `ya` | Yank full pane scrollback to clipboard |
| `Y` | Remove cursor file from inventory |
| `p` | Put inventory files into cwd |
| `i` | Toggle inventory view |
| `z` | Clear inventory (moves entries to the graveyard) |

Inside the inventory view: `t` / `Space` tag-untag for a partial put,
`p` puts tagged (or all) to cwd, `x` / `d` removes an item (to the
graveyard), `Esc` / `i` returns to the directory view.

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
| `gr` | Restore a deleted (struck-through) file from git |
| `]g` / `[g` | Cursor to next / prev git-changed entry (wraps) |
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
| `^a K` / `^a x` | Close tab (confirms first while its child is still running) |
| `^a 1`..`9` | Switch to tab N |
| `^a ^a` | Jump to last-active tab |
| `^a r` | Rename tab |
| `^a R` | Restart tab in place, keeping its number (confirms first while its child is still running) |
| `^z` | Suspend / resume the pane's child (💤); a shell tab's `^z` forwards as usual |
| `^a s` | Send selection paths to pane |
| `^a P` | Pipe file contents to pane |
| `^a i` | Pipe inventory file contents to pane |
| `^a z` | Zoom the active region — list or bottom pane (fullscreen toggle) |
| `^a +` / `^a -` | Grow / shrink the focused split (pane height or vsplit width) |
| `^a \|` | Vertical split — cycle off / top-only / full-height (live-reloading preview of the cursor file) |
| `^a a` / `^a h` | Focus the left file pane (a) |
| `^a b` / `^a l` | Focus the right file pane (b) |
| `^a d` | Toggle dimming of the inactive split column / list |
| `^s n` | Open a second file-commander (column b, at PROJECT_HOME) |
| `^s x` | Close the second file-commander (`^d` quits, keeping `b` open for `-r`) |
| `^a u` | Quick Select — labeled picker for URL/path/SHA/IP |
| `^a v` | Pane scrollback in the in-app pager (search, jump, visual yank) |
| `Ctrl+J` | Newline in pane (multi-line input) |
| `^a ↓` | Send a literal `^a` to the pane (e.g. so Claude receives it) |
| `^a g` | Image-gallery popup: what the agent received + what you've pasted but not sent (`j`/`k` move, `Enter` view, `q` close) |
| `gf` | Jump to file path in pane output |
| `gF` | Jump to file + open at referenced line |

## Pager

Press `Enter` on a file to view it in the built-in pager with
syntax highlighting, search (`/` forward, `?` backward; `n` / `N`
repeat), line numbers, hex dump, markdown rendering, and ANSI color
support. Press `H` (or `F1`) inside the pager for its own help overlay.

`Enter` on a **PNG / JPEG / GIF / WebP** shows the picture full-screen
instead (detected by magic bytes, not the extension). Its verbs:

| Key | Action |
| --- | --- |
| `s` | save a copy (an image file reports where it already is) |
| `y` | copy the image to the clipboard |
| `Y` | copy the file path (a diagram: its mermaid source) |
| `b` | flip to a base64 text buffer |
| `o` | open in the OS image viewer |
| `c` | light/dark toggle (mermaid diagrams only) |
| `q` / `Esc` / `i` | dismiss |

On a terminal with no graphics protocol the picture can't be drawn
inline; `o` still opens it externally.

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

While a `!` capture is running in the pager:

| Key | Action |
|-----|--------|
| `^C` | Interrupt the running capture (`SIGINT` to the child) |
| `^\` | Hard-kill the running capture |
| `^Z` | Send to background; resume later with `:fg` |

## Background tasks & buffer history

Long captured commands shouldn't lock you out of spyc.

| Key | Action |
|-----|--------|
| `^Z` | (in `!` pager) send the running task to the background |
| `:fg` / `:fg N` | resume the most-recent (or specific) backgrounded task |
| `gB` / `:task N` | open the *task viewer* -- a peek view without taking ownership |
| `[t` / `]t` | (in pager, chord) cycle the task viewer prev/next by id |
| `S` / `C` | (in task viewer) pause / continue the underlying task |
| `gp` | reopen the most-recently-closed pager buffer |
| `:bprev` / `:bnext` | walk pager buffer history back/forward |
| `[b` / `]b` | (in pager, chord) walk buffer history back/forward |

Backgrounded tasks render in the pane divider as `[N+]` (running, new
output), `[N●]` (running, quiescent), `[N⏸]` (paused via `S`), `[N✓]`
(exit 0), `[N✗]` (non-zero / killed / crashed), in a distinct color
from pane tabs.
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
| `=` | Temporary glob filter (`=*.rs`, `=^2026`, `=!` picks, `=git` git, `=h` harpoon) |

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
| `Space s` / `I` | Session info (pid, rss, counts) |
| `Space ?` | This help overlay (same as `?`) |
| `gV` / `:version` | Show the spyc version |

The **leader** (`Space`, or `^a Space` from the agent pane) opens a
global/workspace menu: `Space w l\|n\|d` (worktree list/new/delete),
`Space p` (project home), `Space s` (session info), `Space ?` (help),
`Space a` (about). Hold it to see the which-key popup. `W l` / `W n` /
`W d` is a list-focus alias for the same worktree list / new / delete.

`PROJECT_HOME` is auto-set on startup if the launch directory contains
`.git`. New pane tabs default their cwd to the focused column's
worktree/repo root (`gw`'s target); set `[pane] new_tab_cwd =
"project_home"` to pin them to `PROJECT_HOME` instead, or `"browse_dir"`
to open them in the current listing dir.

## Display & config

| Key | Action |
|-----|--------|
| `C` | Toggle colors / mono |
| `^L` | Redraw |
| `^R` | Reload config (also auto-reloads on save) |
| `Esc` (×2) | Cancel a prompt (`Esc`→Normal→`Esc`→cancel) |
| `:activity` | Toggle the activity monitor; `:activity dump` → per-pane why-status report |
| `:archive` | Mounted archives: `info` / `list` / `write` / `discard` / `unmount` / `cancel` (mounting is `Enter` on the archive) |
| `:hooks` | Agent status-hook consent (`on` / `on!` / `off`) |
| `:skill` | Agent skill: `status` / `update` / `remove` / `ask` |
| `:lua` | Lua engine: `status` / `on` / `off` / `reload` |
| `:notify test` | Fire every notification channel to verify setup |
| `:date` | Show date/time (UTC) |
